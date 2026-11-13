//! TSK-1476: the guards hold the operator-only path for project install
//! files. Every command here is a hook payload string fed to the built
//! guards and never run. A guard call that reaches no decision within
//! [`DEADLINE`] counts as a deadline, reported on its own and never read as
//! a refusal or a pass.
use super::{
    codeflow, git, init_repo, line_allows, shell_path, write_agent_policy, TARGETING_POLICY,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::{Duration, Instant};

/// How long one guard call may take before its row counts as a deadline.
const DEADLINE: Duration = Duration::from_secs(20);

const EXEC: &[&str] = &["exec-guard", "--contract", "3"];
const GIT: &[&str] = &["git-guard"];
const EDIT: &[&str] = &["edit-guard"];

/// The folders linked worktrees live in, with the branch of the fixture's
/// worktree in each.
const WORKTREES: &[(&str, &str)] = &[
    (".worktrees/v", "task/v"),
    (".claude/worktrees/w", "task/w"),
];

/// The worktree folders the shipped presets name.
const WORKTREE_FOLDERS: &[&str] = &[".worktrees", ".claude/worktrees"];

/// What one guard call returned.
enum Judged {
    Decided(Output),
    Deadline,
}

/// Feed `payload` to `codeflow hook <guard>` run in `cwd`.
fn judge(guard: &[&str], payload: &str, cwd: &Path) -> Judged {
    let mut command = codeflow();
    command
        .arg("hook")
        .args(guard)
        .current_dir(cwd)
        .env_remove("CLAUDE_PROJECT_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("binary spawns");
    child
        .stdin
        .take()
        .expect("stdin piped")
        .write_all(payload.as_bytes())
        .expect("stdin written");
    let started = Instant::now();
    loop {
        if child.try_wait().expect("child state reads").is_some() {
            return Judged::Decided(child.wait_with_output().expect("binary exits"));
        }
        if started.elapsed() >= DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            return Judged::Deadline;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A shell tool payload.
fn shell(command: &str, cwd: &Path) -> String {
    serde_json::json!({"tool_name": "Bash", "tool_input": {"command": command}, "cwd": cwd})
        .to_string()
}

/// A file tool payload.
fn file_tool(tool: &str, input: &serde_json::Value, cwd: &Path) -> String {
    serde_json::json!({"tool_name": tool, "tool_input": input, "cwd": cwd}).to_string()
}

fn shown(guard: &[&str], out: &Output) -> String {
    format!(
        "{}: exit {:?}: {}",
        guard[0],
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).trim()
    )
}

/// The install check's own refusal: its finding, under hook integrity,
/// naming the operator as the one who runs the verbs.
fn operator_refusal(out: &Output) -> bool {
    let stderr = String::from_utf8_lossy(&out.stderr);
    out.status.code() == Some(2)
        && stderr.contains("git.hook_integrity")
        && stderr.contains("install run `")
        && stderr.contains("only the operator runs `codeflow update` and `codeflow init`")
}

/// A refusal under hook integrity.
fn integrity_refusal(out: &Output) -> bool {
    out.status.code() == Some(2)
        && String::from_utf8_lossy(&out.stderr).contains("git.hook_integrity")
}

/// Wrong verdicts and deadlines, each listed at the end.
#[derive(Default)]
struct Tally {
    wrong: Vec<String>,
    deadlines: Vec<String>,
}

impl Tally {
    /// Each row is refused by one of `guards`, as `refused` reads it.
    fn refused(
        &mut self,
        guards: &[&[&str]],
        label: &str,
        payload: &str,
        cwd: &Path,
        refused: fn(&Output) -> bool,
    ) {
        let mut seen = Vec::new();
        let mut deadline = false;
        for guard in guards {
            match judge(guard, payload, cwd) {
                Judged::Decided(out) if refused(&out) => return,
                Judged::Decided(out) => seen.push(shown(guard, &out)),
                Judged::Deadline => deadline = true,
            }
        }
        if deadline {
            self.deadlines.push(label.to_string());
        } else {
            self.wrong
                .push(format!("should refuse: {label}\n  {}", seen.join("\n  ")));
        }
    }

    /// Each shell row in `rows` is refused, run in `cwd`.
    fn shell_refused(&mut self, rows: &[String], cwd: &Path, refused: fn(&Output) -> bool) {
        for row in rows {
            self.refused(&[EXEC, GIT], row, &shell(row, cwd), cwd, refused);
        }
    }

    /// Each shell row in `rows` is allowed by both shell guards, run in
    /// `cwd`. The exec guard may still answer with the strict text rule's
    /// own finding alone ([`line_allows`]).
    fn shell_allowed(&mut self, rows: &[String], cwd: &Path) {
        for row in rows {
            let payload = shell(row, cwd);
            let mut problems = Vec::new();
            let mut deadline = false;
            for guard in [EXEC, GIT] {
                match judge(guard, &payload, cwd) {
                    Judged::Decided(out) => {
                        let allowed = if guard == EXEC {
                            line_allows(&out)
                        } else {
                            out.status.code() == Some(0)
                        };
                        if !allowed {
                            problems.push(shown(guard, &out));
                        }
                    }
                    Judged::Deadline => deadline = true,
                }
            }
            if !problems.is_empty() {
                self.wrong
                    .push(format!("should allow: {row}\n  {}", problems.join("\n  ")));
            } else if deadline {
                self.deadlines.push(row.clone());
            }
        }
    }

    /// Each shell row in `rows` draws no install finding from exec-guard,
    /// run in `cwd`. A refusal by another rule of the line, such as a read
    /// the guard cannot verify, is that rule's and not this check's.
    fn install_allowed(&mut self, rows: &[String], cwd: &Path) {
        for row in rows {
            match judge(EXEC, &shell(row, cwd), cwd) {
                Judged::Decided(out) => {
                    if String::from_utf8_lossy(&out.stderr).contains("install run `") {
                        self.wrong
                            .push(format!("should allow: {row}\n  {}", shown(EXEC, &out)));
                    }
                }
                Judged::Deadline => self.deadlines.push(row.clone()),
            }
        }
    }

    fn finish(self) {
        assert!(
            self.deadlines.is_empty() && self.wrong.is_empty(),
            "deadline, no decision within {DEADLINE:?}, not a verdict ({}):\n{}\n\nwrong verdicts ({}):\n{}",
            self.deadlines.len(),
            self.deadlines.join("\n"),
            self.wrong.len(),
            self.wrong.join("\n")
        );
    }
}

/// A project on a task branch, in its own folder under `temp`, with
/// committed Claude settings and the targeting policy.
fn project(temp: &Path) -> PathBuf {
    let root = codeflow_core::portable_path::canonicalize(temp)
        .unwrap()
        .join("project");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    init_repo(&root, "task/x");
    std::fs::write(root.join(".claude/settings.json"), "{}\n").unwrap();
    git(&root, &["add", ".claude/settings.json"]);
    git(&root, &["commit", "-m", "chore: fixture settings"]);
    write_agent_policy(&root, TARGETING_POLICY);
    root
}

/// [`project`] with a committed hook shim and a linked worktree in each
/// worktree folder, so every worktree holds its own copies.
fn project_with_worktrees(temp: &Path) -> PathBuf {
    let root = project(temp);
    std::fs::create_dir_all(root.join(".codeflow/git-hooks")).unwrap();
    std::fs::write(root.join(".codeflow/git-hooks/pre-push"), "#!/bin/sh\n").unwrap();
    git(&root, &["add", ".codeflow/git-hooks/pre-push"]);
    git(&root, &["commit", "-m", "chore: fixture hook"]);
    for &(folder, branch) in WORKTREES {
        git(&root, &["worktree", "add", "-q", "-b", branch, folder]);
    }
    root
}

/// AC-1: a build of the binary other than the installed one, run as either
/// install verb, is refused in each form the guards read, and the refusal
/// names the operator.
#[test]
fn the_install_verbs_refuse_in_every_form_the_guards_read() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let at = shell_path(&root);
    let build = "./target/debug/codeflow";
    let rows = [
        format!("{build} update"),
        format!("{build} update --force"),
        format!("{at}/target/debug/codeflow update"),
        "target/release/codeflow init --yes".to_string(),
        format!("env -C {at} {build} update"),
        format!("nice -n 5 {build} update"),
        format!("timeout 60 {build} update"),
        format!("nohup {build} update"),
        format!("command {build} update"),
        format!("exec {build} init --yes"),
        format!("cd {at} && {build} update"),
        format!("true; {build} update"),
        format!("sh -c '{build} update'"),
        format!("bash -lc '{build} init --yes'"),
        format!("eval '{build} update'"),
        format!("echo update | xargs {build}"),
        "find . -name codeflow -type f -exec {} update \\;".to_string(),
        format!(r#"python3 -c "import subprocess; subprocess.run(['{build}', 'update'])""#),
        format!(r#"node -e "require('child_process').execFileSync('{build}', ['update'])""#),
        format!(r#"perl -e 'system("{build}", "init", "--yes")'"#),
        "cargo run -- update".to_string(),
        "cargo run -p codeflow-cli -- init --yes".to_string(),
        "cargo r --bin codeflow -- update --force".to_string(),
        "cargo run update".to_string(),
        "cargo r init --yes".to_string(),
        "cargo run --bin codeflow update --force".to_string(),
        "cargo +stable run -- init".to_string(),
        format!("B={build}; $B update"),
        format!("$(printf {build}) update"),
        "codeflow $V".to_string(),
        r#"codeflow "$@""#.to_string(),
        "codeflow $(printf update)".to_string(),
        format!("{build} ${{VERB}} --force"),
        "CODEFLOW update".to_string(),
        "CodeFlow init --yes".to_string(),
        "codeflow.exe update".to_string(),
        "./target/debug/CodeFlow.EXE update".to_string(),
    ];
    let mut tally = Tally::default();
    tally.shell_refused(&rows, &root, operator_refusal);
    tally.finish();
}

/// AC-1 and AC-2 through programs that start the command after their own
/// options or hand it the words they read, each of which keeps its own
/// grammar. The run is refused whichever of them starts it.
#[test]
fn the_install_verbs_refuse_through_any_launcher() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let build = "./target/debug/codeflow";
    let rows = [
        "sudo codeflow update".to_string(),
        format!("sudo -u root {build} init --yes"),
        "doas codeflow update".to_string(),
        "chrt -o 0 codeflow update".to_string(),
        format!("taskset -c 0 {build} update"),
        "parallel codeflow ::: update".to_string(),
        format!("echo update | parallel {build}"),
        "watch -n 60 codeflow update".to_string(),
        "watch 'codeflow update'".to_string(),
        "busybox env codeflow update".to_string(),
        "gsudo codeflow update".to_string(),
        "pkexec codeflow init --yes".to_string(),
        "npx codeflow update".to_string(),
        "npm exec -- codeflow update".to_string(),
        "npm exec codeflow init --yes".to_string(),
        "pnpm exec codeflow update".to_string(),
        format!("yarn exec {build} update"),
        "bun x codeflow update".to_string(),
        "pn exec codeflow update".to_string(),
        "xargs -n 1 codeflow < verbs.txt".to_string(),
        "echo update | xargs -I{} codeflow {}".to_string(),
    ];
    let mut tally = Tally::default();
    tally.shell_refused(&rows, &root, operator_refusal);
    tally.finish();
}

/// AC-2: the installed binary run as either install verb, in a form that
/// can write, is refused, and the refusal names the operator.
#[test]
fn the_install_verbs_refuse_with_the_installed_binary() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "codeflow update",
        "codeflow update --force",
        "codeflow update --pin 4.0.0",
        "codeflow update --diff update-report.txt",
        "codeflow update --diff=--help",
        "codeflow update --help --force",
        "codeflow init -h --yes",
        "codeflow init --yes",
        "codeflow init --minimal --yes",
        "codeflow init --workspace --yes",
        "/usr/local/bin/codeflow update",
        "~/.cargo/bin/codeflow update",
        "env codeflow update",
        "nice codeflow init --yes",
        "sh -c 'codeflow update'",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    tally.shell_refused(&rows, &root, operator_refusal);
    tally.finish();
}

/// AC-1: words the shell expands without a variable sign or a backtick
/// (braces, patterns and a leading `=`), in the program position, as the
/// first operand, and among the words cargo passes on, are read as words
/// the guard cannot resolve.
#[test]
fn words_the_shell_expands_refuse_where_the_verb_could_follow() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "codeflow {update,--force}",
        "codeflow {update,init}",
        "codeflow upd?te",
        "codeflow [u]pdate",
        "codeflow upd*",
        "./target/debug/codeflow {init,--yes}",
        "{codeflow,} update",
        "{./target/debug/codeflow,update}",
        "./target/debug/codeflo? update",
        "./target/debug/codefl[o]w init --yes",
        "=codeflow update",
        "=codeflow {update,--force}",
        "nice -n 5 codeflow upd?te",
        "cargo run -- $V",
        r#"cargo run -- "$@""#,
        "cargo run -- {update,--force}",
        "cargo run -- upd?te",
        "cargo r {init,--yes}",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    tally.shell_refused(&rows, &root, operator_refusal);
    tally.finish();
}

/// AC-1: a shell reading its input behind a launcher the reader does not
/// strip, a value assigned for a command that can read it, and a search
/// program's option that runs a program are judged.
#[test]
fn runs_behind_launchers_values_and_options_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "printf 'codeflow update' | chrt -o 0 sh",
        "echo codeflow init | taskset -c 0 bash",
        "PAGER='codeflow update' man ls",
        "export PAGER='codeflow update'; man ls",
        "rg --pre ./target/debug/codeflow x update",
        "rg --pre=codeflow pattern init",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    tally.shell_refused(&rows, &root, operator_refusal);
    tally.finish();
}

/// AC-1: an option, variable or editor command whose value a program runs
/// as a command is judged as that command, behind any launcher, for each
/// program and option the guard reads: search programs' pagers and
/// preprocessors, pager, editor and browser variables, Git's command
/// settings, editors' command options, and the suppliers and launchers
/// that run the words after them.
#[test]
fn command_valued_options_refuse_through_launchers() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "ack --pager='env codeflow update' pattern README.md",
        "ack --pager='nice codeflow update' pattern README.md",
        "ack --pager='command codeflow init --yes' pattern README.md",
        "ack --pager='/usr/bin/env ./target/debug/codeflow update' pattern README.md",
        "ack --pager='timeout 5 codeflow update' pattern README.md",
        "ack --pager='xargs codeflow' pattern README.md",
        "ack --pager 'env codeflow update' pattern README.md",
        "ack --pag='env codeflow update' pattern README.md",
        "ack --pager='sh -c \"codeflow update\"' pattern README.md",
        "ack --pager=sh -h 'codeflow update' notes.txt",
        "ag --pager='env codeflow update' pattern README.md",
        "ag --pager 'nice codeflow init --yes' pattern README.md",
        "ag --pag='env codeflow update' pattern README.md",
        "ag --pager=sh 'codeflow update' notes.txt",
        "rg --pre xargs pattern codeflow",
        "rg --pre 'env codeflow update' pattern docs",
        "export ACK_PAGER='env codeflow update'; ack pattern README.md",
        "ACK_PAGER='env codeflow init'; ack pattern README.md",
        "export GH_PAGER='env codeflow update'; gh pr list",
        "export GIT_PAGER='env codeflow update'; git log -1",
        "git -c core.pager='env codeflow update' log -1",
        "git -c core.editor='env codeflow update' commit",
        "less '+!codeflow update' notes.txt",
        "less '+!env codeflow update' notes.txt",
        "LESSOPEN='|env codeflow update %s' less notes.txt",
        "vim -c '!codeflow update' notes.txt",
        "vim '+!env codeflow update' notes.txt",
        "emacs --eval '(shell-command \"codeflow update\")'",
        "man -P 'env codeflow update' ls",
        "find . -name x -exec env codeflow update ';'",
        "echo update | xargs env codeflow",
        "watch -n 60 env codeflow update",
        "parallel env codeflow ::: update",
        "fd -x env codeflow update",
        "flock /tmp/l -c 'env codeflow update'",
        "script -q /dev/null env codeflow update",
        "tar -cf a.tar --use-compress-program='env codeflow update' notes.txt",
        "ssh -o ProxyCommand='env codeflow update' host",
        "sed '1e env codeflow update' notes.txt",
        "gh repo clone owner/repo -- -c core.sshCommand='env codeflow update'",
        "gh gist clone abc123 -- -c core.sshCommand='env codeflow update'",
        "ACK_PAGER=sh ack --nocolor -h 'codeflow update' notes.txt",
        "export GH_PAGER='bash -s'; gh search issues 'codeflow update'",
        "export ACK_PAGER=sh; true; ack --nocolor -h 'codeflow update' notes.txt",
        "export GH_PAGER=sh; cd docs && gh search issues 'codeflow update'",
        "export GH_EDITOR=sh; bash -c \"gh pr create --title 'codeflow init'\"",
        "export ACK_PAGER=sh; (unset ACK_PAGER); ack --nocolor -h 'codeflow update' notes.txt",
        "export GH_PAGER=sh; (GH_PAGER=less); gh search issues 'codeflow update'",
        "export GH_PAGER=sh; unset GH_PAGER | true; gh search issues 'codeflow update'",
        "export ACK_PAGER=sh; builtin readonly ACK_PAGER; unset ACK_PAGER; ack -h 'codeflow update' notes.txt",
        "export GH_PAGER=less; command -p read GH_PAGER < pager.txt; gh search issues 'codeflow update'",
        "export GH_PAGER=sh; env unset GH_PAGER; gh search issues 'codeflow update'",
        "export GH_PAGER=sh; unset GH_PAGER; gh search issues 'codeflow update'",
        "export GH_PAGER=sh; GH_PAGER=less; true; gh search issues 'codeflow update'",
        "export GH_PAGER=sh; { unset GH_PAGER; }; gh search issues 'codeflow update'",
        "export GH_PAGER=sh; command unset GH_PAGER; gh search issues 'codeflow update'",
        "export ACK_PAGER=less; printf -v {ACK_PAGER,sh}; ack --nocolor --nogroup -h 'codeflow update' notes.txt",
        "export ACK_PAGER=sh; readonly {ACK_PAGER,NOTE}; unset ACK_PAGER; ack --nocolor --nogroup -h 'codeflow update' notes.txt",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    tally.shell_refused(&rows, &root, operator_refusal);
    // Ordinary pagers, preprocessors and editor commands on a line that
    // does not name the binary with a verb draw no install finding.
    let controls: Vec<String> = [
        "ack --pager=less pattern README.md",
        "ack --pager='less -R' pattern README.md",
        "rg --pre ./tools/pdftext pattern docs",
        "export ACK_PAGER='less -R'; ack pattern README.md",
        "export GH_PAGER='less -R'; gh pr list",
        "git -c core.pager='less -R' log -1",
        "less +10 notes.txt",
        "vim -c 'set nu' notes.txt",
        "man -P less ls",
        "gh repo clone owner/repo -- --depth 1",
        "builtin echo hello; gh pr list",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    tally.install_allowed(&controls, &root);
    tally.finish();
}

/// AC-1: a builtin that writes names, a search or forge program, and a
/// pager value are outside the closed subset, so the line is judged on its
/// raw text, which names the binary with a verb.
#[test]
fn name_writers_and_data_programs_refuse_outside_known_forms() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "export GH_PAGER=less; typeset -m 'GH_*=sh'; gh search issues 'codeflow update'",
        "export GH_PAGER=less; builtin typeset -m 'GH_*=sh'; gh search issues 'codeflow update'",
        "export GH_PAGER=less; command typeset -m 'GH_*=sh'; gh search issues 'codeflow update'",
        "export GH_PAGER=x; read -p 'GH_PAGER[1]' y; gh search issues 'codeflow update'",
        "export GH_PAGER=less; private GH_PAGER=sh; gh search issues 'codeflow update'",
        "export GH_PAGER=less; trap 'GH_PAGER=sh' DEBUG; gh search issues 'codeflow update'",
        "set -k; gh GH_PAGER=sh search issues 'codeflow update'",
        "hash -p /bin/sh gh; echo 'codeflow update' | gh",
        "gh alias set --shell u 'codeflow update'; gh u",
        "export GH_PAGER=perl; gh search issues 'codeflow update'",
        "export GH_CONFIG_DIR=cfg; gh search issues 'codeflow update'",
        "export RIPGREP_CONFIG_PATH=rg.conf; rg 'codeflow update' docs",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    tally.shell_refused(&rows, &root, operator_refusal);
    tally.finish();
}

