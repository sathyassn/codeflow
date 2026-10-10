//! End-to-end tests for the policy discoverability surface:
//! `codeflow policy <explain|show>` and the loud invalid-policy exits of
//! `codeflow ci` and `codeflow validate`. (The commit-msg hook's loud exit is
//! covered with the other hook contracts in `hooks_cli.rs`.)
//!
//! Each test runs the real binary (`CARGO_BIN_EXE_codeflow`) in a tempdir git
//! repo, pinning the exit-code and message contracts a consumer of the binary
//! (no source) relies on.

use std::path::Path;
use std::process::{Command, Output};

use codeflow_core::hooks::policy_schema::schema;

/// Shared isolated `CODEFLOW_HOME` (the `hooks_cli.rs` pattern): keeps the
/// per-command registry touch out of the developer's real `~/.codeflow`.
fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn codeflow() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    cmd.env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    codeflow()
        .args(args)
        .current_dir(dir)
        .output()
        .expect("binary runs")
}

fn init_repo(dir: &Path) {
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}");
    };
    git(&["init", "-b", "feat/x"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "chore: init"]);
}

fn write_policy(dir: &Path, json: &str) {
    let cf = dir.join(".codeflow");
    std::fs::create_dir_all(&cf).unwrap();
    std::fs::write(cf.join("policy.json"), json).unwrap();
}

// ---------------------------------------------------------------------------
// codeflow policy explain
// ---------------------------------------------------------------------------

#[test]
fn policy_explain_renders_every_key_path() {
    // The whole point of explain: the complete schema, from the binary alone.
    // Every registered leaf path must appear, with the shared level legend.
    let dir = tempfile::tempdir().unwrap();
    let out = run_in(dir.path(), &["policy", "explain"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    for spec in schema() {
        assert!(
            stdout.contains(spec.path),
            "explain must list {}",
            spec.path
        );
    }
    assert!(stdout.contains("off | warn | allow | block"), "{stdout}");
    // The allow≈off trap is stated once, in the legend.
    assert!(stdout.contains("BOTH inactive"), "{stdout}");
    // Defaults render from the real Default impl.
    assert!(stdout.contains(r#"default: ["main","master"]"#), "{stdout}");
}

// ---------------------------------------------------------------------------
// codeflow policy show
// ---------------------------------------------------------------------------

#[test]
fn policy_show_absent_file_is_all_defaults_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    let out = run_in(dir.path(), &["policy", "show"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("no .codeflow/policy.json"), "{stdout}");
    assert!(
        stdout.contains("default  git.commit_format               block"),
        "{stdout}"
    );
    assert!(
        !stdout
            .lines()
            .any(|line| line.trim_start().starts_with("project ")),
        "{stdout}"
    );
}

#[test]
fn policy_show_reports_project_values_and_defaults() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    write_policy(
        dir.path(),
        r#"{"schema_version":1,"git":{"commit_format":"warn"}}"#,
    );
    let out = run_in(dir.path(), &["policy", "show"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("project  git.commit_format               warn"),
        "{stdout}"
    );
    // A key the file does not set stays at (and is labeled) the default.
    assert!(
        stdout.contains("default  git.push_to_protected           block"),
        "{stdout}"
    );
}

#[test]
fn policy_show_flags_invalid_values_and_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    write_policy(dir.path(), r#"{"git":{"commit_ticket_required":"worn"}}"#);
    let out = run_in(dir.path(), &["policy", "show"]);
    assert_eq!(out.status.code(), Some(1), "an invalid file must exit 1");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stdout.contains("INVALID  git.commit_ticket_required"),
        "{stdout}"
    );
    // The consequence is stated loudly: enforcement refuses the whole file
    // (issue 79 AC-10); it never falls back to the built-in defaults.
    assert!(
        stdout.contains("DOES NOT PARSE — enforcement refuses this policy"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("built-in defaults for EVERY key"),
        "{stdout}"
    );
    assert!(stderr.contains("off, warn, allow, block"), "{stderr}");
}

// ---------------------------------------------------------------------------
// loud exits: codeflow ci (2) and codeflow validate (1)
// ---------------------------------------------------------------------------

#[test]
fn ci_exits_two_on_invalid_policy_naming_the_key() {
    // An invalid policy cannot verify the consumer's intent — exit 2 (the
    // could-not-verify code), never a silent pass against built-in defaults.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    write_policy(dir.path(), r#"{"git":{"dep_audit":"blok"}}"#);
    let out = run_in(dir.path(), &["ci", "--base", "HEAD", "--head", "HEAD"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("git.dep_audit"), "{stderr}");
    assert!(stderr.contains("'blok'"), "{stderr}");
    assert!(stderr.contains("nothing was verified"), "{stderr}");
}

#[test]
fn validate_exits_one_on_invalid_policy_and_zero_when_clean() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    // Unknown key: the enforcement loader would silently ignore it — validate
    // must not.
    write_policy(dir.path(), r#"{"git":{"comit_format":"warn"}}"#);
    let out = run_in(dir.path(), &["validate"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown key git.comit_format"), "{stderr}");

    // Fixing the typo makes the same repo validate clean.
    write_policy(dir.path(), r#"{"git":{"commit_format":"warn"}}"#);
    let out = run_in(dir.path(), &["validate"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(".codeflow/policy.json clean"), "{stdout}");
}
