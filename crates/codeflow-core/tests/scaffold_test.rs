//! Integration tests for the scaffold engine — the trust gate.
//!
//! Everything runs in tempdirs against fixture asset trees (`DirSource`),
//! with git config isolated from the host machine.

use std::path::{Path, PathBuf};
use std::process::Command;

use codeflow_core::scaffold::{
    self, Action, DirSource, InitAnswers, InitOptions, Report, Tier, UpdateOptions,
};

// --- fixtures ---------------------------------------------------------------

/// Point git at empty global/system config for the whole test process. The
/// engine under test spawns its own `git` subprocesses which inherit the
/// process environment, so per-`Command` `.env()` cannot reach them. The
/// process-global mutation is made race-free by doing it exactly once behind
/// a `Once` that every test synchronizes on before its first spawn — POSIX
/// `setenv` concurrent with `getenv`/spawn is a data race, so repeated
/// `set_var` from parallel tests (even with identical values) is not benign.
fn isolate_git() {
    static ISOLATE: std::sync::Once = std::sync::Once::new();
    ISOLATE.call_once(|| {
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
    });
}

const MANIFEST: &str = r#"
schema_version = 1

[[entry]]
src = "AGENTS.md.tmpl"
dest = "AGENTS.md"
ownership = "managed-region"
region = "markdown"
tiers = ["minimal", "standard", "full"]
template = true

[[entry]]
src = "gitignore"
dest = ".gitignore"
ownership = "managed-region"
region = "hash"
tiers = ["minimal", "standard", "full"]

[[entry]]
src = "policy.json"
dest = ".codeflow/policy.json"
ownership = "user-owned"
tiers = ["minimal", "standard", "full"]

[[entry]]
src = "git-hooks/pre-commit"
dest = ".codeflow/git-hooks/pre-commit"
ownership = "managed"
tiers = ["minimal", "standard", "full"]
exec = true

[[entry]]
src = "claude/workflows/develop.md"
dest = ".claude/workflows/develop.md"
ownership = "managed"
tiers = ["standard", "full"]

[[entry]]
src = "settings/default.json"
dest = ".claude/settings.json"
ownership = "managed-region"
region = "json"
tiers = ["standard", "full"]
preset = "default"

[[entry]]
src = "settings/acceptEdits.json"
dest = ".claude/settings.json"
ownership = "managed-region"
region = "json"
tiers = ["standard", "full"]
preset = "acceptEdits"

[[entry]]
src = "docs/product.md.tmpl"
dest = "docs/product.md"
ownership = "user-owned"
tiers = ["standard", "full"]
template = true

[[entry]]
src = "docs/decisions/ADR-0001-stack-choice.md.tmpl"
dest = "docs/decisions/ADR-0001-stack-choice.md"
ownership = "user-owned"
tiers = ["standard", "full"]
template = true

[[entry]]
src = "ci/codeflow-ci.yml"
dest = ".github/workflows/codeflow-ci.yml"
ownership = "managed"
tiers = ["standard", "full"]

[[entry]]
src = "pm/epic.md.tmpl"
dest = "project-management/templates/epic.md"
ownership = "managed"
tiers = ["full"]

# Deliberately never authored in the fixture: missing-asset grace.
[[entry]]
src = "claude/agents/cf-reviewer.md"
dest = ".claude/agents/cf-reviewer.md"
ownership = "managed"
tiers = ["standard", "full"]
"#;

const DEVELOP_V1: &str = "# develop workflow\n\nstep one\nstep two\nstep three\nstep four\nstep five\nstep six\nstep seven\nstep eight\n";
const DEVELOP_V2: &str = "# develop workflow\n\nstep one (improved)\nstep two\nstep three\nstep four (v2)\nstep five\nstep six\nstep seven\nstep eight\n";

/// Builds a fixture asset tree. `v2` flips the upstream content forward.
fn fixture_assets(v2: bool) -> (tempfile::TempDir, DirSource) {
    let dir = tempfile::tempdir().expect("assets tempdir");
    let base = dir.path().join("base");
    let write = |rel: &str, content: &str| {
        let path = base.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    };

    write("scaffold-manifest.toml", MANIFEST.trim_start());
    let rules = if v2 {
        "rules v2 for {{PROJECT_NAME}}\nextra rule"
    } else {
        "rules v1 for {{PROJECT_NAME}}"
    };
    write(
        "AGENTS.md.tmpl",
        &format!(
            "# {{{{PROJECT_NAME}}}} contract\n\nIntro outside markers.\n\n<!-- codeflow:managed:begin scaffold={{{{SCAFFOLD_VERSION}}}} -->\n{rules}\n<!-- codeflow:managed:end -->\n\nFooter outside markers.\n"
        ),
    );
    write("gitignore", ".env\n*.pem\n");
    let policy = if v2 {
        r#"{
  "schema_version": 2,
  "git": {
    "commit_to_protected": "block",
    "commit_format": "block",
    "secret_scan": "block",
    "test_gate_on_push": "block",
    "new_gate": "warn"
  },
  "recall": { "share": false }
}"#
    } else {
        r#"{
  "schema_version": 1,
  "git": {
    "commit_to_protected": "block",
    "commit_format": "block",
    "secret_scan": "block",
    "test_gate_on_push": "warn"
  }
}"#
    };
    write("policy.json", policy);
    write("git-hooks/pre-commit", "#!/bin/sh\nexit 0\n");
    write(
        "claude/workflows/develop.md",
        if v2 { DEVELOP_V2 } else { DEVELOP_V1 },
    );
    let settings = |mode: &str| {
        if v2 {
            format!(
                r#"{{
  "permissions": {{ "defaultMode": "{mode}", "deny": ["Read(**/.env)", "Read(**/*.pem)"] }},
  "hooks": {{
    "PreToolUse": [ {{"matcher": "Bash", "hooks": [{{"type": "command", "command": "codeflow hook git-guard"}}]}} ],
    "SessionStart": [ {{"hooks": [{{"type": "command", "command": "codeflow hook session-orient"}}]}} ]
  }},
  "sandbox": {{ "filesystem": {{ "allowRead": ["~/.claude/plugins/cache"] }} }}
}}"#
            )
        } else {
            format!(
                r#"{{
  "permissions": {{ "defaultMode": "{mode}", "deny": ["Read(**/.env)"] }},
  "hooks": {{
    "PreToolUse": [ {{"matcher": "Bash", "hooks": [{{"type": "command", "command": "codeflow hook git-guard"}}]}} ]
  }},
  "sandbox": {{ "filesystem": {{ "allowRead": ["~/.claude/plugins"] }} }}
}}"#
            )
        }
    };
    write("settings/default.json", &settings("default"));
    write("settings/acceptEdits.json", &settings("acceptEdits"));
    write(
        "docs/product.md.tmpl",
        "# {{PROJECT_NAME}}\n\n{{PROJECT_ONE_LINER}}\n\nstack: {{STACK}}\nareas: {{AREAS}}\npurpose: {{PRODUCT_PURPOSE}}\n",
    );
    write(
        "docs/decisions/ADR-0001-stack-choice.md.tmpl",
        "---\nid: ADR-0001\n---\n\n# Initial stack: {{STACK}}\n",
    );
    write("ci/codeflow-ci.yml", "name: codeflow-ci\non: [push]\n");
    write("pm/epic.md.tmpl", "# {{EPIC_ID}} - {{TITLE}}\n");

    let source = DirSource::new(dir.path());
    (dir, source)
}

/// Rewrites a fixture's `scaffold-manifest.toml` to drop the `[[entry]]` whose
/// `dest` matches — simulating an artifact removed or renamed upstream.
fn drop_manifest_entry(assets_dir: &Path, dest: &str) {
    let mpath = assets_dir.join("base/scaffold-manifest.toml");
    let text = std::fs::read_to_string(&mpath).unwrap();
    let marker = "[[entry]]";
    let head_end = text.find(marker).expect("manifest has entries");
    let needle = format!("dest = \"{dest}\"");
    let mut out = text[..head_end].to_string();
    for block in text[head_end..].split(marker) {
        if block.trim().is_empty() || block.contains(&needle) {
            continue;
        }
        out.push_str(marker);
        out.push_str(block);
    }
    std::fs::write(&mpath, out).unwrap();
}

