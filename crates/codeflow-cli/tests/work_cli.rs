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
    let out = cli(root, &["ci", "--base", "main", "--branch", child, "--pr-body", "Task: TSK-002\n## Summary\nWork.\n\n- work\n## Changes\n- Work.\n## Testing\nNot tested: nothing."], Some(bin.path()));
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
    succeeds(&cli(root, &["ci", "--base", "main", "--branch", &branch, "--pr-body", "Task: TSK-003\n## Summary\nDeliver the bounded fix.\n\n- the fix\n## Changes\n- Implement the fix.\n## Testing\nNot tested: Windows."], None));
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
        format!("Task: {id}\n## Summary\nDeliver the bounded fix.\n\n- the fix\n## Changes\n- Implement the fix.\n## Testing\nNot tested: Windows.")
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

#[test]
fn tsk189_claim_reads_the_fetched_target_from_main() {
    let dir = fixture();
    let root = dir.path();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(root, &["switch", "-qc", "integration/test-line"]);
    write(
        root,
        "project-management/tasks/TSK-003.md",
        &record("TSK-003", "[]").replace(
            "integration_target: main",
            "integration_target: integration/test-line",
        ),
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: plan line"]);
    git(root, &["push", "-q", "origin", "integration/test-line"]);
    git(root, &["switch", "-q", "main"]);
    succeeds(&cli(root, &["work", "claim", "TSK-003"], None));
    assert!(git(root, &["branch", "--list", "task/TSK-003-*"]).contains("TSK-003"));
}

#[test]
fn tsk189_start_prefers_fetched_line_without_local_upstream() {
    let dir = fixture();
    let root = dir.path();
    let base = git(root, &["rev-parse", "HEAD"]);
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(root, &["switch", "-qc", "integration/test-line"]);
    write(
        root,
        "project-management/tasks/TSK-003.md",
        &record("TSK-003", "[]").replace(
            "integration_target: main",
            "integration_target: integration/test-line",
        ),
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: plan line"]);
    git(root, &["push", "-q", "origin", "integration/test-line"]);
    git(root, &["switch", "-qc", "task/TSK-003-work"]);
    git(
        root,
        &["update-ref", "refs/heads/integration/test-line", &base],
    );
    let out = cli(root, &["work", "start", "TSK-003"], None);
    succeeds(&out);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("refs/remotes/origin/integration/test-line"),
        "{text}"
    );
}

/// A fake `gh` whose pull request for `branch` has `head` as its tip and a
/// Reviews row approving `named`.
#[cfg(unix)]
fn review_tool_naming(bin: &Path, branch: &str, head: &str, named: &str) {
    use std::os::unix::fs::PermissionsExt;
    let body = format!(
        "## Reviews\n| Reviewer | Scope | Verdict |\n| --- | --- | --- |\n| peer | {named} | approved |\n"
    );
    let json = serde_json::json!({"headRefName":branch,"headRefOid":head,"isCrossRepository":false,"headRepository":{"nameWithOwner":"owner/project"},"body":body}).to_string();
    write(
        bin,
        "gh",
        &format!("#!/bin/sh\ncat <<'PAYLOAD'\n{json}\nPAYLOAD\n"),
    );
    std::fs::set_permissions(bin.join("gh"), std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// TSK-001's own pull request on `branch`, as issue #69 found it: the
/// branch corrects TSK-001's criteria and builds, the review names that
/// commit, and the acceptance is recorded after it in a Closeout commit.
/// Returns the reviewed commit and the branch tip.
#[cfg(unix)]
fn reviewed_predecessor(root: &Path, branch: &str) -> (String, String) {
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
    git(root, &["switch", "-qc", branch, "main"]);
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &format!(
            "{}- AC-2 When rerun, the command shall still succeed.\n",
            record("TSK-001", "[]")
        ),
    );
    write(root, "src/a.rs", "pub fn a() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "feat: build the predecessor"]);
    let reviewed = git(root, &["rev-parse", "HEAD"]);
    let evidence = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(evidence.path(), format!("acceptance:\n  reviewed: {reviewed}\n  review: https://example.test/pr/1#review\n  criteria:\n    AC-1: verified | journey\n    AC-2: verified | unit\n  journey: verified | journey\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n")).unwrap();
    succeeds(&cli(
        root,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            evidence.path().to_str().unwrap(),
        ],
        None,
    ));
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: record the acceptance"]);
    let tip = git(root, &["rev-parse", "HEAD"]);
    git(root, &["switch", "-q", "main"]);
    (reviewed, tip)
}

/// `codeflow ci` as the pre-push hook runs it for a push of `branch`: no
/// pull request body, from `main` to `head`.
#[cfg(unix)]
fn push_check(root: &Path, branch: &str, head: &str, bin: &Path) -> Output {
    cli(
        root,
        &["ci", "--base", "main", "--head", head, "--branch", branch],
        Some(bin),
    )
}

#[cfg(unix)]
fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Issue #69: `work claim --on` accepts a reviewed pin, and the claim's own
/// push check, from the target to the pin, judges the commits up to the pin
/// as the predecessor's reviewed pull request. The claim pushes through the
/// installed pre-push hook to `origin`, and later pushes of the successor's
/// own work pass the same check. The pull request into the target still
/// waits for the predecessor to land (R-42), and the successor may not
/// change the predecessor's criteria itself.
#[test]
#[cfg(unix)]
fn a_claim_on_a_reviewed_head_passes_its_own_push_check() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fixture();
    let root = dir.path();
    write(
        root,
        ".codeflow/policy.json",
        "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\"]}\n}\n",
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "chore: add the policy"]);
    let origin = tempfile::tempdir().unwrap();
    git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    git(
        root,
        &["remote", "add", "origin", origin.path().to_str().unwrap()],
    );
    git(root, &["push", "-q", "origin", "main"]);
    let hooks = tempfile::tempdir().unwrap();
    write(
        hooks.path(),
        "pre-push",
        &format!(
            "#!/bin/sh\nexec '{}' git-hook pre-push \"$@\"\n",
            env!("CARGO_BIN_EXE_codeflow")
        ),
    );
    std::fs::set_permissions(
        hooks.path().join("pre-push"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    git(
        root,
        &["config", "core.hooksPath", hooks.path().to_str().unwrap()],
    );
    let bin = tempfile::tempdir().unwrap();
    let branch = "task/TSK-001-work";
    let (_, pin) = reviewed_predecessor(root, branch);
    review_tool_naming(bin.path(), branch, &pin, &pin);
    let on = format!("TSK-001@{pin}");

    let claimed = cli(
        root,
        &["work", "claim", "TSK-002", "--on", &on],
        Some(bin.path()),
    );
    assert!(claimed.status.success(), "{}", text(&claimed));
    let child = "task/TSK-002-work-tsk-002";
    assert_eq!(
        git(
            origin.path(),
            &["rev-parse", &format!("refs/heads/{child}")]
        ),
        pin
    );
    let first = push_check(root, child, child, bin.path());
    assert!(first.status.success(), "{}", text(&first));
    assert!(
        text(&first).contains("stacks on TSK-001's reviewed head"),
        "{}",
        text(&first)
    );

    // The successor's own work pushes through the same check.
    git(root, &["switch", "-q", child]);
    succeeds(&cli(
        root,
        &["work", "start", "TSK-002", "--on", &on],
        Some(bin.path()),
    ));
    write(root, "src/b.rs", "pub fn b() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "feat: build on the predecessor"]);
    let pushed = Command::new("git")
        .args(["push", "-q", "origin", child])
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env(
            "PATH",
            std::env::join_paths(std::iter::once(bin.path().to_path_buf()).chain(
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
            ))
            .unwrap(),
        )
        .output()
        .unwrap();
    assert!(pushed.status.success(), "{}", text(&pushed));

    // The pull request into the target still waits for the predecessor.
    let landing = cli(root, &["ci", "--base", "main", "--branch", child, "--pr-body", "Task: TSK-002\n## Summary\nWork.\n\n- work\n## Changes\n- Work.\n## Testing\nNot tested: nothing."], Some(bin.path()));
    assert!(!landing.status.success(), "{}", text(&landing));
    assert!(
        text(&landing).contains("not complete"),
        "{}",
        text(&landing)
    );

    // The successor cannot change the predecessor's criteria itself.
    let record = std::fs::read_to_string(root.join("project-management/tasks/TSK-001.md")).unwrap();
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &record.replace("shall still succeed", "shall mostly succeed"),
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: change the predecessor"]);
    let changed = push_check(root, child, "HEAD", bin.path());
    assert!(!changed.status.success(), "{}", text(&changed));
    assert!(
        text(&changed).contains("TSK-001 changes its criteria on this branch"),
        "{}",
        text(&changed)
    );
}

/// The pin is honoured only while a review row names it: with the row gone,
/// the same push check refuses, as it does for a branch tip of the
/// predecessor that no review names (issue #69).
#[test]
#[cfg(unix)]
fn an_unreviewed_predecessor_head_is_not_honoured_by_the_push_check() {
    let dir = fixture();
    let root = dir.path();
    let bin = tempfile::tempdir().unwrap();
    let branch = "task/TSK-001-work";
    let (_, pin) = reviewed_predecessor(root, branch);
    review_tool_naming(bin.path(), branch, &pin, &pin);
    let on = format!("TSK-001@{pin}");
    succeeds(&cli(
        root,
        &["work", "claim", "TSK-002", "--on", &on],
        Some(bin.path()),
    ));
    let child = "task/TSK-002-work-tsk-002";
    review_tool(bin.path(), branch, &pin, false);
    let out = push_check(root, child, child, bin.path());
    assert!(!out.status.success(), "{}", text(&out));
    for needle in [
        "not honoured as reviewed",
        "TSK-001 changes its criteria on this branch",
        "whose anchored status is 'todo', not complete",
    ] {
        assert!(text(&out).contains(needle), "{needle}: {}", text(&out));
    }
}

/// Issue #69: a pin may be the predecessor's tip when the review names the
/// commit before it and the tip adds only TSK-001's status and Closeout, as
/// cf-ship records the completion after review. A tip that adds anything
/// else still needs a review that names it.
#[test]
#[cfg(unix)]
fn a_pin_may_add_only_the_predecessors_status_and_closeout_to_its_review() {
    for code_after_review in [false, true] {
        let dir = fixture();
        let root = dir.path();
        let bin = tempfile::tempdir().unwrap();
        let branch = "task/TSK-001-work";
        let (reviewed, mut tip) = reviewed_predecessor(root, branch);
        if code_after_review {
            git(root, &["switch", "-q", branch]);
            write(root, "src/late.rs", "pub fn late() {}\n");
            git(root, &["add", "."]);
            git(root, &["commit", "-qm", "feat: add code after the review"]);
            tip = git(root, &["rev-parse", "HEAD"]);
            git(root, &["switch", "-q", "main"]);
        }
        review_tool_naming(bin.path(), branch, &tip, &reviewed);
        let on = format!("TSK-001@{tip}");
        let out = cli(
            root,
            &["work", "claim", "TSK-002", "--on", &on],
            Some(bin.path()),
        );
        if code_after_review {
            assert!(!out.status.success(), "{}", text(&out));
            assert!(text(&out).contains("no review names"), "{}", text(&out));
        } else {
            assert!(out.status.success(), "{}", text(&out));
            let child = "task/TSK-002-work-tsk-002";
            let check = push_check(root, child, child, bin.path());
            assert!(check.status.success(), "{}", text(&check));
        }
    }
}

/// Issue #69: a branch stacked on a reviewed predecessor head answers the
/// journey rule only for the paths it changes after that head; the
/// predecessor's own pull request answers for its product code, and a merge
/// of the target brings in nothing the successor answers for. A product
/// change the successor makes itself still needs its journey criterion,
/// and so does a deletion of the predecessor's file, which leaves no
/// difference from the target, also when it is resolved inside a merge
/// (TSK-234 review rounds 1 and 2). An octopus merge cannot be read
/// against a remerge, so it refuses (design D4).
#[test]
#[cfg(unix)]
fn a_stacked_branch_answers_the_journey_rule_for_its_own_paths() {
    let dir = fixture();
    let root = dir.path();
    write(
        root,
        ".codeflow/policy.json",
        "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\"]}\n}\n",
    );
    write(
        root,
        "project-management/tasks/TSK-002.md",
        &record("TSK-002", "[TSK-001]").replace(
            "- AC-1 When run, the command shall succeed. (journey)\n",
            "- AC-1 When read, the notes shall explain the work.\n",
        ),
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: plan a notes task"]);
    let bin = tempfile::tempdir().unwrap();
    let branch = "task/TSK-001-work";
    let (_, pin) = reviewed_predecessor(root, branch);
    review_tool_naming(bin.path(), branch, &pin, &pin);
    succeeds(&cli(
        root,
        &[
            "work",
            "claim",
            "TSK-002",
            "--on",
            &format!("TSK-001@{pin}"),
        ],
        Some(bin.path()),
    ));
    let child = "task/TSK-002-work-tsk-002";
    git(root, &["switch", "-q", child]);
    write(root, "docs/notes.md", "Notes.\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: write the notes"]);
    let notes = push_check(root, child, "HEAD", bin.path());
    assert!(notes.status.success(), "{}", text(&notes));
    let unmerged = git(root, &["rev-parse", "HEAD"]);
    git(root, &["switch", "-q", "main"]);
    write(root, "src/m.rs", "pub fn m() {}\n");
    git(root, &["add", "."]);
    git(
        root,
        &["commit", "-qm", "feat: other work lands on the target"],
    );
    git(root, &["switch", "-qc", "side", "main~1"]);
    write(root, "docs/side.md", "Side.\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: a side note"]);
    git(root, &["switch", "-q", child]);
    git(root, &["merge", "-q", "--no-edit", "main"]);
    let merged = push_check(root, child, "HEAD", bin.path());
    assert!(merged.status.success(), "{}", text(&merged));
    let before = git(root, &["rev-parse", "HEAD"]);
    for (change, needle) in [
        ("delete", "src/a.rs"),
        ("add", "src/c.rs"),
        ("delete inside the merge", "src/a.rs"),
        ("octopus merge", "two-parent merge"),
    ] {
        git(root, &["reset", "-q", "--hard", &before]);
        match change {
            "delete" => {
                git(root, &["rm", "-q", "src/a.rs"]);
                git(root, &["commit", "-qm", "feat: change product code too"]);
            }
            "add" => {
                write(root, "src/c.rs", "pub fn c() {}\n");
                git(root, &["add", "."]);
                git(root, &["commit", "-qm", "feat: change product code too"]);
            }
            "delete inside the merge" => {
                // The merge's tree matches the target's side for src/a.rs,
                // so only its difference from the clean remerge shows it.
                git(root, &["reset", "-q", "--hard", &unmerged]);
                git(root, &["merge", "-q", "--no-ff", "--no-commit", "main"]);
                git(root, &["rm", "-q", "src/a.rs"]);
                git(root, &["commit", "-qm", "chore: merge the target"]);
            }
            _ => {
                git(root, &["reset", "-q", "--hard", &unmerged]);
                git(root, &["merge", "-q", "--no-edit", "main", "side"]);
                let parents = git(root, &["rev-list", "--parents", "-n", "1", "HEAD"]);
                assert_eq!(parents.split(' ').count(), 4, "an octopus merge: {parents}");
            }
        }
        let product = push_check(root, child, "HEAD", bin.path());
        assert!(!product.status.success(), "{change}: {}", text(&product));
        assert!(
            text(&product).contains(needle),
            "{change}: {}",
            text(&product)
        );
        if change != "octopus merge" {
            assert!(
                text(&product).contains("work.journey_criterion"),
                "{change}: {}",
                text(&product)
            );
        }
    }
}

/// Issue #69 keeps the predecessor's record as its review left it: a
/// branch stacked on a reviewed head that reverts the predecessor's status
/// and drops its Closeout, keeping its criteria, is refused (TSK-234
/// review).
#[test]
#[cfg(unix)]
fn a_stacked_branch_keeps_its_predecessors_record() {
    let dir = fixture();
    let root = dir.path();
    let bin = tempfile::tempdir().unwrap();
    let branch = "task/TSK-001-work";
    let (_, pin) = reviewed_predecessor(root, branch);
    review_tool_naming(bin.path(), branch, &pin, &pin);
    succeeds(&cli(
        root,
        &[
            "work",
            "claim",
            "TSK-002",
            "--on",
            &format!("TSK-001@{pin}"),
        ],
        Some(bin.path()),
    ));
    let child = "task/TSK-002-work-tsk-002";
    git(root, &["switch", "-q", child]);
    let path = "project-management/tasks/TSK-001.md";
    let content = std::fs::read_to_string(root.join(path)).unwrap();
    let reverted = content
        .split("## Closeout")
        .next()
        .unwrap()
        .replace("status: complete", "status: todo");
    write(root, path, &reverted);
    git(root, &["add", "."]);
    git(
        root,
        &["commit", "-qm", "docs: drop the predecessor's completion"],
    );
    let out = push_check(root, child, "HEAD", bin.path());
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("TSK-001 differs on this branch from its reviewed head"),
        "{}",
        text(&out)
    );
}

/// Readiness reads the predecessor's current status, never its history
/// (TSK-234 design D3a): a predecessor that landed complete and was then
/// reopened on the target is not a satisfied dependency, so its successor
/// is not claimed without a reviewed pin.
#[test]
#[cfg(unix)]
fn a_reopened_predecessor_is_not_ready_without_a_pin() {
    let dir = fixture();
    let root = dir.path();
    let branch = "task/TSK-001-work";
    reviewed_predecessor(root, branch);
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "chore: land TSK-001",
            branch,
        ],
    );
    let path = "project-management/tasks/TSK-001.md";
    let landed = std::fs::read_to_string(root.join(path)).unwrap();
    assert!(landed.contains("status: complete"), "{landed}");
    let reopened = landed.replace("status: complete", "status: todo").replace(
        "acceptance:\n",
        "acceptance_superseded:\n  reason: fix the predecessor\n",
    );
    write(root, path, &reopened);
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: reopen TSK-001"]);
    let claim = cli(root, &["work", "claim", "TSK-002"], None);
    assert!(!claim.status.success(), "{}", text(&claim));
    assert!(text(&claim).contains("TSK-001"), "{}", text(&claim));
}

/// TSK-234 review round 6: a file literally named
/// `project-management\tasks\TSK-001.md` is not the task's record, so a pin
/// that adds it after the review changed more than the record's status and
/// Closeout, and the claim refuses it.
#[cfg(unix)]
#[test]
fn a_pin_cannot_add_a_path_that_reads_like_the_record() {
    let dir = fixture();
    let root = dir.path();
    let bin = tempfile::tempdir().unwrap();
    let branch = "task/TSK-001-work";
    let (reviewed, _) = reviewed_predecessor(root, branch);
    git(root, &["switch", branch]);
    write(
        root,
        r"project-management\tasks\TSK-001.md",
        "an unreviewed file\n",
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "feat: add an unrelated file"]);
    let pin = git(root, &["rev-parse", "HEAD"]);
    review_tool_naming(bin.path(), branch, &pin, &reviewed);
    git(root, &["switch", "main"]);
    let out = cli(
        root,
        &[
            "work",
            "claim",
            "TSK-002",
            "--on",
            &format!("TSK-001@{pin}"),
        ],
        Some(bin.path()),
    );
    assert!(
        !out.status.success(),
        "the pin changed a file outside the record after review:\n{}",
        text(&out)
    );
}
