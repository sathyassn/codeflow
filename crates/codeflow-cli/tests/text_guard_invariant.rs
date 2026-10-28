//! TSK-397 design D, AC-2 (H-8): the built exec guard never allows a row
//! the 3.2.0 line refuses, in either outcome.
//!
//! The corpus (`fixtures/text-guard-invariant/rows.json`) holds the Codex
//! and security round probes of PR 331 as the round 15 replay assembled
//! them, the 49-line everyday sample and the two shell review fixtures. Its
//! `line` column is the decision of the line binary at `5f2fe3827`, built in
//! its own target directory and run by `generate.py` on the fixture tree
//! this test rebuilds. Each row is judged twice by this build: in the
//! baseline outcome (a Claude payload on a host whose settings show the
//! shipped sandbox) and in the tightened outcome (the same payload as a
//! recognised unsandboxed retry).
//!
//! The paths are macOS paths and the column was recorded on macOS, so the
//! test runs there.
#![cfg(target_os = "macos")]

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/text-guard-invariant")
}

fn read_json(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixtures().join(name)).unwrap()).unwrap()
}

/// The environment `generate.py` gives every run.
fn environment(t: &Path) -> Vec<(&'static str, PathBuf)> {
    vec![
        ("HOME", t.join("home")),
        ("TMPDIR", t.to_path_buf()),
        ("SHELL", PathBuf::from("/bin/zsh")),
        ("PATH", PathBuf::from("/usr/bin:/bin")),
        ("CODEFLOW_HOME", t.join("cfhome")),
        ("CODEFLOW_TEST_CLAUDE_MANAGED_DIR", t.join("managed")),
        ("GIT_CONFIG_GLOBAL", PathBuf::from("/dev/null")),
        ("GIT_CONFIG_SYSTEM", PathBuf::from("/dev/null")),
        ("LANG", PathBuf::from("C.UTF-8")),
    ]
}

/// Rebuild the fixture tree under `t`, as `generate.py` does.
fn build(t: &Path) {
    let tree = read_json("tree.json");
    let mut committed = Vec::new();
    for entry in tree.as_array().unwrap() {
        let path = t.join(entry["path"].as_str().unwrap());
        match entry["kind"].as_str().unwrap() {
            "dir" => std::fs::create_dir_all(&path).unwrap(),
            "link" => {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::os::unix::fs::symlink(entry["target"].as_str().unwrap(), &path).unwrap();
            }
            _ => {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, entry["text"].as_str().unwrap()).unwrap();
                if entry["exec"].as_bool().unwrap() {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                        .unwrap();
                }
            }
        }
        if entry["committed"].as_bool().unwrap() {
            let rel = entry["path"]
                .as_str()
                .unwrap()
                .strip_prefix("repo/")
                .unwrap();
            committed.push(rel.to_string());
        }
    }
    let repo = t.join("repo");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/settings/default.json"),
        repo.join(".claude/settings.json"),
    )
    .unwrap();
    std::fs::write(repo.join(".claude/settings.local.json"), "{}\n").unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .envs(environment(t))
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    };
    git(&["init", "-q", "-b", "task/invariant"]);
    let mut add = vec!["add", "--"];
    add.extend(committed.iter().map(String::as_str));
    git(&add);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.invalid",
        "commit",
        "-q",
        "-m",
        "test: fixture",
    ]);
    std::fs::create_dir_all(t.join("cfhome")).unwrap();
}

/// The exec guard's exit code for one row, in the baseline outcome or as a
/// recognised retry (the tightened outcome).
fn judge(t: &Path, row: &Value, tightened: bool) -> i32 {
    let at = t.to_str().unwrap();
    let command = row["command"].as_str().unwrap().replace("@T@", at);
    let cwd = row["cwd"].as_str().unwrap().replace("@T@", at);
    let mut tool_input = json!({ "command": command });
    if tightened {
        tool_input["dangerouslyDisableSandbox"] = json!(true);
    }
    let payload = json!({
        "tool_name": "Bash",
        "tool_input": tool_input,
        "cwd": cwd,
        "prompt_id": "invariant",
        "scratchpad_dir": t.join("scratch"),
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(["hook", "exec-guard", "--contract", "3"])
        .current_dir(&cwd)
        .env_clear()
        .envs(environment(t))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
    }
    child.wait().unwrap().code().unwrap_or(-1)
}

#[test]
fn no_row_the_line_refuses_is_allowed_in_either_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path().canonicalize().unwrap();
    build(&t);
    let rows = read_json("rows.json");
    let rows = rows.as_array().unwrap();
    let exceptions = read_json("exceptions.json");
    let excepted = |row: &Value, outcome: &str| {
        exceptions.as_array().unwrap().iter().any(|e| {
            e["source"] == row["source"]
                && e["command"] == row["command"]
                && e["allowed_in"].as_str() == Some(outcome)
        })
    };
    // Four workers, as the gate's test-thread cap allows.
    let judged: Vec<(i32, i32)> = std::thread::scope(|scope| {
        let chunk = rows.len().div_ceil(4);
        let workers: Vec<_> = rows
            .chunks(chunk)
            .map(|part| {
                let t = &t;
                scope.spawn(move || {
                    part.iter()
                        .map(|row| (judge(t, row, false), judge(t, row, true)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect()
    });
    let mut wrong = Vec::new();
    let mut distinct = 0;
    for (row, (baseline, tightened)) in rows.iter().zip(&judged) {
        let line = row["line"].as_i64().unwrap();
        let shown = format!(
            "{} {}: {:?}",
            row["set"].as_str().unwrap(),
            row["source"].as_str().unwrap(),
            row["command"].as_str().unwrap()
        );
        // The invariant: what the line refuses stays refused.
        if line == 2 && *baseline != 2 && !excepted(row, "baseline") {
            wrong.push(format!(
                "the baseline allows a row the line refuses: {shown}"
            ));
        }
        if line == 2 && *tightened != 2 && !excepted(row, "tightened") {
            wrong.push(format!(
                "the tightened outcome allows a row the line refuses: {shown}"
            ));
        }
        // The tightened outcome keeps every baseline refusal.
        if *baseline == 2 && *tightened != 2 {
            wrong.push(format!(
                "the tightened outcome allows a baseline refusal: {shown}"
            ));
        }
        // The baseline is the line's rule: it refuses more only where a
        // hardening item says so (H-1, a NUL byte).
        if line == 0 && *baseline == 2 && !row["command"].as_str().unwrap().contains('\0') {
            wrong.push(format!(
                "the baseline refuses a row the line allows: {shown}"
            ));
        }
        if *baseline == 0 && *tightened == 2 {
            distinct += 1;
        }
    }
    // The two payloads select different outcomes: the harness is not
    // judging one outcome twice.
    assert!(
        distinct > 100,
        "only {distinct} rows differ between the outcomes"
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