/// AC-1: a package manager that runs a package's binary by name, a
/// subcommand that runs a command, and a quoted option value naming a
/// command after its own `=` are judged as the run they start.
#[test]
fn package_managers_and_option_values_that_run_the_binary_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "yarn codeflow update",
        "pnpm codeflow update",
        "yarn run codeflow update",
        "pnpm run codeflow init --yes",
        "npm explore pkg -- codeflow update",
        "tar -cf a.tar --checkpoint=1 '--checkpoint-action=exec=codeflow update' notes.txt",
        "tar -cf a.tar --checkpoint-action 'exec=codeflow init --yes' notes.txt",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    tally.shell_refused(&rows, &root, operator_refusal);
    tally.finish();
}

/// AC-1: the word a package runner or manager runs is a program position,
/// so a word the shell expands there refuses, for each runner and manager
/// the guard knows, with and without a subcommand, and for each kind of
/// expansion. A word the shell expands among the words of a program the
/// guard does not know refuses when its text names the binary and a verb.
#[test]
fn expanded_words_a_package_runner_runs_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let mut rows: Vec<String> = Vec::new();
    for runner in [
        "yarn",
        "yarn run",
        "yarn exec",
        "yarn dlx",
        "pnpm",
        "pnpm run",
        "pnpm exec",
        "pnpm dlx",
        "pn",
        "npm exec --",
        "npm x",
        "npm explore pkg --",
        "npx",
        "bunx",
        "bun",
        "bun x",
        "pnpx",
        "corepack pnpm",
        "volta run",
        "uv run",
    ] {
        for word in [
            "{codeflow,update}",
            "{codeflow,} update",
            "codefl?w update",
            "=codeflow update",
        ] {
            rows.push(format!("{runner} {word}"));
        }
    }
    rows.push("pnpm run codefl[o]w init --yes".to_string());
    rows.push("mise exec -- {codeflow,update}".to_string());
    rows.push("tool --run {./target/debug/codeflow,init}".to_string());
    let mut tally = Tally::default();
    tally.shell_refused(&rows, &root, operator_refusal);
    tally.finish();
}