fn opts(tier: Option<Tier>, version: &str) -> InitOptions {
    InitOptions {
        tier,
        force: false,
        binary_version: version.to_string(),
        answers: InitAnswers::default(),
    }
}

fn update_opts(version: &str) -> UpdateOptions {
    UpdateOptions {
        force: false,
        binary_version: version.to_string(),
        diff_out: None,
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn action_of(report: &Report, dest: &str) -> Action {
    report
        .files
        .iter()
        .find(|f| f.dest == dest)
        .unwrap_or_else(|| {
            panic!(
                "no report entry for {dest}; have {:?}",
                report.files.iter().map(|f| &f.dest).collect::<Vec<_>>()
            )
        })
        .action
}

fn project_dir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("project tempdir");
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    (dir, root)
}

// --- init -------------------------------------------------------------------

#[test]
fn fresh_init_empty_dir_bootstrap_grace() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();

    let report = scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();

    // Bootstrap grace: repo created, exactly one commit, made by init itself.
    assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]), "1");
    let subject = git(&root, &["log", "-1", "--format=%s"]);
    assert_eq!(subject, "chore: scaffold codeflow standard tier");
    let body = git(&root, &["log", "-1", "--format=%B"]);
    assert!(!body.contains("Co-Authored-By"), "no AI attribution");

    // Policy is armed BEFORE the scaffold commit, so the committed state is
    // armed too — a later checkout of the protected branch (or a fresh clone)
    // must never resurrect a disarmed bootstrap state.
    let project_toml = read(&root, ".codeflow/project.toml");
    assert!(project_toml.contains("policy_armed = true"));
    let committed = git(&root, &["show", "HEAD:.codeflow/project.toml"]);
    assert!(
        committed.contains("policy_armed = true"),
        "the scaffold commit must carry the armed state"
    );
    assert!(project_toml.contains("tier = \"standard\""));
    assert!(project_toml.contains("scaffold_version = \"2.0.0\""));

    // Files, substitution, ownership records.
    let agents = read(&root, "AGENTS.md");
    assert!(agents.contains("# proj contract"));
    assert!(agents.contains("rules v1 for proj"));
    let product = read(&root, "docs/product.md");
    assert!(product.contains("# proj"));
    assert!(product.contains("stack: unset"));
    assert!(product.contains("areas: core"));
    assert!(
        product.contains("{{PRODUCT_PURPOSE}}"),
        "unknown placeholder survives"
    );
    assert!(root.join(".codeflow/manifest.json").exists());
    assert!(root.join(".codeflow/.baseline/AGENTS.md").exists());
    assert!(root.join(".claude/settings.json").exists());
    assert!(root
        .join("docs/decisions/ADR-0001-stack-choice.md")
        .exists());
    assert!(read(&root, ".codeflow/manifest.json").contains("ADR-0001-stack-choice.md"));

    // Hook wiring + exec bit.
    assert_eq!(
        git(&root, &["config", "core.hooksPath"]),
        ".codeflow/git-hooks"
    );
    assert!(project_toml.contains("git_hooks = \"wired\""));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(root.join(".codeflow/git-hooks/pre-commit"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "hook shim is executable");
    }

    // Missing asset: skipped + warned, init still succeeded.
    assert_eq!(
        action_of(&report, ".claude/agents/cf-reviewer.md"),
        Action::MissingAsset
    );
    assert!(report.warnings.iter().any(|w| w.contains("cf-reviewer")));
    assert_eq!(action_of(&report, "AGENTS.md"), Action::Created);
    assert_eq!(
        action_of(&report, ".github/workflows/codeflow-ci.yml"),
        Action::Created
    );
}

#[test]
fn fresh_init_leaves_clean_committable_tree() {
    // Regression: init must not leave a fresh adopter with a dirty,
    // un-committable tree. The scaffold commit has to capture project.toml's
    // FINAL policy_armed + git_hooks values — otherwise a post-commit re-store
    // flips git_hooks ("unwired" -> "wired") and leaves project.toml modified on
    // the just-armed protected branch, where the commit_to_protected gate then
    // refuses to let the adopter commit it.
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();

    scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();

    // Nothing left uncommitted or untracked after a fresh init.
    let status = git(&root, &["status", "--porcelain"]);
    assert!(
        status.is_empty(),
        "working tree not clean after init:\n{status}"
    );

    // The committed project.toml already carries the final wired + armed record,
    // so the working copy matches HEAD and a checkout/clone never resurrects a
    // stale value.
    let committed = git(&root, &["show", "HEAD:.codeflow/project.toml"]);
    assert!(committed.contains("policy_armed = true"), "{committed}");
    assert!(committed.contains("git_hooks = \"wired\""), "{committed}");
}

#[test]
fn brownfield_init_and_update_do_not_add_a_second_adr_0001() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();

    std::fs::create_dir_all(root.join("docs/decisions")).unwrap();
    std::fs::write(
        root.join("docs/decisions/ADR-0001-existing-boundary.md"),
        "---\nid: ADR-0001\n---\n\n# Existing boundary\n",
    )
    .unwrap();
    git(&root, &["init"]);
    git(&root, &["config", "user.name", "CodeFlow Test"]);
    git(&root, &["config", "user.email", "codeflow@example.invalid"]);
    git(
        &root,
        &["add", "docs/decisions/ADR-0001-existing-boundary.md"],
    );
    git(&root, &["commit", "-m", "docs: record existing boundary"]);

    let report = scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();
    assert_eq!(
        action_of(&report, "docs/decisions/ADR-0001-stack-choice.md"),
        Action::Skipped
    );
    assert!(!root
        .join("docs/decisions/ADR-0001-stack-choice.md")
        .exists());

    let installed = read(&root, ".codeflow/manifest.json");
    assert!(!installed.contains("ADR-0001-stack-choice.md"));

    let update = scaffold::update(&assets, &root, &update_opts("2.1.0")).unwrap();
    assert_eq!(
        action_of(&update, "docs/decisions/ADR-0001-stack-choice.md"),
        Action::Skipped
    );
    assert!(!root
        .join("docs/decisions/ADR-0001-stack-choice.md")
        .exists());
}

#[test]
fn init_minimal_tier_subset_enforces_full_policy() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();

    scaffold::init(&assets, &root, &opts(Some(Tier::Minimal), "2.0.0")).unwrap();

    assert!(root.join("AGENTS.md").exists());
    assert!(root.join(".gitignore").exists());
    assert!(root.join(".codeflow/git-hooks/pre-commit").exists());
    assert!(
        !root.join(".claude/settings.json").exists(),
        "standard-only"
    );
    assert!(!root.join("docs/product.md").exists(), "standard-only");
    assert!(!root.join("project-management").exists(), "full-only");

    // Minimal ships the SAME enforced policy.json as every other tier: git
    // discipline is armed from the first commit, never auto-softened. A project
    // may relax specific rules in policy.json deliberately, but the shipped
    // default enforces them because conventional commits drive the git-cliff
    // version/changelog automation.
    let policy: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    // The fixture policy ships commit_format + secret_scan as block; the old
    // softening would have flipped commit_format to warn. Both stay block, so
    // minimal now enforces git discipline exactly like the other tiers.
    assert_eq!(
        policy["git"]["commit_format"], "block",
        "enforced, not softened to warn"
    );
    assert_eq!(policy["git"]["secret_scan"], "block", "never softened");
    assert!(read(&root, ".codeflow/project.toml").contains("tier = \"minimal\""));
}

