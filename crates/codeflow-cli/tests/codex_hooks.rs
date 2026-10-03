//! Structural tests for the shipped Codex hooks config
//! (`assets/base/codex/hooks.json`). Codex parity (ADR-0008): the SAME
//! `codeflow hook` binaries that bind a Claude session bind an interactive
//! Codex session — only the wiring file differs, so no handler is duplicated.
//! These tests pin that wiring the way `settings_presets.rs` pins the Claude
//! presets: every command is a real hook subcommand, and the parity-relevant
//! events (guards on Bash, orient on `SessionStart` incl. post-compaction) stay
//! wired.

use std::path::PathBuf;

/// Every `codeflow hook` subcommand codeflow ships. A command naming anything
/// else is a typo caught here. (Codex wires a subset — it has no `SessionEnd`
/// event, so `session-summary` is not expected, but it stays a *known* name.)
const KNOWN_HOOKS: [&str; 5] = [
    "git-guard",
    "exec-guard",
    "edit-guard",
    "session-orient",
    "session-summary",
];

fn hooks_json() -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/codex/hooks.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("codex hooks.json is not valid JSON: {e}"))
}

/// Collect every `command` string of a `{"type": "command", ...}` hook object
/// anywhere under the given value.
fn collect_hook_commands(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(serde_json::Value::as_str) == Some("command") {
                let command = map
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .expect("command hook carries a command string");
                out.push(command.to_string());
            }
            for v in map.values() {
                collect_hook_commands(v, out);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                collect_hook_commands(v, out);
            }
        }
        _ => {}
    }
}

#[test]
fn parses_as_json_object_with_hooks() {
    let v = hooks_json();
    assert!(v.is_object(), "top level must be an object");
    assert!(v.get("hooks").is_some(), "hooks key missing");
}

#[test]
fn every_hook_command_is_a_known_codeflow_hook() {
    let v = hooks_json();
    let mut commands = Vec::new();
    collect_hook_commands(&v["hooks"], &mut commands);
    assert!(!commands.is_empty(), "no hook commands found");
    for command in &commands {
        assert!(
            command.starts_with("codeflow hook "),
            "hook command {command:?} must start with \"codeflow hook \""
        );
        let sub = command
            .trim_start_matches("codeflow hook ")
            .split_whitespace()
            .next()
            .unwrap();
        assert!(
            KNOWN_HOOKS.contains(&sub),
            "hook command {command:?} names unknown subcommand {sub:?}"
        );
    }
}

#[test]
fn pretooluse_binds_git_and_exec_guard_on_supported_shells() {
    let v = hooks_json();
    let pre = v["hooks"]["PreToolUse"].clone();
    let mut commands = Vec::new();
    collect_hook_commands(&pre, &mut commands);
    for hook in ["git-guard", "exec-guard"] {
        assert!(
            commands
                .iter()
                .any(|c| c.starts_with(&format!("codeflow hook {hook} --contract 3"))),
            "PreToolUse: {hook} not wired"
        );
    }
    // Keep the shared hook payload compatible with Unix/WSL Bash and native
    // Windows PowerShell command events.
    let matcher = pre[0]["matcher"].as_str().unwrap_or_default();
    assert!(
        matcher.contains("Bash"),
        "PreToolUse matcher must target Bash, got {matcher:?}"
    );
    assert!(
        matcher.contains("PowerShell"),
        "PreToolUse matcher must target PowerShell, got {matcher:?}"
    );
}

#[test]
fn sessionstart_wires_orient_across_all_sources() {
    // Codex parity: orient runs at session start AND after a compaction
    // (source=compact), so the matcher must cover the compact source — that is
    // the post-compaction re-orientation half of the recovery loop.
    let v = hooks_json();
    let start = v["hooks"]["SessionStart"].clone();
    assert!(!start.is_null(), "SessionStart not wired for Codex");
    let mut commands = Vec::new();
    collect_hook_commands(&start, &mut commands);
    assert!(
        commands
            .iter()
            .any(|c| c.starts_with("codeflow hook session-orient --contract 3")),
        "SessionStart: session-orient not wired"
    );
    let matcher = start[0]["matcher"].as_str().unwrap_or_default();
    // Both headline behaviors must stay covered: `startup` = a session opens with
    // the digest; `compact` = it re-orients after a compaction. (resume/clear are
    // wired too, but these two are the behaviors the capability claims.)
    for source in ["startup", "compact"] {
        assert!(
            matcher.contains(source),
            "SessionStart matcher must include the {source:?} source, got {matcher:?}"
        );
    }
}

