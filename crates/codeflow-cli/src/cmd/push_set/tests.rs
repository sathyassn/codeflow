use super::*;

#[test]
fn branch_ancestry_is_a_timed_push_step() {
    let history = History::new();
    let head = history.git(&["rev-parse", "HEAD"]);
    std::fs::write(history.local.path().join("base.txt"), "dirty").unwrap();
    let destination = Destination {
        listing: &OnceCell::new(),
        advertised: &OnceCell::new(),
        answer: &OnceCell::new(),
        anchor: &OnceCell::new(),
        fetch_failed: &OnceCell::new(),
        fork: None,
        url: history.remote.path().to_str(),
        namespace: None,
        policy: &GitPolicy::default(),
    };
    let pushed = PushRef {
        local_ref: "refs/heads/new-name".into(),
        local_sha: head,
        remote_ref: "refs/heads/new-name".into(),
        remote_sha: "0".repeat(40),
    };
    let mut report = StageReport::default();
    let mut steps = Vec::new();
    run_tree_checks(
        Path::new("unused"),
        history.local.path(),
        &destination,
        &[&pushed],
        &mut report,
        &mut steps,
    )
    .unwrap();
    assert!(report.violations.is_empty(), "{report:?}");
    assert!(steps.iter().any(|step| step.name == "destination branch ancestry (new-name)" && !step.configurable), "{steps:?}");
}

#[cfg(unix)]
#[test]
fn r20_git_absence_statuses_are_command_specific() {
    use std::os::unix::process::ExitStatusExt;
    for args in [
        vec!["merge-base", "--independent", "HEAD"],
        vec!["rev-parse", "--is-shallow-repository"],
        vec!["config", "--list"],
    ] {
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        assert!(git_answer(&args, output).is_err(), "{args:?}");
    }
}

#[cfg(unix)]
#[test]
fn r20_documented_absence_and_success_remain_supported() {
    use std::os::unix::process::ExitStatusExt;
    for args in [
        vec!["merge-base", "first", "second"],
        vec!["merge-base", "--is-ancestor", "first", "second"],
        vec!["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
        vec!["config", "--get", "remote.upstream.url"],
        vec!["config", "--bool", "core.sparseCheckout"],
        vec!["config", "-z", "--get-all", "remote.origin.fetch"],
    ] {
        for code in [1, 128] {
            let output = std::process::Output {
                status: std::process::ExitStatus::from_raw(code << 8),
                stdout: Vec::new(),
                stderr: Vec::new(),
            };
            let answer = git_answer(&args, output);
            if code == 1 {
                assert_eq!(answer, Ok(None), "{args:?}");
            } else {
                assert!(answer.is_err(), "{args:?}");
            }
        }
    }
    let oid = format!("{}\n", "a".repeat(40));
    assert_eq!(
        git_answer(
            &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
            std::process::Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: oid.as_bytes().to_vec(),
                stderr: Vec::new(),
            }
        ),
        Ok(Some(oid))
    );
}

#[cfg(unix)]
#[test]
fn r20_revision_success_requires_an_object_id() {
    use std::os::unix::process::ExitStatusExt;
    for stdout in [Vec::new(), b"not-an-object-id\n".to_vec()] {
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout,
            stderr: Vec::new(),
        };
        assert!(git_answer(
            &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
            output
        )
        .is_err());
    }
}

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
        history.commit(
            "project-management/epics/EPC-001.md",
            "---\nid: EPC-001\ntitle: Work\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-10-16\n---\n",
            "docs: plan the epic",
        );
        history.commit(
            "project-management/tasks/TSK-002.md",
            "---\nid: TSK-002\nepic_id: EPC-001\nintegration_target: integration/EPC-001-line\n---\n",
            "docs: bind the epic line",
        );
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
            &format!("---\nid: TSK-001\nepic_id: EPC-001\nintegration_target: {target}\n---\n"),
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
fn push_set_declared_advertisements_require_eligibility_and_protected_tip_agreement() {
    for (target, same_tip) in [
        ("master", true),
        ("master", false),
        ("integration/EPC-999-decoy", false),
    ] {
        let h = History::new();
        let base = h.git(&["rev-parse", "main"]);
        h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
        h.task(target);
        let hidden = h.commit("earlier", "earlier", "fix: earlier work");
        h.advertise(if same_tip { &base } else { &hidden }, target);
        let head = h.commit("later", "later", "fix: later work");
        let range = h.range("task/TSK-001-change", &head, NEW);
        assert_eq!(range.base, base, "{target}, same_tip={same_tip}");
        let selected = if same_tip { target } else { "main" };
        assert!(
            range
                .notices
                .iter()
                .any(|text| text.contains(&format!("uses advertised target '{selected}'"))),
            "{:?}",
            range.notices
        );
    }
}

