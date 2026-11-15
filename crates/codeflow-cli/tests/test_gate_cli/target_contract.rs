//! TSK-1190: target inputs and exit 77, through the built binary in scratch repos.
use super::{command, git, repo, run, stderr};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn artifact(output: &std::process::Output) -> (PathBuf, Value) {
    let text = stderr(output);
    let path = PathBuf::from(
        text.lines()
            .find_map(|line| line.strip_prefix("[codeflow test] artifact: "))
            .expect(&text),
    );
    let record = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    (path, record)
}

fn fixture(exit: i32) -> tempfile::TempDir {
    let config = json!({"schema_version":"1.0","targets":[
        {"name":"inputs","runner":"custom","modes":{
            "quick":{"command":format!("python3 inputs.py {exit}")},
            "full":{"command":format!("python3 inputs.py {exit}")}}},
        {"name":"other","runner":"custom","modes":{
            "quick":{"command":"echo other"},"full":{"command":"echo other"}}}
    ]});
    let dir = repo(&config.to_string());
    std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    std::fs::write(dir.path().join("inputs.py"), r#"import os,sys,json
from pathlib import Path
data={k:os.environ.get(k) for k in ['CODEFLOW_BASE','CODEFLOW_HEAD','CODEFLOW_CHANGED_PATHS_FILE','CODEFLOW_LANDING_LINE']}
print('Summary inputs '+json.dumps(data))
Path('target/inputs.json').write_text(json.dumps(data))
override=Path('target/exit-code')
sys.exit(int(override.read_text() if override.exists() else sys.argv[1]))
"#).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "test: target contract"]);
    dir
}

fn inputs(root: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(root.join("target/inputs.json")).unwrap()).unwrap()
}

#[test]
fn commands_receive_comparison_in_quick_and_unknown_without_base() {
    comparison_fixture("quick");
}

#[test]
fn commands_receive_comparison_in_full_and_unknown_without_base() {
    comparison_fixture("full");
}