/// AC-1: npm reads its operands as data only after one of its own commands
/// that never runs a program, written in full or as a documented alias. An
/// abbreviation, a command that runs a script or a binary, an option the
/// guard does not know before the command, and an expanded word there
/// leave npm read as a package runner.
#[test]
fn npm_runs_the_binary_after_any_word_but_its_own_commands_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "npm exe -- codeflow update",
        "npm exe codeflow update",
        "npm expl pkg -- codeflow update",
        "npm explo pkg -- codeflow update",
        "npm explor pkg -- codeflow update",
        "npm run codeflow update",
        "npm run-script codeflow update",
        "npm rum codeflow update",
        "npm urn codeflow init",
        "npm t -- codeflow update",
        "npm --prefix codeflow exe -- codeflow update",
        "npm -g exe codeflow update",
        "npm --registry update exe -- codeflow update",
        "npm --reg update exe -- codeflow update",
        "npm -- exe codeflow update",
        "npm exe -- {codeflow,update}",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    tally.shell_refused(&rows, &root, operator_refusal);
    tally.finish();
}

/// AC-1: on a shell that reads a backslash as a path separator, an
/// unquoted path of that form is read.
#[test]
fn a_backslash_path_refuses_on_a_shell_that_reads_it_as_a_separator() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let mut tally = Tally::default();
    for row in [
        r".\target\debug\codeflow.exe update",
        r".\target\release\codeflow init --yes",
        r"& .\target\debug\codeflow.exe update --force",
    ] {
        tally.refused(
            &[EXEC],
            row,
            &file_tool("PowerShell", &serde_json::json!({"command": row}), &root),
            &root,
            operator_refusal,
        );
    }
    tally.finish();
}