#[test]
fn push_set_landing_extends_advertised_line_not_first_boundary() {
    for branch in ["integration/EPC-001-line", "main"] {
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
    h.git(&["checkout", "-q", "-b", "integration/EPC-001-line"]);
    h.task("integration/EPC-001-line");
    h.advertise("HEAD", "integration/EPC-001-line");
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
    let old = h.commit("own", "own", "feat: own work");
    h.advertise(&old, "task/TSK-001-change");
    h.git(&["checkout", "-q", "integration/EPC-001-line"]);
    let target = h.commit("line", "line", "feat: line advances");
    h.advertise(&target, "integration/EPC-001-line");
    h.git(&["checkout", "-q", "task/TSK-001-change"]);
    h.git(&[
        "merge",
        "--no-ff",
        "-m",
        "feat: incorporate line",
        "integration/EPC-001-line",
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
        .any(|text| text.contains("integration/EPC-001-line")));
}

#[test]
fn push_set_new_task_with_multiple_boundaries_uses_declared_target() {
    let h = History::new();
    let target = h.git(&["rev-parse", "HEAD"]);
    h.advertise(&target, "integration/EPC-001-line");
    h.git(&["checkout", "-q", "-b", "sibling"]);
    h.commit("sibling", "sibling", "feat: sibling");
    h.advertise("HEAD", "task/sibling");
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change", &target]);
    h.task("integration/EPC-001-line");
    h.git(&["merge", "--no-ff", "-m", "feat: use sibling", "sibling"]);
    let head = h.git(&["rev-parse", "HEAD"]);
    let range = h.range("task/TSK-001-change", &head, NEW);
    assert_eq!(range.base, target);
    assert!(range
        .notices
        .iter()
        .any(|text| text.contains("integration/EPC-001-line")));
}

#[test]
fn push_set_target_base_keeps_every_destination_missing_commit() {
    let h = History::new();
    let common = h.git(&["rev-parse", "HEAD"]);
    h.git(&["checkout", "-q", "-b", "integration/EPC-001-line"]);
    let advertised = h.commit("line", "line", "feat: target diverges");
    h.advertise(&advertised, "integration/EPC-001-line");
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change", &common]);
    h.task("integration/EPC-001-line");
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
        .any(|text| text.contains("integration/EPC-001-line")));
}

#[test]
fn push_set_undeclared_branch_keeps_advertised_history_fallback() {
    for branch in ["feat/change", "plan/change", "task/TSK-001-change"] {
        for existing in [false, true] {
            let h = History::new();
            let line = h.commit("line", "line", "Legacy line subject.");
            h.advertise(&line, "integration/EPC-001-line");
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
    for branch in ["main", "integration/EPC-001-line"] {
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
fn push_set_rejected_task_targets_keep_the_destination_default() {
    for target in ["integration/missing", "HEAD~1"] {
        let h = History::new();
        h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
        h.task(target);
        let old = h.commit("old", "old", "feat: first push");
        h.advertise(&old, "task/TSK-001-change");
        let head = h.commit("new", "new", "feat: next push");
        let range = h.range("task/TSK-001-change", &head, &old);
        assert_eq!(range.base, h.git(&["rev-parse", "main"]));
        assert!(range.note.is_none());
    }
}

#[test]
fn push_set_resolves_qualified_task_target_names() {
    for target in [
        "refs/heads/integration/EPC-001-line",
        "refs/remotes/dest/integration/EPC-001-line",
        "origin/integration/EPC-001-line",
    ] {
        let h = History::new();
        let base = h.git(&["rev-parse", "HEAD"]);
        h.advertise(&base, "integration/EPC-001-line");
        h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
        h.task(target);
        let head = h.git(&["rev-parse", "HEAD"]);
        let range = h.range("task/TSK-001-change", &head, NEW);
        assert_eq!(range.base, base);
        assert!(range
            .notices
            .iter()
            .any(|text| text.contains("advertised target 'integration/EPC-001-line'")));
    }
}

#[test]
fn push_set_task_target_ignores_unrelated_malformed_records() {
    let h = History::new();
    let base = h.commit("line", "line", "feat: line");
    h.advertise(&base, "integration/EPC-001-line");
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
    h.task("integration/EPC-001-line");
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
        .any(|text| text.contains("advertised target 'integration/EPC-001-line'")));
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
    h.task("integration/EPC-001-line");
    h.advertise("HEAD", "integration/EPC-001-line");
    h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
    let old = h.commit("old", "old", "feat: first work");
    h.advertise(&old, "task/TSK-001-change");
    let other = tempfile::tempdir().unwrap();
    codeflow_fixture::clone(other.path(), h.remote.path().to_str().unwrap(), ".")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .run();
    command(other.path(), &["config", "user.name", "Test"]);
    command(
        other.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    command(
        other.path(),
        &["checkout", "-q", "integration/EPC-001-line"],
    );
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
            "HEAD:refs/heads/integration/EPC-001-line",
        ],
    );
    let head = h.commit("new", "new", "feat: next work");
    let range = h.range("task/TSK-001-change", &head, &old);
    assert_eq!(range.base, old);
    assert!(range.notices.iter().any(|text| text
        .contains("declared target 'integration/EPC-001-line'")
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
        &[
            "symbolic-ref",
            "HEAD",
            "refs/heads/integration/EPC-001-line",
        ],
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

#[test]
fn r22_missing_binary_blocks_even_when_push_gate_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let policy = GitPolicy {
        test_gate_on_push: codeflow_core::hooks::PolicyLevel::Off,
        ..GitPolicy::default()
    };
    let refs = git_hook::parse_push_refs(&format!(
        "refs/heads/feat/test {} refs/heads/feat/test {}\n",
        "a".repeat(40),
        "b".repeat(40)
    ));
    let mut report = StageReport::default();
    run_checked_with_binary(dir.path(), &policy, &refs, None, None, &mut report, || {
        Err(std::io::Error::other("binary location unavailable"))
    })
    .unwrap();
    assert!(codeflow_core::hooks::any_blocking(&report.violations));
    assert!(report.violations[0]
        .message
        .contains("binary location unavailable"));
}

#[test]
fn r22_empty_push_set_does_not_require_a_binary() {
    let dir = tempfile::tempdir().unwrap();
    let mut report = StageReport::default();
    run_checked_with_binary(
        dir.path(),
        &GitPolicy::default(),
        &[],
        None,
        None,
        &mut report,
        || panic!("an empty push set has no checks to launch"),
    )
    .unwrap();
    assert!(report.violations.is_empty());
}

#[test]
fn r22_release_preflight_errors_are_precise_nonblocking_notes() {
    let dir = tempfile::tempdir().unwrap();
    let refs = git_hook::parse_push_refs(&format!(
        "refs/heads/feat/test {} refs/heads/feat/test {}\n",
        "a".repeat(40),
        "b".repeat(40)
    ));
    let exe = std::env::current_exe().unwrap();
    let run = |root: &Path| {
        let mut report = StageReport::default();
        release_preflight(
            &exe,
            root,
            &refs[0],
            "origin",
            &GitPolicy::default(),
            &mut report,
            &mut Vec::new(),
        );
        assert!(report.violations.is_empty());
        assert_eq!(report.notes.len(), 1);
        let note = report.notes[0].line("codeflow", "note");
        assert!(note.contains("could not complete"), "{note}");
        assert!(note.contains("the pull request job checks it"), "{note}");
        note
    };
    // The process cannot launch from an absent working directory.
    let note = run(&dir.path().join("absent"));
    assert!(note.contains("python3 scripts/release.py"), "{note}");
    std::fs::create_dir(dir.path().join("scripts")).unwrap();
    std::fs::write(dir.path().join("scripts/release.py"), "print('not JSON')\n").unwrap();
    let note = run(dir.path());
    assert!(note.contains("preflight printed no result"), "{note}");
    std::fs::write(
        dir.path().join("scripts/release.py"),
        "import sys\nprint('preflight failure', file=sys.stderr)\nsys.exit(3)\n",
    )
    .unwrap();
    let note = run(dir.path());
    assert!(note.contains("preflight failure"), "{note}");
    // The unsuccessful status is named beside the script's own text.
    assert!(note.contains("exited with code 3"), "{note}");
    // With no text, the status alone is the diagnostic.
    std::fs::write(
        dir.path().join("scripts/release.py"),
        "import sys\nsys.exit(4)\n",
    )
    .unwrap();
    let note = run(dir.path());
    assert!(note.contains("exited with code 4"), "{note}");
}

#[test]
fn r22_release_preflight_valid_empty_notes_stay_clear() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("scripts/release.py"),
        "print('{\"status\":\"ok\",\"notes\":[]}')\n",
    )
    .unwrap();
    let refs = git_hook::parse_push_refs(&format!(
        "refs/heads/feat/test {} refs/heads/feat/test {}\n",
        "a".repeat(40),
        "b".repeat(40)
    ));
    let mut report = StageReport::default();
    release_preflight(
        &std::env::current_exe().unwrap(),
        dir.path(),
        &refs[0],
        "origin",
        &GitPolicy::default(),
        &mut report,
        &mut Vec::new(),
    );
    assert!(report.notes.is_empty());
    assert!(report.violations.is_empty());
}

#[test]
fn round_two_hook_and_local_plans_use_the_same_epic_target() {
    let line = "integration/EPC-001-outcome";
    let branch = "plan/EPC-001-next";
    // Bound by records on the protected branch, then by none.
    for (bound, expected) in [(true, line), (false, "main")] {
        let h = History::new();
        if bound {
            h.commit(
                "project-management/epics/EPC-001.md",
                "---\nid: EPC-001\ntitle: Outcome\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-10-06\n---\n\n# Outcome\n",
                "docs: plan the epic",
            );
            h.commit(
                "project-management/tasks/TSK-001.md",
                &format!("---\nid: TSK-001\nepic_id: EPC-001\ntitle: Work\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\nintegration_target: {line}\ncreated: 2026-10-06\n---\n\n# Work\n"),
                "docs: plan the task",
            );
        }
        h.git(&["branch", line]);
        let head = h.git(&["rev-parse", "HEAD"]);
        let pushed = PushRef {
            local_ref: format!("refs/heads/{branch}"),
            local_sha: head.clone(),
            remote_ref: format!("refs/heads/{branch}"),
            remote_sha: NEW.into(),
        };
        let judged = Judged::Target {
            target: "main".into(),
            tip: head,
            policy: Box::default(),
        };
        let mut report = StageReport::default();
        let target = ci_target(
            h.local.path(),
            &pushed,
            branch,
            &judged,
            &GitPolicy::default(),
            &mut report,
        )
        .unwrap();
        assert_eq!(target.as_deref(), Some(expected), "{:?}", report.status);
        assert_eq!(
            report
                .status
                .iter()
                .any(|line| line.contains("destination's default")),
            !bound,
            "{:?}",
            report.status
        );
    }
}

#[cfg(unix)]
#[test]
fn round_two_hook_supplied_ci_context_errors_keep_their_block() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("ci-fixture");
    std::fs::write(&exe, "#!/bin/sh\nexit 2\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let policy = GitPolicy {
        test_gate_on_push: codeflow_core::hooks::PolicyLevel::Warn,
        ..Default::default()
    };
    let mut report = StageReport::default();
    run_check_with(
        &exe,
        dir.path(),
        &["ci", "--run-level", "warn", "--into=main"],
        None,
        &policy,
        &mut report,
        &mut Vec::new(),
    );
    assert_eq!(report.violations.len(), 1);
    assert_eq!(
        report.violations[0].level,
        codeflow_core::hooks::PolicyLevel::Block
    );
}

#[cfg(unix)]
#[test]
fn round_two_unavailable_destination_refuses_before_ci() {
    use std::os::unix::fs::PermissionsExt;
    let h = History::new();
    let base = h.git(&["rev-parse", "HEAD"]);
    h.task("main");
    let branch = "task/TSK-001-work";
    let pushed = PushRef {
        local_ref: format!("refs/heads/{branch}"),
        local_sha: h.git(&["rev-parse", "HEAD"]),
        remote_ref: format!("refs/heads/{branch}"),
        remote_sha: base,
    };
    let listing = OnceCell::from(Err("fixture: destination unavailable".into()));
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
        url: None,
        advertised: &advertised,
        namespace: None,
        policy: &GitPolicy::default(),
    };
    let exe = h.local.path().join("ci-fixture");
    std::fs::write(&exe, "#!/bin/sh\nprintf '%s\\n' \"$@\" > arguments.txt\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut report = StageReport::default();
    let error = run_ci_ranges(
        &exe,
        h.local.path(),
        &[&pushed],
        &destination,
        &mut report,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.contains("destination unavailable"), "{error}");
    assert!(!h.local.path().join("arguments.txt").exists());
}

/// Default branch names a destination can advertise and how the hook
/// shows them (TSK-388): an OSC title sequence, which only a server that is
/// not git sends, the C1 control CSI (U+009B), which git accepts in a
/// branch name, and an ordinary name, shown unchanged.
const DEFAULTS: [(&str, &str); 5] = [
    ("ev\u{1b}]0;owned\u{7}il", "ev\\x1b]0;owned\\x07il"),
    ("ev\u{9b}31mil", "ev\\x9b31mil"),
    ("ev\u{2028}il", "ev\\u{2028}il"),
    ("ev\u{202e}lim", "ev\\u{202e}lim"),
    ("trunk", "trunk"),
];

/// Whether `text` holds a control character other than a line break, or
/// a line separator or right-to-left override.
fn controls(text: &str) -> bool {
    text.chars()
        .any(|c| (c.is_control() && c != '\n') || matches!(c, '\u{2028}' | '\u{202e}'))
}

#[test]
fn a_declared_target_falls_back_to_the_default_branch_named_escaped() {
    for (name, shown) in &DEFAULTS[1..] {
        let h = History::new();
        let base = h.git(&["rev-parse", "main"]);
        h.advertise("main", name);
        command(
            h.remote.path(),
            &["symbolic-ref", "HEAD", &format!("refs/heads/{name}")],
        );
        h.git(&["checkout", "-q", "-b", "task/TSK-001-change"]);
        h.task("master");
        let hidden = h.commit("earlier", "earlier", "fix: earlier work");
        h.advertise(&hidden, "master");
        let head = h.commit("later", "later", "fix: later work");
        let range = h.range("task/TSK-001-change", &head, NEW);
        assert_eq!(range.base, base);
        for expected in [
            format!("using the destination's default target '{shown}'"),
            format!("uses advertised target '{shown}'"),
        ] {
            assert!(
                range.notices.iter().any(|text| text.contains(&expected)),
                "{expected}: {:?}",
                range.notices
            );
        }
        assert!(
            !range.notices.iter().any(|text| controls(text)),
            "{:?}",
            range.notices
        );
    }
}

/// What the hook prints when it judges `pushed` against a destination
/// whose default branch is `name` at `tip`: its status lines, violation
/// messages, step names (the commands it shows) and any error. A fixture
/// stands in for `codeflow ci` and writes its arguments to
/// `arguments.txt`.
#[cfg(unix)]
fn judged_against(h: &History, name: &str, tip: &str, pushed: &[PushRef]) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    let oid = git2::Oid::from_str(tip).unwrap();
    let listing = OnceCell::from(Ok(format!(
        "ref: refs/heads/{name}\tHEAD\n{tip}\tHEAD\n{tip}\trefs/heads/{name}\n"
    )));
    let answer = OnceCell::from(Ok(release_line::Destination {
        url: h.remote.path().to_str().map(str::to_string),
        default: Some((name.to_string(), oid)),
        heads: vec![(name.to_string(), oid)],
    }));
    let advertised = OnceCell::new();
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
    let exe = h.local.path().join("ci-fixture");
    std::fs::write(&exe, "#!/bin/sh\nprintf '%s\\n' \"$@\" > arguments.txt\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut report = StageReport::default();
    let mut steps = Vec::new();
    let pushed: Vec<&PushRef> = pushed.iter().collect();
    let error = run_ci_ranges(
        &exe,
        h.local.path(),
        &pushed,
        &destination,
        &mut report,
        &mut steps,
    )
    .err();
    report
        .status
        .into_iter()
        .chain(report.violations.into_iter().map(|v| v.message))
        .chain(steps.into_iter().map(|step| step.name))
        .chain(error)
        .collect()
}

/// The hook names the destination's default branch escaped in its own
/// messages and in the `codeflow ci` command it shows, and hands the name
/// to `codeflow ci` unchanged.
#[cfg(unix)]
#[test]
fn the_destination_default_branch_is_named_escaped() {
    let valid = "{\"schema_version\": 1, \"git\": {}}\n";
    for (name, shown) in DEFAULTS {
        for (policy, expected) in [
            (
                Some(valid),
                vec![
                    format!("with the policy at {shown} "),
                    format!("everything it adds to {shown} at "),
                ],
            ),
            (Some("{\n"), vec![format!("the policy at {shown} ")]),
            (
                None,
                vec![format!(
                    "the destination's default branch {shown} has no policy yet"
                )],
            ),
        ] {
            let h = History::new();
            if let Some(policy) = policy {
                h.commit(".codeflow/policy.json", policy, "chore: add the policy");
            }
            let tip = h.git(&["rev-parse", "HEAD"]);
            let mut pushed = Vec::new();
            for branch in ["integration/release-1", "feat/work"] {
                h.git(&["checkout", "-q", "-b", branch, &tip]);
                pushed.push(PushRef {
                    local_ref: format!("refs/heads/{branch}"),
                    local_sha: h.commit(branch, "work", "feat: work"),
                    remote_ref: format!("refs/heads/{branch}"),
                    remote_sha: NEW.to_string(),
                });
            }
            let printed = judged_against(&h, name, &tip, &pushed);
            for expected in &expected {
                assert!(
                    printed.iter().any(|text| text.contains(expected.as_str())),
                    "{expected}: {printed:#?}"
                );
            }
            assert!(!printed.iter().any(|text| controls(text)), "{printed:#?}");
            // A name with ESC is no branch name, so no check runs for it.
            if policy == Some(valid) && !name.contains('\u{1b}') {
                assert!(
                    printed
                        .iter()
                        .any(|text| text.contains(&format!("--into={shown}"))),
                    "{printed:#?}"
                );
                let handed = std::fs::read_to_string(h.local.path().join("arguments.txt")).unwrap();
                assert!(
                    handed.lines().any(|line| line == format!("--into={name}")),
                    "{handed}"
                );
            }
        }
    }
}

/// A check the hook runs can quote a name the destination supplied; the
/// hook relays its output with each control character escaped, and keeps
/// the rest, line breaks and tabs included, unchanged.
#[cfg(unix)]
#[test]
fn a_relayed_check_output_shows_control_characters_escaped() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("check-fixture");
    std::fs::write(
        &exe,
        "#!/bin/sh\nprintf \"codeflow ci: warning \\342\\200\\224 target 'ev\\033]0;owned\\007il\\342\\200\\256'\\n  sanctioned:\\tfix it\\n\" >&2\n",
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut report = StageReport::default();
    run_check_with(
        &exe,
        dir.path(),
        &["ci"],
        None,
        &GitPolicy::default(),
        &mut report,
        &mut Vec::new(),
    );
    assert_eq!(
        report.relayed,
        [
            "`codeflow ci`: codeflow ci: warning \u{2014} target 'ev\\x1b]0;owned\\x07il\\u{202e}'\n  sanctioned:\tfix it"
        ],
        "{report:?}"
    );
}
