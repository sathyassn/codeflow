//! Selection through the real CLI, real Git trees and executed fixture checks.
//! No run record earns selection credit (TSK-1044, ADR-0089): every fixture
//! seeds a passed, clean, complete full record for the base tree, the kind
//! that used to skip a content check, and each run still executes every
//! requested target. Every fixture uses its own temporary home.
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

struct Fixture {
    root: tempfile::TempDir,
    home: tempfile::TempDir,
    isolated_home: tempfile::TempDir,
    record: PathBuf,
}

impl Fixture {
    fn new(change: impl FnOnce(&mut Value)) -> Self {
        Self::with_environment(change, None)
    }

    fn with_environment(change: impl FnOnce(&mut Value), marker: Option<&str>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
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
        // Seed a real full record through integration's gate engine, under
        // this fixture's own home.
        git(dir, &["branch", "fixture-base"]);
        let mut command = command(
            dir,
            home.path(),
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
            root,
            home,
            isolated_home: tempfile::tempdir().unwrap(),
            record,
        }
    }

    fn full(&self, args: &[&str]) -> std::process::Output {
        // These runs read no seeded record at all.
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

    /// A quick run beside the seeded record executes both targets and
    /// skips none.
    fn runs_every_target(&self) {
        let (text, record) = self.quick();
        for target in ["rust-check", "record-check"] {
            assert!(text.contains(&format!("selected {target}")), "{text}");
            assert!(
                text.contains(&format!("starting target '{target}'")),
                "{text}"
            );
        }
        assert!(
            text.contains("no local run record earns selection credit"),
            "{text}"
        );
        assert_eq!(record["results"].as_array().unwrap().len(), 2);
        assert_eq!(record["selection"]["skipped"], json!([]));
    }

    /// Replace the seeded record with a forgery: the record of a failed
    /// full run, edited to claim success, as any same-user process can.
    fn forge_record_from_failed_run(&self) {
        let failed = self.full(&["test", "--all"]);
        assert!(!failed.status.success(), "{}", stderr(&failed));
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
        std::fs::write(&self.record, serde_json::to_vec(&forged).unwrap()).unwrap();
    }
}

/// A fixture whose `rust-check` fails while `target/fail-check` exists,
/// with an essential mode on each target, and that file in place.
fn failing_fixture() -> Fixture {
    let fixture = Fixture::new(|c| {
        for target in c["targets"].as_array_mut().unwrap() {
            target["modes"]["essential"] = target["modes"]["full"].clone();
            target["evidence"]["covers_modes"] = json!(["quick", "essential"]);
        }
        for mode in ["full", "quick", "essential"] {
            let command = c["targets"][0]["modes"][mode]["command"].as_str().unwrap();
            c["targets"][0]["modes"][mode]["command"] =
                json!(format!("{command} && test ! -f target/fail-check"));
        }
    });
    std::fs::write(fixture.root.path().join("target/fail-check"), "failure").unwrap();
    fixture
}

/// TSK-1044 AC-3, the fail-before reproduction of finding 2: a passed,
/// clean, complete full record of the base tree skipped both content checks
/// of a quick run. It now earns nothing; the record is the same schema a
/// gate wrote before this change, so it also stands for a pre-upgrade
/// record.
#[test]
fn a_green_record_of_the_base_tree_earns_no_skip() {
    let fixture = Fixture::new(|_| {});
    fixture.runs_every_target();
    let output = fixture.full(&["test", "--all"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("starting target 'rust-check'"));
    assert!(stderr(&output).contains("starting target 'record-check'"));
}

/// TSK-1044 AC-3: a forged record, written by a same-user process to claim
/// a passed complete run, removes no target from any selection-capable run:
/// quick, essential and full with `--since`, under the fixture's home or a
/// redirected one. Each run executes the failing check and fails.
#[test]
fn a_forged_record_removes_no_target_from_any_selection_capable_run() {
    let fixture = failing_fixture();
    fixture.forge_record_from_failed_run();
    let redirected = tempfile::tempdir().unwrap();
    let relative = fixture
        .record
        .strip_prefix(fixture.home.path().join(".codeflow"))
        .unwrap();
    let copy = redirected.path().join(".codeflow").join(relative);
    std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
    std::fs::copy(&fixture.record, &copy).unwrap();
    for home in [fixture.home.path(), redirected.path()] {
        for args in [
            vec!["test", "--mode", "quick"],
            vec!["test", "--mode", "essential"],
            vec!["test", "--mode", "full", "--since", "integration/checks"],
        ] {
            let output = run(fixture.root.path(), home, &args);
            let text = stderr(&output);
            assert!(!output.status.success(), "{args:?}: forged credit: {text}");
            assert!(
                text.contains("starting target 'rust-check'"),
                "{args:?}: {text}"
            );
            assert!(
                text.contains("completed target 'rust-check': FAILED"),
                "{args:?}: {text}"
            );
            assert!(!text.contains("skipped rust-check"), "{args:?}: {text}");
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"),
                "{args:?}: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
}

/// An overridden `CodeFlow` home was the case with no credit before; it
/// still runs every target.
#[test]
fn an_overridden_home_runs_every_target() {
    let fixture = Fixture::new(|_| {});
    let output = command(
        fixture.root.path(),
        fixture.home.path(),
        &["test", "--mode", "quick"],
    )
    .env("CODEFLOW_HOME", fixture.home.path().join(".codeflow"))
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("selected rust-check"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"));
}

/// A run without the seeded record and a run whose inputs changed behave
/// the same as one beside it: every target runs.
#[test]
fn changed_inputs_and_a_missing_record_run_every_target() {
    let fixture = Fixture::new(|_| {});
    std::fs::write(fixture.root.path().join("tool.version"), "2.0\n").unwrap();
    fixture.runs_every_target();
    std::fs::remove_dir_all(fixture.record.parent().unwrap()).unwrap();
    fixture.runs_every_target();
}

#[test]
fn secret_environment_is_opaque_in_every_gate_artifact() {
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
            fixture.home.path(),
            &["test", "--mode", "quick"],
        )
        .env("SELECTION_MARKER", marker)
        .output()
        .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(!stderr(&output).contains(marker));
        assert!(!String::from_utf8_lossy(&output.stdout).contains(marker));
        assert!(
            stderr(&output).contains("selected rust-check"),
            "{}",
            stderr(&output)
        );
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

// TSK-449 AC-1: the push set runs `codeflow ci` and no quick target, for a
// records-only change and for a code change alike.
#[test]
fn pre_push_runs_no_quick_target_for_a_records_only_change() {
    pre_push_summary(true);
}

#[test]
fn pre_push_runs_no_quick_target_for_a_code_change() {
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
    for absent in ["record-check", "rust-check", "quick targets", "executed "] {
        assert!(!text.contains(absent), "{records_only}: {absent}: {text}");
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
    let fixture = Fixture::new(|_| {});
    let output = run(fixture.root.path(), fixture.home.path(), &["test"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"));
    let text = stderr(&output);
    let path = text
        .lines()
        .find_map(|line| line.strip_prefix("[codeflow test] durable artifact: "))
        .unwrap();
    let record: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(record["complete"], true);
    assert_eq!(record["passed"], true);
}

/// `--strict` refused a quick run that skipped targets on candidate
/// evidence; with no credit nothing is skipped, so it passes.
#[test]
fn repair_strict_quick_executes_every_target() {
    let fixture = Fixture::new(|_| {});
    for partial in [false, true] {
        if partial {
            std::fs::write(fixture.root.path().join("src/input"), "changed").unwrap();
        }
        let output = run(
            fixture.root.path(),
            fixture.home.path(),
            &["test", "--mode", "quick", "--strict"],
        );
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"));
    }
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
            fixture.home.path(),
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
        // A same-user writer can forge successful evidence; landing must
        // execute anyway.
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
        // Local selection earns no credit from it either (TSK-1044).
        let quick = run(root, fixture.home.path(), &["test", "--mode", "quick"]);
        assert!(!quick.status.success(), "{}", stderr(&quick));
        assert!(
            stderr(&quick).contains("completed target 'rust-check': FAILED"),
            "{}",
            stderr(&quick)
        );
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
        let output = run(&landing, fixture.home.path(), &args);
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
        fixture.home.path(),
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
    let output = command(fixture.root.path(), fixture.home.path(), &["test", "--all"])
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
        let output = run(&landing, fixture.home.path(), &args);
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

/// Essential compared with the task line and could skip on its record;
/// it now runs every target beside a green record.
#[test]
fn essential_runs_every_target_beside_a_green_record() {
    let fixture = Fixture::new(|c| {
        for target in c["targets"].as_array_mut().unwrap() {
            target["modes"]["essential"] = target["modes"]["full"].clone();
            target["evidence"]["covers_modes"] = json!(["quick", "essential"]);
        }
    });
    let output = run(
        fixture.root.path(),
        fixture.home.path(),
        &["test", "--mode", "essential"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("no local run record earns selection credit"),
        "{}",
        stderr(&output)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"));
}

/// A full run with `--since` could skip on a record and was then left
/// incomplete; it now executes every target and is complete evidence.
#[test]
fn an_explicit_full_comparison_runs_every_target() {
    let fixture = Fixture::new(|_| {});
    let output = run(
        fixture.root.path(),
        fixture.home.path(),
        &["test", "--mode", "full", "--since", "integration/checks"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stderr(&output);
    assert!(
        text.contains("--since integration/checks narrows nothing"),
        "{text}"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"));
    let path = text
        .lines()
        .find_map(|line| line.strip_prefix("[codeflow test] artifact: "))
        .unwrap();
    let record: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(record["complete"], true);
}

/// TSK-355 AC-1, AC-4, AC-7 and AC-10 under credit row B (TSK-1044 adopted
/// no protected recorder): beside the fixture's seeded green full record,
/// three explicit quick runs at three commits of one tree (the shape of the
/// recorded `dec65cb133e4` and `d37e55ed86be` pair, chained once more) each
/// execute every target, name the reason and cite no earlier run. Then a
/// fourth, after a check starts failing on the same tree, executes it and
/// fails: a reused passing result would have hidden it.
#[test]
fn quick_runs_of_one_tree_reuse_no_earlier_result() {
    let fixture = Fixture::new(|c| {
        let command = c["targets"][0]["modes"]["quick"]["command"]
            .as_str()
            .unwrap()
            .to_owned();
        c["targets"][0]["modes"]["quick"]["command"] =
            json!(format!("{command} && test ! -f target/fail-check"));
    });
    let dir = fixture.root.path();
    let mut runs: Vec<Value> = Vec::new();
    for round in 0..3 {
        if round > 0 {
            git(dir, &["commit", "--allow-empty", "-qm", "test: same tree"]);
        }
        let (text, record) = fixture.quick();
        for target in ["rust-check", "record-check"] {
            assert!(
                text.contains(&format!("starting target '{target}'")),
                "{text}"
            );
        }
        assert!(
            text.contains("no local run record earns selection credit"),
            "{text}"
        );
        assert_eq!(record["results"].as_array().unwrap().len(), 2);
        let raw = serde_json::to_string(&record).unwrap();
        for earlier in &runs {
            assert_eq!(record["git_tree"], earlier["git_tree"], "one tree");
            assert_ne!(record["revision"], earlier["revision"], "two commits");
            let id = earlier["run_id"].as_str().unwrap();
            assert!(!raw.contains(id), "run {round} names earlier run {id}");
        }
        assert!(!raw.contains("reused"), "{raw}");
        assert_eq!(record["selection"]["skipped"], json!([]));
        runs.push(record);
    }
    // The same tree again, with earlier passing quick records beside it;
    // the check now fails, and the run executes it and fails.
    std::fs::write(dir.join("target/fail-check"), "failure").unwrap();
    git(dir, &["commit", "--allow-empty", "-qm", "test: same tree"]);
    let output = run(dir, fixture.home.path(), &["test", "--mode", "quick"]);
    let text = stderr(&output);
    assert!(
        !output.status.success(),
        "a passing result was reused: {text}"
    );
    assert!(
        text.contains("completed target 'rust-check': FAILED"),
        "{text}"
    );
    assert!(
        text.contains("no local run record earns selection credit"),
        "{text}"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("executed 2; skipped 0"));
}
