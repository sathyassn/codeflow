use super::*;

struct SelectedRange {
    base: String,
    note: Option<Finding>,
    notices: Vec<String>,
}

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
        command(
            history.remote.path(),
            &["init", "--bare", "-q", "-b", "main"],
        );
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

    fn range(&self, branch: &str, head: &str, old: &str) -> SelectedRange {
        self.try_range(branch, head, old).unwrap()
    }

    fn try_range(&self, branch: &str, head: &str, old: &str) -> Result<SelectedRange, String> {
        let listing = OnceCell::new();
        let advertised = OnceCell::new();
        let answer = OnceCell::new();
        let anchor = OnceCell::new();
        let fetch_failed = OnceCell::new();
        let destination = Destination {
            listing: &listing,
            answer: &answer,
            anchor: &anchor,
            fetch_failed: &fetch_failed,
            fork: None,
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
        let mut notices = Vec::new();
        let RangeBase { base, note } =
            range_base(self.local.path(), &pushed, &destination, &mut notices)?
                .ok_or("no range")?;
        Ok(SelectedRange {
            base,
            note,
            notices,
        })
    }
}

fn command(root: &Path, args: &[&str]) -> String {
    let out = codeflow_core::git::command()
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
    assert!(range
        .notices
        .iter()
        .any(|text| text.contains("integration/line")));
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
    assert!(range
        .notices
        .iter()
        .any(|text| text.contains("integration/line")));
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
    assert!(range
        .notices
        .iter()
        .any(|text| text.contains("integration/line")));
}

#[test]
fn push_set_undeclared_branch_keeps_advertised_history_fallback() {
    for branch in ["feat/change", "plan/change", "task/TSK-001-change"] {
        for existing in [false, true] {
            let h = History::new();
            let line = h.commit("line", "line", "Legacy line subject.");
            h.advertise(&line, "integration/line");
            h.git(&["checkout", "-q", "-b", branch]);
            let old = h.commit("old", "old", "feat: first push");
            if existing {
                h.advertise(&old, branch);
            }
            let head = h.commit("new", "new", "feat: next push");
            let range = h.range(branch, &head, if existing { &old } else { NEW });
            assert_eq!(
                range.base,
                if existing { old } else { line },
                "{branch} existing={existing}"
            );
            assert!(range.note.is_none());
        }
    }
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
            .notices
            .iter()
            .any(|text| text.contains("advertised target 'integration/line'")));
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
        .notices
        .iter()
        .any(|text| text.contains("advertised target 'integration/line'")));
}

#[test]
fn r16_push_set_unreadable_task_target_refuses() {
    let h = History::new();
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
    let old = h.commit("old", "old", "feat: first push");
    h.advertise(&old, "task/TSK-001-change");
    let head = h.commit(
        "project-management/tasks/TSK-001.md",
        "malformed",
        "docs: unreadable target",
    );
    assert!(h.try_range("task/TSK-001-change", &head, &old).is_err());
}

#[test]
fn push_set_clean_target_note_has_no_remedy_and_rewrite_uses_its_own() {
    let h = History::new();
    h.task("main");
    let target = h.git(&["rev-parse", "HEAD"]);
    h.advertise(&target, "main");
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
    let old = h.commit("old", "old", "feat: first work");
    h.advertise(&old, "task/TSK-001-change");
    let clean = h.range("task/TSK-001-change", &old, NEW);
    assert!(clean.note.is_none());
    assert!(clean
        .notices
        .iter()
        .any(|text| text.contains("uses advertised target 'main'")));
    h.git(&["checkout", "-q", "-b", "rewrite", &target]);
    let head = h.commit("rewrite", "rewrite", "feat: replacement work");
    let rewrite = h.range("task/TSK-001-change", &head, &old);
    assert_eq!(rewrite.base, target);
    assert_eq!(rewrite.note.unwrap().remedy, remedy::PUSH_REWRITE.remedy());
}

