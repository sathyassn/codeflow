//! The journey gate map (TSK-110 AC-1, SPC-013 R-104, R-107) matches the
//! spec and the code: one entry per journey R-104 lists, in its order; each
//! with a passing control and a fault; every named test exists as a test; a
//! pending journey names an incomplete task; and the gate runs in CI on the
//! Linux gate job and the Windows job.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("{relative}: {error}"))
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The journeys R-104 lists after "the set includes at least:", in order.
fn spec_journeys() -> Vec<String> {
    let spec = read("project-management/specs/SPC-013.md");
    let start = spec.find("- R-104.").expect("R-104");
    let end = start + spec[start..].find("- R-105.").expect("R-105");
    let text = normalize(&spec[start..end]);
    let list = text
        .split_once("the set includes at least:")
        .expect("R-104 lists its journeys")
        .1
        .trim()
        .trim_end_matches('.');
    list.split(';')
        .map(|item| {
            let item = item.trim();
            item.strip_prefix("and ").unwrap_or(item).to_string()
        })
        .collect()
}

struct Journey {
    text: String,
    control: Vec<String>,
    fault: Vec<String>,
    pending: Option<String>,
    unix_only: bool,
    benchmark: bool,
}

fn journeys() -> Vec<Journey> {
    let map: toml::Value =
        toml::from_str(&read("crates/codeflow-cli/tests/journey_gate.toml")).expect("gate map");
    let strings = |value: Option<&toml::Value>| -> Vec<String> {
        value
            .and_then(toml::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().expect("a test is a string").to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    map["journey"]
        .as_array()
        .expect("journeys")
        .iter()
        .map(|entry| Journey {
            text: normalize(entry["text"].as_str().expect("text")),
            control: strings(entry.get("control")),
            fault: strings(entry.get("fault")),
            pending: entry
                .get("pending")
                .and_then(toml::Value::as_str)
                .map(str::to_owned),
            unix_only: entry.get("platform").and_then(toml::Value::as_str) == Some("unix"),
            benchmark: entry
                .get("benchmark")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
        })
        .collect()
}

/// Whether `source` defines `name` as a test (`#[test]` within the lines
/// above it), and whether it is ignored.
fn test_in(source: &str, name: &str) -> Option<bool> {
    let lines: Vec<&str> = source.lines().collect();
    let at = lines.iter().position(|line| {
        let line = line.trim_start();
        line.starts_with(&format!("fn {name}(")) || line.starts_with(&format!("async fn {name}("))
    })?;
    let above = &lines[at.saturating_sub(4)..at];
    if !above
        .iter()
        .any(|line| line.trim_start().starts_with("#[test]"))
    {
        return None;
    }
    Some(
        above
            .iter()
            .any(|line| line.trim_start().starts_with("#[ignore")),
    )
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Resolve one test reference; returns the problem when it does not name a
/// runnable test.
fn check_test(reference: &str, journey: &Journey) -> Option<String> {
    let parts: Vec<&str> = reference.split_whitespace().collect();
    match parts.as_slice() {
        ["python", script, class] => {
            let source = read(script);
            (!source.contains(&format!("class {class}(unittest.TestCase)")))
                .then(|| format!("{reference}: no such unittest class"))
        }
        [package, "--test", target, name] => {
            let file = format!("crates/{package}/tests/{target}.rs");
            let Ok(source) = std::fs::read_to_string(repo_root().join(&file)) else {
                return Some(format!("{reference}: {file} is missing"));
            };
            if source.contains("#![cfg(unix)]") && !journey.unix_only {
                return Some(format!(
                    "{reference}: {file} is unix-only, so the journey needs platform = \"unix\""
                ));
            }
            match test_in(&source, name) {
                None => Some(format!("{reference}: no #[test] fn {name} in {file}")),
                Some(true) if !journey.benchmark => {
                    Some(format!("{reference}: an ignored test runs nowhere by default"))
                }
                Some(_) => None,
            }
        }
        [package, "--lib", path] => {
            let name = path.rsplit("::").next().unwrap_or(path);
            let mut files = Vec::new();
            rust_sources(&repo_root().join(format!("crates/{package}/src")), &mut files);
            let found: Vec<bool> = files
                .iter()
                .filter_map(|file| test_in(&std::fs::read_to_string(file).unwrap(), name))
                .collect();
            match found.as_slice() {
                [false] => None,
                [] => Some(format!("{reference}: no #[test] fn {name} in crates/{package}/src")),
                [true] => Some(format!("{reference}: the test is ignored")),
                _ => Some(format!("{reference}: {name} is defined more than once")),
            }
        }
        _ => Some(format!("{reference}: not `<package> --test <target> <fn>`, `<package> --lib <path>` or `python <script> <class>`")),
    }
}

#[test]
fn the_gate_map_lists_every_r104_journey_in_order() {
    let spec = spec_journeys();
    let map: Vec<String> = journeys().into_iter().map(|journey| journey.text).collect();
    assert!(spec.len() >= 40, "R-104 lists {} journeys", spec.len());
    for (n, (spec, map)) in spec.iter().zip(&map).enumerate() {
        assert_eq!(map, spec, "journey {} differs from R-104", n + 1);
    }
    assert_eq!(
        map.len(),
        spec.len(),
        "the map and R-104 list different counts"
    );
}

#[test]
fn every_journey_names_a_passing_control_and_a_fault_that_exist() {
    let mut problems = Vec::new();
    for journey in journeys() {
        if let Some(task) = &journey.pending {
            let record = read(&format!("project-management/tasks/{task}.md"));
            let status = record
                .lines()
                .find_map(|line| line.strip_prefix("status:"))
                .and_then(|value| value.split('#').next())
                .map(str::trim);
            if status == Some("complete") {
                problems.push(format!(
                    "{}: pending on {task}, which is complete; name its tests",
                    journey.text
                ));
            }
            if !journey.control.is_empty() || !journey.fault.is_empty() {
                problems.push(format!(
                    "{}: a pending journey names no tests",
                    journey.text
                ));
            }
            continue;
        }
        if journey.control.is_empty() {
            problems.push(format!("{}: no passing control", journey.text));
        }
        if journey.fault.is_empty() {
            problems.push(format!("{}: no fault", journey.text));
        }
        for reference in journey.control.iter().chain(&journey.fault) {
            problems.extend(check_test(reference, &journey));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The lines of one job in a workflow, up to the next job.
fn job(workflow: &str, id: &str) -> String {
    let mut lines = workflow
        .lines()
        .skip_while(|line| *line != format!("  {id}:"));
    let head = lines.next().unwrap_or_else(|| panic!("no {id} job"));
    std::iter::once(head)
        .chain(lines.take_while(|line| {
            !(line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
        }))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_gate_runs_on_the_linux_gate_job_and_the_windows_job() {
    let config: serde_json::Value =
        serde_json::from_str(&read(".codeflow/test-config.json")).expect("test config");
    let gate = config["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .find(|target| target["name"] == "journey-gate")
        .expect("a journey-gate target in .codeflow/test-config.json");
    assert_eq!(gate["enabled"], true);
    assert!(
        gate["modes"]["full"]["command"]
            .as_str()
            .is_some_and(|command| command.contains("scripts/journey-gate.py")),
        "the full gate runs the journey gate: {gate}"
    );
    let workflow = read(".github/workflows/codeflow-ci.yml");
    // TSK-203: the Windows suite runs in partitions; one job reads them all.
    assert!(
        job(&workflow, "windows-journeys").contains("scripts/journey-gate.py"),
        "the Windows journeys job runs the journey gate"
    );
    assert!(
        read("scripts/journey-gate.py").contains("journey_gate.toml"),
        "the runner reads the map"
    );
}

#[test]
fn candidate_jobs_follow_the_event_matrix_and_keep_security_on_every_pr() {
    let workflow = read(".github/workflows/codeflow-ci.yml");
    let trigger = workflow.split("permissions:").next().unwrap();
    assert!(trigger.contains("  pull_request:\n  push:"));
    assert!(trigger.contains("integration/**"));
    let condition = "github.event_name == 'pull_request' && contains(fromJSON('[\"main\",\"master\"]'), github.event.pull_request.base.ref) || github.event_name == 'push'";
    for id in [
        "gates",
        "gates-verdict",
        "windows-tests",
        "windows",
        "windows-journeys",
    ] {
        assert!(
            job(&workflow, id).contains(condition),
            "{id} must run for protected-base PRs and line pushes"
        );
    }
    for id in ["secret-scan", "security-review"] {
        assert!(
            !job(&workflow, id)
                .lines()
                .any(|line| line.starts_with("    if:")),
            "{id} runs for every PR"
        );
    }
    // Task PR: security only. Main PR, integration push and main push: all.
    for (event, base, heavy) in [
        ("pull_request", "integration/EPC-020-delivery-system", false),
        ("pull_request", "main", true),
        ("push", "integration/EPC-020-delivery-system", true),
        ("push", "main", true),
    ] {
        assert_eq!(event == "push" || matches!(base, "main" | "master"), heavy);
    }
    assert!(!workflow.contains("  rust:\n"));
    assert!(!workflow.contains("  coverage:\n"));
}

#[test]
fn instrumented_suite_has_serial_membership_and_a_junit_consumer() {
    let profile = read(".config/nextest.toml");
    let parsed: toml::Value = toml::from_str(&profile).unwrap();
    assert_eq!(
        parsed["test-groups"]["serial"]["max-threads"].as_integer(),
        Some(1)
    );
    assert_eq!(
        parsed["profile"]["codeflow"]["fail-fast"].as_bool(),
        Some(false)
    );
    assert_eq!(
        parsed["profile"]["codeflow"]["junit"]["path"].as_str(),
        Some("junit.xml")
    );
    for name in [
        "present_cli",
        "init_e2e",
        "process_group",
        "package(codeflow-present)",
    ] {
        assert!(profile.contains(name), "missing serial member {name}");
    }
    let config: serde_json::Value =
        serde_json::from_str(&read(".codeflow/test-config.json")).unwrap();
    let targets = config["targets"].as_array().unwrap();
    let journey = targets
        .iter()
        .find(|t| t["name"] == "journey-gate")
        .unwrap();
    assert_eq!(journey["requires"], serde_json::json!(["rust-coverage"]));
    assert!(journey["modes"]["full"]["command"]
        .as_str()
        .unwrap()
        .contains("--results"));
    assert!(job(
        &read(".github/workflows/codeflow-ci.yml"),
        "windows-journeys"
    )
    .contains("--results target/nextest/partitions"));
}

/// The quick gate, which the pre-push hook runs on every push, is the light
/// set: fmt, clippy and the Python contracts. The producers (the web build,
/// the workspace build and the binary) belong to essential and full, where
/// the tests embed their output; a quick target requires nothing outside
/// the quick set, or the runner would wait for a target that never runs.
#[test]
fn the_quick_gate_runs_only_the_light_targets() {
    let config: serde_json::Value =
        serde_json::from_str(&read(".codeflow/test-config.json")).expect("test config");
    let targets = config["targets"].as_array().expect("targets");
    let quick: std::collections::BTreeSet<&str> = targets
        .iter()
        .filter(|t| t["modes"].get("quick").is_some())
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        quick,
        [
            "gate-parity",
            "herdr-delivery",
            "rust-clippy",
            "rust-format",
            "skill-triggers",
        ]
        .into_iter()
        .collect(),
        "the quick gate runs the light targets only"
    );
    for target in targets
        .iter()
        .filter(|t| quick.contains(t["name"].as_str().unwrap()))
    {
        let requires: Vec<&str> = target["requires"]
            .as_array()
            .map(|r| r.iter().map(|v| v.as_str().unwrap()).collect())
            .unwrap_or_default();
        assert!(
            requires.iter().all(|r| quick.contains(r)),
            "{} requires {requires:?} outside the quick set",
            target["name"]
        );
    }
}
