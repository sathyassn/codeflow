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

/// Shared isolated `CODEFLOW_HOME` so the suite never writes the developer's
/// real `~/.codeflow/registry.json` (the `init_e2e.rs` pattern).
fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

/// Runs the binary under test with the git-hook shims pointed back at it (they
/// `exec codeflow` from PATH) and host git config neutralized.
fn codeflow(dir: &Path, args: &[&str]) -> Output {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    Command::new(&exe)
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
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .output()
        .expect("codeflow binary runs")
}

/// Runs `git` in `dir` with the binary on PATH (so wired hooks fire) and an
/// author identity, host config neutralized.
fn git(dir: &Path, args: &[&str]) -> Output {
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

        // A failing `quick` target blocks too.
        let config = r#"{"schema_version": "1.0", "targets": [
  {"name": "lint", "runner": "custom", "modes": {"quick": {"command": "false"}, "full": {"command": "true"}}}
]}"#;
        let msg = ["-m", "chore: add a failing lint target"];
        let cfg = ".codeflow/test-config.json";
        commit_on_branch(&root, "feat/base", "feat/quick-fails", cfg, config, &msg);
        let (ok, err) = push(&root, "feat/quick-fails");
        assert!(!ok, "{tier}: failing quick target pushed");
        assert!(
            err.contains("push set failed for: lint"),
            "{tier}: block did not name the failing target:\n{err}"
        );
    }
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
fn headless_peer_runs_warn_in_every_harness_wiring_at_every_tier() {
    // TSK-136 AC-4: a fresh install at every tier ships the guard at `warn`,
    // and the Claude, Codex and Grok wiring each run it; `block` refuses.
    for tier in ["--minimal", "--standard", "--full"] {
        let (_tmp, root) = project();
        init(&root, tier);
        let policy: serde_json::Value =
            serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
        assert_eq!(
            policy["security"]["headless_peer_runs"], "warn",
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
            assert_eq!(out.status.code(), Some(0), "{tier} {wiring}: {stderr}");
            assert!(
                stderr.contains("headless peer run"),
                "{tier} {wiring}: {stderr}"
            );
            let out = run_wired(&root, &hook, "codex --version");
            assert!(out.stderr.is_empty(), "{tier} {wiring}: negative warned");
        }

        let mut policy = policy;
        policy["security"]["headless_peer_runs"] = "block".into();
        std::fs::write(
            root.join(".codeflow/policy.json"),
            serde_json::to_string_pretty(&policy).unwrap(),
        )
        .unwrap();
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
            assert_eq!(out.status.code(), Some(2), "{tier} {wiring}: block level");
        }
    }
}