#[test]
fn tier_upgrade_is_additive() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();

    scaffold::init(&assets, &root, &opts(Some(Tier::Minimal), "2.0.0")).unwrap();
    // User customizes AGENTS.md outside the markers.
    let agents = read(&root, "AGENTS.md");
    std::fs::write(root.join("AGENTS.md"), format!("{agents}\nMy own rules.\n")).unwrap();

    let report = scaffold::init(&assets, &root, &opts(Some(Tier::Standard), "2.0.0")).unwrap();
    assert_eq!(action_of(&report, "AGENTS.md"), Action::Unchanged);
    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Created
    );
    assert_eq!(
        action_of(&report, ".codeflow/git-hooks/pre-commit"),
        Action::Unchanged
    );
    assert!(read(&root, "AGENTS.md").contains("My own rules."));
    assert!(read(&root, ".codeflow/project.toml").contains("tier = \"standard\""));

    let report = scaffold::init(&assets, &root, &opts(Some(Tier::Full), "2.0.0")).unwrap();
    assert_eq!(
        action_of(&report, "project-management/templates/epic.md"),
        Action::Created
    );
    assert!(read(&root, "project-management/templates/epic.md").contains("{{EPIC_ID}}"));

    // Downgrade request is refused (stop managing, never delete).
    let report = scaffold::init(&assets, &root, &opts(Some(Tier::Minimal), "2.0.0")).unwrap();
    assert!(read(&root, ".codeflow/project.toml").contains("tier = \"full\""));
    assert!(report.notes.iter().any(|n| n.contains("downgrade")));
}

#[test]
fn full_tier_init_after_standard_reports_pm_templates_created() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();

    scaffold::init(&assets, &root, &opts(Some(Tier::Standard), "2.0.0")).unwrap();
    assert!(
        !root.join("project-management").exists(),
        "pm templates are full-tier only"
    );

    let report = scaffold::init(&assets, &root, &opts(Some(Tier::Full), "2.0.0")).unwrap();
    let epic = report
        .files
        .iter()
        .find(|f| f.dest == "project-management/templates/epic.md")
        .expect("pm template reported");
    assert_eq!(
        epic.action,
        Action::Created,
        "did not pre-exist: must be created"
    );
    assert!(
        !epic.notes.iter().any(|n| n.contains("adopted")),
        "a freshly installed file must never be reported as adopted"
    );
}

#[test]
fn rerun_after_interrupted_init_reports_created_not_adopted() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();

    scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();

    // Simulate an init that wrote the file (and its baseline) but died before
    // storing the installed record: drop the record, keep file + baseline.
    let manifest_path = root.join(".codeflow/manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/manifest.json")).unwrap();
    manifest["files"]
        .as_object_mut()
        .unwrap()
        .remove(".claude/workflows/develop.md")
        .expect("record existed");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let report = scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();
    let file = report
        .files
        .iter()
        .find(|f| f.dest == ".claude/workflows/develop.md")
        .unwrap();
    assert_eq!(
        file.action,
        Action::Created,
        "codeflow wrote it (baseline matches): the rerun completes the create"
    );
    assert!(
        !file.notes.iter().any(|n| n.contains("adopted")),
        "must not claim adoption of a file the user never created"
    );
    // The record is restored: a third run is a plain unchanged.
    let report = scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();
    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Unchanged
    );
}

#[test]
fn init_adopts_genuinely_preexisting_identical_file_as_unchanged() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();

    // The user already has a file identical to the shipped asset, and
    // codeflow never touched this project (no baseline): genuine adoption.
    std::fs::create_dir_all(root.join(".claude/workflows")).unwrap();
    std::fs::write(root.join(".claude/workflows/develop.md"), DEVELOP_V1).unwrap();

    let report = scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();
    let file = report
        .files
        .iter()
        .find(|f| f.dest == ".claude/workflows/develop.md")
        .unwrap();
    assert_eq!(
        file.action,
        Action::Unchanged,
        "pre-existing file: unchanged"
    );
    assert!(
        file.notes.iter().any(|n| n.contains("adopted as managed")),
        "adoption is reported, never silent"
    );
}

#[test]
fn init_into_existing_repo_is_nondestructive() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();

    // A lived-in repo: commits, AGENTS.md, settings.json, husky, gitignore.
    git(&root, &["init", "--quiet"]);
    std::fs::write(
        root.join("AGENTS.md"),
        "# My agents\n\ncustom instructions\n",
    )
    .unwrap();
    std::fs::write(root.join(".gitignore"), "node_modules/\n").unwrap();
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(
        root.join(".claude/settings.json"),
        r#"{
  "model": "opus",
  "permissions": { "defaultMode": "plan", "deny": ["Read(secrets/**)"] },
  "hooks": {
    "PreToolUse": [ {"matcher": "Bash", "hooks": [{"type": "command", "command": "./my-guard.sh"}]} ]
  }
}"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join(".husky")).unwrap();
    std::fs::write(root.join(".husky/pre-commit"), "npx lint-staged\n").unwrap();
    git(&root, &["add", "-A"]);
    git(
        &root,
        &[
            "-c",
            "user.name=u",
            "-c",
            "user.email=u@x",
            "commit",
            "-qm",
            "chore: seed",
        ],
    );

    let report = scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();

    // AGENTS.md: block appended, user content untouched.
    assert_eq!(action_of(&report, "AGENTS.md"), Action::Merged);
    let agents = read(&root, "AGENTS.md");
    assert!(agents.starts_with("# My agents"));
    assert!(agents.contains("custom instructions"));
    assert!(agents.contains("codeflow:managed:begin"));
    assert!(agents.contains("rules v1 for proj"));

    // .gitignore: hash-marker block appended.
    let gitignore = read(&root, ".gitignore");
    assert!(gitignore.starts_with("node_modules/"));
    assert!(gitignore.contains("# codeflow:managed:begin"));
    assert!(gitignore.contains(".env"));

    // settings.json: structured merge with a printed report.
    assert_eq!(action_of(&report, ".claude/settings.json"), Action::Merged);
    let settings: serde_json::Value =
        serde_json::from_str(&read(&root, ".claude/settings.json")).unwrap();
    assert_eq!(settings["model"], "opus");
    assert_eq!(
        settings["permissions"]["defaultMode"], "plan",
        "user value preserved"
    );
    let deny = settings["permissions"]["deny"].as_array().unwrap();
    assert!(deny.iter().any(|d| d == "Read(secrets/**)"));
    assert!(deny.iter().any(|d| d == "Read(**/.env)"));
    let hooks = settings["hooks"]["PreToolUse"][0]["hooks"]
        .as_array()
        .unwrap();
    assert!(hooks.iter().any(|h| h["command"] == "./my-guard.sh"));
    assert!(hooks
        .iter()
        .any(|h| h["command"] == "codeflow hook git-guard"));
    let merge_notes = &report
        .files
        .iter()
        .find(|f| f.dest == ".claude/settings.json")
        .unwrap()
        .notes;
    assert!(
        merge_notes.iter().any(|n| n.contains("git-guard")),
        "merge report printed"
    );

    // husky: hooks left alone, recorded unwired, instructions printed.
    assert_eq!(git(&root, &["config", "core.hooksPath"]), "");
    assert!(read(&root, ".codeflow/project.toml").contains("git_hooks = \"unwired\""));
    assert!(report.notes.iter().any(|n| n.contains("husky")));
    assert_eq!(read(&root, ".husky/pre-commit"), "npx lint-staged\n");

    // Existing history: no scaffold commit on top of the user's branch.
    assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]), "1");
}

#[test]
fn init_is_idempotent() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();

    scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();
    let agents_before = read(&root, "AGENTS.md");
    let report = scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();

    assert_eq!(
        report.count(Action::Created),
        0,
        "second run creates nothing"
    );
    assert_eq!(report.count(Action::Changed), 0);
    assert_eq!(read(&root, "AGENTS.md"), agents_before);
    assert!(report.count(Action::Unchanged) > 0);
}

