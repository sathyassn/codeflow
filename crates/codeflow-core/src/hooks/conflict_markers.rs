//! Unresolved conflict markers in the lines a change adds (TSK-170, SPC-013
//! planning resolution 26): the matcher, the `conflict-marker-size` lookup
//! and the message text, shared by the pre-commit hook and `codeflow ci`.
//!
//! A marker is judged the way `git diff --check` and git's merge judge it.
//! An opening, closing or diff3 base marker is its character repeated the
//! path's `conflict-marker-size` times (7 unless `.gitattributes` sets it),
//! then a space or the line end. A separator is a line of exactly that many
//! `=`, and counts only between an opening and a closing marker among the
//! file's added lines, so a Markdown setext heading underline is never a
//! finding. A file that must hold markers sets `conflict-marker-size` for its
//! path to a length its markers do not have: git's merge then writes markers
//! of that length there too, so a real conflict in it is still caught.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use super::policy::PolicyLevel;
use super::Violation;

/// The policy rule, set by `git.conflict_markers`.
pub const RULE: &str = "git.conflict_markers";

/// Git's marker length when no attribute sets one.
pub const DEFAULT_SIZE: usize = 7;

/// The attribute git's merge and `git diff --check` read.
const ATTRIBUTE: &str = "conflict-marker-size";

/// Git's binary heuristic, applied to content: a NUL byte in the first 8000
/// bytes.
const BINARY_SNIFF_BYTES: usize = 8000;

/// The kind of one marker line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The line that opens a conflict (`<` repeated).
    Opening,
    /// The diff3 or zdiff3 base section's line (`|` repeated).
    Base,
    /// The line between the two sides (`=` repeated).
    Separator,
    /// The line that closes a conflict (`>` repeated).
    Closing,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Opening => "opening",
            Self::Base => "diff3 base",
            Self::Separator => "separator",
            Self::Closing => "closing",
        }
    }

    fn fill(self) -> char {
        match self {
            Self::Opening => '<',
            Self::Base => '|',
            Self::Separator => '=',
            Self::Closing => '>',
        }
    }
}

/// One marker found on an added line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// The line number in the new file.
    pub line: usize,
    /// What the line is.
    pub kind: Kind,
}

/// The lines a change adds, per text file: each line's number in the new
/// file and its text, in file order.
pub type AddedLines = BTreeMap<String, Vec<(usize, String)>>;

/// Where the `.gitattributes` that decide each path's marker size are read.
#[derive(Debug, Clone, Copy)]
pub enum AttrSource<'a> {
    /// The index of the repository at this git directory, as the commit
    /// being made will record it (`git check-attr --cached`).
    Index {
        /// The repository's git directory, named so an inherited `GIT_DIR`
        /// never points the lookup at another repository.
        git_dir: &'a Path,
        /// The index file the commit records, from [`effective_index`].
        index_file: &'a Path,
    },
    /// The tree of this revision (`git check-attr --source`).
    Revision(&'a str),
}

/// Why the `conflict-marker-size` lookup failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrError {
    /// This git has no `check-attr --source` (it came in git 2.40).
    SourceUnsupported(String),
    /// Any other failure, with git's message.
    Failed(String),
}

impl std::fmt::Display for AttrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceUnsupported(message) | Self::Failed(message) => f.write_str(message),
        }
    }
}

/// The index file a commit in progress records: `GIT_INDEX_FILE` when git
/// set it, as it does for `git commit -a` and `git commit <path>`, resolved
/// against the working directory as git resolves it; otherwise the
/// repository's own index.
#[must_use]
pub fn effective_index(repo: &git2::Repository) -> std::path::PathBuf {
    match std::env::var_os("GIT_INDEX_FILE").filter(|value| !value.is_empty()) {
        Some(value) => {
            let path = std::path::PathBuf::from(value);
            if path.is_absolute() {
                path
            } else {
                std::env::current_dir().map_or(path.clone(), |cwd| cwd.join(&path))
            }
        }
        None => repo.path().join("index"),
    }
}

