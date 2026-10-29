//! Tier-floor end-to-end: enforcement is the floor (ADR-0019). Runs the REAL
//! binary against the REAL embedded asset tree in a tempdir and pins the tier
//! boundaries the redesign establishes:
//!
//!   1. `init --minimal` installs the complete four-plane enforcement floor
//!      (five git hooks, CI, in-session guards, armed policy, lean contract) and
//!      NONE of the method (no skills, no spine docs, no project-management).
//!   2. `init --standard` adds the method on top of that floor.
//!   3. `init --full` adds project-management.
//!   4. `update` from an OLD-minimal install (only the pre-redesign files
//!      recorded and present) reconciles the newly-in-tier files into place —
//!      the agent-os upgrade path, the load-bearing case.
//!   5. tier boundaries hold (minimal excludes method; standard excludes pm).
//!   6. a real enforcement outcome: the commit-msg hook the floor installs
//!      rejects a non-conventional commit under the shipped block policy.
//!
//! The update engine already installs a manifest entry that is in-tier but
//! missing (every ownership branch has a `!dest.exists()` -> install path), so
//! test #4 needs no engine change — it pins that reconciliation so a future
//! refactor cannot silently regress the upgrade path.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The five git-hook shims that make up the git-client enforcement plane.
const FLOOR_HOOKS: [&str; 5] = [
    ".codeflow/git-hooks/pre-commit",
    ".codeflow/git-hooks/commit-msg",
    ".codeflow/git-hooks/pre-push",
    ".codeflow/git-hooks/pre-merge-commit",
    ".codeflow/git-hooks/reference-transaction",
];

/// Dests that the redesign MOVED into the minimal tier — absent from an
/// old-minimal install, and what `update` must reconcile into place.
const NEWLY_IN_TIER: [&str; 10] = [
    ".codeflow/git-hooks/commit-msg",
    ".codeflow/git-hooks/pre-push",
    ".codeflow/git-hooks/pre-merge-commit",
    ".codeflow/git-hooks/reference-transaction",
    ".github/workflows/codeflow-ci.yml",
    ".claude/settings.json",
    ".codex/hooks.json",
    ".codex/config.toml",
    ".grok/hooks/codeflow.json",
    "CLAUDE.md",
];

/// Method artifacts — standard+ only; must be ABSENT at minimal.
const METHOD_ARTIFACTS: [&str; 5] = [
    ".claude/skills/cf-method/SKILL.md",
    ".agents/skills/cf-plan/SKILL.md",
    ".claude/agents/cf-reviewer.md",
    "docs/capabilities.md",
    ".claude/workflows/pipeline.workflow.js",
];

#[path = "support/process_folders.rs"]
mod process_folders;
use process_folders::isolated_home;

/// Runs the binary under test with the git-hook shims pointed back at it (they
/// `exec codeflow` from PATH) and host git config neutralized.
fn codeflow(dir: &Path, args: &[&str]) -> Output {
    codeflow_command(dir, args)
        .output()
        .expect("codeflow binary runs")
}

/// [`codeflow`] with `input` on stdin, such as the init answers.
fn codeflow_with_input(dir: &Path, args: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut child = codeflow_command(dir, args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("codeflow binary starts");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(input.as_bytes())
        .expect("write the answers");
    child.wait_with_output().expect("codeflow binary runs")
}

fn codeflow_command(dir: &Path, args: &[&str]) -> Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    let mut command = Command::new(&exe);
    command
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE");
    command
}

/// Runs `git` in `dir` with the binary on PATH (so wired hooks fire) and an
/// author identity, host config neutralized.
fn git(dir: &Path, args: &[&str]) -> Output {
    git_env(dir, args, &[])
}

/// [`git`] with extra environment, such as a human's override.
fn git_env(dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("PATH", path)
        .env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .envs(envs.iter().copied())
        .output()
        .expect("git runs")
}

fn init(dir: &Path, tier_flag: &str) -> String {
    let out = codeflow(dir, &["init", "--yes", tier_flag]);
    let report = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "init {tier_flag} failed: {report}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !report.contains("CONFLICT") && !report.contains("missing-asset"),
        "init {tier_flag} conflicted or missed an asset:\n{report}"
    );
    report
}