#[test]
fn init_notes_codex_posture_when_codex_layer_present() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();
    // The codex layer on disk, as the real standard/full manifest installs it.
    std::fs::create_dir_all(root.join(".codex")).unwrap();
    std::fs::write(root.join(".codex/hooks.json"), "{}").unwrap();
    std::fs::write(
        root.join(".codex/config.toml"),
        "approval_policy = \"on-request\"\n",
    )
    .unwrap();

    let report = scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();

    // One note: the permission preset configured Claude Code only; codex
    // posture lives in .codex/config.toml, and its in-session guards need the
    // one-time /hooks trust inside codex.
    let note = report
        .notes
        .iter()
        .find(|n| n.contains(".codex/config.toml"))
        .expect("codex posture note in the init report");
    assert!(
        note.contains("permission preset"),
        "scopes the Claude Code preset: {note}"
    );
    assert!(
        note.contains("`/hooks`"),
        "points at the one-time trust step: {note}"
    );
}

#[test]
fn init_without_codex_layer_has_no_codex_note() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();
    let report = scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();
    assert!(
        !report.notes.iter().any(|n| n.contains("codex")),
        "no codex note when no .codex/ layer exists: {:?}",
        report.notes
    );
}

#[test]
fn init_force_overwrites_unmanaged_file() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();

    std::fs::create_dir_all(root.join(".claude/workflows")).unwrap();
    std::fs::write(root.join(".claude/workflows/develop.md"), "mine\n").unwrap();

    let report = scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();
    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Skipped
    );
    assert_eq!(read(&root, ".claude/workflows/develop.md"), "mine\n");

    let mut forced = opts(None, "2.0.0");
    forced.force = true;
    let report = scaffold::init(&assets, &root, &forced).unwrap();
    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Forced
    );
    assert_eq!(read(&root, ".claude/workflows/develop.md"), DEVELOP_V1);
}

#[test]
fn init_records_answers_and_detects_stack() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();
    std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();

    let options = InitOptions {
        tier: None,
        force: false,
        binary_version: "2.0.0".to_string(),
        answers: InitAnswers {
            product_one_liner: Some("a discipline layer".to_string()),
            areas: Some(vec!["core".to_string(), "cli".to_string()]),
            permission_preset: Some("acceptEdits".to_string()),
        },
    };
    scaffold::init(&assets, &root, &options).unwrap();

    let product = read(&root, "docs/product.md");
    assert!(product.contains("a discipline layer"));
    assert!(product.contains("stack: rust"));
    assert!(product.contains("areas: core, cli"));
    let settings: serde_json::Value =
        serde_json::from_str(&read(&root, ".claude/settings.json")).unwrap();
    assert_eq!(settings["permissions"]["defaultMode"], "acceptEdits");
    let toml = read(&root, ".codeflow/project.toml");
    assert!(toml.contains("permission_preset = \"acceptEdits\""));
    assert!(toml.contains("stack = \"rust\""));
}

#[test]
fn standard_init_points_to_consuming_project_customization() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();
    let report = scaffold::init(&assets, &root, &opts(Some(Tier::Standard), "2.0.0")).unwrap();
    let note = report
        .notes
        .iter()
        .find(|note| note.contains("/cf-customize"))
        .expect("standard init prints the customization next step");
    for artifact in [
        "docs/product.md",
        "docs/architecture.md",
        "AGENTS.md",
        "CLAUDE.md",
    ] {
        assert!(
            note.contains(artifact),
            "customization note misses {artifact}"
        );
    }
    assert!(note.contains("tools/MCPs"));
    assert!(note.contains("effective settings"));
}

#[test]
fn minimal_init_does_not_advertise_an_uninstalled_skill() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();
    let report = scaffold::init(&assets, &root, &opts(Some(Tier::Minimal), "2.0.0")).unwrap();
    assert!(
        !report
            .notes
            .iter()
            .any(|note| note.contains("/cf-customize")),
        "minimal tier must not point to a skill it does not install"
    );
}

// --- update -----------------------------------------------------------------

fn init_v1(root: &Path) -> tempfile::TempDir {
    let (assets_dir, assets) = fixture_assets(false);
    scaffold::init(&assets, root, &opts(None, "2.0.0")).unwrap();
    assets_dir
}

#[test]
fn update_replaces_unmodified_managed_files() {
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);

    let (_a2, assets_v2) = fixture_assets(true);
    let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();

    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Changed
    );
    assert_eq!(
        report.count(Action::Removed),
        0,
        "nothing is pruned when the manifest still ships every file"
    );
    assert_eq!(read(&root, ".claude/workflows/develop.md"), DEVELOP_V2);
    assert_eq!(
        read(&root, ".codeflow/.baseline/.claude/workflows/develop.md"),
        DEVELOP_V2,
        "baseline refreshed"
    );
    assert!(read(&root, ".codeflow/project.toml").contains("scaffold_version = \"2.1.0\""));
    assert!(read(&root, ".codeflow/manifest.json").contains("\"scaffold_version\": \"2.1.0\""));
}

#[test]
fn update_keeps_existing_user_owned_baseline_stable_after_root_rename() {
    isolate_git();
    let (project, root) = project_dir();
    let _v1 = init_v1(&root);

    let live_before = read(&root, "docs/product.md");
    let baseline_before = read(&root, ".codeflow/.baseline/docs/product.md");
    let manifest_before: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/manifest.json")).unwrap();
    let hash_before = manifest_before["files"]["docs/product.md"]["sha256"]
        .as_str()
        .unwrap()
        .to_string();

    let renamed_root = project.path().join("linked-worktree-name");
    std::fs::rename(&root, &renamed_root).unwrap();
    let (_a2, assets_v2) = fixture_assets(true);
    let report = scaffold::update(&assets_v2, &renamed_root, &update_opts("2.1.0")).unwrap();

    assert_eq!(action_of(&report, "docs/product.md"), Action::Skipped);
    assert_eq!(read(&renamed_root, "docs/product.md"), live_before);
    assert_eq!(
        read(&renamed_root, ".codeflow/.baseline/docs/product.md"),
        baseline_before,
        "a linked worktree name must not rewrite a write-once doc baseline"
    );
    let manifest_after: serde_json::Value =
        serde_json::from_str(&read(&renamed_root, ".codeflow/manifest.json")).unwrap();
    assert_eq!(
        manifest_after["files"]["docs/product.md"]["sha256"].as_str(),
        Some(hash_before.as_str()),
        "a linked worktree name must not rewrite the installed snapshot hash"
    );
}

#[test]
fn update_restores_a_missing_user_owned_baseline_without_touching_the_live_file() {
    isolate_git();
    let (_project, root) = project_dir();
    let _v1 = init_v1(&root);

    let live_before = read(&root, "docs/product.md");
    let baseline_before = read(&root, ".codeflow/.baseline/docs/product.md");
    std::fs::remove_file(root.join(".codeflow/.baseline/docs/product.md")).unwrap();

    let (_a2, assets_v2) = fixture_assets(true);
    let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();

    assert_eq!(action_of(&report, "docs/product.md"), Action::Skipped);
    assert_eq!(read(&root, "docs/product.md"), live_before);
    assert_eq!(
        read(&root, ".codeflow/.baseline/docs/product.md"),
        baseline_before,
        "a missing write-once baseline should self-heal from the current scaffold"
    );
    let notes = &report
        .files
        .iter()
        .find(|file| file.dest == "docs/product.md")
        .unwrap()
        .notes;
    assert!(notes.iter().any(|note| note.contains("baseline restored")));
}

