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

#[test]
fn repair3_runner_options() {
    let f = Fixture::new();
    let findings = codeflow_core::hooks::exec_guard::evaluate_in(
        "git rebase --exec",
        true,
        &codeflow_core::hooks::policy::SecuritySection::default(),
        codeflow_core::hooks::policy::PolicyLevel::Warn,
        &f.linked,
        &f.root,
    );
    assert!(findings
        .iter()
        .any(|finding| finding.rule == "git.hook_integrity"
            && finding.level == codeflow_core::hooks::policy::PolicyLevel::Block));
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            "git -c color.ui=never rebase -ix 'cargo test' HEAD~3",
            "git rebase --exe='cargo test' HEAD~3",
            "git rebase -x'cargo test' -x 'true' HEAD~3",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        for command in [
            "git rebase -ix 'rm -rf .claude' HEAD~1",
            "git rebase -rx 'rm -rf .claude' HEAD~1",
            "git rebase --exe 'rm -rf .claude' HEAD~1",
            "git rebase --ex='rm -rf .claude' HEAD~1",
            "git rebase -x'rm -rf .claude' HEAD~1",
            "git rebase -x 'true' -x 'rm -rf .claude' HEAD~1",
            "git rebase {--exec,'rm -rf .claude'} HEAD~1",
            "git rebase --exec",
            "git rebase -x",
            "git rebase --exec",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair3_runner_reserved_words_and_launchers() {
    let f = Fixture::new();
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            "! command git rebase -x 'cargo test' HEAD~3",
            "sh -c \"git rebase -x 'cargo test' HEAD~3\"",
            "env X=1 git bisect run ./test.sh",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        for command in [
            "! git rebase -x '! rm -rf .claude' HEAD~1",
            "if git bisect run rm -rf .claude; then true; fi",
            "while git rebase -x 'rm -rf .claude' HEAD~1; do true; done",
            "command git rebase -x 'env X=1 sh -c \"! rm -rf .claude\"' HEAD~1",
            "env X=1 arch -arm64 git rebase -ix 'rm -rf .claude' HEAD~1",
            "sh -c \"! git rebase -x 'rm -rf .claude' HEAD~1\"",
            "git rebase -x '$UNKNOWN_COMMAND' HEAD~1",
            "git -c alias.hidden='config core.bare true' rebase -x 'git hidden' HEAD~1",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair3_runner_locations() {
    let f = Fixture::new();
    git(&f.root, &["switch", "-c", "task/runner"]);
    let scratch = f.temp.path().join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    let root = shell_path(&f.root);
    let subdir = f.linked.join("src");
    std::fs::create_dir(&subdir).unwrap();
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            "git rebase -x 'cargo test' HEAD~3".to_string(),
            "D=..; cd \"$D\"; git rebase -x 'cargo test' HEAD~3".to_string(),
            format!(
                "git -c color.ui=never -C '{}' rebase -x 'cargo test' HEAD~3",
                shell_path(&f.linked)
            ),
        ] {
            let out = f.command(&subdir, hook, &command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        for command in [
            format!("git --git-dir='{root}/.git' --work-tree='{root}' rebase -x 'rm -rf .claude' HEAD~1"),
            format!("GIT_DIR='{root}/.git' GIT_WORK_TREE='{root}' git bisect run rm -rf .claude"),
            format!("git --git-dir='{root}/.git' rebase -x 'rm -rf .claude' HEAD~1"),
            format!("git --git-dir='{root}/.git' -c core.worktree='{}' rebase -x 'rm -rf .claude' HEAD~1", shell_path(&scratch)),
            format!("git --git-dir='{root}/.git' -c core.worktree='{}' rebase -x 'cargo test' HEAD~1", shell_path(&scratch)),
            format!("git --git-dir='{root}/.git' --work-tree='{}' rebase -x 'cargo test' HEAD~1", shell_path(&scratch)),
            "git -C \"$UNKNOWN\" rebase -x 'rm -rf .claude' HEAD~1".into(),
            "git -C \"$UNKNOWN\" status".into(),
            "git rebase -x 'rm -rf .claude' HEAD~1".into(),
        ] {
            let out = f.command(&scratch, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
    }
    // The administrative namespace has its existing git-guard authority
    // refusal; exec-guard can still resolve the literal runner checkout.
    let administrative = format!(
        "git --git-dir='{root}/.git/worktrees/w' --work-tree='{}' rebase -x 'cargo test' HEAD~3",
        shell_path(&f.linked)
    );
    let allowed = f.command(&subdir, "exec-guard", &administrative);
    assert_eq!(allowed.status.code(), Some(0), "{allowed:?}");
    let protected = f.command(&subdir, "git-guard", &administrative);
    assert_eq!(protected.status.code(), Some(2), "{protected:?}");
    assert!(
        String::from_utf8_lossy(&protected.stderr).contains("git.policy_authority"),
        "{protected:?}"
    );
}

#[test]
fn repair3_runner_aliases_and_program_names() {
    let f = Fixture::new();
    target_journey_in(&f, "submodule", "native");
    git(
        &f.root,
        &["config", "alias.runner", "rebase -x 'rm -rf .claude'"],
    );
    git(
        &f.root,
        &["config", "alias.safe-runner", "rebase -x 'cargo test'"],
    );
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            "git safe-runner HEAD~3",
            "git -c \"alias.once=rebase -x 'cargo test'\" once HEAD~3",
            "git-rebase -x 'cargo test' HEAD~3",
            "git-bisect run ./test.sh",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        for command in [
            "git submodule foreach 'git status --short'",
            "git submodule foreach --recursive 'cargo test'",
            "git submodule--helper foreach --quiet 'true'",
            "git-submodule foreach 'printf hello'",
        ] {
            let out = f.command(&f.root, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        for command in [
            "git runner HEAD~1",
            "git -c \"alias.runner=rebase -ix 'rm -rf .claude'\" runner HEAD~1",
            "git-rebase -ix 'rm -rf .claude' HEAD~1",
            "/usr/libexec/git-core/git-rebase -ix 'rm -rf .claude' HEAD~1",
            "git-rebase.exe -ix 'rm -rf .claude' HEAD~1",
            "git-bisect run rm -rf .claude",
            "git-submodule foreach 'rm -rf .claude'",
            "git submodule--helper foreach 'rm -rf .claude'",
        ] {
            let cwd = if command.contains("submodule") {
                &f.root
            } else {
                &f.linked
            };
            let out = f.command(cwd, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair3_runner_git_uses_submodule_branch() {
    let f = Fixture::new();
    target_journey_in(&f, "submodule", "native");
    let command = format!(
        "git -C '{}' submodule foreach 'git reset --hard'",
        shell_path(&f.root)
    );
    let out = f.command(&f.linked, "git-guard", &command);
    assert_eq!(out.status.code(), Some(2), "{command}: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("git.hard_reset_protected"),
        "{out:?}"
    );
    let sub = f.root.join("sub");
    git(&sub, &["switch", "-c", "task/sub"]);
    git(
        &sub,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            &shell_path(&f.temp.path().join("source")),
            "inner",
        ],
    );
    let direct = f.command(&f.linked, "git-guard", &command);
    assert!(
        !String::from_utf8_lossy(&direct.stderr).contains("git.hard_reset_protected"),
        "{direct:?}"
    );
    let recursive = command.replace("foreach", "foreach --recursive");
    let nested = f.command(&f.linked, "git-guard", &recursive);
    assert_eq!(nested.status.code(), Some(2), "{nested:?}");
    assert!(
        String::from_utf8_lossy(&nested.stderr).contains("git.hard_reset_protected"),
        "{nested:?}"
    );
}

#[test]
fn repair3_runner_filter_and_option_sources() {
    let f = Fixture::new();
    for hook in ["exec-guard", "git-guard"] {
        for option in [
            "--setup",
            "--env-filter",
            "--tree-filter",
            "--index-filter",
            "--parent-filter",
            "--msg-filter",
            "--commit-filter",
            "--tag-name-filter",
        ] {
            let command =
                format!("git filter-branch -f {option} 'git config core.hooksPath /dev/null' HEAD");
            let out = f.command(&f.linked, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        for command in [
            "git difftool -y -x 'rm -rf .claude' HEAD~1 HEAD",
            "git difftool --extcmd='rm -rf .claude' HEAD~1 HEAD",
            "git grep -O'rm -rf .claude' needle",
            "git grep --open-files-in-pager='rm -rf .claude' needle",
            "git clone -u 'rm -rf .claude' /source scratch",
            "git fetch --upload-pack='rm -rf .claude' /source",
            "git ls-remote --upload-pack='rm -rf .claude' /source",
            "git push --receive-pack='rm -rf .claude' /source task/w",
            "git send-pack --exec='rm -rf .claude' /source task/w",
            "git archive --exec='rm -rf .claude' --remote=/source HEAD",
            "git daemon --access-hook='rm -rf .claude' .",
            "git instaweb --httpd='rm -rf .claude' --start",
            "git instaweb -d 'rm -rf .claude' --start",
            "git send-email --to-cmd='rm -rf .claude' patch",
            "printf payload | xargs git difftool",
            "printf payload | xargs git-filter-branch",
            "printf payload | xargs git for-each-repo",
            "printf payload | xargs git grep -e pattern",
            "git grep \"$UNKNOWN\" docs",
            "git ls-remote 'ext::sh -c rm% -rf% .claude'",
            "git remote-ext fixture 'sh -c rm% -rf% .claude'",
            "git-remote-ext fixture 'sh -c rm% -rf% .claude'",
            "git clone -c 'filter.fixture.smudge=rm -rf .claude' /source scratch",
            "git clone --config='core.sshCommand=rm -rf .claude' /source scratch",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        // The exec floor owns unread xargs replacement before integrity runs.
        let replacement = f.command(
            &f.linked,
            hook,
            "printf payload | xargs -I{} git grep -O{} pattern",
        );
        assert_eq!(replacement.status.code(), Some(2), "{replacement:?}");
        let rule = if hook == "exec-guard" {
            "security.dangerous_commands"
        } else {
            "git.hook_integrity"
        };
        assert!(String::from_utf8_lossy(&replacement.stderr).contains(rule));
        for command in [
            "git filter-branch -f --index-filter 'true' HEAD",
            "git difftool -y -x 'diff' HEAD~1 HEAD",
            "git grep -Oless needle",
            "git grep needle --literal-filename",
            "git rebase -x ./test.sh HEAD~3",
            "git filter-branch --tree-filter ./test.sh HEAD",
            "printf README.md | xargs git grep pattern",
            "printf README.md | xargs git grep -e pattern --",
            "git grep '$UNKNOWN' README.md",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
    // Existing policy-authority checks already refuse command-local remote
    // changes. Read an out-of-band setting to exercise the runner itself.
    for (key, value) in [
        ("remote.fixture.url", "ext::sh -c rm% -rf% .claude"),
        ("url.ext::sh -c rm% -rf% .claude.insteadOf", "fixture"),
    ] {
        let configured = Fixture::new();
        git(&configured.root, &["config", key, value]);
        for hook in ["exec-guard", "git-guard"] {
            let out = configured.command(&configured.linked, hook, "git ls-remote fixture");
            assert!(refused(&out), "{hook} {key}: {out:?}");
        }
    }
}

#[test]
fn repair3_runner_configuration_and_environment_sources() {
    let f = Fixture::new();
    for hook in ["exec-guard", "git-guard"] {
        for (key, verb) in [
            ("mergetool.fixture.cmd", "mergetool -y -t fixture"),
            ("core.sshCommand", "ls-remote ssh://example.test/none"),
            ("diff.fixture.textconv", "diff"),
            ("filter.fixture.clean", "add README.md"),
            ("filter.fixture.smudge", "cat-file --filters HEAD:README.md"),
            (
                "diff.fixture.textconv",
                "cat-file --textconv HEAD:README.md",
            ),
            ("filter.fixture.clean", "hash-object README.md"),
            ("filter.fixture.smudge", "sparse-checkout reapply"),
            ("core.editor", "commit"),
            ("sequence.editor", "rebase -i HEAD~1"),
            ("core.pager", "log"),
            ("tar.fixture.command", "archive --format=fixture HEAD"),
        ] {
            let command = format!("git -c '{key}=rm -rf .claude' {verb}");
            let out = f.command(&f.linked, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        for (name, verb) in [
            ("GIT_SSH_COMMAND", "ls-remote ssh://example.test/none"),
            ("GIT_SSH", "ls-remote ssh://example.test/none"),
            ("GIT_EXTERNAL_DIFF", "diff"),
            ("GIT_PAGER", "log"),
            ("GIT_EDITOR", "commit"),
            ("GIT_SEQUENCE_EDITOR", "rebase -i HEAD~1"),
            ("GIT_DIFFTOOL_EXTCMD", "difftool -y HEAD~1 HEAD"),
        ] {
            let command = format!("{name}='rm -rf .claude' git {verb}");
            let out = f.command(&f.linked, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        for command in [
            "git -c core.pager=cat log",
            "git --version",
            "git -v",
            "C=never git --config-env=color.ui=C log -1",
            "EDITOR='rm -rf .claude' git config --get user.name",
            "git config alias.example status; git status",
            "GIT_SSH_COMMAND=ssh git status",
            "git -c core.fsmonitor=false status",
            "git -c core.editor=./editor.sh status",
            "git -c core.editor='git status --short' status",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        for command in [
            "EDITOR='rm -rf .claude' git config --edit",
            "C='rm -rf .claude' git --config-env=core.editor=C commit",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
    }
    // An out-of-band config write must be read on the next invocation.
    for hook in ["exec-guard", "git-guard"] {
        let out = f.command(
            &f.linked,
            hook,
            "git -c core.sshCommand='rm -rf .claude' submodule update --init",
        );
        assert!(refused(&out), "{hook}: {out:?}");
    }
    git(
        &f.root,
        &["config", "difftool.fixture.cmd", "rm -rf .claude"],
    );
    for hook in ["exec-guard", "git-guard"] {
        let out = f.command(&f.linked, hook, "git difftool -y -t fixture HEAD~1 HEAD");
        assert!(refused(&out), "{hook}: {out:?}");
    }
}

#[test]
fn repair3_runner_saved_rebase_instructions() {
    let f = Fixture::new();
    let repo = git2::Repository::open(&f.linked).unwrap();
    let state = repo.path().join("rebase-merge");
    std::fs::create_dir(&state).unwrap();
    let todo = state.join("git-rebase-todo");
    for hook in ["exec-guard", "git-guard"] {
        for line in [
            "exec rm -rf .claude\n",
            "x git config core.bare true\n",
            "unread-operation cargo test\n",
            "exec\u{a0}true\n",
        ] {
            std::fs::write(&todo, line).unwrap();
            let out = f.command(&f.linked, hook, "git rebase --continue");
            assert!(refused(&out), "{hook} {line}: {out:?}");
        }
        std::fs::write(&todo, "# next\nexec cargo test\nx ./test.sh\n").unwrap();
        let out = f.command(&f.linked, hook, "git rebase --continue");
        assert_eq!(out.status.code(), Some(0), "{hook}: {out:?}");
    }
}

#[test]
fn repair3_runner_for_each_repo_uses_configured_directories() {
    let f = Fixture::new();
    git(
        &f.root,
        &["config", "runner.repositories", &shell_path(&f.root)],
    );
    for hook in ["exec-guard", "git-guard"] {
        let out = f.command(
            &f.linked,
            hook,
            "git for-each-repo --config=runner.repositories config core.bare true",
        );
        assert!(refused(&out), "{hook}: {out:?}");
        let out = f.command(
            &f.linked,
            hook,
            "git for-each-repo --config runner.repositories status --short",
        );
        assert_eq!(out.status.code(), Some(0), "{hook}: {out:?}");
    }
    let out = f.command(
        &f.linked,
        "git-guard",
        "git for-each-repo --config=runner.repositories reset --hard",
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("git.hard_reset_protected"),
        "{out:?}"
    );
}

#[test]
fn repair3_runner_sources_changed_earlier_on_the_line() {
    let f = Fixture::new();
    let repo = git2::Repository::open(&f.linked).unwrap();
    let state = repo.path().join("rebase-merge");
    std::fs::create_dir(&state).unwrap();
    std::fs::write(state.join("git-rebase-todo"), "exec true\n").unwrap();
    let included = f.temp.path().join("included.conf");
    std::fs::write(&included, "[color]\nui = never\n").unwrap();
    git(&f.root, &["config", "include.path", &shell_path(&included)]);
    git(
        &f.root,
        &["config", "alias.fixture.fsmonitor", "rm -rf .claude"],
    );
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            format!(
                "printf 'exec rm -rf .claude\\n' > '{}'; git rebase --continue",
                shell_path(&state.join("git-rebase-todo"))
            ),
            "git config core.editor 'rm -rf .claude'; git commit".to_string(),
            "git config --rename-section alias.fixture core; git status".to_string(),
            format!(
                "printf '[core]\\npager = rm -rf .claude\\n' > '{}'; git log",
                shell_path(&included)
            ),
        ] {
            let out = f.command(&f.linked, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        let ordinary = f.command(
            &f.linked,
            hook,
            "printf note > notes.txt; git rebase --continue",
        );
        assert_eq!(ordinary.status.code(), Some(0), "{hook}: {ordinary:?}");
    }
}

#[test]
fn repair3_runner_inherits_command_configuration() {
    let f = Fixture::new();
    git(&f.root, &["config", "alias.hidden", "status"]);
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            "git -c alias.hidden='config core.bare true' rebase -x 'git hidden' HEAD~1",
            "git -c alias.hidden=config rebase -x 'git hidden core.bare true' HEAD~1",
            "git -c core.sshCommand='rm -rf .claude' rebase -x 'git ls-remote ssh://example.test/none' HEAD~1",
            "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=alias.hidden GIT_CONFIG_VALUE_0='config core.bare true' git rebase -x 'git hidden' HEAD~1",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        let out = f.command(
            &f.linked,
            hook,
            "git -c alias.hidden='status --short' rebase -x 'git hidden' HEAD~1",
        );
        assert_eq!(out.status.code(), Some(0), "{hook}: {out:?}");
        let out = f.command(&f.linked, hook, "git -c alias.hidden='config core.bare true' rebase -x 'git -c alias.hidden=status hidden' HEAD~1");
        assert_eq!(out.status.code(), Some(0), "{hook}: {out:?}");
    }
}

#[test]
fn repair3_runner_reads_each_configured_helper() {
    let f = Fixture::new();
    // Only submit hook payloads. Never execute a credential operation or helper.
    for hook in ["exec-guard", "git-guard"] {
        let command = "git -c credential.helper= -c 'credential.helper=!rm -rf .claude' -c credential.helper=true ls-remote https://example.test/none";
        let out = f.command(&f.linked, hook, command);
        assert!(refused(&out), "{hook} {command}: {out:?}");
        let cleared = "git -c 'credential.helper=!rm -rf .claude' -c credential.helper= ls-remote https://example.test/none";
        let out = f.command(&f.linked, hook, cleared);
        assert_eq!(out.status.code(), Some(0), "{hook} {cleared}: {out:?}");
    }
}

#[test]
fn repair3_runner_rejects_non_unicode_command_environment() {
    let f = Fixture::new();
    #[cfg(unix)]
    let unread = {
        use std::os::unix::ffi::OsStringExt as _;
        std::ffi::OsString::from_vec(vec![0xff])
    };
    #[cfg(windows)]
    let unread = {
        use std::os::windows::ffi::OsStringExt as _;
        std::ffi::OsString::from_wide(&[0xd800])
    };
    for hook in ["exec-guard", "git-guard"] {
        let out = run_with_stdin(
            codeflow()
                .args(["hook", hook, "--contract", "3"])
                .current_dir(&f.linked)
                .env("HOME", f.temp.path())
                .env("USERPROFILE", f.temp.path())
                .env("GIT_SSH_COMMAND", &unread),
            &guard_payload("git ls-remote ssh://example.test/none", &f.linked),
        );
        assert!(refused(&out), "{hook}: {out:?}");
    }
}

#[test]
fn repair3_runner_rejects_non_unicode_transport() {
    let f = Fixture::new();
    let config = f.root.join(".git/config");
    let mut bytes = std::fs::read(&config).unwrap();
    bytes.extend_from_slice(b"\n[remote \"fixture\"]\nurl = ext::sh -c rm% -rf% .claude\xff\n");
    std::fs::write(config, bytes).unwrap();
    for hook in ["exec-guard", "git-guard"] {
        let out = f.command(&f.linked, hook, "git ls-remote fixture");
        assert!(refused(&out), "{hook}: {out:?}");
    }
}

#[test]
fn repair3_runner_transitive_callbacks() {
    let f = Fixture::new();
    let mut missed = Vec::new();
    for hook in ["exec-guard", "git-guard"] {
        for (key, verb) in [
            ("core.editor", "cherry-pick -e HEAD"),
            ("core.editor", "revert HEAD"),
            ("core.editor", "add -e README.md"),
            ("core.editor", "reset -p"),
            ("core.editor", "checkout -p"),
            ("core.editor", "checkout -mp"),
            ("core.editor", "restore -mp README.md"),
            ("core.editor", "restore -p README.md"),
            ("core.editor", "stash -p"),
            ("core.editor", "replace -e HEAD"),
            ("core.editor", "send-email --annotate patch"),
            ("core.editor", "pull origin main"),
            ("core.fsmonitor", "pull origin main"),
            ("filter.fixture.clean", "pull origin main"),
            ("gpg.program", "rebase --gpg-sign HEAD~1"),
            ("gpg.program", "cherry-pick -S HEAD"),
            ("gpg.program", "revert -S HEAD"),
            ("gpg.program", "am -S patch"),
        ] {
            let command = format!("git -c '{key}=rm -rf .claude' {verb}");
            let out = f.command(&f.linked, hook, &command);
            if !refused(&out) {
                missed.push(format!("{hook} {command}: {out:?}"));
            }
        }
        let out = f.command(
            &f.linked,
            hook,
            "GIT_EDITOR='rm -rf .claude' git add -e README.md",
        );
        if !refused(&out) {
            missed.push(format!("{hook} inherited editor: {out:?}"));
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
    for hook in ["exec-guard", "git-guard"] {
        for verb in [
            "reset --soft HEAD",
            "reset --hard HEAD",
            "checkout HEAD",
            "add README.md",
            "stash list",
            "replace -l",
        ] {
            let command = format!("git -c 'core.editor=rm -rf .claude' {verb}");
            let out = f.command(&f.linked, hook, &command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair4_revision_words_with_editor_and_pager() {
    let f = Fixture::new();
    git(&f.root, &["config", "core.editor", "true"]);
    git(&f.root, &["config", "core.pager", "cat"]);
    for hook in ["git-guard", "exec-guard"] {
        for command in [
            "diff HEAD~1",
            "diff HEAD^ -- README.md",
            "log HEAD~2..HEAD",
            "log -1 HEAD~1",
            "show HEAD^",
            "show HEAD~1:README.md",
            "show feature/café~1",
            "grep HEAD~1",
            "log @{u}..",
            "log HEAD@{1}...HEAD",
            "log HEAD@{yesterday}",
            "log @{upstream}",
            "cherry-pick HEAD~1",
            "cherry-pick abc^..def",
            "revert HEAD~1",
            "merge HEAD~1",
            "commit --fixup HEAD~1",
            "tag -a v1 -m msg HEAD~1",
            "notes add -m x HEAD~1",
        ] {
            let out = run_with_stdin(
                codeflow()
                    .args(["hook", hook, "--contract", "3"])
                    .current_dir(&f.linked)
                    .env("HOME", f.temp.path())
                    .env("USERPROFILE", f.temp.path())
                    .env("GIT_EDITOR", "true"),
                &guard_payload(&format!("git {command}"), &f.linked),
            );
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair4_foreach_preserves_argument_boundaries() {
    let f = Fixture::new();
    target_journey_in(&f, "submodule", "native");
    for hook in ["git-guard", "exec-guard"] {
        for command in [
            "git submodule foreach sh -c 'rm -rf .claude'",
            "git submodule--helper foreach sh -c 'git config core.bare true'",
            "git submodule foreach 'rm -rf \"$toplevel/.claude\"'",
            "git submodule foreach 'rm -rf \"$sm_path/.claude\"'",
        ] {
            let out = f.command(&f.root, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        for command in [
            "git submodule foreach printf '%s' '; rm -rf .claude'",
            "git submodule foreach sh -c 'printf hello'",
            "git submodule foreach 'git status --short'",
        ] {
            let out = f.command(&f.root, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair4_index_callbacks_apply_to_every_verb() {
    let f = Fixture::new();
    for hook in ["git-guard", "exec-guard"] {
        for key in [
            "core.fsmonitor",
            "filter.fixture.clean",
            "filter.fixture.smudge",
            "filter.fixture.process",
            "merge.fixture.driver",
        ] {
            for verb in [
                "ls-files",
                "describe --dirty",
                "grep pattern",
                "rm README.md",
                "mv README.md moved.md",
                "diff-files",
                "clean -n",
            ] {
                let command = format!("git -c '{key}=rm -rf .claude' {verb}");
                let out = f.command(&f.linked, hook, &command);
                assert!(refused(&out), "{hook} {command}: {out:?}");
            }
            let out = f.command(&f.linked, hook, &format!("git -c {key}=true ls-files"));
            assert_eq!(out.status.code(), Some(0), "{hook} {key}: {out:?}");
        }
    }
}

#[test]
fn repair4_option_bodies_use_starting_directory() {
    let f = Fixture::new();
    let subdir = f.linked.join("src");
    std::fs::create_dir(&subdir).unwrap();
    for hook in ["git-guard", "exec-guard"] {
        for verb in [
            "grep -O'rm -rf ../.claude' pattern",
            "send-email --to-cmd='rm -rf ../.claude' patch",
            "difftool -x 'rm -rf ../.claude' HEAD~1 HEAD",
        ] {
            for (cwd, command) in [
                (&subdir, format!("git {verb}")),
                (&f.linked, format!("git -C src {verb}")),
            ] {
                let out = f.command(cwd, hook, &command);
                assert!(refused(&out), "{hook} {command}: {out:?}");
            }
        }
        let out = f.command(&subdir, hook, "git grep -Ocat pattern");
        assert_eq!(out.status.code(), Some(0), "{hook}: {out:?}");
    }
}

#[test]
fn repair4_bare_runner_has_unknown_work_tree() {
    let f = Fixture::new();
    for hook in ["git-guard", "exec-guard"] {
        let command = "git --bare rebase -x true HEAD~1";
        let out = f.command(&f.linked, hook, command);
        assert!(refused(&out), "{hook} {command}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("bare"),
            "{out:?}"
        );
        let out = f.command(&f.linked, hook, "git rebase -x true HEAD~1");
        assert_eq!(out.status.code(), Some(0), "{hook}: {out:?}");
    }
}

#[test]
fn repair4_pager_abbreviations() {
    let f = Fixture::new();
    for hook in ["git-guard", "exec-guard"] {
        for verb in [
            "config --lis",
            "config --get-reg remote",
            "rebase --show-current-p",
            "am --show-current-p=diff",
        ] {
            let command = format!("git -c 'core.pager=rm -rf .claude' {verb}");
            let out = f.command(&f.linked, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn repair4_ordinary_flags_keep_command_values_checked() {
    let f = Fixture::new();
    for hook in ["git-guard", "exec-guard"] {
        for command in [
            "git pull -r",
            "git pull --no-edit",
            "git pull --squash",
            "git pull --no-commit",
            "git pull -S",
            "git pull --gpg-sign",
            "git pull --signoff",
            "git pull --stat",
            "git pull --verify-signatures",
            "git pull --allow-unrelated-histories",
            "git fetch -k",
            "git fetch --porcelain",
            "git fetch --no-auto-maintenance",
            "git fetch --refetch",
            "git ls-remote -h",
            "git ls-remote --sort=refname",
            "git clone --reject-shallow /source scratch/clone",
            "git clone -4 /source scratch/clone",
            "git clone --ipv4 --no-remote-submodules /source scratch/clone",
            "git grep -rn pattern",
            "git grep -p -W --column --null --full-name pattern",
            "git grep --no-color --max-count=1 --only-matching pattern",
            "git grep --show-function --function-context --files-without-match pattern",
            "git grep --extended-regexp pattern",
            "git difftool -O orderfile",
            "git mergetool -O orderfile",
            "git rebase --unknown-runner-option HEAD~1",
            "git rebase -zx true HEAD~1",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        for command in [
            "git rebase --unknown-runner-option -x 'rm -rf .claude' HEAD~1",
            "git grep -rnO'rm -rf .claude' pattern",
            "git rebase --exec",
            "git rebase --exec=$UNKNOWN HEAD~1",
            "git fetch --porcelain --upload-pack='rm -rf .claude'",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        let out = f.command(&f.linked, hook, "git rebase --exec");
        let message = String::from_utf8_lossy(&out.stderr);
        assert!(
            message.contains("--exec") && message.contains("command"),
            "{message}"
        );
    }
}

#[test]
fn repair4_external_git_program_resolution() {
    let f = Fixture::new();
    let programs = f.temp.path().join("programs");
    std::fs::create_dir(&programs).unwrap();
    // Install a real executable in this disposable fixture. The guard must
    // resolve it without executing it or treating its bytes as inline shell.
    let name = format!("git-codeflow-extension{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(env!("CARGO_BIN_EXE_codeflow"), programs.join(&name)).unwrap();
    let search = std::env::join_paths(
        std::iter::once(programs.clone())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    for hook in ["git-guard", "exec-guard"] {
        for source in ["PATH", "GIT_EXEC_PATH"] {
            let mut process = codeflow();
            process
                .args(["hook", hook, "--contract", "3"])
                .current_dir(&f.linked)
                .env("HOME", f.temp.path());
            if source == "PATH" {
                process.env("PATH", &search);
            } else {
                process.env("GIT_EXEC_PATH", &programs);
            }
            let out = run_with_stdin(
                &mut process,
                &guard_payload("git codeflow-extension --version", &f.linked),
            );
            assert_eq!(out.status.code(), Some(0), "{hook} {source}: {out:?}");
        }
        for command in [
            format!(
                "git --exec-path='{}' codeflow-extension --version",
                shell_path(&programs)
            ),
            format!(
                "GIT_EXEC_PATH='{}' git codeflow-extension --version",
                shell_path(&programs)
            ),
        ] {
            let out = f.command(&f.linked, hook, &command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        let out = f.command(&f.linked, hook, "git nosuchcmd");
        assert!(refused(&out), "{out:?}");
        let message = String::from_utf8_lossy(&out.stderr);
        assert!(
            message.contains("git-nosuchcmd")
                && message.contains("PATH")
                && message.contains("exec-path"),
            "{message}"
        );
    }
}

#[test]
fn repair4_pager_configuration_and_environment_routes() {
    let f = Fixture::new();
    for hook in ["git-guard", "exec-guard"] {
        for command in [
            "GIT_PAGER=cat git config --list",
            "PAGER=cat git config --get-regexp remote",
        ] {
            let out = f.command(f.temp.path(), hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        let command = "GIT_PAGER='rm -rf repo/.claude' git config --list";
        let out = f.command(f.temp.path(), hook, command);
        assert!(refused(&out), "{hook} {command}: {out:?}");
        let command = "GIT_PAGER=cat git --git-dir=missing config --list";
        let out = f.command(f.temp.path(), hook, command);
        assert_eq!(out.status.code(), Some(2), "{hook} {command}: {out:?}");
        assert!(
            refused(&out) || String::from_utf8_lossy(&out.stderr).contains("git.policy_authority"),
            "{hook} {command}: {out:?}"
        );
        for verb in [
            "stash list",
            "stash show",
            "config --list",
            "config --get-regexp remote",
            "config list",
            "config get --regexp remote",
            "config get user.name",
            "config --get-all remote.origin.url",
            "config --get-urlmatch http https://example.test",
            "format-patch --stdout HEAD~1",
            "whatchanged",
            "annotate README.md",
            "range-diff HEAD~2 HEAD~1 HEAD",
            "am --show-current-patch",
            "rebase --show-current-patch",
            "submodule summary",
        ] {
            for source in [
                "git -c 'core.pager=rm -rf .claude'",
                "GIT_PAGER='rm -rf .claude' git",
                "PAGER='rm -rf .claude' git",
            ] {
                let command = format!("{source} {verb}");
                let out = f.command(&f.linked, hook, &command);
                assert!(refused(&out), "{hook} {command}: {out:?}");
            }
            let out = f.command(&f.linked, hook, &format!("git -c core.pager=cat {verb}"));
            assert_eq!(out.status.code(), Some(0), "{hook} {verb}: {out:?}");
        }
        for command in [
            "git -c 'core.pager=rm -rf .claude' status",
            "git -c 'core.pager=rm -rf .claude' config --get user.name",
            "git -c 'core.pager=rm -rf .claude' --no-pager stash list",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
}