#[test]
fn push_set_unfetched_declared_target_is_named_without_fetching() {
    let h = History::new();
    h.task("integration/line");
    h.advertise("HEAD", "integration/line");
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
    let old = h.commit("old", "old", "feat: first work");
    h.advertise(&old, "task/TSK-001-change");
    let other = tempfile::tempdir().unwrap();
    command(
        other.path(),
        &["clone", "-q", h.remote.path().to_str().unwrap(), "."],
    );
    command(other.path(), &["config", "user.name", "Test"]);
    command(
        other.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    command(other.path(), &["checkout", "-q", "integration/line"]);
    std::fs::write(other.path().join("advance"), "advance").unwrap();
    command(other.path(), &["add", "advance"]);
    command(
        other.path(),
        &["commit", "-q", "-m", "feat: advance remote line"],
    );
    let tip = command(other.path(), &["rev-parse", "HEAD"]);
    command(
        h.remote.path(),
        &[
            "fetch",
            "-q",
            other.path().to_str().unwrap(),
            "HEAD:refs/heads/integration/line",
        ],
    );
    let head = h.commit("new", "new", "feat: next work");
    let range = h.range("task/TSK-001-change", &head, &old);
    assert_eq!(range.base, old);
    assert!(range
        .notices
        .iter()
        .any(|text| text.contains("declared target 'integration/line'")
            && text.contains(&tip)
            && text.contains("not fetched here")));
    assert!(!is_commit(h.local.path(), &tip).unwrap());

    // With no usable advertised tip, the range stays unresolved but its
    // missing-target notice must still reach the caller.
    for branch in ["main", "task/TSK-001-change"] {
        command(
            h.remote.path(),
            &["update-ref", "-d", &format!("refs/heads/{branch}")],
        );
    }
    // Keep the advertisement well formed while its only commit is unfetched.
    command(
        h.remote.path(),
        &["symbolic-ref", "HEAD", "refs/heads/integration/line"],
    );
    let listing = OnceCell::new();
    let advertised = OnceCell::new();
    let answer = OnceCell::new();
    let anchor = OnceCell::new();
    let fetch_failed = OnceCell::new();
    let destination = Destination {
        listing: &listing,
        answer: &answer,
        anchor: &anchor,
        fetch_failed: &fetch_failed,
        fork: None,
        url: h.remote.path().to_str(),
        advertised: &advertised,
        namespace: None,
        policy: &GitPolicy::default(),
    };
    let pushed = PushRef {
        local_ref: "refs/heads/task/TSK-001-change".into(),
        local_sha: head,
        remote_ref: "refs/heads/task/TSK-001-change".into(),
        remote_sha: NEW.into(),
    };
    let mut notices = Vec::new();
    assert!(
        range_base(h.local.path(), &pushed, &destination, &mut notices)
            .unwrap()
            .is_none()
    );
    assert!(notices
        .iter()
        .any(|text| text.contains(&tip) && text.contains("not fetched here")));
    assert!(!is_commit(h.local.path(), &tip).unwrap());
}

#[test]
fn r15_tracking_namespace_preserves_config_bytes() {
    let history = History::new();
    history.git(&[
        "config",
        "remote.fixture.fetch",
        "+refs/heads/*:refs/remotes/fixture/*\u{a0}",
    ]);
    assert_eq!(
        tracking_namespace(history.local.path(), "fixture").unwrap(),
        None
    );
}
#[cfg(unix)]
#[test]
fn r17_remote_query_exit_codes_keep_absence_distinct() {
    use std::os::unix::process::ExitStatusExt;
    let output = |code| std::process::Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    assert_eq!(
        git_answer(&["remote", "get-url", "missing"], output(2)),
        Ok(None)
    );
    for code in [1, 3, 128] {
        assert!(git_answer(&["remote", "get-url", "missing"], output(code)).is_err());
    }
    assert!(git_answer(&["remote", "update"], output(2)).is_err());
}
