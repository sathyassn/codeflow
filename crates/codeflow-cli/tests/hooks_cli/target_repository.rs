//! TSK-379: built hooks judge the repository holding a target.
use super::{codeflow, enforced_repo, guard_payload, run_with_stdin, shell_path};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

// Installed Git hook shims must execute this test build, including when the
// caller runs an instrumented build. The override belongs to Git's child
// environment, never to the surrounding Cargo or coverage process.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOOK_BINARY", env!("CARGO_BIN_EXE_codeflow"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

struct Fixture {
    temp: tempfile::TempDir,
    root: PathBuf,
    linked: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        enforced_repo(&root);
        let linked = root.join(".worktrees/w");
        git(
            &root,
            &["worktree", "add", "-b", "task/w", &shell_path(&linked)],
        );
        Self { temp, root, linked }
    }

    fn command(&self, cwd: &Path, hook: &str, command: &str) -> Output {
        run_with_stdin(
            codeflow()
                .args(["hook", hook, "--contract", "3"])
                .current_dir(cwd)
                .env("HOME", self.temp.path())
                .env("USERPROFILE", self.temp.path())
                .env("SHELL", "/bin/zsh"),
            &guard_payload(command, cwd),
        )
    }

    fn write(&self, cwd: &Path, route: &str, target: &Path) -> Output {
        let path = shell_path(target);
        match route {
            "redirect" => self.command(cwd, "git-guard", &format!("printf x > '{path}'")),
            "interpreter" => self.command(cwd, "exec-guard", &format!("python3 tool.py '{path}'")),
            "native" => run_with_stdin(
                codeflow()
                    .args(["hook", "edit-guard", "--contract", "3"])
                    .current_dir(cwd)
                    .env("HOME", self.temp.path())
                    .env("USERPROFILE", self.temp.path()),
                &serde_json::json!({"tool_name":"Write", "cwd":cwd,
                    "tool_input":{"file_path":target,"content":"{}"}})
                .to_string(),
            ),
            _ => panic!("unknown route"),
        }
    }

    fn targets(&self) -> [PathBuf; 4] {
        [
            self.root.join(".claude/settings.json"),
            self.root.join(".codeflow/policy.json"),
            self.root.join(".git/hooks/pre-commit"),
            self.linked.join(".claude/settings.json"),
        ]
    }
}

fn refused(out: &Output) -> bool {
    out.status.code() == Some(2)
        && String::from_utf8_lossy(&out.stderr).contains("git.hook_integrity")
}

fn target_journey(location: &str, route: &str) {
    let f = Fixture::new();
    target_journey_in(&f, location, route);
}

fn target_journey_in(f: &Fixture, location: &str, route: &str) {
    let cwd = match location {
        "nested" => {
            let path = f.root.join("nested");
            if !path.exists() {
                std::fs::create_dir(&path).unwrap();
                git(&path, &["init"]);
            }
            path
        }
        "submodule" => {
            let source = f.temp.path().join("source");
            if !source.exists() {
                std::fs::create_dir(&source).unwrap();
                enforced_repo(&source);
                git(
                    &f.root,
                    &[
                        "-c",
                        "protocol.file.allow=always",
                        "submodule",
                        "add",
                        &shell_path(&source),
                        "sub",
                    ],
                );
            }
            let path = f.root.join("sub");
            assert!(path.join(".git").is_file());
            path
        }
        "outside" => f.temp.path().to_path_buf(),
        "unrelated" => {
            let path = f.temp.path().join("session");
            if !path.exists() {
                std::fs::create_dir(&path).unwrap();
                enforced_repo(&path);
            }
            path
        }
        _ => panic!("unknown location"),
    };
    let mut wrong = Vec::new();
    for target in f.targets() {
        let out = f.write(&cwd, route, &target);
        if !refused(&out) {
            wrong.push(format!("{}: {out:?}", target.display()));
        }
    }
    let ordinary = f.write(&cwd, route, &f.root.join("README.md"));
    assert_eq!(ordinary.status.code(), Some(0), "ordinary: {ordinary:?}");
    assert!(wrong.is_empty(), "{location}/{route}: {}", wrong.join("\n"));
}