/// AC-1 through a Git runner: a program value Git runs as a command line
/// reaches the install check itself.
#[test]
fn the_install_check_fires_through_a_git_runner() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "git -c core.pager='codeflow update' log -1",
        "git -c core.pager='./target/debug/codeflow init --yes' log -1",
        // A shell reading its input as the program Git runs: the line is
        // judged on its raw text.
        "git -c core.pager=sh log -1 --format='codeflow update'",
        "git -c core.pager='bash -s' log -1 --format='codeflow init --yes'",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    for row in &rows {
        tally.refused(&[EXEC], row, &shell(row, &root), &root, operator_refusal);
    }
    let controls = vec!["git -c core.pager=sh log -1".to_string()];
    tally.install_allowed(&controls, &root);
    // A shell alias was refused before the install check by the rule for
    // Git runners the guard cannot read, and still is.
    let alias = "git -c alias.u='!./target/debug/codeflow update' u";
    tally.refused(
        &[EXEC, GIT],
        alias,
        &shell(alias, &root),
        &root,
        integrity_refusal,
    );
    tally.finish();
}

/// AC-1 through a program handed output or a file to read: a Git pager,
/// editor, sequence editor, text converter or `grep -O` program, and a
/// pager or editor variable, put the line outside the closed subset, so it
/// is judged on its raw text.
#[test]
fn programs_handed_output_refuse_outside_their_viewer_options() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "git -c core.pager=perl log -1 --format='system(\"codeflow update\");'",
        "git -c pager.log=python3 log -1 --format='codeflow update'",
        "git -c core.editor=ruby commit --allow-empty -e -m 'codeflow update'",
        "git -c core.editor='awk -f /dev/stdin' commit --allow-empty -e -m 'codeflow update'",
        "git -c sequence.editor=node rebase -i -x 'echo codeflow update' HEAD~1",
        "git -c core.pager='emacs --script /dev/stdin' log -1 --format='codeflow update'",
        "git -c core.pager='less -k keys' log -1 --format='codeflow update'",
        "git grep -Operl 'codeflow update'",
        "git -c diff.x.textconv=perl log -p -1 --grep='codeflow update'",
        "ACK_PAGER='emacs --script /dev/stdin' ack -h 'codeflow update' notes.txt",
        "GH_PAGER='vim -S keys.vim' gh search issues 'codeflow update'",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    for row in &rows {
        tally.refused(&[EXEC], row, &shell(row, &root), &root, operator_refusal);
    }
    tally.finish();
}

/// AC-1 through a Git tool runner (a filter, a diff or merge driver, a
/// signing program, fsmonitor, a hook or trailer command), given on the
/// line: the line is outside the closed subset and judged on its raw text.
#[test]
fn tool_runners_reading_code_from_their_input_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "git -c filter.x.clean=perl log -1 --grep='codeflow update'",
        "git -c filter.x.smudge='awk -f -' log -1 --grep='codeflow update'",
        "git -c filter.x.clean='nice python3' log -1 --grep='codeflow update'",
        "git -c filter.x.clean='perl x.pl' log -1 --grep='codeflow update'",
        "git -c diff.external=ruby log -p -1 --grep='codeflow update'",
        "GIT_EXTERNAL_DIFF=node git log -p -1 --grep='codeflow update'",
        "git -c merge.x.driver=php log -1 --grep='codeflow update'",
        "git -c gpg.program=lua log --show-signature -1 --grep='codeflow update'",
        "git -c core.fsmonitor=tclsh log -1 --grep='codeflow update'",
        "git -c hook.x.command=perl log -1 --grep='codeflow update'",
        "git -c trailer.x.command=perl interpret-trailers --trailer 'x=codeflow update'",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    for row in &rows {
        tally.refused(&[EXEC], row, &shell(row, &root), &root, operator_refusal);
    }
    tally.finish();
}

