//! End-to-end scaffold test against the REAL embedded asset tree: runs the
//! real binary's `init --yes --full` in a tempdir and asserts the shipped
//! templates actually render — no skipped or missing artifacts, managed
//! regions that round-trip through `update`, and no engine-substituted
//! placeholder surviving in any installed file. The rest of the scaffold
//! suite exercises the engine against tempdir fixtures; only this test
//! catches a defect in the shipped templates themselves (lost managed-region
//! markers, a malformed placeholder, a tier/preset mistake in the manifest).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Shared isolated `CODEFLOW_HOME` so the suite never writes the developer's
/// real `~/.codeflow/registry.json` (per the `recall_remote_cli.rs` pattern).
fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn codeflow(dir: &Path, args: &[&str]) -> Output {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    // The scaffolded git-hook shims exec `codeflow` from PATH; point them at
    // the binary under test, not whatever codeflow the developer has installed.
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
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .output()
        .expect("codeflow binary runs")
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Every regular file under `dir`, recursively, skipping `.git/`.
fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read_dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) != Some(".git") {
                walk_files(&path, out);
            }
        } else {
            out.push(path);
        }
    }
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// Placeholders: everything the engine substitutes must be gone. Scoped to
/// the manifest's `template = true` dests: non-templated files (pm templates,
/// docs/decisions/template.md) carry `{{DATE}}`-style fill slots for later
/// tooling by design, and unknown placeholders inside rendered files (e.g.
/// `{{PRODUCT_PURPOSE}}` in the docs seeds) are human-fill slots — but the
/// engine-provided keys must never survive in a rendered file.
fn assert_engine_placeholders_rendered(root: &Path) {
    let engine_keys = [
        "PROJECT_NAME",
        "PROJECT_ONE_LINER",
        "AREAS",
        "STACK",
        "TIER",
        "SCAFFOLD_VERSION",
        "DATE",
    ];
    let assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let manifest = codeflow_core::scaffold::ScaffoldManifest::load(
        &codeflow_core::scaffold::DirSource::new(&assets_dir),
    )
    .expect("shipped manifest loads");
    let mut rendered_dests: Vec<&str> = manifest
        .entries
        .iter()
        .filter(|e| e.template)
        .map(|e| e.dest.as_str())
        .collect();
    rendered_dests.sort_unstable();
    rendered_dests.dedup();
    assert!(
        !rendered_dests.is_empty(),
        "expected templated entries in the shipped manifest"
    );
    for dest in rendered_dests {
        let text = read(root, dest);
        for key in engine_keys {
            assert!(
                !text.contains(&format!("{{{{{key}}}}}")),
                "{dest}: engine placeholder {{{{{key}}}}} survived rendering"
            );
        }
    }

    // The operating contract is fully rendered — it carries no fill slots, so
    // ANY surviving {{…}} in it is a template defect.
    for contract in ["AGENTS.md", "CLAUDE.md"] {
        let text = read(root, contract);
        let survivors: Vec<&str> = text.lines().filter(|line| line.contains("{{")).collect();
        assert!(
            survivors.is_empty(),
            "{contract} still contains {{{{…}}}} placeholders after init:\n  {}",
            survivors.join("\n  ")
        );
    }
}

/// Update round-trip: same version, same assets → no-op, no conflicts, and
/// the managed regions regenerate to byte-identical content.
fn assert_update_round_trips(root: &Path) {
    let agents_before = read(root, "AGENTS.md");
    let settings_before = read(root, ".claude/settings.json");
    let out = codeflow(root, &["update"]);
    let report = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "update after fresh init failed: {report}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !report.contains("CONFLICT"),
        "no-op update conflicted:\n{report}"
    );
    assert_eq!(
        read(root, "AGENTS.md"),
        agents_before,
        "managed region did not round-trip"
    );
    assert_eq!(
        read(root, ".claude/settings.json"),
        settings_before,
        "settings merge did not round-trip"
    );
    let mut after = Vec::new();
    walk_files(root, &mut after);
    assert!(
        !after
            .iter()
            .any(|p| p.extension().and_then(|e| e.to_str()) == Some("new")),
        "no-op update left .new conflict files"
    );
}

#[test]
fn init_full_tier_renders_the_real_asset_tree_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();

    // Init: every real manifest entry installs, nothing skipped/missing.
    let out = codeflow(&root, &["init", "--yes", "--full"]);
    let report = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "init failed: {report}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !report.contains("skipped"),
        "fresh full init skipped files:\n{report}"
    );
    assert!(
        !report.contains("missing-asset"),
        "a manifest src is missing from the shipped tree:\n{report}"
    );
    assert!(
        !report.contains("CONFLICT"),
        "fresh init conflicted:\n{report}"
    );
    assert!(
        report.contains("/cf-customize")
            && report.contains("docs/product.md")
            && report.contains("docs/architecture.md"),
        "full init must print the consuming-project customization next step:\n{report}"
    );

    // Bootstrap grace committed a clean tree (real templates, real hooks).
    assert!(
        git_stdout(&root, &["status", "--porcelain"]).is_empty(),
        "working tree not clean after fresh init"
    );

    // Managed-region markers made it through rendering.
    let agents = read(&root, "AGENTS.md");
    assert!(
        agents.contains("codeflow:managed:begin"),
        "AGENTS.md lost its begin marker"
    );
    assert!(
        agents.contains("codeflow:managed:end"),
        "AGENTS.md lost its end marker"
    );
    let eval_script = root.join(".agents/skills/cf-evaluate-model/scripts/eval_kit.py");
    assert!(eval_script.is_file(), "full init omitted cf-evaluate-model");
    let suite = Command::new("python3")
        .arg("-B")
        .arg(&eval_script)
        .arg("validate-suite")
        .arg("--project-root")
        .arg(&root)
        .current_dir(&root)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("installed model-evaluation suite runs");
    assert!(
        suite.status.success(),
        "installed model-evaluation suite is invalid:\n{}\n{}",
        String::from_utf8_lossy(&suite.stdout),
        String::from_utf8_lossy(&suite.stderr)
    );

    let product = read(&root, "docs/product.md");
    assert!(
        product.matches("proj").count() >= 2,
        "product seed must include both the project heading and init one-liner"
    );
    assert!(
        read(&root, "docs/architecture.md").contains("Initial areas recorded by init: `core`."),
        "architecture seed must anchor the areas collected by init"
    );

    let claude: serde_json::Value =
        serde_json::from_str(&read(&root, ".claude/settings.json")).unwrap();
    assert_eq!(claude["sandbox"]["enabled"], true);
    assert_eq!(claude["sandbox"]["failIfUnavailable"], true);
    assert_eq!(claude["sandbox"]["allowUnsandboxedCommands"], true);
    assert!(claude["permissions"]["allow"]
        .as_array()
        .is_some_and(|entries| entries.iter().any(|entry| entry == "WebFetch")));

    let codex = read(&root, ".codex/config.toml");
    assert!(!codex.contains("sandbox_mode"));
    assert!(codex.contains("approvals_reviewer = \"auto_review\""));
    assert!(codex.contains("model_reasoning_effort = \"high\""));
    assert!(codex.contains("web_search = \"live\""));

    assert_engine_placeholders_rendered(&root);
    assert_update_round_trips(&root);
}
