//! Selection through the real CLI, real Git trees and executed fixture checks.
use super::{git, stderr};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn command(dir: &Path, home: &Path, args: &[&str]) -> std::process::Command {
    let mut cmd = super::command(dir, home, args);
    cmd.env_remove("CODEFLOW_HOME")
        .env("HOME", home)
        .env("USERPROFILE", home);
    cmd
}

pub(super) fn run(dir: &Path, home: &Path, args: &[&str]) -> std::process::Output {
    command(dir, home, args).output().unwrap()
}

static EVIDENCE_FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Fixture {
    _account_store: std::sync::MutexGuard<'static, ()>,
    root: tempfile::TempDir,
    home: PathBuf,
    isolated_home: tempfile::TempDir,
    record: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // This directory is keyed by this fixture's unique repository path.
        let directory = self.record.parent().unwrap().parent().unwrap();
        std::fs::remove_dir_all(directory).expect("remove this fixture's durable evidence");
    }
}

impl Fixture {
    fn new(change: impl FnOnce(&mut Value)) -> Self {
        Self::with_environment(change, None)
    }

    fn with_environment(change: impl FnOnce(&mut Value), marker: Option<&str>) -> Self {
        let account_store = EVIDENCE_FIXTURES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let root = tempfile::tempdir().unwrap();
        let home = PathBuf::from(
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .expect("test account home"),
        );
        let dir = root.path();
        git(dir, &["init", "-q", "-b", "integration/checks"]);
        // Integration creates commits in its own process, without the git helper
        // environment or a runner-wide identity. Linked worktrees share this config.
        git(dir, &["config", "user.name", "Test"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        for path in [".codeflow", "src", "project-management/tasks"] {
            std::fs::create_dir_all(dir.join(path)).unwrap();
        }
        std::fs::write(dir.join(".codeflow/policy.json"), r#"{"schema_version":1}"#).unwrap();
        let python = ["python3", "python"]
            .into_iter()
            .find(|name| {
                std::process::Command::new(name)
                    .arg("--version")
                    .output()
                    .is_ok_and(|output| output.status.success())
            })
            .expect("Python is required for the real fixture checks");
        let check_command = |path: &str| format!("\"{python}\" check.py {path}");
        let audit = json!({"tools":{"python":[python,"probe.py","tool.version"]},
            "environment":{}, "environment_complete":true,
            "membership":[python,"probe.py","membership"],"covers_modes":["quick"]});
        let mut config = json!({"schema_version":"1.0", "execution":{"run_everything":["crates/**"]}, "targets":[
            {"name":"rust-check","runner":"custom","input_class":"content", "evidence":audit,
             "narrow":["src/**","check.py","probe.py"],
             "modes":{"full":{"command":check_command("src/input")},"quick":{"command":check_command("src/input")}}},
            {"name":"record-check","runner":"custom","input_class":"content", "evidence":audit,
             "narrow":["project-management/**","check.py","probe.py"],
             "modes":{"full":{"command":check_command("project-management/tasks/TSK-001.md")},"quick":{"command":check_command("project-management/tasks/TSK-001.md")}}}
        ]});
        change(&mut config);
        std::fs::write(
            dir.join(".codeflow/test-config.json"),
            serde_json::to_vec_pretty(&config).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join(".gitignore"),
            "target/\ntool.version\nmembership\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/input"), "good\n").unwrap();
        std::fs::write(dir.join("src/extra"), "good\n").unwrap();
        std::fs::write(
            dir.join("project-management/tasks/TSK-001.md"),
            "---\nid: TSK-001\nuid: 7ac5fb7c-f6b2-4bd6-ad39-b8741dd1d31d\ntitle: Selection fixture\nepic_id: null\nstandalone_reason: Gate selection fixture\nstatus: todo\nwork_type: test\nspecs: []\ndepends_on: []\nintegration_target: integration/checks\ncreated: 2026-10-19\n---\n\n# Selection fixture\n\n## Description\n\nVerify the gate against `src/input`.\n\n## Deliverables\n\n- check: `src/input`\n\n## Acceptance Criteria\n\n- AC-1 (journey) The check reads its declared input.\n",
        )
        .unwrap();
        std::fs::write(dir.join("check.py"), "import pathlib,sys,os\nassert 'broken' not in pathlib.Path(sys.argv[1]).read_text()\nprint('checked '+sys.argv[1])\nif sys.argv[1] == 'src/input': pathlib.Path('target/check-output').write_text(pathlib.Path(sys.argv[1]).read_text())\nif 'SELECTION_MARKER' in os.environ:\n print('Summary '+os.environ['SELECTION_MARKER'])\n print('Summary '+os.environ['SELECTION_MARKER'],file=sys.stderr)\n pathlib.Path('target/secret-report.xml').write_text('<testsuites><testsuite><testcase name=\"'+os.environ['SELECTION_MARKER']+'\"/></testsuite></testsuites>')\n").unwrap();
        std::fs::write(
            dir.join("probe.py"),
            "import pathlib,sys\nprint(pathlib.Path(sys.argv[1]).read_text())\n",
        )
        .unwrap();
        std::fs::write(dir.join("tool.version"), "1.0\n").unwrap();
        std::fs::write(dir.join("membership"), "checks_input\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "test: candidate"]);
        // Seed real full evidence through integration's gate engine. A parent
        // full coverage gate may already hold the CLI's account-wide lock.
        git(dir, &["branch", "fixture-base"]);
        let mut command = command(
            dir,
            home.as_path(),
            &["integrate", "integration/checks", "--into", "fixture-base"],
        );
        if let Some(marker) = marker {
            command.env("SELECTION_MARKER", marker);
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        if let Some(marker) = marker {
            assert!(!stderr(&output).contains(marker), "secret in gate stderr");
            assert!(
                !String::from_utf8_lossy(&output.stdout).contains(marker),
                "secret in gate stdout"
            );
        }
        let record = PathBuf::from(
            stderr(&output)
                .lines()
                .find_map(|line| line.strip_prefix("[codeflow test] durable artifact: "))
                .unwrap(),
        );
        let artifact: Value = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
        assert_eq!(artifact["complete"], true);
        assert_eq!(artifact["clean"], true);
        assert!(artifact["git_tree"].as_str().is_some());
        // The line has another commit with exactly the gated candidate's tree.
        git(
            dir,
            &["commit", "--allow-empty", "-qm", "test: merge candidate"],
        );
        git(dir, &["switch", "-c", "task/TSK-001-selection"]);
        Self {
            _account_store: account_store,
            root,
            home,
            isolated_home: tempfile::tempdir().unwrap(),
            record,
        }
    }

    fn full(&self, args: &[&str]) -> std::process::Output {
        // These CLI runs test execution and output, not account selection credit.
        run(self.root.path(), self.isolated_home.path(), args)
    }

    fn integration_root(&self, merge: bool) -> PathBuf {
        if !merge {
            return self.root.path().to_owned();
        }
        let linked = self.root.path().join(".worktrees/selection");
        git(self.root.path(), &["switch", "integration/checks"]);
        git(
            self.root.path(),
            &[
                "worktree",
                "add",
                linked.to_str().unwrap(),
                "task/TSK-001-selection",
            ],
        );
        linked
    }

    fn quick(&self) -> (String, Value) {
        let output = run(
            self.root.path(),
            self.home.as_path(),
            &["test", "--mode", "quick"],
        );
        assert!(output.status.success(), "{}", stderr(&output));
        let text = stderr(&output);
        let path = text
            .lines()
            .find_map(|line| line.strip_prefix("[codeflow test] artifact: "))
            .unwrap();
        let artifact = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        (text, artifact)
    }

    fn alter_record(&self, change: impl FnOnce(&mut Value)) {
        let mut record = serde_json::from_slice(&std::fs::read(&self.record).unwrap()).unwrap();
        change(&mut record);
        std::fs::write(&self.record, serde_json::to_vec(&record).unwrap()).unwrap();
    }

    fn selected(&self, reason: &str) {
        let (text, record) = self.quick();
        assert!(text.contains("selected rust-check"), "{text}");
        assert!(
            text.contains(reason) || (!cfg!(unix) && text.contains("account home unavailable")),
            "{text}"
        );
        assert!(record["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "rust-check"));
    }
}

#[test]
fn equal_tree_candidate_covers_declared_quick_command_and_all_still_executes() {
    let fixture = Fixture::new(|_| {});
    let (text, record) = fixture.quick();
    assert_eq!(record["complete"], false);
    if cfg!(unix) {
        assert!(text.contains("skipped rust-check"), "{text}");
        assert!(record["results"].as_array().unwrap().is_empty());
        assert_eq!(
            record["selection"]["skipped"],
            json!(["record-check", "rust-check"])
        );
    } else {
        assert!(text.contains("account home unavailable"), "{text}");
        assert_eq!(record["results"].as_array().unwrap().len(), 2);
        assert_eq!(record["selection"]["skipped"], json!([]));
    }
    let output = fixture.full(&["test", "--all"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("starting target 'rust-check'"));
    assert!(stderr(&output).contains("starting target 'record-check'"));
}

#[test]
fn completion_record_changes_tree_and_runs_its_reader() {
    let fixture = Fixture::new(|_| {});
    let path = fixture
        .root
        .path()
        .join("project-management/tasks/TSK-001.md");
    let mut record = std::fs::read_to_string(&path).unwrap();
    record.push_str("\nCompletion evidence.\n");
    std::fs::write(path, record).unwrap();
    git(
        fixture.root.path(),
        &["add", "project-management/tasks/TSK-001.md"],
    );
    git(fixture.root.path(), &["commit", "-qm", "test: completion"]);
    let (text, record) = fixture.quick();
    let base: Value = serde_json::from_slice(&std::fs::read(&fixture.record).unwrap()).unwrap();
    assert_ne!(record["git_tree"], base["git_tree"]);
    assert!(text.contains("selected record-check"), "{text}");
    assert!(
        text.contains(if cfg!(unix) {
            "skipped rust-check"
        } else {
            "selected rust-check"
        }),
        "{text}"
    );
    assert_eq!(
        record["results"].as_array().unwrap().len(),
        if cfg!(unix) { 1 } else { 2 }
    );
}

macro_rules! class_runs {
    ($name:ident, $class:expr) => {
        #[test]
        fn $name() {
            let fixture = Fixture::new(|c| c["targets"][0]["input_class"] = json!($class));
            fixture.selected("input class");
        }
    };
}
class_runs!(history_always_runs, "history");
class_runs!(remote_always_runs, "remote");
class_runs!(time_always_runs, "time");
class_runs!(external_always_runs, "external");
class_runs!(unclassified_always_runs, Option::<String>::None);

macro_rules! bad_record {
    ($name:ident, $reason:expr, $change:expr) => {
        #[test]
        fn $name() {
            let fixture = Fixture::new(|_| {});
            fixture.alter_record($change);
            fixture.selected($reason);
        }
    };
}
bad_record!(
    changed_platform_runs,
    "platform changed",
    |r: &mut Value| {
        r["target_evidence"][0]["before"]["identity"]["platform"] = json!("another-platform");
        r["target_evidence"][0]["after"]["identity"]["platform"] = json!("another-platform");
    }
);
bad_record!(
    changed_architecture_runs,
    "architecture changed",
    |r: &mut Value| {
        r["target_evidence"][0]["before"]["identity"]["architecture"] =
            json!("another-architecture");
        r["target_evidence"][0]["after"]["identity"]["architecture"] =
            json!("another-architecture");
    }
);
bad_record!(
    inputs_changed_during_run_are_rejected,
    "changed during qualifying run",
    |r: &mut Value| {
        r["target_evidence"][0]["after"]["identity"]["inputs"]["src/input"] = json!("changed");
    }
);
bad_record!(
    missing_evidence_runs,
    "missing or corrupt target evidence",
    |r: &mut Value| {
        r["target_evidence"][0]["before"]["identity"]
            .as_object_mut()
            .unwrap()
            .remove("membership");
    }
);
bad_record!(
    unknown_target_evidence_runs,
    "unknown declared target",
    |r: &mut Value| {
        r["target_evidence"][0]["name"] = json!("unknown-check");
    }
);
bad_record!(
    failed_candidate_runs,
    "failed qualifying run",
    |r: &mut Value| r["passed"] = json!(false)
);
bad_record!(
    partial_candidate_runs,
    "partial qualifying run",
    |r: &mut Value| r["complete"] = json!(false)
);
bad_record!(
    dirty_candidate_runs,
    "dirty qualifying run",
    |r: &mut Value| r["clean"] = json!(false)
);
bad_record!(
    legacy_candidate_runs,
    "no qualifying run for the base tree",
    |r: &mut Value| {
        r.as_object_mut().unwrap().remove("git_tree");
    }
);
bad_record!(
    changed_config_runs,
    "config digest changed",
    |r: &mut Value| r["config_digest"] = json!("old-config")
);
bad_record!(
    different_recorded_command_runs,
    "mode-specific command differs",
    |r: &mut Value| r["target_evidence"][0]["command"] = json!("echo different")
);

#[test]
fn tool_version_change_without_tracked_file_change_runs() {
    let fixture = Fixture::new(|_| {});
    std::fs::write(fixture.root.path().join("tool.version"), "2.0\n").unwrap();
    fixture.selected("tool identity changed");
}

#[test]
fn test_membership_change_runs() {
    let fixture = Fixture::new(|_| {});
    std::fs::write(
        fixture.root.path().join("membership"),
        "checks_input\nchecks_other\n",
    )
    .unwrap();
    fixture.selected("test membership changed");
}

#[test]
fn an_empty_tool_probe_is_missing_evidence() {
    let fixture = Fixture::new(|_| {});
    std::fs::write(fixture.root.path().join("tool.version"), "").unwrap();
    fixture.selected("probe returned no evidence");
}

#[test]
fn full_does_not_cover_quick_without_a_declared_relation() {
    let fixture = Fixture::new(|c| c["targets"][0]["evidence"]["covers_modes"] = json!([]));
    fixture.selected("mode is not covered");
}

#[test]
fn corrupt_record_runs() {
    let fixture = Fixture::new(|_| {});
    std::fs::write(&fixture.record, "{broken").unwrap();
    fixture.selected("no qualifying run for the base tree");
}

#[test]
fn missing_qualifying_run_executes() {
    let fixture = Fixture::new(|_| {});
    std::fs::remove_dir_all(fixture.record.parent().unwrap()).unwrap();
    fixture.selected("no qualifying run");
}

#[test]
fn untracked_input_runs_every_target() {
    let fixture = Fixture::new(|_| {});
    std::fs::write(fixture.root.path().join("src/new"), "new").unwrap();
    fixture.selected("untracked input");
}

#[test]
fn ignored_file_under_declared_inputs_still_runs() {
    let fixture = Fixture::new(|_| {});
    std::fs::write(
        fixture.root.path().join(".git/info/exclude"),
        "src/generated\n",
    )
    .unwrap();
    std::fs::write(fixture.root.path().join("src/generated"), "new input").unwrap();
    fixture.selected("untracked declared input");
}

#[test]
fn renamed_input_runs_every_target() {
    let fixture = Fixture::new(|_| {});
    git(fixture.root.path(), &["mv", "src/extra", "src/renamed"]);
    fixture.selected("rename or deletion");
}

#[test]
fn unmatched_changed_input_runs_every_target() {
    let fixture = Fixture::new(|_| {});
    std::fs::write(
        fixture.root.path().join(".gitignore"),
        "target/\ntool.version\nmembership\nextra\n",
    )
    .unwrap();
    fixture.selected("unmatched input");
}

#[test]
fn incomplete_environment_audit_runs() {
    let fixture =
        Fixture::new(|c| c["targets"][0]["evidence"]["environment_complete"] = json!(false));
    fixture.selected("undeclared environment dependencies");
}

#[test]
fn unclassified_environment_runs() {
    let fixture = Fixture::new(|c| {
        c["targets"][0]["evidence"]["environment"]["SELECTION_MARKER"] = json!("unclassified");
    });
    fixture.selected("unclassified environment dependency");
}

#[test]
fn undeclared_configured_environment_runs() {
    let fixture =
        Fixture::new(|c| c["targets"][0]["env"] = json!({"SELECTION_MARKER":"fixture-value"}));
    fixture.selected("undeclared environment dependency");
}

#[test]
fn deleted_input_runs_every_target() {
    let fixture = Fixture::new(|_| {});
    std::fs::remove_file(fixture.root.path().join("src/extra")).unwrap();
    fixture.selected("rename or deletion");
}

#[test]
fn secret_environment_is_opaque_in_every_gate_artifact_and_changes_run() {
    let first = "selection-secret-first-6fdcf7";
    let second = "selection-secret-second-aec314";
    let fixture = Fixture::with_environment(
        |c| {
            for target in c["targets"].as_array_mut().unwrap() {
                target["evidence"]["environment"]["SELECTION_MARKER"] = json!("fingerprint");
                target["report"] = json!({"format":"junit","path":"target/secret-report.xml"});
            }
        },
        Some(first),
    );
    for marker in [first, second] {
        let output = command(
            fixture.root.path(),
            fixture.home.as_path(),
            &["test", "--mode", "quick"],
        )
        .env("SELECTION_MARKER", marker)
        .output()
        .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(!stderr(&output).contains(marker));
        assert!(!String::from_utf8_lossy(&output.stdout).contains(marker));
        if marker == second {
            assert!(
                stderr(&output).contains("selected rust-check"),
                "{}",
                stderr(&output)
            );
            if cfg!(unix) {
                assert!(
                    stderr(&output).contains("declared environment changed"),
                    "{}",
                    stderr(&output)
                );
            }
        } else if cfg!(unix) {
            assert!(
                stderr(&output).contains("skipped rust-check"),
                "{}",
                stderr(&output)
            );
        }
    }
    let mut artifacts = Vec::new();
    files_below(
        &fixture.root.path().join("target/codeflow-gate"),
        &mut artifacts,
    );
    files_below(
        fixture.record.parent().unwrap().parent().unwrap(),
        &mut artifacts,
    );
    for path in artifacts {
        let bytes = std::fs::read(&path).unwrap();
        for marker in [first, second] {
            assert!(
                !bytes
                    .windows(marker.len())
                    .any(|part| part == marker.as_bytes()),
                "secret in {}",
                path.display()
            );
        }
    }
    let record: Value = serde_json::from_slice(&std::fs::read(&fixture.record).unwrap()).unwrap();
    let observation = &record["target_evidence"][0]["before"];
    // Unix provides an owner-only key; other platforms must name the missing
    // safe fingerprint and execute. Both branches are assertions, never skips.
    if cfg!(unix) {
        assert!(observation["identity"]["environment"]["SELECTION_MARKER"]
            .as_str()
            .is_some());
    } else {
        assert!(observation["unavailable"]
            .as_str()
            .unwrap()
            .contains("no safe fingerprint"));
    }
}

#[test]
fn unavailable_fingerprint_runs_with_a_named_reason() {
    let fixture = Fixture::new(|c| {
        c["targets"][0]["evidence"]["environment"]["SELECTION_MARKER"] = json!("sensitive");
    });
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("gate-environment.key")).unwrap();
    assert_unavailable_fingerprint(&fixture, home.path());
}

#[test]
fn changed_producer_output_selects_consumer_and_prerequisite() {
    let fixture = Fixture::new(|c| {
        c["targets"][0]["outputs"] = json!(["target/check-output"]);
        c["targets"][1]["requires"] = json!(["rust-check"]);
    });
    let (text, _) = fixture.quick();
    assert!(
        text.contains(if cfg!(unix) {
            "skipped record-check"
        } else {
            "selected record-check"
        }),
        "{text}"
    );
    std::fs::write(fixture.root.path().join("target/check-output"), "changed").unwrap();
    fixture.selected("producer output changed");
}

#[test]
fn missing_producer_output_selects_consumer_and_prerequisite() {
    let fixture = Fixture::new(|c| {
        c["targets"][0]["outputs"] = json!(["target/check-output"]);
        c["targets"][1]["requires"] = json!(["rust-check"]);
    });
    std::fs::remove_file(fixture.root.path().join("target/check-output")).unwrap();
    fixture.selected("missing or unreadable producer output");
}

#[test]
fn unproven_ancestry_runs_every_target() {
    let fixture = Fixture::new(|_| {});
    git(fixture.root.path(), &["checkout", "--orphan", "unrelated"]);
    git(
        fixture.root.path(),
        &["commit", "-qm", "test: unrelated tree"],
    );
    git(fixture.root.path(), &["switch", "task/TSK-001-selection"]);
    let output = run(
        fixture.root.path(),
        fixture.home.as_path(),
        &["test", "--mode", "quick", "--since", "unrelated"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("unproven ancestry"),
        "{}",
        stderr(&output)
    );
    assert!(stderr(&output).contains("starting target 'rust-check'"));
}

#[test]
fn differing_quick_command_runs_despite_declared_mode_coverage() {
    let fixture = Fixture::new(|c| {
        c["targets"][0]["modes"]["quick"]["command"] = json!("echo different check");
    });
    fixture.selected("mode-specific command differs");
}

#[test]
fn submodule_input_runs() {
    let fixture = Fixture::new(|_| {});
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(fixture.root.path())
        .output()
        .unwrap();
    let oid = String::from_utf8(output.stdout).unwrap();
    git(
        fixture.root.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},src/module", oid.trim()),
        ],
    );
    fixture.selected("submodule input");
}

#[test]
fn pre_push_records_only_change_runs_only_its_quick_reader() {
    pre_push_summary(true);
}

#[test]
fn repair_pre_push_zero_execution_is_not_complete() {
    pre_push_summary(false);
}

fn push_destination(root: &Path) -> PathBuf {
    let destination = root.join(".git/destination.git");
    git(
        root,
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            destination.to_str().unwrap(),
        ],
    );
    git(&destination, &["config", "user.name", "Test"]);
    git(&destination, &["config", "user.email", "test@example.com"]);
    git(
        root,
        &["remote", "add", "origin", destination.to_str().unwrap()],
    );
    git(
        root,
        &[
            "push",
            "origin",
            "integration/checks:refs/heads/main",
            "integration/checks:refs/heads/integration/checks",
        ],
    );
    destination
}

fn pre_push_summary(records_only: bool) {
    use std::io::Write;
    use std::process::Stdio;
    let fixture = Fixture::new(|_| {});
    let root = fixture.root.path();
    let record = root.join("project-management/tasks/TSK-001.md");
    let mut bytes = std::fs::read_to_string(&record).unwrap();
    if records_only {
        bytes.push_str("\nRecorded execution.\n");
    }
    std::fs::write(record, bytes).unwrap();
    git(root, &["add", "project-management/tasks/TSK-001.md"]);
    git(
        root,
        &["commit", "--allow-empty", "-qm", "test: record execution"],
    );
    let destination = push_destination(root);
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    let head = String::from_utf8(head.stdout).unwrap();
    let mut child = command(
        root,
        fixture.home.as_path(),
        &[
            "git-hook",
            "pre-push",
            "origin",
            destination.to_str().unwrap(),
        ],
    )
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    writeln!(child.stdin.take().unwrap(), "refs/heads/task/TSK-001-selection {} refs/heads/task/TSK-001-selection 0000000000000000000000000000000000000000", head.trim()).unwrap();
    let output = child.wait_with_output().unwrap();
    let text = stderr(&output);
    assert!(
        text.contains(if records_only || !cfg!(unix) {
            "selected record-check"
        } else {
            "skipped record-check"
        }),
        "{text}"
    );
    assert!(
        text.contains(if cfg!(unix) {
            "skipped rust-check"
        } else {
            "selected rust-check"
        }),
        "{text}"
    );
    assert!(
        text.contains(if cfg!(unix) {
            if records_only {
                "executed 1; skipped 1; not complete"
            } else {
                "executed 0; skipped 2; not complete"
            }
        } else {
            "executed 2; skipped 0"
        }),
        "{text}"
    );
    if cfg!(unix) {
        assert!(text.contains("quick targets not complete"), "{text}");
        assert!(!text.contains("quick targets passed"), "{text}");
    }
    assert!(text.contains("codeflow ci --base"), "{text}");
    assert!(
        text.contains("range") || text.contains("authority"),
        "{text}"
    );
    assert!(output.status.success(), "{text}");
}

fn files_below(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_below(&path, files);
        } else {
            files.push(path);
        }
    }
}

#[test]
fn repair_full_gate_executes_every_target_with_a_proven_base() {
    use codeflow_core::testing::gate::{run_gate, GateOutcome};
    let fixture = Fixture::new(|_| {});
    let outcome = run_gate(fixture.root.path(), "full").unwrap();
    let GateOutcome::Completed {
        results,
        passed,
        skipped,
        complete,
        ..
    } = outcome
    else {
        panic!("configured full gate must run")
    };
    assert!(passed && complete);
    assert_eq!(results.len(), 2);
    assert!(skipped.is_empty());
    let output = fixture.full(&["test"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"));
}

#[test]
fn repair_strict_refuses_zero_and_partial_execution() {
    let fixture = Fixture::new(|_| {});
    for partial in [false, true] {
        if partial {
            std::fs::write(fixture.root.path().join("src/input"), "changed").unwrap();
        }
        let output = run(
            fixture.root.path(),
            fixture.home.as_path(),
            &["test", "--mode", "quick", "--strict"],
        );
        if !cfg!(unix) {
            assert!(output.status.success(), "{}", stderr(&output));
            assert!(String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"));
            continue;
        }
        assert!(
            !output.status.success(),
            "strict accepted skips: {}",
            stderr(&output)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        let counts = if partial {
            "executed 1; skipped 1; not complete"
        } else {
            "executed 0; skipped 2; not complete"
        };
        assert!(text.contains(counts), "{text}");
    }
}

#[test]
fn repair_overridden_home_denies_selection_credit() {
    let fixture = Fixture::new(|_| {});
    let output = command(
        fixture.root.path(),
        fixture.home.as_path(),
        &["test", "--mode", "quick"],
    )
    .env("CODEFLOW_HOME", fixture.home.as_path().join(".codeflow"))
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("CODEFLOW_HOME is overridden"),
        "{}",
        stderr(&output)
    );
    assert!(stderr(&output).contains("selected rust-check"));
}

#[cfg(unix)]
#[test]
fn repair_writable_records_deny_selection_credit() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new(|_| {});
    for mode in [0o620, 0o602] {
        std::fs::set_permissions(&fixture.record, std::fs::Permissions::from_mode(mode)).unwrap();
        fixture.selected("no qualifying run for the base tree");
    }
}

#[test]
fn repair_no_matching_tree_reason_ignores_unrelated_records() {
    let fixture = Fixture::new(|_| {});
    fixture.alter_record(|r| {
        r.as_object_mut().unwrap().remove("git_tree");
    });
    fixture.selected("no qualifying run for the base tree");
}

#[test]
fn repair_selection_reason_has_no_doubled_separator() {
    let fixture = Fixture::new(|_| {});
    let output = fixture.full(&["test", "--all"]);
    assert!(!stderr(&output).contains("; ;"), "{}", stderr(&output));
}

#[test]
fn repair_integrate_executes_every_target_with_a_proven_base() {
    for merge in [false, true] {
        let fixture = Fixture::new(|_| {});
        let root = fixture.root.path();
        let path = root.join("project-management/tasks/TSK-001.md");
        let mut record = std::fs::read_to_string(&path).unwrap();
        record.push_str("\nRecorded execution.\n");
        std::fs::write(path, record).unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "test: record execution"]);
        let mut args = vec![
            "integrate",
            "task/TSK-001-selection",
            "--into",
            "integration/checks",
        ];
        if merge {
            args.push("--merge");
        }
        let linked = root.join(".worktrees/selection");
        if merge {
            git(root, &["switch", "integration/checks"]);
            git(
                root,
                &[
                    "worktree",
                    "add",
                    linked.to_str().unwrap(),
                    "task/TSK-001-selection",
                ],
            );
        }
        let output = run(
            if merge { &linked } else { root },
            fixture.home.as_path(),
            &args,
        );
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(
            stderr(&output).contains("starting target 'rust-check'"),
            "{}",
            stderr(&output)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[cfg(unix)]
#[test]
fn repair_integrate_forged_records_still_refuse_failing_target() {
    for merge in [false, true] {
        let fixture = Fixture::new(|c| {
            for mode in ["full", "quick"] {
                let command = c["targets"][0]["modes"][mode]["command"].as_str().unwrap();
                c["targets"][0]["modes"][mode]["command"] =
                    json!(format!("{command} && test ! -f target/fail-check"));
            }
        });
        let root = fixture.root.path();
        std::fs::write(root.join("target/fail-check"), "failure").unwrap();
        // A same-user writer can forge successful evidence for an undeclared
        // input. Local selection may credit it; landing must execute anyway.
        let failed = fixture.full(&["test", "--all"]);
        assert!(!failed.status.success());
        let text = stderr(&failed);
        let failed_path = text
            .lines()
            .find_map(|line| line.strip_prefix("[codeflow test] durable artifact: "))
            .unwrap();
        let mut forged: Value =
            serde_json::from_slice(&std::fs::read(failed_path).unwrap()).unwrap();
        forged["passed"] = json!(true);
        forged["complete"] = json!(true);
        for result in forged["results"].as_array_mut().unwrap() {
            result["exit_code"] = json!(0);
            result["error"] = Value::Null;
        }
        std::fs::write(&fixture.record, serde_json::to_vec(&forged).unwrap()).unwrap();
        let (text, _) = fixture.quick();
        assert!(text.contains("skipped rust-check"), "{text}");
        git(root, &["commit", "--allow-empty", "-qm", "test: candidate"]);
        let tip = || {
            std::process::Command::new("git")
                .args(["rev-parse", "integration/checks"])
                .current_dir(root)
                .output()
                .unwrap()
                .stdout
        };
        let before = tip();
        let mut args = vec![
            "integrate",
            "task/TSK-001-selection",
            "--into",
            "integration/checks",
        ];
        if merge {
            args.push("--merge");
        }
        let landing = fixture.integration_root(merge);
        std::fs::create_dir_all(landing.join("target")).unwrap();
        std::fs::write(landing.join("target/fail-check"), "failure").unwrap();
        let output = run(&landing, fixture.home.as_path(), &args);
        assert!(!output.status.success(), "forged record allowed landing");
        assert!(
            stderr(&output).contains("rust-check: exit 1"),
            "{}",
            stderr(&output)
        );
        assert!(
            stderr(&output).contains("executed 2; skipped 0"),
            "{}",
            stderr(&output)
        );
        assert_eq!(tip(), before);
    }
}

#[test]
fn repair_non_secret_values_leave_reports_and_output_intact() {
    for class in [None, Some("audited_non_secret"), Some("unclassified")] {
        let fixture = Fixture::new(|c| {
            c["targets"][0]["env"]["CI_FLAG"] = json!("1");
            if let Some(class) = class {
                c["targets"][0]["evidence"]["environment"]["CI_FLAG"] = json!(class);
            }
        });
        let root = fixture.root.path();
        let config_path = root.join(".codeflow/test-config.json");
        let mut config: Value =
            serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
        let t = &mut config["targets"][0];
        t["modes"]["full"]["command"] = json!("echo Summary 11");
        t["report"] = json!({"format":"junit","path":"target/junit.xml"});
        t["coverage"] = json!({"format":"lcov","path":"target/cov.lcov","rules":[{"scope":"per_file","minimum":90}]});
        std::fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let junit = "<testsuites><testsuite tests=\"1\"><testcase name=\"test1\"/></testsuite></testsuites>";
        let coverage = "SF:src/input\nDA:1,1\nend_of_record\n";
        std::fs::write(root.join("target/junit.xml"), junit).unwrap();
        std::fs::write(root.join("target/cov.lcov"), coverage).unwrap();
        let output = fixture.full(&["test", "--all"]);
        assert!(output.status.success(), "{}", stderr(&output));
        let text = stderr(&output);
        let run = PathBuf::from(
            text.lines()
                .find_map(|l| l.strip_prefix("[codeflow test] artifact: "))
                .unwrap(),
        );
        let artifact: Value = serde_json::from_slice(&std::fs::read(&run).unwrap()).unwrap();
        assert_eq!(artifact["coverage"][0]["overall_percent"], 100.0);
        let dir = run.parent().unwrap().join("rust-check");
        assert_eq!(
            std::fs::read_to_string(dir.join("junit.xml")).unwrap(),
            junit
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("coverage")).unwrap(),
            coverage
        );
        assert!(std::fs::read_to_string(dir.join("stdout.log"))
            .unwrap()
            .contains("Summary 11"));
        assert_eq!(
            std::fs::read_to_string(root.join("target/junit.xml")).unwrap(),
            junit
        );
        assert_eq!(
            std::fs::read_to_string(root.join("target/cov.lcov")).unwrap(),
            coverage
        );
        assert!(
            text.contains("Summary 11"),
            "non-secret progress suppressed: {text}"
        );
    }
}

#[test]
fn repair_sensitive_encoded_values_are_redacted_without_rewriting_source() {
    let marker = "selection-marker&<>\"' é/+ÿÿÿ9Zq";
    let fixture = Fixture::new(|c| {
        c["targets"][0]["env"]["ENCODED_MARKER"] = json!(marker);
        c["targets"][0]["evidence"]["environment"]["ENCODED_MARKER"] = json!("sensitive");
    });
    let root = fixture.root.path();
    let script = r#"import os,json,urllib.parse,html,pathlib,base64,sys
sys.stdout.reconfigure(encoding="utf-8")
v=os.environ['ENCODED_MARKER']
forms=[v,json.dumps(v,ensure_ascii=False)[1:-1],json.dumps(v,ensure_ascii=True)[1:-1],html.escape(v),html.escape(v,quote=False),html.escape(v,quote=False).replace('"','&quot;')]
for ascii in [False,True]:
 for escaped in ['', '&<>', "&<>'"]:
  j=json.dumps(v,ensure_ascii=ascii)[1:-1]
  j=''.join('\\u%04x'%ord(c) if c in escaped else c for c in j)
  forms.extend([j,j.replace(r'\"',r'\u0022')])
forms.append(''.join('\\u%04x'%ord(c) for c in v))
for escaped in ['&<>','&<>"','&<>"\'','all']:
 for non_ascii in [False,True]:
  for fmt in ['&#{};','&#x{:x};','&#x{:X};']:
   forms.append(''.join(fmt.format(ord(c)) if escaped == 'all' or c in escaped or (non_ascii and not c.isascii()) else c for c in v))
for safe in ['', '/']:
 for quote in [urllib.parse.quote,urllib.parse.quote_plus]:
  u=quote(v,safe=safe)
  forms.extend([u,quote(u,safe=safe)])
standard=base64.b64encode(v.encode()).decode()
urlsafe=base64.urlsafe_b64encode(v.encode()).decode()
assert standard != urlsafe
forms.extend([standard,urlsafe])
print('Summary '+ '|'.join(forms))
pathlib.Path('target/encoded-forms.json').write_text(json.dumps(forms),encoding='utf-8')
pathlib.Path('target/encoded.xml').write_text('<testsuites><testsuite><testcase name="'+html.escape(v)+'"/></testsuite></testsuites>',encoding='utf-8')
"#;
    std::fs::write(root.join("encoded.py"), script).unwrap();
    let path = root.join(".codeflow/test-config.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let command = config["targets"][0]["modes"]["full"]["command"]
        .as_str()
        .unwrap()
        .replace("check.py src/input", "encoded.py");
    config["targets"][0]["modes"]["full"]["command"] = json!(command);
    config["targets"][0]["report"] = json!({"format":"junit","path":"target/encoded.xml"});
    std::fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    let output = fixture.full(&["test", "--all"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let forms: Vec<String> =
        serde_json::from_slice(&std::fs::read(root.join("target/encoded-forms.json")).unwrap())
            .unwrap();
    let mut files = Vec::new();
    files_below(&root.join("target/codeflow-gate"), &mut files);
    files_below(
        fixture.record.parent().unwrap().parent().unwrap(),
        &mut files,
    );
    files_below(
        &fixture.isolated_home.path().join(".codeflow/gate-runs"),
        &mut files,
    );
    let mut texts = vec![
        stderr(&output),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ];
    texts.extend(files.iter().map(|p| std::fs::read_to_string(p).unwrap()));
    for text in texts {
        for form in &forms {
            assert!(!text.contains(form), "encoded secret retained: {text}");
        }
    }
    assert!(
        std::fs::read_to_string(root.join("target/encoded.xml"))
            .unwrap()
            .contains("&amp;&lt;&gt;&quot;"),
        "tool-owned source was rewritten"
    );
}

#[test]
fn repair_integrate_refuses_ci_skips_and_leaves_target_unchanged() {
    let fixture = Fixture::new(|c| c["targets"][0]["ci_skip"] = json!(true));
    let root = fixture.root.path();
    git(root, &["commit", "--allow-empty", "-qm", "test: candidate"]);
    let before = std::process::Command::new("git")
        .args(["rev-parse", "integration/checks"])
        .current_dir(root)
        .output()
        .unwrap()
        .stdout;
    let output = command(
        root,
        fixture.home.as_path(),
        &[
            "integrate",
            "task/TSK-001-selection",
            "--into",
            "integration/checks",
        ],
    )
    .env("CI", "true")
    .output()
    .unwrap();
    assert!(!output.status.success(), "integration accepted CI skip");
    assert!(
        stderr(&output).contains("executed 1; skipped 1; not complete"),
        "{}",
        stderr(&output)
    );
    let after = std::process::Command::new("git")
        .args(["rev-parse", "integration/checks"])
        .current_dir(root)
        .output()
        .unwrap()
        .stdout;
    assert_eq!(before, after);
}

#[test]
fn repair_strict_ci_skip_counts_zero_executions() {
    let fixture = Fixture::new(|c| {
        for t in c["targets"].as_array_mut().unwrap() {
            t["ci_skip"] = json!(true);
        }
    });
    let output = command(
        fixture.root.path(),
        fixture.isolated_home.path(),
        &["test", "--strict"],
    )
    .env("CI", "true")
    .output()
    .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("executed 0; skipped 2; not complete"),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        stderr(&output)
    );
}

#[test]
fn repair_fingerprint_reason_names_repository_home() {
    let fixture = Fixture::new(|c| {
        c["targets"][0]["evidence"]["environment"]["SELECTION_MARKER"] = json!("sensitive");
    });
    let home = tempfile::tempdir().unwrap();
    git(home.path(), &["init", "-q"]);
    git(home.path(), &["config", "user.name", "Test"]);
    git(home.path(), &["config", "user.email", "test@example.com"]);
    assert_unavailable_fingerprint(&fixture, home.path());
}

fn assert_unavailable_fingerprint(fixture: &Fixture, home: &Path) {
    let output = command(
        fixture.root.path(),
        fixture.home.as_path(),
        &["test", "--all"],
    )
    .env("CODEFLOW_HOME", home)
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stderr(&output);
    let path = text
        .lines()
        .find_map(|line| line.strip_prefix("[codeflow test] artifact: "))
        .unwrap();
    let record: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let reason = record["target_evidence"][0]["before"]["unavailable"]
        .as_str()
        .unwrap();
    assert!(
        reason.contains(
            "CodeFlow home must be outside every Git repository and the key must be owner-only"
        ),
        "{reason}"
    );
}

#[test]
fn repair_integrate_empty_gate_lands_with_warning() {
    for merge in [false, true] {
        let fixture = Fixture::new(|_| {});
        let root = fixture.root.path();
        std::fs::write(
            root.join(".codeflow/test-config.json"),
            r#"{"schema_version":"1.0","targets":[]}"#,
        )
        .unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "test: empty gate"]);
        let mut args = vec![
            "integrate",
            "task/TSK-001-selection",
            "--into",
            "integration/checks",
        ];
        if merge {
            args.push("--merge");
        }
        let landing = fixture.integration_root(merge);
        let output = run(&landing, fixture.home.as_path(), &args);
        assert!(output.status.success(), "{}", stderr(&output));
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            text.contains("executed 0; skipped 0; not complete"),
            "{text}"
        );
        assert!(
            text.contains("WARNING: no tests ran; this merge is NOT test-verified"),
            "{text}"
        );
        assert!(root.join(".codeflow/test-config.json").exists());
        let landed = std::process::Command::new("git")
            .args(["show", "integration/checks:.codeflow/test-config.json"])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(landed.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&landed.stdout).unwrap()["targets"],
            json!([])
        );
    }
}

#[test]
fn repair_requested_modes_report_passed_without_incomplete_suffix() {
    let fixture = Fixture::new(|c| {
        for target in c["targets"].as_array_mut().unwrap() {
            target["modes"]["essential"] = target["modes"]["full"].clone();
        }
    });
    for mode in ["quick", "essential"] {
        let output = fixture.full(&["test", "--mode", mode, "--all", "--strict"]);
        assert!(output.status.success(), "{}", stderr(&output));
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            text.contains("passed (executed 2; skipped 0; not full-gate evidence)"),
            "{text}"
        );
        assert!(!text.contains("not complete"), "{text}");
    }
}

#[test]
fn essential_default_comparison_uses_the_declared_line() {
    let fixture = Fixture::new(|c| {
        for target in c["targets"].as_array_mut().unwrap() {
            target["modes"]["essential"] = target["modes"]["full"].clone();
            target["evidence"]["covers_modes"] = json!(["quick", "essential"]);
        }
    });
    // The enclosing gate owns the real account's machine lock. Exercise CLI
    // comparison routing under an isolated home, then selection credit through
    // the same core engine under the real account, as Fixture's seeding does.
    let output = fixture.full(&["test", "--mode", "essential"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("integration/checks"),
        "{}",
        stderr(&output)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"));
    if cfg!(unix) {
        let outcome =
            codeflow_core::testing::gate::run_gate(fixture.root.path(), "essential").unwrap();
        assert!(
            matches!(outcome, codeflow_core::testing::gate::GateOutcome::Completed {
            results, passed: true, evidence_skipped: 2, ..
        } if results.is_empty()),
            "essential retains the same account-backed selection credit"
        );
    } else {
        assert!(stderr(&output).contains("account home unavailable"));
    }
}

#[test]
fn repair_redirected_home_forgery_grants_no_credit() {
    let fixture = Fixture::new(|_| {});
    let redirected = tempfile::tempdir().unwrap();
    let original = fixture.home.join(".codeflow");
    let relative = fixture.record.strip_prefix(&original).unwrap();
    let forged = redirected.path().join(".codeflow").join(relative);
    std::fs::create_dir_all(forged.parent().unwrap()).unwrap();
    std::fs::copy(&fixture.record, &forged).unwrap();
    let output = run(
        fixture.root.path(),
        redirected.path(),
        &["test", "--mode", "quick"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stderr(&output);
    let reason = if cfg!(unix) {
        "HOME differs from the effective user's account home"
    } else {
        "selection credit denied: account home unavailable"
    };
    assert!(text.contains(reason), "{text}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"));
}

#[test]
fn repair_explicit_full_comparison_remains_local_incomplete_evidence() {
    use codeflow_core::testing::gate::{run_gate_with_options, GateOptions, GateOutcome};
    let fixture = Fixture::new(|_| {});
    let outcome = run_gate_with_options(
        fixture.root.path(),
        "full",
        &GateOptions {
            since: Some("integration/checks".into()),
            ..GateOptions::default()
        },
    )
    .unwrap();
    let GateOutcome::Completed {
        results,
        passed,
        skipped,
        complete,
        ..
    } = outcome
    else {
        panic!("configured full gate must return execution evidence")
    };
    assert!(passed);
    if cfg!(unix) {
        assert!(!complete);
        assert!(results.is_empty());
        assert_eq!(skipped.len(), 2);
    } else {
        assert_eq!(results.len(), 2);
        assert!(skipped.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn installed_pre_push_honors_the_undeclared_quick_class_and_wait_bound() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let fixture = Fixture::new(|config| {
        config["execution"]["quick_default_class"] = json!("heavy");
        config["execution"]["heavy_wait_seconds"] = json!(1);
    });
    let root = fixture.root.path();
    let home = fixture.isolated_home.path();
    let destination = push_destination(root);
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    let head = String::from_utf8(head.stdout).unwrap();
    let hook = root.join(".git/hooks/pre-push");
    std::fs::write(
        &hook,
        include_str!("../../../../assets/base/git-hooks/pre-push"),
    )
    .unwrap();
    let linked = home.join("linked");
    git(
        root,
        &["worktree", "add", "--detach", linked.to_str().unwrap()],
    );
    std::fs::write(linked.join(".codeflow/test-config.json"), r#"{"schema_version":"1.0","targets":[{"name":"holder","runner":"custom","timeout_seconds":15,"modes":{"full":{"command":"while ! test -f release; do sleep 0.01; done"}}}]}"#).unwrap();
    let (mut holder, rx, _stdout) = super::spawn(command(&linked, home, &["test"]));
    super::wait_for(&rx, "starting target 'holder'");
    let invoke = || {
        let mut child = Command::new("sh")
            .arg(&hook)
            .arg("origin")
            .arg(&destination)
            .current_dir(root)
            .env_remove("CODEFLOW_HOME")
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CI")
            .env(
                "CODEFLOW_HOOK_BINARY",
                std::env::var_os("CODEFLOW_TEST_GATE_BIN")
                    .unwrap_or_else(|| env!("CARGO_BIN_EXE_codeflow").into()),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "refs/heads/task/TSK-001-selection {} refs/heads/task/TSK-001-selection 0000000000000000000000000000000000000000", head.trim()).unwrap();
        child.wait_with_output().unwrap()
    };
    let start = Instant::now();
    let refused = invoke();
    let text = stderr(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(start.elapsed() >= Duration::from_secs(1));
    assert!(text.contains("waiting up to 1s"), "{text}");
    assert!(text.contains(&format!("pid {}", holder.id())), "{text}");
    assert!(text.contains("full mode"), "{text}");
    assert!(text.contains(linked.to_str().unwrap()), "{text}");
    assert!(!text.contains("starting target 'rust-check'"), "{text}");
    std::fs::write(linked.join("release"), "done").unwrap();
    assert!(holder.wait().unwrap().success());
    let after = invoke();
    assert!(after.status.success(), "{}", stderr(&after));
    assert!(
        stderr(&after).contains("quick targets passed"),
        "{}",
        stderr(&after)
    );
}
