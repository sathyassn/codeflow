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
//! | `pm`         | `project-management/{epics,tasks}/*.md` |
//! | `capability` | `docs/capabilities.md` |
//!
//! where `<state>` is the repo's runtime state dir (`.git/codeflow`).
//! Results disclose coverage gaps — repos or source kinds that were never
//! indexed are reported, never silently empty (charter principle 8).

use std::collections::HashSet;
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
    /// Source kind (`ledger`, `session`, `adr`, `pm`, `capability`).
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
const KINDS: &[&str] = &["ledger", "session", "adr", "pm", "capability"];

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
    /// Path relative to the repo root (display + cursor key).
    rel: String,
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
        .filter(|p| {
            p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md"))
        })
        .collect();
    files.sort();
    files
}

fn rel_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// Enumerate all indexable source files for a repo.
fn collect_sources(root: &Path) -> Vec<SourceFile> {
    let mut sources = Vec::new();

    if let Some(state) = runtime_state_dir(root) {
        let ledger_dir = state.join("ledger");
        for (types, kind) in [
            (
                &[crate::ledger::files::WORK_GRAPH, crate::ledger::files::CONFIG][..],
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
                    sources.push(SourceFile {
                        rel: rel_to(root, &abs),
                        abs,
                        kind,
                    });
                }
            }
        }
    }

    for abs in md_files_in(&root.join("docs/decisions")) {
        sources.push(SourceFile {
            rel: rel_to(root, &abs),
            abs,
            kind: "adr",
        });
    }

    for sub in ["epics", "tasks"] {
        for abs in md_files_in(&root.join("project-management").join(sub)) {
            sources.push(SourceFile {
                rel: rel_to(root, &abs),
                abs,
                kind: "pm",
            });
        }
    }

    let caps = root.join("docs/capabilities.md");
    if caps.is_file() {
        sources.push(SourceFile {
            rel: rel_to(root, &caps),
            abs: caps,
            kind: "capability",
        });
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
        let stem = Path::new(&src.rel)
            .file_stem()
            .map_or_else(|| src.rel.clone(), |s| s.to_string_lossy().into_owned());
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
    let repo_key = target.root.to_string_lossy().into_owned();
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
        let repo_key = target.root.to_string_lossy().into_owned();
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
                .find(|t| t.root.to_string_lossy() == repo_key)
                .map_or(repo_key, |t| t.name.clone());
            results.push(RecallResult {
                repo,
                kind,
                path,
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
        write_adr(repo.path(), "ADR-0001-a.md", "First", "alpha topic plus quokka detail");
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
            &[target("proj-a", repo_a.path()), target("proj-b", repo_b.path())],
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