macro_rules! target_test {
    ($name:ident, $cwd:literal, $route:literal) => {
        #[test]
        fn $name() {
            target_journey($cwd, $route);
        }
    };
}
target_test!(target_nested_redirect, "nested", "redirect");
target_test!(target_nested_interpreter, "nested", "interpreter");
target_test!(target_nested_native, "nested", "native");
target_test!(target_submodule_redirect, "submodule", "redirect");
target_test!(target_submodule_interpreter, "submodule", "interpreter");
target_test!(target_submodule_native, "submodule", "native");
target_test!(target_outside_redirect, "outside", "redirect");
target_test!(target_outside_interpreter, "outside", "interpreter");
target_test!(target_outside_native, "outside", "native");
target_test!(target_unrelated_redirect, "unrelated", "redirect");
target_test!(target_unrelated_interpreter, "unrelated", "interpreter");
target_test!(target_unrelated_native, "unrelated", "native");

#[test]
fn target_unrelated_recursive_delete() {
    let f = Fixture::new();
    let session = f.temp.path().join("session");
    std::fs::create_dir(&session).unwrap();
    enforced_repo(&session);
    let command = format!("rm -rf '{}'", shell_path(&f.linked));
    let out = f.command(&session, "git-guard", &command);
    assert!(refused(&out), "{out:?}");
}

