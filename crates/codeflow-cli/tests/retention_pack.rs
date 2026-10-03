//! TSK-130: the guidance-retention pack materialized from the candidate
//! under test. The eval kit runs the real Cargo-built binary's
//! `init --yes --standard` for every retention case, so the fixture carries
//! the rule map (TSK-127) and the re-injection hooks (TSK-128); the
//! after-compaction arm adds only the fixture-local compaction window, and the
//! turn script stays outside the subject tree. Live sessions are not run here.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

const GUIDANCE_HEADING: &str = "## Rules after compaction or resume";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn eval_kit() -> PathBuf {
    repo_root().join("assets/base/agents/skills/cf-evaluate-model/scripts/eval_kit.py")
}

fn python(args: &[&str], home: &Path) -> Output {
    Command::new("python3")
        .arg("-B")
        .arg(eval_kit())
        .args(args)
        .current_dir(repo_root())
        .env("CODEFLOW_HOME", home)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("eval kit runs")
}

fn hook(dir: &Path, home: &Path, payload: &Value) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(["hook", "session-orient"])
        .current_dir(dir)
        .env("CODEFLOW_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("hook runs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("payload written");
    let out = child.wait_with_output().expect("hook exits");
    assert_eq!(out.status.code(), Some(0), "the advisory hook never fails");
    String::from_utf8(out.stdout).expect("utf-8 hook output")
}

fn pack_cases(home: &Path) -> Vec<String> {
    let out = python(&["list-cases", "--pack", "guidance-retention"], home);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("utf-8 case list")
        .lines()
        .map(str::to_string)
        .collect()
}

/// AC-3 and AC-4: each retention fixture, in both arms, materializes from the
/// candidate at the standard tier with the map and hooks; the compaction
/// window lives only in the after-compaction fixture, and the re-injection
/// hook prints the rules on a compaction payload inside it. The hard probes
/// cover every fixture and arm (a paired negative shares its hard probe's
/// fixture); the eval kit's own tests materialize all twelve cases.
#[test]
fn retention_fixtures_come_from_the_candidate_with_map_hooks_and_local_window() {
    let home = tempfile::tempdir().expect("home");
    let run = tempfile::tempdir().expect("run parent");
    let run_root = run.path().join("run");
    let cases = pack_cases(home.path());
    assert_eq!(cases.len(), 12, "six probes in two arms");
    for case in [
        "retention-estimate-in-plan-fresh",
        "retention-estimate-in-plan-after-compaction",
        "retention-status-outcomes-first-fresh",
        "retention-status-outcomes-first-after-compaction",
        "retention-release-flow-shown-fresh",
        "retention-release-flow-shown-after-compaction",
    ] {
        assert!(
            cases.iter().any(|listed| listed == case),
            "{case} left the pack"
        );
        let record = materialize(case, &run_root, home.path());
        let fixture = PathBuf::from(record["path"].as_str().expect("fixture path"));
        assert_scaffold_carries_map_and_hooks(case, &record, &fixture);
        let plan = &record["session"];
        let settings = fixture.join(".claude/settings.local.json");
        if plan["arm"] == "fresh" {
            assert_eq!(plan["warmup"], serde_json::json!([]), "{case}");
            assert!(plan["compaction"].is_null(), "{case}");
            assert!(!settings.exists(), "{case}: the fresh arm sets no window");
        } else {
            assert_eq!(plan["arm"], "after-compaction", "{case}");
            assert_local_window_and_reinjection(case, plan, &fixture, home.path());
        }
        assert_plan_stays_outside_the_subject(case, plan, &fixture);
    }
}

