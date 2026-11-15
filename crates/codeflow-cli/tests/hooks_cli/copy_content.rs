//! TSK-408: both shell guards judge a recursive `cp`, `rsync` or `ditto`
//! by the enforcement files it would create, replace or delete under its
//! destination, and a refused copy names the operand it rests on and its
//! role (the merged TSK-1187). Each command is fed to the guards as a hook
//! payload and never run.
use super::{codeflow, enforced_repo, git, guard_payload, line_allows, run_with_stdin, shell_path};
use std::path::{Path, PathBuf};
use std::process::Output;

const HOOKS: [&str; 2] = ["git-guard", "exec-guard"];

fn hook(name: &str, command: &str, session: &Path) -> Output {
    run_with_stdin(
        codeflow().args(["hook", name]).current_dir(session),
        &guard_payload(command, session),
    )
}

/// The session repository with a registered worktree, ordinary
/// directories, and source trees outside it: one carrying `.claude`, one
/// carrying `.git/hooks`, an empty one and a scratch destination.
struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    claude: String,
    hooks: String,
    empty: String,
    scratch: String,
    outside: String,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    enforced_repo(&root);
    git(
        &root,
        &[
            "worktree",
            "add",
            "-b",
            "task/w",
            &shell_path(&root.join(".worktrees/w")),
        ],
    );
    for dir in ["src", "docs", "out", "dist", "site", "build"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
        std::fs::write(root.join(dir).join("page.txt"), "x\n").unwrap();
    }
    let claude = tmp.path().join("planted-claude");
    std::fs::create_dir_all(claude.join(".claude")).unwrap();
    std::fs::write(claude.join(".claude/settings.json"), "{}\n").unwrap();
    let hooks = tmp.path().join("planted-hooks");
    std::fs::create_dir_all(hooks.join(".git/hooks")).unwrap();
    std::fs::write(hooks.join(".git/hooks/pre-commit"), "#!/bin/sh\n").unwrap();
    let empty = tmp.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let scratch = tmp.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    Fixture {
        claude: shell_path(&claude),
        hooks: shell_path(&hooks),
        empty: shell_path(&empty),
        scratch: shell_path(&scratch),
        outside: shell_path(tmp.path()),
        _tmp: tmp,
        root,
    }
}