/// The marker a line is at `size`, or `None`.
fn classify(text: &str, size: usize) -> Option<Kind> {
    let text = text.strip_suffix('\n').unwrap_or(text);
    let text = text.strip_suffix('\r').unwrap_or(text);
    let kind = match text.as_bytes().first()? {
        b'<' => Kind::Opening,
        b'|' => Kind::Base,
        b'=' => Kind::Separator,
        b'>' => Kind::Closing,
        _ => return None,
    };
    let fill = text.as_bytes()[0];
    let run = text.as_bytes().get(..size)?;
    if !run.iter().all(|byte| *byte == fill) {
        return None;
    }
    let rest = &text.as_bytes()[size..];
    let ends = match kind {
        Kind::Separator => rest.is_empty(),
        _ => rest.is_empty() || rest[0] == b' ',
    };
    ends.then_some(kind)
}

/// The markers among one file's added lines, judged at `size`. Opening,
/// closing and base markers count on their own; a separator counts only
/// after an opening marker and before the closing marker that follows it,
/// across hunks.
#[must_use]
pub fn markers_in<'a>(
    lines: impl IntoIterator<Item = (usize, &'a str)>,
    size: usize,
) -> Vec<Marker> {
    let mut found = Vec::new();
    let mut open = false;
    let mut separators = Vec::new();
    for (line, text) in lines {
        match classify(text, size) {
            Some(Kind::Separator) => {
                if open {
                    separators.push(line);
                }
            }
            Some(kind) => {
                found.push(Marker { line, kind });
                if kind == Kind::Opening {
                    open = true;
                } else if kind == Kind::Closing {
                    if open {
                        found.extend(separators.iter().map(|line| Marker {
                            line: *line,
                            kind: Kind::Separator,
                        }));
                    }
                    open = false;
                    separators.clear();
                }
            }
            None => {}
        }
    }
    found.sort_by_key(|marker| marker.line);
    found
}

/// Git's content test for binary data.
#[must_use]
pub fn is_binary(content: &[u8]) -> bool {
    content
        .iter()
        .take(BINARY_SNIFF_BYTES)
        .any(|byte| *byte == 0)
}

/// The marker size an attribute value sets, as git's merge reads it: a
/// positive number, anything else the default.
fn size_from(value: &str) -> usize {
    value
        .parse::<usize>()
        .ok()
        .filter(|size| *size > 0)
        .unwrap_or(DEFAULT_SIZE)
}

/// The `conflict-marker-size` of each path, read by git itself from
/// `.gitattributes` at `source`. `root` is the top of the work tree.
///
/// # Errors
///
/// [`AttrError::SourceUnsupported`] for a git older than 2.40, which has no
/// `--source`; [`AttrError::Failed`] with git's message otherwise.
pub fn marker_sizes(
    root: &Path,
    source: AttrSource<'_>,
    paths: &[&str],
) -> Result<BTreeMap<String, usize>, AttrError> {
    let failed = |e: &dyn std::fmt::Display| AttrError::Failed(format!("git check-attr: {e}"));
    if paths.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut command = crate::git::command();
    command.arg("-C").arg(root);
    let from = match source {
        AttrSource::Index {
            git_dir,
            index_file,
        } => {
            command
                .arg("--git-dir")
                .arg(git_dir)
                .env("GIT_INDEX_FILE", index_file);
            "--cached".to_string()
        }
        AttrSource::Revision(revision) => format!("--source={revision}"),
    };
    let mut child = command
        .args(["check-attr", "-z", "--stdin", &from, ATTRIBUTE])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failed(&e))?;
    let mut stdin = child.stdin.take().ok_or_else(|| failed(&"no stdin"))?;
    let input: Vec<u8> = paths
        .iter()
        .flat_map(|path| path.bytes().chain(std::iter::once(0)))
        .collect();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output().map_err(|e| failed(&e))?;
    writer
        .join()
        .map_err(|_| failed(&"input writer panicked"))?
        .map_err(|e| failed(&e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let message = format!("git check-attr: {stderr}");
        return Err(
            if matches!(source, AttrSource::Revision(_)) && stderr.contains("unknown option") {
                AttrError::SourceUnsupported(message)
            } else {
                AttrError::Failed(message)
            },
        );
    }
    // `-z` output is path, attribute, value, each ended by a NUL.
    let text = String::from_utf8_lossy(&out.stdout);
    let fields: Vec<&str> = text.split('\0').collect();
    Ok(fields
        .chunks_exact(3)
        .map(|record| (record[0].to_string(), size_from(record[2])))
        .collect())
}

