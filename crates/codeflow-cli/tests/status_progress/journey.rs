//! The AC-9 journey: a project adopted with `codeflow init --full --yes`,
//! one epic with five tasks at five stages, two worktrees and a fake
//! forge, read by `status --epic` as text and as JSON that validates
//! against the schema the adoption installed.
use super::*;

/// Git with no user configuration, no hooks and a fixed identity. The
/// adopted project wires `.codeflow/git-hooks`, which would run whatever
/// `codeflow` the test's `PATH` holds, so the fixture's own commits skip
/// them; the command under test never commits.
fn adopted_git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.test")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.test")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}

/// TSK-001 not started; TSK-002 in progress in one worktree; TSK-003 in
/// review in a second worktree, its pull request's required check red;
/// TSK-004 landed; TSK-005 cancelled. TSK-003 depends on TSK-001.
fn adopted() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let root = base.join("demo");
    let bin = base.join("bin");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let init = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(["init", "--full", "--yes"])
        .current_dir(&root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.test")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.test")
        .output()
        .unwrap();
    assert!(init.status.success(), "init: {}", text(&init));
    // With no user configuration git names the first branch `master`;
    // this project's default branch is `main`.
    adopted_git(&root, &["branch", "-m", "main"]);
    write(&root, "project-management/epics/EPC-001.md", EPIC);
    for (id, status, depends, extra) in [
        ("TSK-001", "todo", "", ""),
        ("TSK-002", "todo", "", "theme: Speed\n"),
        ("TSK-003", "todo", "TSK-001", ""),
        ("TSK-004", "complete", "", ""),
        ("TSK-005", "cancelled", "", ""),
    ] {
        write(
            &root,
            &format!("project-management/tasks/{id}.md"),
            &task_record(id, status, depends, extra),
        );
    }
    adopted_git(&root, &["add", "project-management"]);
    adopted_git(&root, &["commit", "-qm", "docs: plan the demo epic"]);
    adopted_git(&base, &["init", "-q", "--bare", "origin.git"]);
    let origin = base.join("origin.git");
    adopted_git(
        &root,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    adopted_git(&root, &["push", "-q", "origin", "main"]);
    adopted_git(&root, &["fetch", "-q", "origin"]);
    for id in ["TSK-002", "TSK-003"] {
        let branch = format!("task/{id}-work");
        adopted_git(&root, &["switch", "-qc", &branch]);
        write(&root, &format!("src/{id}.rs"), "pub fn work() {}\n");
        adopted_git(&root, &["add", "src"]);
        adopted_git(&root, &["commit", "-qm", "feat: build the outcome"]);
        adopted_git(&root, &["switch", "-q", "main"]);
        let path = root.join(format!(".worktrees/{}", id.to_lowercase()));
        adopted_git(
            &root,
            &["worktree", "add", "-q", path.to_str().unwrap(), &branch],
        );
    }
    adopted_git(
        &root,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/acme/demo.git",
        ],
    );
    let fixture = Fixture {
        log: base.join("gh.log"),
        _dir: dir,
        root,
        bin,
    };
    let head = adopted_git(&fixture.root, &["rev-parse", "task/TSK-003-work"]);
    fixture.gh(&PULLS.replace("TSK003_HEAD", &head));
    fixture
}

#[test]
fn an_adopted_project_reads_every_view_and_validates() {
    let fx = adopted();
    let output = fx.cli(&["status", "--epic", "EPC-001"]);
    let out = text(&output);
    assert!(output.status.success(), "{out}");
    for needle in [
        "Ladder: not started 0%, in progress 20%, in review 50%, reviewed 75%, landed 100%",
        "Overall: 42.5%",
        "path to main:",
        "Order (critical path TSK-001 > TSK-003)",
        "Workers (observed)",
        "Windows (observed, 2 Worker(s))",
        "intake stops:",
        "intake continues:",
        "TSK-003 required check red",
        "checked 2026-11-01T12:00:00Z",
    ] {
        assert!(out.contains(needle), "{needle}: {out}");
    }

    let value = fx.json(&["--epic", "EPC-001"]);
    let stages: BTreeMap<&str, &str> = ["TSK-001", "TSK-002", "TSK-003", "TSK-004", "TSK-005"]
        .iter()
        .map(|id| (*id, task(&value, id)["stage"].as_str().unwrap()))
        .collect();
    assert_eq!(stages["TSK-001"], "not started");
    assert_eq!(stages["TSK-002"], "in progress");
    assert_eq!(stages["TSK-003"], "in review");
    assert_eq!(stages["TSK-004"], "landed");
    assert_eq!(stages["TSK-005"], "cancelled");
    assert_eq!(value["scope"]["percent"], 42.5);
    assert_eq!(task(&value, "TSK-004")["location"]["kind"], "main");
    assert_eq!(task(&value, "TSK-003")["location"]["kind"], "pull request");
    assert_eq!(task(&value, "TSK-002")["location"]["kind"], "branch");
    assert_eq!(value["workers"]["source"], "observed");
    let red = value["issues"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["kind"] == "required check red")
        .expect("the red check is an issue");
    assert_eq!(red["subject"], "TSK-003");

    // The schema the adoption installed, which is the shipped one.
    let installed = std::fs::read(
        fx.root
            .join(".codeflow/schemas/status/report-v1.schema.json"),
    )
    .unwrap();
    assert_eq!(fnv1a(&installed), SCHEMA_DIGEST);
    let schema: Value = serde_json::from_slice(&installed).unwrap();
    let errors = validate(&schema, &schema, &value, "$");
    assert!(errors.is_empty(), "{errors:#?}");
}