fn comparison_fixture(mode: &str) {
    let dir = fixture(0);
    let home = tempfile::tempdir().unwrap();
    let output = run(
        dir.path(),
        home.path(),
        &["test", "--mode", mode, "--since", "HEAD"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let (path, record) = artifact(&output);
    let data = inputs(dir.path());
    assert!(stderr(&output).contains("Summary inputs"));
    assert_eq!(data["CODEFLOW_BASE"], record["revision"]);
    assert_eq!(data["CODEFLOW_HEAD"], record["revision"]);
    let file = Path::new(data["CODEFLOW_CHANGED_PATHS_FILE"].as_str().unwrap());
    assert_eq!(file.parent(), path.parent());
    assert!(std::fs::read(file).unwrap().is_empty());
    assert_eq!(record["comparison"]["base"], record["revision"]);
    assert_eq!(record["comparison"]["proven"], true);
    assert_eq!(record["schema_version"], 2);
    // This is the built-binary journey output, retained by --nocapture.
    println!("{mode} known: {}", data);
    println!(
        "{mode} known binary output:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        stderr(&output)
    );
    println!("known record: {record}");
    let unknown = run(dir.path(), home.path(), &["test", "--mode", mode]);
    assert!(unknown.status.success(), "{}", stderr(&unknown));
    let (_, no_base) = artifact(&unknown);
    let data = inputs(dir.path());
    for key in [
        "CODEFLOW_BASE",
        "CODEFLOW_HEAD",
        "CODEFLOW_CHANGED_PATHS_FILE",
    ] {
        assert_eq!(data[key], "unknown");
    }
    assert_eq!(no_base["comparison"]["proven"], false);
    assert_eq!(no_base["schema_version"], 2);
    assert!(no_base["comparison"]["paths_file"].is_null());
    assert_eq!(
        record["selection"]["selected"],
        no_base["selection"]["selected"]
    );
    assert_eq!(
        record["selection"]["skipped"],
        no_base["selection"]["skipped"]
    );
    assert_eq!(record["passed"], no_base["passed"]);
    assert_eq!(record["complete"], no_base["complete"]);
    assert_eq!(
        record["target_evidence"].as_array().unwrap().len(),
        no_base["target_evidence"].as_array().unwrap().len()
    );
    assert_eq!(no_base["results"].as_array().unwrap().len(), 2);
    println!("{mode} unknown: {}", data);
    println!(
        "{mode} unknown binary output:\n{}{}",
        String::from_utf8_lossy(&unknown.stdout),
        stderr(&unknown)
    );
    println!("unknown record: {no_base}");
}

#[test]
fn reserved_exit_is_not_selected_incomplete_and_never_reused() {
    let dir = fixture(77);
    let home = tempfile::tempdir().unwrap();
    for args in [
        vec!["test", "--mode", "quick"],
        vec!["test", "--mode", "quick", "--strict"],
        vec!["test", "--mode", "full"],
    ] {
        let output = run(dir.path(), home.path(), &args);
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(!output.status.success(), "{text}");
        assert!(text.contains("skipped, not selected"), "{text}");
        assert!(text.contains("not complete"), "{text}");
        let (_, record) = artifact(&output);
        assert_eq!(record["passed"], false);
        assert_eq!(record["complete"], false);
        assert_eq!(record["target_evidence"].as_array().unwrap().len(), 1);
        let result = record["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "inputs")
            .unwrap();
        assert_eq!(result["status"], "not_selected");
        assert_eq!(result["report"], Value::Null);
        println!("{args:?}: {text}");
    }
    // A preceding skipped record gives no reuse: fixing the target really runs it.
    std::fs::write(dir.path().join("target/exit-code"), "0").unwrap();
    let output = run(dir.path(), home.path(), &["test", "--mode", "quick"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("starting target 'inputs'"));
}

#[test]
fn detached_candidate_receives_landing_line_but_no_implicit_base() {
    let dir = fixture(0);
    git(dir.path(), &["switch", "--detach", "HEAD"]);
    let home = tempfile::tempdir().unwrap();
    let output = command(dir.path(), home.path(), &["test", "--mode", "quick"])
        .env("CODEFLOW_LANDING_LINE", "integration/example")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        inputs(dir.path())["CODEFLOW_LANDING_LINE"],
        "integration/example"
    );
    assert_eq!(inputs(dir.path())["CODEFLOW_BASE"], "unknown");
}

#[test]
fn two_worktrees_have_distinct_run_path_files_and_keep_the_base_in_the_record() {
    let dir = fixture(0);
    let temp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::open(dir.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let branch = repo.branch("other", &head, false).unwrap();
    let mut options = git2::WorktreeAddOptions::new();
    options.reference(Some(branch.get()));
    let linked = temp.path().join("linked");
    // Both repositories and this linked worktree live in this test's
    // scratch directories, never in the CodeFlow repository.
    let _worktree = repo.worktree("linked", &linked, Some(&options)).unwrap();
    std::fs::write(dir.path().join("new path\nwith newline"), "x").unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut files = Vec::new();
    for root in [dir.path(), linked.as_path()] {
        let output = run(
            root,
            home.path(),
            &["test", "--mode", "quick", "--since", "HEAD"],
        );
        assert!(output.status.success(), "{}", stderr(&output));
        let (_, record) = artifact(&output);
        assert_eq!(record["comparison"]["base"], head.id().to_string());
        let file = PathBuf::from(
            inputs(root)["CODEFLOW_CHANGED_PATHS_FILE"]
                .as_str()
                .unwrap(),
        );
        assert!(file.starts_with(root.canonicalize().unwrap()));
        files.push(file);
    }
    assert_ne!(files[0], files[1]);
    assert_eq!(
        std::fs::read(&files[0]).unwrap(),
        b"new path\nwith newline\0"
    );
    assert!(std::fs::read(&files[1]).unwrap().is_empty());
}

#[test]
fn no_task_no_declared_target_detached_and_bad_since_all_run_every_target() {
    for case in ["no task", "no target", "detached", "bad since", "shallow"] {
        let dir = fixture(0);
        let home = tempfile::tempdir().unwrap();
        let mut args = vec!["test", "--mode", "quick"];
        match case {
            "no target" => {
                git(dir.path(), &["switch", "-c", "task/TSK-001-inputs"]);
                std::fs::create_dir_all(dir.path().join("project-management/tasks")).unwrap();
                std::fs::write(
                    dir.path().join("project-management/tasks/TSK-001.md"),
                    "---\nid: TSK-001\n---\n",
                )
                .unwrap();
                git(dir.path(), &["add", "."]);
                git(dir.path(), &["commit", "-qm", "test: no declared target"]);
            }
            "detached" => git(dir.path(), &["switch", "--detach", "HEAD"]),
            "bad since" => args.extend(["--since", "missing-revision"]),
            "shallow" => {
                // A real shallow boundary, without a clone or network access.
                let repo = git2::Repository::open(dir.path()).unwrap();
                std::fs::write(
                    repo.path().join("shallow"),
                    format!("{}\n", repo.head().unwrap().target().unwrap()),
                )
                .unwrap();
                args.extend(["--since", "HEAD"]);
            }
            _ => {}
        }
        let output = run(dir.path(), home.path(), &args);
        assert!(output.status.success(), "{case}: {}", stderr(&output));
        let (_, record) = artifact(&output);
        assert_eq!(record["comparison"]["proven"], false, "{case}: {record}");
        assert_eq!(record["results"].as_array().unwrap().len(), 2);
        assert!(record["comparison"]["paths_file"].is_null());
        for key in [
            "CODEFLOW_BASE",
            "CODEFLOW_HEAD",
            "CODEFLOW_CHANGED_PATHS_FILE",
        ] {
            assert_eq!(inputs(dir.path())[key], "unknown", "{case}");
        }
    }
}

#[test]
fn skip_blocks_prerequisites_and_ignores_stale_pass_reports_and_coverage() {
    let dir = fixture(77);
    let path = dir.path().join(".codeflow/test-config.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["execution"] = json!({"parallel":true});
    config["targets"][0]["report"] = json!({"format":"junit","path":"stale.xml"});
    config["targets"][0]["coverage"] = json!({"format":"lcov","path":"stale.lcov"});
    config["targets"][1]["requires"] = json!(["inputs"]);
    std::fs::write(&path, config.to_string()).unwrap();
    std::fs::write(
        dir.path().join("stale.xml"),
        "<testsuites><testsuite><testcase name=\"pass\"/></testsuite></testsuites>",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("stale.lcov"),
        "SF:README.md\nDA:1,1\nend_of_record\n",
    )
    .unwrap();
    let home = tempfile::tempdir().unwrap();
    let output = run(
        dir.path(),
        home.path(),
        &["test", "--mode", "full", "--strict"],
    );
    assert!(!output.status.success());
    assert!(!stderr(&output).contains("starting target 'other'"));
    let (_, record) = artifact(&output);
    assert_eq!(record["complete"], false);
    assert_eq!(record["target_evidence"], json!([]));
    assert_eq!(record["coverage"], json!([]));
    for result in record["results"].as_array().unwrap() {
        assert!(result["report"].is_null());
    }
}

#[test]
fn a_protected_landing_target_cannot_pass_with_a_skipped_check() {
    let dir = fixture(77);
    // main is a protected landing target under the default Git policy.
    let home = tempfile::tempdir().unwrap();
    let output = command(
        dir.path(),
        home.path(),
        &["test", "--mode", "full", "--all"],
    )
    .env("CODEFLOW_LANDING_LINE", "main")
    .output()
    .unwrap();
    assert!(!output.status.success());
    let (_, record) = artifact(&output);
    assert_eq!(record["comparison"]["landing_line"], "main");
    assert_eq!(record["passed"], false);
    assert_eq!(record["complete"], false);
}
