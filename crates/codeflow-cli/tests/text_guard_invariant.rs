//! TSK-397, AC-2 (H-8): the built exec guard never allows a row the 3.2.0
//! line refuses, in either outcome.
//!
//! The corpus (`fixtures/text-guard-invariant/rows.json`) holds the Codex
//! and security round probes of PR 331 as the round 15 replay assembled
//! them, the 49-line everyday sample, the two shell review fixtures and the
//! three directory-change rows of the Fable review. Its `line` column is the
//! decision of the binary built from the integration line this branch last
//! merged, built in its own target directory and run by `generate.py` on
//! the fixture tree this test rebuilds; `line-binary.txt` records that
//! binary's version, source and digest. A merge of the line regenerates
//! it, since the relaxed outcome is the line's rule.
//! Each row is judged twice by this build: in the relaxed outcome (a Claude
//! payload in a checkout whose origin's landed policy sets
//! `security.text_guard: relaxed`; the column is still named `baseline`) and
//! in the strict outcome (the same payload as a recognised unsandboxed
//! retry, which relaxed keeps strict). Every judgement must exit 0
//! (allowed) or 2 (refused).
//!
//! The one exception is the H-3 class (issue 342), defined by
//! [`directory_change_only`]: a line made only of bare `cd`, `pushd` and
//! `popd` builtins writes nothing, so relaxed may allow it where the line
//! refused it. The strict outcome must still refuse it. The class is
//! written out here, apart from the guard's own predicate, so a widening of
//! the guard's predicate fails this test instead of passing as an exception.
//!
//! The baseline refuses more than the line only where a hardening item says
//! so: H-1 (a NUL byte), and the install check (TSK-1476), which refuses
//! under `git.hook_integrity` whatever `security.text_guard` says. The rows
//! that check refuses where the line allowed are listed one by one in
//! [`INSTALL_RUN_OVER_REFUSALS`]; each must still be one the line allowed and
//! the baseline must refuse it with the install finding as its only rule, so
//! a widening of the check fails this test and so does a stale entry.
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
        ("CLAUDE_PROJECT_DIR", t.join("repo")),
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
    // The committed policy chooses `relaxed`; it is honoured only from the
    // remote-tracking refs, so the commit is published as `origin/main`.
    git(&[
        "remote",
        "add",
        "origin",
        t.join("origin.git").to_str().unwrap(),
    ]);
    git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    std::fs::create_dir_all(t.join("cfhome")).unwrap();
}