#[test]
fn guards_and_orient_stay_on_their_own_events() {
    // The events are not interchangeable: a mis-wire that adds a guard to
    // SessionStart, or orient to PreToolUse, must fail here.
    let v = hooks_json();
    let mut pre = Vec::new();
    collect_hook_commands(&v["hooks"]["PreToolUse"], &mut pre);
    let mut start = Vec::new();
    collect_hook_commands(&v["hooks"]["SessionStart"], &mut start);
    assert!(
        !pre.iter().any(|c| c.contains("session-orient")),
        "orient must not ride PreToolUse — it is a per-session digest, not a per-tool gate"
    );
    for guard in ["git-guard", "exec-guard"] {
        assert!(
            !start.iter().any(|c| c.contains(guard)),
            "{guard} must not ride SessionStart — guards gate tool calls, not session open"
        );
    }
}

#[test]
fn dogfood_codex_hooks_matches_shipped_scaffold() {
    // This repo is its own first consumer: the dogfood `.codex/hooks.json` must
    // stay byte-identical to the scaffold it ships, or the two silently diverge.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let shipped = std::fs::read_to_string(root.join("assets/base/codex/hooks.json"))
        .expect("read shipped codex hooks.json");
    let dogfood = std::fs::read_to_string(root.join(".codex/hooks.json"))
        .expect("read dogfood .codex/hooks.json");
    assert_eq!(
        shipped, dogfood,
        "dogfood .codex/hooks.json drifted from assets/base/codex/hooks.json"
    );
}

/// TSK-128: Grok Build's session, compaction and prompt events exist, but it
/// ignores their stdout and discards an allowing prompt hook's output
/// (Grok Build 1.0.41 hook reference): events present, context injection
/// unavailable. So the Grok file wires only the guards, the same payload as
/// Codex, and no advisory `session-orient` whose output no model receives.
#[test]
fn dogfood_grok_hooks_share_pretooluse_and_wire_no_advisory_hook() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let shipped = std::fs::read_to_string(root.join("assets/base/grok/hooks.json"))
        .expect("read shipped grok hooks.json");
    let dogfood = std::fs::read_to_string(root.join(".grok/hooks/codeflow.json"))
        .expect("read dogfood .grok/hooks/codeflow.json");
    assert_eq!(
        shipped, dogfood,
        "dogfood Grok hooks drifted from assets/base/grok/hooks.json"
    );
    let grok: serde_json::Value = serde_json::from_str(&shipped).unwrap();
    let codex: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("assets/base/codex/hooks.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        grok["hooks"]["PreToolUse"][0], codex["hooks"]["PreToolUse"][0],
        "Grok PreToolUse must stay the same git-guard/exec-guard payload as Codex"
    );
    let events: Vec<&String> = grok["hooks"].as_object().unwrap().keys().collect();
    assert_eq!(events, vec!["PreToolUse"], "Grok wires only the guards");
    let mut commands = Vec::new();
    collect_hook_commands(&grok["hooks"], &mut commands);
    assert!(
        !commands
            .iter()
            .any(|c| c.contains("session-orient") || c.contains("prompt-reminder")),
        "an advisory hook on Grok would claim context that never reaches the model"
    );
}