/// AC-1 by the closed subset: a line that names the binary with a verb is
/// data only when every command is a listed program with listed options
/// and no construct outside the model. One row per kind of form outside
/// it: a program off the list, an option off a program's list, each
/// construct, each route by which state the line does not show steers a
/// program, and an interpreter as a Git runner or a fed program.
#[test]
fn every_line_outside_the_closed_subset_that_names_the_verbs_refuses() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        // Programs off the list.
        "ack 'codeflow update' docs",
        "gh search issues 'codeflow update'",
        "sed -n p 'codeflow update'",
        "less -R 'codeflow update'",
        // Options off a listed program's list.
        "grep -z 'codeflow update' notes.md",
        "grep -nZ 'codeflow update' notes.md",
        "rg --no-config --pre ./tools/pdftext 'codeflow update' docs",
        "rg --no-config -z 'codeflow update' docs",
        "git --no-pager log -p --grep='codeflow update'",
        "git --no-pager log --format='%G? codeflow update'",
        "git --no-pager log --pretty=custom --grep='codeflow update'",
        "git --no-pager grep -O 'codeflow update'",
        "git --no-pager grep --textconv 'codeflow update'",
        "printf -v x 'codeflow update'",
        // Constructs outside the model.
        "X=1 grep 'codeflow update' notes.md",
        "export X=1; grep 'codeflow update' notes.md",
        "declare X=1; echo 'codeflow update'",
        "read X < notes.md; echo 'codeflow update'",
        "alias grep=sh; grep 'codeflow update' notes.md",
        "grep() { sh; }; grep 'codeflow update' notes.md",
        "eval \"echo 'codeflow update'\"",
        "source ./x; echo 'codeflow update'",
        "exec echo 'codeflow update'",
        "builtin echo 'codeflow update'",
        "command echo 'codeflow update'",
        "cat <(echo 'codeflow update')",
        "(echo 'codeflow update')",
        "{ echo 'codeflow update'; }",
        "cat <<'EOF'\ncodeflow update\nEOF",
        "cat <<< 'codeflow update'",
        "echo 'codeflow update' >| notes.txt",
        "echo 'codeflow update' 3> notes.txt",
        "grep 'codeflow update' *.md",
        "echo notes.md | xargs grep 'codeflow update'",
        "find . -name x -exec grep 'codeflow update' {} +",
        "echo 'codeflow update' & wait",
        "echo 'codeflow update' |& cat",
        // State the line does not show.
        "rg 'codeflow init' docs",
        "RIPGREP_CONFIG_PATH=rg.conf rg --no-config 'codeflow init' docs",
        "git log --grep='codeflow update'",
        "GIT_PAGER=sh git --no-pager log --grep='codeflow update'",
        "git -c core.pager=less --no-pager log --grep='codeflow update'",
        "git --no-pager -c log.showSignature=true log --grep='codeflow update'",
        "GREP_OPTIONS=-r grep 'codeflow update' notes.md",
        "git --no-pager show HEAD -- 'codeflow update'",
        // An interpreter as a Git runner or a fed program.
        "git -c diff.external='python3 -c \"import os,sys;os.system(sys.argv[1])\"' \
         diff -- 'codeflow update'",
        "git -c diff.external='python3 -c \"import os,sys;os.system(sys.argv[1])\"' \
         log -p -1 -- 'codeflow update'",
        "echo 'os.execute(\"codeflow update\")' | lua -e \"\" -",
        "lua -e \"\" - <<< 'os.execute(\"codeflow update\")'",
        "lua ./run.lua 'codeflow update'",
        "git -c core.pager='lua -e \"\" -' log -1 --format='os.execute(\"codeflow update\")'",
        "GIT_PAGER='lua -' git log -1 --format='os.execute(\"codeflow update\")'",
        "git -c filter.x.clean='lua -e \"\" -' log -1 --grep='codeflow update'",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    for row in &rows {
        tally.refused(&[EXEC], row, &shell(row, &root), &root, operator_refusal);
    }
    tally.finish();
}

/// A listed program is admitted only where its own grammar proves every
/// argument inert. A printf format that reads an argument as a number,
/// directly or through an escape that spells a conversion, a ripgrep
/// `--no-config` that is not its first argument or that an option takes as
/// its value, a Git search (an object read can fetch through a configured
/// transport), the binary by a path, and a redirection to a device each
/// leave the line to its raw text. The guard judges a line the same way for
/// bash, zsh and sh, so one row covers each shell.
#[test]
fn listed_programs_whose_own_grammar_can_run_code_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "printf '%d' 'argv[$(codeflow update)]'",
        "printf '%i\\n' 'a[$(codeflow update)]'",
        "printf '%n' 'codeflow update'",
        "printf '%*s' 'a[$(codeflow update)]' x",
        "printf '%5s' 'codeflow update'",
        "printf '%1$s' 'codeflow update'",
        "printf '%b' 'codeflow update'",
        "printf '\\045d' 'a[$(codeflow update)]'",
        "printf '\\u0025d' 'a[$(codeflow update)]'",
        "printf -- '%d' 'a[$(codeflow update)]'",
        "printf '%s\\n' 'codeflow update' > input.txt; rg -e --no-config input.txt",
        "rg -e --no-config 'codeflow update'",
        "rg --regexp --no-config 'codeflow update'",
        "rg --regexp=--no-config 'codeflow update'",
        "rg -e--no-config 'codeflow update'",
        "rg -ie --no-config 'codeflow update'",
        "rg -g --no-config 'codeflow update' docs",
        "rg -i --no-config 'codeflow update' docs",
        "git --no-pager grep -e 'codeflow update' HEAD -- ordinary.txt",
        "git --no-pager log --grep='codeflow update'",
        "git -P log -1 --format='%h %s' --grep='codeflow update'",
        "git --no-pager --no-lazy-fetch log --grep='codeflow update'",
        "./target/debug/codeflow update --help",
        "echo 'codeflow update' > /dev/tcp/127.0.0.1/9",
        "cat < /dev/udp/127.0.0.1/9; echo 'codeflow update'",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    for row in &rows {
        tally.refused(&[EXEC], row, &shell(row, &root), &root, operator_refusal);
    }
    // A Git search refuses even when the repository configures only the
    // filters `git lfs install` writes, which r12 admitted.
    let line = "git --no-pager grep -e 'codeflow update' HEAD -- notes.txt".to_string();
    for (key, value) in [
        ("filter.lfs.process", "git-lfs filter-process"),
        ("filter.lfs.clean", "git-lfs clean -- %f"),
        ("filter.lfs.smudge", "git-lfs smudge -- %f"),
    ] {
        git(&root, &["config", key, value]);
    }
    tally.refused(
        &[EXEC],
        &line,
        &shell(&line, &root),
        &root,
        operator_refusal,
    );
    tally.finish();
}

/// AC-2 by the closed subset: its listed forms that name the binary with a
/// verb draw no install finding, and lines that do not name it stay clean
/// whatever form they take.
#[test]
fn the_closed_subset_and_lines_not_naming_the_binary_stay_clean() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        "grep -n 'codeflow update' notes.md",
        "rg --no-config 'codeflow init' docs",
        "rg --no-config -n -e 'codeflow update' -- docs",
        "echo 'run codeflow update'",
        "printf '%s\\n' 'run codeflow update'",
        "printf '%s\\n' 'codeflow update' > /dev/null",
        "echo 'codeflow update' > /dev/null",
        "grep -c 'codeflow init' notes.md 2>/dev/null",
        "grep 'codeflow update' notes.md > /dev/null 2>&1",
        "grep -c 'codeflow update' < notes.md",
        "git --no-pager log --grep=update --grep=codeflow --all-match",
        "grep -rn 'codeflow update' docs | head -n 5",
        "codeflow update --help",
        "ACK_PAGER=less ack foo",
        "GIT_PAGER=less git log",
        "git -c filter.lfs.process='git-lfs filter-process' \
         -c filter.lfs.clean='git-lfs clean -- %f' \
         -c filter.lfs.smudge='git-lfs smudge -- %f' log -1",
        "export X=1; make",
        "git lfs pull",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    tally.install_allowed(&rows, &root);
    tally.finish();
}

