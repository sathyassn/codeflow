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

const PRESENT_SKILL_FILES: &[&str] = &[
    "SKILL.md",
    "references/document-authoring.md",
    "references/visual-craft.md",
    "resources/how-presentation-works.md",
    "resources/utility-presentation-system.md",
    "resources/present-document.example.json",
    "assets/review-document.example.json",
    "assets/config.example.toml",
    "assets/primitive-tokens.example.json",
    "agents/openai.yaml",
];
const PRESENT_SCHEMAS: &[&str] = &[
    "document-v1.schema.json",
    "session-history-v1.schema.json",
    "utility-tokens-v1.schema.json",
];

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

fn normalize_crlf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn write_user_owned_present_files(root: &Path) {
    for directory in [
        ".agents/skills/cf-present",
        ".claude/skills/cf-present",
        ".codeflow/present",
    ] {
        std::fs::create_dir_all(root.join(directory)).expect("create user-owned directory");
    }
    std::fs::write(
        root.join(".agents/skills/cf-present/LOCAL-NOTES.md"),
        "project-owned agent sidecar\n",
    )
    .expect("write agent sidecar");
    std::fs::write(
        root.join(".claude/skills/cf-present/LOCAL-NOTES.md"),
        "project-owned Claude sidecar\n",
    )
    .expect("write Claude sidecar");
    std::fs::write(
        root.join(".codeflow/present/config.toml"),
        "retention_days = 14\n",
    )
    .expect("write project-owned presentation config");
    std::fs::write(root.join("UNRELATED.txt"), "unrelated project file\n")
        .expect("write unrelated file");
}

fn assert_user_owned_present_files(root: &Path) {
    assert_eq!(
        read(root, ".agents/skills/cf-present/LOCAL-NOTES.md"),
        "project-owned agent sidecar\n"
    );
    assert_eq!(
        read(root, ".claude/skills/cf-present/LOCAL-NOTES.md"),
        "project-owned Claude sidecar\n"
    );
    assert_eq!(
        read(root, ".codeflow/present/config.toml"),
        "retention_days = 14\n"
    );
    assert_eq!(read(root, "UNRELATED.txt"), "unrelated project file\n");
}

fn assert_present_artifact_parity(root: &Path) {
    let repository = repo_root();
    for relative in PRESENT_SKILL_FILES {
        let source = repository
            .join("assets/base/agents/skills/cf-present")
            .join(relative);
        let canonical = std::fs::read(&source)
            .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
        for installed in [
            root.join(".agents/skills/cf-present").join(relative),
            root.join(".claude/skills/cf-present").join(relative),
            root.join(".codeflow/.baseline/.agents/skills/cf-present")
                .join(relative),
            root.join(".codeflow/.baseline/.claude/skills/cf-present")
                .join(relative),
        ] {
            assert_eq!(
                std::fs::read(&installed)
                    .unwrap_or_else(|error| panic!("read {}: {error}", installed.display())),
                canonical,
                "cf-present artifact drifted at {}",
                installed.display()
            );
        }
    }
    for schema in PRESENT_SCHEMAS {
        let source = repository.join("assets/base/present/schemas").join(schema);
        let canonical = std::fs::read(&source)
            .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
        for installed in [
            root.join(".codeflow/schemas/present").join(schema),
            root.join(".codeflow/.baseline/.codeflow/schemas/present")
                .join(schema),
        ] {
            assert_eq!(
                std::fs::read(&installed)
                    .unwrap_or_else(|error| panic!("read {}: {error}", installed.display())),
                canonical,
                "cf-present schema drifted at {}",
                installed.display()
            );
        }
    }
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
    let settings_path = root.join(".claude/settings.json");
    let mut settings: serde_json::Value =
        serde_json::from_str(&read(root, ".claude/settings.json")).unwrap();
    settings["env"] = serde_json::json!({
        "KEEP_PROJECT_VALUE": "opaque",
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "750000"
    });
    std::fs::write(
        &settings_path,
        format!("{}\n", serde_json::to_string_pretty(&settings).unwrap()),
    )
    .expect("write consumer-owned settings env");
    let settings_before = read(root, ".claude/settings.json");
    let selected_binding = r#"{
  "schema_version": 1,
  "bindings": [
    {
      "role": "claude-judgment-primary",
      "binding_id": "locally-qualified-claude-binding"
    }
  ]
}
"#;
    std::fs::write(
        root.join(".codeflow/model-selection.json"),
        selected_binding,
    )
    .expect("write user-owned model selection");
    write_user_owned_present_files(root);
    for attempt in 1..=2 {
        let out = codeflow(root, &["update"]);
        let report = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(
            out.status.success(),
            "update attempt {attempt} after init failed: {report}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !report.contains("CONFLICT"),
            "no-op update attempt {attempt} conflicted:\n{report}"
        );
    }
    assert_eq!(
        read(root, "AGENTS.md"),
        agents_before,
        "managed region did not round-trip"
    );
    assert_eq!(
        normalize_crlf(&read(root, ".claude/settings.json")),
        normalize_crlf(&settings_before),
        "settings merge did not preserve the consumer-owned env across repeated updates"
    );
    assert_eq!(
        read(root, ".codeflow/model-selection.json"),
        selected_binding,
        "update changed the user-owned model selection"
    );
    assert_user_owned_present_files(root);
    let mut after = Vec::new();
    walk_files(root, &mut after);
    assert!(
        !after
            .iter()
            .any(|p| p.extension().and_then(|e| e.to_str()) == Some("new")),
        "no-op update left .new conflict files"
    );
}