/// TSK-215 AC-1 (issue 29): Grok expands `$name` and `${...}` in a hook
/// command itself and skips the hook when a name is unset, so a shell
/// variable in a `CodeFlow` command turns the guard off in a Grok session.
/// Grok loads its own hook file and, in compat mode, the Claude settings;
/// Codex shares the Grok guard payload. No hook command in any of them, as
/// shipped, installed in this repository or kept as its baseline, carries a
/// `$`.
#[test]
fn no_hook_command_carries_a_dollar_grok_reads_as_a_template() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files: Vec<PathBuf> = [
        "assets/base/grok/hooks.json",
        "assets/base/codex/hooks.json",
        ".grok/hooks/codeflow.json",
        ".codex/hooks.json",
        ".claude/settings.json",
        ".codeflow/.baseline/.grok/hooks/codeflow.json",
        ".codeflow/.baseline/.codex/hooks.json",
        ".codeflow/.baseline/.claude/settings.json",
    ]
    .iter()
    .map(|path| root.join(path))
    .collect();
    let mut presets: Vec<PathBuf> = std::fs::read_dir(root.join("assets/base/settings"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    presets.sort();
    assert!(presets.len() >= 3, "{presets:?}");
    files.extend(presets);
    for file in files {
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut commands = Vec::new();
        collect_hook_commands(&value["hooks"], &mut commands);
        assert!(!commands.is_empty(), "{}", file.display());
        for command in commands {
            assert!(
                !command.contains('$'),
                "{}: Grok would skip this hook as an unset template: {command}",
                file.display()
            );
        }
    }
}

/// TSK-128 AC-5: Codex wires `UserPromptSubmit` to the stable advisory
/// entry, where plain stdout becomes developer context, with no matcher
/// (Codex ignores one on this event) and never the manual command.
#[test]
fn user_prompt_submit_wires_the_stable_advisory_entry() {
    let v = hooks_json();
    let prompt = v["hooks"]["UserPromptSubmit"].clone();
    let mut commands = Vec::new();
    collect_hook_commands(&prompt, &mut commands);
    assert_eq!(commands.len(), 1);
    assert!(commands[0].starts_with("codeflow hook session-orient --contract 3"));
    assert!(prompt[0].get("matcher").is_none());
    let mut all = Vec::new();
    collect_hook_commands(&v["hooks"], &mut all);
    assert!(!all.iter().any(|c| c.contains("prompt-reminder")));
}

#[path = "../../codeflow-core/src/security/guard_forms.rs"]
#[allow(dead_code)]
mod guard_forms;

/// Run the shipped Codex exec-guard wiring through the shell, as Codex
/// does, with a Bash tool call for `command` on stdin.
#[cfg(unix)]
fn run_codex_exec_guard(root: &std::path::Path, command: &str) -> std::process::Output {
    use std::io::Write as _;
    let mut commands = Vec::new();
    collect_hook_commands(&hooks_json()["hooks"]["PreToolUse"], &mut commands);
    let hook = commands
        .into_iter()
        .find(|c| c.starts_with("codeflow hook exec-guard --contract 3"))
        .expect("exec-guard wired");
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(
        exe.parent()
            .map(std::path::Path::to_path_buf)
            .into_iter()
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
    )
    .unwrap();
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": root,
    });
    let mut child = std::process::Command::new("sh")
        .args(["-c", &hook])
        .current_dir(root)
        .env("PATH", path)
        .env("CODEFLOW_HOME", root.join(".home"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// PR 35 review round 2, finding 1: doctor's Grok canary never runs a
/// `codeflow` that PATH finds in the repository. A planted `bin/codeflow`
/// ahead of the real one would write a marker outside the scratch
/// directory and fake a refusal; doctor runs itself instead, leaves no
/// marker, and names where PATH resolves `codeflow` as unverified.
#[cfg(unix)]
#[test]
fn the_grok_canary_never_runs_a_codeflow_planted_on_path() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".grok/hooks")).unwrap();
    std::fs::create_dir_all(project.join("bin")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::copy(
        root.join("assets/base/grok/hooks.json"),
        project.join(".grok/hooks/codeflow.json"),
    )
    .unwrap();
    let marker = dir.path().join("marker");
    let planted = project.join("bin/codeflow");
    std::fs::write(
        &planted,
        format!(
            "#!/bin/sh\ntouch '{}'\necho 'codeflow exec-guard: BLOCKED' >&2\necho '{{\"decision\":\"deny\",\"reason\":\"planted\"}}'\nexit 2\n",
            marker.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o755)).unwrap();
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(
        [project.join("bin"), exe.parent().unwrap().to_path_buf()]
            .into_iter()
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
    )
    .unwrap();
    let out = std::process::Command::new(&exe)
        .args(["doctor", "--check", "grok"])
        .current_dir(&project)
        .env("PATH", path)
        .env("GROK_HOME", dir.path().join("grok-home"))
        .env("CODEFLOW_HOME", dir.path().join("codeflow-home"))
        .env_remove("GROK_FOLDER_TRUST")
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!marker.exists(), "doctor ran the planted codeflow:\n{said}");
    assert!(
        said.contains("canary: the shipped shell guard grok runs refused"),
        "{said}"
    );
    let canonical = std::fs::canonicalize(&planted).unwrap();
    assert!(
        said.contains(&format!(
            "PATH resolves codeflow to {}, inside this repository",
            canonical.display()
        )),
        "{said}"
    );
}