/// An output redirection is data only to `/dev/null`: a target's name says
/// nothing of what it is when the line runs (a FIFO a running shell reads,
/// a descriptor path, a link to either, or a file swapped for one after the
/// guard looked), so a line writing anywhere else that names the binary with
/// a verb refuses, a FIFO, a link and a plain file alike. Input from a
/// named file stays data, since a listed program only reads it.
#[test]
fn output_redirections_other_than_to_dev_null_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    std::fs::write(root.join("plain.txt"), "notes\n").unwrap();
    std::fs::write(root.join("notes.md"), "codeflow update\n").unwrap();
    #[cfg(unix)]
    {
        let made = std::process::Command::new("mkfifo")
            .arg(root.join("commands.fifo"))
            .status()
            .unwrap();
        assert!(made.success(), "mkfifo failed");
        std::os::unix::fs::symlink(root.join("commands.fifo"), root.join("link.txt")).unwrap();
    }
    let rows: Vec<String> = [
        "printf '%s\\n' 'codeflow update' > commands.fifo",
        "printf '%s\\n' 'codeflow update' > link.txt",
        "printf '%s\\n' 'codeflow update' > /proc/self/fd/3",
        "printf '%s\\n' 'codeflow update' > plain.txt",
        "echo 'codeflow update' >> plain.txt",
        "echo 'codeflow update' >| plain.txt",
        "echo 'codeflow update' &> plain.txt",
        "echo 'codeflow update' 2> plain.txt",
        "echo 'codeflow update' 3> plain.txt",
        "echo 'codeflow update' 1>&2",
        "grep 'codeflow update' notes.md 2>&1 > /dev/null",
        "grep 'codeflow update' notes.md > /dev/null 2> plain.txt",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    for row in &rows {
        tally.refused(&[EXEC], row, &shell(row, &root), &root, operator_refusal);
    }
    let allowed: Vec<String> = [
        "printf '%s\\n' 'codeflow update' > /dev/null",
        "grep -c 'codeflow init' notes.md 2>/dev/null",
        "grep 'codeflow update' notes.md > /dev/null 2>&1",
        "cat < notes.md; echo 'codeflow init'",
        "grep -c 'codeflow update' < plain.txt",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    tally.install_allowed(&allowed, &root);
    tally.finish();
}

/// A line outside the closed subset is judged on the text its shell and its
/// printing programs produce, not only on its raw text: shell quoting and
/// backslashes, `$'...'`, and the escapes `printf`, `echo` and `print`
/// decode, in any combination, cannot hide the binary's name or a verb from
/// a FIFO a running shell reads.
#[test]
fn text_the_shell_and_printing_programs_produce_refuses() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    #[cfg(unix)]
    {
        let made = std::process::Command::new("mkfifo")
            .arg(root.join("commands.fifo"))
            .status()
            .unwrap();
        assert!(made.success(), "mkfifo failed");
    }
    let reused = format!(
        "printf '%c' {}$'\\n' c o d e f l o w ' ' u p d a t e $'\\n' > commands.fifo",
        "'#' ".repeat(16)
    );
    let rows: Vec<String> = [
        r"printf cod\eflow' update\n' > commands.fifo",
        r"printf '\ncodeflow update' > commands.fifo",
        r"printf 'codeflow\x20update' > commands.fifo",
        r"printf '\143odeflow update' > commands.fifo",
        r"printf '\u0063odeflow update' > commands.fifo",
        r"printf '%b' '\x63odeflow update' > commands.fifo",
        r"printf '%.8s update' 'codeflowXYZ' > commands.fifo",
        r"echo -e 'codeflow\tupdate' > commands.fifo",
        r"echo '\0143odeflow update' > commands.fifo",
        r"printf '%s\n' $'\x63odeflow update' > commands.fifo",
        r"echo -e co\de$'flow'' up\u0064ate' > commands.fifo",
        r#"sh -c "printf '\\143odeflow update' > commands.fifo""#,
        r"printf 'codeflow upd%xte' 10 > commands.fifo",
        r"printf 'co%xflow update' 222 > commands.fifo",
        r#"printf '%xo%xflow update' "$a" "$b" > commands.fifo"#,
        r#"a=code; printf '%sflow update' "$a" > commands.fifo"#,
        r"printf '\143odeflow%1supdate\n' '' > commands.fifo",
        r"printf '#\c\n\143odeflow update\n' > commands.fifo",
        reused.as_str(),
        r"printf '%5s\n' notes > out.txt",
        r"printf '%s\n' a b > out.txt",
        r"echo -e 'a\tb' > notes.txt",
        r"printf '\x63odeflow\n' > notes.txt",
        r"printf '%x\n' 255 > notes.txt",
        r#"x='\143odeflow update'; echo "$x" > commands.fifo"#,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    for row in &rows {
        tally.refused(&[EXEC], row, &shell(row, &root), &root, operator_refusal);
    }
    let allowed: Vec<String> = [
        r"printf '%s\n' 'notes' > notes.txt",
        r"printf 'codeflow\n' > notes.txt",
        r#"a=notes; printf '%s\n' "$a" > notes.txt"#,
        "printf '%s' notes > out.txt",
        "echo notes > out.txt",
        r"printf '%s\n' notes > out.txt",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    tally.install_allowed(&allowed, &root);
    tally.finish();
}

/// A Git call that names the binary with a verb refuses whatever programs
/// the repository's configuration has it run, an interpreter above all.
#[test]
fn a_git_call_naming_the_verbs_refuses_whatever_git_runs() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let line = "git --no-pager log -1 --grep='codeflow update'".to_string();
    let mut tally = Tally::default();
    for (key, value) in [
        ("filter.x.clean", "lua -e \"\" -"),
        (
            "diff.external",
            "python3 -c \"import os,sys;os.system(sys.argv[1])\"",
        ),
        ("core.fsmonitor", "perl"),
    ] {
        git(&root, &["config", key, value]);
        tally.refused(
            &[EXEC],
            &line,
            &shell(&line, &root),
            &root,
            operator_refusal,
        );
        git(&root, &["config", "--unset", key]);
    }
    tally.finish();
}

