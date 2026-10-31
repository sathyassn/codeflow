//! The native differential fixture for the deletion guard (TSK-286,
//! issue 102).
//!
//! Each row runs a real tool (`xargs`, `parallel`, `find`, `ls`, `rm`)
//! inside a disposable directory under the build's `target/` folder, lists
//! what the tool removed, and compares that with what the guard
//! judged for the same command in the same directory. The guard's
//! judgment is read from every path it resolves for a deletion
//! ([`record`]). A row expects one of three outcomes:
//!
//! - `Exact`: the guard allows the command, and the tool removes exactly
//!   the paths the guard judged, with everything below them.
//! - `Within`: the guard allows the command, and the tool removes only
//!   paths at or below those the guard judged (`find`'s tests select
//!   within the tree the guard judges).
//! - `Unproven`: the guard refuses the command as unproven. The tool still
//!   runs, so the row shows the grammar does something the guard would
//!   have to model.
//!
//! The grammar table in the module documentation of `deletion.rs` lists
//! each grammar as modelled, unproven or untested;
//! `every_listed_grammar_has_a_fixture_row` fails when a modelled or
//! unproven grammar has no row here. Commands name only relative paths or
//! the fixture directory, and the fixture never runs a command against a
//! path outside it.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

thread_local! {
    /// The paths the reader resolved for a deletion while a fixture row
    /// reads its command; `None` outside a row.
    static JUDGED: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

thread_local! {
    /// The directory a fixture row treats as protected, as a temp root or
    /// a home is; `None` outside such a row.
    static STAND_IN: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The protected stand-in of the row being read, if it has one: the
/// row's `guarded/` directory.
pub(super) fn stand_in() -> Option<String> {
    STAND_IN.with(|stand_in| stand_in.borrow().clone())
}

/// Keep `path` as one the reader judged for a deletion.
pub(super) fn record(path: &str) {
    JUDGED.with(|judged| {
        if let Some(paths) = judged.borrow_mut().as_mut() {
            paths.push(path.to_string());
        }
    });
}

/// The guard's verdict on `command` started in `base`, and every path it
/// judged for a deletion.
fn judge(
    command: &str,
    base: &Path,
    stand_in: Option<&Path>,
) -> (Option<super::Found>, Vec<String>) {
    JUDGED.with(|judged| *judged.borrow_mut() = Some(Vec::new()));
    STAND_IN.with(|cell| *cell.borrow_mut() = stand_in.map(|p| p.display().to_string()));
    let found = super::read_deletion(command, Some(base), false);
    STAND_IN.with(|cell| *cell.borrow_mut() = None);
    let paths = JUDGED.with(|judged| judged.borrow_mut().take().unwrap_or_default());
    (found, paths)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    Exact,
    Within,
    Unproven,
    /// The guard refuses it as a protected deletion: a match may be the
    /// protected stand-in. The tool still runs and removes the stand-in.
    Refused,
}

/// Which native tool a row needs; a row whose tool is absent is listed as
/// skipped on that host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Needs {
    /// Any POSIX `sh`, `xargs`, `find` and `rm`.
    Posix,
    /// GNU `xargs` (`-d` is GNU's).
    GnuXargs,
    /// GNU `parallel`.
    Parallel,
    /// GNU `find`, which walks `.` when no start is named.
    GnuFind,
    /// BSD `find`, which reads `-f PATH` and clustered options.
    BsdFind,
    /// `python3`.
    Python,
}

/// The native tools this host has, beyond POSIX ones.
struct Host(Vec<Needs>);

impl Host {
    fn probe() -> Self {
        let answers = |command: &mut Command, mark: &str| {
            command.arg("--version").output().is_ok_and(|out| {
                out.status.success() && String::from_utf8_lossy(&out.stdout).contains(mark)
            })
        };
        let gnu_find = answers(&mut Command::new("find"), "GNU");
        let mut has = vec![if gnu_find {
            Needs::GnuFind
        } else {
            Needs::BsdFind
        }];
        for (needs, present) in [
            (Needs::GnuXargs, answers(&mut Command::new("xargs"), "")),
            (
                Needs::Parallel,
                answers(&mut Command::new("parallel"), "GNU parallel"),
            ),
            (
                Needs::Python,
                answers(&mut Command::new("python3"), "Python"),
            ),
        ] {
            if present {
                has.push(needs);
            }
        }
        Self(has)
    }

    fn has(&self, needs: Needs) -> bool {
        needs == Needs::Posix || self.0.contains(&needs)
    }
}

struct Row {
    /// The grammar key, as the table in `deletion.rs` names it.
    grammar: &'static str,
    /// Entries to create: `dir/`, `file`, or `link -> target`.
    setup: &'static [&'static str],
    command: String,
    expect: Expect,
    needs: Needs,
}