#[test]
fn target_worktree_configuration_writes() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for cwd in repair_cwds(&f) {
        // The wildcard spellings have a real shell expansion in each cwd.
        std::fs::write(cwd.join("core.worktree"), "").unwrap();
        for hook in ["git-guard", "exec-guard"] {
            for command in [
                "KEY=core.worktree; git config \"$KEY\" /decoy",
                "env git config core.bare true",
                "sh -c 'git config core.worktree /decoy'",
                "git config --rename-section decoy core",
                "git config --edit",
                "git config {core.bare,true}",
                "echo true | xargs git config core.bare",
                "echo /x | xargs git config core.worktree",
                "echo branch -D main | xargs git",
                "arch -arm64 git config core.bare true",
                "git config core.worktre? /y",
                "git config core.worktre[e] /x",
                "git config {core.hooksPath,/dev/null}",
                "git config {--get,core.bare,true}",
            ] {
                let out = f.command(&cwd, hook, command);
                if !refused(&out) {
                    wrong.push(format!("{hook}: {command}: {out:?}"));
                }
            }
            for options in ["", "--local ", "--file .git/config ", "--worktree "] {
                let command = format!("git config {options}core.worktree /decoy");
                let out = f.command(&cwd, hook, &command);
                if !refused(&out) {
                    wrong.push(format!("{hook}: {command}: {out:?}"));
                }
            }
            let command = format!(
                "git -C '{}' config core.worktree /decoy",
                shell_path(&f.root)
            );
            let out = f.command(&cwd, hook, &command);
            if !refused(&out) {
                wrong.push(format!("{hook}: {command}: {out:?}"));
            }
            let out = f.command(&cwd, hook, "git -c core.worktree=/decoy status");
            assert_eq!(out.status.code(), Some(0), "transient {hook}: {out:?}");
            let out = f.command(&cwd, hook, r"git config --get-regexp 'remote\..*\.url'");
            assert_eq!(out.status.code(), Some(0), "read {hook}: {out:?}");
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// Each round-two regression drives the built hook, including allowed controls.
#[test]
fn repair2_config_reads() {
    let f = Fixture::new();
    for hook in ["git-guard", "exec-guard"] {
        for flag in [
            "--get",
            "--get-all",
            "--get-regexp",
            "--get-urlmatch",
            "-l",
            "--list",
            "get",
            "list",
        ] {
            let command = format!("git config {flag} 'remote\\..*\\.url'");
            let out = f.command(&f.root, hook, &command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair2_xargs_guarded_verbs() {
    let f = Fixture::new();
    for hook in ["git-guard", "exec-guard"] {
        for command in ["git diff --name-only | xargs git add", "xargs git status"] {
            let out = f.command(&f.root, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        for tail in [
            "",
            "config",
            "worktree",
            "update-ref",
            "symbolic-ref",
            "branch -D",
            "branch -d",
            "branch main",
            "push",
            "reset --hard",
            "reset HEAD",
            "clean",
            "checkout --",
            "switch --",
            "restore --",
            "rm",
        ] {
            let command = format!("printf x | xargs git {tail}");
            let out = f.command(&f.root, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair2_planted_markers_cannot_hide_checkouts() {
    for plant in ["repository", "empty", "foreign", "garbage"] {
        let f = Fixture::new();
        let cover = f.root.join(".worktrees/cover");
        let below = cover.join("below");
        std::fs::create_dir_all(&cover).unwrap();
        git(
            &f.root,
            &[
                "worktree",
                "move",
                &shell_path(&f.linked),
                &shell_path(&below),
            ],
        );
        let away = f.temp.path().join("away");
        std::fs::rename(&below, &away).unwrap();
        git(&f.root, &["worktree", "prune", "--expire", "now"]);
        std::fs::rename(&away, &below).unwrap();
        match plant {
            "repository" => git(&cover, &["init"]),
            "empty" => std::fs::create_dir(cover.join(".git")).unwrap(),
            "foreign" => {
                let foreign = f.temp.path().join("foreign");
                std::fs::create_dir(&foreign).unwrap();
                git(&foreign, &["init"]);
                std::fs::write(
                    cover.join(".git"),
                    format!("gitdir: {}\n", shell_path(&foreign.join(".git"))),
                )
                .unwrap();
            }
            _ => std::fs::write(cover.join(".git"), "garbage\n").unwrap(),
        }
        for hook in ["git-guard"] {
            for command in [
                "rm -rf .worktrees",
                "find .worktrees -delete",
                "git clean -ffd .worktrees",
                "rm -rf .worktrees/cover/below",
            ] {
                let out = f.command(&f.root, hook, command);
                assert!(refused(&out), "{plant} {hook} {command}: {out:?}");
                if command != "rm -rf .worktrees/cover/below" {
                    assert!(
                        String::from_utf8_lossy(&out.stderr).contains("git worktree repair"),
                        "{plant} {command}: {out:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn repair2_planted_ancestor_marker() {
    let f = Fixture::new();
    let cover = f.root.join(".worktrees/cover");
    let content = cover.join("content");
    std::fs::create_dir_all(&content).unwrap();
    let below = content.join("below");
    git(
        &f.root,
        &[
            "worktree",
            "move",
            &shell_path(&f.linked),
            &shell_path(&below),
        ],
    );
    let away = f.temp.path().join("away");
    std::fs::rename(&below, &away).unwrap();
    git(&f.root, &["worktree", "prune", "--expire", "now"]);
    std::fs::rename(&away, &below).unwrap();
    git(&cover, &["init"]);
    let out = f.command(&f.root, "git-guard", "rm -rf .worktrees/cover/content");
    assert!(refused(&out), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("git worktree repair"),
        "{out:?}"
    );
}

#[test]
fn repair2_git_init_locations() {
    let f = Fixture::new();
    for cwd in repair_cwds(&f) {
        for hook in ["git-guard", "exec-guard"] {
            for dir in [
                ".worktrees/cover",
                ".claude/new",
                ".codeflow/new",
                ".git/hooks/new",
                ".git/modules/new",
            ] {
                for option in ["", "--separate-git-dir /unused-git-dir "] {
                    let command = format!("git init {option}'{}'", shell_path(&f.root.join(dir)));
                    let out = f.command(&cwd, hook, &command);
                    assert!(refused(&out), "{cwd:?} {hook} {command}: {out:?}");
                }
                let command = format!(
                    "git init --separate-git-dir='{}' '{}'",
                    shell_path(&f.root.join(dir)),
                    shell_path(&f.temp.path().join("scratch"))
                );
                let out = f.command(&cwd, hook, &command);
                assert!(refused(&out), "{hook} {command}: {out:?}");
            }
            let command = format!("git init '{}'", shell_path(&f.temp.path().join("scratch")));
            let out = f.command(&cwd, hook, &command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
            let command = format!(
                "git -C '{}' init --separate-git-dir '{}'",
                shell_path(&f.root.join(".worktrees")),
                shell_path(&f.temp.path().join("admin"))
            );
            let out = f.command(&cwd, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair2_editor_scripts() {
    let f = Fixture::new();
    for cwd in repair_cwds(&f) {
        for hook in ["git-guard", "exec-guard"] {
            for command in [
                "printf 'e .git/config\\na\\nhooksPath = /tmp/evil\\n.\\nw\\nq\\n' | ed -s",
                "printf 'e .git/config\\na\\n# marker\\n.\\nw\\nq\\n' | ed -s",
                "printf 'e .git/config\\nw\\nq\\n' | ex -s",
                "printf 'e .git/config\\n:wq\\n' | vim -es",
                "printf 'e .git/config\\nw\\nq\\n' | vi -s",
                "printf 'e .git/config\\nwq\\n' | nvim --headless",
                "emacs --batch --eval '(write-region \"pwn\\n\" nil \".git/config\" t)'",
                "printf 'e .git/config\\nw\\nq\\n' | ed -s README.md",
            ] {
                let out = f.command(&cwd, hook, command);
                assert!(refused(&out), "{hook} {command}: {out:?}");
            }
            let out = f.command(&cwd, hook, "vim README.md");
            assert_eq!(out.status.code(), Some(0), "interactive {hook}: {out:?}");
            // The guard does not interpret editor scripts. A file operand
            // cannot prove that stdin does not switch to a second file.
            let out = f.command(
                &cwd,
                hook,
                "printf 'a\\ntext\\n.\\nw\\nq\\n' | ed -s README.md",
            );
            assert!(refused(&out), "unread script {hook}: {out:?}");
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("editor script"),
                "{out:?}"
            );
        }
    }
}

#[test]
fn repair2_git_creation_assignments() {
    let f = Fixture::new();
    for hook in ["git-guard", "exec-guard"] {
        for command in [
            "D=.claude; git clone /source $D/newclone",
            "D=.git/hooks; git clone /source $D/planted",
            "D=.claude; git worktree add $D/wtvar",
            "git clone /source $UNKNOWN/newclone",
            "git clone /source $D/newclone; D=scratch",
            "D=.claude; git -C \"$D\" clone /source newclone",
            "git -C \"$UNKNOWN\" clone /source newclone",
        ] {
            let out = f.command(&f.root, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        let out = f.command(&f.root, hook, "D=scratch; git clone /source $D/newclone");
        assert_eq!(out.status.code(), Some(0), "ordinary {hook}: {out:?}");
    }
}

#[test]
fn repair2_creation_path_spellings() {
    let f = Fixture::new();
    std::fs::create_dir(f.root.join("scratch")).unwrap();
    for hook in ["git-guard", "exec-guard"] {
        for command in [
            "git clone /source scratch/../.claude/newclone",
            "D=scratch/../.claude; git clone /source $D/newclone",
            "git worktree add scratch/../.git/hooks/planted",
            "git init scratch/../.codeflow/new",
        ] {
            let out = f.command(&f.root, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair2_git_command_runners() {
    let f = Fixture::new();
    target_journey_in(&f, "submodule", "native");
    for hook in ["git-guard", "exec-guard"] {
        for command in [
            "git rebase -x 'git -C ../.. config core.hooksPath /dev/null' HEAD~1",
            "git rebase --exec 'git config core.bare true' HEAD~1",
            "git rebase --exec='git config core.worktree /decoy' HEAD~1",
            "git rebase -x 'rm -rf ../../.claude' HEAD~1",
            "git bisect run git config core.bare true",
            "git submodule foreach 'rm -rf ../.claude'",
        ] {
            let cwd = if command.starts_with("git submodule") {
                &f.root
            } else {
                &f.linked
            };
            let out = f.command(cwd, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        for command in [
            "git rebase -x 'cargo test' HEAD~3",
            "git bisect run ./test.sh",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair2_git_directory_parents() {
    let f = Fixture::new();
    for cwd in repair_cwds(&f) {
        let modules = shell_path(&f.root.join(".git/modules"));
        for hook in ["git-guard"] {
            for command in [
                format!("rm -rf '{modules}'"),
                format!("mv '{modules}' /unused-destination"),
                format!("find '{modules}' -delete"),
                format!("rsync -a --delete /empty/ '{modules}/'"),
            ] {
                let out = f.command(&cwd, hook, &command);
                assert!(refused(&out), "{cwd:?} {hook} {command}: {out:?}");
            }
        }
    }
}

#[test]
fn repair2_separate_git_directory_container() {
    let f = Fixture::new();
    let container = f.temp.path().join("metadata");
    std::fs::create_dir(&container).unwrap();
    let separate = container.join("separate");
    let checkout = f.temp.path().join("checkout");
    git(
        f.temp.path(),
        &[
            "init",
            "--separate-git-dir",
            &shell_path(&separate),
            &shell_path(&checkout),
        ],
    );
    for cwd in repair_cwds(&f) {
        let path = shell_path(&container);
        for command in [
            format!("rm -rf '{path}'"),
            format!("mv '{path}' /unused-destination"),
            format!("find '{path}' -delete"),
            format!("rsync -a --delete /empty/ '{path}/'"),
        ] {
            let out = f.command(&cwd, "git-guard", &command);
            assert!(refused(&out), "{cwd:?} {command}: {out:?}");
        }
    }
}

#[test]
fn repair2_modules_namespace() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.root.join(".git/modules")).unwrap();
    let out = f.command(&f.root, "git-guard", "rm -rf .git/modules");
    assert!(refused(&out), "{out:?}");
}

#[test]
fn repair2_marker_error_messages() {
    let f = Fixture::new();
    let away = f.temp.path().join("away");
    std::fs::rename(&f.linked, &away).unwrap();
    git(&f.root, &["worktree", "prune", "--expire", "now"]);
    std::fs::rename(&away, &f.linked).unwrap();
    for route in ["native", "redirect", "interpreter"] {
        let out = f.write(&f.root, route, &f.linked.join(".claude/settings.json"));
        let message = String::from_utf8_lossy(&out.stderr);
        assert!(refused(&out), "{out:?}");
        assert!(message.contains("git worktree repair"), "{message}");
        assert!(
            !message.contains("class=") && !message.contains("code="),
            "{message}"
        );
        assert!(
            message.contains(&shell_path(&f.linked.join(".git"))),
            "{message}"
        );
    }
}

// Round-one repairs are exercised through the installed hook CLI contract.
fn repair_cwds(f: &Fixture) -> Vec<PathBuf> {
    for location in ["nested", "submodule", "unrelated"] {
        target_journey_in(f, location, "native");
    }
    vec![
        f.root.clone(),
        f.linked.clone(),
        f.root.join("nested"),
        f.root.join("sub"),
        f.temp.path().join("session"),
        f.temp.path().to_path_buf(),
    ]
}

#[test]
fn repair_large_build_cleanup() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for folder in [f.linked.join("target"), f.root.join(".worktrees/old-build")] {
        std::fs::create_dir_all(&folder).unwrap();
        for i in 0..4200 {
            std::fs::write(folder.join(format!("object-{i}")), "x").unwrap();
        }
        for cwd in [&f.root, &f.linked] {
            for command in [
                format!("rm -rf '{}'", shell_path(&folder)),
                format!(
                    "mv '{}' '{}'",
                    shell_path(&folder),
                    shell_path(&f.temp.path().join("build"))
                ),
            ] {
                let out = f.command(cwd, "git-guard", &command);
                if !out.status.success() {
                    wrong.push(format!("{command}: {out:?}"));
                }
            }
        }
    }
    let out = f.command(&f.linked, "git-guard", "rm -rf target");
    if !out.status.success() {
        wrong.push(format!("relative: {out:?}"));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn repair_ancestor_marker_creation() {
    let f = Fixture::new();
    let marker = f.temp.path().join(".git");
    for route in ["redirect", "interpreter", "native"] {
        let out = f.write(&f.root, route, &marker);
        assert!(refused(&out), "{route}: {out:?}");
    }
    for target in [&marker, &marker.join("objects")] {
        let out = f.command(
            &f.root,
            "git-guard",
            &format!("mkdir -p '{}'", shell_path(target)),
        );
        assert!(refused(&out), "{out:?}");
    }
}

#[test]
fn repair_unreadable_owner_only_blocks_its_enforcement() {
    let f = Fixture::new();
    let marker = f.temp.path().join(".git");
    for bytes in ["garbage", "gitdir: /no/such/tsk379\n"] {
        std::fs::write(&marker, bytes).unwrap();
        for route in ["redirect", "interpreter", "native"] {
            let out = f.write(&f.root, route, &f.root.join("README.md"));
            assert!(out.status.success(), "{bytes} {route}: {out:?}");
            let out = f.write(&f.root, route, &f.temp.path().join(".claude/settings.json"));
            let message = String::from_utf8_lossy(&out.stderr);
            assert_eq!(out.status.code(), Some(2), "{route}: {out:?}");
            assert!(message.contains(&shell_path(&marker)), "{message}");
            assert!(
                !message.contains("remote-tracking policy metadata"),
                "{message}"
            );
            assert!(!message.contains("git worktree repair"), "{message}");
        }
    }
}

#[test]
fn repair_bare_config_keeps_checkout_protected() {
    let f = Fixture::new();
    let cwds = repair_cwds(&f);
    git(&f.root, &["config", "core.bare", "true"]);
    for cwd in cwds {
        for route in ["redirect", "interpreter", "native"] {
            let out = f.write(&cwd, route, &f.root.join(".claude/settings.json"));
            assert_eq!(out.status.code(), Some(2), "{cwd:?} {route}: {out:?}");
        }
    }
}

#[test]
fn repair_editors_are_writers() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for cwd in repair_cwds(&f) {
        for target in [
            f.root.join(".git/config"),
            f.root.join(".claude/settings.json"),
        ] {
            for editor in ["ed -s", "ex -s", "vi -es", "vim -es", "nvim --headless"] {
                let command = format!(
                    "printf 'a\\n[core]\\n hooksPath = /evil\\n.\\nw\\nq\\n' | {editor} '{}'",
                    shell_path(&target)
                );
                let out = f.command(&cwd, "git-guard", &command);
                if !refused(&out) {
                    wrong.push(format!("{cwd:?} {command}: {out:?}"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn repair_git_directories_are_owners() {
    git_directory_journey(&Fixture::new());
}

fn git_directory_journey(f: &Fixture) {
    let cwds = repair_cwds(f);
    let separate = f.temp.path().join("separate-admin");
    let checkout = f.temp.path().join("separate-checkout");
    git(
        f.temp.path(),
        &[
            "init",
            "--separate-git-dir",
            &shell_path(&separate),
            &shell_path(&checkout),
        ],
    );
    let mut wrong = Vec::new();
    for admin in [f.root.join(".git/modules/sub"), separate.clone()] {
        for name in [
            "config",
            "config.worktree",
            "hooks/pre-commit",
            "HEAD",
            "packed-refs",
            "refs/remotes/origin/main",
        ] {
            for cwd in &cwds {
                for route in ["redirect", "interpreter", "native"] {
                    let out = f.write(cwd, route, &admin.join(name));
                    if !refused(&out) {
                        wrong.push(format!("{}/{route}/{name}: {out:?}", cwd.display()));
                    }
                }
            }
        }
    }
    std::fs::write(separate.join("config"), "[broken").unwrap();
    for route in ["redirect", "interpreter", "native"] {
        let out = f.write(&f.root, route, &separate.join("hooks/pre-commit"));
        assert_eq!(out.status.code(), Some(2), "unreadable {route}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(&shell_path(&separate)),
            "{out:?}"
        );
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn repair_rsync_source_spellings_and_gitfile() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for cwd in repair_cwds(&f) {
        for flag in [
            "--remove-source-files",
            "--rem",
            "--remo",
            "--remov",
            "--remove",
            "--remove-s",
            "--remove-source",
            "--remove-sent-files",
            "--remove-sent",
        ] {
            for source in [&f.linked, &f.linked.join(".git")] {
                let command = format!("rsync -a {flag} '{}' /dst/", shell_path(source));
                let out = f.command(&cwd, "git-guard", &command);
                if !refused(&out) {
                    wrong.push(format!("{cwd:?} {command}: {out:?}"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn repair_git_creation_in_enforcement_directories() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for cwd in repair_cwds(&f) {
        for dir in [".git/hooks/wt", ".claude/clone"] {
            for verb in ["worktree add", "clone /source"] {
                let command = format!(
                    "git -C '{}' {verb} '{}'",
                    shell_path(&f.root),
                    shell_path(&f.root.join(dir))
                );
                let out = f.command(&cwd, "git-guard", &command);
                if !refused(&out) {
                    wrong.push(format!("{cwd:?} {command}: {out:?}"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn repair_hidden_checkout_message() {
    let f = Fixture::new();
    let away = f.temp.path().join("away");
    std::fs::rename(&f.linked, &away).unwrap();
    git(&f.root, &["worktree", "prune", "--expire", "now"]);
    std::fs::rename(&away, &f.linked).unwrap();
    let out = f.command(&f.root, "git-guard", "rm -rf .worktrees");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(refused(&out), "{out:?}");
    assert!(
        message.contains("hidden checkout") && message.contains("git worktree repair"),
        "{message}"
    );
    assert!(
        !message.contains("delete the registered worktree"),
        "{message}"
    );
}

#[test]
fn target_shell_worktree_moves() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "mv .worktrees/w .worktrees/moved",
        "cp -R .worktrees/w backup && rm -rf .worktrees/w",
        "rsync -a --remove-source-files .worktrees/w/ backup/",
    ] {
        let out = f.command(&f.root, "git-guard", command);
        if !refused(&out) {
            wrong.push(format!("{command}: {out:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn target_returned_worktree_after_prune() {
    let f = Fixture::new();
    let away = f.temp.path().join("away");
    std::fs::rename(&f.linked, &away).unwrap();
    git(&f.root, &["worktree", "prune", "--expire", "now"]);
    assert!(!f.root.join(".git/worktrees/w").exists());
    std::fs::rename(&away, &f.linked).unwrap();
    let mut wrong = Vec::new();
    for command in [
        "rm -rf .worktrees",
        "mv .worktrees elsewhere",
        "rsync -a --remove-source-files .worktrees/ backup/",
    ] {
        let out = f.command(&f.root, "git-guard", command);
        if !refused(&out) {
            wrong.push(format!("returned checkout parent: {command}: {out:?}"));
        }
    }
    for cwd in [&f.root, &f.linked] {
        for target in f.targets() {
            for route in ["redirect", "interpreter", "native"] {
                let out = f.write(cwd, route, &target);
                if out.status.code() != Some(2) {
                    wrong.push(format!(
                        "{} {route} {}: {out:?}",
                        cwd.display(),
                        target.display()
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn target_registration_mismatch_names_human_repair() {
    let f = Fixture::new();
    let moved = f.root.join(".worktrees/moved");
    std::fs::rename(&f.linked, &moved).unwrap();
    for hook in ["git-guard", "exec-guard"] {
        let out = f.command(&moved, hook, "git worktree repair");
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(
            err.contains("git worktree repair") && err.contains("outside the harness"),
            "{err}"
        );
    }
}

fn lifecycle_journey(verb: &str) {
    let f = Fixture::new();
    // Remove the checkout outside the guarded session. Prune has no live
    // checkout to protect and remains an available recovery operation.
    if verb == "prune" {
        std::fs::remove_dir_all(&f.linked).unwrap();
    }
    let scratch = f.temp.path().join("scratch");
    let args = match verb {
        "add" => vec!["worktree", "add", "-b", "task/new", ".worktrees/new"],
        "move" => vec!["worktree", "move", ".worktrees/w", ".worktrees/moved"],
        "remove" => vec!["worktree", "remove", ".worktrees/w"],
        "list" => vec!["worktree", "list"],
        "prune" => vec!["worktree", "prune", "--expire", "now"],
        "init" => vec!["init"],
        _ => panic!("unknown verb"),
    };
    let cwd = if verb == "init" {
        std::fs::create_dir(&scratch).unwrap();
        &scratch
    } else {
        &f.root
    };
    for hook in ["git-guard", "exec-guard"] {
        let out = f.command(cwd, hook, &format!("git {}", args.join(" ")));
        assert_eq!(out.status.code(), Some(0), "{verb} {hook}: {out:?}");
    }
    git(cwd, &args);
    if verb == "prune" {
        assert!(!f.root.join(".git/worktrees/w").exists());
    }
    // A successful lifecycle operation does not remove the owning main
    // repository's protection when the next call starts outside it.
    let out = f.write(
        f.temp.path(),
        "native",
        &f.root.join(".claude/settings.json"),
    );
    assert!(refused(&out), "{verb}: {out:?}");
}

macro_rules! lifecycle_test {
    ($name:ident, $verb:literal) => {
        #[test]
        fn $name() {
            lifecycle_journey($verb);
        }
    };
}
lifecycle_test!(target_worktree_add_keeps_enforcement, "add");
lifecycle_test!(target_worktree_move_keeps_enforcement, "move");
lifecycle_test!(target_worktree_remove_keeps_enforcement, "remove");
lifecycle_test!(target_worktree_list_keeps_enforcement, "list");
lifecycle_test!(target_worktree_prune_deleted_keeps_enforcement, "prune");
lifecycle_test!(target_scratch_init_keeps_enforcement, "init");

#[test]
fn target_installed_adopter_journey() {
    let f = Fixture::new();
    let init = codeflow()
        .args(["init", "--minimal", "--yes"])
        .current_dir(&f.root)
        .env("HOME", f.temp.path())
        .env("USERPROFILE", f.temp.path())
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    assert!(f.root.join(".codeflow/git-hooks/pre-commit").is_file());
    println!(
        "Installed the built binary's minimal scaffold into {}",
        f.root.display()
    );
    for location in ["nested", "submodule", "outside", "unrelated"] {
        for route in ["redirect", "interpreter", "native"] {
            target_journey_in(&f, location, route);
            println!(
                "{location}/{route}: all four enforcement targets refused; ordinary file allowed"
            );
        }
    }
    for hook in ["git-guard", "exec-guard"] {
        for command in [
            "git config core.worktree /decoy",
            "git config --local core.worktree /decoy",
            "git config --file .git/config core.worktree /decoy",
            "git config --worktree core.worktree /decoy",
        ] {
            let out = f.command(&f.root, hook, command);
            assert!(refused(&out), "{out:?}");
            println!("{hook}: refused {command}");
        }
    }
    for command in [
        "mv .worktrees/w .worktrees/hidden",
        "cp -R .worktrees/w backup && rm -rf .worktrees/w",
        "rsync -a --remove-source-files .worktrees/w/ backup/",
    ] {
        let out = f.command(&f.root, "git-guard", command);
        assert!(refused(&out), "{out:?}");
        println!("git-guard: refused {command}");
    }
    for args in [
        vec!["worktree", "add", "-b", "task/legit", ".worktrees/legit"],
        vec!["worktree", "move", ".worktrees/legit", ".worktrees/moved"],
        vec!["worktree", "list"],
        vec!["worktree", "remove", ".worktrees/moved"],
    ] {
        let command = format!("git {}", args.join(" "));
        for hook in ["git-guard", "exec-guard"] {
            let out = f.command(&f.root, hook, &command);
            assert_eq!(out.status.code(), Some(0), "{out:?}");
        }
        git(&f.root, &args);
        println!("Allowed and executed {command}");
    }
    let scratch = f.temp.path().join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    for hook in ["git-guard", "exec-guard"] {
        let out = f.command(&scratch, hook, "git init");
        assert_eq!(out.status.code(), Some(0), "{out:?}");
    }
    git(&scratch, &["init"]);
    println!("Allowed and executed git init outside the checkout");
    git_directory_journey(&f);
    println!("Submodule and separate Git directory targets refused through every route and cwd");
    let away = f.temp.path().join("away");
    std::fs::rename(&f.linked, &away).unwrap();
    for hook in ["git-guard", "exec-guard"] {
        let out = f.command(&f.root, hook, "git worktree prune");
        assert_eq!(out.status.code(), Some(0), "{out:?}");
    }
    git(&f.root, &["worktree", "prune", "--expire", "now"]);
    std::fs::rename(&away, &f.linked).unwrap();
    for cwd in [&f.root, &f.linked] {
        for target in f.targets() {
            for route in ["redirect", "interpreter", "native"] {
                let out = f.write(cwd, route, &target);
                assert_eq!(out.status.code(), Some(2), "{out:?}");
            }
        }
    }
    let out = f.command(&f.root, "git-guard", "rm -rf .worktrees");
    assert!(refused(&out), "{out:?}");
    println!("External move, actual prune, return: all writes refused from both checkouts; parent deletion refused");
}