/// AC-6: words that only name the verbs, given outside the closed subset,
/// are judged on their raw text and refuse: the over-refusal the closed
/// list accepts, each said again through a listed form or a file. Lines
/// that do not name the binary with a verb stay allowed.
#[test]
fn words_that_name_the_verbs_outside_the_closed_subset_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        r#"codeflow task new --title "the operator runs codeflow update""#,
        r#"codeflow feedback new "codeflow init wrote a file twice""#,
        r#"codeflow epic new --title "codeflow update reads the pin""#,
        r#"[ "$x" = "codeflow update" ]"#,
        r#"test "$x" = "codeflow init""#,
        "cargo nextest run update",
        "cargo +stable nextest run init",
        "cargo run --example init",
        "cargo run -p codeflow-cli -- task update TSK-1",
        r#"export NOTE="run codeflow update""#,
        r#"NOTE="run codeflow update"; echo done"#,
        "npm --prefix codeflow update",
        "npm -C codeflow update",
        "npm view codeflow init",
        "make -C codeflow init",
        "cp -r codeflow update",
        "ag 'codeflow init' docs",
        "ack 'codeflow update' docs",
        "ACK_PAGER=less ack 'codeflow update' docs",
        r#"jq '.steps[] | select(. == "codeflow update")' plan.json"#,
        "gh search issues -- 'codeflow update'",
        "gh pr list --search 'codeflow update'",
        "rg -n -i 'codeflow update' docs",
        "git -c core.pager='less -R' log -1 --format='codeflow update'",
        "git commit -m 'docs: say the operator runs codeflow update'",
        "git commit -F - <<'EOF'\ndocs: say the operator runs codeflow update\nEOF",
        "git --no-pager log --grep='codeflow update'",
        "git --no-pager grep -n 'codeflow update' -- docs",
        "rg -i --no-config 'codeflow update' docs",
        "./target/debug/codeflow update --help",
        "printf -- '%s %s\\n' codeflow update > notes.txt",
        "printf 'codeflow update\\n' > notes.txt",
        "echo 'codeflow update' >> notes.txt",
        "grep -n 'codeflow update' notes.md 2>&1 | wc -l",
        "ls codeflow 'update\\n'",
        "printf '%s\\n' a b > notes.txt",
        "printf '%d\\n' 5 > notes.txt",
        "echo -e 'a\\tb' > notes.txt",
        "python3 <<< $'print(1)\\nprint(2)'",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    for row in &rows {
        tally.refused(&[EXEC], row, &shell(row, &root), &root, operator_refusal);
    }
    let allowed: Vec<String> = [
        "npm update codeflow",
        "npm up codeflow",
        "npm i codeflow",
        "npm uninstall codeflow",
        "yarn add codeflow",
        "pnpm add -D codeflow",
        "rg --pre ./tools/pdftext pattern docs",
        "cargo run --bin codeflow -- task status",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    tally.install_allowed(&allowed, &root);
    tally.finish();
}

/// AC-1 and AC-2 by location, with no exemption by location: a run in a
/// linked worktree, in a separate scratch repository under the temporary
/// folder, reached from the project by a directory change, or in a folder
/// that is no repository yet, is refused like a run in the project.
#[test]
fn the_install_verbs_refuse_in_every_folder() {
    let temp = tempfile::tempdir().unwrap();
    let root = project_with_worktrees(temp.path());
    let at = shell_path(&root);
    let scratch_temp = tempfile::tempdir().unwrap();
    let scratch = codeflow_core::portable_path::canonicalize(scratch_temp.path())
        .unwrap()
        .join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    init_repo(&scratch, "main");
    let s = shell_path(&scratch);
    let plain_temp = tempfile::tempdir().unwrap();
    let plain = codeflow_core::portable_path::canonicalize(plain_temp.path())
        .unwrap()
        .join("plain");
    std::fs::create_dir_all(&plain).unwrap();

    let mut tally = Tally::default();
    tally.shell_refused(
        &[
            "codeflow update".to_string(),
            format!("{at}/target/debug/codeflow update"),
        ],
        &root.join(".worktrees/v"),
        operator_refusal,
    );
    tally.shell_refused(
        &[
            "codeflow init --yes".to_string(),
            "codeflow update".to_string(),
            format!("{at}/target/debug/codeflow init --yes"),
        ],
        &scratch,
        operator_refusal,
    );
    tally.shell_refused(
        &[
            format!("env -C {s} codeflow init --yes"),
            format!("cd {s} && codeflow update"),
            format!("cd {s} && {at}/target/debug/codeflow init --yes"),
        ],
        &root,
        operator_refusal,
    );
    tally.shell_refused(
        &["codeflow init --yes".to_string()],
        &plain,
        operator_refusal,
    );
    tally.finish();
}

