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
    land_with(&Fixture {
        file,
        contents,
        line,
        record_checks,
        waive,
        ..Fixture::default()
    })
}

/// What a landing fixture varies. The default declares docs and task Markdown
/// as records, with a passing quick target and a conventional commit.
#[derive(Clone, Copy)]
struct Fixture<'a> {
    file: &'a str,
    contents: &'a str,
    line: &'a str,
    record_checks: &'a [&'a str],
    waive: bool,
    policy: &'a str,
    quick: &'a str,
    full: &'a str,
    subject: &'a str,
    /// Empty `execution.record_checks` in the integrator's worktree, hidden
    /// from the dirty-tree check by skip-worktree.
    hide_checks: bool,
}

const DECLARED: &str =
    r#"{"git":{"record_paths":["docs/**/*.md","project-management/tasks/**/*.md"]}}"#;

impl Default for Fixture<'_> {
    fn default() -> Self {
        Self {
            file: "docs/guide.md",
            contents: "A new guide\n",
            line: LINE,
            record_checks: &[],
            waive: false,
            policy: DECLARED,
            quick: "echo quick > quick-ran.txt",
            full: "echo full > full-ran.txt && exit 19",
            subject: "docs: explain the change",
            hide_checks: false,
        }
    }
}

/// Merge a line commit that waives the secret-shaped fixture into the task.
fn waive_fixture(root: &Path, file: &str, line: &str, home_path: &Path) {
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

fn land_with(fixture: &Fixture<'_>) -> Landing {
    let Fixture {
        file,
        contents,
        line,
        record_checks,
        waive,
        ..
    } = *fixture;
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let (root, home_path) = (dir.path(), home.path());
    git(root, &["init", "-q", "-b", "main"], home_path);
    std::fs::create_dir(root.join(".codeflow")).unwrap();
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
    std::fs::write(root.join(".codeflow/policy.json"), fixture.policy).unwrap();
    let config = serde_json::json!({
        "schema_version": "1.0",
        "execution": {"record_checks": record_checks},
        "targets": [
            {"name": "fixture", "runner": "custom", "modes": {
                "quick": {"command": fixture.quick},
                "full": {"command": fixture.full}
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
    if let Some(folder) = Path::new(file).parent() {
        std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    std::fs::write(root.join(file), contents).unwrap();
    git(root, &["add", file], home_path);
    git(root, &["commit", "-qm", fixture.subject], home_path);
    if waive {
        waive_fixture(root, file, line, home_path);
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
    if fixture.hide_checks {
        let config = ".codeflow/test-config.json";
        git(
            &linked,
            &["update-index", "--skip-worktree", config],
            home_path,
        );
        let text = std::fs::read_to_string(linked.join(config)).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["execution"]["record_checks"] = serde_json::json!([]);
        std::fs::write(linked.join(config), value.to_string()).unwrap();
    }
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
    assert!(
        record["reason"]
            .as_str()
            .unwrap()
            .contains("declared regular Markdown"),
        "{record}"
    );
    assert_eq!(record["base"], records.before);
    assert_eq!(record["checks_ran"].as_array().unwrap().len(), 4);

    let code = land("source.rs", "A new guide\n", LINE, &[], false);
    assert!(!code.succeeded, "{}", code.said);
    assert_eq!(code.before, code.after);
    assert!(code.ran("full-ran.txt"), "{}", code.said);
    assert!(!code.ran("quick-ran.txt"));
}

/// A code landing that passes names its class and reason in the record.
#[test]
fn a_code_landing_records_its_class_and_reason() {
    let landed = land_with(&Fixture {
        file: "source.rs",
        full: "echo full > full-ran.txt",
        ..Fixture::default()
    });
    assert!(landed.succeeded, "{}", landed.said);
    assert!(landed.ran("full-ran.txt"), "{}", landed.said);
    assert!(!landed.ran("quick-ran.txt"), "{}", landed.said);
    assert!(
        landed
            .said
            .contains("landing class: code (source.rs is outside"),
        "{}",
        landed.said
    );
    let record = landed.record();
    assert_eq!(record["class"], "code");
    assert!(
        record["reason"].as_str().unwrap().contains("source.rs"),
        "{record}"
    );
}

/// TSK-1306: no declaration means the full gate, through the real CLI.
#[test]
fn a_project_with_no_declaration_runs_the_full_gate() {
    let landed = land_with(&Fixture {
        policy: "{}",
        ..Fixture::default()
    });
    assert!(!landed.succeeded, "{}", landed.said);
    assert_eq!(landed.before, landed.after);
    assert!(landed.ran("full-ran.txt"), "{}", landed.said);
    assert!(!landed.ran("quick-ran.txt"), "{}", landed.said);
}

/// Each required check that fails refuses the landing and leaves the line.
#[test]
fn each_required_check_that_fails_refuses_the_landing() {
    // `validate --docs`: a task record it cannot accept.
    let invalid = land_with(&Fixture {
        file: "project-management/tasks/TSK-001.md",
        contents: "---\nid: not-a-task-id\n---\n",
        ..Fixture::default()
    });
    assert!(!invalid.succeeded, "{}", invalid.said);
    assert_eq!(invalid.before, invalid.after);
    assert!(
        invalid.said.contains("validate --docs failed"),
        "{}",
        invalid.said
    );
    assert!(!invalid.ran("quick-ran.txt"), "{}", invalid.said);

    // `ci` on the range: a commit subject the policy refuses.
    let ci = land_with(&Fixture {
        subject: "not a conventional subject",
        ..Fixture::default()
    });
    assert!(!ci.succeeded, "{}", ci.said);
    assert_eq!(ci.before, ci.after);
    assert!(ci.said.contains("ci --base"), "{}", ci.said);
    assert!(!ci.ran("quick-ran.txt"), "{}", ci.said);

    // The quick gate.
    let quick = land_with(&Fixture {
        quick: "echo quick > quick-ran.txt && exit 7",
        ..Fixture::default()
    });
    assert!(!quick.succeeded, "{}", quick.said);
    assert_eq!(quick.before, quick.after);
    assert!(quick.ran("quick-ran.txt"), "{}", quick.said);
    assert!(!quick.ran("full-ran.txt"), "{}", quick.said);
}

/// The record checks come from the tested commit: a worktree edit hidden by
/// skip-worktree cannot shorten the list.
#[test]
fn a_hidden_worktree_edit_cannot_shorten_the_record_checks() {
    let landed = land_with(&Fixture {
        record_checks: &["portal"],
        hide_checks: true,
        ..Fixture::default()
    });
    assert!(landed.succeeded, "{}", landed.said);
    assert!(landed.ran("portal-ran.txt"), "{}", landed.said);
}

/// `codeflow ci` output for a range, with the line's fetched tip at `tip`.
fn ci_output(root: &Path, home: &Path, tip: &str, base: &str) -> String {
    git(
        root,
        &["update-ref", &format!("refs/remotes/origin/{LINE}"), tip],
        home,
    );
    text(&run(
        root,
        env!("CARGO_BIN_EXE_codeflow"),
        &[
            "ci",
            "--base",
            base,
            "--head",
            "HEAD",
            "--branch",
            "feat/reviewed",
            "--into",
            LINE,
        ],
        home,
    ))
}

/// A repository whose `feat/reviewed` adds each `(file, contents)` as its own
/// commit, from a tip that declares docs Markdown as records.
fn candidate(home: &Path, files: &[(&str, &str)]) -> (tempfile::TempDir, String, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"], home);
    std::fs::create_dir_all(root.join(".codeflow")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join(".codeflow/policy.json"), DECLARED).unwrap();
    git(root, &["add", "."], home);
    git(root, &["commit", "-qm", "test: establish fixture"], home);
    let tip = git(root, &["rev-parse", "HEAD"], home);
    git(root, &["checkout", "-qb", "feat/reviewed"], home);
    let mut commits = Vec::new();
    for (file, contents) in files {
        std::fs::write(root.join(file), contents).unwrap();
        git(root, &["add", file], home);
        git(root, &["commit", "-qm", "docs: explain the change"], home);
        commits.push(git(root, &["rev-parse", "HEAD"], home));
    }
    (dir, tip, commits)
}

#[test]
fn ci_names_the_class_and_the_reason_for_both_classes() {
    let home = tempfile::tempdir().unwrap();
    let (records, tip, _) = candidate(home.path(), &[("docs/guide.md", "A guide\n")]);
    let said = ci_output(records.path(), home.path(), &tip, &tip);
    assert!(
        said.contains("landing class: records-only; required test gate: quick; every changed path"),
        "{said}"
    );

    let (code, tip, _) = candidate(home.path(), &[("source.rs", "code\n")]);
    let said = ci_output(code.path(), home.path(), &tip, &tip);
    assert!(
        said.contains("landing class: code; required test gate: full; source.rs is outside"),
        "{said}"
    );
}

#[test]
fn ci_reads_a_base_that_is_not_the_fetched_tip_as_code() {
    let home = tempfile::tempdir().unwrap();
    let (dir, tip, commits) = candidate(
        home.path(),
        &[("source.rs", "code\n"), ("docs/guide.md", "A guide\n")],
    );
    let said = ci_output(dir.path(), home.path(), &tip, &commits[0]);
    assert!(said.contains("landing class: code"), "{said}");
    assert!(said.contains("is not the fetched tip"), "{said}");
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
