//! TSK-1292: `codeflow models resolve` returns the operator's 2026-11-03
//! default models and efforts from the shipped catalog, and records a
//! session's harness choice without changing the resolution.
//!
//! Unlike `models_managed_catalog.rs`, these tests name catalog ids on
//! purpose: they pin the shipped defaults, so a catalog change that moves a
//! default fails here and must update this file with its decision.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const CATALOG: &str = ".agents/skills/cf-model-orchestrator/resources/current-ensemble.json";

fn managed_text() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json",
    );
    std::fs::read_to_string(path).unwrap()
}

fn managed() -> Value {
    serde_json::from_str(&managed_text()).unwrap()
}

struct Project {
    dir: tempfile::TempDir,
}

impl Project {
    /// A disposable repository carrying the shipped catalog.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CATALOG);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, managed_text()).unwrap();
        let status = Command::new("git")
            .current_dir(dir.path())
            .args(["init", "-q"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .unwrap();
        assert!(status.success());
        Self { dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codeflow"))
            .current_dir(self.root())
            .env("CODEFLOW_HOME", self.root().join("personal"))
            .args(["models", "resolve", "--json"])
            .args(args)
            .output()
            .unwrap()
    }

    fn resolve(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

/// The first participant's seat, pinned id and effort.
fn first(result: &Value) -> (String, String, String) {
    let p = &result["participants"][0];
    (
        p["seat"].as_str().unwrap_or("worker").to_owned(),
        p["pinned_id"].as_str().unwrap().to_owned(),
        p["effort"].as_str().unwrap().to_owned(),
    )
}

fn triple(seat: &str, pinned: &str, effort: &str) -> (String, String, String) {
    (seat.into(), pinned.into(), effort.into())
}

/// AC-1: Opus 5.5 for the Claude seat, GPT-6.1 Sol at medium for the Codex
/// seat, and Grok 4.7 at medium, or at high on a security, release or
/// hard-review trigger.
#[test]
fn the_seats_resolve_the_operator_defaults() {
    let project = Project::new();
    let host = project.resolve(&["--duty", "orchestrate", "--host", "claude-code"]);
    assert_eq!(
        first(&host),
        triple("claude-primary", "claude-opus-5-5", "high")
    );
    // Every host's orchestrate route runs at that seat's own effort.
    for (host, expected) in [
        (
            "codex-app",
            triple("codex-primary", "gpt-6.1-sol", "medium"),
        ),
        (
            "codex-cli",
            triple("codex-primary", "gpt-6.1-sol", "medium"),
        ),
        ("grok-cli", triple("grok-primary", "grok-4.7", "medium")),
    ] {
        let result = project.resolve(&["--duty", "orchestrate", "--host", host]);
        assert_eq!(first(&result), expected, "{host}");
    }
    let codex = project.resolve(&[
        "--duty",
        "unit-review",
        "--host",
        "claude-code",
        "--author",
        "claude",
    ]);
    assert_eq!(
        first(&codex),
        triple("codex-primary", "gpt-6.1-sol", "medium")
    );
    let grok = project.resolve(&[
        "--duty",
        "general-review",
        "--host",
        "claude-code",
        "--author",
        "claude",
    ]);
    assert_eq!(first(&grok), triple("grok-primary", "grok-4.7", "medium"));
    for trigger in [
        "security-sensitive change",
        "release or publication",
        "complex architecture",
    ] {
        let raised = project.resolve(&[
            "--duty",
            "general-review",
            "--host",
            "claude-code",
            "--author",
            "claude",
            "--trigger",
            trigger,
        ]);
        assert_eq!(
            first(&raised),
            triple("grok-primary", "grok-4.7", "high"),
            "{trigger}"
        );
    }
    // The hard-review trigger adds the Grok participant at high.
    let extra = project.resolve(&[
        "--duty",
        "unit-review",
        "--host",
        "claude-code",
        "--author",
        "claude",
        "--trigger",
        "security-sensitive change",
    ]);
    let grok = &extra["participants"][1];
    assert_eq!(grok["pinned_id"], "grok-4.7");
    assert_eq!(grok["effort"], "high");
    // The Codex seat stays at medium on that trigger: only Grok rises.
    assert_eq!(extra["participants"][0]["effort"], "medium");
}

/// AC-1, the sol line: GPT-6 Sol, then GPT-5.6 Sol, back the Codex seat when
/// GPT-6.1 Sol is unavailable, at the seat's own effort.
#[test]
fn the_sol_line_falls_back_in_order() {
    let project = Project::new();
    let args = [
        "--duty",
        "unit-review",
        "--host",
        "claude-code",
        "--author",
        "claude",
    ];
    let result = project.resolve(&args);
    let remaining: Vec<&str> = result["participants"][0]["remaining_alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["pinned_id"].as_str().unwrap())
        .collect();
    assert_eq!(remaining, ["gpt-6-sol", "gpt-5.6-sol"]);
    let mut excluded = args.to_vec();
    excluded.extend(["--exclude", "selector:gpt-6.1-sol"]);
    let fallback = project.resolve(&excluded);
    assert_eq!(
        first(&fallback),
        triple("codex-primary", "gpt-6-sol", "medium")
    );
    assert_eq!(fallback["participants"][0]["reduced_assurance"], false);
}

/// AC-2: Fable and GPT-6 Astra are `launch: subagent-only`: they resolve for
/// subagent duties and never for a seat.
#[test]
fn fable_and_astra_resolve_only_as_subagents() {
    let catalog = managed();
    for id in ["fable", "astra"] {
        let line = catalog["lines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == id)
            .unwrap();
        assert_eq!(line["launch"], "subagent-only", "{id}");
    }
    let project = Project::new();
    // Fable: the reasoning, consultation and advisory review routes.
    for duty in [
        "reasoning-support",
        "consultation",
        "design-and-editorial-review",
    ] {
        let result =
            project.resolve(&["--duty", duty, "--host", "claude-code", "--author", "codex"]);
        let fable = result["participants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["pinned_id"] == "claude-fable-5-1")
            .unwrap_or_else(|| panic!("{duty}: {result}"));
        assert!(fable["seat"].is_null(), "{duty}: {fable}");
    }
    // Astra: the Codex-hosted reasoning worker (Sol serves the seat there),
    // never the seat.
    let astra = project.resolve(&[
        "--duty",
        "reasoning-support",
        "--host",
        "codex-app",
        "--author",
        "codex",
    ]);
    assert_eq!(astra["participants"][0]["pinned_id"], "gpt-6-astra");
    assert!(astra["participants"][0]["seat"].is_null());
    // No seat resolution anywhere returns either line.
    for duty in [
        "orchestrate",
        "independent-plan",
        "unit-review",
        "body-review",
        "design",
    ] {
        for host in ["claude-code", "codex-app", "grok-cli"] {
            let output = project.run(&["--duty", duty, "--host", host, "--author", "claude"]);
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            for p in result["participants"].as_array().unwrap() {
                if p["seat"].is_string() {
                    assert!(
                        !["claude-fable-5-1", "gpt-6-astra"]
                            .contains(&p["pinned_id"].as_str().unwrap()),
                        "{duty} on {host}: {p}"
                    );
                }
            }
        }
    }
}

/// AC-5: Sonnet and Luna stay available when a route names them.
#[test]
fn sonnet_and_luna_still_resolve_when_a_route_names_them() {
    let project = Project::new();
    let bounded = project.resolve(&["--duty", "bounded-execution", "--host", "claude-code"]);
    let mut pins: Vec<&str> = vec![bounded["participants"][0]["pinned_id"].as_str().unwrap()];
    pins.extend(
        bounded["participants"][0]["remaining_alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["pinned_id"].as_str().unwrap()),
    );
    assert!(pins.contains(&"claude-sonnet-5-5"), "{pins:?}");
    let light = project.resolve(&["--duty", "light-execution", "--host", "codex-app"]);
    assert_eq!(first(&light), triple("worker", "gpt-6-luna", "medium"));
}

/// AC-9: a session the harness started at another model or effort runs
/// unchanged. The resolution is identical with and without `--effective`;
/// the output adds only an `effective_session` object (JSON) or one
/// `effective session:` line (text). The record is stored under the
/// harness session id, in that session's ledger file.
#[test]
fn a_harness_choice_is_recorded_and_changes_nothing() {
    let project = Project::new();
    let args = ["--duty", "orchestrate", "--host", "claude-code"];
    let plain = project.resolve(&args);
    let mut with = args.to_vec();
    with.extend([
        "--effective",
        "claude-sonnet-5-5@low",
        "--session",
        "4f1c-session-one",
    ]);
    let output = project.run(&with);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let mut recorded: Value = serde_json::from_slice(&output.stdout).unwrap();
    let effective = recorded
        .as_object_mut()
        .unwrap()
        .remove("effective_session")
        .unwrap();
    // Nothing in the resolution moved: the suggestion stays a suggestion.
    assert_eq!(recorded, plain);
    assert_eq!(effective["model"], "claude-sonnet-5-5");
    assert_eq!(effective["effort"], "low");
    assert_eq!(effective["host"], "claude-code");
    assert_eq!(effective["session"], "4f1c-session-one");
    assert_eq!(effective["applies_over_suggestion"], true);
    // The record is stored, not only echoed, in the session's own file.
    let path = effective["recorded_to"].as_str().unwrap();
    assert!(
        // `sid-` and the first 128 bits of SHA-256("ses-4f1c-session-one").
        path.ends_with("sessions-sid-623cc6e1772f1ec1bdf4312bbcc686bf.jsonl"),
        "{path}"
    );
    let ledger = session_events(project.root(), "4f1c-session-one");
    assert_eq!(ledger.len(), 1, "{ledger:?}");
    assert_eq!(ledger[0]["event"], "effective_model");
    assert_eq!(ledger[0]["session_id"], "ses-4f1c-session-one");
    assert_eq!(ledger[0]["model"], "claude-sonnet-5-5");
    assert_eq!(ledger[0]["effort"], "low");
    assert_eq!(ledger[0]["source"], "harness choice");
    // Malformed values are input errors, and nothing resolves; so is a
    // session id that is not one word of ASCII letters, digits, - or _.
    let too_long = "a".repeat(101);
    for (effective, session) in [
        ("no-at-sign", "s1"),
        ("@high", "s1"),
        ("model@", "s1"),
        ("a b@high", "s1"),
        ("model@hi\u{2003}gh", "s1"),
        ("mod\u{1b}el@high", "s1"),
        ("model@high", ""),
        ("model@high", "../escape"),
        ("model@high", "a b"),
        // One over the 100-character bound that keeps the file name short.
        ("model@high", too_long.as_str()),
    ] {
        let mut args = args.to_vec();
        args.extend(["--effective", effective, "--session", session]);
        let output = project.run(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{effective} {session}: {output:?}"
        );
        assert!(output.stdout.is_empty(), "{effective} {session}");
    }
    // --effective needs --session, and --session needs --effective.
    for extra in [["--effective", "model@high"], ["--session", "s1"]] {
        let mut args = args.to_vec();
        args.extend(extra);
        assert_eq!(project.run(&args).status.code(), Some(2), "{extra:?}");
    }
}

/// AC-9: two sessions in one worktree on one host record their harness
/// choices apart, and each record sits beside that session's own summary,
/// whose `SessionEnd` hook payload carries the same id.
#[test]
fn harness_choices_correlate_with_their_own_session() {
    let project = Project::new();
    for (session, effective) in [
        ("session-a", "claude-opus-5-5@high"),
        ("session-b", "claude-sonnet-5-5@medium"),
    ] {
        let output = project.run(&[
            "--duty",
            "orchestrate",
            "--host",
            "claude-code",
            "--effective",
            effective,
            "--session",
            session,
        ]);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        session_end(project.root(), session);
    }
    for (session, model) in [
        ("session-a", "claude-opus-5-5"),
        ("session-b", "claude-sonnet-5-5"),
    ] {
        let events = session_events(project.root(), session);
        let kinds: Vec<&str> = events
            .iter()
            .map(|e| e["event"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            ["effective_model", "session_end"],
            "{session}: {events:?}"
        );
        for event in &events {
            assert_eq!(event["session_id"], format!("ses-{session}").as_str());
        }
        assert_eq!(events[0]["model"], model, "{session}");
    }
}

/// AC-9: harness ids that differ only by case, by `_` or by a `ses-`
/// prefix each keep their own file, through `--effective` and the real
/// `SessionEnd` hook, on a case-insensitive volume too, and a file an
/// earlier release wrote is never written again. Before, `alpha` and
/// `ses-alpha` both wrote `sessions-ses-alpha.jsonl`, and `Alpha` opened
/// that same file where case folds.
#[test]
fn session_ids_that_differ_by_case_prefix_or_underscore_never_share_a_file() {
    let project = Project::new();
    // A file 3.1.x wrote for session `_alpha`.
    let legacy_line =
        r#"{"event":"session_end","timestamp":"2026-01-01T00:00:00Z","session_id":"ses-_alpha"}"#;
    let sessions = ledger_sessions_dir(project.root());
    std::fs::create_dir_all(&sessions).unwrap();
    let legacy = sessions.join("sessions-ses-_alpha.jsonl");
    std::fs::write(&legacy, format!("{legacy_line}\n")).unwrap();
    let cases = [
        ("alpha", "claude-opus-5-5"),
        ("Alpha", "claude-sonnet-5-5"),
        ("ALPHA", "claude-opus-5-5"),
        ("_alpha", "claude-sonnet-5-5"),
        ("ses-alpha", "claude-opus-5-5"),
        ("a_b", "claude-sonnet-5-5"),
        ("a__b", "claude-opus-5-5"),
    ];
    for (session, model) in cases {
        let effective = format!("{model}@high");
        let output = project.run(&[
            "--duty",
            "orchestrate",
            "--host",
            "claude-code",
            "--effective",
            effective.as_str(),
            "--session",
            session,
        ]);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        session_end(project.root(), session);
    }
    for (session, model) in cases {
        // A shared file would hold another id's records, so four or more.
        let events = session_events(project.root(), session);
        assert_eq!(events.len(), 2, "{session}: {events:?}");
        assert_eq!(events[0]["event"], "effective_model", "{session}");
        assert_eq!(events[0]["model"], model, "{session}");
        assert_eq!(events[1]["event"], "session_end", "{session}");
        for event in &events {
            assert_eq!(
                event["session_id"],
                format!("ses-{session}").as_str(),
                "{session}"
            );
        }
    }
    assert_eq!(
        std::fs::read_to_string(&legacy).unwrap(),
        format!("{legacy_line}\n"),
        "the legacy file is left as it was"
    );
}

/// The sessions-ledger directory under the project's state directory.
fn ledger_sessions_dir(root: &Path) -> std::path::PathBuf {
    root.join(".git/codeflow/ledger/sessions")
}

// The upgrade from a schema 5 catalog (refused, then `codeflow update`) is
// `init_e2e.rs` `an_upgrade_refuses_a_schema_5_catalog_until_codeflow_update`.

/// Run the `SessionEnd` hook for `session` the way a harness does.
fn session_end(root: &Path, session: &str) {
    use std::io::Write as _;
    let mut child = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(root)
        .env("CODEFLOW_HOME", root.join("personal"))
        .args(["hook", "session-summary"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(format!(r#"{{"session_id":"{session}","reason":"other"}}"#).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

/// Every event in harness session `session`'s sessions-ledger file under
/// the project's state directory, in file order. The name compares byte
/// for byte, so a file that differs only by case is not this one.
fn session_events(root: &Path, session: &str) -> Vec<Value> {
    let identity = codeflow_core::ledger::session_identity(session).unwrap();
    let key = identity.file_key();
    let name = format!("sessions-{key}.jsonl");
    let mut stack = vec![root.join(".git")];
    let mut found = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == name.as_str()) {
                for line in std::fs::read_to_string(&path).unwrap().lines() {
                    found.push(serde_json::from_str(line).unwrap());
                }
            }
        }
    }
    found
}