fn row(
    grammar: &'static str,
    setup: &'static [&'static str],
    command: &str,
    expect: Expect,
) -> Row {
    Row {
        grammar,
        setup,
        command: command.to_string(),
        expect,
        needs: Needs::Posix,
    }
}

fn needing(mut row: Row, needs: Needs) -> Row {
    row.needs = needs;
    row
}

const TREE: &[&str] = &["sub/", "sub/x/", "sub/x/f", "sub/y", "sub/z/", "keep"];
const SPACED: &[&str] = &["a", "b c", "d", "keep"];
/// A link below the start that leads out of it.
const LINKED: &[&str] = &["sub/", "sub/link -> ../out", "out/", "out/x", "keep"];
/// The protected stand-in below the start, where a match can name it.
const GUARDED_BELOW: &[&str] = &["sub/", "sub/guarded/", "sub/guarded/f", "sub/y", "keep"];
/// The protected stand-in outside the start, reached through a link below
/// it.
const GUARDED_LINK: &[&str] = &[
    "sub/",
    "sub/link -> ../guarded",
    "guarded/",
    "guarded/f",
    "keep",
];

#[allow(clippy::too_many_lines)]
fn rows() -> Vec<Row> {
    use Expect::{Exact, Refused, Unproven, Within};
    let long = "name-with-some-length\n".repeat(110);
    vec![
        // xargs framing and invocation.
        row(
            "xargs-blanks",
            SPACED,
            r#"printf 'a "b c"\n' | xargs rm -rf"#,
            Exact,
        ),
        row(
            "xargs-blanks",
            SPACED,
            r"printf 'a b\\ c\n' | xargs rm -rf --",
            Exact,
        ),
        row(
            "xargs-batch",
            SPACED,
            r#"printf 'a\nd\n' | xargs sh -c 'rm -rf "$2"' _"#,
            Exact,
        ),
        row(
            "xargs-batch",
            SPACED,
            r#"printf 'a\nd\n' | xargs sh -c 'rm -rf "$0"'"#,
            Exact,
        ),
        row(
            "xargs-batch-size",
            SPACED,
            &format!("xargs rm -rf <<'EOF'\n{long}a\nEOF"),
            Unproven,
        ),
        row(
            "xargs-null",
            SPACED,
            r"printf 'b c\0a\0' | xargs -0 rm -rf",
            Exact,
        ),
        row(
            "xargs-null",
            SPACED,
            r"printf 'b c\0' | xargs --null rm -rf",
            Exact,
        ),
        needing(
            row(
                "xargs-delimiter",
                SPACED,
                "printf 'b c:a' | xargs -d : rm -rf",
                Exact,
            ),
            Needs::GnuXargs,
        ),
        row(
            "xargs-replace",
            SPACED,
            r"printf 'b c\n  a\n' | xargs -I{} rm -rf {}",
            Exact,
        ),
        row(
            "xargs-replace",
            SPACED,
            r"printf 'b c\n' | xargs -I % rm -rf %",
            Exact,
        ),
        row(
            "xargs-replace-partial",
            SPACED,
            r"printf 'c\n' | xargs -I{} rm -rf 'b {}'",
            Unproven,
        ),
        row(
            "xargs-run-control",
            SPACED,
            r"printf 'a\nd\n' | xargs -t -r -P 2 rm -rf",
            Exact,
        ),
        row(
            "xargs-max-lines",
            SPACED,
            r"printf 'a\nd\n' | xargs -L 1 rm -rf",
            Unproven,
        ),
        row(
            "xargs-max-args",
            SPACED,
            r"printf 'a\nd\n' | xargs -n 1 rm -rf",
            Unproven,
        ),
        row(
            "xargs-eof",
            SPACED,
            r"printf 'a\nEND\nd\n' | xargs -E END rm -rf",
            Unproven,
        ),
        row(
            "xargs-arg-file",
            SPACED,
            "printf 'a\\n' > list; xargs -a list rm -rf",
            Unproven,
        ),
        row(
            "xargs-other",
            SPACED,
            r"printf 'a\n' | xargs -s 512 rm -rf",
            Unproven,
        ),
        row(
            "xargs-other",
            SPACED,
            r"printf 'a\n' | xargs -x rm -rf",
            Unproven,
        ),
        row(
            "xargs-unread-input",
            SPACED,
            "printf 'a\\n' > list; cat list | xargs rm -rf",
            Unproven,
        ),
        row(
            "xargs-unread-input",
            SPACED,
            "printf 'd\\n' > list; xargs rm -rf < list",
            Unproven,
        ),
        // GNU parallel.
        needing(
            row(
                "parallel-args",
                SPACED,
                "parallel rm -rf ::: a 'b c'",
                Exact,
            ),
            Needs::Parallel,
        ),
        needing(
            row(
                "parallel-stdin",
                SPACED,
                r"printf 'a\nb c\n' | parallel rm -rf",
                Exact,
            ),
            Needs::Parallel,
        ),
        needing(
            row(
                "parallel-stdin",
                SPACED,
                r"printf 'b c\0d\0' | parallel -0 rm -rf",
                Exact,
            ),
            Needs::Parallel,
        ),
        needing(
            row(
                "parallel-replace",
                SPACED,
                "parallel rm -rf {} ::: 'b c'",
                Exact,
            ),
            Needs::Parallel,
        ),
        needing(
            row("parallel-shell", SPACED, "parallel 'rm -rf a' ::: d", Exact),
            Needs::Parallel,
        ),
        needing(
            row(
                "parallel-replacement-strings",
                SPACED,
                "parallel rm -rf {.} ::: a.x",
                Unproven,
            ),
            Needs::Parallel,
        ),
        needing(
            row(
                "parallel-sources",
                SPACED,
                "parallel rm -rf {1}{2} ::: b ' c'",
                Unproven,
            ),
            Needs::Parallel,
        ),
        needing(
            row(
                "parallel-sources",
                SPACED,
                "parallel rm -rf ::: a ::: d",
                Unproven,
            ),
            Needs::Parallel,
        ),
        needing(
            row(
                "parallel-options",
                SPACED,
                "parallel -N 2 rm -rf ::: a d",
                Unproven,
            ),
            Needs::Parallel,
        ),
        // find. `guarded/` stands in for a protected directory (see
        // [`stand_in`]): a match may be it, or a link may lead to it.
        row(
            "find-print0",
            TREE,
            "find sub -print0 | xargs -0 rm -rf",
            Exact,
        ),
        row(
            "find-print0",
            TREE,
            "find sub/x -print0 | xargs -0 -I{} rm -rf {}",
            Exact,
        ),
        row(
            "find-print0",
            GUARDED_BELOW,
            "find sub -name guarded -print0 | xargs -0 rm -rf",
            Refused,
        ),
        row(
            "find-print0-consumer",
            LINKED,
            r#"find sub -print0 | xargs -0 sh -c 'for f; do rm -rf "$f"/x; done' _"#,
            Unproven,
        ),
        row(
            "find-print0-consumer",
            GUARDED_LINK,
            r#"find sub -name link -print0 | xargs -0 sh -c 'cd -P "$1" && rm -rf "$PWD"' _"#,
            Unproven,
        ),
        row(
            "find-print0-consumer",
            GUARDED_BELOW,
            "find sub -name guarded -print0 | xargs -0 perl -MFile::Path=remove_tree -e 'remove_tree(@ARGV)'",
            Unproven,
        ),
        row(
            "find-print0-consumer",
            GUARDED_BELOW,
            "find sub -name guarded -print0 | xargs -0 -I{} mv {} moved",
            Unproven,
        ),
        row(
            "find-print0-consumer",
            GUARDED_BELOW,
            "find sub -name guarded -print0 | xargs -0 command rm -rf",
            Unproven,
        ),
        needing(
            row(
                "find-print0-consumer",
                GUARDED_BELOW,
                "find sub -name guarded -print0 | parallel -0 perl -MFile::Path=remove_tree -e 'remove_tree(@ARGV)'",
                Unproven,
            ),
            Needs::Parallel,
        ),
        row(
            "find-print0-reader",
            TREE,
            "find sub -print0 | xargs -0 grep -l fixture",
            Exact,
        ),
        row(
            "find-print0-reader",
            TREE,
            "find sub -type f -print0 | xargs -0 wc -l",
            Exact,
        ),
        // Starts: options both tools read, `--` and BSD `-f`.
        row(
            "find-starts",
            GUARDED_BELOW,
            r"find -- sub -name guarded -exec rm -rf {} \;",
            Refused,
        ),
        row(
            "find-starts",
            GUARDED_BELOW,
            r"find -- sub/guarded -maxdepth 0 -exec rm -rf {} \;",
            Refused,
        ),
        needing(
            row(
                "find-starts",
                GUARDED_BELOW,
                r"find -f sub/guarded -- -exec rm -rf {} \;",
                Refused,
            ),
            Needs::BsdFind,
        ),
        row("find-starts", TREE, "find -P sub -name y -delete", Within),
        needing(
            row(
                "find-starts-other",
                GUARDED_BELOW,
                r"find -name guarded -exec rm -rf {} \;",
                Unproven,
            ),
            Needs::GnuFind,
        ),
        needing(
            row(
                "find-starts-other",
                GUARDED_BELOW,
                "find -dx sub -path '*guarded*' -delete",
                Unproven,
            ),
            Needs::BsdFind,
        ),
        needing(
            row(
                "find-starts-other",
                GUARDED_BELOW,
                "find -fsub -- -path '*guarded*' -delete",
                Unproven,
            ),
            Needs::BsdFind,
        ),
        row(
            "find-read",
            GUARDED_BELOW,
            r#"find sub -name guarded | while read f; do rm -rf "$f"; done"#,
            Unproven,
        ),
        row(
            "find-read",
            GUARDED_BELOW,
            r#"find sub -name guarded > list; while IFS= read -r f; do rm -rf "$f"; done < list"#,
            Unproven,
        ),
        row(
            "find-print-blanks",
            &["sub/", "sub/x y", "y", "keep"],
            "find sub | xargs rm -rf",
            Unproven,
        ),
        row("find-delete", TREE, "find sub -name y -delete", Within),
        row("find-delete", TREE, "find sub -delete", Exact),
        row(
            "find-delete",
            GUARDED_BELOW,
            "find sub -path '*guarded*' -delete",
            Refused,
        ),
        row(
            "find-exec",
            TREE,
            r"find sub -name x -exec rm -rf {} \;",
            Within,
        ),
        row(
            "find-exec",
            TREE,
            "find sub -name x -exec rm -rf {} +",
            Within,
        ),
        row(
            "find-exec",
            GUARDED_BELOW,
            r"find sub -name guarded -exec rm -rf {} \;",
            Refused,
        ),
        row(
            "find-exec",
            GUARDED_BELOW,
            "find sub -name guarded -exec rm -rf {} +",
            Refused,
        ),
        row(
            "find-execdir",
            TREE,
            r"find sub -name z -execdir rm -rf {} \;",
            Within,
        ),
        row(
            "find-execdir",
            GUARDED_BELOW,
            r"find sub -name guarded -execdir rm -rf {} \;",
            Refused,
        ),
        row(
            "find-exec-other",
            TREE,
            r#"find sub -name x -exec sh -c 'for f; do rm -rf "$f"; done' _ {} +"#,
            Unproven,
        ),
        row(
            "find-exec-other",
            TREE,
            r#"find sub -exec sh -c 'rm -rf "$2"' _ {} +"#,
            Unproven,
        ),
        row(
            "find-exec-other",
            LINKED,
            r"find sub -name link -exec rm -rf {}/x \;",
            Unproven,
        ),
        row(
            "find-exec-other",
            GUARDED_LINK,
            r#"find sub -name link -exec sh -c 'rm -rf "$1""/"' _ {} \;"#,
            Unproven,
        ),
        row(
            "find-exec-other",
            GUARDED_LINK,
            r#"find sub -name link -exec sh -c 'cd -P "$1" && rm -rf "$PWD"' _ {} \;"#,
            Unproven,
        ),
        row(
            "find-exec-other",
            GUARDED_LINK,
            r#"find sub -name link -execdir sh -c 'cd -P "$1" && rm -rf "$PWD"' _ {} \;"#,
            Unproven,
        ),
        row(
            "find-exec-other",
            GUARDED_LINK,
            r"find sub -name link -exec rm -rf + guarded \;",
            Unproven,
        ),
        row(
            "find-exec-other",
            TREE,
            r"find sub -name z -execdir rm -rf x \;",
            Unproven,
        ),
        // Round 02: a command that is not a plain remover reaches a
        // protected match.
        needing(
            row(
                "find-exec-other",
                GUARDED_BELOW,
                r"find sub -name guarded -exec python3 -c 'import shutil,sys; shutil.rmtree(sys.argv[1])' {} \;",
                Unproven,
            ),
            Needs::Python,
        ),
        row(
            "find-exec-other",
            GUARDED_BELOW,
            r"find sub -name guarded -exec perl -e 'use File::Path qw(remove_tree); remove_tree($ARGV[0])' {} \;",
            Unproven,
        ),
        row(
            "find-exec-other",
            GUARDED_BELOW,
            r"find sub -name guarded -exec mv {} moved \;",
            Unproven,
        ),
        row(
            "find-exec-other",
            GUARDED_BELOW,
            r"find sub -name guarded -exec env rm -rf {} \;",
            Unproven,
        ),
        row(
            "find-exec-other",
            GUARDED_BELOW,
            r"find sub -name guarded -exec /bin/rm -rf {} \;",
            Unproven,
        ),
        row(
            "find-exec-read",
            TREE,
            r"find sub -name y -exec grep -l fixture {} +",
            Exact,
        ),
        row(
            "find-exec-read",
            TREE,
            r"find sub -type f -execdir cat {} \;",
            Exact,
        ),
        row(
            "find-follow",
            &["sub/", "sub/link -> ../out", "out/", "out/x", "keep"],
            "find -L sub -name x -exec rm -rf {} +",
            Unproven,
        ),
        row(
            "find-other-output",
            TREE,
            r"find sub -maxdepth 0 -exec echo keep \; | xargs rm -rf",
            Unproven,
        ),
        // ls prints bare names, which the consumer reads from its own
        // directory.
        row(
            "ls-names",
            &["sub/", "sub/keep", "keep", "other"],
            "ls sub | xargs rm -rf",
            Within,
        ),
        // Separators: a backslash is a name character on Unix.
        row(
            "unix-backslash",
            &["a\\b", "a/", "a/b"],
            r"rm -rf 'a\b'",
            Exact,
        ),
        row(
            "unix-backslash",
            &["a\\b", "a/", "a/b"],
            r"printf 'a\\b\0' | xargs -0 rm -rf",
            Exact,
        ),
        row(
            "unix-backslash",
            &["a\\b", "a/", "a/b"],
            "rm -rf a/b",
            Exact,
        ),
    ]
}

