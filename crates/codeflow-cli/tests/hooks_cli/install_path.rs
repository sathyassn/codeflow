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

/// A refusal under hook integrity whose message names the operator.
fn operator_refusal(out: &Output) -> bool {
    let stderr = String::from_utf8_lossy(&out.stderr);
    out.status.code() == Some(2)
        && stderr.contains("git.hook_integrity")
        && stderr.contains("operator")
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

/// AC-1: a CodeFlow binary other than the installed one, run as either
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
        format!("git -c alias.u='!{build} update' u"),
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
    tally.shell_refused(&["codeflow init --yes".to_string()], &plain, operator_refusal);
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
            format!(
                r#"python3 -c "open('{folder}/.codeflow/policy.json', 'w').write('x')""#
            ),
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
                (
                    "write",
                    serde_json::json!({"path": target, "content": "x"}),
                ),
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

/// AC-3: every shipped Claude preset denies the native edit tools each
/// worktree's copy of every enforcement file the root checkout's denies
/// name, wherever the project sits under the session's folder. Git's own
/// files are left out: a linked worktree keeps its configuration and hooks
/// in the common directory the root's denies already cover.
#[test]
fn every_shipped_claude_preset_denies_the_worktree_copies() {
    let expected: Vec<String> = codeflow_core::security::actions::table()
        .claude_edit_denies
        .iter()
        .filter_map(|rule| rule.strip_prefix("Edit(/")?.strip_suffix(')'))
        .filter(|relative| !relative.starts_with(".git/") && !relative.starts_with('/'))
        .flat_map(|relative| {
            WORKTREE_FOLDERS
                .iter()
                .map(move |folder| format!("Edit(//**/{folder}/*/{relative})"))
        })
        .collect();
    assert!(!expected.is_empty(), "the action table names no root denies");
    let presets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/settings");
    let mut missing = Vec::new();
    for name in ["default.json", "acceptEdits.json", "bypass-sandboxed.json"] {
        let text = std::fs::read_to_string(presets.join(name)).unwrap();
        let preset: serde_json::Value = serde_json::from_str(&text).unwrap();
        let deny: Vec<&str> = preset["permissions"]["deny"]
            .as_array()
            .unwrap_or_else(|| panic!("{name} has no deny list"))
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        for entry in &expected {
            if !deny.contains(&entry.as_str()) {
                missing.push(format!("{name}: {entry}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "missing worktree denies ({}):\n{}",
        missing.len(),
        missing.join("\n")
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
    assert!(named.is_empty(), "Codex profiles name worktree paths: {named:?}");
}
