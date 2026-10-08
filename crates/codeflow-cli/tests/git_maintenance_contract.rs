//! Cargo's fixture environment must reach both Git and Git started by `CodeFlow`.

use std::path::Path;
use std::process::{Command, Output};

fn fixture_command(program: &str, root: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(root)
        .env("CODEFLOW_HOME", root.join("home"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.test")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    command
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn init_never_starts_automatic_maintenance() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let trace = fixture.path().join("git-trace.jsonl");
    success(
        fixture_command(env!("CARGO_BIN_EXE_codeflow"), &root)
            .args(["init", "--yes", "--full"])
            .env("GIT_TRACE2_EVENT", &trace)
            .output()
            .unwrap(),
    );
    let events: Vec<serde_json::Value> = std::fs::read_to_string(trace)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(events.iter().any(|event| event["event"] == "start"));
    for event in events {
        if event["event"] == "child_start" {
            let args = event["argv"].as_array().expect("child argv");
            assert!(
                !args.iter().any(|arg| arg == "maintenance" || arg == "gc"),
                "init started automatic maintenance: {event}"
            );
        }
    }
}

#[test]
fn git_inherits_maintenance_settings_from_the_command_environment() {
    let fixture = tempfile::tempdir().unwrap();
    success(
        fixture_command("git", fixture.path())
            .args(["init", "-q"])
            .output()
            .unwrap(),
    );
    for (key, value) in [("maintenance.auto", "false"), ("gc.auto", "0")] {
        let output = success(
            fixture_command("git", fixture.path())
                .args(["config", "--show-origin", "--get", key])
                .output()
                .unwrap(),
        );
        assert_eq!(
            output.trim_end().split_once('\t'),
            Some(("command line:", value)),
            "{key} must come from Cargo's forced environment: {output}"
        );
    }
}