fn materialize(case: &str, run_root: &Path, home: &Path) -> Value {
    let out = python(
        &[
            "materialize",
            "--run-root",
            run_root.to_str().expect("utf-8 path"),
            "--case",
            case,
            "--trial",
            "1",
            "--codeflow",
            env!("CARGO_BIN_EXE_codeflow"),
        ],
        home,
    );
    assert!(
        out.status.success(),
        "{case}: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("trial record")
}

fn assert_scaffold_carries_map_and_hooks(case: &str, record: &Value, fixture: &Path) {
    assert_eq!(
        record["guidance_wiring"],
        serde_json::json!({
            "rule_map": true,
            "compact_reinjection": true,
            "prompt_reminder": true
        }),
        "{case}"
    );
    let project =
        std::fs::read_to_string(fixture.join(".codeflow/project.toml")).expect("project.toml");
    assert!(project.contains("standard"), "{case}: {project}");
    let agents = std::fs::read_to_string(fixture.join("AGENTS.md")).expect("AGENTS.md");
    // TSK-184: the map carries these duties as moment rows.
    assert!(
        agents.contains("| give a duration, date or effort |"),
        "{case}"
    );
    assert!(
        agents
            .contains("| report status or hand off | each item by its outcome in words, IDs after"),
        "{case}"
    );
    assert!(agents.contains("| show something complex |"), "{case}");
    assert!(!fixture.join("TASK.md").exists(), "{case}: no TASK.md");
}

fn assert_local_window_and_reinjection(case: &str, plan: &Value, fixture: &Path, home: &Path) {
    let settings = fixture.join(".claude/settings.local.json");
    let local: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).expect("local settings"))
            .expect("local settings JSON");
    assert_eq!(
        local,
        serde_json::json!({"env": {
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "100000",
            "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "100"
        }}),
        "{case}: the window and the pinned percentage"
    );
    // The pin matters because the scaffold's shared settings lower the
    // percentage by default (TSK-211).
    let shared: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.join(".claude/settings.json")).expect("shared settings"),
    )
    .expect("shared settings JSON");
    assert_eq!(
        shared["env"]["CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"], "50",
        "{case}: the project default the pin overrides"
    );
    let tracked = Command::new("git")
        .args(["ls-files", ".claude/settings.local.json"])
        .current_dir(fixture)
        .output()
        .expect("git ls-files");
    assert!(
        tracked.stdout.is_empty(),
        "{case}: the window stays untracked"
    );

    // TSK-128's hook, as wired in this fixture, re-injects the rules after
    // compaction: the path the live arm depends on.
    let block = hook(
        fixture,
        home,
        &serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "SessionStart",
            "source": "compact"
        }),
    );
    assert!(block.contains(GUIDANCE_HEADING), "{case}: {block}");
    // TSK-184: the duty is a moment row of the map, re-injected as such.
    assert!(
        block.contains("give a duration, date or effort: /cf-estimate"),
        "{block}"
    );
    assert!(block.contains("cf-present"), "{block}");
    let reminder = hook(
        fixture,
        home,
        &serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "UserPromptSubmit",
            "prompt": plan["probe_prompt"]
        }),
    );
    let reminder = reminder.trim();
    println!(
        "{case}: probe reminder {}",
        if reminder.is_empty() {
            "none".to_string()
        } else {
            format!("{} bytes", reminder.len())
        }
    );
}

/// The turn plan is evaluator-only: no scripted turn reaches the subject.
fn assert_plan_stays_outside_the_subject(case: &str, plan: &Value, fixture: &Path) {
    let probe = plan["probe_prompt"].as_str().expect("probe");
    for entry in walk(fixture) {
        let text = std::fs::read_to_string(&entry).unwrap_or_default();
        assert!(
            !text.contains(probe),
            "{case}: probe in {}",
            entry.display()
        );
        for turn in plan["warmup"].as_array().expect("warm-up") {
            assert!(
                !text.contains(turn.as_str().expect("turn")),
                "{case}: warm-up turn in {}",
                entry.display()
            );
        }
    }
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

/// Windows has no mode bits, so the eval kit makes the evaluator key and its
/// folder owner-only through their access lists, writes the key only once
/// both are proven private, and refuses it once either lets in another
/// account.
#[cfg(windows)]
#[test]
fn the_windows_evaluator_key_is_refused_once_others_can_reach_it() {
    let home = tempfile::tempdir().expect("home");
    let judgements = home.path().join("judgements.json");
    let record = || {
        python(
            &[
                "record-judgement",
                "--judgements",
                judgements.to_str().expect("utf-8 path"),
                "--assertion",
                "a",
                "--excerpt-digest",
                &format!("sha256:{}", "1".repeat(64)),
                "--verdict",
                "pass",
                "--judge",
                "human: ana",
                "--judge-config",
                "reads the rubric",
                "--rationale",
                "names PAY-12",
            ],
            home.path(),
        )
    };
    let folder = home.path().join("eval");
    let icacls = |target: &Path, args: &[&str]| {
        let out = Command::new("icacls")
            .arg(target)
            .args(args)
            .output()
            .expect("icacls runs");
        assert!(out.status.success(), "{out:?}");
    };
    // A folder that passes read access to everyone on what it holds: the
    // kit's own grant cannot remove that entry, so no key is written.
    std::fs::create_dir_all(&folder).expect("key folder");
    icacls(&folder, &["/grant", "*S-1-1-0:(OI)(IO)R"]);
    let refused = record();
    assert!(
        !refused.status.success(),
        "an inheritable grant to everyone"
    );
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("open to other accounts (S-1-1-0)"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(!folder.join("judgement.key").exists());
    icacls(&folder, &["/remove:g", "*S-1-1-0"]);
    let made = record();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    for target in [folder.join("judgement.key"), folder] {
        icacls(&target, &["/grant", "*S-1-1-0:R"]);
        let refused = record();
        assert!(!refused.status.success(), "{target:?} open to everyone");
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("open to other accounts (S-1-1-0)"),
            "{}",
            String::from_utf8_lossy(&refused.stderr)
        );
        icacls(&target, &["/remove:g", "*S-1-1-0"]);
    }
    assert!(record().status.success());
}