/// Each command in `refused` is refused by both hooks under
/// `git.hook_integrity`, its message holding every text in `names`; each
/// in `allowed` is allowed by both. The failure lists every miss.
fn assert_both(session: &Path, refused: &[(String, Vec<String>)], allowed: &[String]) {
    let mut wrong = Vec::new();
    for name in HOOKS {
        for (command, names) in refused {
            let out = hook(name, command, session);
            let err = String::from_utf8_lossy(&out.stderr);
            if out.status.code() != Some(2) || !err.contains("git.hook_integrity") {
                wrong.push(format!("{name} should refuse: {command}\n{err}"));
            } else if let Some(missing) = names.iter().find(|n| !err.contains(n.as_str())) {
                wrong.push(format!("{name} should name `{missing}`: {command}\n{err}"));
            }
        }
        for command in allowed {
            let out = hook(name, command, session);
            if !line_allows(&out) {
                let err = String::from_utf8_lossy(&out.stderr);
                wrong.push(format!("{name} should allow: {command}\n{err}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

fn row(command: String, names: &[&str]) -> (String, Vec<String>) {
    (command, names.iter().map(|n| (*n).to_string()).collect())
}

/// AC-1: a recursive copy or sync that would create or replace an
/// enforcement file or a checkout's `.git` is refused by both hooks,
/// naming the file and its source.
#[test]
fn a_copy_carrying_enforcement_files_is_refused_by_both_guards() {
    let f = fixture();
    let (a, b) = (&f.claude, &f.hooks);
    let refused = vec![
        row(
            format!("cp -R {a}/.claude ."),
            &[".claude/settings.json", a],
        ),
        row(format!("cp -R {b}/.git ."), &[".git", b]),
        row(
            format!("cp -R {a}/ .worktrees/w"),
            &[".claude/settings.json", a],
        ),
        row(format!("cp -R {b}/ .worktrees/w"), &[".git", b]),
        row(format!("rsync -a {a}/ ./"), &[".claude/settings.json", a]),
        row(format!("rsync -a {b}/ ./"), &[".git", b]),
        row(format!("ditto {a} ."), &[".claude/settings.json", a]),
        row(format!("ditto {b} ."), &[".git", b]),
    ];
    assert_both(&f.root, &refused, &[]);
}

/// AC-2: a sync that would delete under a destination holding enforcement
/// files, by any deleting option, or remove a source that holds them, is
/// refused by both hooks naming the file.
#[test]
fn a_sync_deleting_or_removing_enforcement_files_is_refused_by_both_guards() {
    let f = fixture();
    let e = &f.empty;
    let s = &f.scratch;
    let mut refused: Vec<_> = [
        "--delete",
        "--delete-before",
        "--delete-during",
        "--delete-after",
        "--delete-delay",
        "--delete-excluded",
        "--delete-missing-args",
        "--del",
    ]
    .iter()
    .map(|flag| {
        row(
            format!("rsync -a {flag} {e}/ .worktrees/w/"),
            &[&format!("`rsync {flag}`"), ".worktrees/w"],
        )
    })
    .collect();
    // An ordinary folder holding the worktree's enforcement files only,
    // which the worktree removal check does not cover.
    refused.push(row(
        format!("rsync -a --del {e}/ .worktrees/w/.claude/"),
        &[".claude"],
    ));
    refused.push(row(
        format!("rsync -a --remove-source-files .claude/ {s}/dst/"),
        &["removes"],
    ));
    refused.push(row(
        format!("rsync -a --remove-source-files .git/hooks/ {s}/dst/"),
        &["removes"],
    ));
    assert_both(&f.root, &refused, &[]);
}

/// Review F1: a `-n` that may be the value of an option before it is no
/// dry run, so the deleting or planting sync is still refused by both
/// hooks, in the bundled and the separate form.
#[test]
fn a_dry_run_letter_that_may_be_an_option_value_is_judged_by_both_guards() {
    let f = fixture();
    let (a, b, e) = (&f.claude, &f.hooks, &f.empty);
    let mut refused = Vec::new();
    for options in ["-aen", "-ae -n", "-aMn", "-aM -n", "-aT -n", "-aTn"] {
        refused.push(row(
            format!("rsync {options} --delete {e}/ .worktrees/w/"),
            &[],
        ));
        refused.push(row(format!("rsync {options} {a}/ ./"), &[]));
        refused.push(row(format!("rsync {options} {b}/ ./"), &[]));
    }
    let allowed = vec![
        format!("rsync -an --delete {e}/ .worktrees/w/"),
        format!("rsync -a -T tmp -n {a}/ ./"),
    ];
    assert_both(&f.root, &refused, &allowed);
}

/// AC-3: a source the guard cannot read, copied into a place that holds
/// enforcement files, is refused as cannot-verify naming the source; a
/// run-time destination refuses a source that holds an enforcement name.
#[test]
fn a_source_the_guard_cannot_read_is_refused_by_both_guards() {
    let f = fixture();
    let o = &f.outside;
    let a = &f.claude;
    let unreadable = Path::new(o).join("unreadable");
    std::fs::create_dir_all(unreadable.join("inner")).unwrap();
    #[cfg(unix)]
    let _restore = super::RestorePermissions::deny(&unreadable);
    let mut refused = vec![
        row(format!("cp -R {o}/missing/ ./"), &["missing"]),
        row(
            format!("rsync -a {o}/planted-?laude/ ./"),
            &["planted-?laude"],
        ),
        row("rsync -a host:src/ ./".to_string(), &["host:src/"]),
        row("rsync -a rsync://h/m/ ./".to_string(), &["rsync://h/m/"]),
        row(format!("cp -R {a} \"$OUT\""), &["$OUT", ".claude"]),
    ];
    if cfg!(unix) {
        refused.push(row(format!("rsync -a {o}/unreadable/ ./"), &["unreadable"]));
    }
    assert_both(&f.root, &refused, &[]);
    // A source filled in at run time: git-guard names it; exec-guard's
    // strict text rule refuses the run-time word as well.
    let command = format!("cp -R \"$(echo {a})/\" ./");
    for name in HOOKS {
        let out = hook(name, &command, &f.root);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{name}: {err}");
        assert!(err.contains("git.hook_integrity"), "{name}: {err}");
    }
}

/// AC-3: a destination inside a configured worktree location too large to
/// search for hidden checkouts is refused as cannot-verify.
#[test]
fn a_destination_past_the_discovery_bound_is_refused_by_both_guards() {
    let f = fixture();
    let big = f.root.join(".worktrees/big");
    for n in 0..4100 {
        std::fs::create_dir_all(big.join(format!("d{n}"))).unwrap();
    }
    let p = shell_path(&f.root.join("docs"));
    let refused = vec![row(
        format!("rsync -a {p}/ .worktrees/big/"),
        &[".worktrees/big/", "4096"],
    )];
    assert_both(&f.root, &refused, &[]);
}

/// AC-4: ordinary copies and syncs keep passing, and a dry run of each
/// refused sync passes.
#[test]
fn ordinary_copies_and_dry_runs_keep_passing() {
    let f = fixture();
    let (a, b, e, s) = (&f.claude, &f.hooks, &f.empty, &f.scratch);
    let allowed: Vec<String> = vec![
        "cp -R src/ build/".into(),
        "rsync -a docs/ site/".into(),
        "ditto src build".into(),
        "rsync -a --delete out/ dist/".into(),
        format!("cp -R .claude {s}/backup"),
        format!("rsync -a .claude/ {s}/backup/"),
        "cp README.md notes.md".into(),
        format!("rsync -an {a}/ ./"),
        format!("rsync -a --dry-run {b}/ ./"),
        format!("rsync -an --delete {e}/ .worktrees/w/"),
        format!("rsync -a --dry-run --delete-after {e}/ .worktrees/w/"),
        format!("rsync -an --remove-source-files .claude/ {s}/dst/"),
    ];
    assert_both(&f.root, &[], &allowed);
}

/// AC-7 to AC-9: a git-guard refusal of a copy names the operand it rests
/// on and its role. The issue's repro: copying the CI workflow to a
/// literal scratch path passes; to a run-time path it stays refused, and
/// the message names the run-time destination and the workflow as its
/// source.
#[test]
fn a_copy_refusal_names_its_operand_and_role() {
    let f = fixture();
    let s = &f.scratch;
    let ci = ".github/workflows/codeflow-ci.yml";
    let refused = |command: &str, names: &[&str]| {
        let out = hook("git-guard", command, &f.root);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{command}: {err}");
        assert!(err.contains("git.hook_integrity"), "{command}: {err}");
        for name in names {
            assert!(err.contains(name), "{command}: missing `{name}`: {err}");
        }
    };
    let out = hook("git-guard", &format!("cp {ci} {s}/ci.yml"), &f.root);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The guard reads `TMPDIR` from its own environment, so the issue's
    // line names a scratch path and passes.
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "git-guard"])
            .current_dir(&f.root)
            .env("TMPDIR", s),
        &guard_payload(&format!("cp {ci} \"$TMPDIR/ci.yml\""), &f.root),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // A destination the guard cannot resolve stays refused, and the
    // message names it and the workflow as its source.
    refused(
        &format!("cp {ci} \"$OUT/ci.yml\""),
        &[
            "`$OUT/ci.yml`, which the guard cannot resolve",
            ci,
            "as its source",
        ],
    );
    // AC-8: a literal integrity destination stays refused as the
    // destination.
    refused(
        "cp README.md .codeflow/policy.json",
        &["`.codeflow/policy.json`", "as its destination"],
    );
    refused(
        &format!("cp {ci} \"$(mktemp)\""),
        &["which the guard cannot resolve", ci, "as its source"],
    );
    // AC-9: a source-only case and a destination case per verb.
    let policy = ".codeflow/policy.json";
    // `install` given a run-time word is refused first by the writer check
    // that names that word.
    refused(&format!("install {policy} \"$X\""), &["`\"$X\"`"]);
    for verb in ["install", "rsync", "ln -s"] {
        if verb != "install" {
            refused(
                &format!("{verb} {policy} \"$X\""),
                &[
                    "`$X`, which the guard cannot resolve",
                    policy,
                    "as its source",
                ],
            );
        }
        refused(
            &format!("{verb} README.md {policy}"),
            &[&format!("`{policy}`"), "as its destination"],
        );
    }
    refused(
        &format!("mv {policy} \"$X\""),
        &[&format!("removes `{policy}`"), "as its source"],
    );
    refused(
        &format!("mv README.md {policy}"),
        &[&format!("`{policy}`"), "as its destination"],
    );
    // Like `install`, `tee` given a run-time word is refused by the writer
    // check that names that word.
    refused(&format!("tee \"$X\" < {policy}"), &["`\"$X\"`"]);
    refused(&format!("tee {policy}"), &["`tee` targets", policy]);
    // A plain read of the workflow passes.
    let out = hook("git-guard", &format!("cat {ci}"), &f.root);
    assert_eq!(out.status.code(), Some(0));
}
