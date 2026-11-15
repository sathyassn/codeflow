//! TSK-1010 journeys. Only isolated temporary repositories and their own files
//! are read; no repository tree traversal or test-module includes.
#![cfg(unix)]
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn project(root: &Path, shared: bool, exclusive: bool) {
    std::fs::create_dir_all(root.join(".codeflow")).unwrap();
    if !root.join(".git").exists() {
        let repo = git2::Repository::init(root).unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("fixture", "fixture@example.invalid").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "fixture", &tree, &[])
            .unwrap();
    }
    std::fs::write(
        root.join("worker.py"),
        r#"import pathlib, sys, time, json
folder, name, barrier = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
start = time.time()
(folder / (name + '.started')).write_text(str(start))
if barrier != '-':
    deadline = time.monotonic() + 10
    while not (folder / (barrier + '.started')).exists():
        if time.monotonic() >= deadline: raise RuntimeError('overlap barrier expired')
        time.sleep(.01)
time.sleep(1)
if name == 'a' and (folder / 'hold-exclusive').exists():
    deadline = time.monotonic() + 30
    while not (folder / 'release-exclusive').exists():
        if time.monotonic() >= deadline: raise RuntimeError('holder release expired')
        time.sleep(.01)
(folder / (name + '.json')).write_text(json.dumps([start, time.time()]))
"#,
    )
    .unwrap();
    let mut target = serde_json::json!({"name":"resource","runner":"custom","heavy":true,
        "host_exclusive":exclusive,"modes":{"quick":{"command":"python3 worker.py"}}});
    if shared {
        target["shared_resources"] = serde_json::json!(["browser-cache", "port-4700"]);
    }
    let config = serde_json::json!({"schema_version":"1.0", "execution":{"parallel":true,"max_parallel":2,"host_jobs":8},"targets":[target]});
    std::fs::write(
        root.join(".codeflow/test-config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
}

fn configure(root: &Path, folder: &Path, id: &str, unrelated: bool, reverse: bool) {
    let path = root.join(".codeflow/test-config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["targets"][0]["modes"]["quick"]["command"] =
        format!("python3 worker.py '{}' {id} -", folder.display()).into();
    if reverse {
        config["targets"][0]["shared_resources"] =
            serde_json::json!(["port-4700", "browser-cache"]);
    }
    if unrelated {
        let other = if id == "a" { "b-free" } else { "a-free" };
        config["targets"].as_array_mut().unwrap().push(serde_json::json!({"name":"unrelated","runner":"custom","heavy":true,
            "modes":{"quick":{"command":format!("python3 worker.py '{}' {id}-free {other}",folder.display())}}}));
    }
    std::fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
}

fn run(root: &Path, home: &Path, wait: &str) -> Child {
    run_mode(root, home, wait, "quick")
}

fn run_mode(root: &Path, home: &Path, wait: &str, mode: &str) -> Child {
    let installed = install(home);
    Command::new(installed)
        .args(["test", "--mode", mode, "--all", "--wait-seconds", wait])
        .current_dir(root)
        .env("CODEFLOW_HOME", home)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env_remove("CODEFLOW_RESERVED_LOCK_FDS")
        .env_remove("CI")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn install(home: &Path) -> PathBuf {
    let installed = home.join("bin/codeflow");
    if !installed.exists() {
        std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
        let binary = std::env::var_os("CODEFLOW_TSK1010_TEST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_codeflow").into());
        std::fs::copy(binary, &installed).unwrap();
    }
    installed
}

#[test]
fn a_nested_reserve_refusal_records_the_original_owner() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let home = temp.path().join("home");
    project(&root, false, true);
    configure(&root, temp.path(), "a", false, false);
    let binary = install(&home);
    let output = Command::new(&binary)
        .args(["reserve", "--"])
        .arg(&binary)
        .args(["test", "--mode", "quick", "--all"])
        .current_dir(&root)
        .env("CODEFLOW_HOME", &home)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env_remove("CODEFLOW_RESERVED_LOCK_FDS")
        .env_remove("CI")
        .output()
        .unwrap();
    assert!(!output.status.success());
    println!(
        "nested reserve: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record = artifact(&root);
    assert_eq!(record["wait"]["holder"]["mode"], "reserve", "{record}");
    assert!(record["wait"]["holder"]["pid"].is_number());
    assert!(record["wait"]["holder"]["worktree"].is_string(), "{record}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("asked at"));
}

fn finish(child: Child) -> Output {
    child.wait_with_output().unwrap()
}
fn assert_ok(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn artifacts(root: &Path) -> Vec<serde_json::Value> {
    let folder = root.join("target/codeflow-gate");
    let paths: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("run.json"))
        .filter(|path| path.is_file())
        .collect();
    paths
        .iter()
        .map(|path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
        .collect()
}
fn artifact(root: &Path) -> serde_json::Value {
    let mut records = artifacts(root);
    assert_eq!(records.len(), 1);
    println!("run.json {}: {}", root.display(), records[0]);
    records.pop().unwrap()
}
fn interval(folder: &Path, name: &str) -> [f64; 2] {
    let interval =
        serde_json::from_slice(&std::fs::read(folder.join(format!("{name}.json"))).unwrap())
            .unwrap();
    println!("{name}: {interval:?}");
    interval
}
fn overlap(a: [f64; 2], b: [f64; 2]) -> bool {
    a[0] < b[1] && b[0] < a[1]
}

#[test]
fn shared_names_serialize_in_reverse_order_while_unrelated_targets_overlap() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    let home = temp.path().join("home");
    project(&a, true, false);
    // Disposable fixture checkouts are wholly under TMPDIR, never the task
    // worktree directory. Their shared git common directory is exercised too.
    git2::Repository::open(&a)
        .unwrap()
        .worktree("fixture-b", &b, None)
        .unwrap();
    project(&b, true, false);
    configure(&a, temp.path(), "a", true, false);
    configure(&b, temp.path(), "b", true, true);
    let first = run(&a, &home, "10");
    let second = run(&b, &home, "10");
    assert_ok(&finish(first));
    assert_ok(&finish(second));
    assert!(!overlap(
        interval(temp.path(), "a"),
        interval(temp.path(), "b")
    ));
    assert!(overlap(
        interval(temp.path(), "a-free"),
        interval(temp.path(), "b-free")
    ));
    let records = [artifact(&a), artifact(&b)];
    assert!(
        records.iter().any(|record| record["wait"].is_object()),
        "{records:?}"
    );
}

#[test]
fn exclusive_refusal_records_holder_and_request_time() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    let home = temp.path().join("home");
    project(&a, false, true);
    project(&b, false, false);
    configure(&a, temp.path(), "a", false, false);
    configure(&b, temp.path(), "b", false, false);
    std::fs::write(temp.path().join("hold-exclusive"), "hold").unwrap();
    let first = run(&a, &home, "0");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !temp.path().join("a.started").exists() {
        assert!(Instant::now() < deadline, "holder never started");
        std::thread::sleep(Duration::from_millis(10));
    }
    let second = finish(run(&b, &home, "0"));
    std::fs::write(temp.path().join("release-exclusive"), "release").unwrap();
    assert!(!second.status.success());
    assert_ok(&finish(first));
    let record = artifact(&b);
    assert!(record["wait"]["wait_started"].is_number(), "{record}");
    assert!(record["wait"]["acquired"].is_null(), "{record}");
    assert!(record["wait"]["holder"]["pid"].is_number(), "{record}");
    let text = String::from_utf8_lossy(&second.stderr);
    assert!(text.contains("asked at"), "{text}");
    assert_eq!(record["schema_version"], 2);
    assert!(artifact(&a).get("wait").is_none());
}

#[test]
fn different_names_overlap_without_wait_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    let home = temp.path().join("home");
    for (root, id, other) in [(&a, "a", "b"), (&b, "b", "a")] {
        project(root, true, false);
        configure(root, temp.path(), id, false, false);
        let path = root.join(".codeflow/test-config.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        config["targets"][0]["shared_resources"] = serde_json::json!([id]);
        config["targets"][0]["modes"]["quick"]["command"] =
            format!("python3 worker.py '{}' {id} {other}", temp.path().display()).into();
        std::fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    }
    let first = run(&a, &home, "10");
    let second = run(&b, &home, "10");
    assert_ok(&finish(first));
    assert_ok(&finish(second));
    assert!(overlap(
        interval(temp.path(), "a"),
        interval(temp.path(), "b")
    ));
    assert!(artifact(&a).get("wait").is_none());
    assert!(artifact(&b).get("wait").is_none());
}

#[test]
fn an_unreadable_exclusive_holder_is_explicitly_unknown() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    let home = temp.path().join("home");
    project(&a, false, true);
    project(&b, false, false);
    configure(&a, temp.path(), "a", false, false);
    configure(&b, temp.path(), "b", false, false);
    std::fs::write(temp.path().join("hold-exclusive"), "hold").unwrap();
    let first = run(&a, &home, "0");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !temp.path().join("a.started").exists() {
        assert!(Instant::now() < deadline, "holder never started");
        std::thread::sleep(Duration::from_millis(10));
    }
    std::fs::write(home.join("locks/host.record"), "not a holder record").unwrap();
    let second = finish(run(&b, &home, "0"));
    std::fs::write(temp.path().join("release-exclusive"), "release").unwrap();
    assert_ok(&finish(first));
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("holder unknown"));
    assert_eq!(artifact(&b)["wait"]["holder"], "holder unknown");
}

