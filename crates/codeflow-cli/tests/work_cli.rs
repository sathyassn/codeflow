//! Real CLI coverage of reviewed stacks and standalone claims.
use std::path::Path;
use std::process::{Command, Output};

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}
fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}
fn record(id: &str, deps: &str) -> String {
    format!("---\nid: {id}\nepic_id: null\nstandalone_reason: bounded outcome\nintegration_target: main\ntitle: work {id}\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: {deps}\ncreated: 2026-09-29\n---\n\n## Description\nWork.\n\n## Acceptance Criteria\n- AC-1 When run, the command shall succeed. (journey)\n")
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "user.email", "t@example.test"]);
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &record("TSK-001", "[]"),
    );
    write(
        root,
        "project-management/tasks/TSK-002.md",
        &record("TSK-002", "[TSK-001]"),
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: plan tasks"]);
    dir
}
fn cli(root: &Path, args: &[&str], bin: Option<&Path>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    cmd.current_dir(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    if let Some(bin) = bin {
        let path = std::env::join_paths(std::iter::once(bin.to_path_buf()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        cmd.env("PATH", path);
    }
    cmd.output().unwrap()
}
fn succeeds(out: &Output) {
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(unix)]
fn review_tool(bin: &Path, branch: &str, sha: &str, reviewed: bool) {
    use std::os::unix::fs::PermissionsExt;
    let body = if reviewed {
        format!("## Reviews\n| Reviewer | Scope | Verdict |\n| --- | --- | --- |\n| peer | {sha} | approved |\n")
    } else {
        "## Reviews\nNone: pending".into()
    };
    let json = serde_json::json!({"headRefName":branch,"headRefOid":sha,"isCrossRepository":false,"headRepository":{"nameWithOwner":"owner/project"},"body":body}).to_string();
    let script = format!("#!/bin/sh\ncat <<'PAYLOAD'\n{json}\nPAYLOAD\n");
    write(bin, "gh", &script);
    std::fs::set_permissions(bin.join("gh"), std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
#[cfg(unix)]
fn claim_checks_prospective_pin_then_start_checks_actual_ancestry() {
    let dir = fixture();
    let root = dir.path();
    let bin = tempfile::tempdir().unwrap();
    let branch = "task/TSK-001-work";
    git(
        root,
        &[
            "config",
            "remote.review.url",
            "https://github.com/owner/project.git",
        ],
    );
    git(
        root,
        &["config", &format!("branch.{branch}.remote"), "review"],
    );
    git(root, &["switch", "-qc", branch]);
    write(root, "src/a.rs", "pub fn a() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "feat: predecessor"]);
    let pin = git(root, &["rev-parse", "HEAD"]);
    let on = format!("TSK-001@{pin}");
    review_tool(bin.path(), branch, &pin, true);
    git(root, &["switch", "-q", "main"]);
    let claimed = cli(
        root,
        &["work", "claim", "TSK-002", "--on", &on],
        Some(bin.path()),
    );
    succeeds(&claimed);
    let child = "task/TSK-002-work-tsk-002";
    assert_eq!(git(root, &["rev-parse", child]), pin);
    git(root, &["switch", "-q", child]);
    succeeds(&cli(
        root,
        &["work", "start", "TSK-002", "--on", &on],
        Some(bin.path()),
    ));
    // The explicit stack never changes landing readiness.
    let out = cli(root, &["ci", "--base", "main", "--branch", child, "--pr-body", "Task: TSK-002\n## Summary\nWork.\n## Changes\n- Work.\n## Testing\nNot tested: nothing."], Some(bin.path()));
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not complete"));
    review_tool(bin.path(), branch, &pin, false);
    assert!(!cli(
        root,
        &["work", "start", "TSK-002", "--on", &on],
        Some(bin.path())
    )
    .status
    .success());
}

#[test]
fn claim_renames_branch_holding_new_standalone_record() {
    let dir = fixture();
    let root = dir.path();
    git(root, &["switch", "-qc", "fix/standalone"]);
    write(
        root,
        "project-management/tasks/TSK-003.md",
        &record("TSK-003", "[]"),
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: record standalone"]);
    let before = git(root, &["rev-parse", "HEAD"]);
    succeeds(&cli(root, &["work", "claim", "TSK-003"], None));
    assert_eq!(
        git(root, &["branch", "--show-current"]),
        "task/TSK-003-work-tsk-003"
    );
    assert_eq!(git(root, &["rev-parse", "HEAD"]), before);
    succeeds(&cli(root, &["work", "start", "TSK-003"], None));
}

#[test]
#[cfg(unix)]
fn review_lookup_binds_repository_and_bounds_descendant_stdout() {
    let dir = fixture();
    let root = dir.path();
    let bin = tempfile::tempdir().unwrap();
    let branch = "task/TSK-001-work";
    git(
        root,
        &[
            "config",
            "remote.review.url",
            "https://github.com/owner/project.git",
        ],
    );
    git(
        root,
        &["config", &format!("branch.{branch}.remote"), "review"],
    );
    git(root, &["switch", "-qc", branch]);
    let pin = git(root, &["rev-parse", "HEAD"]);
    let on = format!("TSK-001@{pin}");
    git(root, &["switch", "-qc", "task/TSK-002-child"]);
    review_tool(bin.path(), branch, &pin, true);
    let script = bin.path().join("gh");
    let original = std::fs::read_to_string(&script).unwrap();
    std::fs::write(&script, original.replace("owner/project", "other/project")).unwrap();
    let out = cli(
        root,
        &["work", "start", "TSK-002", "--on", &on],
        Some(bin.path()),
    );
    assert!(
        !out.status.success(),
        "another repository supplied the verdict"
    );
    std::fs::write(&script, original.replace("cat <<", "sleep 8 &\ncat <<")).unwrap();
    let started = std::time::Instant::now();
    let _ = cli(
        root,
        &["work", "start", "TSK-002", "--on", &on],
        Some(bin.path()),
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(7),
        "lookup exceeded its deadline"
    );
}

#[test]
#[cfg(unix)]
fn next_suggests_only_an_exact_reviewed_predecessor() {
    let dir = fixture();
    let root = dir.path();
    let bin = tempfile::tempdir().unwrap();
    let branch = "task/TSK-001-work";
    git(
        root,
        &[
            "config",
            "remote.review.url",
            "https://github.com/owner/project.git",
        ],
    );
    git(
        root,
        &["config", &format!("branch.{branch}.remote"), "review"],
    );
    git(root, &["switch", "-qc", branch]);
    write(root, "src/predecessor.rs", "pub fn ready() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "feat: build"]);
    let pin = git(root, &["rev-parse", "HEAD"]);
    git(root, &["switch", "-q", "main"]);
    review_tool(bin.path(), branch, &pin, true);
    let out = cli(root, &["work", "next"], Some(bin.path()));
    succeeds(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains(&format!(
        "startable on TSK-001@{pin} once that pin is named"
    )));
    review_tool(bin.path(), branch, &pin, false);
    let out = cli(root, &["work", "next"], Some(bin.path()));
    succeeds(&out);
    assert!(!String::from_utf8_lossy(&out.stdout).contains("startable on"));
}

#[test]
fn standalone_allocation_claim_start_complete_and_ci_is_one_pr() {
    let dir = fixture();
    let root = dir.path();
    succeeds(&cli(root, &["ids", "seed"], None));
    git(root, &["switch", "-qc", "feat/standalone"]);
    succeeds(&cli(
        root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "bounded independent fix",
            "--into",
            "main",
            "standalone outcome",
        ],
        None,
    ));
    let path = "project-management/tasks/TSK-003.md";
    let record = std::fs::read_to_string(root.join(path)).unwrap();
    write(
        root,
        path,
        &record.replace(
            "- AC-1\n",
            "- AC-1 When run, the command shall succeed (journey).\n",
        ),
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: record standalone criteria"]);
    succeeds(&cli(root, &["work", "claim", "TSK-003"], None));
    let branch = git(root, &["branch", "--show-current"]);
    assert!(branch.starts_with("task/TSK-003-"));
    succeeds(&cli(root, &["work", "start", "TSK-003"], None));
    write(root, "src/fix.rs", "pub fn fixed() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fix: build standalone outcome"]);
    let reviewed = git(root, &["rev-parse", "HEAD"]);
    let evidence = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(evidence.path(), format!("acceptance:\n  reviewed: {reviewed}\n  review: session:fixture@sha256:00\n  criteria:\n    AC-1: verified | standalone journey\n  journey: verified | standalone journey\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n")).unwrap();
    succeeds(&cli(
        root,
        &[
            "task",
            "status",
            "TSK-003",
            "complete",
            "--acceptance",
            evidence.path().to_str().unwrap(),
        ],
        None,
    ));
    git(root, &["add", "."]);
    git(
        root,
        &["commit", "-qm", "docs: complete standalone outcome"],
    );
    succeeds(&cli(root, &["ci", "--base", "main", "--branch", &branch, "--pr-body", "Task: TSK-003\n## Summary\nDeliver the bounded fix.\n## Changes\n- Implement the fix.\n## Testing\nNot tested: Windows."], None));
    assert!(!cli(root, &["work", "start", "TSK-003"], None)
        .status
        .success());
}

/// The hosted policy workflow checks out the base and passes the pull
/// request head as data. Judged that way, the standalone one-PR route is
/// admitted from the head revision, and a task PR whose record is already
/// on the target is admitted alike.
/// Commit a code change on the current branch, then complete `id` with an
/// acceptance block and commit that; returns the resulting head.
fn complete(root: &Path, id: &str, file: &str) -> String {
    write(root, file, "pub fn fixed() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fix: build the outcome"]);
    let reviewed = git(root, &["rev-parse", "HEAD"]);
    let evidence = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(evidence.path(), format!("acceptance:\n  reviewed: {reviewed}\n  review: session:fixture@sha256:00\n  criteria:\n    AC-1: verified | standalone journey\n  journey: verified | standalone journey\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n")).unwrap();
    succeeds(&cli(
        root,
        &[
            "task",
            "status",
            id,
            "complete",
            "--acceptance",
            evidence.path().to_str().unwrap(),
        ],
        None,
    ));
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: complete the outcome"]);
    git(root, &["rev-parse", "HEAD"])
}

#[test]
fn base_checkout_ci_admits_the_standalone_route_and_a_task_pr() {
    let dir = fixture();
    let root = dir.path();
    succeeds(&cli(root, &["ids", "seed"], None));
    let main = git(root, &["rev-parse", "main"]);
    let body = |id: &str| {
        format!("Task: {id}\n## Summary\nDeliver the bounded fix.\n## Changes\n- Implement the fix.\n## Testing\nNot tested: Windows.")
    };

    // The standalone route: record and code on one branch, absent from main.
    git(root, &["switch", "-qc", "feat/standalone"]);
    succeeds(&cli(
        root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "bounded independent fix",
            "--into",
            "main",
            "standalone outcome",
        ],
        None,
    ));
    let path = "project-management/tasks/TSK-003.md";
    let record = std::fs::read_to_string(root.join(path)).unwrap();
    write(
        root,
        path,
        &record.replace(
            "- AC-1\n",
            "- AC-1 When run, the command shall succeed (journey).\n",
        ),
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: record standalone criteria"]);
    succeeds(&cli(root, &["work", "claim", "TSK-003"], None));
    let standalone = git(root, &["branch", "--show-current"]);
    succeeds(&cli(root, &["work", "start", "TSK-003"], None));
    let standalone_head = complete(root, "TSK-003", "src/fix.rs");

    // A task PR whose record main already holds.
    git(root, &["switch", "-q", "main"]);
    succeeds(&cli(root, &["work", "claim", "TSK-001"], None));
    let task = git(
        root,
        &[
            "branch",
            "--list",
            "task/TSK-001-*",
            "--format=%(refname:short)",
        ],
    );
    git(root, &["switch", "-q", &task]);
    succeeds(&cli(root, &["work", "start", "TSK-001"], None));
    let task_head = complete(root, "TSK-001", "src/one.rs");

    // Judged from the base checkout, the head named as data.
    git(root, &["switch", "-q", "main"]);
    for (branch, head, id) in [
        (&standalone, &standalone_head, "TSK-003"),
        (&task, &task_head, "TSK-001"),
    ] {
        let out = cli(
            root,
            &[
                "ci",
                "--base",
                &main,
                "--head",
                head,
                "--branch",
                branch,
                "--pr-body",
                &body(id),
            ],
            None,
        );
        assert!(
            out.status.success(),
            "{id} from the base checkout:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
#[cfg(unix)]
fn stacked_pins_refuse_wrong_identity_advanced_tip_and_nonancestor_head() {
    let dir = fixture();
    let root = dir.path();
    let bin = tempfile::tempdir().unwrap();
    let branch = "task/TSK-001-work";
    git(
        root,
        &[
            "config",
            "remote.review.url",
            "https://github.com/owner/project.git",
        ],
    );
    git(
        root,
        &["config", &format!("branch.{branch}.remote"), "review"],
    );
    git(root, &["switch", "-qc", branch]);
    write(root, "src/a.rs", "pub fn a() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "feat: predecessor"]);
    let pin = git(root, &["rev-parse", "HEAD"]);
    let on = format!("TSK-001@{pin}");
    review_tool(bin.path(), branch, &pin, true);
    git(root, &["switch", "-qc", "task/TSK-002-child", "main"]);
    let out = cli(
        root,
        &["work", "start", "TSK-002", "--on", &on],
        Some(bin.path()),
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not an ancestor"));
    let wrong = format!("TSK-002@{pin}");
    assert!(!cli(
        root,
        &["work", "start", "TSK-002", "--on", &wrong],
        Some(bin.path())
    )
    .status
    .success());
    git(root, &["switch", "-q", branch]);
    git(
        root,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "feat: advance predecessor",
        ],
    );
    git(root, &["switch", "-q", "task/TSK-002-child"]);
    let out = cli(
        root,
        &["work", "start", "TSK-002", "--on", &on],
        Some(bin.path()),
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("new reviewed head"));
}

#[test]
#[cfg(unix)]
fn multiple_pins_require_a_single_prospective_base_before_claim_mutates() {
    for comparable in [false, true] {
        let dir = fixture();
        let root = dir.path();
        let bin = tempfile::tempdir().unwrap();
        write(
            root,
            "project-management/tasks/TSK-003.md",
            &record("TSK-003", "[TSK-001, TSK-002]"),
        );
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "docs: plan third"]);
        git(
            root,
            &[
                "config",
                "remote.review.url",
                "https://github.com/owner/project.git",
            ],
        );
        let mut pins = Vec::new();
        let mut scripts = Vec::new();
        for id in ["TSK-001", "TSK-002"] {
            if !comparable {
                git(root, &["switch", "-q", "main"]);
            }
            let branch = format!("task/{id}-work");
            git(root, &["switch", "-qc", &branch]);
            git(
                root,
                &["config", &format!("branch.{branch}.remote"), "review"],
            );
            write(root, &format!("src/{id}.rs"), "// reviewed\n");
            git(root, &["add", "."]);
            git(root, &["commit", "-qm", "feat: predecessor"]);
            let pin = git(root, &["rev-parse", "HEAD"]);
            pins.push(format!("{id}@{pin}"));
            review_tool(bin.path(), &branch, &pin, true);
            let script = std::fs::read_to_string(bin.path().join("gh")).unwrap();
            scripts.push(format!(
                "{branch})\n{};;\n",
                script.trim_start_matches("#!/bin/sh\n")
            ));
        }
        std::fs::write(
            bin.path().join("gh"),
            format!("#!/bin/sh\ncase \"$3\" in\n{}esac\n", scripts.join("")),
        )
        .unwrap();
        let prospective = git(root, &["rev-parse", "HEAD"]);
        git(root, &["switch", "-q", "main"]);
        let before = git(root, &["for-each-ref", "refs/heads/"]);
        let out = cli(
            root,
            &[
                "work", "claim", "TSK-003", "--on", &pins[0], "--on", &pins[1],
            ],
            Some(bin.path()),
        );
        if comparable {
            succeeds(&out);
            assert_eq!(
                git(root, &["rev-parse", "task/TSK-003-work-tsk-003"]),
                prospective
            );
        } else {
            assert!(!out.status.success());
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("no reviewed base contains all pins")
            );
            assert_eq!(git(root, &["for-each-ref", "refs/heads/"]), before);
        }
    }
}

/// Claim and start resolve every pin's hosted repository afresh: a
/// remote-tracking copy of a later predecessor from another repository,
/// arriving while an earlier pin's review is looked up, is refused.
#[test]
#[cfg(unix)]
fn a_conflicting_remote_arriving_between_pin_lookups_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fixture();
    let root = dir.path();
    let bin = tempfile::tempdir().unwrap();
    write(
        root,
        "project-management/tasks/TSK-003.md",
        &record("TSK-003", "[TSK-001, TSK-002]"),
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: plan third"]);
    git(
        root,
        &[
            "config",
            "remote.review.url",
            "https://github.com/owner/project.git",
        ],
    );
    git(
        root,
        &[
            "config",
            "remote.fork.url",
            "https://github.com/other/fork.git",
        ],
    );
    let mut pins = Vec::new();
    let mut cases = Vec::new();
    for id in ["TSK-001", "TSK-002"] {
        let branch = format!("task/{id}-work");
        git(root, &["switch", "-qc", &branch]);
        git(
            root,
            &["config", &format!("branch.{branch}.remote"), "review"],
        );
        git(
            root,
            &["commit", "--allow-empty", "-qm", "feat: predecessor"],
        );
        let pin = git(root, &["rev-parse", "HEAD"]);
        pins.push(pin.clone());
        let body = format!("## Reviews\n| Reviewer | Scope | Verdict |\n| --- | --- | --- |\n| peer | {pin} | approved |\n");
        let json = serde_json::json!({"headRefName":branch,"headRefOid":pin,"isCrossRepository":false,"headRepository":{"nameWithOwner":"owner/project"},"body":body}).to_string();
        cases.push((branch, json));
    }
    // The first predecessor's lookup fetches a fork's copy of the second.
    let script = format!(
        "#!/bin/sh\ncase \"$3\" in\n{first})\ngit update-ref refs/remotes/fork/{second} {pin}\ncat <<'PAYLOAD'\n{first_json}\nPAYLOAD\n;;\n{second})\ncat <<'PAYLOAD'\n{second_json}\nPAYLOAD\n;;\nesac\n",
        first = cases[0].0,
        second = cases[1].0,
        pin = pins[1],
        first_json = cases[0].1,
        second_json = cases[1].1,
    );
    write(bin.path(), "gh", &script);
    std::fs::set_permissions(
        bin.path().join("gh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let on = [
        "--on".to_string(),
        format!("TSK-001@{}", pins[0]),
        "--on".to_string(),
        format!("TSK-002@{}", pins[1]),
    ];
    for command in ["start", "claim"] {
        git(
            root,
            &["update-ref", "-d", "refs/remotes/fork/task/TSK-002-work"],
        );
        if command == "start" {
            git(root, &["switch", "-qc", "task/TSK-003-child"]);
        } else {
            git(root, &["switch", "-q", "main"]);
        }
        let before = git(root, &["for-each-ref", "refs/heads/"]);
        let mut args = vec!["work", command, "TSK-003"];
        args.extend(on.iter().map(String::as_str));
        let out = cli(root, &args, Some(bin.path()));
        assert!(
            !out.status.success(),
            "{command} admitted a conflicting pin"
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("conflicting predecessor repositories"),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(git(root, &["for-each-ref", "refs/heads/"]), before);
    }
}

#[test]
fn task_help_and_doctor_explain_standalone_and_retired_policy() {
    let dir = fixture();
    let root = dir.path();
    let help = cli(root, &["task", "new", "--help"], None);
    succeeds(&help);
    assert!(String::from_utf8_lossy(&help.stdout).contains("may run on its task branch"));
    let help = cli(root, &["work", "claim", "--help"], None);
    succeeds(&help);
    assert!(String::from_utf8_lossy(&help.stdout).contains("--on <TSK-NNN@SHA>"));
    write(
        root,
        ".codeflow/policy.json",
        "{\"schema_version\":1,\"git\":{\"direct_changes\":\"forbid\"}}\n",
    );
    let doctor = cli(root, &["doctor", "--check", "adopter-fit"], None);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&doctor.stdout),
        String::from_utf8_lossy(&doctor.stderr)
    );
    assert!(
        text.contains("git.direct_changes is retired and ignored"),
        "{text}"
    );
}
