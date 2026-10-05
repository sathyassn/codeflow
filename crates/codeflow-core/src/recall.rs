//! `codeflow recall` — FTS5 search over the durable memory corpus
//! (charter §8, D17).
//!
//! The index is a rebuildable cache in `~/.codeflow/recall.db` (`SQLite`,
//! bundled — no system dependency). Source of truth stays markdown + JSONL;
//! sync is lazy and incremental at query time via per-file cursors
//! (mtime + size). No daemon.
//!
//! Indexed sources per repo:
//!
//! | kind         | files |
//! |--------------|-------|
//! | `ledger`     | `<state>/ledger/{work-graph,config}/*.jsonl` (per line) |
//! | `session`    | `<state>/ledger/{sessions,memory-events}/*.jsonl` (per line) |
//! | `adr`        | `docs/decisions/*.md` |
//! | `pm`         | `project-management/{epics,tasks,specs,feedback}/*.md` |
//! | `capability` | `docs/capabilities.md` |
//! | `product`    | `docs/product.md` |
//! | `plan`       | `docs/plan/**/*.md` (recursive) |
//!
//! where `<state>` is the repo's runtime state dir (`.git/codeflow`).
//! Results disclose coverage gaps — repos or source kinds that were never
//! indexed are reported, never silently empty (charter principle 8).

use std::collections::HashSet;
#[cfg(unix)]
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use thiserror::Error;

/// Recall errors.
#[derive(Debug, Error)]
pub enum RecallError {
    #[error("empty query: recall needs at least one searchable term")]
    EmptyQuery,

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// A repo to index and search.
#[derive(Debug, Clone)]
pub struct RepoTarget {
    /// Display name (registry name or directory name).
    pub name: String,
    /// Repo root path.
    pub root: PathBuf,
}

/// Options for a recall run.
#[derive(Debug, Clone)]
pub struct RecallOptions {
    /// Drop indexed rows for the target repos and re-sync from scratch.
    pub rebuild: bool,
    /// Maximum number of results.
    pub limit: usize,
}

impl Default for RecallOptions {
    fn default() -> Self {
        Self {
            rebuild: false,
            limit: 20,
        }
    }
}

/// One ranked search hit.
#[derive(Debug, Clone)]
pub struct RecallResult {
    /// Repo display name.
    pub repo: String,
    /// Source kind (`ledger`, `session`, `adr`, `pm`, `capability`,
    /// `product`, `plan`).
    pub kind: String,
    /// Source path relative to the repo root.
    pub path: String,
    /// Document title (frontmatter title, heading, or event type).
    pub title: String,
    /// Match snippet with `[` `]` highlight markers.
    pub snippet: String,
}

/// Aggregate sync statistics across the targeted repos.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncStats {
    /// Files (re-)indexed because their cursor changed or was absent.
    pub files_indexed: usize,
    /// Files skipped because mtime + size matched the stored cursor.
    pub files_skipped: usize,
    /// Vanished files whose rows were removed from the index.
    pub files_removed: usize,
}

/// The outcome of a recall run: ranked results plus coverage disclosure.
#[derive(Debug, Clone)]
pub struct RecallReport {
    pub results: Vec<RecallResult>,
    /// Coverage notes: skipped repos, source kinds never indexed, etc.
    pub notes: Vec<String>,
    pub stats: SyncStats,
}

/// Source kinds in the recall corpus.
const KINDS: &[&str] = &[
    "ledger",
    "session",
    "adr",
    "pm",
    "capability",
    "product",
    "plan",
];

// ---------------------------------------------------------------------------
// Repo runtime state location
// ---------------------------------------------------------------------------

