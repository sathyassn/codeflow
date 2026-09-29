use super::*;

struct History {
    local: tempfile::TempDir,
    remote: tempfile::TempDir,
}

impl History {
    fn new() -> Self {
        let history = Self {
            local: tempfile::tempdir().unwrap(),
            remote: tempfile::tempdir().unwrap(),
        };
        history.git(&["init", "-q", "-b", "main"]);
        history.git(&["config", "user.name", "Test"]);
        history.git(&["config", "user.email", "test@example.invalid"]);
        history.commit("base.txt", "base", "chore: base");
        command(history.remote.path(), &["init", "--bare", "-q"]);
        history.advertise("main", "main");
        history
    }

    fn git(&self, args: &[&str]) -> String {
        command(self.local.path(), args)
    }

    fn commit(&self, path: &str, text: &str, message: &str) -> String {
        let file = self.local.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
        self.git(&["add", path]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn advertise(&self, revision: &str, branch: &str) {
        command(
            self.remote.path(),
            &[
                "fetch",
                "-q",
                self.local.path().to_str().unwrap(),
                &format!("{revision}:refs/heads/{branch}"),
            ],
        );
    }

    fn task(&self, target: &str) {
        self.commit(
            "project-management/tasks/TSK-001.md",
            &format!("---\nid: TSK-001\nintegration_target: {target}\n---\n"),
            "chore: record target",
        );
    }

    fn range(&self, branch: &str, head: &str, old: &str) -> RangeBase {
        let advertised = OnceCell::new();
        let destination = Destination {
            url: self.remote.path().to_str(),
            advertised: &advertised,
            namespace: None,
            policy: &GitPolicy::default(),
        };
        let pushed = PushRef {
            local_ref: format!("refs/heads/{branch}"),
            local_sha: head.to_string(),
            remote_ref: format!("refs/heads/{branch}"),
            remote_sha: old.to_string(),
        };
        range_base(self.local.path(), &pushed, &destination).unwrap()
    }
}

fn command(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

const NEW: &str = "0000000000000000000000000000000000000000";

#[test]
fn push_set_landing_extends_advertised_line_not_first_boundary() {
    for branch in ["integration/line", "main"] {
        let h = History::new();
        let old = h.git(&["rev-parse", "HEAD"]);
        h.advertise(&old, branch);
        h.git(&["checkout", "-q", "-b", "task/a"]);
        h.commit("a", "a", "feat: a");
        h.advertise("HEAD", "task/a");
        h.git(&["checkout", "-q", "-b", "task/b", &old]);
        h.commit("b", "b", "feat: b");
        h.advertise("HEAD", "task/b");
        h.git(&["checkout", "-q", "-b", "candidate", &old]);
        h.git(&["merge", "--no-ff", "-m", "feat: land a", "task/a"]);
        h.git(&["merge", "--no-ff", "-m", "feat: land b", "task/b"]);
        let head = h.git(&["rev-parse", "HEAD"]);
        assert_eq!(h.range(branch, &head, &old).base, old, "{branch}");
    }
}

#[test]
fn push_set_task_that_merged_line_uses_target_not_previous_push() {
    let h = History::new();
    h.git(&["checkout", "-q", "-b", "integration/line"]);
    h.task("integration/line");
    h.advertise("HEAD", "integration/line");
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
    let old = h.commit("own", "own", "feat: own work");
    h.advertise(&old, "task/TSK-001-change");
    h.git(&["checkout", "-q", "integration/line"]);
    let target = h.commit("line", "line", "feat: line advances");
    h.advertise(&target, "integration/line");
    h.git(&["checkout", "-q", "task/TSK-001-change"]);
    h.git(&[
        "merge",
        "--no-ff",
        "-m",
        "feat: incorporate line",
        "integration/line",
    ]);
    let head = h.git(&["rev-parse", "HEAD"]);
    // A different checkout and an uncommitted target must not change the push.
    h.git(&["checkout", "-q", "main"]);
    let path = h.local.path().join("project-management/tasks/TSK-001.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "---\nid: TSK-001\nintegration_target: main\n---\n").unwrap();
    let range = h.range("task/TSK-001-change", &head, &old);
    assert_eq!(range.base, target);
    assert!(range.note.unwrap().text.contains("integration/line"));
}

#[test]
fn push_set_new_task_with_multiple_boundaries_uses_declared_target() {
    let h = History::new();
    let target = h.git(&["rev-parse", "HEAD"]);
    h.advertise(&target, "integration/line");
    h.git(&["checkout", "-q", "-b", "sibling"]);
    h.commit("sibling", "sibling", "feat: sibling");
    h.advertise("HEAD", "task/sibling");
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change", &target]);
    h.task("integration/line");
    h.git(&["merge", "--no-ff", "-m", "feat: use sibling", "sibling"]);
    let head = h.git(&["rev-parse", "HEAD"]);
    let range = h.range("task/TSK-001-change", &head, NEW);
    assert_eq!(range.base, target);
    assert!(range.note.unwrap().text.contains("integration/line"));
}

#[test]
fn push_set_target_base_keeps_every_destination_missing_commit() {
    let h = History::new();
    let common = h.git(&["rev-parse", "HEAD"]);
    h.git(&["checkout", "-q", "-b", "integration/line"]);
    let advertised = h.commit("line", "line", "feat: target diverges");
    h.advertise(&advertised, "integration/line");
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change", &common]);
    h.task("integration/line");
    let missing = h.commit("missing", "missing", "Invalid commit subject");
    let head = h.commit("later", "later", "feat: later work");
    let range = h.range("task/TSK-001-change", &head, NEW);
    assert_eq!(range.base, common);
    // Cutting at the pushed branch's own earlier commit would lose this defect.
    assert!(!h
        .git(&["rev-list", &format!("{missing}..{head}")])
        .lines()
        .any(|s| s == missing));
    let checked = h.git(&["rev-list", &format!("{}..{head}", range.base)]);
    let absent = h.git(&["rev-list", &head, "--not", &advertised, &common]);
    assert!(checked.lines().any(|s| s == missing));
    assert!(absent.lines().all(|s| checked.lines().any(|c| c == s)));
    h.git(&["merge-base", "--is-ancestor", &range.base, &advertised]);
    assert!(range.note.unwrap().text.contains("integration/line"));
}

#[test]
fn push_set_ordinary_branch_uses_policy_target() {
    let h = History::new();
    let target = h.git(&["rev-parse", "HEAD"]);
    h.git(&["checkout", "-q", "-b", "feat/change"]);
    let old = h.commit("old", "old", "feat: first push");
    h.advertise(&old, "feat/change");
    let head = h.commit("new", "new", "feat: second push");
    let range = h.range("feat/change", &head, &old);
    assert_eq!(range.base, target);
    assert!(range.note.unwrap().text.contains("main"));
}

#[test]
fn push_set_line_rewrites_keep_advertised_history_fallback() {
    for branch in ["main", "integration/line"] {
        let h = History::new();
        let common = h.git(&["rev-parse", "HEAD"]);
        let old = h.commit("old", "old", "feat: old line");
        h.advertise(&old, branch);
        h.git(&["checkout", "-q", "-b", "rewrite", &common]);
        let head = h.commit("new", "new", "feat: rewritten line");
        let range = h.range(branch, &head, &old);
        assert_eq!(range.base, common);
        let note = range.note.unwrap().text;
        assert!(note.contains("rewrites the destination"));
        assert!(!note.contains("uses advertised target"));
    }
}

#[test]
fn push_set_unresolvable_task_targets_keep_advertised_history_fallback() {
    for target in ["integration/missing", "HEAD~1"] {
        let h = History::new();
        h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
        h.task(target);
        let old = h.commit("old", "old", "feat: first push");
        h.advertise(&old, "task/TSK-001-change");
        let head = h.commit("new", "new", "feat: next push");
        let range = h.range("task/TSK-001-change", &head, &old);
        assert_eq!(range.base, old);
        assert!(range.note.is_none());
    }
}

#[test]
fn push_set_resolves_qualified_task_target_names() {
    for target in [
        "refs/heads/integration/line",
        "refs/remotes/dest/integration/line",
        "origin/integration/line",
    ] {
        let h = History::new();
        let base = h.git(&["rev-parse", "HEAD"]);
        h.advertise(&base, "integration/line");
        h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
        h.task(target);
        let head = h.git(&["rev-parse", "HEAD"]);
        let range = h.range("task/TSK-001-change", &head, NEW);
        assert_eq!(range.base, base);
        assert!(range
            .note
            .unwrap()
            .text
            .contains("advertised target 'integration/line'"));
    }
}

#[test]
fn push_set_task_target_ignores_unrelated_malformed_records() {
    let h = History::new();
    let base = h.commit("line", "line", "feat: line");
    h.advertise(&base, "integration/line");
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
    h.task("integration/line");
    let head = h.commit(
        "project-management/tasks/TSK-002.md",
        "malformed",
        "docs: unrelated record",
    );
    let range = h.range("task/TSK-001-change", &head, NEW);
    assert_eq!(range.base, base);
    assert!(range
        .note
        .unwrap()
        .text
        .contains("advertised target 'integration/line'"));
}

#[test]
fn push_set_unreadable_task_target_keeps_advertised_history_fallback() {
    let h = History::new();
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
    let old = h.commit("old", "old", "feat: first push");
    h.advertise(&old, "task/TSK-001-change");
    let head = h.commit(
        "project-management/tasks/TSK-001.md",
        "malformed",
        "docs: unreadable target",
    );
    let range = h.range("task/TSK-001-change", &head, &old);
    assert_eq!(range.base, old);
    assert!(range.note.is_none());
}
