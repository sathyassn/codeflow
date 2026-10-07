//! Disposable history for epic-line adoption tests. No installed Git hooks;
//! the hook tests drive the real pre-push entry point with Git's ref payload.
#![allow(dead_code)]
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

pub const LINE: &str = "integration/EPC-001-outcome";
pub const EPIC_PATH: &str = "project-management/epics/EPC-001.md";
pub const ZERO: &str = "0000000000000000000000000000000000000000";
const EPIC: &str = "---\nid: EPC-001\ntitle: Outcome\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-10-06\n---\n\n# Outcome\n\n## Summary\n\nDeliver work.\n\n## Acceptance Criteria\n\n- AC-1 Work runs.\n";

pub struct Line {
    pub dir: tempfile::TempDir,
    home: tempfile::TempDir,
}
impl Line {
    pub fn new() -> Self {
        let f = Self {
            dir: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
        };
        f.git(&["init", "-qb", "main"]);
        f.git(&["config", "user.name", "Test"]);
        f.git(&["config", "user.email", "test@example.test"]);
        f.write(EPIC_PATH, EPIC);
        f.write("project-management/tasks/TSK-001.md", &format!("---\nid: TSK-001\nepic_id: EPC-001\ntitle: Work\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\nintegration_target: {LINE}\ncreated: 2026-10-06\n---\n\n# Work\n\n## Description\n\nDeliver work.\n\n## Acceptance Criteria\n\n- AC-1 Work runs.\n- AC-2 The command succeeds. (journey)\n"));
        f.write(".codeflow/policy.json", "{\"schema_version\":1,\"git\":{\"product_paths\":[\"src/**\"],\"test_gate_on_push\":\"off\"}}\n");
        f.write("src/lib.rs", "pub fn base() {}\n");
        f.commit("chore: plan work");
        f.git(&["init", "--bare", "-q", "-b", "main", ".git/destination.git"]);
        f.git(&[
            "remote",
            "add",
            "origin",
            f.root().join(".git/destination.git").to_str().unwrap(),
        ]);
        f.git(&["switch", "-qc", LINE]);
        f
    }
    pub fn root(&self) -> &Path {
        self.dir.path()
    }
    pub fn command(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        c.current_dir(self.root())
            .env("CODEFLOW_HOME", self.home.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "CODEFLOW_PR_BODY",
            "GITHUB_EVENT_NAME",
            "GITHUB_HEAD_REF",
            "GITHUB_BASE_REF",
            "CI_PIPELINE_SOURCE",
            "CI_MERGE_REQUEST_IID",
            "BITBUCKET_PR_ID",
            "CODEFLOW_INTEGRATE_TOKEN",
            "CODEFLOW_HUMAN_OVERRIDE",
        ] {
            c.env_remove(key);
        }
        c
    }
    pub fn git(&self, args: &[&str]) -> String {
        let o = self.command("git").args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", output(&o));
        String::from_utf8(o.stdout).unwrap().trim().into()
    }
    pub fn write(&self, path: &str, text: &str) {
        let p = self.root().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    pub fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", message]);
        self.git(&["rev-parse", "HEAD"])
    }
    pub fn land_task(&self) {
        self.git(&["switch", "-qc", "task/TSK-001-work"]);
        self.write("src/lib.rs", "pub fn work() {}\n");
        let path = "project-management/tasks/TSK-001.md";
        let record = std::fs::read_to_string(self.root().join(path))
            .unwrap()
            .replace("AC-1 Work runs.", "AC-1 The planned work runs.");
        self.write(path, &record);
        let reviewed = self.commit("feat: implement planned work");
        self.write(path, &format!("{}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review/2\n  criteria:\n    AC-1: verified | focused test\n    AC-2: verified | journey\n  journey: verified | exercised command\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n", record.replace("status: todo", "status: complete")));
        self.commit("docs: complete planned work");
        self.git(&["switch", "-q", LINE]);
        self.merge("task/TSK-001-work");
    }
    pub fn direct(&self) -> String {
        self.write("src/lib.rs", "pub fn direct() {}\n");
        self.commit("fix: repair false positive")
    }
    pub fn adoption(&self, oid: &str) {
        self.write(EPIC_PATH, &EPIC.replacen("---\n", &format!("---\nline_adoptions:\n  - commit: '{oid}'\n    reason: Repair false positive\n    review: https://example.test/review/1\n"), 1));
    }
    pub fn merge(&self, branch: &str) {
        self.git(&[
            "merge",
            "--no-ff",
            "-qm",
            "chore: land reviewed work",
            branch,
        ]);
    }
    pub fn land_adoption(&self, oid: &str) {
        self.git(&["switch", "-qc", "plan/adopt"]);
        self.adoption(oid);
        self.commit("docs: adopt direct commit");
        self.git(&["switch", "-q", LINE]);
        self.merge("plan/adopt");
    }
    pub fn ci(&self, base: &str, branch: &str) -> Output {
        self.command(env!("CARGO_BIN_EXE_codeflow")).args(["ci", "--base", base, "--head", "HEAD", "--branch", branch, "--pr-body", "## Summary\nRepair the line.\n\n- adopt reviewed work\n\nTask: EPC-001\n\n## Changes\n- repair\n\n## Testing\n- fixture\n"]).output().unwrap()
    }
    pub fn push(&self, old: &str) -> Output {
        let head = self.git(&["rev-parse", "HEAD"]);
        let mut child = self
            .command(env!("CARGO_BIN_EXE_codeflow"))
            .args([
                "git-hook",
                "pre-push",
                "origin",
                self.root().join(".git/destination.git").to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(
            child.stdin.take().unwrap(),
            "refs/heads/{LINE} {head} refs/heads/{LINE} {old}"
        )
        .unwrap();
        child.wait_with_output().unwrap()
    }
}
pub fn output(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}
pub fn passes(o: &Output) {
    assert!(o.status.success(), "{}", output(o));
}
pub fn blocks(o: &Output, text: &str) {
    assert_eq!(o.status.code(), Some(1), "{}", output(o));
    assert!(output(o).contains(text), "missing {text}: {}", output(o));
}