#[test]
fn update_three_way_merges_user_modified_file() {
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);

    // User edits the tail; upstream v2 edits the head. Disjoint = clean merge.
    let mine =
        read(&root, ".claude/workflows/develop.md").replace("step eight", "step eight (mine)");
    std::fs::write(root.join(".claude/workflows/develop.md"), &mine).unwrap();

    let (_a2, assets_v2) = fixture_assets(true);
    let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();

    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Merged
    );
    let merged = read(&root, ".claude/workflows/develop.md");
    assert!(
        merged.contains("step one (improved)"),
        "upstream change applied"
    );
    assert!(merged.contains("step eight (mine)"), "user change kept");
    assert!(!root.join(".claude/workflows/develop.md.new").exists());
}

#[test]
fn update_twice_preserves_merged_user_edits() {
    // Regression: after a clean 3-way merge, a SECOND `codeflow update` must not
    // silently wipe the merged-in user edits. The bug recorded hash(merged) as
    // the manifest hash, so the next run classified the file "unmodified" and
    // overwrote it with the pristine shipped version.
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);

    let mine =
        read(&root, ".claude/workflows/develop.md").replace("step eight", "step eight (mine)");
    std::fs::write(root.join(".claude/workflows/develop.md"), &mine).unwrap();

    let (_a2, assets_v2) = fixture_assets(true);
    // First update: clean 3-way merge (upstream head change + user tail edit).
    let r1 = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();
    assert_eq!(
        action_of(&r1, ".claude/workflows/develop.md"),
        Action::Merged
    );
    let after1 = read(&root, ".claude/workflows/develop.md");
    assert!(after1.contains("step eight (mine)") && after1.contains("step one (improved)"));

    // Second update against the SAME assets (no new upstream change): the merged
    // user edit must survive.
    let _r2 = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();
    let after2 = read(&root, ".claude/workflows/develop.md");
    assert!(
        after2.contains("step eight (mine)"),
        "second update wiped the merged user edit"
    );
    assert!(!root.join(".claude/workflows/develop.md.new").exists());
}

#[test]
fn update_conflict_writes_dot_new_and_never_clobbers() {
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);

    // User and upstream both rewrite the same line: conflict.
    let mine = read(&root, ".claude/workflows/develop.md").replace("step one", "step one (user)");
    std::fs::write(root.join(".claude/workflows/develop.md"), &mine).unwrap();

    let (_a2, assets_v2) = fixture_assets(true);
    let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();

    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Conflicted
    );
    assert!(report.has_conflicts());
    assert_eq!(
        read(&root, ".claude/workflows/develop.md"),
        mine,
        "file untouched"
    );
    // The proposal is the merge with conflict markers: the user's line and
    // the upstream line both survive for the user to resolve.
    let proposal = read(&root, ".claude/workflows/develop.md.new");
    assert!(proposal.contains("<<<<<<<"), "{proposal}");
    assert!(proposal.contains("step one (user)"), "{proposal}");
    assert!(
        DEVELOP_V2.lines().all(|l| proposal.contains(l)),
        "{proposal}"
    );
    let notes = &report
        .files
        .iter()
        .find(|f| f.dest == ".claude/workflows/develop.md")
        .unwrap()
        .notes;
    assert!(notes.iter().any(|n| n.contains(".new")));
}

#[test]
fn update_prunes_orphaned_unmodified_managed_file() {
    // An artifact removed/renamed upstream (its manifest entry gone) must be
    // pruned — the file, its baseline, and its manifest record — not left as a
    // lingering orphan that collides with a renamed replacement.
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);
    assert!(root.join(".claude/workflows/develop.md").exists());
    assert!(root
        .join(".codeflow/.baseline/.claude/workflows/develop.md")
        .exists());

    let (a2, _) = fixture_assets(true);
    drop_manifest_entry(a2.path(), ".claude/workflows/develop.md");
    let report =
        scaffold::update(&DirSource::new(a2.path()), &root, &update_opts("2.1.0")).unwrap();

    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Removed
    );
    assert!(
        !root.join(".claude/workflows/develop.md").exists(),
        "orphan file removed"
    );
    assert!(
        !root
            .join(".codeflow/.baseline/.claude/workflows/develop.md")
            .exists(),
        "orphan baseline removed"
    );
    assert!(
        !root.join(".claude/workflows").exists(),
        "now-empty parent dir cleaned"
    );
    assert!(
        !read(&root, ".codeflow/manifest.json").contains(".claude/workflows/develop.md"),
        "manifest record dropped"
    );
}

#[test]
fn update_prunes_already_deleted_managed_orphan() {
    // If the orphan's file was already removed from disk, update still cleans up
    // its baseline + record and reports it removed (the NotFound branch).
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);
    std::fs::remove_file(root.join(".claude/workflows/develop.md")).unwrap();

    let (a2, _) = fixture_assets(true);
    drop_manifest_entry(a2.path(), ".claude/workflows/develop.md");
    let report =
        scaffold::update(&DirSource::new(a2.path()), &root, &update_opts("2.1.0")).unwrap();

    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Removed
    );
    assert!(
        !root
            .join(".codeflow/.baseline/.claude/workflows/develop.md")
            .exists(),
        "baseline cleaned even though the file was already gone"
    );
    assert!(
        !read(&root, ".codeflow/manifest.json").contains(".claude/workflows/develop.md"),
        "record dropped"
    );
}

#[test]
fn update_keeps_user_modified_managed_orphan_unmanaged() {
    // A user-MODIFIED managed file that is dropped upstream is never deleted —
    // it is kept on disk and merely unmanaged.
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);
    std::fs::write(
        root.join(".claude/workflows/develop.md"),
        "my own workflow\n",
    )
    .unwrap();

    let (a2, _) = fixture_assets(true);
    drop_manifest_entry(a2.path(), ".claude/workflows/develop.md");
    let report =
        scaffold::update(&DirSource::new(a2.path()), &root, &update_opts("2.1.0")).unwrap();

    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::KeptUserModified
    );
    assert_eq!(
        read(&root, ".claude/workflows/develop.md"),
        "my own workflow\n",
        "user's file is never deleted"
    );
    assert!(
        !read(&root, ".codeflow/manifest.json").contains(".claude/workflows/develop.md"),
        "no longer managed"
    );
}

#[test]
fn update_never_deletes_user_owned_orphan() {
    // A user-owned file dropped upstream is kept (never codeflow's to delete),
    // just unmanaged.
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);
    assert!(root.join("docs/product.md").exists());

    let (a2, _) = fixture_assets(true);
    drop_manifest_entry(a2.path(), "docs/product.md");
    let report =
        scaffold::update(&DirSource::new(a2.path()), &root, &update_opts("2.1.0")).unwrap();

    assert_eq!(action_of(&report, "docs/product.md"), Action::Skipped);
    assert!(
        root.join("docs/product.md").exists(),
        "user-owned file kept"
    );
    assert!(
        !read(&root, ".codeflow/manifest.json").contains("docs/product.md"),
        "no longer managed"
    );
}

#[test]
fn update_keeps_user_modified_file_when_upstream_unchanged() {
    isolate_git();
    let (_p, root) = project_dir();
    let assets_v1_dir = init_v1(&root);

    let mine = read(&root, ".claude/workflows/develop.md").replace("step two", "step two (mine)");
    std::fs::write(root.join(".claude/workflows/develop.md"), &mine).unwrap();

    // Same assets, same version: nothing upstream changed.
    let report = scaffold::update(
        &DirSource::new(assets_v1_dir.path()),
        &root,
        &update_opts("2.0.0"),
    )
    .unwrap();
    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::KeptUserModified
    );
    assert_eq!(read(&root, ".claude/workflows/develop.md"), mine);
}

