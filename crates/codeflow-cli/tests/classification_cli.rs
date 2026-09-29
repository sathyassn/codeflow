//! Pull request classification in `codeflow ci` (TSK-104, SPC-013 R-64,
//! R-70 to R-72, R-78): CI fixtures per class, per unnamed surface, per
//! work prefix, the mismatched `Task:` line, the self-authorising record and
//! the spike path. Each test runs the Cargo-built binary in a tempdir
//! repository with durable work tracking on.

use std::path::Path;
use std::process::Command;

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn codeflow() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    cmd.env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("CI_PIPELINE_SOURCE")
        .env_remove("CI_MERGE_REQUEST_IID")
        .env_remove("BITBUCKET_PR_ID")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn task(id: &str, work_type: &str, status: &str) -> String {
    format!(
        "---\nid: {id}\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: main\ntitle: \"work {id}\"\nstatus: {status}\nwork_type: {work_type}\nspecs: []\ndepends_on: []\ncreated: 2026-09-26\n---\n\n# {id}: work\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n"
    )
}

const EPIC: &str = "---\nid: EPC-001\ntitle: \"outcome\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-09-26\n---\n\n# EPC-001: outcome\n\n## Summary\n\nAn outcome.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall deliver.\n";

/// `main` holds an epic, a feature task TSK-001 and a spike TSK-002, and a
/// policy naming `src/**` as product code and `api/**` as watched.
fn tracked_repo(policy_git: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    for sub in [
        "project-management/epics",
        "project-management/tasks",
        ".codeflow",
        "src",
    ] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    std::fs::write(root.join("project-management/epics/EPC-001.md"), EPIC).unwrap();
    std::fs::write(
        root.join("project-management/tasks/TSK-001.md"),
        task("TSK-001", "feat", "todo"),
    )
    .unwrap();
    std::fs::write(
        root.join("project-management/tasks/TSK-002.md"),
        task("TSK-002", "spike", "todo"),
    )
    .unwrap();
    std::fs::write(
        root.join(".codeflow/policy.json"),
        format!("{{\n  \"schema_version\": 1,\n  \"git\": {{{policy_git}}}\n}}\n"),
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn base() {}\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: plan the work"]);
    dir
}

const DEFAULT_POLICY: &str =
    "\"product_paths\": [\"src/**\"], \"breaking_watch_paths\": [\"api/**\"]";

/// Commit `files` (path, content) on a new `branch` from `main`.
fn branch_with(root: &Path, branch: &str, files: &[(&str, &str)]) {
    git(root, &["switch", "-C", branch, "main"]);
    for (path, content) in files {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    git(root, &["add", "-A"]);
    git(root, &["commit", "-m", "chore: change for the fixture"]);
}

fn body(task_line: &str) -> String {
    format!(
        "## Summary\nA bounded change.\n\n{task_line}\n\n## Changes\n- one change\n\n## Testing\n- focused test\n"
    )
}

fn ci(root: &Path, branch: &str, pr_body: &str) -> (i32, String) {
    ci_into(root, "main", branch, pr_body)
}

fn ci_into(root: &Path, base: &str, branch: &str, pr_body: &str) -> (i32, String) {
    let out = codeflow()
        .args([
            "ci",
            "--base",
            base,
            "--head",
            "HEAD",
            "--branch",
            branch,
            "--pr-body",
            pr_body,
        ])
        .current_dir(root)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn assert_passes(result: &(i32, String), what: &str) {
    assert_eq!(result.0, 0, "{what}: {}", result.1);
}

fn assert_blocks(result: &(i32, String), what: &str, needle: &str) {
    assert_eq!(result.0, 1, "{what}: {}", result.1);
    assert!(
        result.1.contains(needle),
        "{what}: missing {needle:?} in {}",
        result.1
    );
}

/// AC-1: one fixture per class, and the unclassified and planning-only
/// negative controls.
#[test]
fn each_class_passes_and_an_unclassified_pull_request_blocks() {
    let dir = tracked_repo(DEFAULT_POLICY);
    let root = dir.path();

    branch_with(
        root,
        "feat/tracked",
        &[("src/lib.rs", "pub fn tracked() {}\n")],
    );
    let tracked = ci(root, "feat/tracked", &body("Task: TSK-001"));
    assert_passes(&tracked, "tracked");
    assert!(tracked
        .1
        .contains("class: tracked TSK-001 (from the Task: line)"));

    branch_with(root, "docs/typo", &[("docs/guide.md", "# Guide\n")]);
    let direct = ci(
        root,
        "docs/typo",
        &body("Task: none: fix a typo in the guide"),
    );
    assert_blocks(&direct, "unnamed docs", "is neither");

    branch_with(root, "plan/next", &[("docs/plan/next.md", "# Next\n")]);
    let planning = ci(root, "plan/next", &body("Task: EPC-001"));
    assert_passes(&planning, "planning-only");
    assert!(planning.1.contains("class: planning-only"));

    branch_with(
        root,
        "feat/unnamed",
        &[("src/lib.rs", "pub fn unnamed() {}\n")],
    );
    assert_blocks(
        &ci(root, "feat/unnamed", &body("")),
        "unclassified",
        "unclassified pull request",
    );

    branch_with(
        root,
        "plan/sneaky",
        &[("src/lib.rs", "pub fn sneaky() {}\n")],
    );
    assert_blocks(
        &ci(root, "plan/sneaky", &body("Task: EPC-001")),
        "planning PR with product code",
        "planning-only pull request touches a product path: src/lib.rs",
    );

    branch_with(root, "fix/bad", &[("docs/guide.md", "# Guide\n")]);
    assert_blocks(
        &ci(root, "fix/bad", &body("Task: TSK-NNN | none: <reason>")),
        "template placeholder",
        "is neither",
    );
}

/// Naming a task is required on every surface, regardless of legacy policy.
#[test]
fn an_unnamed_change_blocks_on_every_surface() {
    let dir = tracked_repo(DEFAULT_POLICY);
    let root = dir.path();
    let direct = body("Task: none: small change");
    for (path, _member) in [
        ("src/feature.rs", "product_paths"),
        ("api/schema.json", "watched_contract_paths"),
        (".codeflow/policy.json", "policy"),
        (".codeflow/git-hooks/pre-commit", "hook_sources"),
        ("Cargo.lock", "dependency_manifests"),
        ("web/package.json", "dependency_manifests"),
        ("project-management/templates/task.md", "record_schema"),
        ("AGENTS.md", "managed_instructions"),
        (".claude/skills/x/SKILL.md", "managed_instructions"),
        (".github/workflows/ci.yml", "ci_workflows"),
        ("docs/decisions/template.md", "shipped_templates"),
    ] {
        let content = if Path::new(path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            "{\"schema_version\": 1}\n"
        } else {
            "x\n"
        };
        branch_with(root, "fix/direct", &[(path, content)]);
        assert_blocks(&ci(root, "fix/direct", &direct), path, "is neither");
    }

    let forbid = tracked_repo("\"direct_changes\": \"forbid\"");
    branch_with(
        forbid.path(),
        "docs/typo",
        &[("docs/guide.md", "# Guide\n")],
    );
    assert_blocks(
        &ci(forbid.path(), "docs/typo", &direct),
        "forbidden",
        "is neither",
    );

    // An empty product list does not exempt unnamed work.
    let narrowed = tracked_repo("\"product_paths\": []");
    branch_with(
        narrowed.path(),
        "ci/tweak",
        &[(".github/workflows/ci.yml", "x\n")],
    );
    assert_blocks(
        &ci(narrowed.path(), "ci/tweak", &direct),
        "narrowed",
        "is neither",
    );
}

/// AC-2: the anchored preflight runs for the named task whatever the branch
/// (a `fix/` pull request naming the release task), on every work prefix
/// carrying the id, and blocks a mismatched `Task:` line.
#[test]
fn the_preflight_runs_on_any_branch_and_a_mismatch_blocks() {
    let dir = tracked_repo(DEFAULT_POLICY);
    let root = dir.path();
    for branch in [
        "task/TSK-001-work",
        "fix/TSK-001-repair",
        "feat/TSK-001-thing",
    ] {
        branch_with(root, branch, &[("src/lib.rs", "pub fn work() {}\n")]);
        let result = ci(root, branch, &body("Task: TSK-001"));
        assert_passes(&result, branch);
        assert!(
            result
                .1
                .contains("class: tracked TSK-001 (from the Task: line)"),
            "{}",
            result.1
        );
    }

    branch_with(
        root,
        "fix/release-notes",
        &[("src/lib.rs", "pub fn notes() {}\n")],
    );
    assert_passes(
        &ci(root, "fix/release-notes", &body("Task: TSK-001")),
        "fix/ naming the task",
    );

    branch_with(
        root,
        "fix/TSK-001-repair",
        &[("src/lib.rs", "pub fn r() {}\n")],
    );
    assert_blocks(
        &ci(root, "fix/TSK-001-repair", &body("Task: TSK-002")),
        "mismatch",
        "names another task than branch 'fix/TSK-001-repair' (TSK-001)",
    );

    // A task that is not startable fails the preflight through the Task line.
    let done = tracked_repo(DEFAULT_POLICY);
    std::fs::write(
        done.path().join("project-management/tasks/TSK-001.md"),
        task("TSK-001", "feat", "cancelled"),
    )
    .unwrap();
    git(done.path(), &["commit", "-am", "chore: cancel the task"]);
    branch_with(
        done.path(),
        "fix/late",
        &[("src/lib.rs", "pub fn late() {}\n")],
    );
    assert_blocks(
        &ci(done.path(), "fix/late", &body("Task: TSK-001")),
        "cancelled task without closeout",
        "work.valid_graph",
    );

    // A branch claiming an id with no visible record blocks too.
    branch_with(
        root,
        "fix/TSK-404-ghost",
        &[("src/lib.rs", "pub fn g() {}\n")],
    );
    assert_blocks(
        &ci(root, "fix/TSK-404-ghost", &body("Task: TSK-404")),
        "ghost id",
        "not present at the merge-base",
    );
}

/// AC-7: a pull request that adds a task record cannot claim that task.
#[test]
fn a_record_cannot_authorise_itself() {
    let dir = tracked_repo(DEFAULT_POLICY);
    let root = dir.path();
    branch_with(
        root,
        "feat/self",
        &[
            (
                "project-management/tasks/TSK-003.md",
                &task("TSK-003", "feat", "todo"),
            ),
            ("src/lib.rs", "pub fn self_made() {}\n"),
        ],
    );
    assert_blocks(
        &ci(root, "feat/self", &body("Task: TSK-003")),
        "self-authorising",
        "not present at the merge-base",
    );
}

/// AC-5: a spike lands findings and its own record; product code blocks.
#[test]
fn a_spike_lands_only_findings_and_its_record() {
    let dir = tracked_repo(DEFAULT_POLICY);
    let root = dir.path();
    let record = task("TSK-002", "spike", "todo").replace("Work.\n", "Work, with findings.\n");
    branch_with(
        root,
        "spike/TSK-002-probe",
        &[
            (
                "docs/research/cache-choice.md",
                "# Cache choice\n\nSources: none.\n",
            ),
            ("project-management/tasks/TSK-002.md", &record),
        ],
    );
    assert_passes(
        &ci(root, "spike/TSK-002-probe", &body("Task: TSK-002")),
        "spike findings",
    );

    branch_with(
        root,
        "spike/TSK-002-probe",
        &[
            ("docs/research/cache-choice.md", "# Cache choice\n"),
            ("src/cache.rs", "pub fn cache() {}\n"),
        ],
    );
    assert_blocks(
        &ci(root, "spike/TSK-002-probe", &body("Task: TSK-002")),
        "spike with code",
        "spike TSK-002 changes src/cache.rs",
    );

    // `Task: none` cannot drop the spike the branch carries.
    branch_with(root, "spike/TSK-002-probe", &[("README.md", "# Edited\n")]);
    assert_blocks(
        &ci(root, "spike/TSK-002-probe", &body("Task: none: small edit")),
        "spike with a direct line",
        "is neither",
    );
}

/// Merge `from` into the checked-out branch with a merge commit.
fn merge(root: &Path, from: &str) {
    git(
        root,
        &[
            "merge",
            "--no-ff",
            "-q",
            "-m",
            &format!("Merge {from}"),
            from,
        ],
    );
}

/// An epic line holding one planned and landed task: the whole line lands
/// as one pull request, and only a verified line does.
#[test]
fn only_a_verified_epic_line_lands_as_one_pull_request() {
    let dir = tracked_repo(DEFAULT_POLICY);
    let root = dir.path();
    let line = "integration/EPC-001-outcome";
    git(root, &["branch", line, "main"]);
    let record = task("TSK-003", "feat", "todo").replace(
        "integration_target: main",
        &format!("integration_target: {line}"),
    );
    branch_with(
        root,
        "plan/line",
        &[("project-management/tasks/TSK-003.md", &record)],
    );
    // Bind the hand-written record in the shared id registry (TSK-101), as
    // a maintainer's seed does, so the merge rule judges the line itself.
    let seeded = codeflow()
        .args(["ids", "seed"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        seeded.status.success(),
        "ids seed: {}{}",
        String::from_utf8_lossy(&seeded.stdout),
        String::from_utf8_lossy(&seeded.stderr)
    );
    git(root, &["switch", "-q", line]);
    merge(root, "plan/line");
    git(root, &["switch", "-q", "-c", "task/TSK-003-work"]);
    std::fs::write(root.join("src/lib.rs"), "pub fn line() {}\n").unwrap();
    git(root, &["commit", "-qam", "feat: build on the line"]);
    git(root, &["switch", "-q", line]);
    merge(root, "task/TSK-003-work");

    let landed = ci(root, line, &body("Task: EPC-001"));
    assert_passes(&landed, "verified epic line");
    assert!(
        landed.1.contains("class: epic integration line of EPC-001"),
        "{}",
        landed.1
    );
    assert!(
        landed.1.contains("bound by provenance: TSK-003"),
        "{}",
        landed.1
    );

    // The same commits under a name that is not the epic's line.
    for (name, needle) in [
        ("integration/not-an-epic", "names no epic"),
        (
            "integration/EPC-009-other",
            "EPC-009, which has no epic record",
        ),
        (
            "integration/EPC-001-elsewhere",
            "no task of EPC-001 targets",
        ),
    ] {
        git(root, &["branch", "-f", name, line]);
        git(root, &["switch", "-q", name]);
        assert_blocks(&ci(root, name, &body("Task: EPC-001")), name, needle);
    }

    // An epic cancelled on the target after the line forked is closed.
    git(root, &["switch", "-q", "-c", "plan/cancel", "main"]);
    std::fs::write(
        root.join("project-management/epics/EPC-001.md"),
        EPIC.replace("status: planning", "status: cancelled"),
    )
    .unwrap();
    git(root, &["commit", "-qam", "chore: cancel the epic"]);
    git(root, &["switch", "-q", "main"]);
    merge(root, "plan/cancel");
    git(root, &["switch", "-q", line]);
    assert_blocks(
        &ci(root, line, &body("Task: EPC-001")),
        "cancelled after fork",
        "EPC-001 is cancelled",
    );
    git(root, &["switch", "-q", "main"]);
    git(root, &["reset", "-q", "--hard", "HEAD~1"]);
    git(root, &["switch", "-q", line]);

    // The line lands on the project's default target only.
    git(root, &["branch", "release/next", "main"]);
    git(root, &["switch", "-q", line]);
    assert_blocks(
        &ci_into(root, "release/next", line, &body("Task: EPC-001")),
        "wrong target",
        "lands on 'main', not 'release/next'",
    );

    // Work committed directly on the line is untracked.
    std::fs::write(root.join("src/lib.rs"), "pub fn untracked() {}\n").unwrap();
    git(root, &["commit", "-qam", "feat: slip one in"]);
    assert_blocks(
        &ci(root, line, &body("Task: EPC-001")),
        "untracked addition",
        "a product change made directly on the line",
    );
}

/// The range is one tree diff from the merge-base: a product change made
/// while resolving a merge counts, and paths are read without Git quoting.
#[test]
fn the_whole_range_is_classified_whatever_its_shape() {
    let dir = tracked_repo(DEFAULT_POLICY);
    let root = dir.path();
    branch_with(root, "side", &[("docs/plan/a.md", "# A\n")]);
    git(root, &["switch", "-q", "-C", "plan/merged", "main"]);
    git(root, &["merge", "--no-ff", "--no-commit", "side"]);
    std::fs::write(root.join("src/lib.rs"), "pub fn in_the_merge() {}\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "Merge side"]);
    assert_blocks(
        &ci(root, "plan/merged", &body("Task: EPC-001")),
        "merge resolution",
        "planning-only pull request touches a product path: src/lib.rs",
    );
    assert_blocks(
        &ci(root, "plan/merged", &body("Task: none: merge")),
        "merge resolution, direct",
        "is neither",
    );

    // A product change in an earlier commit of the range still counts.
    branch_with(root, "fix/two", &[("src/lib.rs", "pub fn first() {}\n")]);
    std::fs::write(root.join("README.md"), "# Later\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "docs: a later commit"]);
    assert_blocks(
        &ci(root, "fix/two", &body("Task: none: two commits")),
        "earlier commit",
        "is neither",
    );

    for (path, _member) in [
        ("src/\u{3c0}.rs", "product_paths"),
        (".claude/a\tb.md", "managed_instructions"),
    ] {
        branch_with(root, "fix/quoted", &[(path, "x\n")]);
        assert_blocks(
            &ci(root, "fix/quoted", &body("Task: none: quoted")),
            path,
            "is neither",
        );
    }
}

/// Tracking is read at the target: a pull request that deletes the records
/// or the full-tier state is still classified.
#[test]
fn a_pull_request_cannot_switch_tracking_off_for_itself() {
    let dir = tracked_repo(DEFAULT_POLICY);
    let root = dir.path();
    git(root, &["switch", "-q", "-c", "chore/untrack"]);
    git(root, &["rm", "-rq", "project-management"]);
    std::fs::write(root.join("src/lib.rs"), "pub fn after() {}\n").unwrap();
    git(root, &["commit", "-qam", "chore: drop the records"]);
    assert_blocks(
        &ci(root, "chore/untrack", &body("")),
        "records removed",
        "unclassified pull request",
    );

    let full = tracked_repo(DEFAULT_POLICY);
    let root = full.path();
    git(root, &["rm", "-rq", "project-management"]);
    std::fs::write(
        root.join(".codeflow/project.toml"),
        "schema_version = 1\ntier = \"full\"\nscaffold_version = \"3.0.0\"\nstack = \"rust\"\nareas = []\npolicy_armed = true\ngit_hooks = \"wired\"\npermission_preset = \"default\"\n",
    )
    .unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "chore: full tier without records"]);
    git(root, &["switch", "-q", "-c", "chore/downgrade"]);
    std::fs::write(
        root.join(".codeflow/project.toml"),
        std::fs::read_to_string(root.join(".codeflow/project.toml"))
            .unwrap()
            .replace("tier = \"full\"", "tier = \"standard\""),
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn after() {}\n").unwrap();
    git(root, &["commit", "-qam", "chore: downgrade"]);
    assert_blocks(
        &ci(root, "chore/downgrade", &body("")),
        "tier downgraded",
        "unclassified pull request",
    );
    git(root, &["switch", "-q", "-c", "chore/unstate", "main"]);
    git(root, &["rm", "-q", ".codeflow/project.toml"]);
    std::fs::write(root.join("src/lib.rs"), "pub fn after() {}\n").unwrap();
    git(root, &["commit", "-qam", "chore: drop the state"]);
    assert_blocks(
        &ci(root, "chore/unstate", &body("")),
        "state removed",
        "unclassified pull request",
    );
}

#[test]
fn a_pr_that_names_no_task_or_epic_is_refused_whatever_it_touches() {
    let dir = tracked_repo(DEFAULT_POLICY);
    for path in ["docs/plan/next.md", "docs/guide.md", "src/new.rs"] {
        branch_with(dir.path(), "plan/next", &[(path, "change\n")]);
        for line in ["", "Task:", "Task: none: small work"] {
            assert_blocks(
                &ci(dir.path(), "plan/next", &body(line)),
                path,
                "work.classification",
            );
        }
    }
}

#[test]
fn tracking_inactive_requires_a_named_unit_and_rejects_placeholders() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["commit", "--allow-empty", "-m", "chore: start"]);
    branch_with(root, "docs/unit", &[("docs/guide.md", "guide\n")]);
    assert_passes(
        &ci(root, "docs/unit", &body("Task: clarify installation")),
        "named unit",
    );
    for line in [
        "",
        "Task:",
        "Task: none",
        "Task: none: typo",
        "Task: `TSK-NNN | EPC-NNN | <unit name>`",
        "Task: TSK-NNN",
        "Task: one\nTask: two",
        "Task: `unclosed unit",
        "Task: unclosed unit`",
    ] {
        assert_blocks(
            &ci(root, "docs/unit", &body(line)),
            line,
            "work.classification",
        );
    }
}

#[test]
fn standalone_record_and_code_are_admitted_together_at_completion() {
    let dir = tracked_repo(DEFAULT_POLICY);
    let root = dir.path();
    let own = task("TSK-003", "feat", "todo")
        .replace("epic_id: EPC-001", "epic_id: null")
        .replace(
            "standalone_reason: null",
            "standalone_reason: one bounded outcome",
        );
    branch_with(
        root,
        "task/TSK-003-work",
        &[
            ("project-management/tasks/TSK-003.md", &own),
            ("src/new.rs", "pub fn work() {}\n"),
        ],
    );
    let seeded = codeflow()
        .args(["ids", "seed"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    let reviewed = String::from_utf8(out.stdout).unwrap().trim().to_string();
    let complete = format!("{}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review/1\n  criteria:\n    AC-1: verified | test ran\n    AC-2: verified | journey ran\n  journey: verified | CLI fixture\n  not_verified: none\n  follow_ups: none: done\n  verdict: approved\n```\n", own.replace("status: todo", "status: complete"));
    std::fs::write(root.join("project-management/tasks/TSK-003.md"), complete).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "docs: complete standalone"]);
    assert_passes(
        &ci(root, "task/TSK-003-work", &body("Task: TSK-003")),
        "standalone completion",
    );
    let out = codeflow()
        .args(["work", "start", "TSK-003"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot start from status"));
    std::fs::write(
        root.join("project-management/tasks/TSK-004.md"),
        task("TSK-004", "feat", "todo"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "docs: add unrelated task"]);
    assert_blocks(
        &ci(root, "task/TSK-003-work", &body("Task: TSK-003")),
        "second record",
        "only its own standalone task record",
    );
}
