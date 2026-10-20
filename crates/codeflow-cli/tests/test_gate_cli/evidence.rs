//! Selection through the real CLI, real Git trees and executed fixture checks.
use super::{git, run, stderr};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

struct Fixture {
    root: tempfile::TempDir,
    home: tempfile::TempDir,
    record: PathBuf,
}

impl Fixture {
    fn new(change: impl FnOnce(&mut Value)) -> Self {
        Self::with_environment(change, &[])
    }

    fn with_environment(change: impl FnOnce(&mut Value), environment: &[(&str, &str)]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let dir = root.path();
        git(dir, &["init", "-q", "-b", "integration/checks"]);
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
        let command = |path: &str| format!("\"{python}\" check.py {path}");
        let audit = json!({"tools":{"python":[python,"probe.py","tool.version"]},
            "environment":{}, "environment_complete":true,
            "membership":[python,"probe.py","membership"],"covers_modes":["quick"]});
        let mut config = json!({"schema_version":"1.0", "execution":{"run_everything":["crates/**"]}, "targets":[
            {"name":"rust-check","runner":"custom","input_class":"content", "evidence":audit,
             "narrow":["src/**","check.py","probe.py"],
             "modes":{"full":{"command":command("src/input")},"quick":{"command":command("src/input")}}},
            {"name":"record-check","runner":"custom","input_class":"content", "evidence":audit,
             "narrow":["project-management/**","check.py","probe.py"],
             "modes":{"full":{"command":command("project-management/tasks/TSK-001.md")},"quick":{"command":command("project-management/tasks/TSK-001.md")}}}
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
        let output = super::command(dir, home.path(), &["test", "--all"])
            .envs(environment.iter().copied())
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        for (_, marker) in environment {
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
        Self { root, home, record }
    }

    fn quick(&self) -> (String, Value) {
        let output = run(
            self.root.path(),
            self.home.path(),
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
        assert!(text.contains(reason), "{text}");
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
    assert!(text.contains("skipped rust-check"), "{text}");
    assert!(record["results"].as_array().unwrap().is_empty());
    assert_eq!(record["complete"], false);
    assert_eq!(
        record["selection"]["skipped"],
        json!(["record-check", "rust-check"])
    );
    let output = run(fixture.root.path(), fixture.home.path(), &["test", "--all"]);
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
    let base: Value = serde_json::from_slice(&std::fs::read(fixture.record).unwrap()).unwrap();
    assert_ne!(record["git_tree"], base["git_tree"]);
    assert!(text.contains("selected record-check"), "{text}");
    assert!(text.contains("skipped rust-check"), "{text}");
    assert_eq!(record["results"].as_array().unwrap().len(), 1);
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
bad_record!(legacy_candidate_runs, "legacy record", |r: &mut Value| {
    r.as_object_mut().unwrap().remove("git_tree");
});
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
fn full_does_not_cover_quick_without_a_declared_relation() {
    let fixture = Fixture::new(|c| c["targets"][0]["evidence"]["covers_modes"] = json!([]));
    fixture.selected("mode is not covered");
}

#[test]
fn corrupt_record_runs() {
    let fixture = Fixture::new(|_| {});
    std::fs::write(&fixture.record, "{broken").unwrap();
    fixture.selected("corrupt or unreadable");
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
        &[("SELECTION_MARKER", first)],
    );
    for marker in [first, second] {
        let output = super::command(
            fixture.root.path(),
            fixture.home.path(),
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
        }
    }
    let mut artifacts = Vec::new();
    files_below(&fixture.root.path().join("target"), &mut artifacts);
    files_below(&fixture.home.path().join("gate-runs"), &mut artifacts);
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
    let key = fixture.home.path().join("gate-environment.key");
    if key.exists() {
        std::fs::remove_file(&key).unwrap();
    }
    std::fs::create_dir(&key).unwrap();
    fixture.selected("no safe fingerprint");
}

#[test]
fn changed_producer_output_selects_consumer_and_prerequisite() {
    let fixture = Fixture::new(|c| {
        c["targets"][0]["outputs"] = json!(["target/check-output"]);
        c["targets"][1]["requires"] = json!(["rust-check"]);
    });
    let (text, _) = fixture.quick();
    assert!(text.contains("skipped record-check"), "{text}");
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
        fixture.home.path(),
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
    use std::io::Write;
    use std::process::Stdio;
    let fixture = Fixture::new(|_| {});
    let root = fixture.root.path();
    let record = root.join("project-management/tasks/TSK-001.md");
    let mut bytes = std::fs::read_to_string(&record).unwrap();
    bytes.push_str("\nRecorded execution.\n");
    std::fs::write(record, bytes).unwrap();
    git(root, &["add", "project-management/tasks/TSK-001.md"]);
    git(root, &["commit", "-qm", "test: record execution"]);
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
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    let head = String::from_utf8(head.stdout).unwrap();
    let mut child = super::command(
        root,
        fixture.home.path(),
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
    assert!(text.contains("selected record-check"), "{text}");
    assert!(text.contains("skipped rust-check"), "{text}");
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
