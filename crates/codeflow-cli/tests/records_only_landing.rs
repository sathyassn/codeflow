//! Exercise the real CLI's selected gate and its local landing evidence.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(root: &Path, program: &str, args: &[&str], home: &Path) -> Output {
    Command::new(program)
        .args(args)
        .current_dir(root)
        .env("CODEFLOW_HOME", home)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.test")
        .env("GIT_COMMITTER_EMAIL", "test@example.test")
        .env("CLAUDECODE", "")
        .env("CODEX_SESSION_ID", "")
        .env("CODEX_THREAD_ID", "")
        .env("GROK_SESSION_ID", "")
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn git(root: &Path, args: &[&str], home: &Path) -> String {
    let output = run(root, "git", args, home);
    assert!(output.status.success(), "git {args:?}: {}", text(&output));
    String::from_utf8(output.stdout).unwrap().trim().into()
}

const LINE: &str = "integration/landing";

/// One landing through the real CLI and what it left behind.
struct Landing {
    succeeded: bool,
    said: String,
    before: String,
    after: String,
    linked: PathBuf,
    root: tempfile::TempDir,
    _home: tempfile::TempDir,
}

impl Landing {
    fn ran(&self, marker: &str) -> bool {
        self.linked.join(marker).exists()
    }

    fn record(&self) -> serde_json::Value {
        let path = self
            .root
            .path()
            .join(format!(".git/codeflow/landings/{}.json", self.after));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }
}

/// Land a branch that writes `contents` to `file` into `line` with
/// `integrate --merge`. `record_checks` becomes the fixture's
/// `execution.record_checks`; `waive` first lands, on the line, a
/// fingerprint for line 1 of the branch commit and merges the line back in.
fn land(file: &str, contents: &str, line: &str, record_checks: &[&str], waive: bool) -> Landing {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let (root, home_path) = (dir.path(), home.path());
    git(root, &["init", "-q", "-b", "main"], home_path);
    std::fs::create_dir(root.join(".codeflow")).unwrap();
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
    std::fs::write(root.join(".codeflow/policy.json"), "{}").unwrap();
    let config = serde_json::json!({
        "schema_version": "1.0",
        "execution": {"record_checks": record_checks},
        "targets": [
            {"name": "fixture", "runner": "custom", "modes": {
                "quick": {"command": "echo quick > quick-ran.txt"},
                "full": {"command": "echo full > full-ran.txt && exit 19"}
            }},
            {"name": "portal", "runner": "custom", "modes": {
                "full": {"command": "echo portal > portal-ran.txt"}
            }}
        ]
    });
    std::fs::write(root.join(".codeflow/test-config.json"), config.to_string()).unwrap();
    git(root, &["add", "."], home_path);
    git(
        root,
        &["commit", "-qm", "test: establish fixture"],
        home_path,
    );
    git(root, &["branch", line], home_path);
    git(root, &["checkout", "-qb", "feat/reviewed"], home_path);
    std::fs::write(root.join(file), contents).unwrap();
    git(root, &["add", file], home_path);
    git(
        root,
        &["commit", "-qm", "docs: explain the change"],
        home_path,
    );
    if waive {
        let commit = git(root, &["rev-parse", "HEAD"], home_path);
        git(root, &["checkout", "-q", line], home_path);
        std::fs::write(
            root.join(".gitleaksignore"),
            format!("{commit}:{file}:github-pat:1\n"),
        )
        .unwrap();
        git(root, &["add", ".gitleaksignore"], home_path);
        git(
            root,
            &["commit", "-qm", "chore: waive a reviewed fixture"],
            home_path,
        );
        git(root, &["checkout", "-q", "feat/reviewed"], home_path);
        git(root, &["merge", "-q", "--no-edit", line], home_path);
    }
    let linked = root.join(".worktrees/integrator");
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "chore/integrator",
            linked.to_str().unwrap(),
            "main",
        ],
        home_path,
    );
    let before = git(root, &["rev-parse", line], home_path);
    let output = run(
        &linked,
        env!("CARGO_BIN_EXE_codeflow"),
        &["integrate", "feat/reviewed", "--into", line, "--merge"],
        home_path,
    );
    let after = git(root, &["rev-parse", line], home_path);
    Landing {
        succeeded: output.status.success(),
        said: text(&output),
        before,
        after,
        linked,
        root: dir,
        _home: home,
    }
}

