//! Real hook invocations after external corruption of checkout identity.
use super::{codeflow, enforced_repo, git, guard_payload, run_with_stdin, shell_path};
use std::path::{Path, PathBuf};
use std::process::Output;

struct Fixture {
    temp: tempfile::TempDir,
    root: PathBuf,
    linked: PathBuf,
    admin: PathBuf,
    other: PathBuf,
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
        let admin = root.join(".git/worktrees/w");
        let other = temp.path().join("other");
        std::fs::create_dir(&other).unwrap();
        git(&other, &["init", "-b", "main"]);
        Self {
            temp,
            root,
            linked,
            admin,
            other,
        }
    }

    fn command(&self, cwd: &Path, guard: &str, command: &str) -> Output {
        run_with_stdin(
            codeflow()
                .args(["hook", guard])
                .current_dir(cwd)
                .env("HOME", self.temp.path())
                .env("USERPROFILE", self.temp.path()),
            &guard_payload(command, cwd),
        )
    }

    fn edit(&self, cwd: &Path, path: &Path) -> Output {
        run_with_stdin(
            codeflow()
                .args(["hook", "edit-guard"])
                .current_dir(cwd)
                .env("HOME", self.temp.path())
                .env("USERPROFILE", self.temp.path()),
            &serde_json::json!({"tool_name": "Write", "cwd": cwd,
                "tool_input": {"file_path": path, "content": "{}"}})
            .to_string(),
        )
    }

    fn writes_refuse(&self, cwd: &Path, path: &Path, context: &str, wrong: &mut Vec<String>) {
        let p = shell_path(path);
        for (guard, command) in [
            ("git-guard", format!("printf x > '{p}'")),
            ("exec-guard", format!(r#"python3 -c 'open("{p}", "w")'"#)),
        ] {
            expect(
                &self.command(cwd, guard, &command),
                2,
                &format!("{context}: {guard}: {command}"),
                wrong,
            );
        }
        expect(
            &self.edit(cwd, path),
            2,
            &format!("{context}: edit {}", path.display()),
            wrong,
        );
    }

    fn corrupt_refuses(&self, cwd: &Path, context: &str, wrong: &mut Vec<String>) {
        for path in [
            self.root.join(".codeflow/policy.json"),
            self.root.join(".git/hooks/pre-commit"),
            self.linked.join(".claude/settings.json"),
            self.linked.join(".codeflow/policy.json"),
        ] {
            self.writes_refuse(cwd, &path, context, wrong);
        }
        // Ordinary commands must fail during discovery too, before selecting
        // any policy from the forged checkout.
        for guard in ["git-guard", "exec-guard"] {
            expect(
                &self.command(cwd, guard, "printf hello"),
                2,
                &format!("{context}: discovery {guard}"),
                wrong,
            );
        }
        expect(
            &self.edit(cwd, &cwd.join("notes.txt")),
            2,
            &format!("{context}: discovery edit"),
            wrong,
        );
    }
}

fn expect(output: &Output, code: i32, context: &str, wrong: &mut Vec<String>) {
    if output.status.code() != Some(code) {
        wrong.push(format!("{context}: expected {code}: {output:?}"));
    }
}

fn link(target: &Path, path: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, path).unwrap();
    #[cfg(windows)]
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, path).unwrap();
    } else {
        std::os::windows::fs::symlink_file(target, path).unwrap();
    }
}

