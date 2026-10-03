//! TSK-147 review rounds 2 and 3: every Codex and Grok trust probe the
//! reviewer ran, kept as a regression. Doctor reads harness trust
//! statically, and a static reading can prove only that a hook does NOT run
//! (Warn). No probe may yield Pass: a configuration that matches is a Note,
//! "configured; runtime not verified", naming the check that does verify it.
//!
//! The probes are ported from the review evidence under the session
//! scratchpad: `doctor-trust-r2-probe.rs`,
//! `doctor-codex-normalization-r2-probe.rs` and `neighboring-probes.py`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use codeflow_core::doctor::{run_check, CapturedRun, CheckResult, Options, Status};

const CODEX_HASH: &str = "sha256:42d1067865f6352ae0975d92a473334f58d7748b8afb7a27e36b7dc834d204d3";
const CODEX_HOOK: &str = r#"{"hooks":{"PreToolUse":[{"matcher":"^(Bash|PowerShell)$","hooks":[{"type":"command","command":"codeflow hook git-guard","timeout":10}]}]}}"#;
const GROUP: &str = r#"[{"hooks":[{"type":"command","command":"x","timeout":10}]}]"#;

fn opts(project: &Path, home: &Path) -> Options {
    Options {
        project_dir: project.to_string_lossy().into_owned(),
        harness_home: Some(home.to_path_buf()),
        look_path: Some(|_| Ok("synthetic-harness".into())),
        env_var: Some(|_| None),
        // The grok guard canary (TSK-215) answers as a refusing guard; this
        // file probes trust, not the guard.
        exec_command_capture: Some(|_, _, _| {
            Ok(CapturedRun {
                code: Some(2),
                stdout: r#"{"decision":"deny","reason":"codeflow exec-guard: BLOCKED"}"#.into(),
                stderr: "codeflow exec-guard: BLOCKED".into(),
            })
        }),
        ..Options::default()
    }
}