/// The exec guard's exit code and message for one row, in the baseline
/// outcome or as a recognised retry (the tightened outcome).
fn judge(t: &Path, row: &Value, tightened: bool) -> (i32, String) {
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
        .stderr(Stdio::piped())
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
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Rows the line allowed that the install check refuses in the baseline, by
/// set and source. Each falls in an accepted over-refusal class the pull
/// request's table lists (AC-6).
const INSTALL_RUN_OVER_REFUSALS: &[(&str, &str)] = &[
    ("codex-rounds", "history.json:51"),
    ("codex-rounds", "history.json:52"),
    ("codex-rounds", "history.json:53"),
    ("codex-rounds", "history.json:54"),
    ("codex-rounds", "history.json:55"),
    ("codex-rounds", "history.json:57"),
    ("codex-rounds", "history.json:59"),
    ("codex-rounds", "history.json:897"),
    ("codex-rounds", "history.json:899"),
    ("codex-rounds", "history.json:900"),
    ("codex-rounds", "history.json:901"),
    ("codex-rounds", "history.json:902"),
    ("codex-rounds", "history.json:904"),
    ("codex-rounds", "history.json:905"),
    ("codex-rounds", "history.json:906"),
    ("codex-rounds", "history.json:962"),
    ("codex-rounds", "history.json:963"),
    ("codex-rounds", "history.json:1212"),
    ("codex-rounds", "history.json:1214"),
    ("codex-rounds", "history.json:1216"),
    ("codex-rounds", "history.json:1229"),
    ("codex-rounds", "history.json:1240"),
    ("codex-rounds", "controls.json:11"),
    ("codex-rounds", "controls.json:12"),
    ("codex-rounds", "closure.json:11"),
    ("shell-ordinary-review", "27"),
    ("shell-ordinary-review", "30"),
    ("shell-ordinary-review", "43"),
    ("shell-ordinary-review", "47"),
    ("shell-ordinary-review", "95"),
    ("shell-ordinary-review", "110"),
    ("shell-ordinary-review", "112"),
    ("shell-ordinary-review", "122"),
    ("shell-ordinary-review", "126"),
    ("shell-round-four-review", "17"),
    ("shell-round-four-review", "29"),
    ("shell-round-four-review", "53"),
    ("shell-round-four-review", "57"),
    ("shell-round-four-review", "58"),
    ("shell-round-four-review", "59"),
    ("shell-round-four-review", "60"),
    ("shell-round-four-review", "61"),
    ("shell-round-four-review", "62"),
    ("shell-round-four-review", "63"),
    ("shell-round-four-review", "65"),
    ("shell-round-four-review", "66"),
    ("shell-round-four-review", "67"),
    ("shell-round-four-review", "68"),
    ("shell-round-four-review", "71"),
    ("shell-round-four-review", "72"),
    ("shell-round-four-review", "335"),
];

/// A refusal whose only rule is the install check's finding.
fn install_finding_only(stderr: &str) -> bool {
    stderr.contains("policy rule git.hook_integrity (block)")
        && stderr.contains("possible install run")
        && stderr.matches("policy rule ").count() == 1
}

/// The H-3 class: every simple command is a bare `cd`, `pushd` or `popd`,
/// joined only by `&&`, `;` or newlines, and each argument is a literal
/// word or a plain `"$NAME"`. No other command, quoting, expansion or
/// operator appears.
fn directory_change_only(text: &str) -> bool {
    let literal = |word: &str| {
        !word.is_empty()
            && word.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(c, '/' | '.' | '_' | '-' | '~' | '+' | ':' | '@' | ',')
            })
    };
    let plain_variable = |word: &str| {
        word.strip_prefix("\"$")
            .and_then(|rest| rest.strip_suffix('"'))
            .is_some_and(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
    };
    let mut commands = 0;
    for command in text.split("&&").flat_map(|part| part.split([';', '\n'])) {
        let mut words = command.split([' ', '\t']).filter(|word| !word.is_empty());
        let Some(program) = words.next() else {
            continue;
        };
        if !matches!(program, "cd" | "pushd" | "popd")
            || !words.all(|word| literal(word) || plain_variable(word))
        {
            return false;
        }
        commands += 1;
    }
    commands > 0
}

#[test]
fn no_row_the_line_refuses_is_allowed_in_either_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path().canonicalize().unwrap();
    build(&t);
    let rows = read_json("rows.json");
    let rows = rows.as_array().unwrap();
    // Four workers, as the gate's test-thread cap allows.
    let judged: Vec<((i32, String), (i32, String))> = std::thread::scope(|scope| {
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
    let mut listed_seen = 0;
    for (row, ((baseline, baseline_stderr), (tightened, _))) in rows.iter().zip(&judged) {
        let line = row["line"].as_i64().unwrap();
        let listed = INSTALL_RUN_OVER_REFUSALS.contains(&(
            row["set"].as_str().unwrap(),
            row["source"].as_str().unwrap(),
        ));
        let install_over_refusal =
            listed && line == 0 && *baseline == 2 && install_finding_only(baseline_stderr);
        let shown = format!(
            "{} {}: {:?}",
            row["set"].as_str().unwrap(),
            row["source"].as_str().unwrap(),
            row["command"].as_str().unwrap()
        );
        for (outcome, code) in [("baseline", baseline), ("tightened", tightened)] {
            if !matches!(code, 0 | 2) {
                wrong.push(format!("the {outcome} outcome exited {code}: {shown}"));
            }
        }
        // The invariant: what the line refuses stays refused, apart from
        // the H-3 class in the baseline.
        let command = row["command"].as_str().unwrap();
        if line == 2 && *baseline != 2 && !directory_change_only(command) {
            wrong.push(format!(
                "the baseline allows a row the line refuses: {shown}"
            ));
        }
        if line == 2 && *tightened != 2 {
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
        // hardening item says so (H-1, a NUL byte; the install check, row by
        // row).
        if line == 0 && *baseline == 2 && !command.contains('\0') && !install_over_refusal {
            wrong.push(format!(
                "the baseline refuses a row the line allows: {shown}"
            ));
        }
        if listed {
            listed_seen += 1;
            if !install_over_refusal {
                wrong.push(format!(
                    "a listed install over-refusal is not one the line allowed and the \
                     baseline refuses with the install finding alone: {shown}"
                ));
            }
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
    assert_eq!(
        listed_seen,
        INSTALL_RUN_OVER_REFUSALS.len(),
        "a listed install over-refusal names no row of the corpus"
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