/// AC-2: a form that writes nothing stays allowed, and so do ordinary
/// commands that name the verbs only as data or run other programs' verbs
/// of the same name.
#[test]
fn forms_that_write_nothing_and_ordinary_commands_stay_allowed() {
    let temp = tempfile::tempdir().unwrap();
    let root = project(temp.path());
    let rows: Vec<String> = [
        // Forms that write nothing.
        "codeflow update --help",
        "codeflow update -h",
        "codeflow init --help",
        "codeflow init -h",
        "codeflow help update",
        "codeflow help init",
        "codeflow --version",
        "codeflow -V",
        "codeflow --help",
        "./target/debug/codeflow --version",
        // Other codeflow verbs, and names of the binary as data.
        "codeflow doctor",
        "codeflow orient",
        "codeflow test --mode quick",
        "codeflow validate --docs",
        "codeflow task status",
        "codeflow hook exec-guard",
        "command -v codeflow",
        "which codeflow",
        "cargo run -- doctor",
        "cargo build",
        "cargo test update",
        // Other programs' verbs of the same name.
        "cargo init --vcs none --name scratch scratch",
        "npm init -y",
        "brew update",
        "rustup update",
        // The words as data.
        "echo the operator runs codeflow update",
        "printf 'codeflow update\\n' > /dev/null",
        "grep -rn 'codeflow update' docs",
        "git commit -F msg.txt",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    tally.shell_allowed(&rows, &root);
    tally.finish();
}

/// AC-3: a write to a linked worktree's copy of the harness settings, hooks
/// or policy is refused for each shell route, from the main checkout and
/// from inside the worktree.
#[test]
fn worktree_copies_refuse_every_shell_write_route() {
    let temp = tempfile::tempdir().unwrap();
    let root = project_with_worktrees(temp.path());
    let mut tally = Tally::default();
    for &(folder, _) in WORKTREES {
        let rows = [
            format!("echo '{{}}' > {folder}/.claude/settings.json"),
            format!("printf x | tee {folder}/.codeflow/policy.json"),
            format!("cp base.txt {folder}/.claude/settings.json"),
            format!("mv base.txt {folder}/.codeflow/policy.json"),
            format!("ln -sf base.txt {folder}/.claude/settings.local.json"),
            format!("sed -i.bak s/a/b/ {folder}/.claude/settings.json"),
            format!("echo x > {folder}/.codeflow/git-hooks/pre-push"),
            format!("rm {folder}/.codeflow/policy.json"),
            format!(r#"python3 -c "open('{folder}/.codeflow/policy.json', 'w').write('x')""#),
        ];
        tally.shell_refused(&rows, &root, integrity_refusal);
        let inside = [
            "echo x > .claude/settings.json".to_string(),
            "cp base.txt .codeflow/policy.json".to_string(),
            "echo x > .codeflow/git-hooks/pre-push".to_string(),
        ];
        tally.shell_refused(&inside, &root.join(folder), integrity_refusal);
    }
    tally.finish();
}

/// AC-3: a write to a linked worktree's copy through a native file tool is
/// refused by the edit guard, for the Claude, Grok and Codex tool forms.
/// Codex seats keep these copies guarded by the hooks alone, so the Codex
/// rows are their protection.
#[test]
fn worktree_copies_refuse_native_file_tools() {
    let temp = tempfile::tempdir().unwrap();
    let root = project_with_worktrees(temp.path());
    let mut tally = Tally::default();
    for &(folder, _) in WORKTREES {
        for relative in [
            ".claude/settings.json",
            ".codeflow/policy.json",
            ".codeflow/git-hooks/pre-push",
        ] {
            let target = shell_path(&root.join(folder).join(relative));
            let payloads = [
                (
                    "Write",
                    serde_json::json!({"file_path": target, "content": "{}"}),
                ),
                (
                    "Edit",
                    serde_json::json!({"file_path": target, "old_string": "{}", "new_string": "{ }"}),
                ),
                ("write", serde_json::json!({"path": target, "content": "x"})),
                (
                    "apply_patch",
                    serde_json::json!({"command": format!(
                        "*** Begin Patch\n*** Update File: {target}\n@@\n-{{}}\n+{{ }}\n*** End Patch\n"
                    )}),
                ),
            ];
            for (tool, input) in payloads {
                tally.refused(
                    &[EDIT],
                    &format!("{tool} {folder}/{relative}"),
                    &file_tool(tool, &input, &root),
                    &root,
                    integrity_refusal,
                );
            }
        }
    }
    tally.finish();
}

/// AC-3: the shipped Claude presets name no worktree path. A deny on a
/// worktree's copy would apply to git's own writes in a sandboxed session
/// too, so `git worktree add`, `merge` and `checkout` would fail there. The
/// guard rows above hold those copies.
#[test]
fn shipped_claude_presets_leave_the_worktree_copies_to_the_guards() {
    let presets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/settings");
    let mut named = Vec::new();
    for name in ["default.json", "acceptEdits.json", "bypass-sandboxed.json"] {
        let text = std::fs::read_to_string(presets.join(name)).unwrap();
        for folder in WORKTREE_FOLDERS {
            if text.contains(&format!("{folder}/*/")) {
                named.push(format!("{name}: {folder}"));
            }
        }
    }
    assert!(named.is_empty(), "presets name worktree paths: {named:?}");
}

/// AC-4: every guard and hook message that names an install verb as the
/// way to clear it names the operator as the one who runs it: the guard
/// remedies, the policy warnings the hooks print, the messages the hook
/// entry points and the session start print, and the fallback line of
/// every shipped hook entry and git hook shim.
#[test]
fn every_guard_and_hook_message_names_the_operator() {
    let mut unnamed = Vec::new();
    let names_a_verb =
        |line: &str| line.contains("codeflow update") || line.contains("codeflow init");
    let mut catalogued = 0;
    for clearing in codeflow_core::remedy::CATALOG {
        if names_a_verb(clearing.text) {
            catalogued += 1;
            if !clearing.text.contains("operator") {
                unnamed.push(format!("remedy {}", clearing.name));
            }
        }
    }
    assert!(
        catalogued >= 15,
        "the catalogued remedies naming a verb were not found: {catalogued}"
    );
    // Messages outside the catalogue, in every production source file.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    unnamed.extend(source_messages_without_the_operator(&workspace));
    let mut hook_files: Vec<String> = [
        "assets/base/settings/default.json",
        "assets/base/settings/acceptEdits.json",
        "assets/base/settings/bypass-sandboxed.json",
        "assets/base/codex/hooks.json",
        "assets/base/grok/hooks.json",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    for shim in [
        "commit-msg",
        "pre-commit",
        "pre-merge-commit",
        "pre-push",
        "reference-transaction",
    ] {
        hook_files.push(format!("assets/base/git-hooks/{shim}"));
    }
    let mut fallbacks = 0;
    for file in &hook_files {
        let text = std::fs::read_to_string(workspace.join(file)).unwrap();
        for line in text
            .lines()
            .filter(|line| line.contains("echo") && names_a_verb(line))
        {
            fallbacks += 1;
            if !line.contains("the operator") {
                unnamed.push(format!("{file}: {}", line.trim()));
            }
        }
    }
    assert!(
        fallbacks >= 25,
        "the shipped fallback lines were not found: {fallbacks}"
    );
    assert!(
        unnamed.is_empty(),
        "messages that name an install verb without the operator ({}):\n{}",
        unnamed.len(),
        unnamed.join("\n")
    );
}

/// The lines of every production source file of the three crates that
/// tell the reader to run an install verb, or offer it as the way through,
/// without naming the operator. A file's test module, from its
/// `mod tests {` line, and the `_tests.rs` files are test code.
fn source_messages_without_the_operator(workspace: &Path) -> Vec<String> {
    let names_a_verb =
        |line: &str| line.contains("codeflow update") || line.contains("codeflow init");
    let mut found = Vec::new();
    let tells_to_run = |line: &str| {
        [
            "run `codeflow update",
            "run `codeflow init",
            "run codeflow update",
            "run codeflow init",
            "then codeflow update",
            "then `codeflow update",
            "(codeflow update",
            "(codeflow init",
            "(`codeflow update",
            "(`codeflow init",
        ]
        .iter()
        .any(|phrase| line.contains(phrase))
    };
    // Lines whose reader is the operator who ran the verb, since only the
    // verb itself prints them.
    let exempt: &[(&str, &str)] = &[
        // `init --workspace`'s own refusals.
        (
            "crates/codeflow-core/src/root_checkout.rs",
            "rerun codeflow init --workspace",
        ),
        (
            "crates/codeflow-core/src/root_checkout.rs",
            "run codeflow init --workspace at the umbrella repository's root checkout",
        ),
        // The comment `init --workspace` writes into `.gitignore`.
        (
            "crates/codeflow-core/src/root_checkout.rs",
            "each its own git repository (codeflow init --workspace)",
        ),
        // `update`'s own report of a conflicting file.
        (
            "crates/codeflow-core/src/scaffold/update.rs",
            "then rerun codeflow update to record the shipped baseline",
        ),
    ];
    let mut sources = Vec::new();
    for krate in ["codeflow-core", "codeflow-cli", "codeflow-present"] {
        rust_sources(
            &workspace.join("crates").join(krate).join("src"),
            &mut sources,
        );
    }
    assert!(sources.len() > 100, "sources found: {}", sources.len());
    for path in &sources {
        let source = path
            .strip_prefix(workspace)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(path).unwrap();
        for line in text.lines().take_while(|line| line.trim() != "mod tests {") {
            let comment = line.trim_start().starts_with("//");
            if !comment
                && (tells_to_run(line) || (names_a_verb(line) && line.contains("; then")))
                && !line.contains("operator")
                && !exempt
                    .iter()
                    .any(|(file, phrase)| source.ends_with(file) && line.contains(phrase))
            {
                found.push(format!("{source}: {}", line.trim()));
            }
        }
    }
    found
}

/// The Rust source files under `dir`, test files named `*_tests.rs` left
/// out, in every subfolder.
fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && !path
                .file_stem()
                .is_some_and(|stem| stem.to_string_lossy().ends_with("_tests"))
        {
            out.push(path);
        }
    }
}

/// The shipped Codex profiles name no worktree path, since a pattern with a
/// wildcard folder
/// makes Codex refuse to delete every folder it reaches, worktrees
/// included. The edit guard rows above are the protection of those copies.
#[test]
fn codex_keeps_the_worktree_copies_guarded_by_the_hooks() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/codex/config.toml");
    let text = std::fs::read_to_string(path).unwrap();
    let named: Vec<&str> = text
        .lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with('#'))
        .filter(|line| line.contains(".worktrees/") || line.contains(".claude/worktrees/"))
        .collect();
    assert!(
        named.is_empty(),
        "Codex profiles name worktree paths: {named:?}"
    );
}