fn check(check: &str, options: &Options) -> CheckResult {
    run_check(check, options).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// The trust hash Codex records for a one-handler group, as the reviewer
/// computed it: sha256 of the sorted-key JSON identity.
fn ident(event: &str, matcher: Option<&str>, timeout: u64) -> String {
    let matcher = matcher.map_or_else(String::new, |m| {
        format!(r#","matcher":{}"#, serde_json::to_string(m).unwrap())
    });
    let identity = format!(
        r#"{{"event_name":"{event}","hooks":[{{"async":false,"command":"x","timeout":{timeout},"type":"command"}}]{matcher}}}"#
    );
    format!(
        "sha256:{}",
        codeflow_core::scaffold::sha256_hex(identity.as_bytes())
    )
}

fn state(path: &Path, event: &str, hash: &str, enabled: bool) -> String {
    format!(
        "[hooks.state.{}]\ntrusted_hash = \"{hash}\"\nenabled = {enabled}\n",
        serde_json::to_string(&format!("{}:{event}:0:0", path.display())).unwrap()
    )
}

fn project_trust(project: &Path, level: &str) -> String {
    format!(
        "[projects.{}]\ntrust_level = \"{level}\"\n",
        serde_json::to_string(&project.display().to_string()).unwrap()
    )
}

/// A fresh project and harness home under `base/label`.
fn codex_case(base: &Path, label: &str) -> (PathBuf, PathBuf) {
    let project = base.join(label).join("project");
    let home = base.join(label).join("home");
    fs::create_dir_all(project.join(".codex")).unwrap();
    fs::create_dir_all(home.join(".codex")).unwrap();
    (project, home)
}

fn grok_case(project: &Path, home: &Path) {
    fs::create_dir_all(project.join(".grok/hooks")).unwrap();
    fs::create_dir_all(home.join(".grok")).unwrap();
    fs::write(
        project.join(".grok/hooks/codeflow.json"),
        include_str!("../../../assets/base/grok/hooks.json"),
    )
    .unwrap();
    for name in ["config.toml", "managed_config.toml", "trusted_folders.toml"] {
        let _ = fs::remove_file(home.join(".grok").join(name));
    }
}

fn store(home: &Path, records: &[(&Path, bool)]) {
    let mut text = String::new();
    for (path, trusted) in records {
        writeln!(
            text,
            "[folders.{}]\ntrusted = {trusted}",
            serde_json::to_string(&path.display().to_string()).unwrap()
        )
        .unwrap();
    }
    fs::write(home.join(".grok/trusted_folders.toml"), text).unwrap();
}

/// Every probe, labelled, with its result.
#[allow(clippy::too_many_lines)]
fn probes(base: &Path) -> Vec<(String, CheckResult)> {
    let mut out = Vec::new();
    let mut record = |label: &str, result: CheckResult| out.push((label.to_string(), result));

    // Round 2: `doctor-trust-r2-probe.rs`, the Codex records.
    for (label, hook, body) in [
        (
            "codex-positive",
            CODEX_HOOK,
            format!("trusted_hash = \"{CODEX_HASH}\""),
        ),
        (
            "codex-disabled",
            CODEX_HOOK,
            format!("trusted_hash = \"{CODEX_HASH}\"\nenabled = false"),
        ),
        (
            "codex-malformed-record",
            CODEX_HOOK,
            format!("trusted_hash = \"{CODEX_HASH}\"\nenabled = \"yes\""),
        ),
        (
            "codex-changed-command-old-hash",
            r#"{"hooks":{"PreToolUse":[{"matcher":"^(Bash|PowerShell)$","hooks":[{"type":"command","command":"changed","timeout":10}]}]}}"#,
            format!("trusted_hash = \"{CODEX_HASH}\""),
        ),
        (
            "codex-malformed-event",
            r#"{"hooks":{"PreToolUse":{"matcher":"x","hooks":[]}}}"#,
            String::new(),
        ),
    ] {
        let (project, home) = codex_case(base, label);
        // One component at a time: on Windows `join(".codex/hooks.json")`
        // keeps the `/`, while the key Codex records names the file with
        // `\` throughout, as doctor does.
        let path = project.join(".codex").join("hooks.json");
        fs::write(&path, hook).unwrap();
        let config = if body.is_empty() {
            String::new()
        } else {
            format!(
                "[hooks.state.{}]\n{body}\n",
                serde_json::to_string(&format!("{}:pre_tool_use:0:0", path.display())).unwrap()
            )
        };
        fs::write(home.join(".codex/config.toml"), config).unwrap();
        record(label, check("codex", &opts(&project, &home)));
    }

    // Round 2: `doctor-codex-normalization-r2-probe.rs`.
    for (label, event_key, event_name, hooks, hash) in [
        (
            "codex-ignored-matcher",
            "UserPromptSubmit",
            "user_prompt_submit",
            r#"[{"matcher":"bogus","hooks":[{"type":"command","command":"codeflow hook session-orient","timeout":10}]}]"#,
            "8cae15bf3a1950e8e9b95a6945dd894e1c3eedd8ae9e72843f01e09e5c97e099",
        ),
        (
            "codex-invalid-matcher",
            "PreToolUse",
            "pre_tool_use",
            r#"[{"matcher":"[","hooks":[{"type":"command","command":"x","timeout":10}]}]"#,
            "20a7b5a3325c6c3477af23788f7be368fcc8ed32b060cbb5dc84cc1af89ac28c",
        ),
        (
            "codex-zero-timeout",
            "PreToolUse",
            "pre_tool_use",
            r#"[{"matcher":"^Bash$","hooks":[{"type":"command","command":"x","timeout":0}]}]"#,
            "de305438036e547f0be717966bc7968b34e3c7da64088816f6bddbab0f9ef872",
        ),
        (
            "codex-empty-command",
            "PreToolUse",
            "pre_tool_use",
            r#"[{"matcher":"^Bash$","hooks":[{"type":"command","command":"   ","timeout":10}]}]"#,
            "218d05af2d3beb588aeb86a90ed64aef17344d31dc8bbd286883ed090ecefc77",
        ),
    ] {
        let (project, home) = codex_case(base, label);
        let path = project.join(".codex").join("hooks.json");
        fs::write(&path, format!(r#"{{"hooks":{{"{event_key}":{hooks}}}}}"#)).unwrap();
        fs::write(
            home.join(".codex/config.toml"),
            format!(
                "[hooks.state.{}]\ntrusted_hash = \"sha256:{hash}\"\n",
                serde_json::to_string(&format!("{}:{event_name}:0:0", path.display())).unwrap()
            ),
        )
        .unwrap();
        record(label, check("codex", &opts(&project, &home)));
    }

    // Round 3: `neighboring-probes.py`, the normalized positive controls.
    for (label, event, event_name, matcher, timeout, normalized) in [
        (
            "normalized-ignored-matcher",
            "UserPromptSubmit",
            "user_prompt_submit",
            Some("bogus"),
            Some(10),
            10,
        ),
        (
            "normalized-zero-timeout",
            "PreToolUse",
            "pre_tool_use",
            Some("^Bash$"),
            Some(0),
            1,
        ),
        (
            "normalized-default-timeout",
            "PreToolUse",
            "pre_tool_use",
            None,
            None,
            600,
        ),
        (
            "normalized-session-end-cap",
            "SessionEnd",
            "session_end",
            None,
            Some(99),
            3,
        ),
        (
            "normalized-interrupt-default",
            "Interrupt",
            "interrupt",
            Some("["),
            None,
            1,
        ),
    ] {
        let (project, home) = codex_case(base, label);
        let path = project.join(".codex").join("hooks.json");
        let timeout = timeout.map_or_else(String::new, |t: u64| format!(r#","timeout":{t}"#));
        let matcher_field = matcher.map_or_else(String::new, |m| format!(r#""matcher":"{m}","#));
        fs::write(
            &path,
            format!(
                r#"{{"hooks":{{"{event}":[{{{matcher_field}"hooks":[{{"type":"command","command":"x"{timeout}}}]}}]}}}}"#
            ),
        )
        .unwrap();
        let hashed_matcher = if matches!(event, "UserPromptSubmit" | "Stop" | "Interrupt") {
            None
        } else {
            matcher
        };
        fs::write(
            home.join(".codex/config.toml"),
            project_trust(&project, "trusted")
                + &state(
                    &path,
                    event_name,
                    &ident(event_name, hashed_matcher, normalized),
                    true,
                ),
        )
        .unwrap();
        record(label, check("codex", &opts(&project, &home)));
    }

    // Round 3: a duplicate event key, which Codex's typed reader rejects.
    let (project, home) = codex_case(base, "duplicate-event");
    let path = project.join(".codex").join("hooks.json");
    fs::write(
        &path,
        format!(r#"{{"hooks":{{"PreToolUse":[],"PreToolUse":{GROUP}}}}}"#),
    )
    .unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        project_trust(&project, "trusted")
            + &state(
                &path,
                "pre_tool_use",
                &ident("pre_tool_use", None, 10),
                true,
            ),
    )
    .unwrap();
    record(
        "codex-duplicate-known-event",
        check("codex", &opts(&project, &home)),
    );

    // Round 3: a project marked untrusted keeps its approved hook hash.
    let (project, home) = codex_case(base, "explicitly-untrusted-project");
    let path = project.join(".codex").join("hooks.json");
    fs::write(&path, format!(r#"{{"hooks":{{"PreToolUse":{GROUP}}}}}"#)).unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        project_trust(&project, "untrusted")
            + &state(
                &path,
                "pre_tool_use",
                &ident("pre_tool_use", None, 10),
                true,
            ),
    )
    .unwrap();
    record(
        "codex-explicitly-untrusted-project",
        check("codex", &opts(&project, &home)),
    );

    // Round 3: a symlinked hooks file, its source key disabled and its
    // target key trusted.
    #[cfg(unix)]
    {
        let (project, home) = codex_case(base, "symlinked-hook-file");
        let target = project.join("hook-declarations.json");
        fs::write(&target, format!(r#"{{"hooks":{{"PreToolUse":{GROUP}}}}}"#)).unwrap();
        let path = project.join(".codex").join("hooks.json");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let hash = ident("pre_tool_use", None, 10);
        fs::write(
            home.join(".codex/config.toml"),
            project_trust(&project, "trusted")
                + &state(&path, "pre_tool_use", &hash, false)
                + &state(&target, "pre_tool_use", &hash, true),
        )
        .unwrap();
        record(
            "codex-symlink-source-disabled-target-trusted",
            check("codex", &opts(&project, &home)),
        );
    }

    // Round 3: a linked checkout, and the Grok topology on the same repo.
    let linked_base = base.join("linked");
    let root = linked_base.join("main");
    let linked = linked_base.join("linked");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "Review Probe"]);
    git(&root, &["config", "user.email", "review@example.invalid"]);
    fs::write(root.join("tracked"), "fixture\n").unwrap();
    git(&root, &["add", "tracked"]);
    git(&root, &["commit", "-qm", "fixture"]);
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "probe-linked",
            linked.to_str().unwrap(),
        ],
    );
    let home = linked_base.join("home");
    fs::create_dir_all(home.join(".codex")).unwrap();
    let hash = ident("pre_tool_use", None, 10);
    let mut config = project_trust(&root, "trusted") + &project_trust(&linked, "trusted");
    for (checkout, enabled) in [(&root, false), (&linked, true)] {
        fs::create_dir_all(checkout.join(".codex")).unwrap();
        let path = checkout.join(".codex").join("hooks.json");
        fs::write(&path, format!(r#"{{"hooks":{{"PreToolUse":{GROUP}}}}}"#)).unwrap();
        config += &state(&path, "pre_tool_use", &hash, enabled);
    }
    fs::write(home.join(".codex/config.toml"), config).unwrap();
    record(
        "codex-linked-checkout-local-trusted-main-disabled",
        check("codex", &opts(&linked, &home)),
    );
    record(
        "codex-main-disabled-control",
        check("codex", &opts(&root, &home)),
    );

    grok_case(&linked, &home);
    store(&home, &[(&root, true)]);
    record(
        "grok-linked-checkout-main-grant",
        check("grok", &opts(&linked, &home)),
    );
    store(&home, &[(&root, false), (&linked, true)]);
    record(
        "grok-linked-checkout-main-deny-linked-grant",
        check("grok", &opts(&linked, &home)),
    );
    let managed = home.join(".grok/worktrees/managed-clone");
    grok_case(&managed, &home);
    store(&home, &[(&managed, true)]);
    record(
        "grok-managed-worktree-cannot-verify",
        check("grok", &opts(&managed, &home)),
    );

    // Round 2: `doctor-trust-r2-probe.rs`, the Grok records.
    let project = base.join("grok").join("project");
    let home = base.join("grok").join("home");
    fs::create_dir_all(&project).unwrap();
    let env_on: fn(&str) -> Option<String> =
        |name| (name == "GROK_FOLDER_TRUST").then(|| "enabled".to_string());
    let env_off: fn(&str) -> Option<String> =
        |name| (name == "GROK_FOLDER_TRUST").then(|| "NO".to_string());

    grok_case(&project, &home);
    fs::write(
        home.join(".grok/config.toml"),
        "[folder_trust]\nenabled = false\n",
    )
    .unwrap();
    let mut options = opts(&project, &home);
    options.env_var = Some(env_on);
    record("grok-env-on-over-user-off", check("grok", &options));
    options.env_var = Some(env_off);
    record("grok-env-off", check("grok", &options));

    grok_case(&project, &home);
    fs::write(
        home.join(".grok/trusted_folders.toml"),
        format!(
            "[folders.{}]\ntrusted = true\ndecided_at = \"yesterday\"\n",
            serde_json::to_string(&project.display().to_string()).unwrap()
        ),
    )
    .unwrap();
    record(
        "grok-malformed-record",
        check("grok", &opts(&project, &home)),
    );

    for (label, file) in [
        (
            "grok-user-version-override-enables-gate-untrusted",
            "config.toml",
        ),
        (
            "grok-managed-version-override-enables-gate-untrusted",
            "managed_config.toml",
        ),
    ] {
        grok_case(&project, &home);
        fs::write(
            home.join(".grok").join(file),
            "[folder_trust]\nenabled = false\n\n[[version_overrides]]\n[version_overrides.folder_trust]\nenabled = true\n",
        )
        .unwrap();
        record(label, check("grok", &opts(&project, &home)));
    }

    grok_case(&project, &home);
    store(
        &home,
        &[(project.parent().unwrap(), true), (&project, false)],
    );
    record("grok-nearest-deny", check("grok", &opts(&project, &home)));

    grok_case(&project, &home);
    store(&home, &[(&project, true)]);
    record("grok-trusted-folder", check("grok", &opts(&project, &home)));

    // A relative GROK_HOME: no process-wide directory change is needed, as
    // doctor refuses a relative home before it reads anything.
    let relative = Options {
        project_dir: project.to_string_lossy().into_owned(),
        look_path: Some(|_| Ok("synthetic-harness".into())),
        env_var: Some(|name| (name == "GROK_HOME").then(|| "relative-grok".to_string())),
        ..Options::default()
    };
    record("grok-relative-home-store", check("grok", &relative));

    let repo = base.join("repo");
    let linked = base.join("repo-linked");
    let linked_home = base.join("repo-linked-home");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "probe@example.invalid"]);
    git(&repo, &["config", "user.name", "Probe"]);
    fs::write(repo.join("tracked"), "x").unwrap();
    git(&repo, &["add", "tracked"]);
    git(&repo, &["commit", "-qm", "probe"]);
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "probe-linked",
            linked.to_str().unwrap(),
        ],
    );
    grok_case(&linked, &linked_home);
    store(&linked_home, &[(&linked, true)]);
    record(
        "grok-linked-worktree-keyed-to-linked-checkout",
        check("grok", &opts(&linked, &linked_home)),
    );
    out
}

/// TSK-147 round 3 F2: no static trust reading yields Pass, and every
/// configuration doctor finds matching is a Note that says the runtime is
/// not verified and names the check that verifies it.
#[test]
fn no_trust_probe_passes_without_an_observed_run() {
    let base = tempfile::tempdir().unwrap();
    let base = codeflow_core::portable_path::canonicalize(base.path()).unwrap();
    let results = probes(&base);
    assert!(results.len() >= 30, "{} probes", results.len());
    let mut passed = Vec::new();
    for (label, result) in &results {
        match &result.status {
            Status::Pass => passed.push(format!("{label}: {}", result.message)),
            Status::Note(remedy) if result.message.contains("configured; runtime not verified") => {
                let remedy = remedy.to_string();
                assert!(
                    remedy.contains("cannot verify") && remedy.contains("real"),
                    "{label}: the note names the check that verifies it: {remedy}"
                );
            }
            _ => {}
        }
    }
    assert!(
        passed.is_empty(),
        "static readings passed:\n{}",
        passed.join("\n")
    );
    // The matching configurations the reviewer used as positive controls
    // are notes, not passes.
    for label in [
        "codex-positive",
        "normalized-default-timeout",
        "grok-env-off",
        "grok-linked-checkout-main-grant",
        "grok-trusted-folder",
    ] {
        let (_, result) = results.iter().find(|(l, _)| l == label).unwrap();
        assert!(
            matches!(result.status, Status::Note(_))
                && result.message.contains("configured; runtime not verified"),
            "{label}: {:?}: {}",
            result.status,
            result.message
        );
    }
}
