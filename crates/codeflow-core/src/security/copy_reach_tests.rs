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
    assert_eq!(base_name("/src/.claude/"), Some(Ok(".claude".to_string())));
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

/// Security review F1: a copy that follows links writes a linked
/// directory's contents, so under a run-time destination the walk reads
/// through each source link. The verdict's staging input: `stage/codeflow`
/// links to a directory holding `.claude/settings.json`.
#[cfg(unix)]
#[test]
fn a_run_time_destination_reads_through_source_links() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    repository(&root);
    let payload = tmp.path().join("payload");
    std::fs::create_dir_all(payload.join(".claude")).unwrap();
    std::fs::write(payload.join(".claude/settings.json"), "{}\n").unwrap();
    let stage = tmp.path().join("stage");
    std::fs::create_dir_all(&stage).unwrap();
    std::os::unix::fs::symlink(&payload, stage.join("codeflow")).unwrap();
    // The same link one level down.
    std::fs::create_dir_all(stage.join("a")).unwrap();
    std::os::unix::fs::symlink(&payload, stage.join("a/b")).unwrap();
    let staged = stage.display();
    for line in [
        format!("rsync -aL {staged}/ $W"),
        format!("cp -RL {staged}/ $W"),
        format!("scp -r {staged}/codeflow $W"),
        format!("scp -r {staged} $W"),
    ] {
        let found = judged(&line, &root).unwrap_or_else(|| panic!("{line}: passed"));
        assert!(found.contains("cannot resolve"), "{line}: {found}");
        assert!(found.contains(".claude"), "{line}: {found}");
    }
    // A link to a directory with no enforcement name keeps passing.
    let docs = tmp.path().join("docs");
    std::fs::create_dir_all(docs.join("guide")).unwrap();
    let plain = tmp.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    std::os::unix::fs::symlink(&docs, plain.join("docs")).unwrap();
    assert_eq!(
        judged(&format!("rsync -aL {}/ $W", plain.display()), &root),
        None
    );
}

/// Security review F1: a source link the walk cannot read, and a link
/// cycle, refuse under a run-time destination.
#[cfg(unix)]
#[test]
fn a_dangling_or_cyclic_source_link_refuses_a_run_time_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    repository(&root);
    let dangling = tmp.path().join("dangling");
    std::fs::create_dir_all(&dangling).unwrap();
    std::os::unix::fs::symlink(tmp.path().join("nowhere"), dangling.join("x")).unwrap();
    let line = format!("rsync -aL {}/ $W", dangling.display());
    let found = judged(&line, &root).unwrap_or_else(|| panic!("{line}: passed"));
    assert!(found.contains("cannot verify"), "{line}: {found}");
    assert!(found.contains("cannot read the link"), "{line}: {found}");
    let cycle = tmp.path().join("cycle");
    std::fs::create_dir_all(&cycle).unwrap();
    std::os::unix::fs::symlink(&cycle, cycle.join("again")).unwrap();
    // The walk ends either at WALK_LIMIT or where the system stops
    // resolving the nested links; both refuse.
    let line = format!("cp -RL {}/ $W", cycle.display());
    let found = judged(&line, &root).unwrap_or_else(|| panic!("{line}: passed"));
    assert!(found.contains("cannot verify"), "{line}: {found}");
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
    assert_eq!(compared(".CLAUDE"), Ok(".claude".to_string()));
    if cfg!(windows) {
        assert_eq!(compared(".claude."), Ok(".claude".to_string()));
        assert_eq!(compared(".Git "), Ok(".git".to_string()));
        assert!(compared("CLAUDE~1").is_err());
    } else {
        assert_eq!(compared(".claude."), Ok(".claude.".to_string()));
        assert_eq!(compared("CLAUDE~1"), Ok("claude~1".to_string()));
    }
    assert_eq!(compared("docs"), Ok("docs".to_string()));
}

/// Review N1: a filter rule or another documented value that starts with
/// a dash is the option's value, so an ordinary sync passes and a planted
/// one is still judged.
#[test]
fn a_value_that_starts_with_a_dash_stays_the_option_value() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    repository(&root);
    let plain = tmp.path().join("plain");
    std::fs::create_dir_all(plain.join("docs")).unwrap();
    let source = tmp.path().join("src");
    std::fs::create_dir_all(source.join(".claude")).unwrap();
    let p = plain.display();
    let s = source.display();
    // A bare `-@` is not read as an option at all (it fails closed as a
    // source the guard cannot read), so the window is given in a cluster.
    for options in ["-a -f -C", "-a -f -p", "-a@ -1", "-af -C"] {
        let ordinary = format!("rsync {options} {p}/ ./");
        assert_eq!(judged(&ordinary, &root), None, "{ordinary}");
        let planted = format!("rsync {options} {s}/ ./");
        assert!(judged(&planted, &root).is_some(), "{planted}");
    }
}

/// Review N2: a source's own name is compared as the filesystem opens it
/// under a run-time destination too.
#[test]
fn a_source_own_name_is_compared_as_the_filesystem_opens_it() {
    assert_eq!(enforcement_name(Some(".CLAUDE")), Ok(true));
    assert_eq!(enforcement_name(Some("docs")), Ok(false));
    assert_eq!(enforcement_name(None), Ok(false));
    if cfg!(windows) {
        assert_eq!(enforcement_name(Some(".claude.")), Ok(true));
        assert_eq!(enforcement_name(Some(".git ")), Ok(true));
        assert!(enforcement_name(Some("CLAUDE~1")).is_err());
    } else {
        assert_eq!(enforcement_name(Some(".claude.")), Ok(false));
        assert_eq!(enforcement_name(Some("CLAUDE~1")), Ok(false));
    }
}

