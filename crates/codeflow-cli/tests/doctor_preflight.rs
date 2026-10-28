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
    let (code, stdout) = preflight_status(root, path_dir);
    assert_eq!(code, Some(0), "{stdout}");
    stdout
}

/// The preflight's exit code and output.
fn preflight_status(root: &Path, path_dir: &Path) -> (Option<i32>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(root)
        .env("PATH", path_dir)
        .env("CODEFLOW_HOME", root.join("personal"))
        .args(["doctor", "--preflight"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    (
        output.status.code(),
        format!("{stdout}\n{}", String::from_utf8_lossy(&output.stderr)),
    )
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

/// What a fake `codex` does with a `features list` run.
#[cfg(unix)]
#[derive(Clone, Copy, PartialEq)]
enum Fake {
    /// Prints the feature table; in a folder with `.codex/config.toml` it
    /// shows `network_proxy true`, as when Codex loads the project config.
    LoadsProject,
    /// Prints the feature table but never the project's setting, as when
    /// Codex skips an untrusted project's config.
    SkipsProject,
    /// Refuses the run in a folder without `.codex/config.toml`, with
    /// Codex's own message, as Codex 0.163 and later do at TUI startup for
    /// a project-only profile.
    RejectsStartup,
    /// Refuses the run in the project folder.
    RejectsProject,
    /// Exits 0 and prints nothing.
    PrintsNothing,
    /// Prints a junk line, then exactly 64 KiB of valid feature rows, so
    /// the last 64 KiB alone look like a whole valid table.
    PadsOutput,
    /// In the project folder shows `network_proxy` with the project's value
    /// but `hooks` without it: contradictory evidence.
    Contradicts,
    /// Answers `--version` with something that is not a codex-cli version.
    NoVersion,
}

/// What a fake `codex` does with a `codex sandbox` run (TSK-337).
#[cfg(unix)]
#[derive(Clone, Copy, PartialEq)]
enum Grants {
    /// Runs the command after `--` unsandboxed, so the real permissions
    /// check answers.
    Runs,
    /// Prints a failing permissions result, as when the profile from the
    /// launch folder cannot write the home lock folder.
    Denied,
    /// Aborts with no output, as a sandbox whose profile cannot run a
    /// program does.
    Aborts,
}

/// A fake `codex` on its own `PATH` folder. It logs each `features list`
/// and `sandbox` run's folder and arguments to `codex.log` beside it.
#[cfg(unix)]
fn fake_codex(fake: Fake) -> tempfile::TempDir {
    fake_codex_with(fake, Grants::Runs)
}

#[cfg(unix)]
fn fake_codex_with(fake: Fake, grants: Grants) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    let log = bin.path().join("codex.log");
    let version = if fake == Fake::NoVersion {
        "echo 'something else'"
    } else {
        "echo 'codex-cli 9.9.9'"
    };
    let body = match fake {
        Fake::LoadsProject => "proxy=false\nif [ -f .codex/config.toml ]; then proxy=true; fi\nprintf 'apps  stable  true\\nnetwork_proxy  stable  %s\\n' \"$proxy\"\n",
        Fake::SkipsProject | Fake::NoVersion => "printf 'apps  stable  true\\nnetwork_proxy  stable  false\\n'\n",
        Fake::RejectsStartup => "if [ ! -f .codex/config.toml ]; then\n  echo 'Error loading configuration: default_permissions requires a `[permissions]` table' >&2\n  exit 1\nfi\nprintf 'network_proxy  stable  true\\n'\n",
        Fake::RejectsProject => "if [ -f .codex/config.toml ]; then\n  echo 'Error loading configuration: unknown permission key' >&2\n  exit 1\nfi\nprintf 'network_proxy  stable  false\\n'\n",
        Fake::PrintsNothing => "",
        Fake::PadsOutput => "echo 'JUNK'\ni=0\nwhile [ $i -lt 2048 ]; do printf 'f%05d             stable  true\\n' $i; i=$((i+1)); done\n",
        Fake::Contradicts => "proxy=false\nif [ -f .codex/config.toml ]; then proxy=true; fi\nprintf 'hooks  stable  false\\nnetwork_proxy  stable  %s\\n' \"$proxy\"\n",
    };
    let sandbox = match grants {
        Grants::Runs => "while [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"",
        Grants::Denied => "echo 'warn  permissions: essential and full gates cannot write their lock or evidence directories (/h/.codeflow/locks: Operation not permitted (os error 1))'\necho '      clear it: relaunch'\nexit 0",
        Grants::Aborts => "exit 134",
    };
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then {version}; exit 0; fi\nprintf '%s|%s\\n' \"$PWD\" \"$*\" >> '{log}'\nif [ \"$1\" = sandbox ]; then\n{sandbox}\nfi\n{body}",
        log = log.display()
    );
    let path = bin.path().join("codex");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

/// A configured project whose Codex config sets a feature, so the probe can
/// tell whether Codex loaded it.
#[cfg(unix)]
fn builder_project() -> tempfile::TempDir {
    let project = configured_project();
    write(
        project.path(),
        ".codex/config.toml",
        "default_permissions = \"cf-guard\"\n[features]\nnetwork_proxy = true\n[permissions.cf-guard]\n[permissions.cf-builder]\n",
    );
    project
}

#[cfg(unix)]
fn builder_line(project: &Path, fake: Fake) -> (String, tempfile::TempDir) {
    let bin = fake_codex(fake);
    // A refusal exits 1 (TSK-337); every other builder line exits 0.
    let (code, out) = preflight_status(project, bin.path());
    let rejected = matches!(fake, Fake::RejectsStartup | Fake::RejectsProject);
    assert_eq!(code, Some(i32::from(rejected)), "{out}");
    let line = out
        .lines()
        .find(|line| line.starts_with("codex builder profile:"))
        .unwrap_or_else(|| panic!("no builder line in\n{out}"))
        .to_string();
    (line, bin)
}

/// TSK-1003 AC-4: when the installed Codex refuses the builder overrides in
/// the run without a project config, the preflight says the seat will not
/// open.
#[cfg(unix)]
#[test]
fn a_codex_that_rejects_the_builder_profile_at_startup_is_reported() {
    let project = builder_project();
    let (line, _bin) = builder_line(project.path(), Fake::RejectsStartup);
    assert!(
        line.starts_with("codex builder profile: observed REJECTED by codex-cli 9.9.9: rejected without a project config: Error loading configuration: default_permissions requires a `[permissions]` table"),
        "{line}"
    );
    assert!(line.contains("will not open"), "{line}");
    assert!(!line.contains("in the project folder:"), "{line}");
}

/// TSK-1003 AC-4: a refusal in the project folder is a rejection too.
#[cfg(unix)]
#[test]
fn a_codex_that_rejects_the_project_profile_is_reported() {
    let project = builder_project();
    let (line, _bin) = builder_line(project.path(), Fake::RejectsProject);
    assert!(
        line.contains("REJECTED by codex-cli 9.9.9: rejected in the project folder: Error loading configuration: unknown permission key"),
        "{line}"
    );
}

/// TSK-1003 AC-4: acceptance with evidence that the project config loaded,
/// and the exact launch overrides in both runs, one outside the project.
#[cfg(unix)]
#[test]
fn a_codex_that_loads_the_project_config_is_reported_with_the_exact_overrides() {
    let project = builder_project();
    let (line, bin) = builder_line(project.path(), Fake::LoadsProject);
    assert!(
        line.starts_with("codex builder profile: observed codex-cli 9.9.9 accepts the builder launch overrides without a project config, as the TUI startup load reads them, and in the project folder; its feature table there is consistent with the project config loading (features.network_proxy = true only in the project folder), which does not prove which config layer supplied it"),
        "{line}"
    );
    assert!(
        line.contains("doctor cannot observe the whole boundary a running seat enforces; builder grants: observed effective from "),
        "{line}"
    );
    let log = std::fs::read_to_string(bin.path().join("codex.log")).unwrap();
    let loads: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("|features list"))
        .collect();
    assert_eq!(loads.len(), 2, "{log}");
    let expected = "features list -c permissions.cf-builder.description=\"CodeFlow builder seat\" -c permissions.cf-builder.filesystem={\"~/.codeflow/locks\"=\"write\",\"~/.codeflow/gate-runs\"=\"write\"} -c default_permissions=\"cf-builder\"";
    for load in &loads {
        assert!(load.ends_with(&format!("|{expected}")), "{load}");
    }
    let project_dir = std::fs::canonicalize(project.path()).unwrap();
    let dirs: Vec<std::path::PathBuf> = loads
        .iter()
        .map(|load| std::fs::canonicalize(load.split('|').next().unwrap()).unwrap_or_default())
        .collect();
    assert!(dirs.contains(&project_dir), "{log}");
    assert!(dirs.iter().any(|dir| *dir != project_dir), "{log}");
}

/// TSK-1003: exit 0 in the project folder without the project's setting
/// means Codex skipped the project config; that is said, never accepted.
#[cfg(unix)]
#[test]
fn a_codex_that_skips_the_project_config_is_reported() {
    let project = builder_project();
    let (line, _bin) = builder_line(project.path(), Fake::SkipsProject);
    assert!(
        line.contains("but Codex did not load the project config there (features.network_proxy shows false, the project sets true)"),
        "{line}"
    );
    assert!(!line.contains("consistent with"), "{line}");
}

/// TSK-1003: with no feature that tells the two runs apart, project loading
/// is unverified.
#[cfg(unix)]
#[test]
fn project_loading_without_a_distinguishing_feature_is_unverified() {
    let project = configured_project();
    let (line, _bin) = builder_line(project.path(), Fake::LoadsProject);
    assert!(
        line.contains("whether Codex loaded the project config is unverified"),
        "{line}"
    );
}

/// TSK-1003: exit 0 with no feature table, a missing codex-cli version and
/// an executable that cannot run are unavailable, never accepted.
#[cfg(unix)]
#[test]
fn unexpected_codex_answers_are_unavailable() {
    let project = builder_project();
    let (line, _bin) = builder_line(project.path(), Fake::PrintsNothing);
    assert!(
        line.starts_with("codex builder profile: unavailable: not checked without a project config: codex exited 0 but did not print a feature table"),
        "{line}"
    );
    let (line, _bin) = builder_line(project.path(), Fake::NoVersion);
    assert_eq!(
        line,
        "codex builder profile: unavailable: codex --version did not print a codex-cli version"
    );
    let bin = tempfile::tempdir().unwrap();
    write(bin.path(), "codex", "not run\n");
    let out = preflight(project.path(), bin.path());
    assert!(
        out.contains("codex builder profile: unavailable: codex --version did not run: "),
        "{out}"
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
    let bin = fake_codex(Fake::LoadsProject);
    let out = preflight(project.path(), bin.path());
    assert!(
        out.contains(
            "codex builder profile: unavailable: .codex/config.toml defines no cf-builder profile"
        ),
        "{out}"
    );
    assert!(!bin.path().join("codex.log").exists(), "codex ran");
}

/// TSK-1003: one feature showing the project's value never outweighs
/// another that contradicts it; the line says unverified.
#[cfg(unix)]
#[test]
fn contradictory_feature_evidence_is_unverified() {
    let project = configured_project();
    write(
        project.path(),
        ".codex/config.toml",
        "default_permissions = \"cf-guard\"\n[features]\nhooks = true\nnetwork_proxy = true\n[permissions.cf-guard]\n[permissions.cf-builder]\n",
    );
    let (line, _bin) = builder_line(project.path(), Fake::Contradicts);
    assert!(
        line.contains("whether Codex loaded the project config is unverified, because the features the project sets disagree or are missing"),
        "{line}"
    );
}

/// TSK-1003 (SEC-2): a `codex` planted in the project is never run, whether
/// `PATH` reaches it through a relative entry or an absolute one inside the
/// project; the next `codex` on `PATH` outside the project is used.
#[cfg(unix)]
#[test]
fn a_codex_inside_the_project_or_on_a_relative_path_entry_is_never_run() {
    use std::os::unix::fs::PermissionsExt;
    let project = builder_project();
    let marker = project.path().join("planted-ran");
    for dir in [project.path().to_path_buf(), project.path().join("bin")] {
        std::fs::create_dir_all(&dir).unwrap();
        let planted = dir.join("codex");
        std::fs::write(
            &planted,
            format!(
                "#!/bin/sh\ntouch '{}'\necho 'codex-cli 9.9.9'\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let outside = fake_codex(Fake::LoadsProject);
    let path = std::env::join_paths([
        std::path::PathBuf::from("."),
        std::path::PathBuf::new(),
        project.path().join("bin"),
        outside.path().to_path_buf(),
    ])
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(project.path())
        .env("PATH", path)
        .env("CODEFLOW_HOME", project.path().join("personal"))
        .args(["doctor", "--preflight"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(!marker.exists(), "a planted codex ran:\n{out}");
    let outside_dir = std::fs::canonicalize(outside.path()).unwrap();
    assert!(
        out.contains(&format!(
            "executable codex: observed on PATH at {}",
            outside_dir.join("codex").display()
        )),
        "{out}"
    );
    assert!(
        out.contains("codex builder profile: observed codex-cli 9.9.9"),
        "{out}"
    );
}

/// TSK-1003 (SEC-2): with only a planted `codex` on `PATH`, none is found.
#[cfg(unix)]
#[test]
fn only_a_planted_codex_means_codex_unavailable() {
    use std::os::unix::fs::PermissionsExt;
    let project = builder_project();
    let planted = project.path().join("codex");
    std::fs::write(&planted, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = preflight(project.path(), project.path());
    assert!(
        out.contains("codex builder profile: unavailable: codex not found on PATH"),
        "{out}"
    );
}

/// TSK-1003: output larger than the capture bound is never judged from its
/// tail. A table whose last 64 KiB look valid is unavailable, not accepted.
#[cfg(unix)]
#[test]
fn output_over_the_capture_bound_is_unavailable() {
    let project = builder_project();
    let (line, _bin) = builder_line(project.path(), Fake::PadsOutput);
    assert!(
        line.starts_with("codex builder profile: unavailable: not checked without a project config: it printed more than 64 KiB on a stream, so its output was not judged"),
        "{line}"
    );
}

/// TSK-337: the grant check runs `codex sandbox` once, from the builder
/// launch root, with every launch override but the selection, `-P
/// cf-builder`, and the permissions check of this `codeflow`; the real
/// check's pass is reported effective and the preflight exits 0.
#[cfg(unix)]
#[test]
fn the_builder_grants_are_checked_inside_codex_sandbox_from_the_launch_root() {
    let project = builder_project();
    let (line, bin) = builder_line(project.path(), Fake::LoadsProject);
    let project_dir = std::fs::canonicalize(project.path()).unwrap();
    assert!(
        line.ends_with(&format!(
            "; builder grants: observed effective from {} under `codex sandbox` with the launch overrides: the gate lock folders and the gate evidence folder are writable",
            project_dir.display()
        )),
        "{line}"
    );
    let log = std::fs::read_to_string(bin.path().join("codex.log")).unwrap();
    let runs: Vec<&str> = log.lines().filter(|l| l.contains("|sandbox ")).collect();
    assert_eq!(runs.len(), 1, "{log}");
    let (cwd, args) = runs[0].split_once('|').unwrap();
    assert_eq!(std::fs::canonicalize(cwd).unwrap(), project_dir);
    assert!(
        args.starts_with(&format!(
            "sandbox -C {} -c permissions.cf-builder.description=\"CodeFlow builder seat\" -c permissions.cf-builder.filesystem={{\"~/.codeflow/locks\"=\"write\",\"~/.codeflow/gate-runs\"=\"write\"}} -P cf-builder -- ",
            project_dir.display()
        )),
        "{args}"
    );
    assert!(args.ends_with(" doctor --check permissions"), "{args}");
}

/// TSK-337: a grant that is not effective from the launch root fails the
/// preflight (exit 1) and names the refused folder; a sandbox that runs
/// nothing is unavailable, never effective, and does not fail it.
#[cfg(unix)]
#[test]
fn a_builder_grant_that_is_not_effective_fails_the_preflight() {
    let project = builder_project();
    let bin = fake_codex_with(Fake::LoadsProject, Grants::Denied);
    let (code, out) = preflight_status(project.path(), bin.path());
    assert_eq!(code, Some(1), "{out}");
    let line = out
        .lines()
        .find(|line| line.starts_with("codex builder profile:"))
        .unwrap();
    assert!(
        line.contains("builder grants: observed NOT EFFECTIVE from ")
            && line.contains("/h/.codeflow/locks: Operation not permitted")
            && line.contains("cannot run `codeflow reserve` or a gate there"),
        "{line}"
    );

    let bin = fake_codex_with(Fake::LoadsProject, Grants::Aborts);
    let (code, out) = preflight_status(project.path(), bin.path());
    assert_eq!(code, Some(0), "{out}");
    assert!(
        out.contains("builder grants: unavailable: codex sandbox printed no permissions result (exit code 134); whether a builder seat can write the gate folders is unverified"),
        "{out}"
    );
}

/// TSK-337: a refusal of the overrides fails the preflight too, and no
/// sandbox run follows it.
#[cfg(unix)]
#[test]
fn a_rejected_builder_profile_fails_the_preflight_without_a_sandbox_run() {
    let project = builder_project();
    let bin = fake_codex(Fake::RejectsStartup);
    let (code, out) = preflight_status(project.path(), bin.path());
    assert_eq!(code, Some(1), "{out}");
    let log = std::fs::read_to_string(bin.path().join("codex.log")).unwrap();
    assert!(!log.contains("|sandbox "), "{log}");
}

/// TSK-337: in a linked worktree the probe runs from the main checkout,
/// where a builder seat starts, and reads that checkout's Codex config.
#[cfg(unix)]
#[test]
fn a_linked_worktree_is_probed_from_its_main_checkout() {
    let main = builder_project();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .current_dir(main.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.invalid",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "init",
    ]);
    git(&["worktree", "add", "-q", ".worktrees/task"]);
    let linked = main.path().join(".worktrees/task");
    // The worktree's own config defines no builder profile; the main
    // checkout's does, so the probe runs.
    write(&linked, ".codex/config.toml", "[permissions.cf-guard]\n");
    let bin = fake_codex(Fake::LoadsProject);
    let out = preflight(&linked, bin.path());
    let main_dir = std::fs::canonicalize(main.path()).unwrap();
    assert!(
        out.contains(&format!(
            "builder grants: observed effective from {}",
            main_dir.display()
        )),
        "{out}"
    );
    let log = std::fs::read_to_string(bin.path().join("codex.log")).unwrap();
    let linked_dir = std::fs::canonicalize(&linked).unwrap();
    let dirs: Vec<std::path::PathBuf> = log
        .lines()
        .map(|run| std::fs::canonicalize(run.split('|').next().unwrap()).unwrap_or_default())
        .collect();
    // The startup load runs in an empty temporary folder; the project load
    // and the sandbox run in the main checkout, never in the worktree.
    assert_eq!(dirs.len(), 3, "{log}");
    assert_eq!(
        dirs.iter().filter(|dir| **dir == main_dir).count(),
        2,
        "{log}"
    );
    assert!(!dirs.contains(&linked_dir), "{log}");
}