/// Resolve a repo's runtime state directory (`.git/codeflow`, shared across
/// worktrees — charter §4.1).
///
/// Handles both a `.git` directory and a `.git` file (worktree/submodule
/// gitfile indirection): for worktree gitdirs the common git directory is
/// used so all worktrees share one state dir. Returns `None` when `.git`
/// is absent entirely.
#[must_use]
pub fn runtime_state_dir(repo_root: &Path) -> Option<PathBuf> {
    let dot_git = repo_root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git.join("codeflow"));
    }
    if dot_git.is_file() {
        let content = fs::read_to_string(&dot_git).ok()?;
        let gitdir = content.strip_prefix("gitdir:")?.trim();
        let mut gitdir = PathBuf::from(gitdir);
        if !gitdir.is_absolute() {
            gitdir = repo_root.join(gitdir);
        }
        // Worktree gitdir: <common>/.git/worktrees/<name> → use <common>/.git
        let components: Vec<String> = gitdir
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if let Some(pos) = components.iter().rposition(|c| c == "worktrees") {
            let common: PathBuf = components[..pos].iter().collect();
            return Some(common.join("codeflow"));
        }
        return Some(gitdir.join("codeflow"));
    }
    None
}

// ---------------------------------------------------------------------------
// Source enumeration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct SourceFile {
    /// Reversibly encoded path relative to the repo root (cursor identity).
    rel: String,
    /// Lossy human-readable path used only for titles and returned results.
    display: String,
    abs: PathBuf,
    kind: &'static str,
}

fn jsonl_files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .is_some_and(|n| crate::ledger::is_jsonl_file(&n.to_string_lossy()))
        })
        .collect();
    files.sort();
    files
}

fn md_files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")))
        .collect();
    files.sort();
    files
}

