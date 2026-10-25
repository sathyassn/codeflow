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

/// A fake `codex` on its own `PATH` folder. It answers `--version`, logs
/// each config load's folder and arguments to `codex.log` beside it, and
/// refuses the load with Codex's own message when `reject_without_project`
/// is set and the folder has no `.codex/config.toml`, as Codex 0.163 and
/// later do at TUI startup for a project-only profile.
#[cfg(unix)]
fn fake_codex(reject_without_project: bool) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    let log = bin.path().join("codex.log");
    let reject = if reject_without_project {
        "if [ ! -f .codex/config.toml ]; then\n  echo 'Error loading configuration: default_permissions requires a `[permissions]` table' >&2\n  exit 1\nfi\n"
    } else {
        ""
    };
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'codex-cli 9.9.9'; exit 0; fi\nprintf '%s|%s\\n' \"$PWD\" \"$*\" >> '{log}'\n{reject}echo 'apps  stable  true'\n",
        log = log.display()
    );
    let path = bin.path().join("codex");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

/// TSK-1003 AC-4: when the installed Codex refuses the builder overrides in
/// its startup load, which has no project config, the preflight says the
/// builder seat will not open.
#[cfg(unix)]
#[test]
fn a_codex_that_rejects_the_builder_profile_at_startup_is_reported() {
    let project = configured_project();
    let bin = fake_codex(true);
    let out = preflight(project.path(), bin.path());
    let line = out
        .lines()
        .find(|line| line.starts_with("codex builder profile:"))
        .unwrap_or_else(|| panic!("no builder line in\n{out}"));
    assert!(
        line.starts_with("codex builder profile: observed REJECTED by codex-cli 9.9.9: rejected in the startup load, without the project config: Error loading configuration: default_permissions requires a `[permissions]` table"),
        "{line}"
    );
    assert!(line.contains("will not open"), "{line}");
    assert!(
        !line.contains("project load:"),
        "the project load passed: {line}"
    );
}

/// TSK-1003 AC-4: a Codex that accepts the overrides both ways is reported
/// accepted, and the probe passes the exact launch overrides in both loads,
/// one of them from a folder with no project config.
#[cfg(unix)]
#[test]
fn a_codex_that_accepts_the_builder_profile_is_reported_with_the_exact_overrides() {
    let project = configured_project();
    let bin = fake_codex(false);
    let out = preflight(project.path(), bin.path());
    assert!(
        out.contains("codex builder profile: observed accepted by codex-cli 9.9.9 with the builder launch overrides, in the startup load and the project load"),
        "{out}"
    );
    let log = std::fs::read_to_string(bin.path().join("codex.log")).unwrap();
    let loads: Vec<&str> = log.lines().collect();
    assert_eq!(loads.len(), 2, "{log}");
    let expected = "features list -c permissions.cf-builder.description=\"CodeFlow builder seat\" -c default_permissions=\"cf-builder\"";
    for load in &loads {
        assert!(load.ends_with(&format!("|{expected}")), "{load}");
    }
    let project_dir = std::fs::canonicalize(project.path()).unwrap();
    let dirs: Vec<std::path::PathBuf> = loads
        .iter()
        .map(|load| std::fs::canonicalize(load.split('|').next().unwrap()).unwrap_or_default())
        .collect();
    assert!(dirs.contains(&project_dir), "{log}");
    assert!(
        dirs.iter().any(|dir| *dir != project_dir),
        "one load runs outside the project: {log}"
    );
}

/// TSK-1003: with no builder profile configured, nothing is run.
#[cfg(unix)]
#[test]
fn no_builder_profile_means_no_codex_run() {
    let project = configured_project();
    write(
        project.path(),
        ".codex/config.toml",
        "default_permissions = \"cf-guard\"\n[permissions.cf-guard]\n",
    );
    let bin = fake_codex(false);
    let out = preflight(project.path(), bin.path());
    assert!(
        out.contains(
            "codex builder profile: unavailable: .codex/config.toml defines no cf-builder profile"
        ),
        "{out}"
    );
    assert!(!bin.path().join("codex.log").exists(), "codex ran");
}
