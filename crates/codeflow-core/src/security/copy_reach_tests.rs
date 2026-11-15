//! TSK-408: the recursive copy and sync reader.
use super::*;
use crate::security::unresolved::copy_read;
use std::path::Path;

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

/// A repository at `root` whose enforcement set is the default one.
fn repository(root: &Path) {
    git2::Repository::init(root).unwrap();
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(root.join(".claude/settings.json"), "{}\n").unwrap();
}

/// Judge `line` (program first) from `cwd`, with `root` as the session.
fn judged(line: &str, cwd: &Path) -> Option<String> {
    let all = words(line);
    let copy = copy_read(&all[0], &all[1..]).unwrap();
    let resolve = |word: &str| (!word.contains('$')).then(|| cwd.join(word));
    let roots = [cwd];
    let place = Place {
        resolve: &resolve,
        roots: &roots,
    };
    judge(&all[0], &copy, &place)
}

#[test]
fn rsync_options_are_read_with_their_aliases_and_abbreviations() {
    for deleting in [
        "--del",
        "--dele",
        "--delete",
        "--delete-before",
        "--delete-during",
        "--delete-after",
        "--delete-delay",
        "--delete-excluded",
        "--delete-missing-args",
    ] {
        assert!(rsync_deletes(deleting), "{deleting}");
    }
    for other in ["--delay-updates", "--de", "-d", "--max-delete", "--dry-run"] {
        assert!(!rsync_deletes(other), "{other}");
    }
    for removing in [
        "--remove-source-files",
        "--remove-sent-files",
        "--remove-source",
        "--remove-s",
    ] {
        assert!(rsync_removes_source(removing), "{removing}");
    }
    assert!(!rsync_removes_source("--recursive"));
}

#[test]
fn only_recursive_copies_are_read() {
    let flags = |line: &str| words(line);
    assert!(recursive("cp", &flags("-R")));
    assert!(recursive("cp", &flags("-a")));
    assert!(recursive("cp", &flags("--recursive")));
    assert!(recursive("cp", &flags("--arch")));
    assert!(!recursive("cp", &flags("-p")));
    assert!(recursive("rsync", &[]));
    assert!(recursive("ditto", &[]));
    assert!(recursive("scp", &flags("-r")));
    assert!(!recursive("mv", &flags("-f")));
}

#[test]
fn remote_and_pattern_spellings_are_recognised() {
    assert!(remote("host:src/"));
    assert!(remote("user@host:src"));
    assert!(remote("rsync://h/m/"));
    assert!(!remote("/src/a:b"));
    assert!(!remote("./a"));
    assert!(pattern("/src/s*/"));
    assert!(!pattern("/src/plain/"));
    assert_eq!(base_name("/src/.claude/"), Some(".claude"));
    assert_eq!(base_name("."), None);
    assert_eq!(base_name("/"), None);
}

#[test]
fn a_source_holding_an_enforcement_name_refuses_and_an_ordinary_one_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    repository(&root);
    let source = tmp.path().join("src");
    std::fs::create_dir_all(source.join(".claude")).unwrap();
    std::fs::write(source.join(".claude/settings.json"), "{}").unwrap();
    let plain = tmp.path().join("plain");
    std::fs::create_dir_all(plain.join("docs")).unwrap();
    let s = source.display();
    let p = plain.display();

    let found = judged(&format!("rsync -a {s}/ ./"), &root).expect("refused");
    assert!(found.contains(".claude/settings.json"), "{found}");
    assert!(found.contains(&format!("{s}/")), "{found}");
    assert!(judged(&format!("cp -R {s}/.claude ."), &root).is_some());
    assert!(judged(&format!("ditto {s} ."), &root).is_some());
    // A case-different name lands on the same path on a case-insensitive
    // destination.
    let upper = tmp.path().join("upper");
    std::fs::create_dir_all(upper.join(".CLAUDE")).unwrap();
    assert!(judged(&format!("rsync -a {}/ ./", upper.display()), &root).is_some());

    assert_eq!(judged(&format!("rsync -a {p}/ ./"), &root), None);
    assert_eq!(judged(&format!("cp -R {p} build"), &root), None);
    assert_eq!(judged(&format!("rsync -an {s}/ ./"), &root), None);
    assert_eq!(judged(&format!("rsync -a --dry-run {s}/ ./"), &root), None);
    // A plain file copy keeps the destination-word verdict.
    assert_eq!(
        judged(&format!("cp {s}/.claude/settings.json x.json"), &root),
        None
    );
}