/// Recursively collect markdown files under `dir` (sorted). Plan docs nest
/// (`docs/plan/v2/…`), so a flat read would miss the charter and its siblings.
fn md_files_under(dir: &Path) -> Vec<PathBuf> {
    const MAX_DEPTH: usize = 64;
    const MAX_ENTRIES: usize = 10_000;
    let mut files = Vec::new();
    let mut pending = vec![(dir.to_path_buf(), 0_usize)];
    let mut visited = 0_usize;
    while let Some((current, depth)) = pending.pop() {
        if depth > MAX_DEPTH || visited >= MAX_ENTRIES {
            continue;
        }
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            if visited >= MAX_ENTRIES {
                break;
            }
            visited += 1;
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                if depth < MAX_DEPTH {
                    pending.push((path, depth + 1));
                }
            } else if metadata.is_file()
                && path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn rel_to(root: &Path, path: &Path) -> String {
    let encoded = encode_path(path.strip_prefix(root).unwrap_or(path));
    #[cfg(windows)]
    {
        encoded.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        encoded
    }
}

fn display_rel_to(root: &Path, path: &Path) -> String {
    let display = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    #[cfg(windows)]
    {
        display.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        display
    }
}

#[cfg(unix)]
fn encode_path(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut encoded = String::new();
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(unix)]
fn display_encoded_path(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &encoded[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(not(unix))]
fn display_encoded_path(encoded: &str) -> String {
    encoded.replace("%25", "%")
}

#[cfg(not(unix))]
fn encode_path(path: &Path) -> String {
    path.to_string_lossy().replace('%', "%25")
}

fn source_file(root: &Path, abs: PathBuf, kind: &'static str) -> SourceFile {
    SourceFile {
        rel: rel_to(root, &abs),
        display: display_rel_to(root, &abs),
        abs,
        kind,
    }
}

/// Enumerate all indexable source files for a repo.
fn collect_sources(root: &Path) -> Vec<SourceFile> {
    let mut sources = Vec::new();

    if let Some(state) = runtime_state_dir(root) {
        let ledger_dir = state.join("ledger");
        for (types, kind) in [
            (
                &[
                    crate::ledger::files::WORK_GRAPH,
                    crate::ledger::files::CONFIG,
                ][..],
                "ledger",
            ),
            (
                &[
                    crate::ledger::files::SESSIONS,
                    crate::ledger::files::MEMORY_EVENTS,
                ][..],
                "session",
            ),
        ] {
            for ty in types {
                for abs in jsonl_files_in(&ledger_dir.join(ty)) {
                    sources.push(source_file(root, abs, kind));
                }
            }
        }
    }

    for abs in md_files_in(&root.join("docs/decisions")) {
        sources.push(source_file(root, abs, "adr"));
    }

    let pm_root = root.join("project-management");
    for abs in crate::workgraph::layout::epic_record_files(&pm_root)
        .into_iter()
        .chain(crate::workgraph::layout::task_record_files(&pm_root))
        .chain(crate::workgraph::layout::spec_record_files(&pm_root))
        .chain(crate::feedback::item_files(root))
    {
        sources.push(source_file(root, abs, "pm"));
    }

    let caps = root.join("docs/capabilities.md");
    if caps.is_file() {
        sources.push(source_file(root, caps, "capability"));
    }

    // Product WHYs: the human-owned charter of purpose/scope/non-goals plus the
    // plan of record. Without these, product decisions (e.g. D17/D18/D22) are
    // un-findable via recall while technical whys (ADRs, capabilities) are.
    let product = root.join("docs/product.md");
    if product.is_file() {
        sources.push(source_file(root, product, "product"));
    }

    for abs in md_files_under(&root.join("docs/plan")) {
        sources.push(source_file(root, abs, "plan"));
    }

    sources
}

// ---------------------------------------------------------------------------
// Index schema and sync
// ---------------------------------------------------------------------------

fn open_db(db_path: &Path) -> Result<Connection, RecallError> {
    if let Some(dir) = db_path.parent() {
        fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(db_path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS cursors (
            repo   TEXT NOT NULL,
            source TEXT NOT NULL,
            kind   TEXT NOT NULL,
            mtime  INTEGER NOT NULL,
            size   INTEGER NOT NULL,
            PRIMARY KEY (repo, source)
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS docs USING fts5(
            repo, kind, path, title, content
        );",
    )?;
    Ok(conn)
}

fn file_cursor(path: &Path) -> Option<(i64, i64)> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((
        i64::try_from(mtime.as_secs()).unwrap_or(i64::MAX),
        i64::try_from(meta.len()).unwrap_or(i64::MAX),
    ))
}

/// Extract a display title from markdown content: frontmatter `title`,
/// else the first `#` heading, else the file stem.
fn markdown_title(content: &str, fallback: &str) -> String {
    if let Ok((fm, _)) = crate::validate::parse_frontmatter(content.as_bytes()) {
        if let Some(serde_yaml::Value::String(t)) = fm.get("title") {
            return t.clone();
        }
    }
    content
        .lines()
        .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
        .unwrap_or_else(|| fallback.to_string())
}

/// Index one source file into `docs`, replacing any prior rows for it.
fn index_file(conn: &Connection, repo_key: &str, src: &SourceFile) -> Result<(), RecallError> {
    conn.execute(
        "DELETE FROM docs WHERE repo = ?1 AND path = ?2",
        (repo_key, &src.rel),
    )?;

    let content = fs::read_to_string(&src.abs).unwrap_or_default();
    let mut insert = conn.prepare_cached(
        "INSERT INTO docs (repo, kind, path, title, content) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    if src.kind == "ledger" || src.kind == "session" {
        // JSONL: one document per event line so snippets point at events.
        for line in content.lines().filter(|l| !l.trim().is_empty()) {
            let title = serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|v| {
                    v.get("event")
                        .and_then(serde_json::Value::as_str)
                        .map(ToString::to_string)
                })
                .unwrap_or_else(|| "event".to_string());
            insert.execute((repo_key, src.kind, &src.rel, &title, line))?;
        }
    } else {
        let stem = Path::new(&src.display)
            .file_stem()
            .map_or_else(|| src.display.clone(), |s| s.to_string_lossy().into_owned());
        let title = markdown_title(&content, &stem);
        insert.execute((repo_key, src.kind, &src.rel, &title, &content))?;
    }

    Ok(())
}

/// Lazily sync one repo into the index. Returns per-repo coverage notes.
fn sync_repo(
    conn: &Connection,
    target: &RepoTarget,
    stats: &mut SyncStats,
    notes: &mut Vec<String>,
) -> Result<(), RecallError> {
    let repo_key = encode_path(&target.root);
    let sources = collect_sources(&target.root);

    // Stale cursor cleanup: drop rows for files that no longer exist.
    let current: HashSet<&str> = sources.iter().map(|s| s.rel.as_str()).collect();
    let stored: Vec<String> = {
        let mut stmt = conn.prepare_cached("SELECT source FROM cursors WHERE repo = ?1")?;
        let rows = stmt.query_map([&repo_key], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for gone in stored.iter().filter(|s| !current.contains(s.as_str())) {
        conn.execute(
            "DELETE FROM docs WHERE repo = ?1 AND path = ?2",
            (&repo_key, gone),
        )?;
        conn.execute(
            "DELETE FROM cursors WHERE repo = ?1 AND source = ?2",
            (&repo_key, gone),
        )?;
        stats.files_removed += 1;
    }

    // Incremental per-file sync keyed on (mtime, size).
    for src in &sources {
        let Some((mtime, size)) = file_cursor(&src.abs) else {
            continue;
        };
        let unchanged = match conn.query_row(
            "SELECT 1 FROM cursors WHERE repo = ?1 AND source = ?2 AND mtime = ?3 AND size = ?4",
            (&repo_key, &src.rel, mtime, size),
            |_row| Ok(()),
        ) {
            Ok(()) => true,
            Err(rusqlite::Error::QueryReturnedNoRows) => false,
            Err(e) => return Err(e.into()),
        };
        if unchanged {
            stats.files_skipped += 1;
            continue;
        }
        index_file(conn, &repo_key, src)?;
        conn.execute(
            "INSERT INTO cursors (repo, source, kind, mtime, size) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (repo, source) DO UPDATE SET kind = ?3, mtime = ?4, size = ?5",
            (&repo_key, &src.rel, src.kind, mtime, size),
        )?;
        stats.files_indexed += 1;
    }

    // Coverage disclosure: kinds with nothing to index in this repo.
    let present: HashSet<&str> = sources.iter().map(|s| s.kind).collect();
    let missing: Vec<&str> = KINDS
        .iter()
        .filter(|k| !present.contains(*k))
        .copied()
        .collect();
    if !missing.is_empty() {
        notes.push(format!(
            "{}: no {} sources indexed",
            target.name,
            missing.join("/")
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Build a defensive FTS5 MATCH expression from free text: alphanumeric
/// terms, each quoted, OR-combined so question-style queries rank by
/// overlap (bm25) instead of demanding every word.
fn build_match_expr(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.to_lowercase()))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

/// Run recall: lazily sync the target repos, then query the FTS5 index.
///
/// Targets whose root path is missing are skipped with a coverage note
/// (the registry may reference moved or deleted repos). With
/// `options.rebuild`, indexed rows for the targets are dropped first and
/// re-synced — the index is a cache, rebuildable at will.
///
/// # Errors
///
/// Returns [`RecallError::EmptyQuery`] when the query has no searchable
/// terms, or [`RecallError::Sqlite`] / [`RecallError::Io`] on index failure.
pub fn recall(
    db_path: &Path,
    targets: &[RepoTarget],
    query: &str,
    options: &RecallOptions,
) -> Result<RecallReport, RecallError> {
    let match_expr = build_match_expr(query).ok_or(RecallError::EmptyQuery)?;

    let conn = open_db(db_path)?;
    let mut notes = Vec::new();
    let mut stats = SyncStats::default();

    let mut searched_keys: Vec<String> = Vec::new();
    for target in targets {
        if !target.root.is_dir() {
            notes.push(format!(
                "{}: path missing ({}) — skipped",
                target.name,
                target.root.display()
            ));
            continue;
        }
        let repo_key = encode_path(&target.root);
        if options.rebuild {
            conn.execute("DELETE FROM docs WHERE repo = ?1", [&repo_key])?;
            conn.execute("DELETE FROM cursors WHERE repo = ?1", [&repo_key])?;
        }
        sync_repo(&conn, target, &mut stats, &mut notes)?;
        searched_keys.push(repo_key);
    }

    let mut results = Vec::new();
    if !searched_keys.is_empty() {
        let placeholders = searched_keys
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT repo, kind, path, title,
                    snippet(docs, 4, '[', ']', ' … ', 12)
             FROM docs
             WHERE docs MATCH ?1 AND repo IN ({placeholders})
             ORDER BY bm25(docs)
             LIMIT {}",
            options.limit
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&match_expr];
        for key in &searched_keys {
            params.push(key);
        }
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (repo_key, kind, path, title, snippet) = row?;
            let repo = targets
                .iter()
                .find(|t| encode_path(&t.root) == repo_key)
                .map_or(repo_key, |t| t.name.clone());
            results.push(RecallResult {
                repo,
                kind,
                path: display_encoded_path(&path),
                title,
                snippet,
            });
        }
    }

    Ok(RecallReport {
        results,
        notes,
        stats,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a fake initialized repo with a runtime state dir.
    fn make_repo(root: &Path) {
        fs::create_dir_all(root.join(".codeflow")).unwrap();
        fs::write(root.join(".codeflow/policy.json"), "{}").unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
    }

    fn write_memory_event(root: &Path, line: &str) {
        let dir = root.join(".git/codeflow/ledger/memory-events");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory-events.jsonl");
        let mut existing = fs::read_to_string(&path).unwrap_or_default();
        existing.push_str(line);
        existing.push('\n');
        fs::write(path, existing).unwrap();
    }

    fn write_adr(root: &Path, name: &str, title: &str, body: &str) {
        let dir = root.join("docs/decisions");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(name),
            format!("---\ntitle: {title}\nstatus: accepted\n---\n\n# {title}\n\n{body}\n"),
        )
        .unwrap();
    }

    fn write_product(root: &Path, body: &str) {
        let dir = root.join("docs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("product.md"), body).unwrap();
    }

    fn write_plan(root: &Path, rel: &str, body: &str) {
        let path = root.join("docs/plan").join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn target(name: &str, root: &Path) -> RepoTarget {
        RepoTarget {
            name: name.to_string(),
            root: root.to_path_buf(),
        }
    }

    #[test]
    fn test_runtime_state_dir_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        assert_eq!(
            runtime_state_dir(dir.path()),
            Some(dir.path().join(".git/codeflow"))
        );
    }

    #[test]
    fn test_runtime_state_dir_worktree_gitfile() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        let wt = dir.path().join("wt");
        fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(
            wt.join(".git"),
            format!("gitdir: {}\n", main.join(".git/worktrees/wt").display()),
        )
        .unwrap();
        assert_eq!(
            runtime_state_dir(&wt),
            Some(main.join(".git/codeflow")),
            "worktrees must share the common runtime state dir"
        );
    }

    #[test]
    fn test_runtime_state_dir_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(runtime_state_dir(dir.path()), None);
    }

    #[cfg(unix)]
    #[test]
    fn markdown_walk_skips_directory_symlink_cycles() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let plan = dir.path().join("docs/plan/v2");
        fs::create_dir_all(&plan).unwrap();
        fs::write(plan.join("charter.md"), "# Charter\n").unwrap();
        symlink(dir.path().join("docs/plan"), plan.join("cycle")).unwrap();

        let files = md_files_under(&dir.path().join("docs/plan"));
        assert_eq!(files, vec![plan.join("charter.md")]);
    }

    #[cfg(unix)]
    #[test]
    fn path_identity_distinguishes_non_utf8_bytes() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let first = Path::new(OsStr::from_bytes(b"docs/a\x80.md"));
        let second = Path::new(OsStr::from_bytes(b"docs/a\x81.md"));
        let first_key = encode_path(first);
        let second_key = encode_path(second);
        assert_ne!(first_key, second_key);
        assert_eq!(first_key, "docs/a%80.md");
        assert_eq!(second_key, "docs/a%81.md");
    }

    #[test]
    fn test_build_match_expr() {
        assert_eq!(
            build_match_expr("why SQLite FTS5?").as_deref(),
            Some("\"why\" OR \"sqlite\" OR \"fts5\"")
        );
        assert_eq!(build_match_expr("?!').("), None);
    }

    #[test]
    fn test_recall_indexes_adr_and_capabilities() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        make_repo(repo.path());
        write_adr(
            repo.path(),
            "ADR-0042-recall-index.md",
            "Use bundled zanzibar index",
            "We chose the zanzibar approach because cursors are cheap.",
        );
        fs::create_dir_all(repo.path().join("docs")).unwrap();
        fs::write(
            repo.path().join("docs/capabilities.md"),
            "# Capabilities\n\n- CAP-001 zanzibar querying (shipped)\n",
        )
        .unwrap();

        let db = home.path().join("recall.db");
        let report = recall(
            &db,
            &[target("r1", repo.path())],
            "zanzibar",
            &RecallOptions::default(),
        )
        .unwrap();

        let kinds: HashSet<&str> = report.results.iter().map(|r| r.kind.as_str()).collect();
        assert!(kinds.contains("adr"), "kinds: {kinds:?}");
        assert!(kinds.contains("capability"), "kinds: {kinds:?}");
        let adr = report.results.iter().find(|r| r.kind == "adr").unwrap();
        assert_eq!(adr.title, "Use bundled zanzibar index");
        assert!(adr.path.starts_with("docs/decisions/"));
    }

    /// Product WHYs must be findable: a charter decision in a nested
    /// `docs/plan/**/*.md` and a scope decision in `docs/product.md` each
    /// surface as their own source kind, not just technical ADRs.
    #[test]
    fn test_recall_indexes_product_and_plan() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        make_repo(repo.path());
        write_product(
            repo.path(),
            "# codeflow — product\n\n## Non-goals\n\n\
             D18: no orchestration framework — the harness supplies the middle.\n",
        );
        // Nested to prove the recursive walk reaches docs/plan/v2/… (the charter).
        write_plan(
            repo.path(),
            "v2/00-charter.md",
            "# Charter\n\nD22: v1 is a quarry, not a source tree; \
             code crosses only by a deliberate keep-decision.\n",
        );
        let db = home.path().join("recall.db");
        let targets = [target("r1", repo.path())];

        // Charter decision term reaches the nested plan doc.
        let plan_hit = recall(&db, &targets, "D22 quarry", &RecallOptions::default()).unwrap();
        let plan = plan_hit
            .results
            .iter()
            .find(|r| r.kind == "plan")
            .expect("nested charter plan doc must surface");
        assert_eq!(plan.path, "docs/plan/v2/00-charter.md");
        assert_eq!(plan.title, "Charter");

        // Scope decision term reaches docs/product.md as its own kind.
        let product_hit = recall(
            &db,
            &targets,
            "D18 orchestration",
            &RecallOptions::default(),
        )
        .unwrap();
        let product = product_hit
            .results
            .iter()
            .find(|r| r.kind == "product")
            .expect("docs/product.md must surface");
        assert_eq!(product.path, "docs/product.md");
    }

    /// Dogfood layout: `epics/EPC-NNN/EPC-NNN.md` and
    /// `epics/EPC-NNN/tasks/TSK-*.md` nest one level deeper than the flat
    /// `epics/EPC-NNN.md` / `tasks/TSK-*.md` layout — a flat directory read
    /// would miss both entirely (this repo's own epics live there).
    #[test]
    fn test_recall_indexes_nested_epics_and_tasks() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        make_repo(repo.path());

        let epic_dir = repo.path().join("project-management/epics/EPC-009");
        fs::create_dir_all(&epic_dir).unwrap();
        fs::write(
            epic_dir.join("EPC-009.md"),
            "---\ntitle: Quokka rollout\nstatus: in_progress\n---\n\n\
             # Quokka rollout\n\nBrings quokka support to the platform.\n",
        )
        .unwrap();

        let task_dir = epic_dir.join("tasks");
        fs::create_dir_all(&task_dir).unwrap();
        fs::write(
            task_dir.join("TSK-009-001.md"),
            "---\ntitle: Quokka wiring task\nstatus: todo\n---\n\n\
             # Quokka wiring task\n\nWire the quokka adapter.\n",
        )
        .unwrap();

        let db = home.path().join("recall.db");
        let report = recall(
            &db,
            &[target("r1", repo.path())],
            "quokka",
            &RecallOptions::default(),
        )
        .unwrap();

        let paths: HashSet<&str> = report.results.iter().map(|r| r.path.as_str()).collect();
        assert!(
            paths.contains("project-management/epics/EPC-009/EPC-009.md"),
            "nested epic must surface: {paths:?}"
        );
        assert!(
            paths.contains("project-management/epics/EPC-009/tasks/TSK-009-001.md"),
            "nested task must surface: {paths:?}"
        );
        assert!(
            report.results.iter().all(|r| r.kind == "pm"),
            "nested epic/task files must be indexed as pm: {:?}",
            report.results
        );
    }

    #[test]
    fn test_recall_indexes_frozen_specs() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        make_repo(repo.path());
        let specs = repo.path().join("project-management/specs");
        fs::create_dir_all(&specs).unwrap();
        fs::write(
            specs.join("SPC-009.md"),
            "---\ntitle: Quokka wire protocol\nstatus: implemented\n---\n\n\
             # Quokka wire protocol\n\nPins the quokka frame checksum.\n",
        )
        .unwrap();

        let report = recall(
            &home.path().join("recall.db"),
            &[target("r1", repo.path())],
            "quokka checksum",
            &RecallOptions::default(),
        )
        .unwrap();

        assert!(report.results.iter().any(|result| {
            result.path == "project-management/specs/SPC-009.md" && result.kind == "pm"
        }));
    }

    #[test]
    fn test_recall_finds_feedback_by_its_verbatim_words() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        make_repo(repo.path());
        let feedback = repo.path().join(crate::feedback::FEEDBACK_DIR);
        fs::create_dir_all(&feedback).unwrap();
        fs::write(
            feedback.join("FB-001.md"),
            "---\nid: FB-001\ntitle: Version the API\nstatus: received\n---\n\n\
             # FB-001: Version the API\n\n## Verbatim\n\n> always say zebrafish versioning\n",
        )
        .unwrap();
        fs::write(feedback.join("INDEX.md"), "# zebrafish index\n").unwrap();

        let report = recall(
            &home.path().join("recall.db"),
            &[target("r1", repo.path())],
            "zebrafish",
            &RecallOptions::default(),
        )
        .unwrap();

        let paths: Vec<&str> = report.results.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            paths,
            ["project-management/feedback/FB-001.md"],
            "{paths:?}"
        );
        assert_eq!(report.results[0].kind, "pm");
    }

    #[test]
    fn test_recall_incremental_cursor_skips_unchanged() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        make_repo(repo.path());
        write_adr(repo.path(), "ADR-0001-a.md", "First", "alpha topic");
        let db = home.path().join("recall.db");
        let targets = [target("r1", repo.path())];

        let first = recall(&db, &targets, "alpha", &RecallOptions::default()).unwrap();
        assert_eq!(first.stats.files_indexed, 1);
        assert_eq!(first.stats.files_skipped, 0);

        // Unchanged: second run must skip via the (mtime, size) cursor.
        let second = recall(&db, &targets, "alpha", &RecallOptions::default()).unwrap();
        assert_eq!(second.stats.files_indexed, 0);
        assert_eq!(second.stats.files_skipped, 1);

        // Changed content (size differs) → re-indexed and findable.
        write_adr(
            repo.path(),
            "ADR-0001-a.md",
            "First",
            "alpha topic plus quokka detail",
        );
        let third = recall(&db, &targets, "quokka", &RecallOptions::default()).unwrap();
        assert_eq!(third.stats.files_indexed, 1);
        assert_eq!(third.results.len(), 1);
        assert!(third.results[0].snippet.contains("[quokka]"));
    }

    #[test]
    fn test_recall_removes_vanished_sources() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        make_repo(repo.path());
        write_adr(repo.path(), "ADR-0001-a.md", "First", "ephemeral xylophone");
        let db = home.path().join("recall.db");
        let targets = [target("r1", repo.path())];

        recall(&db, &targets, "xylophone", &RecallOptions::default()).unwrap();
        fs::remove_file(repo.path().join("docs/decisions/ADR-0001-a.md")).unwrap();

        let after = recall(&db, &targets, "xylophone", &RecallOptions::default()).unwrap();
        assert_eq!(after.stats.files_removed, 1);
        assert!(
            after.results.is_empty(),
            "deleted sources must leave the index"
        );
    }

    #[test]
    fn test_recall_rebuild_drops_and_resyncs() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        make_repo(repo.path());
        write_adr(repo.path(), "ADR-0001-a.md", "First", "rebuildable cache");
        let db = home.path().join("recall.db");
        let targets = [target("r1", repo.path())];

        recall(&db, &targets, "rebuildable", &RecallOptions::default()).unwrap();
        let rebuilt = recall(
            &db,
            &targets,
            "rebuildable",
            &RecallOptions {
                rebuild: true,
                limit: 20,
            },
        )
        .unwrap();
        assert_eq!(
            rebuilt.stats.files_indexed, 1,
            "rebuild must drop cursors and re-index"
        );
        assert_eq!(rebuilt.results.len(), 1);
    }

    #[test]
    fn test_recall_skips_missing_repo_with_note() {
        let home = tempfile::tempdir().unwrap();
        let db = home.path().join("recall.db");
        let gone = RepoTarget {
            name: "ghost".into(),
            root: PathBuf::from("/nonexistent/ghost-repo"),
        };
        let report = recall(&db, &[gone], "anything", &RecallOptions::default()).unwrap();
        assert!(report.results.is_empty());
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("ghost") && n.contains("skipped")),
            "notes: {:?}",
            report.notes
        );
    }

    #[test]
    fn test_recall_discloses_source_coverage_gaps() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        make_repo(repo.path());
        write_adr(repo.path(), "ADR-0001-a.md", "Only ADRs", "lonely corpus");
        let db = home.path().join("recall.db");

        let report = recall(
            &db,
            &[target("gappy", repo.path())],
            "lonely",
            &RecallOptions::default(),
        )
        .unwrap();
        let note = report
            .notes
            .iter()
            .find(|n| n.starts_with("gappy:"))
            .expect("coverage gap note expected");
        for kind in ["ledger", "session", "pm", "capability"] {
            assert!(note.contains(kind), "note missing {kind}: {note}");
        }
        assert!(!note.contains("adr"), "adr was indexed: {note}");
    }

    #[test]
    fn test_recall_indexes_jsonl_per_event() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        make_repo(repo.path());
        write_memory_event(
            repo.path(),
            r#"{"event":"decision","timestamp":"2026-06-10T10:00:00Z","summary":"picked flatbuffers over protobuf for zero-copy"}"#,
        );
        write_memory_event(
            repo.path(),
            r#"{"event":"milestone","timestamp":"2026-06-10T11:00:00Z","summary":"shipped the parser"}"#,
        );
        let db = home.path().join("recall.db");

        let report = recall(
            &db,
            &[target("r1", repo.path())],
            "flatbuffers",
            &RecallOptions::default(),
        )
        .unwrap();
        assert_eq!(report.results.len(), 1, "only the matching event line");
        assert_eq!(report.results[0].kind, "session");
        assert_eq!(report.results[0].title, "decision");
    }

    /// AC #10 evidence: a "why" question answered from an ADR + a session
    /// summary across two registered repos.
    #[test]
    fn test_recall_answers_why_across_two_repos() {
        let home = tempfile::tempdir().unwrap();
        let repo_a = tempfile::tempdir().unwrap();
        let repo_b = tempfile::tempdir().unwrap();
        make_repo(repo_a.path());
        make_repo(repo_b.path());

        write_adr(
            repo_a.path(),
            "ADR-0007-sqlite-recall.md",
            "SQLite FTS5 for recall",
            "Context: recall needs full-text search with no system dependency.\n\
             Decision: use SQLite FTS5 via bundled rusqlite.\n\
             Consequences: the index is a rebuildable cache; no daemon.",
        );
        write_memory_event(
            repo_b.path(),
            r#"{"event":"session_summary","timestamp":"2026-06-11T09:00:00Z","summary":"Wired recall to SQLite FTS5 because the bundled build avoids a system dependency; cursors make sync lazy."}"#,
        );

        let db = home.path().join("recall.db");
        let report = recall(
            &db,
            &[
                target("proj-a", repo_a.path()),
                target("proj-b", repo_b.path()),
            ],
            "why did we choose SQLite FTS5 for recall",
            &RecallOptions::default(),
        )
        .unwrap();

        let repos: HashSet<&str> = report.results.iter().map(|r| r.repo.as_str()).collect();
        assert!(
            repos.contains("proj-a") && repos.contains("proj-b"),
            "both repos must answer: {repos:?}"
        );
        let kinds: HashSet<&str> = report.results.iter().map(|r| r.kind.as_str()).collect();
        assert!(
            kinds.contains("adr") && kinds.contains("session"),
            "ADR and session summary must both surface: {kinds:?}"
        );
    }

    #[test]
    fn test_recall_empty_query_is_error() {
        let home = tempfile::tempdir().unwrap();
        let db = home.path().join("recall.db");
        let err = recall(&db, &[], "  ?! ", &RecallOptions::default()).unwrap_err();
        assert!(matches!(err, RecallError::EmptyQuery));
    }
}