/// The findings for `files` at `level`, each file judged at its size in
/// `sizes` (the default when absent). Silent below `warn`.
#[must_use]
pub fn check(
    level: PolicyLevel,
    files: &AddedLines,
    sizes: &BTreeMap<String, usize>,
) -> Vec<Violation> {
    if !level.is_active() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (path, lines) in files {
        let size = sizes.get(path).copied().unwrap_or(DEFAULT_SIZE);
        for marker in markers_in(lines.iter().map(|(n, text)| (*n, text.as_str())), size) {
            out.push(Violation::new(
                RULE,
                level,
                format!(
                    "{path}:{} adds an unresolved {} conflict marker ({})",
                    marker.line,
                    marker.kind.name(),
                    marker.kind.fill().to_string().repeat(size)
                ),
                crate::remedy::CONFLICT_MARKER.with(&[("path", path)]),
            ));
        }
    }
    out
}

/// The pre-commit plane's findings over the staged diff, at `level`: the
/// index the commit records ([`effective_index`]) against `HEAD`. Gitlinks
/// (submodule entries) are skipped: their object is a commit in another
/// repository, never text here.
#[must_use]
pub fn staged(repo: &git2::Repository, level: PolicyLevel) -> Vec<Violation> {
    let fail = |error: &str| {
        vec![incomplete(
            level,
            error,
            crate::remedy::SECRET_SCAN_INCOMPLETE.remedy(),
        )]
    };
    let Some(root) = repo.workdir() else {
        return Vec::new();
    };
    let index_file = effective_index(repo);
    let index = match git2::Index::open(&index_file) {
        Ok(index) => index,
        Err(error) => {
            return fail(&format!(
                "cannot read the index {}: {error}",
                index_file.display()
            ))
        }
    };
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    // Text is forced, as `codeflow ci` does with `--text`: a `binary` or
    // `-diff` attribute never hides a text addition; content decides.
    let mut options = git2::DiffOptions::new();
    options.force_text(true).context_lines(0);
    let diff = match repo.diff_tree_to_index(head_tree.as_ref(), Some(&index), Some(&mut options)) {
        Ok(diff) => diff,
        Err(error) => return fail(&error.to_string()),
    };
    let mut files = AddedLines::new();
    let mut blobs: BTreeMap<String, git2::Oid> = BTreeMap::new();
    let walked = diff.foreach(
        &mut |_, _| true,
        None,
        None,
        Some(&mut |delta, _hunk, line| {
            if line.origin() == '+' && delta.new_file().mode() != git2::FileMode::Commit {
                if let Some(path) = delta.new_file().path() {
                    let path = path.to_string_lossy().into_owned();
                    let number = line
                        .new_lineno()
                        .and_then(|n| usize::try_from(n).ok())
                        .unwrap_or(0);
                    let text = String::from_utf8_lossy(line.content()).into_owned();
                    blobs.insert(path.clone(), delta.new_file().id());
                    files.entry(path).or_default().push((number, text));
                }
            }
            true
        }),
    );
    if let Err(error) = walked {
        return fail(&error.to_string());
    }
    for (path, oid) in &blobs {
        match repo.find_blob(*oid) {
            Ok(blob) if is_binary(blob.content()) => {
                files.remove(path);
            }
            Ok(_) => {}
            Err(error) => return fail(&error.to_string()),
        }
    }
    let paths: Vec<&str> = files.keys().map(String::as_str).collect();
    let source = AttrSource::Index {
        git_dir: repo.path(),
        index_file: &index_file,
    };
    match marker_sizes(root, source, &paths) {
        Ok(sizes) => check(level, &files, &sizes),
        Err(error) => fail(&error.to_string()),
    }
}