/// Create the row's entries under `base`.
fn build(base: &Path, setup: &[&str]) {
    for entry in setup {
        if let Some((link, target)) = entry.split_once(" -> ") {
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, base.join(link)).unwrap();
            #[cfg(not(unix))]
            let _ = (link, target);
        } else if let Some(dir) = entry.strip_suffix('/') {
            std::fs::create_dir_all(base.join(dir)).unwrap();
        } else {
            std::fs::write(base.join(entry), b"fixture\n").unwrap();
        }
    }
}

/// Every entry below `base`, relative, without following links.
fn listing(base: &Path) -> BTreeSet<PathBuf> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeSet<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            out.insert(path.strip_prefix(base).unwrap().to_path_buf());
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                walk(base, &path, out);
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(base, base, &mut out);
    out
}

/// A judged path relative to `base`, with `.` and `..` folded; `None` for
/// a path outside it.
fn relative(base: &Path, judged: &str) -> Option<PathBuf> {
    let path = Path::new(judged);
    let path = if path.is_absolute() {
        path.strip_prefix(base).ok()?.to_path_buf()
    } else {
        path.to_path_buf()
    };
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(name) => out.push(name),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// What the guard's judged paths remove from `before`: each path and
/// everything below it. A final `*`, which the reader keeps for the
/// protected classification, stands for every entry of its directory
/// whose name does not start with a dot.
fn predicted(base: &Path, judged: &[String], before: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    let roots: Vec<PathBuf> = judged.iter().filter_map(|j| relative(base, j)).collect();
    let covers = |root: &PathBuf, path: &PathBuf| {
        if root.file_name().is_some_and(|name| name == "*") {
            let dir = root.parent().unwrap_or(Path::new(""));
            path.strip_prefix(dir).is_ok_and(|rest| {
                rest.components()
                    .next()
                    .is_some_and(|first| !first.as_os_str().to_string_lossy().starts_with('.'))
            })
        } else {
            path.starts_with(root)
        }
    };
    before
        .iter()
        .filter(|path| roots.iter().any(|root| covers(root, path)))
        .cloned()
        .collect()
}

/// Run `command` with `sh` in `base`.
fn run_native(base: &Path, command: &str) -> std::process::Output {
    Command::new("sh")
        .args(["-c", command])
        .current_dir(base)
        .env("LC_ALL", "C")
        .output()
        .unwrap()
}

/// A fresh fixture directory under the build's `target/` folder, so no
/// temp-root rule applies to the paths the rows delete.
fn fixture_dir() -> tempfile::TempDir {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&target).unwrap();
    tempfile::Builder::new()
        .prefix("deletion-native-")
        .tempdir_in(target)
        .unwrap()
}