fn waiting_reader(
    child: &mut Child,
) -> (
    std::sync::mpsc::Receiver<()>,
    std::thread::JoinHandle<String>,
) {
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        for line in std::io::BufReader::new(stderr).lines() {
            let line = line.unwrap();
            if line.contains("waiting up to") {
                let _ = tx.send(());
            }
            text.push_str(&line);
            text.push('\n');
        }
        text
    });
    (rx, reader)
}

fn wait_started(folder: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !folder.join("a.started").exists() {
        assert!(Instant::now() < deadline, "holder never started");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_successful_exclusive_wait_records_acquisition() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    let home = temp.path().join("home");
    project(&a, false, true);
    project(&b, false, false);
    configure(&a, temp.path(), "a", false, false);
    configure(&b, temp.path(), "b", false, false);
    std::fs::write(temp.path().join("hold-exclusive"), "hold").unwrap();
    let first = run(&a, &home, "0");
    wait_started(temp.path());
    let mut second = run(&b, &home, "10");
    let (waiting, reader) = waiting_reader(&mut second);
    let observed = waiting.recv_timeout(Duration::from_secs(20));
    std::fs::write(temp.path().join("release-exclusive"), "release").unwrap();
    let output = finish(second);
    assert_ok(&finish(first));
    let stderr = reader.join().unwrap();
    assert!(observed.is_ok(), "{stderr}");
    assert_ok(&output);
    let record = artifact(&b);
    assert!(record["wait"]["acquired"].is_number(), "{record}");
    assert!(record["wait"]["seconds_waited"].as_f64().unwrap() > 0.0);
    assert!(record["wait"]["holder"]["pid"].is_number());
}

#[test]
fn an_output_lease_wait_records_the_writer() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let home = temp.path().join("home");
    project(&root, false, false);
    configure(&root, temp.path(), "a", false, false);
    let path = root.join(".codeflow/test-config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["targets"][0]["report"] = serde_json::json!({"format":"junit","path":"report.xml"});
    std::fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    std::fs::write(temp.path().join("hold-exclusive"), "hold").unwrap();
    let first = run(&root, &home, "0");
    wait_started(temp.path());
    let mut second = run(&root, &home, "0");
    let (waiting, reader) = waiting_reader(&mut second);
    let observed = waiting.recv_timeout(Duration::from_secs(20));
    std::fs::write(temp.path().join("release-exclusive"), "release").unwrap();
    let output = finish(second);
    assert_ok(&finish(first));
    let stderr = reader.join().unwrap();
    assert!(observed.is_ok(), "{stderr}");
    assert_ok(&output);
    let records = artifacts(&root);
    assert_eq!(records.len(), 2);
    let record = records
        .iter()
        .find(|record| record.get("wait").is_some())
        .expect("writer wait missing");
    assert_eq!(record["wait"]["holder"]["mode"], "quick");
    assert!(record["wait"]["acquired"].is_number());
}

#[test]
fn reserve_contention_records_the_reserve_holder() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    let home = temp.path().join("home");
    project(&a, false, false);
    project(&b, false, true);
    configure(&a, temp.path(), "a", false, false);
    configure(&b, temp.path(), "b", false, false);
    std::fs::write(temp.path().join("hold-exclusive"), "hold").unwrap();
    // Install the binary as run() does, without starting a gate first.
    std::fs::create_dir_all(home.join("bin")).unwrap();
    let binary = std::env::var_os("CODEFLOW_TSK1010_TEST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_codeflow").into());
    std::fs::copy(binary, home.join("bin/codeflow")).unwrap();
    let first = Command::new(home.join("bin/codeflow"))
        .args(["reserve", "--", "python3", "worker.py"])
        .arg(temp.path())
        .args(["a", "-"])
        .current_dir(&a)
        .env("CODEFLOW_HOME", &home)
        .env("CARGO_TARGET_DIR", a.join("target"))
        .env_remove("CODEFLOW_RESERVED_LOCK_FDS")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_started(temp.path());
    let second = finish(run(&b, &home, "1"));
    std::fs::write(temp.path().join("release-exclusive"), "release").unwrap();
    assert_ok(&finish(first));
    assert!(!second.status.success());
    let record = artifact(&b);
    assert_eq!(record["wait"]["holder"]["mode"], "reserve", "{record}");
    assert!(record["wait"]["seconds_waited"].as_f64().unwrap() >= 1.0);
    assert!(String::from_utf8_lossy(&second.stderr).contains("asked at"));
}