fn exists(root: &Path, rel: &str) -> bool {
    root.join(rel).exists()
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

#[cfg(unix)]
fn is_executable(root: &Path, rel: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(root.join(rel))
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
#[cfg(not(unix))]
fn is_executable(_root: &Path, _rel: &str) -> bool {
    true
}

/// A tempdir project directory (`proj/` under a tempdir so the project name is
/// stable and never the tempdir's random name).
fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    (dir, root)
}

#[test]
fn init_minimal_installs_the_four_plane_floor_and_not_the_method() {
    let (_tmp, root) = project();
    init(&root, "--minimal");

    // Plane 1 — git hooks: all five present and executable.
    for hook in FLOOR_HOOKS {
        assert!(exists(&root, hook), "minimal missing git hook {hook}");
        assert!(is_executable(&root, hook), "git hook {hook} not executable");
    }
    // Plane 2 — CI.
    assert!(
        exists(&root, ".github/workflows/codeflow-ci.yml"),
        "minimal missing CI workflow"
    );
    // Plane 3 — in-session guards (Claude settings + codex starter).
    assert!(
        exists(&root, ".claude/settings.json"),
        "minimal missing .claude/settings.json"
    );
    let settings = read(&root, ".claude/settings.json");
    assert!(
        settings.contains("codeflow hook git-guard")
            && settings.contains("codeflow hook exec-guard"),
        "minimal settings.json is missing the git-guard/exec-guard wiring:\n{settings}"
    );
    assert!(
        exists(&root, ".codex/config.toml"),
        "minimal missing .codex/config.toml"
    );
    assert!(
        exists(&root, ".codex/hooks.json"),
        "minimal missing .codex/hooks.json"
    );
    assert!(
        exists(&root, ".grok/hooks/codeflow.json"),
        "minimal missing .grok/hooks/codeflow.json"
    );
    // The command rules and sandbox profiles generated for Codex and Grok
    // (ADR-0075).
    assert!(
        read(&root, ".codex/rules/codeflow.rules").contains("decision = \"forbidden\""),
        "minimal missing the Codex command rules"
    );
    assert!(
        read(&root, ".grok/sandbox.toml").contains("[profiles.cf-guard-worktree]"),
        "minimal missing the Grok sandbox profiles"
    );
    // Plane 4 — the armed policy, at block level.
    let policy = read(&root, ".codeflow/policy.json");
    for rule in [
        "commit_format",
        "secret_scan",
        "branch_naming",
        "ai_attribution",
        "commit_emoji",
    ] {
        assert!(
            policy.contains(&format!("\"{rule}\": \"block\"")),
            "minimal policy.json rule {rule} is not block-level:\n{policy}"
        );
    }
    // Lean contract present.
    assert!(exists(&root, "AGENTS.md"), "minimal missing AGENTS.md");
    assert!(exists(&root, "CLAUDE.md"), "minimal missing CLAUDE.md");
    assert!(
        read(&root, "CLAUDE.md").contains("@AGENTS.md"),
        "lean CLAUDE.md must @-include AGENTS.md"
    );

    // The METHOD is absent — minimal installs no standard-only entry (boundary #5).
    for artifact in METHOD_ARTIFACTS {
        assert!(
            !exists(&root, artifact),
            "minimal must NOT install the method artifact {artifact}"
        );
    }
    assert!(
        !exists(&root, ".claude/skills"),
        "minimal must not install .claude/skills"
    );
    assert!(!exists(
        &root,
        ".agents/skills/cf-customize/references/claude-context-policy.md"
    ));
    assert!(
        !exists(&root, "project-management"),
        "minimal must not install project-management"
    );
    assert!(
        !exists(&root, "docs/product.md"),
        "minimal must not install the docs spine"
    );
}

#[test]
fn init_standard_adds_the_method_and_not_pm() {
    let (_tmp, root) = project();
    init(&root, "--standard");

    // The floor is still there (spot-check one hook, CI, guards).
    assert!(exists(&root, ".codeflow/git-hooks/commit-msg"));
    assert!(exists(&root, ".github/workflows/codeflow-ci.yml"));
    assert!(read(&root, ".claude/settings.json").contains("codeflow hook git-guard"));

    // The method is now installed.
    for artifact in METHOD_ARTIFACTS {
        assert!(
            exists(&root, artifact),
            "standard must install the method artifact {artifact}"
        );
    }
    assert!(exists(&root, "docs/architecture.md"));
    assert!(exists(&root, "docs/product.md"));
    assert!(
        exists(&root, ".agents/skills/cf-method/SKILL.md"),
        "cross-harness mirror ships at standard"
    );
    for artifact in [
        ".claude/skills/cf-customize/references/claude-context-policy.md",
        ".agents/skills/cf-customize/references/claude-context-policy.md",
    ] {
        assert!(
            exists(&root, artifact),
            "standard must install context policy reference {artifact}"
        );
    }

    // Project-management is NOT — that is the full tier (boundary #5).
    assert!(
        !exists(&root, "project-management/templates/epic.md"),
        "standard must NOT install project-management"
    );
}

/// The starter docs a fresh scaffold writes must pass the doc-graph check
/// they ship with, at every tier that writes them. Codex review round 2 of
/// the dash-free starter templates: an unquoted `: ` in the ADR-0001 title
/// made its frontmatter unparseable, and only the dash scan was tested.
#[test]
fn fresh_standard_and_full_starter_docs_validate() {
    for tier in ["--standard", "--full"] {
        let (_tmp, root) = project();
        init(&root, tier);
        let adr = read(&root, "docs/decisions/ADR-0001-stack-choice.md");
        assert!(
            !adr.contains('\u{2014}') && !adr.contains('\u{2013}'),
            "{tier}: starter ADR carries a policy character:\n{adr}"
        );
        let out = codeflow(&root, &["validate", "--docs"]);
        assert!(
            out.status.success(),
            "{tier}: validate --docs failed on a fresh scaffold:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn init_full_adds_project_management() {
    let (_tmp, root) = project();
    init(&root, "--full");

    for tmpl in [
        "project-management/templates/epic.md",
        "project-management/templates/task.md",
        "project-management/templates/spec.md",
    ] {
        assert!(exists(&root, tmpl), "full must install {tmpl}");
    }
    // The floor and method are still present at full.
    assert!(exists(&root, ".codeflow/git-hooks/reference-transaction"));
    assert!(exists(&root, ".claude/skills/cf-method/SKILL.md"));
}

/// THE load-bearing test: an old-minimal repo (only the pre-redesign files
/// recorded and present) gains the newly-in-tier enforcement on `codeflow
/// update`. We build a real minimal install, then roll it back to the
/// old-minimal shape (delete the moved files + baselines, drop their manifest
/// records, roll the scaffold version back), then run update and assert the
/// engine reconciles every moved file into place and records it.
#[test]
fn update_from_old_minimal_installs_the_newly_in_tier_files() {
    let (_tmp, root) = project();
    init(&root, "--minimal");
    simulate_old_minimal(&root);

    // Precondition: the moved files really are gone.
    for dest in NEWLY_IN_TIER {
        assert!(
            !exists(&root, dest),
            "old-minimal fixture should not contain {dest}"
        );
    }

    let out = codeflow(&root, &["update"]);
    let report = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "update failed: {report}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!report.contains("CONFLICT"), "update conflicted:\n{report}");
    assert!(report.contains("added"), "update added nothing:\n{report}");

    // Every moved file is now installed (hooks executable) and recorded.
    let records = manifest_files(&root);
    for dest in NEWLY_IN_TIER {
        assert!(
            exists(&root, dest),
            "update did not install in-tier-but-missing {dest}:\n{report}"
        );
        if dest.starts_with(".codeflow/git-hooks/") {
            assert!(
                is_executable(&root, dest),
                "reconciled hook {dest} is not executable"
            );
        }
        assert!(
            records.get(dest).is_some(),
            "update installed {dest} but did not record it in the manifest"
        );
    }
    // The reconciled guards are actually wired, not empty placeholders.
    assert!(
        read(&root, ".claude/settings.json").contains("codeflow hook git-guard"),
        "reconciled settings.json lost the git-guard wiring"
    );
}

/// A real enforcement outcome at the floor: after `init --minimal`, the
/// commit-msg hook rejects a non-conventional message under the block policy —
/// both as a direct hook invocation and as a real `git commit` on a feature
/// branch (the wired hook fires and blocks the commit).
#[test]
fn minimal_floor_blocks_a_non_conventional_commit() {
    let (_tmp, root) = project();
    init(&root, "--minimal");

    // Direct hook invocation: bad message -> non-zero, good message -> zero.
    let bad = root.join("bad-msg.txt");
    std::fs::write(&bad, "this is not a conventional subject\n").unwrap();
    let bad_out = codeflow(&root, &["git-hook", "commit-msg", bad.to_str().unwrap()]);
    let bad_msg = format!(
        "{}{}",
        String::from_utf8_lossy(&bad_out.stdout),
        String::from_utf8_lossy(&bad_out.stderr)
    );
    assert!(
        !bad_out.status.success(),
        "commit-msg hook accepted a non-conventional message: {bad_msg}"
    );
    assert!(
        bad_msg.contains("commit_format"),
        "commit-msg block did not name the commit_format rule:\n{bad_msg}"
    );
    let good = root.join("good-msg.txt");
    std::fs::write(&good, "feat: add a real feature\n").unwrap();
    assert!(
        codeflow(&root, &["git-hook", "commit-msg", good.to_str().unwrap()])
            .status
            .success(),
        "commit-msg hook rejected a valid conventional message"
    );

    // End-to-end: a real bad commit on a non-protected feature branch is blocked
    // by the wired hook (proving the plane is actually armed, not just present).
    assert!(git(&root, &["checkout", "-b", "feat/enforcement"])
        .status
        .success());
    std::fs::write(root.join("newfile.txt"), "hello\n").unwrap();
    assert!(git(&root, &["add", "newfile.txt"]).status.success());
    let commit = git(&root, &["commit", "-m", "not conventional at all"]);
    assert!(
        !commit.status.success(),
        "a non-conventional commit was accepted by the wired commit-msg hook"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&commit.stdout),
        String::from_utf8_lossy(&commit.stderr)
    );
    assert!(
        combined.contains("BLOCKED"),
        "commit rejection did not report a BLOCK:\n{combined}"
    );
}

/// Runs `git` and asserts success, naming the step on failure.
fn git_ok(dir: &Path, args: &[&str], what: &str) {
    let out = git(dir, args);
    assert!(
        out.status.success(),
        "{what} failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Commits one file on a new branch cut from `from`.
fn commit_on_branch(root: &Path, from: &str, branch: &str, file: &str, body: &str, msg: &[&str]) {
    git_ok(root, &["checkout", "-q", from], "checkout base");
    git_ok(root, &["checkout", "-q", "-b", branch], "checkout branch");
    std::fs::write(root.join(file), body).unwrap();
    git_ok(root, &["add", file], "add");
    let mut args = vec!["commit", "-q"];
    args.extend_from_slice(msg);
    git_ok(root, &args, "commit");
}

/// Pushes a branch and returns (success, stderr).
fn push(root: &Path, branch: &str) -> (bool, String) {
    let out = git(root, &["push", "-q", "origin", branch]);
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// TSK-132: at every tier a fresh scaffold ships `test_gate_on_push` at
/// block, and a real `git push` runs the fast push set: `codeflow ci` on the
/// pushed range blocks a commit that slipped past commit-msg, a failing
/// `quick` target blocks, and a clean push goes through with a timing note.
#[test]
fn push_set_blocks_a_bad_push_at_every_tier() {
    for tier in ["--minimal", "--standard", "--full"] {
        let (tmp, root) = project();
        init(&root, tier);
        let policy: serde_json::Value =
            serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
        assert_eq!(
            policy["git"]["test_gate_on_push"], "block",
            "{tier}: fresh scaffold must block on a failed push set"
        );
        let remote = tmp.path().join("remote.git");
        git_ok(
            tmp.path(),
            &["init", "--bare", "-q", "remote.git"],
            "bare init",
        );
        git_ok(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
            "remote add",
        );
        let head = git(&root, &["branch", "--show-current"]);
        let start = String::from_utf8_lossy(&head.stdout).trim().to_string();
        // The destination's landed history: the scaffold commit on its
        // default branch, fetched into the destination (a push to a
        // protected branch is refused). Only landed history bounds a new
        // branch's range.
        let refspec = format!("{start}:{start}");
        git_ok(
            &remote,
            &["fetch", "-q", root.to_str().unwrap(), &refspec],
            "seed the destination",
        );
        git_ok(&root, &["fetch", "-q", "origin"], "fetch the destination");

        // `init` already committed the scaffold; add one reviewed change.
        let good = ["-m", "chore: add a base file"];
        commit_on_branch(&root, &start, "feat/base", "base.txt", "base\n", &good);
        let (ok, err) = push(&root, "feat/base");
        assert!(ok, "{tier}: clean push blocked:\n{err}");
        assert!(err.contains("push set finished in"), "{tier}: {err}");

        // A commit that skipped commit-msg is caught by `codeflow ci` on the
        // pushed range.
        let bad = ["--no-verify", "-m", "Not conventional."];
        commit_on_branch(&root, "feat/base", "feat/bad", "a.txt", "a\n", &bad);
        let (ok, err) = push(&root, "feat/bad");
        assert!(!ok, "{tier}: bad push went through");
        assert!(
            err.contains("git.test_gate_on_push") && err.contains("codeflow ci"),
            "{tier}: block did not name the push set check:\n{err}"
        );

        // A failing `quick` target no longer runs on push (TSK-449): the push
        // set is `codeflow ci`, and the quick gate is cited at ready.
        let config = r#"{"schema_version": "1.0", "targets": [
  {"name": "lint", "runner": "custom", "modes": {"quick": {"command": "false"}, "full": {"command": "true"}}}
]}"#;
        let msg = ["-m", "chore: add a failing lint target"];
        let cfg = ".codeflow/test-config.json";
        commit_on_branch(&root, "feat/base", "feat/quick-fails", cfg, config, &msg);
        let (ok, err) = push(&root, "feat/quick-fails");
        assert!(ok, "{tier}: a quick target stopped the push:\n{err}");
        assert!(
            !err.contains("quick targets"),
            "{tier}: a quick target ran:\n{err}"
        );
    }
}

/// TSK-207 (sathyassn/codeflow#12): after `init --minimal`, a human's
/// `CODEFLOW_HUMAN_OVERRIDE=1` lets the first push of the default branch to
/// an empty remote, and a commit on it, through the installed hooks; without
/// it both are refused. Force-pushing and deleting the protected branch stay
/// refused with the override set.
#[test]
fn human_override_lands_protected_commits_and_pushes_but_nothing_destructive() {
    const HUMAN: &[(&str, &str)] = &[("CODEFLOW_HUMAN_OVERRIDE", "1")];
    let (tmp, root) = project();
    init(&root, "--minimal");
    git_ok(
        tmp.path(),
        &["init", "--bare", "-q", "remote.git"],
        "bare init",
    );
    let remote = tmp.path().join("remote.git");
    git_ok(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
        "remote add",
    );
    let head = git(&root, &["branch", "--show-current"]);
    let main = String::from_utf8_lossy(&head.stdout).trim().to_string();
    let stderr = |out: &Output| String::from_utf8_lossy(&out.stderr).to_string();

    // The bootstrap push: refused without the override, landed with it.
    let out = git(&root, &["push", "-q", "origin", &main]);
    assert!(
        !out.status.success(),
        "agent-path bootstrap push went through"
    );
    assert!(
        stderr(&out).contains("git.push_to_protected"),
        "{}",
        stderr(&out)
    );
    let out = git_env(&root, &["push", "-q", "origin", &main], HUMAN);
    assert!(
        out.status.success(),
        "human bootstrap push refused:\n{}",
        stderr(&out)
    );

    // A commit on the protected branch: refused without, made with it.
    std::fs::write(root.join("notes.txt"), "one\n").unwrap();
    git_ok(&root, &["add", "notes.txt"], "add");
    let out = git(&root, &["commit", "-q", "-m", "docs: add notes"]);
    assert!(
        !out.status.success(),
        "agent-path protected commit went through"
    );
    assert!(
        stderr(&out).contains("git.commit_to_protected"),
        "{}",
        stderr(&out)
    );
    let out = git_env(&root, &["commit", "-q", "-m", "docs: add notes"], HUMAN);
    assert!(
        out.status.success(),
        "human protected commit refused:\n{}",
        stderr(&out)
    );
    let out = git_env(&root, &["push", "-q", "origin", &main], HUMAN);
    assert!(
        out.status.success(),
        "human fast-forward push refused:\n{}",
        stderr(&out)
    );

    // Destructive updates stay refused with the override.
    let amend = ["commit", "-q", "--amend", "-m", "docs: add the notes"];
    git_ok_env(&root, &amend, HUMAN, "amend");
    let out = git_env(&root, &["push", "-q", "-f", "origin", &main], HUMAN);
    assert!(
        !out.status.success(),
        "force push of a protected branch went through"
    );
    assert!(
        stderr(&out).contains("git.force_push_protected"),
        "{}",
        stderr(&out)
    );
    let delete = format!(":{main}");
    let out = git_env(&root, &["push", "-q", "origin", &delete], HUMAN);
    assert!(
        !out.status.success(),
        "deletion of a protected branch went through"
    );
    assert!(
        stderr(&out).contains("git.delete_protected"),
        "{}",
        stderr(&out)
    );

    refuses_a_force_push_over_an_unfetched_tip(tmp.path(), &root, &remote, &main);
}

/// A remote tip a clone never fetched cannot be proven a fast-forward, so
/// a human's forced push over it is refused (TSK-207).
fn refuses_a_force_push_over_an_unfetched_tip(tmp: &Path, root: &Path, remote: &Path, main: &str) {
    const HUMAN: &[(&str, &str)] = &[("CODEFLOW_HUMAN_OVERRIDE", "1")];
    // Another clone moves the remote; a forced push from here would discard
    // a commit this clone cannot see.
    let other = tmp.join("other");
    codeflow_fixture::clone(tmp, remote.to_str().unwrap(), "other")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .run();
    std::fs::write(other.join("more.txt"), "two\n").unwrap();
    git_ok(&other, &["add", "more.txt"], "add in the other clone");
    git_ok(&other, &["commit", "-q", "-m", "docs: add more"], "commit");
    git_ok(
        &other,
        &["push", "-q", "origin", main],
        "push from the other clone",
    );
    let out = git_env(root, &["push", "-q", "-f", "origin", main], HUMAN);
    assert!(
        !out.status.success(),
        "force push over an unfetched remote tip went through"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("cannot be proven a fast-forward"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// [`git_ok`] with extra environment.
fn git_ok_env(dir: &Path, args: &[&str], envs: &[(&str, &str)], what: &str) {
    let out = git_env(dir, args, envs);
    assert!(
        out.status.success(),
        "{what} failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The restored v1 commit standard (ADR-0020) is enforced by the floor: after
/// `init --minimal`, the commit-msg hook rejects a story-body commit — a
/// conventional subject followed by a prose paragraph — both as a direct hook
/// invocation naming `git.commit_body` and as a real `git commit` on a feature
/// branch, while a bullet-body commit of the same change passes.
#[test]
fn minimal_floor_blocks_a_story_body_commit() {
    let (_tmp, root) = project();
    init(&root, "--minimal");

    // Direct hook invocation: a prose body is rejected and names the body rule.
    let story = root.join("story-msg.txt");
    std::fs::write(
        &story,
        "feat: add a thing\n\nThis is a story paragraph about why the thing was added.\n",
    )
    .unwrap();
    let out = codeflow(&root, &["git-hook", "commit-msg", story.to_str().unwrap()]);
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !out.status.success(),
        "commit-msg hook accepted a story-body commit: {msg}"
    );
    assert!(
        msg.contains("commit_body"),
        "commit-msg block did not name the commit_body rule:\n{msg}"
    );

    // The same change with a bullet body passes.
    let bullets = root.join("bullet-msg.txt");
    std::fs::write(&bullets, "feat: add a thing\n\n- wire the new path\n").unwrap();
    assert!(
        codeflow(
            &root,
            &["git-hook", "commit-msg", bullets.to_str().unwrap()]
        )
        .status
        .success(),
        "commit-msg hook rejected a conforming bullet-body commit"
    );

    // End-to-end: a real story-body commit on a feature branch is blocked by the
    // wired hook.
    assert!(git(&root, &["checkout", "-b", "feat/body-shape"])
        .status
        .success());
    std::fs::write(root.join("body.txt"), "hello\n").unwrap();
    assert!(git(&root, &["add", "body.txt"]).status.success());
    let commit = git(
        &root,
        &[
            "commit",
            "-m",
            "feat: add a thing",
            "-m",
            "This is a story paragraph, not a bullet.",
        ],
    );
    assert!(
        !commit.status.success(),
        "a story-body commit was accepted by the wired commit-msg hook"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&commit.stdout),
        String::from_utf8_lossy(&commit.stderr)
    );
    assert!(
        combined.contains("BLOCKED") && combined.contains("commit_body"),
        "commit rejection did not report a commit_body BLOCK:\n{combined}"
    );
}

/// The footer/ticket amendment (ADR-0020): the shipped floor is STRICT by
/// default — a standard git-trailer footer (`Refs:`) blocks because no trailer is
/// opted in, and the same trailer commits cleanly only after the project adds the
/// token to `commit_footer_tokens`. Proven end-to-end through the wired
/// commit-msg hook and a real `git commit`.
#[test]
fn minimal_floor_is_strict_then_opt_in_for_trailers() {
    let (_tmp, root) = project();
    init(&root, "--minimal");

    // Direct hook invocation: a Refs footer BLOCKS at the shipped strict default.
    let msg = root.join("trailer-msg.txt");
    std::fs::write(&msg, "feat: add a thing\n\n- wire it\n\nRefs: PROJ-142\n").unwrap();
    let blocked = codeflow(&root, &["git-hook", "commit-msg", msg.to_str().unwrap()]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&blocked.stdout),
        String::from_utf8_lossy(&blocked.stderr)
    );
    assert!(
        !blocked.status.success() && text.contains("commit_body"),
        "an unopted `Refs:` trailer must block at the strict floor:\n{text}"
    );

    // Opt `Refs` into the project's policy, then the same trailer is accepted.
    let policy_path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&policy_path).unwrap()).unwrap();
    policy["git"]["commit_footer_tokens"] = serde_json::json!(["Refs"]);
    std::fs::write(&policy_path, serde_json::to_string_pretty(&policy).unwrap()).unwrap();

    let allowed = codeflow(&root, &["git-hook", "commit-msg", msg.to_str().unwrap()]);
    assert!(
        allowed.status.success(),
        "an opted-in `Refs:` trailer must pass: {}{}",
        String::from_utf8_lossy(&allowed.stdout),
        String::from_utf8_lossy(&allowed.stderr)
    );

    // End-to-end: with the opt-in in place, a real Refs-trailer commit lands.
    assert!(git(&root, &["checkout", "-b", "feat/trailer"])
        .status
        .success());
    std::fs::write(root.join("trailer.txt"), "hello\n").unwrap();
    assert!(git(&root, &["add", "."]).status.success());
    let commit = git(
        &root,
        &[
            "commit",
            "-m",
            "feat: add a thing",
            "-m",
            "- wire it",
            "-m",
            "Refs: PROJ-142",
        ],
    );
    assert!(
        commit.status.success(),
        "an opted-in Refs-trailer commit was rejected by the wired hook:\n{}{}",
        String::from_utf8_lossy(&commit.stdout),
        String::from_utf8_lossy(&commit.stderr)
    );
}

/// Rolls a fresh `--minimal` install back to the OLD-minimal shape: the moved
/// files and their baselines are deleted, their manifest records dropped, and
/// the recorded scaffold version rolled back — exactly what a repo initialized
/// before the redesign looks like on disk.
fn simulate_old_minimal(root: &Path) {
    let mpath = root.join(".codeflow/manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&mpath).unwrap()).unwrap();
    {
        let files = manifest["files"]
            .as_object_mut()
            .expect("manifest.files is an object");
        for dest in NEWLY_IN_TIER {
            files.remove(dest);
        }
    }
    manifest["scaffold_version"] = serde_json::Value::from("2.1.0");
    std::fs::write(&mpath, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

    for dest in NEWLY_IN_TIER {
        let _ = std::fs::remove_file(root.join(dest));
        let _ = std::fs::remove_file(root.join(".codeflow/.baseline").join(dest));
    }

    let ppath = root.join(".codeflow/project.toml");
    let rolled = std::fs::read_to_string(&ppath)
        .unwrap()
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("scaffold_version") {
                "scaffold_version = \"2.1.0\"".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&ppath, format!("{rolled}\n")).unwrap();
}

/// The `files` map from `.codeflow/manifest.json`.
fn manifest_files(root: &Path) -> serde_json::Map<String, serde_json::Value> {
    let manifest: serde_json::Value =
        serde_json::from_str(&read(root, ".codeflow/manifest.json")).unwrap();
    manifest["files"].as_object().cloned().unwrap_or_default()
}

/// TSK-137: at every tier the fresh policy has no `human_authorization` key
/// and validates without a deprecation warning; an adopter's policy that
/// still has the key validates with one warning.
#[test]
fn fresh_policy_has_no_human_authorization_at_every_tier() {
    for tier in ["--minimal", "--standard", "--full"] {
        let (_tmp, root) = project();
        init(&root, tier);
        let text = read(&root, ".codeflow/policy.json");
        assert!(!text.contains("human_authorization"), "{tier}: {text}");
        let out = codeflow(&root, &["validate"]);
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        assert!(out.status.success(), "{tier}: {err}");
        assert!(!err.contains("deprecated"), "{tier}: {err}");

        let mut policy: serde_json::Value = serde_json::from_str(&text).unwrap();
        policy["human_authorization"] = serde_json::Value::from("none");
        std::fs::write(
            root.join(".codeflow/policy.json"),
            serde_json::to_string_pretty(&policy).unwrap(),
        )
        .unwrap();
        let out = codeflow(&root, &["validate"]);
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        assert!(out.status.success(), "{tier}: {err}");
        assert_eq!(
            err.matches("human_authorization is deprecated").count(),
            1,
            "{tier}: {err}"
        );
    }
}

/// Every `PreToolUse` hook command in a harness wiring file.
#[cfg(unix)]
fn pretooluse_commands(root: &Path, rel: &str) -> Vec<String> {
    let wiring: serde_json::Value = serde_json::from_str(&read(root, rel)).unwrap();
    wiring["hooks"]["PreToolUse"]
        .as_array()
        .unwrap_or_else(|| panic!("{rel}: no PreToolUse"))
        .iter()
        .flat_map(|entry| entry["hooks"].as_array().cloned().unwrap_or_default())
        .filter_map(|hook| hook["command"].as_str().map(ToString::to_string))
        .collect()
}

/// Run a wired hook command through the shell, as the harness does, with a
/// Bash tool call for `command` on stdin.
#[cfg(unix)]
fn run_wired(root: &Path, hook: &str, command: &str) -> Output {
    use std::io::Write as _;
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": root,
    });
    let mut child = Command::new("sh")
        .args(["-c", hook])
        .current_dir(root)
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("hook runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[cfg(unix)]
#[test]
fn headless_peer_runs_block_in_every_harness_wiring_at_every_tier() {
    // TSK-136 AC-4 as amended by ADR-0075 D4: a fresh install at every tier
    // ships the guard at `block`, and the Claude, Codex and Grok wiring each
    // run it; a project that sets `warn` is advised only.
    for tier in ["--minimal", "--standard", "--full"] {
        let (_tmp, root) = project();
        init(&root, tier);
        let policy: serde_json::Value =
            serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
        assert_eq!(
            policy["security"]["headless_peer_runs"], "block",
            "{tier}: fresh policy level"
        );
        for wiring in [
            ".claude/settings.json",
            ".codex/hooks.json",
            ".grok/hooks/codeflow.json",
        ] {
            let hook = pretooluse_commands(&root, wiring)
                .into_iter()
                .find(|command| command.contains("hook exec-guard"))
                .unwrap_or_else(|| panic!("{tier} {wiring}: exec-guard not wired"));
            let out = run_wired(&root, &hook, "codex exec 'review the diff'");
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert_eq!(out.status.code(), Some(2), "{tier} {wiring}: {stderr}");
            assert!(
                stderr.contains("headless peer run"),
                "{tier} {wiring}: {stderr}"
            );
            let out = run_wired(&root, &hook, "codex --version");
            assert!(out.stderr.is_empty(), "{tier} {wiring}: negative warned");
        }

        let mut policy = policy;
        policy["security"]["headless_peer_runs"] = "warn".into();
        std::fs::write(
            root.join(".codeflow/policy.json"),
            serde_json::to_string_pretty(&policy).unwrap(),
        )
        .unwrap();
        // AC-1 without a remote reads HEAD, not an uncommitted policy edit.
        git_ok(
            &root,
            &["switch", "-qc", "feat/policy-warning"],
            "policy branch",
        );
        git_ok(&root, &["add", ".codeflow/policy.json"], "stage policy");
        git_ok(
            &root,
            &["commit", "-qm", "chore: record warning policy"],
            "commit policy",
        );
        for wiring in [
            ".claude/settings.json",
            ".codex/hooks.json",
            ".grok/hooks/codeflow.json",
        ] {
            let hook = pretooluse_commands(&root, wiring)
                .into_iter()
                .find(|command| command.contains("hook exec-guard"))
                .unwrap();
            let out = run_wired(&root, &hook, "claude -p 'summarize'");
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert_eq!(out.status.code(), Some(0), "{tier} {wiring}: warn level");
            assert!(
                stderr.contains("headless peer run"),
                "{tier} {wiring}: {stderr}"
            );
        }
    }
}

#[path = "../../codeflow-core/src/security/guard_forms.rs"]
#[allow(dead_code)]
mod guard_forms;

#[cfg(unix)]
#[test]
fn every_harness_wiring_judges_composed_deletions_and_help_at_every_tier() {
    // TSK-141 AC-5: at every tier, the Claude, Codex and Grok wiring refuse
    // each composed deletion, pass each project deletion, and judge each
    // help invocation and its data twin as the unit tests do. The guard
    // only judges; nothing here runs the commands.
    for tier in ["--minimal", "--standard", "--full"] {
        let (_tmp, root) = project();
        init(&root, tier);
        for wiring in [
            ".claude/settings.json",
            ".codex/hooks.json",
            ".grok/hooks/codeflow.json",
        ] {
            let hook = pretooluse_commands(&root, wiring)
                .into_iter()
                .find(|command| command.contains("hook exec-guard"))
                .unwrap_or_else(|| panic!("{tier} {wiring}: exec-guard not wired"));
            for (form, _) in guard_forms::COMPOSED_PAIRS {
                let out = run_wired(&root, &hook, form);
                let stderr = String::from_utf8_lossy(&out.stderr);
                assert_eq!(
                    out.status.code(),
                    Some(2),
                    "{tier} {wiring} {form}: {stderr}"
                );
                assert!(
                    stderr.contains("security.dangerous_commands"),
                    "{tier} {wiring} {form}: {stderr}"
                );
            }
            for command in guard_forms::PROJECT_DELETIONS {
                let out = run_wired(&root, &hook, command);
                let stderr = String::from_utf8_lossy(&out.stderr);
                // TSK-397: the strict text rule, the default, may refuse a
                // word filled in at run time; the relaxed rule's decisions
                // are proven by the `text_guard_invariant` replay.
                if out.status.code() == Some(2)
                    && stderr.contains("the strict text rule cannot verify the run-time argument")
                {
                    continue;
                }
                assert_eq!(
                    out.status.code(),
                    Some(0),
                    "{tier} {wiring} {command}: {stderr}"
                );
                assert!(stderr.is_empty(), "{tier} {wiring} {command}: {stderr}");
            }
            for (help, twin) in guard_forms::HELP_PAIRS {
                let out = run_wired(&root, &hook, help);
                let stderr = String::from_utf8_lossy(&out.stderr);
                assert_eq!(
                    out.status.code(),
                    Some(0),
                    "{tier} {wiring} {help}: {stderr}"
                );
                assert!(stderr.is_empty(), "{tier} {wiring} {help}: {stderr}");
                // A fresh install blocks headless peer runs (ADR-0075 D4).
                let out = run_wired(&root, &hook, twin);
                let stderr = String::from_utf8_lossy(&out.stderr);
                assert_eq!(
                    out.status.code(),
                    Some(2),
                    "{tier} {wiring} {twin}: {stderr}"
                );
                assert!(
                    stderr.contains("headless peer run"),
                    "{tier} {wiring} {twin}: {stderr}"
                );
            }
        }
    }
}

// ---- The compaction default in the Claude settings (TSK-211) ---------------

const CLAUDE_SETTINGS: &str = ".claude/settings.json";
const CLAUDE_SETTINGS_BASELINE: &str = ".codeflow/.baseline/.claude/settings.json";

/// The `env` every Claude settings preset ships.
fn shipped_compaction_env() -> serde_json::Value {
    serde_json::json!({
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "1000000",
        "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "35"
    })
}

fn read_json(root: &Path, rel: &str) -> serde_json::Value {
    serde_json::from_str(&read(root, rel)).unwrap_or_else(|e| panic!("parse {rel}: {e}"))
}

fn write_json(root: &Path, rel: &str, value: &serde_json::Value) {
    std::fs::write(
        root.join(rel),
        format!("{}\n", serde_json::to_string_pretty(value).unwrap()),
    )
    .unwrap_or_else(|e| panic!("write {rel}: {e}"));
}

fn update(root: &Path) -> String {
    let out = codeflow(root, &["update"]);
    let report = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "update failed: {report}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    report
}

/// TSK-211 AC-1: `codeflow init` writes the compaction default, as strings,
/// at every tier and with every permission preset the init prompt offers.
#[test]
fn init_writes_the_compaction_default_at_every_tier_and_preset() {
    for tier_flag in ["--minimal", "--standard", "--full"] {
        for (answer, mode) in [
            ("default", None),
            ("acceptEdits", Some("acceptEdits")),
            ("bypassPermissions", Some("bypassPermissions")),
        ] {
            let (_tmp, root) = project();
            let out = codeflow_with_input(
                &root,
                &["init", tier_flag],
                &format!("proj\ncore\n{answer}\n"),
            );
            assert!(
                out.status.success(),
                "init {tier_flag} {answer}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let written = read_json(&root, CLAUDE_SETTINGS);
            assert_eq!(
                written["permissions"]
                    .get("defaultMode")
                    .and_then(serde_json::Value::as_str),
                mode,
                "{tier_flag} {answer}: the wrong preset was written"
            );
            assert_eq!(
                written["env"],
                shipped_compaction_env(),
                "{tier_flag} {answer}"
            );
        }
    }
}

/// TSK-211 AC-2: update keeps a project's own compaction choices. The
/// project removes one shipped key, changes the other and adds its own;
/// two updates leave the file as the project wrote it and print no value.
/// Deleting the whole `env` afterwards is kept the same way.
#[test]
fn update_keeps_the_projects_compaction_choices() {
    let (_tmp, root) = project();
    init(&root, "--minimal");
    let mut value = read_json(&root, CLAUDE_SETTINGS);
    value["env"] = serde_json::json!({
        "PROJECT_TOKEN": "opaque-project-value",
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "400000"
    });
    write_json(&root, CLAUDE_SETTINGS, &value);
    let before = read(&root, CLAUDE_SETTINGS);

    for attempt in 1..=2 {
        let report = update(&root);
        assert_eq!(
            read(&root, CLAUDE_SETTINGS),
            before,
            "update {attempt} changed the project's env:\n{report}"
        );
        assert!(
            !report.contains("opaque-project-value") && !report.contains("400000"),
            "update {attempt} printed an env value:\n{report}"
        );
    }

    value.as_object_mut().unwrap().remove("env");
    write_json(&root, CLAUDE_SETTINGS, &value);
    let before = read(&root, CLAUDE_SETTINGS);
    for attempt in 1..=2 {
        update(&root);
        assert_eq!(
            read(&root, CLAUDE_SETTINGS),
            before,
            "update {attempt} restored an env the project removed"
        );
    }
}

/// Roll a fresh install back to a release that shipped the compaction
/// percentage at `shipped`, with the project's own `env` entries on top.
fn roll_back_compaction(root: &Path, shipped: &str, project_env: &serde_json::Value) {
    for rel in [CLAUDE_SETTINGS, CLAUDE_SETTINGS_BASELINE] {
        let mut earlier = read_json(root, rel);
        earlier["env"]["CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"] = serde_json::json!(shipped);
        if rel == CLAUDE_SETTINGS {
            for (key, value) in project_env.as_object().unwrap() {
                earlier["env"][key] = value.clone();
            }
        }
        write_json(root, rel, &earlier);
    }
}

/// TSK-1029 review: an `env` value can be a secret, so neither the printed
/// report nor the `--diff` file of an update that changes the settings
/// shows one, even as context beside a migrated value.
#[test]
fn update_diff_never_shows_an_env_value() {
    // A synthetic value standing for a secret; no output may show it.
    const SECRET: &str = "opaque-neighbour-value-31337";
    let (tmp, root) = project();
    init(&root, "--minimal");
    roll_back_compaction(&root, "50", &serde_json::json!({"PROJECT_TOKEN": SECRET}));
    let diff_file = tmp.path().join("update.diff");

    let out = codeflow(
        &root,
        &["update", "--diff", diff_file.to_str().expect("utf-8 path")],
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "update failed: {stdout}\n{stderr}");
    let diff = std::fs::read_to_string(&diff_file).expect("diff file");

    let env = &read_json(&root, CLAUDE_SETTINGS)["env"];
    assert_eq!(env["CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"], "35", "{stdout}");
    assert_eq!(env["PROJECT_TOKEN"], SECRET, "{stdout}");
    assert!(
        diff.contains("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"),
        "the diff does not name the migrated key:\n{diff}"
    );
    for (name, text) in [("stdout", &stdout), ("stderr", &stderr), ("diff", &diff)] {
        assert!(!text.contains(SECRET), "{name} shows an env value:\n{text}");
    }
}

/// Run `codeflow update` with `extra` flags and `--diff`, and return its
/// stdout, its stderr and the diff file.
fn update_with_diff(tmp: &Path, root: &Path, extra: &[&str]) -> [String; 3] {
    let diff_file = tmp.join("update.diff");
    let mut args = vec!["update"];
    args.extend_from_slice(extra);
    args.extend(["--diff", diff_file.to_str().expect("utf-8 path")]);
    let out = codeflow(root, &args);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "update failed: {stdout}\n{stderr}");
    let diff = std::fs::read_to_string(&diff_file).expect("diff file");
    [stdout, stderr, diff]
}

/// TSK-1029 review 02: a project field that the shipped settings lack never reaches
/// the `--diff` file, whatever its name is spelled like and whatever type
/// its value has.
#[test]
fn update_diff_never_shows_a_project_api_key_value() {
    const SECRET: &str = "opaque-neighbour-value-31337";
    const NUMBER: i64 = 271_828;
    let (tmp, root) = project();
    init(&root, "--minimal");
    roll_back_compaction(&root, "50", &serde_json::json!({}));
    // The project's fields go right after `env`, so the changed env line
    // has them as patch context.
    let mut settings = serde_json::Map::new();
    for (key, value) in read_json(&root, CLAUDE_SETTINGS).as_object().unwrap() {
        settings.insert(key.clone(), value.clone());
        if key == "env" {
            settings.insert("api-key".into(), serde_json::json!(SECRET));
            settings.insert("api key".into(), serde_json::json!(NUMBER));
            settings.insert(
                "projectTools".into(),
                serde_json::json!({
                    "httpHeaders": {"Authorization": SECRET},
                    "http-headers": {"X-Trace": NUMBER},
                    "api.key": [SECRET],
                }),
            );
        }
    }
    write_json(&root, CLAUDE_SETTINGS, &settings.into());

    let outputs = update_with_diff(tmp.path(), &root, &[]);
    assert_eq!(
        read_json(&root, CLAUDE_SETTINGS)["api-key"],
        SECRET,
        "the update dropped a project field"
    );
    assert!(
        outputs[2].contains("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"),
        "the diff does not name the migrated key:\n{}",
        outputs[2]
    );
    for (name, text) in ["stdout", "stderr", "diff"].iter().zip(&outputs) {
        assert!(
            !text.contains(SECRET),
            "{name} shows a project value:\n{text}"
        );
        assert!(
            !text.contains(&NUMBER.to_string()),
            "{name} shows a project number:\n{text}"
        );
    }
}

/// TSK-1029 review 02: `update --force --diff` over a managed workflow the
/// project edited shows the shipped lines and hides the project's
/// values; a workflow that no longer parses is withheld whole.
#[test]
fn update_force_diff_hides_project_values_in_a_managed_workflow() {
    const SECRET: &str = "opaque-neighbour-value-31337";
    const WORKFLOW: &str = ".github/workflows/codeflow-ci.yml";
    for malformed in [false, true] {
        let (tmp, root) = project();
        init(&root, "--minimal");
        let shipped = read(&root, WORKFLOW);
        let edited = if malformed {
            format!("{shipped}\nenv: [\n  RELEASE_NOTE: {SECRET}\n")
        } else {
            format!("{shipped}\nenv:\n  RELEASE_NOTE: {SECRET}\n")
        };
        std::fs::write(root.join(WORKFLOW), edited).expect("edit workflow");

        let outputs = update_with_diff(tmp.path(), &root, &["--force"]);
        assert_eq!(read(&root, WORKFLOW), shipped, "--force did not restore");
        let diff = &outputs[2];
        assert!(diff.contains(&format!("=== {WORKFLOW}")), "{diff}");
        if malformed {
            assert!(diff.contains("content withheld"), "{diff}");
        } else {
            assert!(
                diff.contains("keys the shipped file does not have (names not shown): 1 removed"),
                "{diff}"
            );
            assert!(
                !diff.contains("RELEASE_NOTE"),
                "a project key is named:\n{diff}"
            );
        }
        for (name, text) in ["stdout", "stderr", "diff"].iter().zip(&outputs) {
            assert!(
                !text.contains(SECRET),
                "{name} shows a project value:\n{text}"
            );
        }
    }
}

/// Run `codeflow update` with `extra` flags and `--diff`, whether or not it
/// succeeds, and return its stdout, its stderr and the diff file (empty when
/// none was written).
fn update_outputs(tmp: &Path, root: &Path, extra: &[&str]) -> [String; 3] {
    let diff_file = tmp.join("update.diff");
    let _ = std::fs::remove_file(&diff_file);
    let mut args = vec!["update"];
    args.extend_from_slice(extra);
    args.extend(["--diff", diff_file.to_str().expect("utf-8 path")]);
    let out = codeflow(root, &args);
    [
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        std::fs::read_to_string(&diff_file).unwrap_or_default(),
    ]
}

fn assert_nowhere(outputs: &[String; 3], secret: &str, case: &str) {
    for (name, text) in ["stdout", "stderr", "diff"].iter().zip(outputs) {
        assert!(
            !text.contains(secret),
            "{case}: {name} shows a project value:\n{text}"
        );
    }
}

/// TSK-1029 review 03: a project value that replaces a shipped scalar, of
/// the same type or another, never reaches the export, even as context
/// beside the migrated compaction value.
#[test]
fn update_diff_never_shows_a_replaced_shipped_scalar() {
    const SECRET: &str = "opaque-neighbour-value-31337";
    let (tmp, root) = project();
    init(&root, "--minimal");
    roll_back_compaction(&root, "50", &serde_json::json!({}));
    let mut settings = read_json(&root, CLAUDE_SETTINGS);
    settings["$schema"] =
        serde_json::json!(format!("https://user:{SECRET}@example.invalid/s.json"));
    settings["includeCoAuthoredBy"] = serde_json::json!(SECRET);
    write_json(&root, CLAUDE_SETTINGS, &settings);

    let outputs = update_with_diff(tmp.path(), &root, &[]);
    assert!(
        outputs[2].contains("env.CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"),
        "{}",
        outputs[2]
    );
    assert_nowhere(&outputs, SECRET, "replaced scalar");
}

/// TSK-1029 review 03: the recorded baseline is the project's to write, so
/// a tampered, stale, missing or malformed baseline never lets a project
/// line, list item or scalar into the export.
#[test]
fn update_diff_never_trusts_the_baseline() {
    const SECRET: &str = "opaque-neighbour-value-31337";
    const RULES: &str = ".codeflow/rules/writing.md";
    let line = format!("export DEPLOY_KEY={SECRET}");
    let item = format!("Bash(curl -H 'Authorization: {SECRET}')");
    let url = format!("https://user:{SECRET}@example.invalid/s.json");
    for baseline in ["tampered", "stale", "missing", "malformed"] {
        let (tmp, root) = project();
        init(&root, "--minimal");
        let shipped_rules = read(&root, RULES);
        let rules_baseline = root.join(".codeflow/.baseline").join(RULES);
        let settings_baseline = root.join(CLAUDE_SETTINGS_BASELINE);

        // The project's copies hold the planted line, item and scalar.
        std::fs::write(root.join(RULES), format!("{shipped_rules}{line}\n")).unwrap();
        roll_back_compaction(&root, "50", &serde_json::json!({}));
        let mut settings = read_json(&root, CLAUDE_SETTINGS);
        settings["$schema"] = serde_json::json!(url);
        settings["permissions"]["allow"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!(item));
        write_json(&root, CLAUDE_SETTINGS, &settings);

        match baseline {
            "tampered" => {
                std::fs::write(&rules_baseline, format!("{shipped_rules}{line}\n")).unwrap();
                write_json(&root, CLAUDE_SETTINGS_BASELINE, &settings);
            }
            "stale" => {
                let older = shipped_rules.replacen('\n', "\nan older shipped line\n", 1);
                std::fs::write(&rules_baseline, older).unwrap();
            }
            "missing" => {
                std::fs::remove_file(&rules_baseline).unwrap();
                std::fs::remove_file(&settings_baseline).unwrap();
            }
            _ => {
                std::fs::write(&rules_baseline, format!("{line}\n<<<<<<<\n")).unwrap();
                std::fs::write(&settings_baseline, format!("{{\"x\": \"{SECRET}\"")).unwrap();
            }
        }

        let outputs = update_outputs(tmp.path(), &root, &["--force"]);
        assert_nowhere(&outputs, SECRET, baseline);
    }
}

/// TSK-1029 review 03: update's own report lines name places, never what
/// the project wrote: a step before the secret scan is named by its
/// position, a policy value by its key, and a malformed project TOML line
/// by its line and column, on success and on failure alike.
#[test]
fn update_reports_never_show_project_values() {
    const SECRET: &str = "opaque-neighbour-value-31337";
    const WORKFLOW: &str = ".github/workflows/codeflow-ci.yml";
    const CHECKOUT: &str = "          fetch-depth: 0\n";
    const POLICY: &str = ".codeflow/policy.json";
    const POLICY_BASELINE: &str = ".codeflow/.baseline/.codeflow/policy.json";
    let (tmp, root) = project();
    init(&root, "--minimal");

    // Steps before the gitleaks step, one named and one unnamed.
    let workflow = read(&root, WORKFLOW);
    let at = workflow.find("  secret-scan:").expect("secret-scan job");
    let checkout = at + workflow[at..].find(CHECKOUT).expect("checkout step") + CHECKOUT.len();
    let mut planted = workflow.clone();
    planted.insert_str(
        checkout,
        &format!("      - name: {SECRET}\n        run: echo one\n      - run: echo {SECRET}\n"),
    );
    std::fs::write(root.join(WORKFLOW), planted).unwrap();

    // A policy value that held the recorded default, and one the project
    // chose where the recorded default differs from the shipped one.
    for rel in [POLICY, POLICY_BASELINE] {
        let mut policy = read_json(&root, rel);
        policy["git"]["force_push_unprotected"] = serde_json::json!(SECRET);
        policy["git"]["test_gate_on_push"] = if rel == POLICY {
            serde_json::json!(SECRET)
        } else {
            serde_json::json!("warn")
        };
        write_json(&root, rel, &policy);
    }

    let outputs = update_outputs(tmp.path(), &root, &[]);
    assert!(
        outputs[0].contains("before its gitleaks step"),
        "the step warning is missing:\n{}",
        outputs[0]
    );
    assert_nowhere(&outputs, SECRET, "report");

    // A malformed project TOML line fails the update without showing it.
    let toml = root.join(".codeflow/project.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(&toml, format!("{text}\nbroken = \"{SECRET}\n")).unwrap();
    let outputs = update_outputs(tmp.path(), &root, &[]);
    assert!(
        outputs[1].contains("project.toml") && outputs[1].contains("line "),
        "the parse error lost its place:\n{}",
        outputs[1]
    );
    assert_nowhere(&outputs, SECRET, "malformed project.toml");
}

/// TSK-1029 review 03, second pass: every refusal update reaches names the
/// file and place of a bad value and never the value, even when the value
/// holds the delimiters a parser puts around it. Each case checks that its
/// own diagnostic fired and that update still refuses or warns as before.
#[test]
fn update_refusals_never_show_adversarial_values() {
    const SECRET: &str = "opaque-neighbour-value-31337";
    const PROJECT: &str = ".codeflow/project.toml";
    const POLICY: &str = ".codeflow/policy.json";
    const PENDING: &str = ".codeflow/.pending-update/.codeflow/rules/writing.md.json";

    /// Plants one bad value in a fresh install.
    type Plant = fn(&Path);
    // Each case: its name, how it plants the value, whether update must
    // refuse (`None`: a warning only, the outcome is not judged), and the
    // texts its own diagnostic prints in stderr.
    let cases: [(&str, Plant, Option<bool>, &[&str]); 4] = [
        (
            "unparseable scaffold_version",
            |root| {
                let text = read(root, PROJECT);
                let pinned = text
                    .lines()
                    .find(|line| line.starts_with("scaffold_version"))
                    .expect("scaffold_version line")
                    .to_string();
                let bad = format!("scaffold_version = \"1.0` {SECRET}\"");
                std::fs::write(root.join(PROJECT), text.replace(&pinned, &bad)).unwrap();
            },
            Some(false),
            &["scaffold_version", PROJECT, "not a parseable version"],
        ),
        (
            "duplicate project key",
            |root| {
                let text = read(root, PROJECT);
                let planted = format!("{text}\n\"{SECRET}\" = 1\n\"{SECRET}\" = 2\n");
                std::fs::write(root.join(PROJECT), planted).unwrap();
            },
            Some(false),
            &["project.toml", "TOML parse error at line "],
        ),
        (
            "pending resolution holding an escaped quote",
            |root| {
                let folder = Path::new(PENDING).parent().expect("pending folder");
                std::fs::create_dir_all(root.join(folder)).unwrap();
                std::fs::write(root.join(PENDING), format!("\"x\\\" {SECRET}\"")).unwrap();
            },
            Some(false),
            &[PENDING, "line 1, column "],
        ),
        (
            "policy value holding a backtick",
            |root| {
                let mut policy = read_json(root, POLICY);
                policy["git"]["test_gate_on_push"] = serde_json::json!(format!("x` {SECRET}"));
                write_json(root, POLICY, &policy);
            },
            None,
            &["cannot parse policy", ", column "],
        ),
    ];

    let mut problems = Vec::new();
    for (case, plant, succeeds, expected) in cases {
        let (_tmp, root) = project();
        init(&root, "--minimal");
        plant(&root);
        let out = codeflow(&root, &["update"]);
        let (stdout, stderr) = (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        );
        for (name, text) in [("stdout", &stdout), ("stderr", &stderr)] {
            if text.contains(SECRET) {
                problems.push(format!("{case}: {name} shows the value:\n{text}"));
            }
        }
        if succeeds.is_some_and(|succeeds| succeeds != out.status.success()) {
            problems.push(format!("{case}: the outcome changed:\n{stderr}"));
        }
        for text in expected {
            if !stderr.contains(text) {
                problems.push(format!("{case}: stderr lacks {text:?}:\n{stderr}"));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n\n"));
}

/// TSK-1029 review: when the shipped default changes and the project keeps
/// its own value, the update says so by key, even when nothing else in the
/// settings changes; the next update, with nothing new shipped, is quiet.
#[test]
fn update_reports_a_kept_project_value_when_the_default_moves() {
    const LINE: &str =
        "settings: kept the project's \"env.CLAUDE_AUTOCOMPACT_PCT_OVERRIDE\"; the shipped default changed";
    let (_tmp, root) = project();
    init(&root, "--minimal");
    roll_back_compaction(&root, "50", &serde_json::json!({}));
    let mut chosen = read_json(&root, CLAUDE_SETTINGS);
    chosen["env"]["CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"] = serde_json::json!("40");
    write_json(&root, CLAUDE_SETTINGS, &chosen);
    let before = read(&root, CLAUDE_SETTINGS);

    let first = update(&root);
    assert_eq!(read(&root, CLAUDE_SETTINGS), before, "{first}");
    assert!(first.contains(LINE), "no kept-value notice:\n{first}");
    assert!(
        !first.contains("\"40\""),
        "the report printed a value:\n{first}"
    );

    let second = update(&root);
    assert_eq!(read(&root, CLAUDE_SETTINGS), before, "{second}");
    assert!(
        !second.contains("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"),
        "the next update repeated the notice with nothing new shipped:\n{second}"
    );
}

/// TSK-211 AC-2: a project installed before the default, whose recorded
/// baseline has no `env`, gains both keys on update; one with its own `env`
/// keeps every entry and gains the two keys; a second update is a no-op.
#[test]
fn update_brings_the_compaction_default_to_an_earlier_install() {
    for own_env in [None, Some(serde_json::json!({"PROJECT_FLAG": "1"}))] {
        let (_tmp, root) = project();
        init(&root, "--minimal");
        for rel in [CLAUDE_SETTINGS, CLAUDE_SETTINGS_BASELINE] {
            let mut earlier = read_json(&root, rel);
            let earlier_map = earlier.as_object_mut().unwrap();
            earlier_map.remove("env");
            if let (CLAUDE_SETTINGS, Some(env)) = (rel, &own_env) {
                earlier_map.insert("env".to_string(), env.clone());
            }
            write_json(&root, rel, &earlier);
        }

        update(&root);
        let mut expected = own_env
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .cloned()
            .unwrap_or_default();
        expected.extend(shipped_compaction_env().as_object().unwrap().clone());
        assert_eq!(
            read_json(&root, CLAUDE_SETTINGS)["env"],
            serde_json::Value::Object(expected),
            "own env {own_env:?}"
        );

        let after_first = read(&root, CLAUDE_SETTINGS);
        update(&root);
        assert_eq!(
            read(&root, CLAUDE_SETTINGS),
            after_first,
            "the second update was not a no-op"
        );
    }
}

/// TSK-211 AC-2: a malformed `env` is reported on every update, even when
/// nothing else in the settings changes, and is left exactly as it was. The
/// report never prints an `env` value.
#[test]
fn update_reports_a_malformed_env_on_every_run_without_its_values() {
    for (env, diagnostic) in [
        (
            serde_json::json!(["secret-array-value"]),
            "\"env\" is not an object",
        ),
        (
            serde_json::json!({"PROJECT_TOKEN": "secret-map-value", "PORT": 8080}),
            "\"env\" has a value that is not a string",
        ),
    ] {
        let (_tmp, root) = project();
        init(&root, "--minimal");
        let mut value = read_json(&root, CLAUDE_SETTINGS);
        value["env"] = env.clone();
        write_json(&root, CLAUDE_SETTINGS, &value);
        let before = read(&root, CLAUDE_SETTINGS);

        for attempt in 1..=2 {
            let report = update(&root);
            assert!(
                report.contains(diagnostic),
                "update {attempt} did not report the malformed env {env}:\n{report}"
            );
            assert!(
                !report.contains("secret-array-value")
                    && !report.contains("secret-map-value")
                    && !report.contains("8080"),
                "update {attempt} printed an env value:\n{report}"
            );
            assert_eq!(
                read(&root, CLAUDE_SETTINGS),
                before,
                "update {attempt} touched a malformed env"
            );
        }
    }
}

/// TSK-211 AC-2: with no recorded baseline, a project's own `env` gains
/// neither shipped key, and every update names the keys it left unset,
/// without printing the project's values.
#[test]
fn update_names_the_keys_it_did_not_add_without_a_baseline() {
    let (_tmp, root) = project();
    init(&root, "--minimal");
    let mut value = read_json(&root, CLAUDE_SETTINGS);
    value["env"] = serde_json::json!({"PROJECT_TOKEN": "secret-project-value"});
    write_json(&root, CLAUDE_SETTINGS, &value);
    std::fs::remove_file(root.join(CLAUDE_SETTINGS_BASELINE)).expect("drop the baseline");

    let mut after_first = String::new();
    for attempt in 1..=2 {
        let report = update(&root);
        for key in [
            "env.CLAUDE_CODE_AUTO_COMPACT_WINDOW",
            "env.CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
        ] {
            assert!(
                report.contains(key),
                "update {attempt} did not name {key}:\n{report}"
            );
        }
        assert!(
            !report.contains("secret-project-value"),
            "update {attempt} printed an env value:\n{report}"
        );
        assert_eq!(
            read_json(&root, CLAUDE_SETTINGS)["env"],
            serde_json::json!({"PROJECT_TOKEN": "secret-project-value"}),
            "update {attempt} changed the project's env"
        );
        if attempt == 1 {
            after_first = read(&root, CLAUDE_SETTINGS);
        } else {
            assert_eq!(
                read(&root, CLAUDE_SETTINGS),
                after_first,
                "the second update was not a no-op"
            );
        }
    }
}