/// Run one row; `Err` holds why it disagrees, `Ok(None)` a skip.
fn check(row: &Row, host: &Host) -> Result<Option<String>, String> {
    let dir = fixture_dir();
    let base = dir.path().canonicalize().unwrap();
    build(&base, row.setup);
    let before = listing(&base);
    let stand_in = row.setup.iter().find_map(|entry| {
        entry
            .strip_suffix("guarded/")
            .map(|parent| format!("{parent}guarded"))
    });
    let (found, judged) = judge(
        &row.command,
        &base,
        stand_in.as_ref().map(|path| base.join(path)).as_deref(),
    );
    match (row.expect, &found) {
        (Expect::Unproven, Some(found)) if found.unproven.is_some() => {}
        (Expect::Unproven, _) => return Err(format!("expected unproven, guard gave {found:?}")),
        (Expect::Refused, Some(found)) if found.unproven.is_none() => {}
        (Expect::Refused, _) => return Err(format!("expected refused, guard gave {found:?}")),
        (_, Some(found)) => return Err(format!("expected allowed, guard refused: {found:?}")),
        (_, None) => {}
    }
    if !host.has(row.needs) {
        return Ok(Some(format!("skipped: no {:?} on this host", row.needs)));
    }
    let output = run_native(&base, &row.command);
    let after = listing(&base);
    let removed: BTreeSet<PathBuf> = before.difference(&after).cloned().collect();
    if let Some(stand_in) = &stand_in {
        // A refused form with a stand-in is shown to reach it natively.
        if !removed.iter().any(|path| path.starts_with(stand_in)) {
            return Err(format!(
                "the native run did not reach the stand-in {stand_in}: removed {removed:?}; \
                 stderr {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    if matches!(row.expect, Expect::Unproven | Expect::Refused) {
        return Ok(None);
    }
    let predicted = predicted(&base, &judged, &before);
    let agrees = match row.expect {
        Expect::Exact => removed == predicted,
        _ => removed.is_subset(&predicted),
    };
    if agrees {
        Ok(None)
    } else {
        Err(format!(
            "native removed {removed:?}; guard judged {judged:?}, predicting {predicted:?}; \
             tool status {:?}, stderr {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// AC-1, AC-2: each grammar's native tool removes exactly what the guard
/// judged, or the guard refuses the grammar as unproven.
#[cfg(unix)]
#[test]
fn native_tools_remove_what_the_guard_judged() {
    let host = Host::probe();
    let mut failures = Vec::new();
    for row in rows() {
        match check(&row, &host) {
            Ok(None) => eprintln!("ok      {:<28} {:?}", row.grammar, row.command),
            Ok(Some(skip)) => eprintln!("skip    {:<28} {:?} ({skip})", row.grammar, row.command),
            Err(why) => {
                eprintln!("DIFFERS {:<28} {:?}: {why}", row.grammar, row.command);
                failures.push(format!("{}: {}: {why}", row.grammar, row.command));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Grammars whose native rows live in another fixture: the `sed` script
/// reader's own differential test, which runs native `sed` on each script
/// and compares the files it creates with the files the reader names.
const ELSEWHERE: &[(&str, &str, &str)] = &[
    (
        "sed-w",
        include_str!("sed_script.rs"),
        "fn the_files_native_sed_writes_are_the_files_the_reader_reports",
    ),
    (
        "sed-data",
        include_str!("sed_script.rs"),
        "fn the_files_native_sed_writes_are_the_files_the_reader_reports",
    ),
];

/// The grammar table in `deletion.rs`'s module documentation, as
/// `(key, status)` rows.
fn grammar_table() -> Vec<(String, String)> {
    include_str!("deletion.rs")
        .lines()
        .take_while(|line| line.starts_with("//!"))
        .filter_map(|line| {
            let cells: Vec<&str> = line
                .strip_prefix("//! |")?
                .split('|')
                .map(str::trim)
                .collect();
            let key = cells.first()?.strip_prefix('`')?.strip_suffix('`')?;
            let status = cells.get(2)?;
            Some((key.to_string(), (*status).to_string()))
        })
        .collect()
}

/// AC-3: every grammar the table lists as modelled or unproven has a
/// fixture row, an untested one has none, and every row names a listed
/// grammar.
#[test]
fn every_listed_grammar_has_a_fixture_row() {
    let table = grammar_table();
    assert!(
        table.len() > 20,
        "the grammar table did not parse: {table:?}"
    );
    let mut keys: BTreeSet<&str> = rows().iter().map(|row| row.grammar).collect();
    for (key, source, test) in ELSEWHERE {
        assert!(source.contains(test), "`{key}`: `{test}` is gone");
        keys.insert(key);
    }
    for (key, status) in &table {
        match status.as_str() {
            "modelled" | "unproven" => {
                assert!(
                    keys.contains(key.as_str()),
                    "`{key}` ({status}) has no fixture row"
                );
            }
            "untested" => {
                assert!(
                    !keys.contains(key.as_str()),
                    "`{key}` is untested but has a row"
                );
            }
            other => panic!("`{key}` has status `{other}`"),
        }
    }
    for key in keys {
        assert!(
            table.iter().any(|(listed, _)| listed == key),
            "fixture row `{key}` is not in the grammar table"
        );
    }
    for row in rows() {
        let status = &table.iter().find(|(key, _)| key == row.grammar).unwrap().1;
        assert_eq!(
            status == "unproven",
            row.expect == Expect::Unproven,
            "`{}` is {status} but its row expects {:?}: {}",
            row.grammar,
            row.expect,
            row.command
        );
    }
}