#[test]
fn a_gate_refused_before_targets_start_still_writes_version_two_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    let home = temp.path().join("home");
    project(&a, false, true);
    project(&b, false, false);
    configure(&a, temp.path(), "a", false, false);
    configure(&b, temp.path(), "b", false, false);
    std::fs::write(temp.path().join("hold-exclusive"), "hold").unwrap();
    let first = run(&a, &home, "0");
    wait_started(temp.path());
    let second = finish(run_mode(&b, &home, "0", "essential"));
    std::fs::write(temp.path().join("release-exclusive"), "release").unwrap();
    assert_ok(&finish(first));
    assert!(!second.status.success());
    println!("refusal: {}", String::from_utf8_lossy(&second.stderr));
    let record = artifact(&b);
    assert_eq!(record["schema_version"], 2);
    assert_eq!(record["passed"], false);
    assert_eq!(record["complete"], false);
    assert!(record["wait"]["holder"]["pid"].is_number(), "{record}");
    assert!(record["wait"]["wait_started"].is_number());
    assert!(record["wait"]["acquired"].is_null());
    for field in [
        "revision",
        "git_tree",
        "tree",
        "config_digest",
        "selection",
        "targets",
        "results",
        "coverage",
        "tools",
        "environment",
    ] {
        assert!(
            record.get(field).is_some(),
            "missing schema-2 field {field}"
        );
    }
}
