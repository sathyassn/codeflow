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
fn judge(command: &str, base: &Path) -> (Option<super::Found>, Vec<String>) {
    JUDGED.with(|judged| *judged.borrow_mut() = Some(Vec::new()));
    let found = super::read_deletion(command, Some(base), false);
    let paths = JUDGED.with(|judged| judged.borrow_mut().take().unwrap_or_default());
    (found, paths)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    Exact,
    Within,
    Unproven,
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

#[allow(clippy::too_many_lines)]
fn rows() -> Vec<Row> {
    use Expect::{Exact, Unproven, Within};
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
        // find.
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
            "find-print-blanks",
            &["sub/", "sub/x y", "y", "keep"],
            "find sub | xargs rm -rf",
            Unproven,
        ),
        row("find-delete", TREE, "find sub -name y -delete", Within),
        row("find-delete", TREE, "find sub -delete", Exact),
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
            TREE,
            r#"find sub -name x -exec sh -c 'for f; do rm -rf "$f"; done' _ {} +"#,
            Within,
        ),
        row(
            "find-exec-positionals",
            TREE,
            r#"find sub -exec sh -c 'rm -rf "$2"' _ {} +"#,
            Unproven,
        ),
        row(
            "find-path-building",
            LINKED,
            r"find sub -name link -exec rm -rf {}/x \;",
            Unproven,
        ),
        row(
            "find-path-building",
            LINKED,
            r#"find sub -name link -exec sh -c 'rm -rf "$1"/x' _ {} \;"#,
            Unproven,
        ),
        row(
            "find-path-building",
            LINKED,
            r#"find sub -print0 | xargs -0 sh -c 'for f; do rm -rf "$f"/x; done' _"#,
            Unproven,
        ),
        row(
            "find-execdir",
            TREE,
            r"find sub -name z -execdir rm -rf {} \;",
            Within,
        ),
        row(
            "find-execdir-relative",
            TREE,
            r"find sub -name z -execdir rm -rf x \;",
            Unproven,
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

/// Whether this host's `xargs` is GNU's: only GNU's answers `--version`.
fn gnu_xargs() -> bool {
    Command::new("xargs")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
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
fn check(row: &Row, gnu_xargs: bool, parallel: bool) -> Result<Option<String>, String> {
    let dir = fixture_dir();
    let base = dir.path().canonicalize().unwrap();
    build(&base, row.setup);
    let before = listing(&base);
    let (found, judged) = judge(&row.command, &base);
    match (row.expect, &found) {
        (Expect::Unproven, Some(found)) if found.unproven.is_some() => {}
        (Expect::Unproven, _) => return Err(format!("expected unproven, guard gave {found:?}")),
        (_, Some(found)) => return Err(format!("expected allowed, guard refused: {found:?}")),
        (_, None) => {}
    }
    let runs = match row.needs {
        Needs::Posix => true,
        Needs::GnuXargs => gnu_xargs,
        Needs::Parallel => parallel,
    };
    if !runs {
        return Ok(Some(format!("skipped: no {:?} on this host", row.needs)));
    }
    let output = run_native(&base, &row.command);
    let after = listing(&base);
    let removed: BTreeSet<PathBuf> = before.difference(&after).cloned().collect();
    if row.expect == Expect::Unproven {
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
    let gnu_xargs = gnu_xargs();
    let parallel = Command::new("parallel")
        .arg("--version")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("GNU parallel"));
    let mut failures = Vec::new();
    for row in rows() {
        match check(&row, gnu_xargs, parallel) {
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