fn assert_present_cleanup_inventory_is_narrow(root: &Path) {
    let manifest = codeflow_core::scaffold::ScaffoldManifest::load(
        &codeflow_core::scaffold::DirSource::new(repo_root().join("assets")),
    )
    .expect("shipped manifest loads");
    let mut managed = manifest
        .entries
        .iter()
        .filter(|entry| {
            entry.src.starts_with("agents/skills/cf-present/")
                || entry.src.starts_with("present/schemas/")
        })
        .map(|entry| entry.dest.as_str())
        .collect::<Vec<_>>();
    managed.sort_unstable();
    managed.dedup();
    assert_eq!(
        managed.len(),
        PRESENT_SKILL_FILES.len() * 2 + PRESENT_SCHEMAS.len()
    );
    for destination in &managed {
        std::fs::remove_file(root.join(destination))
            .unwrap_or_else(|error| panic!("remove managed {destination}: {error}"));
    }
    assert!(managed
        .iter()
        .all(|destination| !root.join(destination).exists()));
    assert_user_owned_present_files(root);
}

/// TSK-137: the inert `human_authorization` key is gone from the scaffolded
/// policy, which validates without a deprecation warning.
fn assert_policy_has_no_human_authorization(root: &Path) {
    assert!(
        !read(root, ".codeflow/policy.json").contains("human_authorization"),
        "fresh policy still carries human_authorization"
    );
    let validate = codeflow(root, &["validate"]);
    assert!(
        validate.status.success(),
        "validate failed on a fresh policy"
    );
    assert!(
        !String::from_utf8_lossy(&validate.stderr).contains("deprecated"),
        "a fresh policy printed a deprecation"
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
    assert_policy_has_no_human_authorization(&root);
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

    // TSK-132: the shipped push set blocks by default.
    let policy: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    assert_eq!(policy["git"]["test_gate_on_push"], "block");

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
    assert_present_artifact_parity(&root);
    assert_update_round_trips(&root);
    assert_present_artifact_parity(&root);
    assert_present_cleanup_inventory_is_narrow(&root);
}

#[test]
fn init_full_tier_preserves_representative_brownfield_present_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("brownfield");
    std::fs::create_dir(&root).unwrap();
    let init = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&root)
        .output()
        .expect("git init runs");
    assert!(init.status.success());
    write_user_owned_present_files(&root);

    let out = codeflow(&root, &["init", "--yes", "--full"]);
    assert!(
        out.status.success(),
        "brownfield init failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains("CONFLICT"));
    assert_user_owned_present_files(&root);
    assert_present_artifact_parity(&root);
    assert_update_round_trips(&root);
    assert_present_artifact_parity(&root);
}

/// TSK-134: in a freshly scaffolded project at every tier, `codeflow test`
/// warns about a `CARGO_TARGET_DIR` outside the worktree and refuses a full
/// gate while another holds the gate lock.
#[test]
fn fresh_scaffolds_apply_the_gate_lock_and_target_check_at_every_tier() {
    use codeflow_core::testing::gate_guard::{acquire_full_gate_lock, lock_dirs};

    for tier in ["--minimal", "--standard", "--full"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let init = codeflow(&root, &["init", "--yes", tier]);
        assert!(init.status.success(), "{tier}: init failed");
        std::fs::write(
            root.join(".codeflow/test-config.json"),
            r#"{"schema_version": "1.0", "targets": [
  {"name": "rust", "runner": "custom", "modes": {"full": {"command": "cargo --version"}}}]}"#,
        )
        .unwrap();
        let home = tmp.path().join("home");
        let gate = |target_dir: Option<&Path>| {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
            cmd.arg("test")
                .current_dir(&root)
                .env("CODEFLOW_HOME", &home)
                .env_remove("CARGO_TARGET_DIR");
            if let Some(dir) = target_dir {
                cmd.env("CARGO_TARGET_DIR", dir);
            }
            cmd.output().expect("codeflow test runs")
        };

        let outside = tmp.path().join("shared-target");
        let warned = gate(Some(&outside));
        let err = String::from_utf8_lossy(&warned.stderr).to_string();
        assert_eq!(warned.status.code(), Some(0), "{tier}: {err}");
        assert!(err.contains("warning: CARGO_TARGET_DIR="), "{tier}: {err}");

        let held = acquire_full_gate_lock(&lock_dirs(&root, Some(&home)), &root).unwrap();
        let locked = gate(None);
        let err = String::from_utf8_lossy(&locked.stderr).to_string();
        assert_eq!(locked.status.code(), Some(1), "{tier}: {err}");
        assert!(
            err.contains("another full gate is running"),
            "{tier}: {err}"
        );
        drop(held);

        // A child forked by a sibling test can hold the released descriptor
        // until its exec; allow that short window.
        let passed = (0..40).any(|_| {
            let out = gate(Some(&root.join("target")));
            out.status.success() || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                false
            }
        });
        assert!(passed, "{tier}: the gate runs once the lock is free");
    }
}