fn token() -> String {
    format!("ghp_{}", "a".repeat(36))
}

#[test]
fn records_run_quick_and_record_checks_while_code_requires_full() {
    let records = land("docs/guide.md", "A new guide\n", LINE, &[], false);
    assert!(records.succeeded, "{}", records.said);
    assert_ne!(records.before, records.after);
    assert!(records.ran("quick-ran.txt"));
    assert!(!records.ran("full-ran.txt"));
    assert!(!records.ran("portal-ran.txt"));
    assert!(
        records.said.contains("landing class: records-only"),
        "{}",
        records.said
    );
    let record = records.record();
    assert_eq!(record["class"], "records-only");
    assert_eq!(record["base"], records.before);
    assert_eq!(record["checks_ran"].as_array().unwrap().len(), 4);

    let code = land("source.rs", "A new guide\n", LINE, &[], false);
    assert!(!code.succeeded, "{}", code.said);
    assert_eq!(code.before, code.after);
    assert!(code.ran("full-ran.txt"), "{}", code.said);
    assert!(!code.ran("quick-ran.txt"));
}

#[test]
fn a_release_line_keeps_the_full_gate_for_records() {
    let release = land(
        "docs/guide.md",
        "A new guide\n",
        "integration/release-4.0",
        &[],
        false,
    );
    assert!(!release.succeeded, "{}", release.said);
    assert_eq!(release.before, release.after);
    assert!(release.ran("full-ran.txt"), "{}", release.said);
    assert!(!release.ran("quick-ran.txt"));
}

#[test]
fn declared_record_checks_run_and_a_missing_one_refuses() {
    let checked = land("docs/guide.md", "A new guide\n", LINE, &["portal"], false);
    assert!(checked.succeeded, "{}", checked.said);
    assert!(checked.ran("portal-ran.txt"), "{}", checked.said);
    assert!(checked.ran("quick-ran.txt"));
    let ran = checked.record()["checks_ran"].to_string();
    assert!(ran.contains("test --mode full --only portal"), "{ran}");

    let missing = land("docs/guide.md", "A new guide\n", LINE, &["absent"], false);
    assert!(!missing.succeeded, "{}", missing.said);
    assert_eq!(missing.before, missing.after);
    assert!(missing.said.contains("absent"), "{}", missing.said);
    assert!(!missing.ran("quick-ran.txt"));
}

#[test]
fn the_range_secret_scan_names_the_path_and_honours_a_line_tip_fingerprint() {
    let secret = land("docs/guide.md", &token(), LINE, &[], false);
    assert!(!secret.succeeded, "{}", secret.said);
    assert_eq!(secret.before, secret.after);
    assert!(secret.said.contains("secret scan found"), "{}", secret.said);
    assert!(secret.said.contains("docs/guide.md:1"), "{}", secret.said);
    assert!(secret.said.contains(".gitleaksignore"), "{}", secret.said);
    assert!(!secret.said.contains(&token()), "{}", secret.said);
    assert!(!secret.ran("quick-ran.txt"));

    let waived = land("docs/guide.md", &token(), LINE, &[], true);
    assert!(waived.succeeded, "{}", waived.said);
    assert_ne!(waived.before, waived.after);
}

#[test]
fn codeflow_declares_its_docs_checks_as_record_checks() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.codeflow/test-config.json");
    let config = codeflow_core::testing::config::load_test_config(&path).unwrap();
    for name in ["docs-validation", "docs-portal"] {
        assert!(
            config
                .execution
                .record_checks
                .iter()
                .any(|check| check == name),
            "{name} is missing from execution.record_checks"
        );
        assert!(
            config
                .targets
                .iter()
                .any(|target| target.name == name && target.modes.contains_key("full")),
            "{name} has no full mode"
        );
    }
}
