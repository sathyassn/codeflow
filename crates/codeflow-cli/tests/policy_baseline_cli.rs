//! TSK-1057: `codeflow policy baseline` with the built binary on a scratch
//! repository. The writers refuse in an agent session and write nothing;
//! `show` and doctor report a repository with no baseline. Recording a
//! baseline needs a terminal outside every agent session, which a test run
//! from a session cannot supply; the enforcement is tested through the
//! core API with a temporary store (`policy_baseline_tests.rs`).
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const MARKERS: [&str; 5] = [
    "CLAUDECODE",
    "CODEX_SESSION_ID",
    "CODEX_THREAD_ID",
    "CODEX_CI",
    "GROK_SESSION_ID",
];

struct Scratch {
    temp: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Self {
        let scratch = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        let remote = scratch.temp.path().join("remote.git");
        std::fs::create_dir(scratch.work()).unwrap();
        scratch.git(&["init", "-q", "-b", "main"]);
        scratch.git(&["config", "user.name", "Test"]);
        scratch.git(&["config", "user.email", "test@example.invalid"]);
        scratch.git(&[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ]);
        scratch.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        std::fs::create_dir_all(scratch.work().join(".codeflow")).unwrap();
        std::fs::write(
            scratch.work().join(".codeflow/policy.json"),
            r#"{"git":{"push_to_protected":"block"}}"#,
        )
        .unwrap();
        scratch.git(&["add", ".codeflow/policy.json"]);
        scratch.git(&["commit", "-q", "-m", "chore: adopt codeflow"]);
        scratch.git(&["push", "-q", "origin", "main"]);
        scratch.git(&["remote", "set-head", "origin", "-a"]);
        scratch
    }

    fn work(&self) -> PathBuf {
        self.temp.path().join("work")
    }

    fn command(&self, program: &Path) -> Command {
        let home = self.temp.path();
        let mut c = Command::new(program);
        c.current_dir(self.work())
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("CODEFLOW_HOME", home.join("state"))
            .env("GIT_CONFIG_GLOBAL", home.join("global.config"))
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        c
    }

    fn git(&self, args: &[&str]) {
        let out = self.command(Path::new("git")).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn codeflow(&self, args: &[&str], marked: bool) -> Output {
        let mut command = self.command(Path::new(env!("CARGO_BIN_EXE_codeflow")));
        for marker in MARKERS {
            command.env_remove(marker);
        }
        if marked {
            command.env("CLAUDECODE", "1");
        }
        command.args(args).stdin(Stdio::null()).output().unwrap()
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn the_writers_refuse_in_an_agent_session_and_write_nothing() {
    let scratch = Scratch::new();
    for args in [
        ["policy", "baseline", "init"].as_slice(),
        [
            "policy",
            "baseline",
            "init",
            "--confirm",
            "0000000000000000000000000000000000000000",
        ]
        .as_slice(),
        [
            "policy", "baseline", "refresh", "--reset", "--reason", "test",
        ]
        .as_slice(),
    ] {
        let out = scratch.codeflow(args, true);
        let text = said(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {text}");
        assert!(text.contains("CLAUDECODE is set"), "{args:?}: {text}");
        assert!(!text.contains("written:"), "{text}");
        // Without the marker, the harness ancestor or the missing terminal
        // still refuses: this test runs from a session and from no terminal.
        let out = scratch.codeflow(args, false);
        let text = said(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {text}");
        assert!(
            text.contains("an agent harness") || text.contains("not a terminal"),
            "{args:?}: {text}"
        );
        assert!(!text.contains("written:"), "{text}");
    }
}

#[test]
fn show_and_doctor_report_a_repository_with_no_baseline() {
    let scratch = Scratch::new();
    let out = scratch.codeflow(&["policy", "baseline", "show"], true);
    let text = said(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("baseline: none for this repository"),
        "{text}"
    );
    assert!(
        text.contains("policy source: refs/remotes/origin/main"),
        "{text}"
    );
    let out = scratch.codeflow(&["doctor", "--check", "policy-source"], true);
    let text = said(&out);
    assert!(
        text.contains("policy baseline: none for this repository"),
        "{text}"
    );
}