/// A finding that the check could not complete, at the rule's level, so an
/// unread change is never a pass.
#[must_use]
pub fn incomplete(level: PolicyLevel, error: &str, remedy: crate::remedy::Remedy) -> Violation {
    Violation::new(
        RULE,
        level,
        format!("conflict-marker check incomplete: {error}"),
        remedy,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A marker line built at run time, so this file holds none itself.
    fn m(fill: char, size: usize, label: &str) -> String {
        format!("{}{label}", fill.to_string().repeat(size))
    }

    fn kinds(lines: &[String], size: usize) -> Vec<(usize, Kind)> {
        markers_in(
            lines
                .iter()
                .enumerate()
                .map(|(i, text)| (i + 1, text.as_str())),
            size,
        )
        .into_iter()
        .map(|marker| (marker.line, marker.kind))
        .collect()
    }

    fn files(path: &str, lines: &[String]) -> AddedLines {
        let mut out = AddedLines::new();
        out.insert(
            path.to_string(),
            lines
                .iter()
                .enumerate()
                .map(|(i, text)| (i + 1, text.clone()))
                .collect(),
        );
        out
    }

    #[test]
    fn each_marker_kind_is_found_with_and_without_a_label() {
        // AC-1.
        for label in ["", " HEAD", " feat/x (add the thing)"] {
            let lines = vec![
                m('<', 7, label),
                "ours".into(),
                m('|', 7, label),
                "base".into(),
                m('=', 7, ""),
                "theirs".into(),
                m('>', 7, label),
            ];
            assert_eq!(
                kinds(&lines, 7),
                vec![
                    (1, Kind::Opening),
                    (3, Kind::Base),
                    (5, Kind::Separator),
                    (7, Kind::Closing)
                ],
                "label {label:?}"
            );
        }
    }

    #[test]
    fn opening_closing_and_base_markers_count_on_their_own() {
        // AC-1: a leftover half of a conflict is still a finding.
        assert_eq!(kinds(&[m('<', 7, " HEAD")], 7), vec![(1, Kind::Opening)]);
        assert_eq!(kinds(&[m('>', 7, " main")], 7), vec![(1, Kind::Closing)]);
        assert_eq!(kinds(&[m('|', 7, " base")], 7), vec![(1, Kind::Base)]);
    }

    #[test]
    fn a_separator_counts_only_between_an_opening_and_a_closing_marker() {
        // AC-2: a Markdown setext heading underline is never a finding.
        let heading = vec!["Title".into(), m('=', 7, ""), String::new(), "text".into()];
        assert!(kinds(&heading, 7).is_empty());
        // After a closing marker, or before any opening one, it is text.
        let after = vec![
            m('<', 7, " a"),
            m('>', 7, " b"),
            "Title".into(),
            m('=', 7, ""),
        ];
        assert_eq!(
            kinds(&after, 7),
            vec![(1, Kind::Opening), (2, Kind::Closing)]
        );
        // Opened but never closed: the separator is not judged.
        let open = vec![m('<', 7, " a"), m('=', 7, "")];
        assert_eq!(kinds(&open, 7), vec![(1, Kind::Opening)]);
    }

    #[test]
    fn the_order_is_judged_across_hunks() {
        // An opening in one hunk and its separator in a later one.
        let lines = [
            (3, m('<', 7, " HEAD")),
            (40, m('=', 7, "")),
            (41, "theirs".to_string()),
            (42, m('>', 7, " feat/x")),
        ];
        let found: Vec<(usize, Kind)> = markers_in(lines.iter().map(|(n, t)| (*n, t.as_str())), 7)
            .into_iter()
            .map(|marker| (marker.line, marker.kind))
            .collect();
        assert_eq!(
            found,
            vec![
                (3, Kind::Opening),
                (40, Kind::Separator),
                (42, Kind::Closing)
            ]
        );
    }

    #[test]
    fn a_marker_must_start_its_line_and_have_the_exact_size() {
        // AC-2.
        let lines = vec![
            format!(" {}", m('<', 7, " HEAD")),
            format!("x{}", m('>', 7, "")),
            m('<', 8, " HEAD"),
            m('<', 6, " HEAD"),
            m('>', 7, "x"),
            m('=', 8, ""),
            m('=', 7, " "),
        ];
        assert!(kinds(&lines, 7).is_empty(), "{:?}", kinds(&lines, 7));
    }

    #[test]
    fn a_line_end_may_carry_a_carriage_return_or_newline() {
        let lines = vec![
            format!("{}\r\n", m('<', 7, " HEAD")),
            format!("{}\r", m('=', 7, "")),
            format!("{}\n", m('>', 7, "")),
        ];
        assert_eq!(kinds(&lines, 7).len(), 3);
    }

    #[test]
    fn the_path_size_decides_what_is_a_marker() {
        // AC-3: at size 32, 7-character markers are text and 32-character
        // ones are markers.
        let seven = vec![m('<', 7, " a"), m('=', 7, ""), m('>', 7, " b")];
        assert!(kinds(&seven, 32).is_empty());
        let long = vec![m('<', 32, " a"), m('=', 32, ""), m('>', 32, " b")];
        assert_eq!(kinds(&long, 32).len(), 3);
    }

    #[test]
    fn attribute_values_read_as_git_merge_reads_them() {
        assert_eq!(size_from("32"), 32);
        assert_eq!(size_from("unspecified"), DEFAULT_SIZE);
        assert_eq!(size_from("unset"), DEFAULT_SIZE);
        assert_eq!(size_from("set"), DEFAULT_SIZE);
        assert_eq!(size_from("0"), DEFAULT_SIZE);
        assert_eq!(size_from("-3"), DEFAULT_SIZE);
    }

    #[test]
    fn binary_content_is_recognised_by_a_nul_byte() {
        assert!(is_binary(b"abc\0def"));
        assert!(!is_binary(b"plain text\n"));
    }

    #[test]
    fn check_names_file_line_and_marker_at_each_level() {
        // AC-1.
        let lines = vec![m('<', 7, " HEAD"), "x".into(), m('>', 7, " b")];
        let found = check(
            PolicyLevel::Block,
            &files("src/a.rs", &lines),
            &BTreeMap::new(),
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].rule, RULE);
        assert_eq!(found[0].level, PolicyLevel::Block);
        assert!(
            found[0].message.starts_with("src/a.rs:1 "),
            "{}",
            found[0].message
        );
        assert!(found[0].message.contains("opening"), "{}", found[0].message);
        assert!(
            found[0].message.contains(&m('<', 7, "")),
            "{}",
            found[0].message
        );
        assert!(
            found[1].message.starts_with("src/a.rs:3 "),
            "{}",
            found[1].message
        );
        assert!(found[0].remedy.contains(
            "resolve the conflict and restage, or set conflict-marker-size for the path in .gitattributes"
        ));
        assert!(found[0].remedy.contains("src/a.rs"));
        let warned = check(
            PolicyLevel::Warn,
            &files("src/a.rs", &lines),
            &BTreeMap::new(),
        );
        assert_eq!(warned.len(), 2);
        assert_eq!(warned[0].level, PolicyLevel::Warn);
        for quiet in [PolicyLevel::Off, PolicyLevel::Allow] {
            assert!(check(quiet, &files("src/a.rs", &lines), &BTreeMap::new()).is_empty());
        }
    }

    #[test]
    fn check_reads_each_paths_size() {
        let lines = vec![m('<', 7, " a"), m('=', 7, ""), m('>', 7, " b")];
        let sizes = BTreeMap::from([("fixture.txt".to_string(), 32)]);
        assert!(check(PolicyLevel::Block, &files("fixture.txt", &lines), &sizes).is_empty());
        assert_eq!(
            check(PolicyLevel::Block, &files("other.txt", &lines), &sizes).len(),
            3
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = crate::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn marker_sizes_read_the_staged_and_the_committed_attributes() {
        // AC-3: the index as the commit will record it, and a revision.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "chore: init"]);
        std::fs::create_dir_all(root.join("fixtures")).unwrap();
        std::fs::write(
            root.join(".gitattributes"),
            "fixtures/** conflict-marker-size=32\n",
        )
        .unwrap();
        // Written but not staged: the index does not have it yet.
        let git_dir = root.join(".git");
        let index_file = git_dir.join("index");
        let index = AttrSource::Index {
            git_dir: &git_dir,
            index_file: &index_file,
        };
        let paths = ["fixtures/x.txt", "a.txt"];
        let unstaged = marker_sizes(root, index, &paths).unwrap();
        assert_eq!(unstaged.get("fixtures/x.txt"), Some(&DEFAULT_SIZE));
        git(root, &["add", ".gitattributes"]);
        let staged = marker_sizes(root, index, &paths).unwrap();
        assert_eq!(staged.get("fixtures/x.txt"), Some(&32));
        assert_eq!(staged.get("a.txt"), Some(&DEFAULT_SIZE));
        // The revision before the commit has no attribute; after it does.
        let before = marker_sizes(root, AttrSource::Revision("HEAD"), &paths).unwrap();
        assert_eq!(before.get("fixtures/x.txt"), Some(&DEFAULT_SIZE));
        git(root, &["commit", "-q", "-m", "chore: add attributes"]);
        let after = marker_sizes(root, AttrSource::Revision("HEAD"), &paths).unwrap();
        assert_eq!(after.get("fixtures/x.txt"), Some(&32));
        assert!(marker_sizes(root, AttrSource::Revision("HEAD"), &[])
            .unwrap()
            .is_empty());
    }
}