#[test]
fn checkout_identity_commondir_is_an_integrity_file() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for cwd in [&f.root, &f.linked] {
        f.writes_refuse(cwd, &f.admin.join("commondir"), "commondir", &mut wrong);
        let command = format!(
            "ln -sfn '{}' '{}'",
            shell_path(&f.other.join(".git")),
            shell_path(&f.admin.join("commondir"))
        );
        expect(
            &f.command(cwd, "git-guard", &command),
            2,
            &command,
            &mut wrong,
        );
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_forged_common_directory_fails_before_policy() {
    let mut wrong = Vec::new();
    for mode in [
        "absolute",
        "relative",
        "missing",
        "symlink-text",
        "symlink-file",
    ] {
        let f = Fixture::new();
        let common = f.admin.join("commondir");
        match mode {
            "relative" => std::fs::write(&common, "../../../../other/.git\n").unwrap(),
            "missing" => std::fs::write(&common, "../../../../missing/.git\n").unwrap(),
            "symlink-text" => {
                let alias = f.temp.path().join("alias");
                link(&f.other.join(".git"), &alias);
                std::fs::write(&common, format!("{}\n", alias.display())).unwrap();
            }
            "symlink-file" => {
                let text = f.temp.path().join("common-text");
                std::fs::write(&text, format!("{}\n", f.other.join(".git").display())).unwrap();
                std::fs::remove_file(&common).unwrap();
                link(&text, &common);
            }
            _ => std::fs::write(&common, format!("{}\n", f.other.join(".git").display())).unwrap(),
        }
        f.corrupt_refuses(&f.linked, mode, &mut wrong);
        f.corrupt_refuses(&f.root, mode, &mut wrong);
        for cwd in [&f.root, &f.linked] {
            for guard in ["git-guard", "exec-guard", "edit-guard"] {
                for command in ["git worktree list", "git worktree prune"] {
                    expect(
                        &f.command(cwd, guard, command),
                        0,
                        "corrupt common recovery",
                        &mut wrong,
                    );
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_main_checks_each_registration() {
    let mut wrong = Vec::new();
    for mode in [
        "absolute",
        "relative",
        "missing",
        "missing-leaf",
        "symlink-real",
        "symlink-marker-real",
        "symlink-other",
        "symlink-local-other",
    ] {
        let f = Fixture::new();
        let registration = f.admin.join("gitdir");
        let text = match mode {
            "relative" => "../../../../other/.git".to_string(),
            "missing" => "../../../../missing/.git".to_string(),
            "missing-leaf" => "missing".to_string(),
            "symlink-real" | "symlink-marker-real" | "symlink-other" | "symlink-local-other" => {
                let alias = if mode == "symlink-marker-real" {
                    let decoy = f.temp.path().join("decoy");
                    std::fs::create_dir(&decoy).unwrap();
                    decoy.join(".git")
                } else if mode == "symlink-local-other" {
                    f.linked.join("alias")
                } else {
                    f.temp.path().join("alias")
                };
                let target = if matches!(mode, "symlink-real" | "symlink-marker-real") {
                    &f.linked
                } else {
                    &f.other
                };
                link(&target.join(".git"), &alias);
                alias.display().to_string()
            }
            _ => f.other.join(".git").display().to_string(),
        };
        std::fs::write(registration, format!("{text}\n")).unwrap();
        if matches!(mode, "missing" | "missing-leaf") {
            f.corrupt_refuses(&f.linked, mode, &mut wrong);
            for guard in ["git-guard", "exec-guard"] {
                expect(
                    &f.command(&f.root, guard, "printf hello"),
                    0,
                    mode,
                    &mut wrong,
                );
            }
            expect(
                &f.edit(&f.root, &f.root.join("notes.txt")),
                0,
                mode,
                &mut wrong,
            );
        } else {
            f.corrupt_refuses(&f.root, mode, &mut wrong);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_replaced_marker_cannot_move_the_checkout() {
    let mut wrong = Vec::new();
    for mode in ["gitfile", "gitfile-config", "directory-config", "symlink"] {
        let f = Fixture::new();
        let marker = f.linked.join(".git");
        std::fs::remove_file(&marker).unwrap();
        match mode {
            "directory-config" => {
                git(&f.linked, &["init", "-b", "main"]);
                git(
                    &f.linked,
                    &["config", "core.worktree", &shell_path(&f.other)],
                );
            }
            "symlink" => link(&f.other.join(".git"), &marker),
            _ => {
                if mode == "gitfile-config" {
                    git(
                        &f.other,
                        &["config", "core.worktree", &shell_path(&f.other)],
                    );
                }
                std::fs::write(
                    &marker,
                    format!("gitdir: {}\n", f.other.join(".git").display()),
                )
                .unwrap();
            }
        }
        f.corrupt_refuses(&f.linked, mode, &mut wrong);
        if mode == "symlink" {
            f.corrupt_refuses(
                &f.linked.canonicalize().unwrap(),
                "canonical symlink cwd",
                &mut wrong,
            );
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_home_code_paths_include_the_own_checkout() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for cwd in [&f.root, &f.linked] {
        for command in [
            r#"python3 -c 'import os,shutil; shutil.rmtree(os.path.expanduser("~/repo/.worktrees/w"))'"#,
            r#"python3 -c 'open("~/repo/.worktrees/w", "w")'"#,
            r#"python3 -c 'open(os.path.join(os.path.expanduser("~"), "repo/.claude/settings.json"), "w")'"#,
            r#"python3 -c 'open(os.path.expanduser("~//repo/.claude/settings.json"), "w")'"#,
            r#"python3 -c 'open(os.path.expanduser("~///repo/.worktrees/w"), "w")'"#,
        ] {
            expect(
                &f.command(cwd, "exec-guard", command),
                2,
                command,
                &mut wrong,
            );
        }
        #[cfg(windows)]
        for command in [
            r#"python3 -c 'open(os.path.expanduser(r"~\repo\.claude\settings.json"), "w")'"#,
            r#"python3 -c 'open(os.path.expanduser(r"~\\repo\.worktrees\w"), "w")'"#,
        ] {
            expect(
                &f.command(cwd, "exec-guard", command),
                2,
                command,
                &mut wrong,
            );
        }
        for guard in ["git-guard", "exec-guard"] {
            for command in [
                "python3 tool.py .",
                "python3 -m pytest .",
                "rm build/*.o",
                "git worktree list",
                r#"python3 -c 'print("~")'"#,
                r#"python3 -c 'open(os.path.expanduser("~")+"/."+"zsh"+"rc", "a")'"#,
                r#"node -e 'require("fs").rmSync(require("os").homedir()+"/repo/.worktrees/w/.claude/settings.json")'"#,
            ] {
                expect(&f.command(cwd, guard, command), 0, command, &mut wrong);
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_valid_pointers_and_worktree_lifecycle_stay_usable() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    // Native and relative registrations, and Git's CRLF/trailing-space
    // readings, keep the same protection when viewed from either checkout.
    for registration in [
        format!("{}\r\n", f.linked.join(".git").display()),
        format!("{} \n", f.linked.join(".git").display()),
        "../../../.worktrees/w/.git\n".into(),
    ] {
        std::fs::write(f.admin.join("gitdir"), registration).unwrap();
        std::fs::write(f.admin.join("commondir"), "../..\r\n").unwrap();
        for cwd in [&f.root, &f.linked] {
            expect(
                &f.command(cwd, "git-guard", "git worktree list"),
                0,
                "valid registration",
                &mut wrong,
            );
            f.writes_refuse(
                cwd,
                &f.linked.join(".claude/settings.json"),
                "valid registration",
                &mut wrong,
            );
        }
    }
    std::fs::write(
        f.admin.join("gitdir"),
        format!("{}\n", f.linked.join(".git").display()),
    )
    .unwrap();
    std::fs::write(f.admin.join("commondir"), "../..\n").unwrap();
    let added = f.root.join(".worktrees/new");
    let moved = f.root.join(".worktrees/moved");
    for (command, args) in [
        (
            "git worktree add .worktrees/new -b task/new",
            vec!["worktree", "add", ".worktrees/new", "-b", "task/new"],
        ),
        (
            "git worktree move .worktrees/new .worktrees/moved",
            vec!["worktree", "move", ".worktrees/new", ".worktrees/moved"],
        ),
    ] {
        for guard in ["git-guard", "exec-guard"] {
            expect(&f.command(&f.root, guard, command), 0, command, &mut wrong);
        }
        git(&f.root, &args);
        let cwd = if moved.exists() { &moved } else { &added };
        f.writes_refuse(
            cwd,
            &f.root.join(".codeflow/policy.json"),
            "lifecycle main",
            &mut wrong,
        );
        f.writes_refuse(
            &f.root,
            &cwd.join(".claude/settings.json"),
            "lifecycle linked",
            &mut wrong,
        );
    }
    for guard in ["git-guard", "exec-guard"] {
        expect(
            &f.command(&f.root, guard, "git worktree remove .worktrees/moved"),
            0,
            "remove",
            &mut wrong,
        );
    }
    git(&f.root, &["worktree", "remove", ".worktrees/moved"]);
    expect(
        &f.command(&f.linked, "git-guard", "git worktree list"),
        0,
        "after remove",
        &mut wrong,
    );
    // A symlink to the correct administrative directory is a valid marker.
    std::fs::remove_file(f.linked.join(".git")).unwrap();
    link(&f.admin, &f.linked.join(".git"));
    f.writes_refuse(
        &f.linked,
        &f.root.join(".codeflow/policy.json"),
        "valid symlink marker",
        &mut wrong,
    );
    for cwd in [&f.linked, &f.linked.canonicalize().unwrap()] {
        for guard in ["git-guard", "exec-guard"] {
            expect(
                &f.command(cwd, guard, "python3 -m pytest ."),
                0,
                "valid symlink marker",
                &mut wrong,
            );
        }
        expect(
            &f.edit(cwd, &cwd.join("notes.txt")),
            0,
            "valid symlink marker edit",
            &mut wrong,
        );
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_administrative_case_follows_the_filesystem() {
    let f = Fixture::new();
    let probe = f.temp.path().join("case-probe");
    std::fs::write(&probe, "case").unwrap();
    let folds_case = f.temp.path().join("CASE-PROBE").exists();
    let administrative_parent = f.root.join(".git/worktrees");
    let intermediate = f.root.join(".git/renaming-worktrees");
    std::fs::rename(&administrative_parent, &intermediate).unwrap();
    std::fs::rename(&intermediate, f.root.join(".git/WORKTREES")).unwrap();
    let mut wrong = Vec::new();
    // On a folding filesystem the original pointer still resolves to the
    // same administrative directory. Else it is broken and must fail closed.
    for guard in ["git-guard", "exec-guard"] {
        expect(
            &f.command(&f.linked, guard, "python3 -m pytest ."),
            if folds_case { 0 } else { 2 },
            "administrative directory case",
            &mut wrong,
        );
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_missing_sibling_allows_status_and_ordinary_work() {
    let f = Fixture::new();
    std::fs::remove_dir_all(&f.linked).unwrap();
    let mut wrong = Vec::new();
    let output = codeflow()
        .arg("status")
        .current_dir(&f.root)
        .output()
        .unwrap();
    expect(
        &output,
        0,
        "status can diagnose a missing sibling",
        &mut wrong,
    );
    for guard in ["git-guard", "exec-guard"] {
        expect(
            &f.command(&f.root, guard, "printf hello"),
            0,
            "ordinary work with a prunable sibling",
            &mut wrong,
        );
    }
    expect(
        &f.edit(&f.root, &f.root.join("notes.txt")),
        0,
        "ordinary edit with a prunable sibling",
        &mut wrong,
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_administrative_writes_refuse_before_registration() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    let planted = f.root.join(".git/worktrees/planted");
    for cwd in [&f.root, &f.linked] {
        for path in [
            planted.clone(),
            planted.join("gitdir"),
            planted.join("commondir"),
            planted.join("HEAD"),
            f.root.join(".git/HEAD"),
            f.admin.join("HEAD"),
        ] {
            f.writes_refuse(cwd, &path, "administrative write", &mut wrong);
        }
        for command in [
            format!("mkdir -p '{}'", shell_path(&planted)),
            format!("touch '{}'", shell_path(&planted.join("new"))),
            format!(
                "mkdir -p '{}' && printf '%s\\n' '{}' > '{}'",
                shell_path(&planted),
                shell_path(&f.other.join(".git")),
                shell_path(&planted.join("gitdir"))
            ),
        ] {
            expect(
                &f.command(cwd, "git-guard", &command),
                2,
                &command,
                &mut wrong,
            );
        }
        for command in [
            "git status",
            "git switch task/next",
            "git checkout task/next",
            "git worktree add ../next",
            "git worktree move ../next ../moved",
            "git worktree remove ../next",
            "git worktree lock ../next",
            "git worktree unlock ../next",
        ] {
            for guard in ["git-guard", "exec-guard", "edit-guard"] {
                expect(&f.command(cwd, guard, command), 0, command, &mut wrong);
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_complete_code_roots_refuse_but_project_arguments_allow() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for cwd in [&f.root, &f.linked] {
        let relative = if cwd == &f.root { "../repo" } else { "../.." };
        for path in [
            shell_path(cwd),
            ".".into(),
            relative.into(),
            shell_path(&f.root),
            shell_path(&f.linked),
        ] {
            let command = format!(r#"python3 -c 'import shutil; shutil.rmtree("{path}")'"#);
            expect(
                &f.command(cwd, "exec-guard", &command),
                2,
                &command,
                &mut wrong,
            );
        }
        for command in [
            "python3 tool.py .",
            "python3 -m pytest .",
            "python3 -c 'print(\"~\")'",
            "node -e 'os.homedir()+\"/repo/.worktrees/w\"'",
            "python3 -c 'os.path.expanduser(\"~\")+\"/.\"+\"zsh\"+\"rc\"'",
        ] {
            expect(
                &f.command(cwd, "exec-guard", command),
                0,
                command,
                &mut wrong,
            );
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_recovery_is_a_single_git_command() {
    let f = Fixture::new();
    std::fs::write(
        f.admin.join("gitdir"),
        format!("{}\n", f.other.join(".git").display()),
    )
    .unwrap();
    let mut wrong = Vec::new();
    for cwd in [&f.root, &f.linked] {
        for guard in ["git-guard", "exec-guard", "edit-guard"] {
            for command in ["git worktree list", "git worktree prune"] {
                expect(&f.command(cwd, guard, command), 0, command, &mut wrong);
            }
        }
        for guard in ["git-guard", "exec-guard"] {
            for command in [
                "git worktree list > notes.txt",
                "git worktree prune; printf hello",
                "git worktree list $(printf hello)",
                "git -c core.worktree=elsewhere worktree prune",
                "git worktree remove --force ../other",
                "git worktree list\nprintf hello",
                "git worktree prune | cat",
            ] {
                expect(&f.command(cwd, guard, command), 2, command, &mut wrong);
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_absent_registration_keeps_spelling_and_recovers() {
    let mut wrong = Vec::new();
    for removal in ["prune", "remove"] {
        let f = Fixture::new();
        // Simulate an external rm -rf .worktrees/w, then drive recovery
        // through the built hooks before executing Git's operation.
        std::fs::remove_dir_all(&f.linked).unwrap();
        f.writes_refuse(
            &f.root,
            &f.linked.join("notes.txt"),
            "absent spelling",
            &mut wrong,
        );
        f.writes_refuse(
            &f.root,
            &f.linked.join(".claude/settings.json"),
            "absent checkout",
            &mut wrong,
        );
        f.writes_refuse(&f.root, &f.admin.join("gitdir"), "absent admin", &mut wrong);
        for guard in ["git-guard", "exec-guard"] {
            expect(
                &f.command(&f.root, guard, "printf hello"),
                0,
                "ordinary work before recovery",
                &mut wrong,
            );
        }
        expect(
            &f.edit(&f.root, &f.root.join("notes.txt")),
            0,
            "ordinary edit",
            &mut wrong,
        );
        let command = if removal == "prune" {
            "git worktree prune".into()
        } else {
            format!("git worktree remove --force '{}'", shell_path(&f.linked))
        };
        for guard in ["git-guard", "exec-guard", "edit-guard"] {
            expect(
                &f.command(&f.root, guard, &command),
                0,
                &command,
                &mut wrong,
            );
        }
        if removal == "prune" {
            git(&f.root, &["worktree", "prune"]);
        } else {
            git(
                &f.root,
                &["worktree", "remove", "--force", &shell_path(&f.linked)],
            );
        }
        assert!(
            !f.admin.exists(),
            "Git must remove the prunable registration"
        );
        for guard in ["git-guard", "exec-guard"] {
            expect(
                &f.command(&f.root, guard, "printf hello"),
                0,
                "ordinary work after recovery",
                &mut wrong,
            );
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_recovery_does_not_expand_registered_spellings() {
    let f = Fixture::new();
    let absent = f.temp.path().join("$(printf evil)");
    std::fs::write(
        f.admin.join("gitdir"),
        format!("{}\n", absent.join(".git").display()),
    )
    .unwrap();
    let command = format!("git worktree remove --force \"{}\"", shell_path(&absent));
    let mut wrong = Vec::new();
    for guard in ["git-guard", "exec-guard"] {
        expect(
            &f.command(&f.root, guard, &command),
            2,
            "recovery cannot expand its target",
            &mut wrong,
        );
    }
    let literal = format!("git worktree remove --force '{}'", shell_path(&absent));
    for guard in ["git-guard", "exec-guard", "edit-guard"] {
        expect(
            &f.command(&f.root, guard, &literal),
            0,
            "literal recovery target",
            &mut wrong,
        );
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn checkout_identity_force_recovery_proves_the_command_target() {
    let f = Fixture::new();
    let child = f.other.join("child");
    std::fs::create_dir(&child).unwrap();
    let alias = f.root.join("escape");
    link(&child, &alias);
    let absent_marker = alias.join("../.worktrees/w/.git");
    std::fs::write(
        f.admin.join("gitdir"),
        format!("{}\n", absent_marker.display()),
    )
    .unwrap();
    let command = format!("git worktree remove --force '{}'", shell_path(&f.linked));
    assert!(
        !codeflow_core::hooks::repo::worktree_recovery(&command, &f.root),
        "lexical equality to an absent registration must not certify an existing command target"
    );
    std::fs::remove_dir_all(&f.linked).unwrap();
    std::fs::create_dir_all(f.other.join(".worktrees/w")).unwrap();
    // Follow the native filesystem's handling of a parent after a symlink.
    // Both spellings must be absent; lexical equality alone is insufficient.
    let registered_absent = match std::fs::metadata(absent_marker.parent().unwrap()) {
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => panic!("cannot inspect the registration fixture: {error}"),
    };
    assert_eq!(
        codeflow_core::hooks::repo::worktree_recovery(&command, &f.root),
        registered_absent
    );
}

#[test]
fn checkout_identity_recovery_preserves_the_shell_grammar() {
    let f = Fixture::new();
    let absent = f.temp.path().join("$(printf evil)");
    std::fs::write(
        f.admin.join("gitdir"),
        format!("{}\n", absent.join(".git").display()),
    )
    .unwrap();
    let command = format!(
        "git worktree remove --force \"{}\"",
        shell_path(&absent).replace('$', "\\$")
    );
    let mut wrong = Vec::new();
    for command in [&command, "git worktree prune", "git worktree list"] {
        let output = run_with_stdin(codeflow().args(["hook", "git-guard"]).current_dir(&f.root),
            &serde_json::json!({"tool_name":"PowerShell", "cwd":f.root, "tool_input":{"command":command}}).to_string());
        let code = if command.contains("remove") { 2 } else { 0 };
        expect(&output, code, "recovery grammar", &mut wrong);
    }
    std::fs::write(
        f.admin.join("gitdir"),
        format!("{}\n", f.root.join("@targets/.git").display()),
    )
    .unwrap();
    let output = run_with_stdin(codeflow().args(["hook", "git-guard"]).current_dir(&f.root),
        &serde_json::json!({"tool_name":"PowerShell", "cwd":f.root, "tool_input":{"command":"git worktree remove --force @targets"}}).to_string());
    expect(
        &output,
        2,
        "PowerShell splatting is not a literal target",
        &mut wrong,
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