/// Review F1 (round c): the last name of a source is read on this host's
/// separators, for its landing place and for its own name.
#[test]
fn a_source_name_is_read_on_the_host_separators() {
    let ok = |name: &str| Some(Ok(name.to_string()));
    assert_eq!(base_name("src/.claude"), ok(".claude"));
    assert_eq!(base_name("src/.claude/"), ok(".claude"));
    assert!(names_contents("src/"));
    assert!(names_contents("src/."));
    assert!(names_contents("."));
    assert!(!names_contents("src"));
    assert_eq!(Host::THIS.last_name("src/policy.json"), "policy.json");
    if cfg!(windows) {
        assert_eq!(Host::THIS.last_name("src\\policy.json"), "policy.json");
        assert_eq!(base_name("src\\.claude"), ok(".claude"));
        assert_eq!(base_name(".\\.claude."), ok(".claude"));
        assert!(names_contents("src\\"));
        assert!(names_contents("src\\."));
    } else {
        // Elsewhere a backslash is part of the name.
        assert_eq!(Host::THIS.last_name("src\\policy.json"), "src\\policy.json");
        assert_eq!(base_name("src\\.claude"), ok("src\\.claude"));
        assert!(!names_contents("src\\"));
    }
}

/// Review F1 and N2 (round c): the Windows reading of a source's last
/// name, checked on every host. A directory written with `\` is named by
/// its final component, for its landing place and for its own name.
#[test]
fn the_windows_reading_of_a_source_name_splits_on_both_separators() {
    let windows = WINDOWS_SEPARATORS;
    assert_eq!(last_name_on("src\\.claude", windows), ".claude");
    assert_eq!(last_name_on("src\\.claude\\", windows), ".claude");
    assert_eq!(last_name_on(".\\.claude.", windows), ".claude.");
    assert_eq!(
        last_name_on("C:\\work\\src/.codeflow", windows),
        ".codeflow"
    );
    assert_eq!(last_name_on("src\\policy.json", windows), "policy.json");
    assert!(names_contents_on("src\\", windows));
    assert!(names_contents_on("src\\.", windows));
    assert!(!names_contents_on("src\\.claude", windows));
    // The name Win32 opens: the trailing dot is dropped, so `.claude.` is
    // `.claude` once its parent written with `\` is split off.
    assert_eq!(
        Host::WINDOWS.entry_name(".\\.claude."),
        Some(Ok(".claude".to_string()))
    );
}

/// Review F2 (round e): every entry or landing name goes through one
/// reading, `Host::opened`. On Windows a named stream opens the file before
/// its first `:`, stream type syntax and a short name cannot be mapped, and
/// a drive-relative word is named after its drive. Checked on every host.
#[test]
fn an_entry_name_is_read_as_windows_opens_it_on_every_host() {
    let w = Host::WINDOWS;
    let ok = |name: &str| Some(Ok(name.to_string()));
    for (source, name) in [
        ("src\\.claude:notes", ".claude"),
        ("src/.claude:notes", ".claude"),
        ("src\\.claude. ", ".claude"),
        ("payload\\config:notes", "config"),
        ("payload\\config.:x", "config"),
        ("C:.claude", ".claude"),
        ("C:\\work\\.codeflow", ".codeflow"),
        ("plain", "plain"),
    ] {
        assert_eq!(w.entry_name(source), ok(name), "{source}");
    }
    for source in [
        "payload\\config::$DATA",
        "src\\.claude::$INDEX_ALLOCATION",
        "src\\.claude:$I30:$INDEX_ALLOCATION",
        "src\\:notes",
        "src\\CLAUDE~1",
        "src\\...",
    ] {
        assert!(
            matches!(w.entry_name(source), Some(Err(_))),
            "{source}: {:?}",
            w.entry_name(source)
        );
    }
    assert_eq!(w.entry_name("C:"), None);
    assert_eq!(w.entry_name("src\\."), None);
    assert_eq!(w.compared(".Claude:notes"), Ok(".claude".to_string()));
    // Elsewhere `\`, `:` and a trailing dot are parts of a name.
    let p = Host::POSIX;
    assert_eq!(
        p.entry_name("payload\\config:notes"),
        ok("payload\\config:notes")
    );
    assert_eq!(p.entry_name("src/.claude."), ok(".claude."));
    assert_eq!(p.entry_name("C:.claude"), ok("C:.claude"));
    assert_eq!(p.entry_name("src/a::$DATA"), ok("a::$DATA"));
}

/// Review F1 (round c): on Windows a source written with backslashes
/// lands at its last name, so a copy into the checkout root is refused,
/// naming `.claude`.
#[cfg(windows)]
#[test]
fn a_backslash_source_into_the_checkout_is_refused_on_windows() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    repository(&root);
    std::fs::create_dir_all(root.join("src/.claude")).unwrap();
    std::fs::write(root.join("src/.claude/settings.json"), "{}").unwrap();
    for line in [
        "cp -R src\\.claude .",
        "rsync -a src\\.claude ./",
        "cp -R .\\.claude. .",
        "rsync -a .\\.claude. ./",
    ] {
        let found = judged(line, &root).expect(line);
        // The enforcement path named, after the source operand.
        let named = found
            .rsplit_once("enforcement path")
            .map_or("", |(_, rest)| rest);
        assert!(named.contains(".claude"), "{line}: {found}");
    }
    // Under a destination filled in at run time, the directory's own name
    // is read on both separators (N2).
    let plain = root.join("plain/.claude.");
    std::fs::create_dir_all(&plain).unwrap();
    for line in [
        "cp -R plain\\.claude. $OUT",
        "rsync -a plain\\.claude. $OUT",
    ] {
        let found = judged(line, &root).expect(line);
        assert!(found.contains("enforcement entry"), "{line}: {found}");
    }
}