/// PR 35 review round 3, finding 1: doctor never re-executes a binary to
/// prove the Grok guard refuses, so swapping the path doctor was launched
/// from cannot answer for it. Doctor starts from a symlink, or from a copy
/// in a folder the repository controls; a FIFO trust store pauses it after
/// start-up while the path is replaced with a script that writes a marker
/// and fakes a refusal. No marker appears, and the refusal comes from the
/// guard judged in process.
#[cfg(unix)]
#[test]
fn the_grok_canary_runs_nothing_a_swapped_launch_path_could_answer() {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for form in ["symlink", "copy"] {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".grok/hooks")).unwrap();
        std::fs::create_dir_all(project.join("bin")).unwrap();
        std::fs::copy(
            root.join("assets/base/grok/hooks.json"),
            project.join(".grok/hooks/codeflow.json"),
        )
        .unwrap();
        let launch = project.join("bin/codeflow");
        if form == "symlink" {
            std::os::unix::fs::symlink(&exe, &launch).unwrap();
        } else {
            std::fs::copy(&exe, &launch).unwrap();
            std::fs::set_permissions(&launch, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let marker = dir.path().join("marker");
        let fake = dir.path().join("fake-codeflow");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\ntouch '{}'\necho 'codeflow exec-guard: BLOCKED' >&2\necho '{{\"decision\":\"deny\",\"reason\":\"fake\"}}'\nexit 2\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let grok_home = dir.path().join("grok-home");
        std::fs::create_dir_all(&grok_home).unwrap();
        let fifo = grok_home.join("trusted_folders.toml");
        assert!(std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());

        let child = std::process::Command::new(&launch)
            .args(["doctor", "--check", "grok"])
            .current_dir(&project)
            .env("GROK_HOME", &grok_home)
            .env("CODEFLOW_HOME", dir.path().join("codeflow-home"))
            .env_remove("GROK_FOLDER_TRUST")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        // Opening the FIFO for writing waits until doctor reads its trust
        // store, after start-up; the launch path is swapped only then.
        let (swap, launch_path) = (fake.clone(), launch.clone());
        let writer = std::thread::spawn(move || {
            let mut pipe = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
            std::fs::remove_file(&launch_path).unwrap();
            std::os::unix::fs::symlink(&swap, &launch_path).unwrap();
            pipe.write_all(b"").unwrap();
        });
        let out = child.wait_with_output().unwrap();
        writer.join().unwrap();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !marker.exists(),
            "{form}: doctor ran the swapped launch path:\n{said}"
        );
        assert!(
            said.contains("canary: the shipped shell guard grok runs refused")
                && said.contains("judged in process"),
            "{form}: {said}"
        );
    }
}

/// Every file under `dir`, with its bytes, so a test can show that a run
/// changed nothing there.
#[cfg(unix)]
fn tree(dir: &std::path::Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            let kind = std::fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_dir() {
                files.insert(path.clone(), Vec::new());
                pending.push(path);
            } else if kind.is_symlink() {
                let target = std::fs::read_link(&path).unwrap();
                files.insert(path, target.into_os_string().into_encoded_bytes());
            } else {
                let bytes = std::fs::read(&path).unwrap();
                files.insert(path, bytes);
            }
        }
    }
    files
}