#[test]
fn update_regenerates_agents_region_round_trip() {
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);

    // User wraps the managed block with their own content.
    let agents = read(&root, "AGENTS.md");
    std::fs::write(
        root.join("AGENTS.md"),
        format!("PREFACE BY USER\n\n{agents}\nAPPENDIX BY USER\n"),
    )
    .unwrap();

    let (_a2, assets_v2) = fixture_assets(true);
    let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();
    assert_eq!(action_of(&report, "AGENTS.md"), Action::Changed);

    let updated = read(&root, "AGENTS.md");
    assert!(updated.starts_with("PREFACE BY USER"));
    assert!(updated.ends_with("APPENDIX BY USER\n"));
    assert!(updated.contains("rules v2 for proj"));
    assert!(updated.contains("extra rule"));
    assert!(!updated.contains("rules v1"));
    assert!(updated.contains("scaffold=2.1.0"));

    // Round-trip: a second identical update is a no-op.
    let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();
    assert_eq!(action_of(&report, "AGENTS.md"), Action::Unchanged);
    assert_eq!(read(&root, "AGENTS.md"), updated);
}

#[test]
fn update_settings_merge_preserves_foreign_keys() {
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);

    // User adds their own keys and hooks after init.
    let mut settings: serde_json::Value =
        serde_json::from_str(&read(&root, ".claude/settings.json")).unwrap();
    settings["model"] = "opus".into();
    settings["hooks"]["PreToolUse"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"type": "command", "command": "./mine.sh"}));
    settings["sandbox"]["filesystem"]["allowRead"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("~/project-owned-reference"));
    std::fs::write(
        root.join(".claude/settings.json"),
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    let (_a2, assets_v2) = fixture_assets(true);
    let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();
    assert_eq!(action_of(&report, ".claude/settings.json"), Action::Merged);

    let after: serde_json::Value =
        serde_json::from_str(&read(&root, ".claude/settings.json")).unwrap();
    assert_eq!(after["model"], "opus");
    let hooks = after["hooks"]["PreToolUse"][0]["hooks"].as_array().unwrap();
    assert!(hooks.iter().any(|h| h["command"] == "./mine.sh"));
    assert!(hooks
        .iter()
        .any(|h| h["command"] == "codeflow hook git-guard"));
    assert_eq!(
        after["hooks"]["SessionStart"][0]["hooks"][0]["command"], "codeflow hook session-orient",
        "new v2 hook arrived"
    );
    let deny = after["permissions"]["deny"].as_array().unwrap();
    assert!(
        deny.iter().any(|d| d == "Read(**/*.pem)"),
        "new v2 deny rule arrived"
    );
    let allow_read = after["sandbox"]["filesystem"]["allowRead"]
        .as_array()
        .unwrap();
    assert!(allow_read
        .iter()
        .any(|entry| entry == "~/.claude/plugins/cache"));
    assert!(allow_read
        .iter()
        .any(|entry| entry == "~/project-owned-reference"));
    assert!(!allow_read.iter().any(|entry| entry == "~/.claude/plugins"));
}

#[test]
fn update_adds_new_policy_keys_without_mutating_user_values() {
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);

    // User flips a value and deletes a key they do not want.
    let mut policy: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    policy["git"]["commit_format"] = "warn".into();
    policy["git"]
        .as_object_mut()
        .unwrap()
        .remove("test_gate_on_push");
    std::fs::write(
        root.join(".codeflow/policy.json"),
        serde_json::to_string_pretty(&policy).unwrap(),
    )
    .unwrap();

    let (_a2, assets_v2) = fixture_assets(true);
    let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();
    assert_eq!(
        action_of(&report, ".codeflow/policy.json"),
        Action::KeysAdded
    );

    let after: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    assert_eq!(
        after["git"]["commit_format"], "warn",
        "user value never mutated"
    );
    assert!(
        after["git"].get("test_gate_on_push").is_none(),
        "user deletion respected"
    );
    assert_eq!(after["git"]["new_gate"], "warn", "new default key added");
    assert_eq!(after["recall"]["share"], false, "new section added");
    assert_eq!(after["schema_version"], 2, "schema_version advanced");
    let notes = &report
        .files
        .iter()
        .find(|f| f.dest == ".codeflow/policy.json")
        .unwrap()
        .notes;
    assert!(notes.iter().any(|n| n.contains("git.new_gate")));
}

#[test]
fn update_moves_test_gate_on_push_at_the_old_default_and_recommends_block() {
    // T132-4 as amended by ADR-0075 (TSK-171 AC-7): a value still equal to
    // the old shipped default moves to the new one, reported; a value that
    // differs from both is kept and the new default recommended.
    isolate_git();
    for (value, expected, note) in [
        (
            "warn",
            "block",
            Some("moved git.test_gate_on_push from \"warn\" to \"block\""),
        ),
        ("off", "off", Some("kept git.test_gate_on_push = \"off\"")),
        (
            "allow",
            "allow",
            Some("kept git.test_gate_on_push = \"allow\""),
        ),
        ("block", "block", None),
    ] {
        let (_p, root) = project_dir();
        let _v1 = init_v1(&root);
        let mut policy: serde_json::Value =
            serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
        policy["git"]["test_gate_on_push"] = value.into();
        std::fs::write(
            root.join(".codeflow/policy.json"),
            serde_json::to_string_pretty(&policy).unwrap(),
        )
        .unwrap();

        let (_a2, assets_v2) = fixture_assets(true);
        let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();

        let after: serde_json::Value =
            serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
        assert_eq!(after["git"]["test_gate_on_push"], expected, "{value}");
        let notes = &report
            .files
            .iter()
            .find(|f| f.dest == ".codeflow/policy.json")
            .unwrap()
            .notes;
        let found = notes.iter().find(|n| n.contains("git.test_gate_on_push"));
        assert_eq!(found.is_some(), note.is_some(), "{value}: {notes:?}");
        if let (Some(found), Some(note)) = (found, note) {
            assert!(found.starts_with(note), "{found}");
        }
    }
}

#[test]
fn update_removes_the_deprecated_human_authorization_key() {
    // TSK-137: an adopter file from before the removal carries the inert key;
    // update deletes it and says so, and keeps every other value.
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);
    let mut policy: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    policy["human_authorization"] = "none".into();
    policy["git"]["commit_format"] = "warn".into();
    std::fs::write(
        root.join(".codeflow/policy.json"),
        serde_json::to_string_pretty(&policy).unwrap(),
    )
    .unwrap();

    let (_a2, assets_v2) = fixture_assets(true);
    let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();

    let after: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    assert!(after.get("human_authorization").is_none(), "{after}");
    assert_eq!(after["git"]["commit_format"], "warn", "other values kept");
    let notes = &report
        .files
        .iter()
        .find(|f| f.dest == ".codeflow/policy.json")
        .unwrap()
        .notes;
    assert!(
        notes
            .iter()
            .any(|n| n == "removed deprecated key human_authorization"),
        "{notes:?}"
    );
}

#[test]
fn update_reinstalls_missing_managed_file() {
    isolate_git();
    let (_p, root) = project_dir();
    let assets_dir = init_v1(&root);

    std::fs::remove_file(root.join(".claude/workflows/develop.md")).unwrap();
    let report = scaffold::update(
        &DirSource::new(assets_dir.path()),
        &root,
        &update_opts("2.0.0"),
    )
    .unwrap();
    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Added
    );
    assert_eq!(read(&root, ".claude/workflows/develop.md"), DEVELOP_V1);
}

/// Appends a `[scaffold] ignore = [...]` opt-out to an initialized project.toml.
fn add_scaffold_ignore(root: &Path, globs: &[&str]) {
    let list = globs
        .iter()
        .map(|g| format!("\"{g}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let mut toml = read(root, ".codeflow/project.toml");
    toml.push_str("\n[scaffold]\nignore = [");
    toml.push_str(&list);
    toml.push_str("]\n");
    std::fs::write(root.join(".codeflow/project.toml"), toml).unwrap();
}

#[test]
fn update_ignore_glob_keeps_deleted_managed_file_deleted() {
    // A managed file the user deleted, matched by `[scaffold] ignore`, must NOT
    // be resurrected by update — and the opt-out survives update's rewrite of
    // project.toml.
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);
    add_scaffold_ignore(&root, &[".claude/**"]);

    std::fs::remove_file(root.join(".claude/workflows/develop.md")).unwrap();

    let (_a2, assets_v2) = fixture_assets(true);
    let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();

    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Skipped,
        "ignored file reported as skipped, not added"
    );
    assert!(
        !root.join(".claude/workflows/develop.md").exists(),
        "ignored managed file must stay deleted"
    );
    // The user-owned opt-out round-trips through update's project.toml rewrite.
    let toml = read(&root, ".codeflow/project.toml");
    assert!(
        toml.contains("[scaffold]") && toml.contains(".claude/**"),
        "opt-out preserved"
    );
}