#[test]
fn a_deleting_sync_refuses_where_its_destination_holds_enforcement_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    repository(&root);
    std::fs::create_dir_all(root.join("out")).unwrap();
    std::fs::create_dir_all(root.join("dist")).unwrap();
    let empty = tmp.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let e = empty.display();
    for flag in [
        "--delete",
        "--del",
        "--delete-delay",
        "--delete-missing-args",
    ] {
        let found = judged(&format!("rsync -a {flag} {e}/ ./"), &root).expect(flag);
        assert!(found.contains(flag), "{found}");
    }
    assert_eq!(judged("rsync -a --delete out/ dist/", &root), None);
}

#[test]
fn a_source_the_guard_cannot_read_refuses_only_where_it_matters() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    repository(&root);
    std::fs::create_dir_all(root.join("build")).unwrap();
    let missing = tmp.path().join("missing");
    let m = missing.display();
    let found = judged(&format!("rsync -a {m}/ ./"), &root).expect("refused");
    assert!(found.contains("cannot verify"), "{found}");
    assert!(judged("rsync -a host:src/ ./", &root).is_some());
    assert!(judged("rsync -a rsync://h/m/ ./", &root).is_some());
    assert!(judged(&format!("rsync -a {}/s*/ ./", tmp.path().display()), &root).is_some());
    // The same source into a place that holds no enforcement path keeps
    // its verdict.
    assert_eq!(judged(&format!("rsync -a {m}/ build/"), &root), None);
}

#[test]
fn a_run_time_destination_refuses_a_source_holding_an_enforcement_name() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    repository(&root);
    let source = tmp.path().join("src");
    std::fs::create_dir_all(source.join("deep/er/.git/hooks")).unwrap();
    let plain = tmp.path().join("plain");
    std::fs::create_dir_all(plain.join("docs")).unwrap();
    let found = judged(&format!("cp -R {} $OUT", source.display()), &root).expect("refused");
    assert!(found.contains("cannot resolve"), "{found}");
    assert!(found.contains(".git"), "{found}");
    assert_eq!(
        judged(&format!("cp -R {} $OUT", plain.display()), &root),
        None
    );
}

#[cfg(unix)]
#[test]
fn a_link_at_the_landing_place_into_enforcement_files_refuses() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    repository(&root);
    std::os::unix::fs::symlink(root.join(".claude"), root.join("x")).unwrap();
    std::os::unix::fs::symlink(root.join("docs-real"), root.join("y")).unwrap();
    std::fs::create_dir_all(root.join("docs-real")).unwrap();
    let source = tmp.path().join("src");
    std::fs::create_dir_all(source.join("x")).unwrap();
    std::fs::write(source.join("x/settings.json"), "{}").unwrap();
    let found = judged(&format!("cp -R {}/ ./", source.display()), &root).expect("refused");
    assert!(found.contains("through the link"), "{found}");
    let ordinary = tmp.path().join("ordinary");
    std::fs::create_dir_all(ordinary.join("y")).unwrap();
    assert_eq!(
        judged(&format!("cp -R {}/ ./", ordinary.display()), &root),
        None
    );
}

/// Review F1: an option the reader does not know may take a value, and a
/// known option that takes one takes the next word, so a later `-n` is no
/// dry run there.
#[test]
fn a_dry_run_letter_that_may_be_an_option_value_is_no_dry_run() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    repository(&root);
    let empty = tmp.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let source = tmp.path().join("src");
    std::fs::create_dir_all(source.join(".claude")).unwrap();
    let e = empty.display();
    let s = source.display();
    for options in ["-aen", "-ae -n", "-aMn", "-aM -n", "-aT -n", "-aTn"] {
        let deleting = format!("rsync {options} --delete {e}/ ./");
        assert!(judged(&deleting, &root).is_some(), "{deleting}");
        let planted = format!("rsync {options} {s}/ ./");
        assert!(judged(&planted, &root).is_some(), "{planted}");
    }
    assert_eq!(judged(&format!("rsync -an --delete {e}/ ./"), &root), None);
    assert_eq!(judged(&format!("rsync -aT tmp -n {s}/ ./"), &root), None);
}

/// Review F2: names are compared as Windows opens them.
#[test]
fn names_are_compared_as_the_filesystem_opens_them() {
    assert_eq!(compared(".CLAUDE"), ".claude");
    if cfg!(windows) {
        assert_eq!(compared(".claude."), ".claude");
        assert_eq!(compared(".Git "), ".git");
        assert!(short_alias("CLAUDE~1"));
    } else {
        assert_eq!(compared(".claude."), ".claude.");
        assert!(!short_alias("CLAUDE~1"));
    }
    assert!(!short_alias("docs"));
}
