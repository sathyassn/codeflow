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
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    for row in &rows {
        tally.refused(&[EXEC], row, &shell(row, &root), &root, operator_refusal);
    }
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

/// AC-6: words that only name the verbs as data stay allowed: the binary's
/// own verbs that record free text, test operands, other cargo
/// subcommands, an assigned value no command on the line reads, a folder
/// of that name given to a program that never runs its operands, and a
/// search program's pattern.
#[test]
fn words_that_name_the_verbs_as_data_stay_allowed() {
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
        "make -C codeflow init",
        "cp -r codeflow update",
        "ag 'codeflow init' docs",
        "ack 'codeflow update' docs",
        r#"jq '.steps[] | select(. == "codeflow update")' plan.json"#,
        "rg --pre ./tools/pdftext pattern docs",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let mut tally = Tally::default();
    tally.install_allowed(&rows, &root);
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
        "./target/debug/codeflow update --help",
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
        "printf 'codeflow update\\n' > notes.txt",
        "grep -rn 'codeflow update' docs",
        "git commit -m 'docs: say the operator runs codeflow update'",
        "git commit -F - <<'EOF'\ndocs: say the operator runs codeflow update\nEOF",
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
    // Messages outside the catalogue that tell the reader to run a verb.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tells_to_run = |line: &str| {
        [
            "run `codeflow update",
            "run `codeflow init",
            "run codeflow update",
            "run codeflow init",
            "then codeflow update",
            "then `codeflow update",
        ]
        .iter()
        .any(|phrase| line.contains(phrase))
    };
    for source in [
        "crates/codeflow-core/src/hooks/orient.rs",
        "crates/codeflow-core/src/hooks/policy_schema.rs",
        "crates/codeflow-core/src/scaffold/version.rs",
        "crates/codeflow-core/src/scaffold/init.rs",
        "crates/codeflow-core/src/scaffold/mod.rs",
        "crates/codeflow-core/src/scaffold/release_pin.rs",
        "crates/codeflow-core/src/doctor/mod.rs",
        "crates/codeflow-core/src/doctor/ci_pin.rs",
        "crates/codeflow-core/src/model_catalog/inputs.rs",
        "crates/codeflow-present/src/error.rs",
        "crates/codeflow-cli/src/cmd/hook.rs",
    ] {
        let text = std::fs::read_to_string(workspace.join(source)).unwrap();
        for line in text.lines() {
            let comment = line.trim_start().starts_with("//");
            if !comment
                && (tells_to_run(line) || (names_a_verb(line) && line.contains("; then")))
                && !line.contains("operator")
            {
                unnamed.push(format!("{source}: {}", line.trim()));
            }
        }
    }
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