#[test]
fn update_ignore_glob_leaves_non_matching_files_managed() {
    // A managed file OUTSIDE the ignore globs still upgrades normally — the
    // opt-out is scoped to matching paths only.
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);
    add_scaffold_ignore(&root, &[".codex/**"]);

    let (_a2, assets_v2) = fixture_assets(true);
    let report = scaffold::update(&assets_v2, &root, &update_opts("2.1.0")).unwrap();

    assert_eq!(
        action_of(&report, ".claude/workflows/develop.md"),
        Action::Changed,
        "non-ignored managed file still regenerates"
    );
    assert_eq!(read(&root, ".claude/workflows/develop.md"), DEVELOP_V2);
}

#[test]
fn update_ignore_glob_does_not_prune_ignored_orphan() {
    // A file upstream stopped shipping (an orphan) that also matches an ignore
    // glob must NOT be pruned or reported as orphaned — the user opted out.
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);
    add_scaffold_ignore(&root, &[".claude/**"]);

    let (a2, _) = fixture_assets(true);
    drop_manifest_entry(a2.path(), ".claude/workflows/develop.md");
    let report =
        scaffold::update(&DirSource::new(a2.path()), &root, &update_opts("2.1.0")).unwrap();

    assert_eq!(
        report.count(Action::Removed),
        0,
        "ignored orphan not pruned"
    );
    assert!(
        root.join(".claude/workflows/develop.md").exists(),
        "ignored orphan file kept on disk"
    );
    assert!(
        !report
            .files
            .iter()
            .any(|f| f.dest == ".claude/workflows/develop.md" && f.action == Action::Removed),
        "ignored orphan never reported as removed"
    );
}

#[test]
fn update_requires_initialized_project() {
    isolate_git();
    let (_a, assets) = fixture_assets(false);
    let (_p, root) = project_dir();
    let err = scaffold::update(&assets, &root, &update_opts("2.0.0")).unwrap_err();
    assert!(err.to_string().contains("codeflow init"));
}

#[test]
fn update_writes_diff_file() {
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);

    let (_a2, assets_v2) = fixture_assets(true);
    let diff_path = root.join("update.diff");
    let mut options = update_opts("2.1.0");
    options.diff_out = Some(diff_path.clone());
    scaffold::update(&assets_v2, &root, &options).unwrap();

    let diff = std::fs::read_to_string(&diff_path).unwrap();
    assert!(diff.contains("=== .claude/workflows/develop.md"));
    assert!(diff.contains("+step one (improved)"));
}

// --- version skew -------------------------------------------------------------

#[test]
fn version_skew_warns_when_behind_only() {
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root); // records scaffold_version 2.0.0

    let warning = scaffold::version_skew_warning(&root, "2.1.0").expect("warns when behind");
    assert!(warning.contains("2.0.0"));
    assert!(warning.contains("2.1.0"));
    assert!(warning.contains("codeflow update"));
    assert!(scaffold::version_skew_warning(&root, "2.0.0").is_none());

    // Uninitialized directory: silent.
    let (_q, other) = project_dir();
    assert!(scaffold::version_skew_warning(&other, "2.1.0").is_none());
}

// --- work records (SPC-013 R-81, R-83) ---------------------------------------

fn set_policy_key(root: &Path, key: &str, value: &str) {
    let mut policy: serde_json::Value =
        serde_json::from_str(&read(root, ".codeflow/policy.json")).unwrap();
    policy["git"][key] = value.into();
    std::fs::write(
        root.join(".codeflow/policy.json"),
        serde_json::to_string_pretty(&policy).unwrap(),
    )
    .unwrap();
}

/// The v2 fixture assets with `git.work_records` in the shipped default.
fn assets_shipping_work_records() -> (tempfile::TempDir, DirSource) {
    let (dir, _) = fixture_assets(true);
    let path = dir.path().join("base/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    policy["git"]["work_records"] = "block".into();
    std::fs::write(&path, serde_json::to_string_pretty(&policy).unwrap()).unwrap();
    let source = DirSource::new(dir.path());
    (dir, source)
}

#[test]
fn update_preserves_every_work_records_value_including_the_default() {
    isolate_git();
    for (value, expected) in [("block", "block"), ("warn", "warn"), ("off", "warn")] {
        let (_p, root) = project_dir();
        let _v1 = init_v1(&root);
        set_policy_key(&root, "work_records", value);
        set_policy_key(&root, "commit_format", "block");
        let (_a2, assets) = assets_shipping_work_records();
        let report = scaffold::update(&assets, &root, &update_opts("2.1.0")).unwrap();
        let after: serde_json::Value =
            serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
        assert_eq!(after["git"]["work_records"], expected, "from {value}");
        assert_eq!(
            after["git"]["commit_format"], "block",
            "a default-equal value is kept"
        );
        let notes = &report
            .files
            .iter()
            .find(|f| f.dest == ".codeflow/policy.json")
            .unwrap()
            .notes;
        assert_eq!(
            notes.iter().any(|n| n.contains("rewritten to `warn`")),
            value == "off",
            "{notes:?}"
        );
    }
}

/// TSK-133 AC-4: `codeflow update` adds `git.work_planning` at the shipped
/// default when the project has none, and keeps a value the project set,
/// the default included.
#[test]
fn update_adds_work_planning_and_keeps_an_explicit_level() {
    isolate_git();
    for (value, expected) in [
        (None, "block"),
        (Some("warn"), "warn"),
        (Some("block"), "block"),
    ] {
        let (_p, root) = project_dir();
        let _v1 = init_v1(&root);
        if let Some(value) = value {
            set_policy_key(&root, "work_planning", value);
        }
        let (dir, _) = fixture_assets(true);
        let path = dir.path().join("base/policy.json");
        let mut shipped: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        shipped["git"]["work_planning"] = "block".into();
        std::fs::write(&path, serde_json::to_string_pretty(&shipped).unwrap()).unwrap();
        let assets = DirSource::new(dir.path());
        scaffold::update(&assets, &root, &update_opts("2.1.0")).unwrap();
        let after: serde_json::Value =
            serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
        assert_eq!(after["git"]["work_planning"], expected, "from {value:?}");
    }
}

/// TSK-170 AC-5: `codeflow update` adds `git.conflict_markers` at the
/// shipped default, `block`, reports the added key, and keeps a level the
/// project set.
#[test]
fn update_adds_conflict_markers_and_reports_it() {
    isolate_git();
    let real: serde_json::Value =
        serde_json::from_str(include_str!("../../../assets/base/policy.json")).unwrap();
    assert_eq!(real["git"]["conflict_markers"], "block");
    for (value, expected) in [(None, "block"), (Some("warn"), "warn")] {
        let (_p, root) = project_dir();
        let _v1 = init_v1(&root);
        if let Some(value) = value {
            set_policy_key(&root, "conflict_markers", value);
        }
        let (dir, _) = fixture_assets(true);
        let path = dir.path().join("base/policy.json");
        let mut shipped: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        shipped["git"]["conflict_markers"] = real["git"]["conflict_markers"].clone();
        std::fs::write(&path, serde_json::to_string_pretty(&shipped).unwrap()).unwrap();
        let assets = DirSource::new(dir.path());
        let report = scaffold::update(&assets, &root, &update_opts("2.1.0")).unwrap();
        let after: serde_json::Value =
            serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
        assert_eq!(after["git"]["conflict_markers"], expected, "from {value:?}");
        let notes = &report
            .files
            .iter()
            .find(|f| f.dest == ".codeflow/policy.json")
            .unwrap()
            .notes;
        assert_eq!(
            notes
                .iter()
                .any(|n| n.contains("added key git.conflict_markers")),
            value.is_none(),
            "{notes:?}"
        );
    }
}

#[test]
fn update_records_the_work_records_baseline_once_for_existing_records() {
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);
    let (_a2, assets) = fixture_assets(true);
    scaffold::update(&assets, &root, &update_opts("2.1.0")).unwrap();
    assert!(
        !read(&root, ".codeflow/project.toml").contains("work_records_baseline"),
        "a project without records gets no baseline"
    );

    std::fs::create_dir_all(root.join("project-management/tasks")).unwrap();
    std::fs::write(
        root.join("project-management/tasks/TSK-001.md"),
        "---\nid: TSK-001\ntitle: t\nstatus: complete\nwork_type: feat\n---\n",
    )
    .unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "chore: add a record"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    let report = scaffold::update(&assets, &root, &update_opts("2.2.0")).unwrap();
    let state = read(&root, ".codeflow/project.toml");
    assert!(
        state.contains(&format!("work_records_baseline = \"{head}\"")),
        "{state}"
    );
    assert!(report
        .notes
        .iter()
        .any(|n| n.contains("work_records_baseline")));

    git(
        &root,
        &["commit", "-q", "--allow-empty", "-m", "chore: later"],
    );
    scaffold::update(&assets, &root, &update_opts("2.3.0")).unwrap();
    assert!(
        read(&root, ".codeflow/project.toml").contains(&head),
        "a recorded baseline is never moved"
    );
}

