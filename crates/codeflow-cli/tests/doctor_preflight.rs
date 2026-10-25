//! `codeflow doctor --preflight` prints the evidence the orchestrator
//! preflight reads, marking each value configured, observed or unavailable
//! (TSK-452 AC-6). Disposable project folders only; nothing is executed.
use std::path::{Path, PathBuf};
use std::process::Command;

const CATALOG: &str = ".agents/skills/cf-model-orchestrator/resources/current-ensemble.json";

fn repo_file(rel: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(rel)).expect("repository file reads")
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A project with the managed seat catalog and both harness settings.
fn configured_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        CATALOG,
        &repo_file(
            "assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json",
        ),
    );
    write(
        dir.path(),
        ".claude/settings.json",
        "{\"sandbox\": {\"enabled\": true, \"failIfUnavailable\": true}}\n",
    );
    write(
        dir.path(),
        ".codex/config.toml",
        "default_permissions = \"cf-guard\"\n[permissions.cf-guard]\n[permissions.cf-builder]\n",
    );
    dir
}

fn preflight(root: &Path, path_dir: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(root)
        .env("PATH", path_dir)
        .env("CODEFLOW_HOME", root.join("personal"))
        .args(["doctor", "--preflight"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

#[test]
fn configured_values_are_marked_configured_and_never_observed() {
    let project = configured_project();
    let empty = tempfile::tempdir().unwrap();
    let out = preflight(project.path(), empty.path());
    assert!(
        out.starts_with("preflight evidence for cf-model-orchestrator; read it, then make the live observations the preflight requires"),
        "{out}"
    );
    for line in [
        "seat claude-primary: configured claude-opus-5-5 through claude-code, efforts medium|high|xhigh",
        "seat codex-primary: configured gpt-6-astra through codex-cli",
        "sandbox claude: configured enabled true, failIfUnavailable true (.claude/settings.json)",
        "sandbox codex: configured default profile cf-guard, profiles cf-builder|cf-guard (.codex/config.toml)",
        "executable claude: unavailable: not found on PATH",
        "executable codex: unavailable: not found on PATH",
        "tool gh: unavailable: not found on PATH",
    ] {
        assert!(out.contains(line), "missing {line:?} in\n{out}");
    }
    assert!(!out.contains("observed"), "nothing was observed:\n{out}");
}

#[test]
fn executables_on_path_are_marked_observed() {
    let project = configured_project();
    let bin = tempfile::tempdir().unwrap();
    for name in ["claude", "codex", "git", "gh"] {
        write(bin.path(), name, "not run\n");
    }
    let out = preflight(project.path(), bin.path());
    for name in [
        "executable claude",
        "executable codex",
        "tool git",
        "tool gh",
    ] {
        assert!(
            out.contains(&format!("{name}: observed on PATH at ")),
            "{name} not observed in\n{out}"
        );
    }
    assert!(
        out.contains("tool herdr: unavailable: not found on PATH"),
        "{out}"
    );
    // A found executable never stands in for the live selection.
    assert!(
        out.contains("claude-primary live model and effort: unavailable: doctor cannot observe what a running session selected"),
        "{out}"
    );
}

#[test]
fn missing_evidence_is_reported_unavailable() {
    let project = tempfile::tempdir().unwrap();
    let empty = tempfile::tempdir().unwrap();
    let out = preflight(project.path(), empty.path());
    for line in [
        "seats: unavailable: no seat catalog at .agents/skills/cf-model-orchestrator/resources/current-ensemble.json",
        "sandbox claude: unavailable: no readable .claude/settings.json",
        "sandbox codex: unavailable: no readable .codex/config.toml",
        "effective boundary: unavailable: doctor cannot observe the boundary a running session enforces",
    ] {
        assert!(out.contains(line), "missing {line:?} in\n{out}");
    }
    assert!(!out.contains("configured"), "nothing is configured:\n{out}");
}

#[test]
fn an_unparsable_codex_config_is_reported_unavailable() {
    let project = configured_project();
    write(
        project.path(),
        ".codex/config.toml",
        "default_permissions = [\n",
    );
    let empty = tempfile::tempdir().unwrap();
    let out = preflight(project.path(), empty.path());
    assert!(
        out.contains("sandbox codex: unavailable: .codex/config.toml is not valid TOML"),
        "{out}"
    );
}
