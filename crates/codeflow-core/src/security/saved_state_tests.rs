//! The saved-state pass and inventory (TSK-1461): the whole-corpus
//! differential first, then the scope table, the controls, the fallback rows
//! and the refusals the pass adds.

use super::*;
use crate::hooks::policy::PolicyLevel;
use crate::portable_path::slashed;
use crate::security::startup::{class_target, evaluate as startup_evaluate, RULE};

/// A home and a project in a temporary folder.
struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let project = dir.path().join("project");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        Self {
            _dir: dir,
            home,
            project,
        }
    }

    /// The class locations for this fixture's home.
    fn locations(&self) -> StartupEnv {
        StartupEnv {
            home: Some(self.home.clone()),
            ..StartupEnv::default()
        }
    }

    /// The pass's messages for `command` run in the project.
    fn pass(&self, command: &str) -> Vec<String> {
        self.pass_in(command, &self.project, &self.locations())
    }

    fn pass_in(&self, command: &str, cwd: &Path, env: &StartupEnv) -> Vec<String> {
        evaluate(command, cwd, env)
            .into_iter()
            .map(|finding| {
                assert_eq!(finding.rule, RULE, "{command}");
                assert!(finding.level_fixed, "{command}");
                finding.message
            })
            .collect()
    }

    /// What the exec guard's startup checks find: the class's readers, then
    /// the pass.
    fn judge(&self, command: &str) -> Vec<Violation> {
        judge_in(command, &self.project, &self.locations())
    }

    fn mkdir(&self, path: &Path) {
        std::fs::create_dir_all(path).unwrap();
    }
}

fn judge_in(command: &str, cwd: &Path, env: &StartupEnv) -> Vec<Violation> {
    let mut found = startup_evaluate(command, cwd, env);
    found.extend(evaluate(command, cwd, env));
    found
}

fn refused(found: &[Violation]) -> bool {
    !found.is_empty()
}

// ---------------------------------------------------------------------------
// The whole-corpus differential
// ---------------------------------------------------------------------------

/// Every command of the guard corpus: the forms `guard_forms` pins and the
/// text-guard invariant's payloads, with the directory each runs in.
fn corpus(t: &Path) -> Vec<(String, PathBuf)> {
    use crate::security::guard_forms as forms;
    let repo = t.join("repo");
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let mut add = |command: &str| out.push((command.to_string(), repo.clone()));
    for (a, b) in forms::COMPOSED_PAIRS
        .iter()
        .chain(forms::REVIEW_SYMLINK_PROBES)
        .chain(forms::PACKAGE_RUNNER_PAIRS)
        .chain(forms::HELP_PAIRS)
    {
        add(a);
        add(b);
    }
    for command in forms::ROOTED_DELETIONS
        .iter()
        .chain(forms::RUN_TIME_PROJECT_DELETIONS)
        .chain(forms::PROJECT_DELETIONS)
        .chain(forms::NESTINGS)
        .chain(forms::SPACED_INPUT_FORMS)
    {
        add(command);
    }
    for (_, command, _) in forms::REVIEW_PROBES {
        add(command);
    }
    for (_, command, _) in forms::REVIEW_ROUND_TWO_PROBES
        .iter()
        .chain(forms::REVIEW_ROUND_THREE_PROBES)
        .chain(forms::REVIEW_ROUND_FOUR_PROBES)
    {
        add(command);
    }
    for (command, _) in forms::XARGS_OPTION_FORMS {
        add(command);
    }
    let rows: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(invariant().join("rows.json")).unwrap())
            .unwrap();
    let root = slashed(t);
    for row in rows.as_array().unwrap() {
        let command = row["command"].as_str().unwrap().replace("@T@", &root);
        let cwd = row["cwd"].as_str().unwrap().replace("@T@", &root);
        out.push((command, PathBuf::from(cwd)));
    }
    out
}

fn invariant() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codeflow-cli/tests/fixtures/text-guard-invariant")
}

