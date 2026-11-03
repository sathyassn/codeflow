//! TSK-401: project additions survive updates from the actual 3.1.1 inputs.
//! The reduced manifest isolates the four affected file kinds. The installed
//! 3.1.1 binary journey remains separate acceptance evidence.

use std::path::Path;

use codeflow_core::scaffold::{self, DirSource, InitAnswers, InitOptions, Tier, UpdateOptions};

#[path = "support/git_isolation.rs"]
mod git_isolation;

const MANIFEST: &str = r#"
schema_version = 1

[[entry]]
src = "policy.json"
dest = ".codeflow/policy.json"
ownership = "user-owned"
tiers = ["full"]

[[entry]]
src = "settings/default.json"
dest = ".claude/settings.json"
ownership = "managed-region"
region = "json"
preset = "default"
tiers = ["full"]

[[entry]]
src = "codex/rules/codeflow.rules"
dest = ".codex/rules/codeflow.rules"
ownership = "managed"
tiers = ["full"]
"#;

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn prior_assets() -> (tempfile::TempDir, DirSource) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base");
    write(&base, "scaffold-manifest.toml", MANIFEST);
    for (name, destination) in [
        ("policy.json", "policy.json"),
        ("settings.json", "settings/default.json"),
        ("codeflow.rules", "codex/rules/codeflow.rules"),
    ] {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/enforcement-3.1.1")
            .join(name);
        write(&base, destination, &std::fs::read_to_string(fixture).unwrap());
    }
    let source = DirSource::new(dir.path());
    (dir, source)
}

fn project(source: &DirSource) -> tempfile::TempDir {
    git_isolation::isolate_git();
    let dir = tempfile::tempdir().unwrap();
    git_isolation::own_repository(dir.path());
    scaffold::init(
        source,
        dir.path(),
        &InitOptions {
            tier: Some(Tier::Full),
            force: false,
            binary_version: "3.1.1".into(),
            answers: InitAnswers::default(),
        },
    )
    .unwrap();
    dir
}

fn update(source: &DirSource, root: &Path) {
    scaffold::update(
        source,
        root,
        &UpdateOptions {
            force: false,
            binary_version: "4.0.0".into(),
            diff_out: None,
        },
    )
    .unwrap();
}

fn survives(relative: &str, patch: impl FnOnce(&str) -> (String, String)) {
    let (_assets, source) = prior_assets();
    let project = project(&source);
    let path = project.path().join(relative);
    let before = std::fs::read_to_string(&path).unwrap();
    let (patched, addition) = patch(&before);
    assert_ne!(before, patched, "fixture patch must add project content");
    std::fs::write(&path, &patched).unwrap();

    for _ in 0..2 {
        update(&source, project.path());
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains(&addition),
            "{relative}: update must preserve the exact project addition {addition:?}"
        );
        assert!(
            !project.path().join(format!("{relative}.new")).exists(),
            "{relative}: unchanged shipped content must not cause a conflict"
        );
    }
}

#[test]
fn policy_addition_survives_unchanged_shipped_content() {
    survives(".codeflow/policy.json", |before| {
        let addition = "\"blocked_names\" : [ \"project-private\" ],";
        (
            before.replacen("\"git\": {", &format!("\"git\": {{\n    {addition}"), 1),
            addition.into(),
        )
    });
}

#[test]
fn claude_deny_addition_survives_unchanged_shipped_content() {
    survives(".claude/settings.json", |before| {
        // Whitespace and the equivalent JSON escape belong to the person too.
        let addition = "\t\t\"Read(project-private/\\u002a\\u002a)\" ,";
        (
            before.replacen("\"deny\": [", &format!("\"deny\": [\n{addition}"), 1),
            addition.into(),
        )
    });
}

#[test]
fn codex_prefix_rule_addition_survives_unchanged_shipped_content() {
    survives(".codex/rules/codeflow.rules", |before| {
        let addition = "\n# Project deployment needs a person.\nprefix_rule(pattern = [\"project-deploy\"], decision = \"forbidden\")\n";
        (format!("{before}{addition}"), addition.into())
    });
}

#[test]
fn project_toml_addition_survives_unchanged_shipped_content() {
    survives(".codeflow/project.toml", |before| {
        let addition = "\n# Project-local extension.\n[project_tools]\nname = 'project-check' # Keep the chosen spelling.\n";
        (format!("{before}{addition}"), addition.into())
    });
}
