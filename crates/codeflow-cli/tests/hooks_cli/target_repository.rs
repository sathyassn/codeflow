//! TSK-379: built hooks judge the repository holding a target.
use super::{
    codeflow, enforced_repo, expansion_codeflow, guard_payload, run_with_stdin, shell_path,
};
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
        // Both Git's includeIf paths and marker diagnostics name the physical
        // directory. Windows TEMP may use an 8.3 alias; keep every fixture
        // path in the long, non-verbatim spelling used by the guards.
        let parent = codeflow_core::portable_path::canonicalize(&std::env::temp_dir()).unwrap();
        let temp = tempfile::tempdir_in(parent).unwrap();
        let base = codeflow_core::portable_path::canonicalize(temp.path()).unwrap();
        let root = base.join("repo");
        std::fs::create_dir(&root).unwrap();
        enforced_repo(&root);
        let linked = root.join(".worktrees/w");
        git(
            &root,
            &["worktree", "add", "-b", "task/w", &shell_path(&linked)],
        );
        Self { temp, root, linked }
    }

    fn guard_command(&self, cwd: &Path, hook: &str) -> Command {
        let mut cmd = expansion_codeflow();
        cmd.args(["hook", hook, "--contract", "3"])
            .current_dir(cwd)
            .env("HOME", self.temp.path())
            .env("USERPROFILE", self.temp.path())
            .env("SHELL", "/bin/zsh");
        cmd
    }

    fn command(&self, cwd: &Path, hook: &str, command: &str) -> Output {
        run_with_stdin(
            &mut self.guard_command(cwd, hook),
            &guard_payload(command, cwd),
        )
    }

    fn command_with_tmpdir(
        &self,
        cwd: &Path,
        hook: &str,
        command: &str,
        tmpdir: Option<&Path>,
    ) -> Output {
        let mut cmd = self.guard_command(cwd, hook);
        if let Some(tmpdir) = tmpdir {
            cmd.env("TMPDIR", tmpdir);
        } else {
            cmd.env_remove("TMPDIR");
        }
        run_with_stdin(&mut cmd, &guard_payload(command, cwd))
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

/// Issue #342 commands are classified from a linked task worktree. None of
/// the payloads run, including the absolute temporary destinations.
#[test]
fn tsk1002_issue342_resolved_write_regressions() {
    let f = Fixture::new();
    assert!(f.linked.join(".git").is_file());
    for directory in ["build", "dst", "tests"] {
        std::fs::create_dir(f.linked.join(directory)).unwrap();
    }
    for file in ["build/a.txt", "dst/ordinary.txt", "tests/old.txt"] {
        std::fs::write(f.linked.join(file), "ordinary fixture\n").unwrap();
    }
    for line in [
        r#"L=/private/tmp/sp; echo x | tee $L/run.log | grep -E "^a""#,
        r#"L=/private/tmp/sp; python3 tools/run.py 2>&1 | tee $L/run.log | grep -E "SURVIVED|MISSING|^[0-9]+/""#,
        r#"D=tests; rm $D/old.txt && grep -n "^TSK-123" README.md"#,
        "D=dst; cp build/a.txt $D/*",
        "D=/private/tmp/sp; cp build/a.txt $D/*",
    ] {
        let out = f.command(&f.linked, "git-guard", line);
        assert_eq!(out.status.code(), Some(0), "{line}: {out:?}");
    }
    for line in [
        r#"L=.codeflow; echo x | tee $L/policy.json | grep -E "^a""#,
        "D=.codeflow; touch $D/*",
        "D=.codeflow; cp build/a.txt $D/*",
    ] {
        let out = f.command(&f.linked, "git-guard", line);
        assert!(refused(&out), "{line}: {out:?}");
    }
}

#[test]
fn tsk1002_r12_tee_destinations_are_writes() {
    let f = Fixture::new();
    let tmpdir = f.temp.path().join("logs");
    std::fs::create_dir(&tmpdir).unwrap();
    let mut wrong = Vec::new();
    for program in [
        "tee",
        "gtee",
        "command tee",
        "command -p tee",
        "builtin tee",
        "env tee",
        "timeout 5 tee",
        "nohup tee",
    ] {
        let line = format!(
            r#"echo x | {program} "$TMPDIR/run.log" | grep -E "SURVIVED|MISSING|^[0-9]+/""#
        );
        for guard in ["git-guard", "exec-guard"] {
            let out = f.command_with_tmpdir(&f.linked, guard, &line, Some(&tmpdir));
            if out.status.code() != Some(0) {
                wrong.push(format!("known TMPDIR: {guard}: {line}: {out:?}"));
            }
            let out = f.command_with_tmpdir(&f.linked, guard, &line, None);
            let expected = Some(if guard == "git-guard" { 2 } else { 0 });
            if out.status.code() != expected || guard == "git-guard" && !refused(&out) {
                wrong.push(format!("absent TMPDIR: {guard}: {line}: {out:?}"));
            }
        }
    }
    let out = f.command_with_tmpdir(
        &f.linked,
        "git-guard",
        "echo x | tee \"$TMPDIR/.codeflow/policy.json\"",
        Some(&f.linked),
    );
    assert!(refused(&out), "TMPDIR inside the checkout: {out:?}");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[cfg(windows)]
#[test]
fn tsk1002_review3_windows_unmapped_path_spellings() {
    let f = Fixture::new();
    let root = f.linked.to_str().unwrap().replace('/', "\\");
    let home = f.temp.path().to_str().unwrap().replace('/', "\\");
    for (guard, line) in [
        ("git-guard", format!(r"printf x > '\\.\{root}\.git\config'")),
        ("git-guard", r"printf x > '.git\config::$DATA'".to_string()),
        ("exec-guard", format!(r"cat '{home}\.codex\AUTH~1.JSO'")),
        (
            "exec-guard",
            r"cat '\\?\UNC\invalid\share\.codex\auth.json'".to_string(),
        ),
    ] {
        let out = f.command(&f.linked, guard, &line);
        assert_eq!(out.status.code(), Some(2), "{line}: {out:?}");
    }
    let out = f.command(&f.linked, "exec-guard", "cat notes.md");
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}

/// TSK-1002 review 01, findings 1 to 4, through the installed hooks: a
/// search selector that names the root, inline code in an attached option,
/// a file written without a heredoc and then run, and a parent component
/// after an existing link. Each refuses; its public control passes.
#[test]
fn tsk1002_review01_classes_refuse_through_the_hooks() {
    let f = Fixture::new();
    let copies = f.linked.join("copies");
    std::fs::create_dir_all(&copies).unwrap();
    std::fs::write(copies.join("id_rsa"), "planted fixture").unwrap();
    std::fs::create_dir_all(f.linked.join("visible")).unwrap();
    std::fs::write(f.linked.join("visible/note.txt"), "ordinary fixture").unwrap();
    let mut wrong = Vec::new();
    // The link cases need a Unix symlink; other hosts run the rest.
    #[cfg(unix)]
    let (link_refuse, link_allow) = {
        let store = f.temp.path().join(".codex/child");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(f.temp.path().join(".codex/auth.json"), "planted fixture").unwrap();
        std::os::unix::fs::symlink(&store, f.linked.join("alias")).unwrap();
        (
            vec!["cat alias/../auth.json"],
            vec!["cat visible/../visible/note.txt"],
        )
    };
    #[cfg(not(unix))]
    let (link_refuse, link_allow): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    let refuse: Vec<&str> = [
        "rg -g 'copies/**' needle copies",
        "node --print='require(\"fs\").readFileSync(\"copies/id_rsa\",\"utf8\")'",
        "printf 'print(1)' > run.py; python3 run.py",
        "echo 'print(1)' > run.py && python3 run.py",
        // Review 04: deno's inline code is still read.
        "deno eval 'Deno.readTextFileSync(\"copies/id_rsa\")'",
        "deno run --allow-read - < copies/id_rsa",
    ]
    .into_iter()
    .chain(link_refuse)
    .collect();
    let allow: Vec<&str> = [
        "rg -g 'visible/**' needle visible",
        "node --print='require(\"fs\").readFileSync(\"visible/note.txt\",\"utf8\")'",
        "python3 gen.py > out.json; python3 use.py out.json",
        // Review 04: deno subcommands and permission flags are ordinary.
        "deno test",
        "deno task dev",
        "deno fmt",
        "deno run --allow-read main.ts",
        "deno run --allow-net=example.com --unstable-kv --watch main.ts",
        "deno run -A --reload --no-remote --node-modules-dir=auto main.ts",
        "deno eval 'console.log(1)'",
    ]
    .into_iter()
    .chain(link_allow)
    .collect();
    for line in &refuse {
        if f.command(&f.linked, "exec-guard", line).status.code() != Some(2) {
            wrong.push(format!("allowed: {line}"));
        }
    }
    for line in &allow {
        let out = f.command(&f.linked, "exec-guard", line);
        if out.status.code() != Some(0) {
            wrong.push(format!("refused: {line}: {out:?}"));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// TSK-1002 review 03: a directory change in any spelling env or pwsh
/// accepts is where the command reads and writes. Attached, clustered,
/// abbreviated and `--`-terminated forms refuse a store read or a checkout
/// file write as the separated form does; a directory the guard cannot
/// establish refuses a relative operand that could name a secret. Public
/// directories stay usable.
#[test]
fn tsk1002_directory_changes_in_every_launcher_spelling() {
    let f = Fixture::new();
    let store = f.temp.path().join(".codex");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("auth.json"), "planted fixture").unwrap();
    let public = f.temp.path().join("public");
    std::fs::create_dir_all(&public).unwrap();
    std::fs::write(public.join("auth.json"), "ordinary fixture").unwrap();
    let store = shell_path(&store);
    let public = shell_path(&public);
    let mut wrong = Vec::new();
    let reads = [
        "env -C '%' cat auth.json",
        "env -C'%' cat auth.json",
        "env -iC '%' cat auth.json",
        "env -vC '%' cat auth.json",
        "env -0iC'%' cat auth.json",
        "env --chdir='%' cat auth.json",
        "env --chdir='%' -- cat auth.json",
        "env --ch '%' cat auth.json",
        "env -u X -C '%' -- FOO=1 cat auth.json",
        "pwsh -WorkingDirectory '%' -Command 'Get-Content auth.json'",
        "pwsh -wd '%' -c 'Get-Content auth.json'",
        "pwsh -wo '%' -c 'Get-Content auth.json'",
        "pwsh -WorkingDir '%' -c 'Get-Content auth.json'",
        // Code runs where its call runs.
        "env -C '%' python3 -c 'open(\"auth.json\").read()'",
        "env -iC'%' node -e 'require(\"fs\").readFileSync(\"auth.json\")'",
        "ruby -C '%' -e 'File.read(\"auth.json\")'",
    ];
    for form in reads {
        let line = form.replace('%', &store);
        if f.command(&f.linked, "exec-guard", &line).status.code() != Some(2) {
            wrong.push(format!("allowed: {line}"));
        }
        let line = form.replace('%', &public);
        let out = f.command(&f.linked, "exec-guard", &line);
        if out.status.code() != Some(0) {
            wrong.push(format!("refused public: {line}: {out:?}"));
        }
    }
    for line in [
        "D=$(mktemp -d); env -C \"$D\" cat auth.json",
        "env --frobnicate -C '%' cat auth.json",
    ] {
        let line = line.replace('%', &public);
        let out = f.command(&f.linked, "exec-guard", &line);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.code() != Some(2) {
            wrong.push(format!("allowed unknown directory: {line}"));
        } else if stderr.contains("__unverified") || !stderr.contains("`env -") {
            // The refusal names env's words as written, never a marker.
            wrong.push(format!("unclear refusal: {line}: {stderr}"));
        }
    }
    for line in [
        "env -C .git tee config",
        "env -C.git tee config",
        "env -iC .git tee config",
        "env --chdir=.git -- tee config",
        "env -C .git sh -c 'printf x > config'",
        "pwsh -wd .git -c 'Set-Content config x'",
        "pwsh -WorkingDirectory .git -Command 'Set-Content config x'",
    ] {
        // Both guards run before every shell command; either refusal stops it.
        if ["git-guard", "exec-guard"]
            .iter()
            .all(|guard| f.command(&f.linked, guard, line).status.code() != Some(2))
        {
            wrong.push(format!("allowed write: {line}"));
        }
    }
    // The shell opens a redirect before env runs, in its own directory.
    for line in [
        "env -C .git printf x > notes.txt",
        "env -iC src tee notes.txt",
    ] {
        let out = f.command(&f.linked, "git-guard", line);
        if out.status.code() != Some(0) {
            wrong.push(format!("refused public write: {line}: {out:?}"));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// TSK-1002 UNC finding: an administrative share, a verbatim or device UNC
/// spelling, or any share whose path ends in a home store names the store,
/// and a share write names the checkout file. Before this change the guard
/// compared `\\localhost\C$\...` with the drive-spelled home and allowed it.
/// Controls: an ordinary file through an administrative share, a UNC home
/// and a UNC cwd stay usable.
#[cfg(windows)]
#[test]
#[allow(clippy::too_many_lines)] // One fixture: share refusals, then home, cwd and identity controls.
fn tsk1002_unc_spellings_of_stores_and_checkouts_refuse() {
    let f = Fixture::new();
    let home = f.temp.path().to_str().unwrap().replace('/', "\\");
    let root = f.linked.to_str().unwrap().replace('/', "\\");
    let (drive, home_tail) = (&home[..1], &home[3..]);
    let root_tail = &root[3..];
    std::fs::create_dir_all(f.temp.path().join(".codex")).unwrap();
    std::fs::write(f.temp.path().join(".codex/auth.json"), "planted fixture").unwrap();
    std::fs::write(f.temp.path().join("notes.md"), "ordinary fixture").unwrap();
    let store = r".codex\auth.json";
    let mut wrong = Vec::new();
    for (guard, line) in [
        (
            "exec-guard",
            format!(r"cat '\\localhost\{drive}$\{home_tail}\{store}'"),
        ),
        (
            "exec-guard",
            format!(
                "cat '//localhost/{drive}$/{}/.codex/auth.json'",
                home_tail.replace('\\', "/")
            ),
        ),
        (
            "exec-guard",
            format!(r"cat '\\127.0.0.1\{drive}$\{home_tail}\{store}'"),
        ),
        (
            "exec-guard",
            format!(r"cat '\\?\UNC\localhost\{drive}$\{home_tail}\{store}'"),
        ),
        (
            "exec-guard",
            format!(r"cat '\\.\UNC\localhost\{drive}$\{home_tail}\{store}'"),
        ),
        ("exec-guard", format!(r"cat '\\?\{home}\{store}'")),
        (
            "exec-guard",
            format!(r"cat '\\invalid.invalid\share\Users\u\{store}'"),
        ),
        (
            "git-guard",
            format!(r"printf x > '\\localhost\{drive}$\{root_tail}\.git\config'"),
        ),
        // Review 01: a share that hides the store's directory name, a
        // verbatim name an ordinary path would change, the NT namespace and
        // a stream of a protected file.
        (
            "exec-guard",
            r"cat '\\localhost\fixture-share\auth.json'".into(),
        ),
        (
            "exec-guard",
            r"rg needle '\\localhost\fixture-share\docs'".into(),
        ),
        ("exec-guard", format!(r"cat '\\?\{home}\public \auth.json'")),
        ("exec-guard", format!(r"cat '\\?\{home}\.codex.\auth.json'")),
        ("exec-guard", format!(r"cat '\??\{home}\{store}'")),
        ("exec-guard", format!(r"cat '{home}\{store}:notes'")),
        (
            "git-guard",
            format!(r"printf x > '{root}\.git\config:notes'"),
        ),
    ] {
        let out = f.command(&f.linked, guard, &line);
        if out.status.code() != Some(2) {
            wrong.push(format!("refuse {guard}: {line}: {out:?}"));
        }
    }
    // Review 04: the share refusal names the drive-letter remedy.
    let out = f.command(
        &f.linked,
        "exec-guard",
        r"cat '\\localhost\fixture-share\notes.md'",
    );
    if !String::from_utf8_lossy(&out.stderr).contains("drive-letter path") {
        wrong.push(format!("share refusal without its remedy: {out:?}"));
    }
    for line in [
        format!(r"cat '\\localhost\{drive}$\{home_tail}\notes.md'"),
        format!(r"cat '\\?\{home}\notes.md'"),
        format!(r"cat '\??\{home}\notes.md'"),
    ] {
        let out = f.command(&f.linked, "exec-guard", &line);
        if out.status.code() != Some(0) {
            wrong.push(format!("allow: {line}: {out:?}"));
        }
    }
    // A UNC home and cwd make the hook read through the share, so these
    // controls need the administrative share to answer on this host.
    let unc_home = format!(r"\\localhost\{drive}$\{home_tail}");
    if std::fs::metadata(&unc_home).is_ok() {
        for (command, code) in [("cat ~/notes.md", 0), ("cat ~/.codex/auth.json", 2)] {
            let mut cmd = f.guard_command(&f.linked, "exec-guard");
            cmd.env("HOME", &unc_home).env("USERPROFILE", &unc_home);
            let out = run_with_stdin(&mut cmd, &guard_payload(command, &f.linked));
            if out.status.code() != Some(code) {
                wrong.push(format!("UNC home: {command}: {out:?}"));
            }
        }
        // A checkout's identity is never established from a share spelling:
        // the share's host may be another machine, whose `C:` is not this
        // one's, so the registration comparison refuses (hosted run
        // 40674611502). Secret judgments map any host's share, because
        // there a wrong mapping can only refuse more.
        let linked_unc = PathBuf::from(format!(r"\\localhost\{drive}$\{root_tail}"));
        let out = run_with_stdin(
            &mut f.guard_command(&f.linked, "exec-guard"),
            &guard_payload("cat notes.md", &linked_unc),
        );
        if out.status.code() != Some(2) {
            wrong.push(format!("UNC checkout identity was established: {out:?}"));
        }
        // A plain directory outside any checkout reads through the share.
        let unc_cwd = PathBuf::from(&unc_home);
        for (command, code) in [("cat notes.md", 0), ("cat .codex/auth.json", 2)] {
            let out = run_with_stdin(
                &mut f.guard_command(f.temp.path(), "exec-guard"),
                &guard_payload(command, &unc_cwd),
            );
            if out.status.code() != Some(code) {
                wrong.push(format!("UNC cwd: {command}: {out:?}"));
            }
        }
    } else {
        eprintln!("administrative share unavailable; UNC home and cwd controls skipped");
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn tsk1002_review3_environment_ignore_inputs_cannot_hide_secrets() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.linked.join("scan/ignored")).unwrap();
    std::fs::write(
        f.linked.join("scan/ignored/id_rsa.rs"),
        "fake private fixture",
    )
    .unwrap();
    std::fs::write(f.linked.join(".gitignore"), "scan/ignored/\n").unwrap();
    let config = f.temp.path().join("rg-options");
    std::fs::write(&config, "--no-ignore-vcs\n").unwrap();
    let mut cmd = f.guard_command(&f.linked, "exec-guard");
    cmd.env("RIPGREP_CONFIG_PATH", config);
    let out = run_with_stdin(
        &mut cmd,
        &guard_payload("rg -g '*.rs' token scan", &f.linked),
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("security.secret_reads"));
}

#[test]
fn tsk1002_review3_nul_path_streams_preserve_file_names() {
    let f = Fixture::new();
    std::fs::write(f.temp.path().join("notes.md"), "ordinary fixture").unwrap();
    for (line, expected) in [
        ("printf '%s\\0' \"$HOME/notes.md\" | xargs -0 cat", 0),
        ("printf '%s\\000' \"$HOME/notes.md\" | xargs -0 cat", 0),
        (
            "printf '%s\\0' \"$HOME/.codex/auth.json\" | xargs -0 cat",
            2,
        ),
        (
            "printf() { :; }; printf '%s\\0' \"$HOME/notes.md\" | xargs -0 cat",
            2,
        ),
    ] {
        let out = f.command(&f.linked, "exec-guard", line);
        assert_eq!(out.status.code(), Some(expected), "{line}: {out:?}");
    }
}

#[cfg(windows)]
#[test]
fn tsk1002_ci1_windows_home_streams_stay_guarded() {
    let f = Fixture::new();
    std::fs::create_dir(f.temp.path().join(".codex")).unwrap();
    std::fs::write(
        f.temp.path().join(".codex/auth.json"),
        "fake private fixture\n",
    )
    .unwrap();
    std::fs::write(f.temp.path().join("notes.md"), "ordinary fixture\n").unwrap();
    for line in [
        "echo ~/.codex/auth.json | xargs cat",
        "echo ~/.codex/auth.json | xargs -I{} cat {}",
        "xargs cat <<< ~/.codex/auth.json",
        "printf '%s\\0' ~/.codex/auth.json | xargs -0 cat",
    ] {
        let out = f.command(&f.linked, "exec-guard", line);
        assert_eq!(out.status.code(), Some(2), "{line}: {out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("security.secret_reads"));
    }
    let public = codeflow_core::portable_path::slashed(&f.temp.path().join("notes.md"));
    for line in [
        format!("printf '%s\\n' '{public}' | xargs cat"),
        "printf '%s\\0' \"$HOME/notes.md\" | xargs -0 cat".to_string(),
    ] {
        let out = f.command(&f.linked, "exec-guard", &line);
        assert_eq!(out.status.code(), Some(0), "{line}: {out:?}");
    }
}

#[test]
fn tsk1002_issue342_tee_keeps_write_and_input_read_protection() {
    let f = Fixture::new();
    for line in [
        r#"echo x | tee ".codeflow/policy.json""#,
        r#"echo x | gtee ".git/config""#,
        r#"echo x | tee "$TMPDIR/.git/config""#,
    ] {
        let out = f.command(&f.linked, "git-guard", line);
        assert!(refused(&out), "{line}: {out:?}");
    }
    for line in [
        r#"tee "$TMPDIR/run.log" < "$HOME/.ssh/id_rsa""#,
        r#"gtee "$TMPDIR/run.log" < "$HOME/.ssh/id_rsa""#,
        r#"tee "$TMPDIR/run.log" <(cat "$HOME/.ssh/id_rsa")"#,
    ] {
        let out = f.command(&f.linked, "exec-guard", line);
        assert_eq!(out.status.code(), Some(2), "{line}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("security.secret_reads"),
            "{line}: {out:?}"
        );
    }
}

#[test]
fn tsk1002_r13_home_catalog_globs_and_named_metadata_paths() {
    let f = Fixture::new();
    let home = f.temp.path();
    for directory in [".codex", ".claude/skills"] {
        std::fs::create_dir_all(home.join(directory)).unwrap();
    }
    std::fs::write(home.join(".codex/auth.json"), "fake secret\n").unwrap();
    std::fs::write(home.join(".codex/public.rs"), "ordinary fixture\n").unwrap();
    std::fs::write(home.join(".claude/skills/public.md"), "ordinary fixture\n").unwrap();
    for line in ["cat ~/.codex/auth.*", "cat ~/.codex/auth.{json}"] {
        let out = f.command(&f.linked, "exec-guard", line);
        assert_eq!(out.status.code(), Some(2), "{line}: {out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("security.secret_reads"));
    }
    std::fs::remove_file(home.join(".codex/auth.json")).unwrap();
    let out = f.command(&f.linked, "exec-guard", "cat ~/.codex/auth.*");
    assert_eq!(out.status.code(), Some(2), "absent home rule: {out:?}");
    for line in [
        "cat ~/.codex/*.rs",
        "cat ~/.claude/skills/*.md",
        "cat '~/.codex/auth.*'",
        "git -C . status",
        "git log -- HOME",
    ] {
        let out = f.command(&f.linked, "exec-guard", line);
        assert_eq!(out.status.code(), Some(0), "{line}: {out:?}");
    }
    let root = shell_path(&f.root);
    let line = format!("HOME='{root}'; git status -- ~/.claude/settings.json");
    let out = f.command(&f.linked, "exec-guard", &line);
    assert_eq!(out.status.code(), Some(2), "{line}: {out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("security.secret_reads"));
}

#[test]
fn tsk1002_r13_linked_search_inspects_ignored_build_roots() {
    let f = Fixture::new();
    std::fs::write(f.linked.join(".gitignore"), "/target/\n").unwrap();
    std::fs::create_dir(f.linked.join("target")).unwrap();
    std::fs::write(f.linked.join("target/id_rsa.rs"), "fake secret\n").unwrap();
    std::fs::write(f.linked.join("public.rs"), "ordinary fixture\n").unwrap();
    for line in [
        "rg -g '*.rs' foo .",
        "rg -g '*.rs' foo ./",
        "rg --no-ignore -g '*.rs' foo .",
        "rg -g '*.rs' foo target",
        "rg -g '*' foo .",
    ] {
        let out = f.command(&f.linked, "exec-guard", line);
        assert_eq!(out.status.code(), Some(2), "{line}: {out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("security.secret_reads"));
    }
}

#[test]
fn tsk1002_r13_redirect_globs_protect_possible_hook_targets() {
    let f = Fixture::new();
    let commands = [
        "printf x > .gi?/hooks/pre-commit",
        "printf x > .git/hook[s]/pre-commit",
    ];
    let hook = f.root.join(".git/hooks/pre-commit");
    assert!(!hook.exists());
    for line in commands {
        let out = f.command(&f.root, "git-guard", line);
        assert!(refused(&out), "prospective hook: {line}: {out:?}");
    }
    std::fs::write(&hook, "# planted fixture\n").unwrap();
    for line in commands {
        let out = f.command(&f.root, "git-guard", line);
        assert!(refused(&out), "existing hook: {line}: {out:?}");
    }
    for line in [
        "printf x > src/hook[s]/pre-commit",
        "printf x > '.gi?/hooks/pre-commit'",
        "printf x > '.git/hook[s]/pre-commit'",
        "cat .gi?/hooks/pre-commit",
    ] {
        let out = f.command(&f.root, "git-guard", line);
        assert_eq!(
            out.status.code(),
            Some(0),
            "literal/read control: {line}: {out:?}"
        );
    }
    #[cfg(unix)]
    {
        std::fs::remove_file(&hook).unwrap();
        std::os::unix::fs::symlink("pre-commit", &hook).unwrap();
        for line in commands {
            let out = f.command(&f.root, "git-guard", line);
            assert!(refused(&out), "unexpandable hook: {line}: {out:?}");
        }
    }
}

#[test]
fn tsk1002_r14_redirect_terminal_escapes_do_not_crash() {
    let f = Fixture::new();
    let mut wrong = Vec::new();
    for line in [
        r"printf x > .git/config\",
        r"printf x > .codeflow/policy.json\",
        r#"printf x > ".git/config\"#,
        r#"printf x > ".codeflow/policy.json\"#,
    ] {
        let out = f.command(&f.linked, "git-guard", line);
        if !refused(&out) {
            wrong.push(format!("protected incomplete target: {line:?}: {out:?}"));
        }
    }
    for line in [r"printf x > ordinary\", r#"printf x > "ordinary\"#] {
        let out = f.command(&f.linked, "git-guard", line);
        if !matches!(out.status.code(), Some(0 | 2)) {
            wrong.push(format!("incomplete target crashed: {line:?}: {out:?}"));
        }
    }
    for line in [r"printf x > ordinary\\", r"printf x > 'ordinary\'"] {
        let out = f.command(&f.linked, "git-guard", line);
        if out.status.code() != Some(0) {
            wrong.push(format!("complete literal target: {line:?}: {out:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn tsk1002_r14_home_file_globs_keep_public_siblings() {
    let f = Fixture::new();
    let project = f.temp.path().join(".claude/projects/fixture");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("notes.md"), "ordinary fixture\n").unwrap();
    std::fs::write(project.join("trace.jsonl"), "fake private trace\n").unwrap();
    std::fs::create_dir(project.join("nested")).unwrap();
    std::fs::write(project.join("nested/notes.md"), "ordinary nested fixture\n").unwrap();
    std::fs::write(project.join("nested/trace.jsonl"), "fake nested trace\n").unwrap();
    let mut wrong = Vec::new();
    for (line, refuse) in [
        ("cat ~/.claude/projects/*/*.md", false),
        ("cat ~/.claude/projects/**/*.md", false),
        ("shopt -s globstar; cat ~/.claude/projects/**/*.md", false),
        ("cat ~/.claude/projects/*/notes.md", false),
        ("cat ~/.claude/projects/*/*.jsonl", true),
        ("cat ~/.claude/projects/**/*.jsonl", true),
        ("shopt -s globstar; cat ~/.claude/projects/**/*.jsonl", true),
        ("cat ~/.claude/projects/*/*", true),
        ("cat ~/.codex/auth.*", true),
        ("cat ~/.codex/auth.{json}", true),
    ] {
        let out = f.command(&f.linked, "exec-guard", line);
        let expected = Some(if refuse { 2 } else { 0 });
        if out.status.code() != expected
            || refuse && !String::from_utf8_lossy(&out.stderr).contains("security.secret_reads")
        {
            wrong.push(format!("{line}: expected {expected:?}: {out:?}"));
        }
    }
    let selected_secret = project.join("id_rsa.md");
    std::fs::write(&selected_secret, "fake private Markdown name\n").unwrap();
    let selectors = [
        "cat ~/.claude/projects/*/*.md",
        "cat ~/.claude/projects/**/*.md",
        "shopt -s globstar; cat ~/.claude/projects/**/*.md",
    ];
    for line in selectors {
        let out = f.command(&f.linked, "exec-guard", line);
        if out.status.code() != Some(2)
            || !String::from_utf8_lossy(&out.stderr).contains("security.secret_reads")
        {
            wrong.push(format!("selected secret name: {line}: {out:?}"));
        }
    }
    #[cfg(unix)]
    {
        std::fs::remove_file(&selected_secret).unwrap();
        std::os::unix::fs::symlink("trace.jsonl", project.join("renamed.md")).unwrap();
        for line in selectors {
            let out = f.command(&f.linked, "exec-guard", line);
            if out.status.code() != Some(2)
                || !String::from_utf8_lossy(&out.stderr).contains("security.secret_reads")
            {
                wrong.push(format!("selected secret link: {line}: {out:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
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
            let unknown_files = hook == "exec-guard" && command.contains("xargs git add");
            assert_eq!(
                out.status.code(),
                Some(if unknown_files { 2 } else { 0 }),
                "{hook} {command}: {out:?}"
            );
            if unknown_files {
                let diagnostic = String::from_utf8_lossy(&out.stderr);
                assert!(
                    diagnostic.contains("security.secret_reads")
                        && diagnostic.contains("cannot verify")
                );
            }
        }
        let out = f.command(&f.root, hook, "echo README.md | xargs git add");
        assert_eq!(
            out.status.code(),
            Some(0),
            "literal file input: {hook}: {out:?}"
        );
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
fn empty_removal_operand_does_not_name_a_git_directory_container() {
    let f = Fixture::new();
    let container = f.temp.path().join("scratch");
    std::fs::create_dir(&container).unwrap();
    git2::Repository::init_bare(container.join("store.git")).unwrap();
    for command in ["rm -f '' notes.txt", "rmdir ''"] {
        let out = f.command(&container, "git-guard", command);
        assert_eq!(out.status.code(), Some(0), "{command}: {out:?}");
    }
    for command in ["rm -rf .", "rm -rf store.git", "find . -delete"] {
        let out = f.command(&container, "git-guard", command);
        assert!(refused(&out), "{command}: {out:?}");
    }
}

#[test]
fn runner_home_in_program_path_assignments() {
    let f = Fixture::new();
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            "PATH=\"$HOME/bin:$PATH\" git status",
            "PATH=\"${HOME}/bin:${PATH}\" git status",
            "export PATH=\"$HOME/bin:$PATH\"; git status",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        for command in [
            "PATH=\"$UNPROVEN/bin:$PATH\" git status",
            "HOME=$UNPROVEN; PATH=\"$HOME/bin:$PATH\" git status",
            "PATH=\"$HOME/bin:$PATH\" git config core.bare true",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn runner_home_tilde_operands() {
    let f = Fixture::new();
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            "git clone https://example.test/x/y.git ~/scratch/y",
            "git -c core.pager=true diff ~",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        for command in [
            "git clone https://example.test/x/y.git ~someone/scratch/y",
            "git clone https://example.test/x/y.git ~/scratch/*",
            "git clone https://example.test/x/y.git ~/{scratch,repo}/y",
            "git clone https://example.test/x/y.git ~/repo/.claude/y",
            "git clone https://example.test/x/y.git ~/repo/.git/hooks/y",
            "git rebase -x ~/bin/* HEAD~1",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(2), "{hook} {command}: {out:?}");
        }
    }
}

#[test]
fn runner_sources_preserve_spaces_and_case_in_redirects() {
    let temp = tempfile::Builder::new()
        .prefix("Mixed Case ")
        .tempdir()
        .unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    enforced_repo(&root);
    let f = Fixture {
        temp,
        linked: root.join(".worktrees/w"),
        root,
    };
    let linked = f.root.join(".worktrees/Mixed Case");
    git(
        &f.root,
        &["worktree", "add", "-b", "task/spaced", &shell_path(&linked)],
    );
    let repo = git2::Repository::open(&linked).unwrap();
    let state = repo.path().join("rebase-merge");
    std::fs::create_dir(&state).unwrap();
    let todo = state.join("git-rebase-todo");
    std::fs::write(&todo, "exec true\n").unwrap();
    for hook in ["exec-guard", "git-guard"] {
        let command = format!(
            "printf 'exec rm -rf .claude\\n' > '{}'; git rebase --continue",
            shell_path(&todo)
        );
        let out = f.command(&linked, hook, &command);
        assert!(refused(&out), "{hook}: {out:?}");
        let ordinary = f.command(
            &linked,
            hook,
            "printf note > 'Mixed Notes'; git rebase --continue",
        );
        assert_eq!(ordinary.status.code(), Some(0), "{hook}: {ordinary:?}");
    }
}

#[test]
fn runner_callbacks_outside_checkout_keep_known_starting_directory() {
    let f = Fixture::new();
    let global = f.temp.path().join("global.conf");
    std::fs::write(&global, "[filter \"lfs\"]\n clean = git-lfs clean -- %f\n smudge = git-lfs smudge -- %f\n process = git-lfs filter-process\n").unwrap();
    for hook in ["exec-guard", "git-guard"] {
        let run = |command: &str| {
            run_with_stdin(
                codeflow()
                    .args(["hook", hook, "--contract", "3"])
                    .current_dir(f.temp.path())
                    .env("HOME", f.temp.path())
                    .env("GIT_CONFIG_GLOBAL", &global),
                &guard_payload(command, f.temp.path()),
            )
        };
        for command in ["git -C . status", "git config --file config user.name x"] {
            let out = run(command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
        if hook == "exec-guard" {
            let out = run("git clean -fdx -- build/");
            assert_eq!(out.status.code(), Some(0), "{hook}: {out:?}");
        }
        let out = run("git --git-dir=missing status");
        assert_eq!(out.status.code(), Some(2), "{hook}: {out:?}");
        let command = format!(
            "git -c core.fsmonitor='rm -rf {}' status",
            shell_path(&f.root.join(".claude"))
        );
        let out = run(&command);
        assert!(refused(&out), "{hook}: {out:?}");
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

// Security round 8, SEC-379-R8-1: rsync reads an empty operand as the
// current directory, so it removes or replaces the checkout's files.
#[test]
fn empty_copy_operands_read_as_the_current_directory() {
    let f = Fixture::new();
    let empty = f.temp.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    let dest = shell_path(&f.temp.path().join("dest"));
    let empty = shell_path(&empty);
    for cwd in [&f.root, &f.linked] {
        for command in [
            format!("rsync -a --remove-source-files '' '{dest}'"),
            format!("rsync -a --delete '{empty}/' ''"),
            format!("rsync -a --delete '{empty}/' ."),
            format!("mv '' '{dest}'"),
        ] {
            let out = f.command(cwd, "git-guard", &command);
            assert!(refused(&out), "{}: {command}: {out:?}", cwd.display());
        }
        for command in [
            "rm -f '' notes.txt".to_string(),
            "find '' -name x".to_string(),
            format!("rsync -a --delete '{empty}/' build/"),
        ] {
            let out = f.command(cwd, "git-guard", &command);
            assert_eq!(out.status.code(), Some(0), "{command}: {out:?}");
        }
    }
}

// Security round 8, SEC-379-R8-2: a repository copied, moved, synced,
// extracted, linked or cloned into place earlier on the line is state the
// guard read before the line ran. With or without global callbacks, the
// next Git command there is refused; ordinary writes that create no
// repository state keep passing.
#[test]
fn earlier_writes_make_the_next_runner_state_unknown() {
    let f = Fixture::new();
    let evil = f.temp.path().join("evil");
    std::fs::create_dir(&evil).unwrap();
    git(&evil, &["init", "-b", "main"]);
    let wipe = format!("rm -rf {}", shell_path(&f.root.join(".claude")));
    git(&evil, &["config", "core.fsmonitor", &wipe]);
    let stage = f.temp.path().join("stage");
    std::fs::create_dir_all(stage.join("x")).unwrap();
    let evil = shell_path(&evil);
    let lfs = f.temp.path().join("global.conf");
    std::fs::write(&lfs, "[filter \"lfs\"]\n clean = git-lfs clean -- %f\n smudge = git-lfs smudge -- %f\n process = git-lfs filter-process\n").unwrap();
    for global in [Path::new("/dev/null"), lfs.as_path()] {
        for hook in ["exec-guard", "git-guard"] {
            let run = |cwd: &Path, command: &str| {
                run_with_stdin(
                    codeflow()
                        .args(["hook", hook, "--contract", "3"])
                        .current_dir(cwd)
                        .env("HOME", f.temp.path())
                        .env("USERPROFILE", f.temp.path())
                        .env("GIT_CONFIG_GLOBAL", global),
                    &guard_payload(command, cwd),
                )
            };
            for command in [
                format!("cp -R '{evil}/.git' x/ && git -C x status"),
                format!("mv '{evil}/.git' x/ && git -C x status"),
                format!("cp -R '{evil}' x/ && git -C x/evil status"),
                format!("rsync -a '{evil}/' x/ && git -C x status"),
                format!("ln -s '{evil}/.git' x/.git && git -C x status"),
                format!("printf 'gitdir: {evil}/.git\\n' > x/.git && git -C x status"),
                format!("cd x && cp -R '{evil}/.git' . && git status"),
            ] {
                let out = run(&stage, &command);
                assert_eq!(out.status.code(), Some(2), "{hook} {command}: {out:?}");
                if hook == "exec-guard" {
                    assert!(refused(&out), "{hook} {command}: {out:?}");
                }
            }
            for command in [
                "git -C . status",
                "git config --file config user.name x",
                "printf x > notes.txt && git -C . status",
            ] {
                let out = run(&stage, command);
                assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
            }
            for command in [
                "printf note > notes.txt && git add notes.txt",
                "cp README.md copy.md && git add copy.md",
                "cp ../../base.txt . && git add base.txt",
                "mkdir -p build && cp README.md build/ && git status",
                "rm -rf build && git status",
            ] {
                let out = run(&f.linked, command);
                assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
            }
        }
    }
}

// Fable review round 5 majors 1 and 2, security round 8 R8-3: `~` and
// `$HOME` name the home the line assigns before them, in every resolver.
#[test]
fn reassigned_home_resolves_tilde_and_home_words() {
    let f = Fixture::new();
    let root = shell_path(&f.root);
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            format!("HOME={root}; git clone https://example.test/x/y.git ~/.claude/skills/x"),
            format!(
                "export HOME={root}; git clone https://example.test/x/y.git ~/.claude/skills/x"
            ),
            format!("HOME={root}; rm -rf ~/.claude"),
            format!("HOME={root}; printf x > ~/.claude/settings.json"),
            format!("HOME={root}; printf x > \"$HOME/.claude/settings.json\""),
            format!("HOME={root}; git init ~/.claude/x"),
            "HOME=$UNPROVEN; git clone https://example.test/x/y.git ~/scratch/y".to_string(),
            "HOME=/zz PATH=\"$HOME/bin:$PATH\" git status".to_string(),
            format!("HOME={root} bash -c 'rm -rf ~/.claude'"),
        ] {
            let out = f.command(&f.linked, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        for command in [
            "git clone https://example.test/x/y.git ~/scratch/y",
            "PATH=\"$HOME/bin:$PATH\" git status",
            "git -C ~/repo status",
            "git stash push -- ~/repo/README.md",
            "git log -1 --format='%h~%s'",
        ] {
            let out = f.command(&f.linked, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
    let out = f.command(
        &f.linked,
        "exec-guard",
        &format!("HOME={root}; python3 tool.py ~/.claude/settings.json"),
    );
    assert!(refused(&out), "{out:?}");
    // A quoted `~` is a directory named `~`, not the home.
    let out = f.command(&f.linked, "git-guard", "git -C '~/repo' status");
    assert_eq!(out.status.code(), Some(2), "{out:?}");
}

// Fable review round 5 major 3: Git's own output options are writes.
#[test]
fn git_output_options_cannot_write_enforcement_files() {
    let f = Fixture::new();
    let root = shell_path(&f.root);
    for hook in ["exec-guard", "git-guard"] {
        for (cwd, command) in [
            (
                &f.linked,
                format!("git archive HEAD -o {root}/.claude/settings.json"),
            ),
            (
                &f.root,
                "git archive -o .git/hooks/pre-commit HEAD".to_string(),
            ),
            (
                &f.root,
                "git archive -vo .git/hooks/pre-commit HEAD".to_string(),
            ),
            (
                &f.linked,
                format!("git format-patch -o {root}/.git/hooks HEAD~1"),
            ),
            (
                &f.linked,
                format!("git diff --output={root}/.claude/settings.json"),
            ),
            (
                &f.linked,
                format!("git diff --output {root}/.claude/settings.json"),
            ),
            (
                &f.linked,
                format!("git log --output={root}/.codeflow/policy.json -1"),
            ),
            (
                &f.linked,
                format!("git bundle create {root}/.claude/settings.json HEAD"),
            ),
            (
                &f.linked,
                format!("git config --file {root}/.codeflow/policy.json x.y z"),
            ),
            (
                &f.root,
                "git checkout-index -a --prefix=.claude/".to_string(),
            ),
            (
                &f.root,
                "git merge-file .claude/settings.json base.txt base.txt".to_string(),
            ),
            (
                &f.linked,
                "git -c alias.x='archive -o .claude/settings.json HEAD' x".to_string(),
            ),
        ] {
            let out = f.command(cwd, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        for (cwd, command) in [
            (&f.linked, "git archive -o build/x.tar HEAD"),
            (&f.linked, "git format-patch -o patches/ HEAD~1"),
            (&f.linked, "git diff --output=out.diff"),
            (&f.linked, "git log -1 --output-indicator-new=+"),
            (&f.linked, "git format-patch --stdout HEAD~1"),
            (&f.root, "git config --file .git/config user.name x"),
            (
                &f.root,
                "git merge-file -p .claude/settings.json base.txt base.txt",
            ),
        ] {
            let out = f.command(cwd, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
}

// Codex review round 9, C1 and C4: builtin wrappers, appends and sourced
// files change HOME too. Ordinary read-only calls pass; TSK-1002 AC-6
// requires secret-store file operands to refuse regardless of the program.
#[test]
fn wrapped_and_unproven_home_changes_are_read() {
    let f = Fixture::new();
    let root = shell_path(&f.root);
    let script = f.temp.path().join("home.sh");
    std::fs::write(&script, format!("HOME={root}\n")).unwrap();
    let script = shell_path(&script);
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            format!("command export HOME={root}; printf x > ~/.claude/settings.json"),
            format!("builtin export HOME={root}; printf x > ~/.claude/settings.json"),
            format!("command export HOME={root}; git clone https://example.test/x/y.git ~/.claude/skills/x"),
            format!("builtin export HOME={root}; git clone https://example.test/x/y.git ~/.claude/skills/x"),
            format!("command declare -x HOME={root}; git init ~/.claude/x"),
            "HOME+=/x; printf x > ~/.claude/settings.json".to_string(),
            format!("HOME={root}; HOME+=/.claude; printf x > ~/settings.json"),
            format!(". '{script}'; printf x > ~/.claude/settings.json"),
        ] {
            let out = f.command(&f.root, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        let command = format!("HOME={root}; git status -- ~/.claude/settings.json");
        let out = f.command(&f.root, hook, &command);
        assert_eq!(
            out.status.code(),
            Some(if hook == "exec-guard" { 2 } else { 0 }),
            "{hook}: {out:?}"
        );
        if hook == "exec-guard" {
            assert!(String::from_utf8_lossy(&out.stderr).contains("security.secret_reads"));
        }
        for command in [
            "export -n HOME; git status".to_string(),
            "command -v git && git status".to_string(),
        ] {
            let out = f.command(&f.root, hook, &command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
}

// Codex review round 9, C2: output operands are read with the shared
// option tables, so another option's value is never taken for the output
// and Git's accepted abbreviations count.
#[test]
fn git_outputs_follow_option_arity_and_abbreviations() {
    let f = Fixture::new();
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            "git merge-file --marker-size 7 .claude/settings.json base.json replacement.json",
            "git merge-file -L a -L b -L c .claude/settings.json base.json replacement.json",
            "git bundle create --version 2 .claude/settings.json HEAD",
            "git -c alias.x='bundle create --version 2 .claude/settings.json HEAD' x",
            "git checkout-index -f --pref=.claude/ -- settings.json",
            "git -c alias.x='checkout-index -f --pref=.claude/ -- settings.json' x",
            "git config --fil .claude/settings.local.json user.name x",
            "git config --file .claude/settings.local.json user.name x",
            "git -c alias.x='config --fil .claude/settings.local.json user.name x' x",
            "git format-patch -ko .git/hooks HEAD~1",
            "git merge-file -- .claude/settings.json base.json replacement.json",
            "git merge-file .claude/settings.json base.json replacement.json -- -p",
        ] {
            let out = f.command(&f.root, hook, command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        for command in [
            "git merge-file -p .claude/settings.json base.json replacement.json",
            "git merge-file .claude/settings.json base.json replacement.json -p",
            "git merge-file .claude/settings.json base.json replacement.json --stdout",
            "git merge-file -p -- .claude/settings.json base.json replacement.json",
            "git merge-file --marker-size 7 out.txt base.json replacement.json",
            "git bundle create --version 2 ../repo.bundle HEAD",
            "git checkout-index -f --pref=build/ -- settings.json",
            "git format-patch --stdout HEAD~1",
            "git diff --output-indicator-new=+ HEAD~1",
            "git archive -o build/x.tar HEAD",
            "git config --file build/x.conf user.name x",
        ] {
            let out = f.command(&f.root, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
}

// Codex review round 9, C3: a conditional include keyed on the new
// worktree's Git directory applies only once the worktree exists. Both the
// add and a later command there are refused while it sets a callback; a
// benign include and the ordinary lifecycle keep passing.
#[test]
fn worktree_add_reads_the_new_worktree_configuration() {
    let f = Fixture::new();
    let stage = f.temp.path().join("stage");
    for name in ["existingwt", "plainwt"] {
        std::fs::create_dir_all(stage.join(name)).unwrap();
    }
    let marker = shell_path(&f.temp.path().join("callback-marker"));
    let callback = f.temp.path().join("conditional.conf");
    std::fs::write(
        &callback,
        format!("[core]\n\tfsmonitor = printf ran > {marker}\n"),
    )
    .unwrap();
    let benign = f.temp.path().join("benign.conf");
    std::fs::write(&benign, "[user]\n\temail = work@example.com\n").unwrap();
    let root = shell_path(&f.root);
    for (name, file) in [("existingwt", &callback), ("plainwt", &benign)] {
        git(
            &f.root,
            &[
                "config",
                &format!("includeIf.gitdir:{root}/.git/worktrees/{name}.path"),
                &shell_path(file),
            ],
        );
    }
    let stage_path = shell_path(&stage);
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            format!("git -C {root} worktree add --detach {stage_path}/existingwt"),
            format!("git -C {root} worktree add --detach {stage_path}/existingwt && git -C {stage_path}/existingwt status"),
            format!("git -C {root} worktree add --detach {stage_path}/plainwt && git -C {stage_path}/plainwt status"),
            // Any worktree's include can apply to a new one (round 11).
            format!("git -C {root} worktree add --detach {stage_path}/plainwt"),
        ] {
            let out = f.command(&stage, hook, &command);
            assert_eq!(out.status.code(), Some(2), "{hook} {command}: {out:?}");
        }
        let out = f.command(&stage, hook, "git worktree list");
        assert_eq!(out.status.code(), Some(0), "{hook}: {out:?}");
    }
}

// Codex rounds 10 and 11 (R9-1, R10-1): Git derives the new worktree's
// administrative name through sanitizing and a counter (`work..1` becomes
// `work.1`, `work1.lock` and `work1.lock.lock` become `work1`, a taken
// `work1` becomes `work11`), so the guard does not predict it. An include
// keyed on any worktree's Git directory is read for every add; its callback
// runs during the add. A benign include, one keyed on the main Git
// directory and the ordinary lifecycle keep passing.
#[test]
fn worktree_add_reads_includes_of_every_admin_name() {
    for (requested, admin, taken) in [
        ("work..1", "work.1", false),
        ("work1.lock", "work1", false),
        ("work1.lock.lock", "work1", false),
        ("work1", "work11", true),
    ] {
        let f = Fixture::new();
        let root = shell_path(&f.root);
        let stage = f.temp.path().join("stage");
        std::fs::create_dir_all(stage.join(requested)).unwrap();
        if taken {
            let old = f.temp.path().join("old/work1");
            git(&f.root, &["worktree", "add", "--detach", &shell_path(&old)]);
        }
        let marker = f.temp.path().join("callback-marker");
        let callback = f.temp.path().join("callback.conf");
        std::fs::write(
            &callback,
            format!(
                "[core]\n\tfsmonitor = printf ran > {}\n",
                shell_path(&marker)
            ),
        )
        .unwrap();
        git(
            &f.root,
            &[
                "config",
                &format!("includeIf.gitdir:{root}/.git/worktrees/{admin}.path"),
                &shell_path(&callback),
            ],
        );
        let dest = shell_path(&stage.join(requested));
        for hook in ["exec-guard", "git-guard"] {
            for options in [
                "--detach",
                "-b task/new",
                "--lock --reason test -b task/new",
                "--orphan -b task/new",
            ] {
                let command = format!("git -C {root} worktree add {options} {dest}");
                let out = f.command(&stage, hook, &command);
                assert!(refused(&out), "{hook} {command}: {out:?}");
            }
        }
        #[cfg(unix)]
        {
            git(&f.root, &["worktree", "add", "--detach", &dest]);
            assert!(
                f.root.join(".git/worktrees").join(admin).is_dir(),
                "{requested}"
            );
            assert!(marker.exists(), "{requested}: the include did not apply");
        }
    }
    let f = Fixture::new();
    let root = shell_path(&f.root);
    let benign = f.temp.path().join("benign.conf");
    std::fs::write(&benign, "[user]\n\temail = work@example.com\n").unwrap();
    for (key, file) in [
        (
            format!("includeIf.gitdir:{root}/.git/worktrees/other.path"),
            &benign,
        ),
        (format!("includeIf.gitdir:{root}/.git.path"), &benign),
        ("includeIf.onbranch:task/**.path".to_string(), &benign),
    ] {
        git(&f.root, &["config", &key, &shell_path(file)]);
    }
    for hook in ["exec-guard", "git-guard"] {
        for command in [
            "git worktree add .worktrees/n -b task/n",
            "git worktree add --detach .worktrees/m",
            "git worktree list",
            "git worktree remove .worktrees/w",
            "git worktree prune",
        ] {
            let out = f.command(&f.root, hook, command);
            assert_eq!(out.status.code(), Some(0), "{hook} {command}: {out:?}");
        }
    }
    // Round 13 (R13-1): a conditional include that sets a hooks path, even
    // an empty one, changes where Git looks for hooks in the new worktree.
    for hooks in ["", "hooks-elsewhere"] {
        let f = Fixture::new();
        let root = shell_path(&f.root);
        let conf = f.temp.path().join("hooks.conf");
        std::fs::write(&conf, format!("[core]\n\thooksPath = \"{hooks}\"\n")).unwrap();
        git(
            &f.root,
            &[
                "config",
                &format!("includeIf.gitdir:{root}/.git/worktrees/hookswt.path"),
                &shell_path(&conf),
            ],
        );
        for hook in ["exec-guard", "git-guard"] {
            let out = f.command(
                &f.root,
                hook,
                "git worktree add --detach .worktrees/hookswt",
            );
            assert!(refused(&out), "{hook} hooksPath={hooks:?}: {out:?}");
            let out = f.command(&f.root, hook, "git worktree list");
            assert_eq!(out.status.code(), Some(0), "{hook}: {out:?}");
        }
    }
}

// Codex round 12, R11-1: Git reads an include by the spelling it was given,
// so a relative include inside a linked configuration file resolves from
// the link's directory, not the target's. The guard reads each include the
// same way; a benign child there still passes.
#[cfg(unix)]
#[test]
fn worktree_add_reads_linked_includes_by_their_logical_path() {
    for (spelling, callback_logical) in [
        ("direct", true),
        ("direct", false),
        ("alias", true),
        ("alias", false),
    ] {
        let f = Fixture::new();
        let root = shell_path(&f.root);
        let marker = f.temp.path().join("callback-marker");
        let physical = f.temp.path().join("physical");
        let logical = f.temp.path().join("logical");
        std::fs::create_dir_all(&physical).unwrap();
        std::fs::create_dir_all(&logical).unwrap();
        std::fs::write(
            physical.join("parent.conf"),
            "[include]\n\tpath = child.conf\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(physical.join("parent.conf"), logical.join("parent.conf"))
            .unwrap();
        let callback = format!(
            "[core]\n\tfsmonitor = printf ran > {}\n",
            shell_path(&marker)
        );
        let benign = "[user]\n\temail = work@example.com\n".to_string();
        let (at_logical, at_physical) = if callback_logical {
            (callback, benign)
        } else {
            (benign, callback)
        };
        std::fs::write(logical.join("child.conf"), at_logical).unwrap();
        std::fs::write(physical.join("child.conf"), at_physical).unwrap();
        // One case per spelling of the same file: the direct path, or the
        // path through a directory alias. Only one include is active, so a
        // guard that skipped the alias would be caught.
        let alias = f.temp.path().join("alias");
        std::os::unix::fs::symlink(&logical, &alias).unwrap();
        let include = if spelling == "alias" {
            alias.join("parent.conf")
        } else {
            logical.join("parent.conf")
        };
        git(
            &f.root,
            &[
                "config",
                &format!("includeIf.gitdir:{root}/.git/worktrees/linkedwt.path"),
                &shell_path(&include),
            ],
        );
        let stage = f.temp.path().join("stage");
        std::fs::create_dir_all(stage.join("linkedwt")).unwrap();
        let dest = shell_path(&stage.join("linkedwt"));
        let command = format!("git -C {root} worktree add --detach {dest}");
        for hook in ["exec-guard", "git-guard"] {
            let out = f.command(&stage, hook, &command);
            if callback_logical {
                assert!(refused(&out), "{spelling} {hook} {command}: {out:?}");
            } else {
                assert_eq!(
                    out.status.code(),
                    Some(0),
                    "{spelling} {hook} {command}: {out:?}"
                );
            }
        }
        // Real Git takes the logical child, and runs it during the add.
        git(&f.root, &["worktree", "add", "--detach", &dest]);
        let out = Command::new("git")
            .args(["config", "--get", "core.fsmonitor"])
            .current_dir(stage.join("linkedwt"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert_eq!(out.status.success(), callback_logical, "{out:?}");
        assert_eq!(
            marker.exists(),
            callback_logical,
            "marker for {callback_logical}"
        );
    }
}

// Codex round 13, R12-1: Git can reach the administrative directory through
// an alias and match a condition spelled with the resolved path, whose
// parent is not `worktrees`. The guard reads every conditional include,
// whatever its condition, so the add is refused; real Git applies it.
#[cfg(unix)]
#[test]
fn worktree_add_reads_includes_through_an_admin_alias() {
    for kind in ["gitdir", "gitdir/i"] {
        let f = Fixture::new();
        let root = shell_path(&f.root);
        let admin = f.root.join(".git/worktrees");
        let store = f.root.join(".git/admin-store");
        std::fs::rename(&admin, &store).unwrap();
        std::os::unix::fs::symlink(&store, &admin).unwrap();
        let marker = f.temp.path().join("callback-marker");
        let callback = f.temp.path().join("callback.conf");
        std::fs::write(
            &callback,
            format!(
                "[core]\n\tfsmonitor = printf ran > {}\n",
                shell_path(&marker)
            ),
        )
        .unwrap();
        let spelled = shell_path(&store.canonicalize().unwrap().join("aliaswt"));
        git(
            &f.root,
            &[
                "config",
                &format!("includeIf.{kind}:{spelled}.path"),
                &shell_path(&callback),
            ],
        );
        let stage = f.temp.path().join("stage");
        std::fs::create_dir_all(stage.join("aliaswt")).unwrap();
        let dest = shell_path(&stage.join("aliaswt"));
        let command = format!("git -C {root} worktree add --detach {dest}");
        for hook in ["exec-guard", "git-guard"] {
            let out = f.command(&stage, hook, &command);
            assert!(refused(&out), "{hook} {command}: {out:?}");
        }
        git(&f.root, &["worktree", "add", "--detach", &dest]);
        let out = Command::new("git")
            .args(["config", "--get", "core.fsmonitor"])
            .current_dir(stage.join("aliaswt"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(out.status.success(), "{kind}: {out:?}");
        assert!(marker.exists(), "{kind}: the include did not apply");
    }
}