/// Rebuild the invariant's fixture tree under `t`: its folders, files and
/// links, without a repository.
fn invariant_tree(t: &Path) {
    let tree: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(invariant().join("tree.json")).unwrap())
            .unwrap();
    std::fs::create_dir_all(t.join("repo")).unwrap();
    std::fs::create_dir_all(t.join("home")).unwrap();
    for item in tree.as_array().unwrap() {
        let at = t.join(item["path"].as_str().unwrap());
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        match item["kind"].as_str().unwrap() {
            "dir" => std::fs::create_dir_all(&at).unwrap(),
            "file" => std::fs::write(&at, item["text"].as_str().unwrap_or_default()).unwrap(),
            "link" => {
                #[cfg(unix)]
                std::os::unix::fs::symlink(item["target"].as_str().unwrap(), &at).unwrap();
            }
            _ => {}
        }
    }
}

/// A finding as the differential compares it.
fn shown_findings(found: &[Violation]) -> Vec<String> {
    found
        .iter()
        .map(|v| format!("{} {:?} {}", v.rule, v.level, v.message))
        .collect()
}

/// TSK-1461 class proof: over the whole corpus, both hooks, both outcomes,
/// the findings with the saved-state work turned off are the guard as it
/// stood before, and turning it on keeps that list as it is, in order, and
/// adds only saved-state findings after it.
#[test]
fn saved_state_findings_follow_every_other_finding_unchanged() {
    use crate::hooks::sandbox_state::Outcome;
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path();
    invariant_tree(t);
    let rows = corpus(t);
    assert!(rows.len() > 2500, "corpus has {} rows", rows.len());
    let levels = crate::hooks::policy::SecuritySection::default();
    let git_policy = crate::hooks::policy::GitPolicy::default();
    let outcomes = [
        Outcome::Relaxed,
        Outcome::Strict("unsandboxed retry".into()),
    ];
    let chunks: Vec<&[(String, PathBuf)]> = rows.chunks(rows.len().div_ceil(4)).collect();
    let problems: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                let levels = &levels;
                let git_policy = &git_policy;
                let outcomes = &outcomes;
                scope.spawn(move || {
                    let mut problems = Vec::new();
                    for (command, cwd) in chunk {
                        for outcome in outcomes {
                            let judge = || {
                                crate::hooks::exec_guard::evaluate_ruled(
                                    command,
                                    true,
                                    levels,
                                    PolicyLevel::Block,
                                    cwd,
                                    cwd,
                                    outcome,
                                )
                            };
                            let off = shown_findings(&without(judge));
                            let on = shown_findings(&judge());
                            let extra_rule = on
                                .get(off.len()..)
                                .unwrap_or_default()
                                .iter()
                                .all(|f| f.starts_with(&format!("{RULE} ")));
                            if on.get(..off.len()) != Some(&off[..]) || !extra_rule {
                                problems.push(format!(
                                    "exec-guard {outcome:?} {command}\n  off: {off:?}\n  on:  {on:?}"
                                ));
                            } else if on.len() > off.len() {
                                // Shown with `--nocapture`: each row the
                                // pass adds a refusal to.
                                eprintln!(
                                    "added {outcome:?} {command}: {:?} (before: {})",
                                    &on[off.len()..],
                                    off.len()
                                );
                            }
                        }
                        let context = crate::hooks::git_guard::GuardContext {
                            policy: git_policy,
                            current_branch: "task/x",
                            integrate_token: false,
                            pr_base_lookup: None,
                            dir_target_lookup: None,
                            alias_lookup: None,
                            branch_lookup: None,
                            discard_lookup: Some(&|_| Ok(None)),
                            root_checkout: None,
                        };
                        let judge = || {
                            crate::hooks::git_guard::evaluate_report_at(command, &context, cwd)
                                .violations
                        };
                        let off = shown_findings(&without(judge));
                        let on = shown_findings(&judge());
                        if on != off {
                            problems.push(format!(
                                "git-guard {command}\n  off: {off:?}\n  on:  {on:?}"
                            ));
                        }
                    }
                    problems
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect()
    });
    assert!(
        problems.is_empty(),
        "{} rows changed:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Scope: a target is judged where the command that names it runs
// ---------------------------------------------------------------------------

/// Each operation the pass judges, with a relative operand, run from each
/// directory-change form. `D` holds saved state below `x`, a
/// `session-env` folder and a source tree; the project holds none of
/// them. A form whose directory change ends before the operation adds no
/// finding; a form whose operation runs in `D` refuses with the operation's
/// own message.
#[test]
fn a_target_is_judged_in_the_directory_its_command_runs_in() {
    let f = Fixture::new();
    let d = f.project.join("D");
    f.mkdir(&d.join("x/session-env"));
    f.mkdir(&d.join("session-env"));
    std::fs::write(d.join("session-env/f"), "x").unwrap();
    f.mkdir(&d.join("src"));
    let x = slashed(&d.join("x"));
    let operations: Vec<(&str, String)> = vec![
        (
            "rm -rf x",
            format!("`rm` removes `x`, which is or holds saved shell state: `{x}`"),
        ),
        (
            "rmdir session-env",
            "`rmdir` removes `session-env`, which is saved shell state: `session-env`".into(),
        ),
        (
            "unlink session-env/f",
            "`unlink` removes `session-env/f`, which is saved shell state: `session-env`".into(),
        ),
        (
            "find x -delete",
            format!("`find` removes `x`, which is or holds saved shell state: `{x}`"),
        ),
        (
            "cp -r src x",
            format!("`cp` places files into `x`, which holds saved shell state below it: `{x}`"),
        ),
        (
            "mv x y",
            format!("`mv` moves `x`, which is or holds saved shell state: `{x}`"),
        ),
        (
            "tar -xf a.tar -C x",
            format!("`tar` places files into `x`, which holds saved shell state below it: `{x}`"),
        ),
        (
            "rsync -a src/ x",
            format!("`rsync` places files into `x`, which holds saved shell state below it: `{x}`"),
        ),
    ];
    let unbound = [
        "(cd D; pwd); OP",
        "cd D | cat; OP",
        "p=$(cd D; pwd); OP",
        "sh -c 'cd D'; OP",
        "env -C D true; OP",
        "pushd D; popd; OP",
        "OP; cd D",
    ];
    let bound = [
        "cd D && OP",
        "{ cd D; }; OP",
        "(cd D && OP)",
        "env -C D OP",
        "sh -c 'cd D; OP'",
        "pushd D; OP",
    ];
    let mut wrong = Vec::new();
    for (op, message) in &operations {
        for form in unbound {
            let command = form.replace("OP", op);
            let found = f.pass(&command);
            if !found.is_empty() {
                wrong.push(format!("refused: {command}: {found:?}"));
            }
        }
        for form in bound {
            let command = form.replace("OP", op);
            let found = f.pass(&command);
            if found != [message.clone()] {
                wrong.push(format!("{command}: {found:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// A command run from several directories refuses only when it refuses in
/// every one of them; a directory or operand the reader cannot resolve
/// leaves the other checks' judgment as it is.
#[test]
fn a_target_the_reader_cannot_bind_exactly_adds_no_finding() {
    let f = Fixture::new();
    let d = f.home.join("D");
    f.mkdir(&d.join("x/session-env"));
    std::fs::write(f.project.join("README"), "x").unwrap();
    let to_d = slashed(&d);
    for command in [
        "cd \"$X\"; rm -rf *".to_string(),
        "rm -rf \"$X\"".to_string(),
        "eval \"$Y\"; rm -rf *".to_string(),
        format!("if c; then cd {to_d}; fi; rm -rf x"),
        format!("for p in a {to_d}/x; do rm -rf \"$p\"; done"),
        format!("pushd {to_d}; popd; rm -rf x"),
        "rm -rf \"$UNSET\"/x".to_string(),
    ] {
        assert_eq!(f.pass(&command), Vec::<String>::new(), "{command}");
        assert_eq!(
            shown_findings(&f.judge(&command)),
            shown_findings(&without(|| f.judge(&command))),
            "{command}"
        );
    }
    // Every reading refuses: both directories hold saved state at `x`.
    let e = f.home.join("E");
    f.mkdir(&e.join("x/session-env"));
    let command = format!(
        "if c; then cd {to_d}; else cd {}; fi; rm -rf x",
        slashed(&e)
    );
    assert_eq!(f.pass(&command).len(), 2, "{command}");
}

/// The assignment forms a prefix may take do not hide the program after
/// them.
#[test]
fn assignment_prefixes_keep_the_program_after_them() {
    let f = Fixture::new();
    f.mkdir(&f.project.join("D/x/session-env"));
    let x = slashed(&f.project.join("D/x"));
    for prefix in ["X=1", "X+=1", "a[0]=1"] {
        let command = format!("cd D && {prefix} rm -rf x");
        assert_eq!(
            f.pass(&command),
            [format!(
                "`rm` removes `x`, which is or holds saved shell state: `{x}`"
            )],
            "{command}"
        );
    }
}

/// Lines that ran before the pass existed still run in an ordinary
/// project: a directory change that ends before the removal, an extraction
/// or copy after a subshell visits a large tree, and a removal after the
/// directory stack returns.
#[test]
fn ordinary_lines_after_a_directory_change_still_run() {
    use crate::hooks::sandbox_state::Outcome;
    let f = Fixture::new();
    std::fs::write(f.project.join("README"), "x").unwrap();
    f.mkdir(&f.project.join("a"));
    f.mkdir(&f.project.join("build/out"));
    let levels = crate::hooks::policy::SecuritySection::default();
    for command in [
        "(cd /; pwd); rm -rf *",
        "(cd /usr; pwd); tar xf a.tar",
        "(cd /usr; ls); cp -r a b",
        "pushd /usr; popd; rm -rf build",
    ] {
        assert_eq!(f.pass(command), Vec::<String>::new(), "{command}");
        for outcome in [
            Outcome::Relaxed,
            Outcome::Strict("unsandboxed retry".into()),
        ] {
            let found = crate::hooks::exec_guard::evaluate_ruled(
                command,
                true,
                &levels,
                PolicyLevel::Block,
                &f.project,
                &f.project,
                &outcome,
            );
            assert!(
                !found.iter().any(|v| v.rule == RULE),
                "{command}: {found:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The inventory: writes the class's readers refuse
// ---------------------------------------------------------------------------

/// The shell state a harness or terminal tool saves for its later commands
/// is in the class. Every write kind refuses, in every naming form the
/// guard reads.
#[test]
fn saved_shell_state_writes_refuse() {
    let f = Fixture::new();
    for dir in [
        ".claude/session-env/abc",
        ".claude/shell-snapshots",
        ".codex/shell_snapshots",
        ".config/herdr-core/templates",
    ] {
        f.mkdir(&f.home.join(dir));
    }
    let home = slashed(&f.home);
    let mut wrong = Vec::new();
    for target in [
        ".claude/session-env/abc/sessionstart-hook-0.sh",
        ".claude/session-env/new.sh",
        ".claude/shell-snapshots/snapshot-zsh-1.sh",
        ".codex/shell_snapshots/s.1.sh",
        ".config/herdr-core/templates/build.toml",
    ] {
        for spelled in [
            format!("~/{target}"),
            format!("$HOME/{target}"),
            format!("{home}/{target}"),
        ] {
            for command in [
                format!("echo 'export PATH=/tmp/x:$PATH' >> {spelled}"),
                format!("printf x > {spelled}"),
                format!("tee -a {spelled} < /dev/null"),
                format!("cp evil.sh {spelled}"),
                format!("mv evil.sh {spelled}"),
                format!("ln -sf /tmp/evil.sh {spelled}"),
            ] {
                if !refused(&f.judge(&command)) {
                    wrong.push(command);
                }
            }
        }
    }
    for command in [
        "cd ~/.claude && echo x >> session-env/abc/hook.sh",
        "cd ~/.claude/session-env/abc && printf x > hook.sh",
        "cd ~/.codex/shell_snapshots && cp /tmp/evil.sh s.sh",
        "cd ~/.claude && tar -xf state.tar",
        "tar -xf state.tar -C ~/.codex",
        "unzip -o t.zip -d ~/.config/herdr-core",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(command.to_string());
        }
    }
    assert!(wrong.is_empty(), "not refused:\n{}", wrong.join("\n"));
    // With the work turned off, the same writes are not in the class.
    assert!(
        without(|| class_target(&f.home.join(".claude/session-env/new.sh"), &f.locations()))
            .is_none()
    );
}

/// `CLAUDE_CONFIG_DIR` and `CODEX_HOME` move the saved state with them,
/// the default folders stay protected, and the file `CLAUDE_ENV_FILE`
/// names is in the class when the guard has it.
#[test]
fn saved_shell_state_follows_the_environment() {
    let f = Fixture::new();
    let claude = f.home.join("profiles/one");
    let codex = f.home.join("codex-state");
    let env_file = f.home.join("profiles/one/session-env/abc/hook-1.sh");
    let env = StartupEnv {
        claude_config: Some(claude.clone()),
        codex_home: Some(codex.clone()),
        claude_env_file: Some(env_file.clone()),
        ..f.locations()
    };
    assert_eq!(
        class_target(&claude.join("session-env/abc/x.sh"), &env).as_deref(),
        Some("$CLAUDE_CONFIG_DIR/session-env")
    );
    assert!(class_target(&claude.join("shell-snapshots/s.sh"), &env).is_some());
    assert!(class_target(&codex.join("shell_snapshots/s.sh"), &env).is_some());
    assert!(class_target(&f.home.join(".claude/session-env/x.sh"), &env).is_some());
    assert!(class_target(&f.home.join(".codex/shell_snapshots/x.sh"), &env).is_some());
    assert!(class_target(&env_file, &env).is_some());
    assert!(class_target(&claude.join("notes.md"), &env).is_none());
    assert!(class_target(&codex.join("config.toml"), &env).is_none());
    let mut wrong = Vec::new();
    for command in [
        format!("echo x >> {}/session-env/abc/x.sh", slashed(&claude)),
        format!("cp evil.sh {}/shell_snapshots/s.sh", slashed(&codex)),
        format!("printf x > {}", slashed(&env_file)),
        "echo x >> $CLAUDE_CONFIG_DIR/session-env/abc/x.sh".to_string(),
        "echo x >> \"${CODEX_HOME}/shell_snapshots/s.sh\"".to_string(),
        "echo 'export PATH=/tmp/x:$PATH' >> \"$CLAUDE_ENV_FILE\"".to_string(),
    ] {
        if !refused(&judge_in(&command, &f.project, &env)) {
            wrong.push(command);
        }
    }
    // A removal through a variable the guard holds is resolved by it.
    f.mkdir(&claude.join("session-env/abc"));
    assert_eq!(
        f.pass_in("rm -rf \"$CLAUDE_CONFIG_DIR\"", &f.project, &env),
        [format!(
            "`rm` removes `{0}`, which is or holds saved shell state: `{0}`",
            slashed(&claude)
        )]
    );
    assert!(wrong.is_empty(), "not refused:\n{}", wrong.join("\n"));
}

/// The saved state folders are protected by name, wherever they are; a
/// name that only contains one is not.
#[test]
fn saved_state_folders_are_protected_wherever_they_are() {
    let f = Fixture::new();
    let env = f.locations();
    let mut wrong = Vec::new();
    for path in [
        "other/profiles/two/session-env/abc/x.sh",
        "elsewhere/shell-snapshots/snapshot-zsh-1.sh",
        "accounts/three/shell_snapshots/s.1.sh",
        "accounts/three/shell_snapshots",
        "other-xdg/herdr-core/templates/build.md",
    ] {
        let absolute = f.home.join(path);
        if class_target(&absolute, &env).is_none() {
            wrong.push(format!("not in the class: {path}"));
        }
        for command in [
            format!("echo x >> {}", slashed(&absolute)),
            format!("cp evil.sh {}", slashed(&absolute)),
        ] {
            if !refused(&f.judge(&command)) {
                wrong.push(format!("not refused: {command}"));
            }
        }
    }
    for path in [
        "notes/my-session-env/x.sh",
        "notes/session-env.txt",
        "src/shell_snapshot.rs",
        "docs/shell-snapshots-guide.md",
        "other-xdg/herdr-core/notes.md",
        "other-xdg/templates/build.md",
        "other-xdg/herdr-core/templatesx/build.md",
    ] {
        if class_target(&f.home.join(path), &env).is_some() {
            wrong.push(format!("in the class: {path}"));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

// ---------------------------------------------------------------------------
// The pass: removals, moves, copies and extractions
// ---------------------------------------------------------------------------

/// A tree removed or moved away refuses when it is, holds or lies above
/// saved state, wherever that is; one with none of it, a plain file named
/// like a folder, and an absent target do not.
#[test]
fn removing_or_moving_a_tree_that_holds_saved_state_refuses() {
    let f = Fixture::new();
    let holder = f.home.join("work/holder");
    f.mkdir(&holder.join("session-env"));
    f.mkdir(&f.home.join("work/mine/notes"));
    std::fs::write(f.project.join("shell_snapshots"), "ordinary data").unwrap();
    let h = slashed(&holder);
    assert_eq!(
        f.pass(&format!("rm -rf {h}")),
        [format!(
            "`rm` removes `{h}`, which is or holds saved shell state: `{h}`"
        )]
    );
    assert_eq!(
        f.pass(&format!("find {h} -delete")),
        [format!(
            "`find` removes `{h}`, which is or holds saved shell state: `{h}`"
        )]
    );
    assert_eq!(
        f.pass(&format!("mv {h} ../kept")),
        [format!(
            "`mv` moves `{h}`, which is or holds saved shell state: `{h}`"
        )]
    );
    assert_eq!(
        f.pass(&format!("find {h} -exec rm -rf {{}} +")),
        [format!(
            "`find` removes `{h}`, which is or holds saved shell state: `{h}`"
        )]
    );
    f.mkdir(&f.project.join("-holder/session-env"));
    assert_eq!(f.pass("rm -rf -- -holder").len(), 1);
    // `rmdir` and `unlink` cannot remove a tree: only the path counts.
    assert!(f.pass(&format!("rmdir {h}")).is_empty());
    assert!(f.pass(&format!("unlink {h}")).is_empty());
    for command in [
        format!("rm -rf {}", slashed(&f.home.join("work/mine"))),
        format!("mv {} ../kept", slashed(&f.home.join("work/mine"))),
        "rm -rf shell_snapshots".to_string(),
        "rm -rf build".to_string(),
        format!("find {} -name x -print", h),
    ] {
        assert!(f.pass(&command).is_empty(), "{command}");
    }
    // The default location counts only once it exists.
    let config = f.home.join(".codex");
    f.mkdir(&config);
    assert!(f.pass("rm -rf ~/.codex").is_empty());
    f.mkdir(&config.join("shell_snapshots"));
    assert_eq!(
        f.pass("rm -rf ~/.codex"),
        [format!(
            "`rm` removes `~/.codex`, which is or holds saved shell state: `{}`",
            slashed(&config)
        )]
    );
}

/// A removal does not follow an ordinary directory link: it removes the
/// link. A link whose own path is a saved-state location stays protected,
/// and a trailing slash follows the link to the tree it names.
#[cfg(unix)]
#[test]
fn a_removal_is_judged_by_the_node_it_removes() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.mkdir(&f.home.join("outside/shell_snapshots"));
    symlink(f.home.join("outside"), f.project.join("glob-link")).unwrap();
    assert!(f.pass("rm -rf glob-*").is_empty());
    assert!(f.pass("rm -rf glob-link").is_empty());
    assert_eq!(f.pass("rm -rf glob-link/").len(), 1);
    let stored = f.home.join("stored");
    f.mkdir(&stored.join("shell_snapshots"));
    let alias = f.project.join("config-link");
    symlink(&stored, &alias).unwrap();
    let env = StartupEnv {
        codex_home: Some(alias.clone()),
        ..f.locations()
    };
    assert_eq!(
        f.pass_in("rm -rf config-link", &f.project, &env),
        [format!(
            "`rm` removes `config-link`, which is or holds saved shell state: `{}`",
            slashed(&alias)
        )]
    );
}

/// A tree placed into a directory refuses when the directory is, lies in
/// or lies above a known location, or holds a saved-state folder found
/// within four levels below it; a single file keeps its own destination's
/// judgment, and a directory with none of it, and a read, pass.
#[test]
fn placing_a_tree_over_saved_state_refuses() {
    let f = Fixture::new();
    let profiles = f.home.join("work/profiles");
    for dir in [
        "other/session-env/abc",
        "deep/a/b/shell_snapshots",
        "tools/herdr-core/templates",
        "mine/notes",
        "empty",
    ] {
        f.mkdir(&profiles.join(dir));
    }
    f.mkdir(&f.project.join("src"));
    std::fs::write(f.project.join("notes.txt"), "x").unwrap();
    let at = |name: &str| slashed(&profiles.join(name));
    let mut wrong = Vec::new();
    for name in ["other", "deep", "deep/a", "tools"] {
        let place = at(name);
        for command in [
            format!("tar -xf state.tar -C {place}"),
            format!("tar -C {place} -xzf state.tgz"),
            format!("tar --directory={place} -xf state.tar"),
            format!("bsdtar -xf state.tar -C {place}"),
            format!("unzip -o t.zip -d {place}"),
            format!("cd {place} && tar -xf ../state.tar"),
            format!("cd {place} && unzip -o ../t.zip"),
            format!("rsync -a src/ {place}/"),
            format!("cp -R src/. {place}"),
            format!("cp -a src {place}/"),
            format!("ditto src {place}"),
            format!("mv src {place}"),
        ] {
            let found = f.pass(&command);
            if found.len() != 1 {
                wrong.push(format!("{command}: {found:?}"));
            }
        }
    }
    for command in [
        format!("tar -xf state.tar -C {}", at("mine")),
        format!("tar -xf state.tar -C {}", at("empty")),
        format!("unzip -o t.zip -d {}", at("mine")),
        format!("rsync -a src/ {}/", at("mine")),
        format!("cp -R src/. {}", at("mine")),
        format!("tar -tf state.tar -C {}", at("other")),
        format!("cp notes.txt {}", at("other")),
        format!("cp -r notes.txt {}", at("other")),
        format!("mv notes.txt {}", at("other")),
        format!("rsync -a {}/ backup/", at("other")),
        format!("ls {}", at("other")),
    ] {
        let found = f.pass(&command);
        if !found.is_empty() {
            wrong.push(format!("refused: {command}: {found:?}"));
        }
    }
    // A known location by path, with no read below it: the home and the
    // configuration folder above the default saved state.
    let claude = f.home.join(".claude");
    for command in [
        "cp -r src ~",
        "tar -xf a.tar -C ~/.claude",
        "ditto src ~/.claude/session-env/x",
    ] {
        let found = f.pass(command);
        if found.len() != 1 {
            wrong.push(format!("{command}: {found:?}"));
        }
    }
    assert_eq!(
        f.pass("tar -xf a.tar -C ~/.claude"),
        [format!(
            "`tar` places files into `~/.claude`, which is, holds or lies in saved shell state: `~/.claude/session-env`"
        )]
    );
    assert!(!claude.exists());
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// The scan reads four levels below the target: a folder at the fourth
/// level is found, a directory past it is not read and refuses, and a
/// repository's own administrative tree is read without the cutoff.
#[test]
fn the_scan_stops_at_its_depth_and_refuses_what_it_cannot_read() {
    let f = Fixture::new();
    f.mkdir(&f.project.join("src"));
    let near = f.home.join("near");
    f.mkdir(&near.join("a/b/c/session-env"));
    assert_eq!(
        f.pass(&format!("cp -r src {}", slashed(&near))),
        [format!(
            "`cp` places files into `{0}`, which holds saved shell state below it: `{0}`",
            slashed(&near)
        )]
    );
    let deep = f.home.join("deep");
    f.mkdir(&deep.join("a/b/c/d/other"));
    assert_eq!(
        f.pass(&format!("cp -r src {}", slashed(&deep))),
        [format!(
            "`cp` places files into `{0}`, which the guard cannot show is clear of saved shell state: `{0}` (the guard cannot read this target in full to check for saved shell state; name a narrower target directory that the guard can read)",
            slashed(&deep)
        )]
    );
    let shallow = f.home.join("shallow");
    f.mkdir(&shallow.join("a/b/c"));
    assert!(f
        .pass(&format!("cp -r src {}", slashed(&shallow)))
        .is_empty());
}

/// A link found in a placement's tree is judged by where it leads: to, above
/// or into saved state refuses, as does one the scan cannot resolve; a link
/// to an ordinary folder passes.
#[cfg(unix)]
#[test]
fn a_link_in_a_placement_tree_is_judged_by_where_it_leads() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.mkdir(&f.project.join("src"));
    let state = f.home.join("profile/session-env");
    f.mkdir(&state.join("abc"));
    f.mkdir(&f.home.join("ordinary"));
    let make = |name: &str, to: &Path| {
        let dir = f.home.join("links").join(name);
        f.mkdir(&dir);
        symlink(to, dir.join("x")).unwrap();
        slashed(&dir)
    };
    for tree in [
        make("to-state", &state),
        make("inside", &state.join("abc")),
        make("above", &f.home.join("profile")),
        make("broken", &f.home.join("does/not/exist")),
    ] {
        assert_eq!(
            f.pass(&format!("tar -xf a.tar -C {tree}")).len(),
            1,
            "{tree}"
        );
    }
    let ordinary = make("ordinary", &f.home.join("ordinary"));
    assert!(f.pass(&format!("tar -xf a.tar -C {ordinary}")).is_empty());
}

/// Each finding of the pass is one always-blocking `security.shell_startup`
/// finding, and the exec guard puts it after every other finding.
#[test]
fn the_exec_guard_appends_saved_state_findings_last() {
    let f = Fixture::new();
    f.mkdir(&f.project.join("x/session-env"));
    let found = crate::hooks::exec_guard::evaluate_ruled(
        "rm -rf x",
        true,
        &crate::hooks::policy::SecuritySection::default(),
        PolicyLevel::Block,
        &f.project,
        &f.project,
        &crate::hooks::sandbox_state::Outcome::Relaxed,
    );
    let last = found.last().expect("a finding");
    assert_eq!(last.rule, RULE);
    assert!(last.level_fixed);
    assert_eq!(
        last.message,
        format!(
            "`rm` removes `x`, which is or holds saved shell state: `{}`",
            slashed(&f.project.join("x"))
        )
    );
    assert!(without(|| crate::hooks::exec_guard::evaluate_ruled(
        "rm -rf x",
        true,
        &crate::hooks::policy::SecuritySection::default(),
        PolicyLevel::Block,
        &f.project,
        &f.project,
        &crate::hooks::sandbox_state::Outcome::Relaxed,
    ))
    .iter()
    .all(|v| v.message != last.message));
}