// --- product paths (SPC-013 R-71, R-114; TSK-104) ---------------------------

fn product_paths(root: &Path) -> serde_json::Value {
    let policy: serde_json::Value =
        serde_json::from_str(&read(root, ".codeflow/policy.json")).unwrap();
    policy["git"]["product_paths"].clone()
}

/// Remove `git.product_paths` from the policy and its shipped baseline, as a
/// project installed before the key existed has it.
fn drop_product_paths(root: &Path) {
    for path in [
        root.join(".codeflow/policy.json"),
        root.join(".codeflow/.baseline/.codeflow/policy.json"),
    ] {
        let mut policy: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        policy["git"]
            .as_object_mut()
            .unwrap()
            .remove("product_paths");
        std::fs::write(&path, serde_json::to_string_pretty(&policy).unwrap()).unwrap();
    }
}

#[test]
fn init_writes_the_product_paths_of_the_detected_stack() {
    isolate_git();
    for (marker, stack) in [
        (Some("Cargo.toml"), "rust"),
        (Some("package.json"), "node"),
        (Some("pyproject.toml"), "python"),
        (None, "unset"),
    ] {
        let (_p, root) = project_dir();
        if let Some(file) = marker {
            std::fs::write(root.join(file), "").unwrap();
        }
        let (_a, assets) = fixture_assets(false);
        scaffold::init(&assets, &root, &opts(None, "2.0.0")).unwrap();
        let expected: Vec<&str> =
            codeflow_core::workgraph::classify::stack_product_paths(stack).to_vec();
        assert_eq!(product_paths(&root), serde_json::json!(expected), "{stack}");
    }
}

#[test]
fn update_adds_product_paths_once_and_keeps_a_project_value() {
    isolate_git();
    let (_p, root) = project_dir();
    std::fs::write(root.join("Cargo.toml"), "").unwrap();
    let _v1 = init_v1(&root);
    drop_product_paths(&root);

    let (_a2, assets) = fixture_assets(true);
    let report = scaffold::update(&assets, &root, &update_opts("2.1.0")).unwrap();
    assert_eq!(
        product_paths(&root),
        serde_json::json!(["src/**", "crates/**", "build.rs"])
    );
    let notes = &report
        .files
        .iter()
        .find(|f| f.dest == ".codeflow/policy.json")
        .unwrap()
        .notes;
    assert!(
        notes.iter().any(|n| n.contains("git.product_paths")),
        "{notes:?}"
    );

    let mut policy: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    policy["git"]["product_paths"] = serde_json::json!(["engine/**"]);
    std::fs::write(
        root.join(".codeflow/policy.json"),
        serde_json::to_string_pretty(&policy).unwrap(),
    )
    .unwrap();
    scaffold::update(&assets, &root, &update_opts("2.2.0")).unwrap();
    assert_eq!(product_paths(&root), serde_json::json!(["engine/**"]));
}

// --- headless peer runs (TSK-136) --------------------------------------------

fn headless_level(root: &Path) -> serde_json::Value {
    let policy: serde_json::Value =
        serde_json::from_str(&read(root, ".codeflow/policy.json")).unwrap();
    policy["security"]["headless_peer_runs"].clone()
}

#[test]
fn update_adds_headless_peer_runs_at_warn_and_keeps_an_explicit_level() {
    isolate_git();
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);
    // Installed before the key existed; the next release ships it at warn,
    // as `assets/base/policy.json` does.
    assert!(headless_level(&root).is_null());
    let (a2, assets) = fixture_assets(true);
    let shipped = a2.path().join("base/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&shipped).unwrap()).unwrap();
    policy["security"] = serde_json::json!({"headless_peer_runs": "warn"});
    std::fs::write(&shipped, serde_json::to_string_pretty(&policy).unwrap()).unwrap();

    scaffold::update(&assets, &root, &update_opts("2.1.0")).unwrap();
    assert_eq!(headless_level(&root), "warn");

    for (version, level) in [("2.2.0", "block"), ("2.3.0", "off")] {
        let mut policy: serde_json::Value =
            serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
        policy["security"]["headless_peer_runs"] = level.into();
        std::fs::write(
            root.join(".codeflow/policy.json"),
            serde_json::to_string_pretty(&policy).unwrap(),
        )
        .unwrap();
        scaffold::update(&assets, &root, &update_opts(version)).unwrap();
        assert_eq!(headless_level(&root), level, "{version}");
    }
}

/// TSK-140 AC-14: `init` writes the release rule's adoption marker and no
/// transition table; `update` adds the marker to a project that lacks it,
/// keeps a value already written, and never writes a table.
#[test]
fn init_and_update_write_the_adoption_marker_and_never_a_table() {
    isolate_git();
    let tables = ["release_rule_baseline", "release_records_baseline"];
    let (_p, root) = project_dir();
    let _v1 = init_v1(&root);
    let state = read(&root, ".codeflow/project.toml");
    assert!(state.contains("release_rules = 1"), "init: {state}");
    for table in tables {
        assert!(!state.contains(table), "init wrote {table}: {state}");
    }

    // A project from before the rule: no marker until update brings it.
    let without = state.replace("release_rules = 1\n", "");
    std::fs::write(root.join(".codeflow/project.toml"), &without).unwrap();
    let (_a2, assets) = fixture_assets(true);
    scaffold::update(&assets, &root, &update_opts("2.1.0")).unwrap();
    let state = read(&root, ".codeflow/project.toml");
    assert!(state.contains("release_rules = 1"), "update: {state}");
    for table in tables {
        assert!(!state.contains(table), "update wrote {table}: {state}");
    }

    // Written once: update never rewrites a value it finds.
    std::fs::write(
        root.join(".codeflow/project.toml"),
        state.replace("release_rules = 1", "release_rules = 2"),
    )
    .unwrap();
    scaffold::update(&assets, &root, &update_opts("2.2.0")).unwrap();
    let state = read(&root, ".codeflow/project.toml");
    assert!(state.contains("release_rules = 2"), "kept: {state}");
}
