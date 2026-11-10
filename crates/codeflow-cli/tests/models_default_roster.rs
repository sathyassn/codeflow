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
/// unchanged. The resolution is identical with and without `--effective`,
/// and the effective model and effort are stored in the sessions ledger.
#[test]
fn a_harness_choice_is_recorded_and_changes_nothing() {
    let project = Project::new();
    let args = ["--duty", "orchestrate", "--host", "claude-code"];
    let plain = project.resolve(&args);
    let mut with = args.to_vec();
    with.extend(["--effective", "claude-sonnet-5-5@low"]);
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
    assert_eq!(effective["applies_over_suggestion"], true);
    // The record is stored, not only echoed.
    let ledger = find_effective_record(&project.root().join(".git"));
    assert_eq!(ledger["event"], "effective_model");
    assert_eq!(ledger["model"], "claude-sonnet-5-5");
    assert_eq!(ledger["effort"], "low");
    assert_eq!(ledger["source"], "harness choice");
    let path = effective["recorded_to"].as_str().unwrap();
    assert!(path.ends_with("sessions.jsonl"), "{path}");
    // A malformed value is an input error, and nothing resolves.
    for bad in ["no-at-sign", "@high", "model@", "a b@high"] {
        let mut args = args.to_vec();
        args.extend(["--effective", bad]);
        let output = project.run(&args);
        assert_eq!(output.status.code(), Some(2), "{bad}: {output:?}");
        assert!(output.stdout.is_empty(), "{bad}");
    }
}

/// The one `effective_model` event under the project's state directory.
fn find_effective_record(root: &Path) -> Value {
    let mut stack = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "sessions.jsonl") {
                for line in std::fs::read_to_string(&path).unwrap().lines() {
                    let event: Value = serde_json::from_str(line).unwrap();
                    if event["event"] == "effective_model" {
                        found.push(event);
                    }
                }
            }
        }
    }
    assert_eq!(found.len(), 1, "{found:?}");
    found.remove(0)
}