/// PR 35 review round 4, finding 1: the Grok canary reads no repository.
/// Doctor's temporary folder sits inside a repository, directly or through
/// a symlink: first a plain one, then one with two remotes and no
/// `origin`. Each time the canary refuses with Grok's deny answer, and
/// nothing in that repository changes, a refusal ledger included.
#[cfg(unix)]
#[test]
fn the_grok_canary_reads_and_writes_no_repository_around_tmpdir() {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut failures = Vec::new();
    for form in ["direct", "symlink"] {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".grok/hooks")).unwrap();
        std::fs::copy(
            root.join("assets/base/grok/hooks.json"),
            project.join(".grok/hooks/codeflow.json"),
        )
        .unwrap();
        let parent = dir.path().join("parent");
        std::fs::create_dir_all(parent.join("tmp")).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&parent)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(status.status.success(), "git {args:?}: {status:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        let tmpdir = if form == "symlink" {
            let link = dir.path().join("tmp-link");
            std::os::unix::fs::symlink(parent.join("tmp"), &link).unwrap();
            link
        } else {
            parent.join("tmp")
        };
        for remotes in ["no remotes", "two remotes, no origin"] {
            if remotes != "no remotes" {
                git(&["remote", "add", "upstream", "https://example.invalid/a.git"]);
                git(&["remote", "add", "fork", "https://example.invalid/b.git"]);
            }
            let before = tree(&parent);
            let out = std::process::Command::new(&exe)
                .args(["doctor", "--check", "grok"])
                .current_dir(&project)
                .env("TMPDIR", &tmpdir)
                .env("GROK_HOME", dir.path().join("grok-home"))
                .env("CODEFLOW_HOME", dir.path().join("codeflow-home"))
                .env_remove("GROK_FOLDER_TRUST")
                .output()
                .unwrap();
            let said = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let refused = said.contains("canary: the shipped shell guard grok runs refused");
            let after = tree(&parent);
            let changed: Vec<_> = after
                .iter()
                .filter(|(path, bytes)| before.get(*path) != Some(*bytes))
                .map(|(path, _)| path.display().to_string())
                .chain(
                    before
                        .keys()
                        .filter(|path| !after.contains_key(*path))
                        .map(|path| path.display().to_string()),
                )
                .collect();
            if !refused || !changed.is_empty() {
                let first = said.lines().next().unwrap_or_default().to_string();
                failures.push(format!(
                    "{form} TMPDIR, {remotes}: refused {refused}; changed {changed:?}; {first}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// TSK-215: the shipped Grok exec-guard command, run through the shell as
/// Grok runs it, refuses a dangerous command in the payload Grok Build
/// 1.0.46 sends, where every field comes under both spellings (captured
/// from a live session), and lets an ordinary command through. Grok shows
/// only a hook's first stderr line as its deny reason, so the refusal also
/// comes as Grok's deny decision on stdout with the rule and its sanctioned
/// path; a Claude-shaped payload gets nothing on stdout.
#[cfg(unix)]
#[test]
fn grok_wiring_refuses_in_the_payload_grok_sends() {
    use std::io::Write as _;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let grok: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("assets/base/grok/hooks.json")).unwrap(),
    )
    .unwrap();
    let mut commands = Vec::new();
    collect_hook_commands(&grok["hooks"]["PreToolUse"], &mut commands);
    let hook = commands
        .into_iter()
        .find(|c| c.starts_with("codeflow hook exec-guard --contract 3"))
        .expect("exec-guard wired for grok");
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(
        exe.parent()
            .map(std::path::Path::to_path_buf)
            .into_iter()
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    for (command, refused, grok) in [
        ("rm -rf /", true, true),
        ("echo hello", false, true),
        ("rm -rf /", true, false),
    ] {
        let input = serde_json::json!({"command": command, "description": "probe"});
        let payload = if grok {
            serde_json::json!({
                "hookEventName": "pre_tool_use",
                "cwd": dir.path(),
                "toolName": "run_terminal_command",
                "toolInput": input,
                "hook_event_name": "PreToolUse",
                "tool_name": "run_terminal_command",
                "tool_input": input,
            })
        } else {
            serde_json::json!({
                "hook_event_name": "PreToolUse",
                "cwd": dir.path(),
                "tool_name": "Bash",
                "tool_input": input,
            })
        };
        let mut child = std::process::Command::new("sh")
            .args(["-c", &hook])
            .current_dir(dir.path())
            .env("PATH", &path)
            .env("CODEFLOW_HOME", dir.path().join(".home"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("unreadable hook payload"), "{stderr}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        if refused {
            assert_eq!(out.status.code(), Some(2), "{command}: {stderr}");
            assert!(stderr.contains("exec-guard: BLOCKED"), "{stderr}");
        } else {
            assert_eq!(out.status.code(), Some(0), "{command}: {stderr}");
        }
        if refused && grok {
            let decision: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
            assert_eq!(decision["decision"], "deny", "{stdout}");
            let reason = decision["reason"].as_str().unwrap();
            assert!(
                reason.starts_with(
                    "codeflow exec-guard: BLOCKED — policy rule security.dangerous_commands"
                ),
                "{reason}"
            );
            assert!(reason.contains("\n  sanctioned: "), "{reason}");
            assert!(reason.contains("\npolicy source: "), "{reason}");
        } else {
            assert!(stdout.is_empty(), "{command} grok={grok}: {stdout}");
        }
    }
}

/// TSK-141 AC-5: the Codex wiring refuses each composed deletion, passes a
/// project deletion, and lets a help invocation through while its data twin
/// is reported.
#[cfg(unix)]
#[test]
fn the_codex_exec_guard_wiring_judges_composed_deletions_and_help() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (form, _) in guard_forms::COMPOSED_PAIRS {
        let out = run_codex_exec_guard(root, form);
        assert_eq!(out.status.code(), Some(2), "{form}");
        // A policy refusal from the current binary carries no reinstall advice.
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("codeflow-cli-installer.sh"),
            "{form}: {stderr}"
        );
        assert!(!stderr.contains("codeflow update"), "{form}: {stderr}");
    }
    for command in guard_forms::PROJECT_DELETIONS {
        let out = run_codex_exec_guard(root, command);
        assert_eq!(out.status.code(), Some(0), "{command}");
        assert!(out.stderr.is_empty(), "{command}");
    }
    for (help, twin) in guard_forms::HELP_PAIRS {
        let out = run_codex_exec_guard(root, help);
        assert!(out.stderr.is_empty(), "{help}");
        let out = run_codex_exec_guard(root, twin);
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("headless peer run"),
            "{twin}"
        );
    }
}

/// TSK-180: the built hook runs a cleanup after a trap reset or a removed
/// hook, and refuses one whose reset or removal may not take effect.
#[cfg(unix)]
#[test]
fn the_codex_exec_guard_wiring_holds_the_round_four_probes() {
    use guard_forms::Expect;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::os::unix::fs::symlink("/", root.join("root-link")).unwrap();
    for sub in ["build", "empty"] {
        std::fs::create_dir(root.join(sub)).unwrap();
    }
    let mut wrong = Vec::new();
    for (case, command, expect) in guard_forms::REVIEW_ROUND_FOUR_PROBES {
        let out = run_codex_exec_guard(root, command);
        let want = if *expect == Expect::Allowed { 0 } else { 2 };
        if out.status.code() != Some(want) {
            wrong.push(format!(
                "{case} ({expect:?}): {command} -> {:?} {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

#[test]
fn native_edit_tools_are_wired_to_the_edit_guard() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (harness, tools) in [
        ("codex", vec!["apply_patch", "Edit", "Write"]),
        ("grok", vec!["write", "search_replace", "Edit", "Write"]),
    ] {
        let value: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(format!("assets/base/{harness}/hooks.json")))
                .unwrap(),
        )
        .unwrap();
        let entry = &value["hooks"]["PreToolUse"][1];
        let matcher = regex::Regex::new(entry["matcher"].as_str().unwrap()).unwrap();
        for tool in tools {
            assert!(matcher.is_match(tool), "{harness}: {tool}");
        }
        assert!(!matcher.is_match("Bash"));
        assert!(entry["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .starts_with("codeflow hook edit-guard --contract 3"));
    }
}
