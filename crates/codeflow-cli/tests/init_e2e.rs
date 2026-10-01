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
    "references/feedback-loop.md",
    "references/visual-craft.md",
    "resources/explanation-method.md",
    "resources/how-presentation-works.md",
    "resources/utility-presentation-system.md",
    "resources/figure-grammar.md",
    "resources/figure-grammar-specimens.md",
    "resources/present-document.example.json",
    "resources/design-system/README.md",
    "resources/design-system/tokens.json",
    "resources/design-system/tokens.css",
    "resources/design-system/chrome.css",
    "resources/design-system/figure.css",
    "resources/design-system/chrome.js",
    "resources/design-system/portal.reference.html",
    "resources/design-system/present.reference.html",
    "assets/review-document.example.json",
    "assets/config.example.toml",
    "assets/primitive-tokens.example.json",
    "agents/openai.yaml",
];
const PRESENT_SCHEMAS: &[&str] = &[
    "document-v1.schema.json",
    "document-v2.schema.json",
    "session-history-v1.schema.json",
    "session-history-v2.schema.json",
    "session-responses-v1.schema.json",
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
        // `task new` commits to the ID registry; with no global config, git
        // would otherwise take the identity from the account's passwd entry.
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
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
    assert_design_kit_matches_in_docs_portal(root);
}

/// The design system kit ships byte-identical in both presentation skills, so
/// the docs-portal copies must equal the cf-present source too.
fn assert_design_kit_matches_in_docs_portal(root: &Path) {
    let repository = repo_root();
    let kit_files = PRESENT_SKILL_FILES
        .iter()
        .filter(|relative| relative.starts_with("resources/design-system/"));
    let mut checked = 0;
    for relative in kit_files {
        let canonical = std::fs::read(
            repository
                .join("assets/base/agents/skills/cf-present")
                .join(relative),
        )
        .unwrap_or_else(|error| panic!("read kit source {relative}: {error}"));
        for installed in [
            ".agents/skills/cf-docs-portal",
            ".claude/skills/cf-docs-portal",
            ".codeflow/.baseline/.agents/skills/cf-docs-portal",
            ".codeflow/.baseline/.claude/skills/cf-docs-portal",
        ] {
            let path = root.join(installed).join(relative);
            assert_eq!(
                std::fs::read(&path)
                    .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
                canonical,
                "design kit drifted at {}",
                path.display()
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 8, "the design kit is eight files");
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

/// TSK-085: the scaffolded catalog is the managed schema 5 roster and the
/// model-bindings check accepts it after an update. A designated roster
/// without full-suite records is advisory (WARN), never a failure.
fn assert_model_bindings_after_update(root: &Path) {
    let out = codeflow(root, &["update"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let managed = read(
        &repo_root(),
        "assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json",
    );
    for prefix in [".agents", ".claude"] {
        assert_eq!(
            read(
                root,
                &format!("{prefix}/skills/cf-model-orchestrator/resources/current-ensemble.json")
            ),
            managed,
            "{prefix} catalog differs from the managed asset"
        );
    }
    let out = codeflow(root, &["doctor", "--check", "model-bindings"]);
    let report = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "doctor model-bindings failed: {report}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!report.contains("FAIL"), "{report}");
    assert!(report.contains("illustrative: context-free"), "{report}");
    assert!(report.contains("doctor did not launch a model"), "{report}");
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
    assert_model_bindings_after_update(&root);
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

/// TSK-127: a fresh scaffold at every tier installs the kernel-rendered rule
/// map, its references and the CLAUDE file through the real binary, and
/// `codeflow doctor` warns once an AGENTS.md passes Codex's 32 KiB limit.
#[test]
fn fresh_scaffolds_install_the_rule_map_at_every_tier() {
    use codeflow_core::scaffold::rule_map::{managed_block, File, Kernel, OUTPUTS};
    use codeflow_core::scaffold::Tier;

    let kernel = Kernel::shipped();
    for (flag, tier) in [
        ("--minimal", Tier::Minimal),
        ("--standard", Tier::Standard),
        ("--full", Tier::Full),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let init = codeflow(&root, &["init", "--yes", flag]);
        assert!(
            init.status.success(),
            "{flag}: init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );

        let agents = read(&root, "AGENTS.md");
        let output = OUTPUTS
            .iter()
            .find(|output| output.file == File::Agents && output.tier == tier)
            .unwrap();
        let rendered = kernel
            .render(output)
            .replace("{{SCAFFOLD_VERSION}}", env!("CARGO_PKG_VERSION"));
        assert_eq!(
            managed_block(&agents),
            managed_block(&rendered),
            "{flag}: installed map is not the kernel render"
        );
        for rule in kernel.rules_for(tier) {
            assert!(
                agents.contains(&kernel.render_rule_at(tier, rule)),
                "{flag}: rule {} missing",
                rule.id
            );
        }
        for reference in [
            "workflow-discipline.md",
            "git-rules.md",
            "worktrees.md",
            "writing.md",
        ] {
            assert_eq!(
                read(&root, &format!(".codeflow/rules/{reference}")),
                read(&repo_root(), &format!("assets/base/rules/{reference}")).replace(
                    "{{WORKSPACE_ROOT_BRANCH}}",
                    codeflow_core::root_checkout::WORKSPACE_ROOT_BRANCH
                ),
                "{flag}: reference {reference} not installed"
            );
        }
        assert!(
            read(&root, "CLAUDE.md").contains("@AGENTS.md"),
            "{flag}: CLAUDE.md"
        );

        let doctor = |label: &str| {
            let out = codeflow(&root, &["doctor", "--check", "instructions"]);
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(text.contains("instructions"), "{flag} {label}: {text}");
            text
        };
        let fresh = doctor("fresh");
        assert!(!fresh.contains("over Codex"), "{flag}: {fresh}");

        let mut grown = agents.clone();
        grown.push_str("\n## Service conventions\n\n");
        grown.push_str(
            &"- Keep the ledger double-entry invariant on every change.\n".repeat(16 * 1024 / 58),
        );
        std::fs::write(root.join("AGENTS.md"), &grown).unwrap();
        // TSK-150: the one byte failure left, the complete document against
        // Codex's instruction limit.
        assert_eq!(
            codeflow_core::scaffold::rule_map::codex_overflow(&grown),
            None,
            "{flag}: {} bytes",
            grown.len()
        );
        let sixteen = doctor("16 KiB section");
        assert!(!sixteen.contains("over Codex"), "{flag}: {sixteen}");

        grown.push_str(&"x".repeat(32 * 1024));
        std::fs::write(root.join("AGENTS.md"), &grown).unwrap();
        let over = doctor("oversized");
        assert!(over.contains("over Codex"), "{flag}: {over}");
    }
}

/// TSK-127 review probe (Codex F1): a nested `AGENTS.md` whose chain passes
/// Codex's 32 KiB limit warns through the real binary, whether doctor runs
/// from the project root or from the nested directory.
#[test]
fn doctor_warns_for_an_oversized_nested_instruction_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let init = codeflow(&root, &["init", "--yes", "--minimal"]);
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    let nested = root.join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join("AGENTS.md"), "x".repeat(32 * 1024 + 1)).unwrap();

    for cwd in [&root, &nested] {
        let out = codeflow(cwd, &["doctor", "--check", "instructions"]);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            text.contains("nested/AGENTS.md with its parent instructions")
                && text.contains("over Codex"),
            "{}: {text}",
            cwd.display()
        );
    }
}

/// `git` with the binary under test first on `PATH`, so the scaffolded hook
/// shims run it.
fn git_with_binary(dir: &Path, args: &[&str]) {
    let out = git_output_with_binary(dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// [`git_with_binary`], returning the output (hook messages go to stderr).
fn git_output_with_binary(dir: &Path, args: &[&str]) -> Output {
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
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs")
}

/// TSK-133 AC-4 (journey): a freshly scaffolded full-tier project ships
/// `git.work_planning = block`; a work branch carrying an unplanned task id
/// is refused once by `work start` and once by `codeflow ci`, and pre-commit
/// lets the commit through. The adopter sets `warn`: both report and pass,
/// and `codeflow update` keeps the value.
#[test]
fn a_fresh_full_tier_project_checks_planning_once_at_the_policy_level() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let init = codeflow(&root, &["init", "--yes", "--full"]);
    assert!(init.status.success(), "init --full failed");
    let policy_path = root.join(".codeflow/policy.json");
    let policy: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    assert_eq!(policy["git"]["work_planning"], "block");

    let target = git_stdout(&root, &["branch", "--show-current"]);
    let target = target.trim();
    let branch = "fix/TSK-404-unplanned";
    git_with_binary(&root, &["switch", "-q", "-c", branch]);
    std::fs::write(root.join("fix.txt"), "a fix\n").unwrap();
    git_with_binary(&root, &["add", "fix.txt"]);
    git_with_binary(&root, &["commit", "-q", "-m", "fix: repair the thing"]);

    let ci = |root: &Path| {
        codeflow(
            root,
            &["ci", "--base", target, "--head", "HEAD", "--branch", branch],
        )
    };
    let text = |out: &Output| {
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };
    let blocked = ci(&root);
    assert_eq!(blocked.status.code(), Some(1), "{}", text(&blocked));
    assert!(text(&blocked).contains("(block)"), "{}", text(&blocked));
    assert!(
        text(&blocked).contains("level work_planning = block (configured)"),
        "{}",
        text(&blocked)
    );
    let start = codeflow(&root, &["work", "start", "TSK-404"]);
    assert_eq!(start.status.code(), Some(1), "{}", text(&start));
    assert!(
        text(&start).contains("work start: error:"),
        "{}",
        text(&start)
    );

    let mut policy = policy;
    policy["git"]["work_planning"] = "warn".into();
    std::fs::write(
        &policy_path,
        format!("{}\n", serde_json::to_string_pretty(&policy).unwrap()),
    )
    .unwrap();
    git_with_binary(&root, &["add", ".codeflow/policy.json"]);
    git_with_binary(
        &root,
        &[
            "commit",
            "-q",
            "-m",
            "chore: report planning findings as warnings",
        ],
    );
    let warned = ci(&root);
    assert_eq!(warned.status.code(), Some(0), "{}", text(&warned));
    assert!(
        text(&warned).contains("level work_planning = warn (configured)")
            && text(&warned).contains("(warn)"),
        "{}",
        text(&warned)
    );
    let start = codeflow(&root, &["work", "start", "TSK-404"]);
    assert_eq!(start.status.code(), Some(0), "{}", text(&start));
    assert!(
        text(&start).contains("work start: warning:")
            && text(&start).contains("git.work_planning is warn"),
        "{}",
        text(&start)
    );

    let update = codeflow(&root, &["update"]);
    assert!(update.status.success(), "{}", text(&update));
    let after: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    assert_eq!(after["git"]["work_planning"], "warn");
}

/// TSK-129 AC-5 (serves EPC-020 AC-13): the whole files that TSK-129 split,
/// with the section directories that now hold their text.
const TSK129_SPLIT_FILES: &[(&str, &str)] = &[
    (
        "skills/cf-model-orchestrator/resources/quality-contract.md",
        "skills/cf-model-orchestrator/resources/quality",
    ),
    (
        "skills/cf-model-orchestrator/resources/capability-routing.md",
        "skills/cf-model-orchestrator/resources/routing",
    ),
    (
        "skills/cf-delegate/SKILL.md",
        "skills/cf-delegate/resources",
    ),
    (
        "skills/cf-model-orchestrator/SKILL.md",
        "skills/cf-model-orchestrator/references",
    ),
];

/// Files the split added beside each whole file, in one skill tree.
fn tsk129_added_files(root: &Path, tree: &str) -> Vec<String> {
    let mut added = Vec::new();
    for (_, dir) in TSK129_SPLIT_FILES {
        let mut files = Vec::new();
        walk_files(&root.join(tree).join(dir), &mut files);
        for file in files {
            let rel = file
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let name = rel.rsplit('/').next().unwrap();
            if !dir.ends_with("resources") || name.starts_with("lane-") || name == "edit-access.md"
            {
                added.push(rel);
            }
        }
    }
    added.sort();
    added
}

fn skill_tree_snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    for tree in [".claude/skills", ".agents/skills"] {
        walk_files(&root.join(tree), &mut files);
    }
    let mut snapshot: Vec<_> = files
        .into_iter()
        .map(|f| {
            let rel = f
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel, std::fs::read(&f).unwrap())
        })
        .collect();
    snapshot.sort();
    snapshot
}

/// Rewind a fresh scaffold to the pre-split install: each split file becomes
/// one whole managed file (its index or core followed by every added file),
/// recorded as installed in the manifest and baseline, and the added files
/// are removed with their records.
fn rewind_to_whole_files(root: &Path) {
    let manifest_path = root.join(".codeflow/manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&read(root, ".codeflow/manifest.json")).unwrap();
    let files = manifest["files"].as_object_mut().expect("manifest files");
    for tree in [".claude", ".agents"] {
        let added = tsk129_added_files(root, tree);
        for (whole, dir) in TSK129_SPLIT_FILES {
            let whole = format!("{tree}/{whole}");
            let mut text = read(root, &whole);
            for rel in added
                .iter()
                .filter(|rel| rel.starts_with(&format!("{tree}/{dir}/")))
            {
                text.push('\n');
                text.push_str(&read(root, rel));
            }
            std::fs::write(root.join(&whole), &text).unwrap();
            std::fs::write(root.join(".codeflow/.baseline").join(&whole), &text).unwrap();
            files.get_mut(&whole).expect("whole file record")["sha256"] =
                serde_json::json!(codeflow_core::scaffold::sha256_hex(text.as_bytes()));
        }
        for rel in &added {
            std::fs::remove_file(root.join(rel)).unwrap();
            let _ = std::fs::remove_file(root.join(".codeflow/.baseline").join(rel));
            assert!(files.remove(rel).is_some(), "{rel} had no manifest record");
        }
    }
    std::fs::write(
        &manifest_path,
        format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()),
    )
    .unwrap();
}

#[test]
fn split_references_install_and_update_replaces_whole_files_at_standard_and_full() {
    for tier in ["--standard", "--full"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let init = codeflow(&root, &["init", "--yes", tier]);
        assert!(
            init.status.success(),
            "{tier}: init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );

        // Installed: every split file and its added files, in both trees,
        // byte-identical to the shipped assets.
        let assets = repo_root().join("assets/base");
        for tree in [".claude", ".agents"] {
            let added = tsk129_added_files(&root, tree);
            // TSK-184: five routing sections and the other-hosts reference
            // merged into the orchestrator's seat section and plan section.
            assert!(
                added.len() >= 3 + 16 + 3 + 5,
                "{tier} {tree}: split files missing: {added:?}"
            );
            for rel in added.iter().cloned().chain(
                TSK129_SPLIT_FILES
                    .iter()
                    .map(|(w, _)| format!("{tree}/{w}")),
            ) {
                let skill_rel = rel.split_once("/skills/").unwrap().1;
                let src = ["agents/skills", "claude/skills"]
                    .iter()
                    .map(|s| assets.join(s).join(skill_rel))
                    .find(|p| p.is_file())
                    .unwrap_or_else(|| panic!("{rel} has no shipped source"));
                assert_eq!(
                    normalize_crlf(&read(&root, &rel)),
                    normalize_crlf(&std::fs::read_to_string(src).unwrap()),
                    "{tier}: {rel} differs from its asset"
                );
            }
        }
        let fresh = skill_tree_snapshot(&root);

        rewind_to_whole_files(&root);
        let out = codeflow(&root, &["update"]);
        let report = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(
            out.status.success(),
            "{tier}: update failed: {report}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !report.contains("CONFLICT"),
            "{tier}: update conflicted:\n{report}"
        );
        // No stale copies: the skill trees match a fresh scaffold exactly, and
        // no conflict or backup file is left anywhere.
        assert_eq!(
            skill_tree_snapshot(&root),
            fresh,
            "{tier}: update left the skill trees different from a fresh scaffold"
        );
        let mut all = Vec::new();
        walk_files(&root, &mut all);
        let stale: Vec<_> = all
            .iter()
            .filter(|p| {
                let name = p.file_name().unwrap().to_string_lossy();
                name.ends_with(".new") || name.ends_with(".orig") || name.ends_with(".bak")
            })
            .collect();
        assert!(stale.is_empty(), "{tier}: stale copies left: {stale:?}");
        let again = codeflow(&root, &["update"]);
        let again_report = String::from_utf8_lossy(&again.stdout).to_string();
        assert!(
            again.status.success() && !again_report.contains("CONFLICT"),
            "{tier}: second update was not clean: {again_report}"
        );
    }
}

fn output_text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// TSK-135 AC-4 (journey): in a freshly scaffolded full-tier project the
/// shipped templates and the checks agree. The PR template names the light
/// class, and a docs-only pull request with only Summary and Changes passes
/// `codeflow ci` with no Release impact. The spec template carries the
/// `open_questions` list, and `spec status approved` reads it: a listed
/// question is refused, an empty list approves.
#[test]
fn a_fresh_full_tier_project_scales_checks_to_the_change_class() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let init = codeflow(&root, &["init", "--yes", "--full"]);
    assert!(init.status.success(), "{}", output_text(&init));
    let template = read(&root, ".github/pull_request_template.md");
    assert!(
        template.contains("A range of only Markdown under")
            && template.contains("needs Summary and Changes"),
        "{template}"
    );
    assert!(read(&root, "project-management/templates/spec.md").contains("open_questions: []"));

    // A docs-only pull request: Summary and Changes suffice.
    let target = git_stdout(&root, &["branch", "--show-current"]);
    let target = target.trim().to_string();
    git_with_binary(&root, &["branch", "integration/guide", &target]);
    let target = "integration/guide".to_string();
    git_with_binary(&root, &["switch", "-q", "-c", "docs/guide"]);
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/guide.md"), "# Guide\n\nHow to start.\n").unwrap();
    git_with_binary(&root, &["add", "docs/guide.md"]);
    git_with_binary(&root, &["commit", "-q", "-m", "docs: add a starting guide"]);
    let allocated = codeflow(
        &root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "bounded guide",
            "--into",
            &target,
            "guide",
        ],
    );
    assert!(allocated.status.success(), "{}", output_text(&allocated));
    let task_path = root.join("project-management/tasks/TSK-001.md");
    let task = std::fs::read_to_string(&task_path).unwrap().replace(
        "- AC-1\n",
        "- AC-1 When read, the guide shall explain setup.\n",
    );
    std::fs::write(&task_path, task).unwrap();
    git_with_binary(&root, &["branch", "-m", "task/TSK-001-guide"]);
    git_with_binary(&root, &["add", "project-management/tasks/TSK-001.md"]);
    git_with_binary(&root, &["commit", "-q", "-m", "docs: record guide task"]);
    let body = "Task: TSK-001\n\n## Summary\n\nAdds a starting guide.\n\n\
                ## Changes\n\n- a guide for new readers\n";
    let ci = codeflow(
        &root,
        &[
            "ci",
            "--base",
            &target,
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-guide",
            "--pr-body",
            body,
        ],
    );
    let stderr = String::from_utf8_lossy(&ci.stderr);
    assert_eq!(ci.status.code(), Some(0), "{}", output_text(&ci));
    assert!(
        !stderr.contains("git.pr_sections") && !stderr.contains("git.pr_release_impact"),
        "{stderr}"
    );

    // A spec lists its open questions; approval reads the list.
    git_with_binary(&root, &["switch", "-q", &target]);
    git_with_binary(&root, &["switch", "-q", "-c", "plan/contract"]);
    let epic = codeflow(&root, &["epic", "new", "outcome"]);
    assert!(epic.status.success(), "{}", output_text(&epic));
    let spec = codeflow(&root, &["spec", "new", "--for", "EPC-001", "contract"]);
    assert!(spec.status.success(), "{}", output_text(&spec));
    let path = root.join("project-management/specs/SPC-001.md");
    let fresh = std::fs::read_to_string(&path).unwrap();
    assert!(fresh.contains("open_questions: []"), "{fresh}");
    std::fs::write(
        &path,
        fresh.replace(
            "open_questions: []",
            "open_questions: [\"Which store is authoritative?\"]",
        ),
    )
    .unwrap();
    let refused = codeflow(&root, &["spec", "status", "SPC-001", "approved"]);
    assert_eq!(refused.status.code(), Some(1), "{}", output_text(&refused));
    assert!(
        output_text(&refused).contains("still open: Which store is authoritative?"),
        "{}",
        output_text(&refused)
    );
    std::fs::write(&path, &fresh).unwrap();
    let approved = codeflow(&root, &["spec", "status", "SPC-001", "approved"]);
    assert!(approved.status.success(), "{}", output_text(&approved));
}

const SPEC_TEMPLATE: &str = "project-management/templates/spec.md";

/// Put the spec template back to how a release before TSK-135 shipped it,
/// recorded as unmodified (file, baseline and manifest hash agree), and
/// return the current shipped text.
fn record_an_older_spec_template(root: &Path) -> String {
    let current = read(root, SPEC_TEMPLATE);
    let line = current
        .lines()
        .find(|l| l.starts_with("open_questions:"))
        .expect("shipped template carries open_questions");
    let older = current.replace(&format!("{line}\n"), "");
    std::fs::write(root.join(SPEC_TEMPLATE), &older).unwrap();
    std::fs::write(root.join(".codeflow/.baseline").join(SPEC_TEMPLATE), &older).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_str(&read(root, ".codeflow/manifest.json")).unwrap();
    manifest["files"][SPEC_TEMPLATE]["sha256"] =
        codeflow_core::scaffold::sha256_hex(older.as_bytes()).into();
    std::fs::write(
        root.join(".codeflow/manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    current
}

/// How many PR templates `.github` holds, compared without case: the
/// shipped name differs from a kept `PULL_REQUEST_TEMPLATE.md` only in case.
fn pr_templates(root: &Path) -> usize {
    std::fs::read_dir(root.join(".github"))
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("pull_request_template.md")
        })
        .count()
}

/// TSK-135 AC-4 (update): a full-tier adopter on an older spec template and
/// a kept PR template with an accepted heading mapping runs `codeflow
/// update`. The unmodified spec template gains `open_questions`; the kept
/// template, the mapping and a docs-only body in the template's own
/// headings all stand.
#[test]
fn update_migrates_the_spec_template_and_keeps_the_pr_mapping() {
    const KEPT: &str = "## Description\n\n<!-- What and why. -->\n\n## Changes\n\n- \n\n\
                        ## How has this been tested?\n\n## Release notes\n";
    const TEMPLATE: &str = ".github/PULL_REQUEST_TEMPLATE.md";
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.email", "t@example.com"],
        &["config", "user.name", "t"],
    ] {
        git_with_binary(&root, args);
    }
    std::fs::create_dir_all(root.join(".github")).unwrap();
    std::fs::write(root.join(TEMPLATE), KEPT).unwrap();
    std::fs::write(root.join("app.rs"), "fn main() {}\n").unwrap();
    git_with_binary(&root, &["add", "."]);
    git_with_binary(&root, &["commit", "-q", "-m", "chore: existing project"]);
    let init = codeflow(&root, &["init", "--yes", "--full"]);
    assert!(init.status.success(), "{}", output_text(&init));
    assert_eq!(pr_templates(&root), 1);

    // The adopter accepted the proposed mapping.
    let policy_path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    let mapping = &mut policy["git"]["pr_section_mapping"];
    assert_eq!(mapping["state"], "diagnosed", "{mapping}");
    mapping["state"] = "accepted".into();
    mapping["decided"] = "2026-09-27".into();
    let accepted = mapping.clone();
    std::fs::write(
        &policy_path,
        format!("{}\n", serde_json::to_string_pretty(&policy).unwrap()),
    )
    .unwrap();

    let current = record_an_older_spec_template(&root);

    let update = codeflow(&root, &["update"]);
    assert!(update.status.success(), "{}", output_text(&update));
    assert_eq!(
        read(&root, SPEC_TEMPLATE),
        current,
        "the spec template migrates"
    );
    assert_eq!(read(&root, TEMPLATE), KEPT, "the kept template stands");
    assert_eq!(pr_templates(&root), 1);
    let after: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    assert_eq!(after["git"]["pr_section_mapping"], accepted);

    // A docs-only body in the template's own headings passes at block.
    git_with_binary(&root, &["switch", "-q", "-c", "chore/adopt"]);
    git_with_binary(&root, &["add", "-A"]);
    git_with_binary(&root, &["commit", "-q", "-m", "chore: adopt codeflow"]);
    git_with_binary(&root, &["switch", "-q", "-c", "docs/guide"]);
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/guide.md"), "# Guide\n").unwrap();
    git_with_binary(&root, &["add", "docs/guide.md"]);
    git_with_binary(&root, &["commit", "-q", "-m", "docs: add a guide"]);
    let allocated = codeflow(
        &root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "bounded guide",
            "--into",
            "chore/adopt",
            "guide",
        ],
    );
    assert!(allocated.status.success(), "{}", output_text(&allocated));
    let task_path = root.join("project-management/tasks/TSK-001.md");
    let task = std::fs::read_to_string(&task_path).unwrap().replace(
        "- AC-1\n",
        "- AC-1 When read, the guide shall explain setup.\n",
    );
    std::fs::write(&task_path, task).unwrap();
    git_with_binary(&root, &["branch", "-m", "task/TSK-001-guide"]);
    git_with_binary(&root, &["add", "project-management/tasks/TSK-001.md"]);
    git_with_binary(&root, &["commit", "-q", "-m", "docs: record guide task"]);
    let body = "Task: TSK-001\n\n## Description\n\nAdds a guide.\n\n\
                ## Changes\n\n- a guide\n";
    let ci = codeflow(
        &root,
        &[
            "ci",
            "--base",
            "chore/adopt",
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-guide",
            "--pr-body",
            body,
        ],
    );
    let all = output_text(&ci);
    assert_eq!(ci.status.code(), Some(0), "{all}");
    assert!(all.contains("level pr_sections = block"), "{all}");
    assert!(
        !String::from_utf8_lossy(&ci.stderr).contains("git.pr_sections"),
        "{all}"
    );
}

/// TSK-131 AC-5 (serves EPC-020 AC-13): the installed doctrine files and the
/// assets they come from. The skill files install in both trees.
const TSK131_DOCTRINE_FILES: &[(&str, &str)] = &[
    (
        "skills/cf-model-orchestrator/resources/quality/findings.md",
        "agents/skills/cf-model-orchestrator/resources/quality/findings.md",
    ),
    (
        "skills/cf-model-orchestrator/resources/quality-contract.md",
        "agents/skills/cf-model-orchestrator/resources/quality-contract.md",
    ),
    (
        "skills/cf-model-orchestrator/resources/quality/blockers-and-gates.md",
        "agents/skills/cf-model-orchestrator/resources/quality/blockers-and-gates.md",
    ),
    (
        "skills/cf-model-orchestrator/resources/quality/design-implementation.md",
        "agents/skills/cf-model-orchestrator/resources/quality/design-implementation.md",
    ),
    (
        "skills/cf-model-orchestrator/SKILL.md",
        "agents/skills/cf-model-orchestrator/SKILL.md",
    ),
    (
        "skills/cf-develop/SKILL.md",
        "agents/skills/cf-develop/SKILL.md",
    ),
    (
        "skills/cf-consult/SKILL.md",
        "agents/skills/cf-consult/SKILL.md",
    ),
];

/// Every installed doctrine path and its asset, at the standard and full
/// tiers.
fn tsk131_installed() -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = TSK131_DOCTRINE_FILES
        .iter()
        .flat_map(|(rel, src)| {
            [".claude", ".agents"]
                .iter()
                .map(move |tree| (format!("{tree}/{rel}"), (*src).to_string()))
        })
        .collect();
    files.extend([
        (
            ".claude/agents/cf-reviewer.md".to_string(),
            "claude/agents/cf-reviewer.md".to_string(),
        ),
        (
            ".codeflow/rules/writing.md".to_string(),
            "rules/writing.md".to_string(),
        ),
        (
            ".codeflow/rules/workflow-discipline.md".to_string(),
            "rules/workflow-discipline.md".to_string(),
        ),
    ]);
    files
}

/// Record `older` as the unmodified install of `rel`: the file, its
/// baseline and its manifest hash agree, as a release before TSK-131 left
/// them. `None` removes the file and its records, as for a file that release
/// did not ship.
fn record_as_installed(root: &Path, rel: &str, older: Option<&str>) {
    let mut manifest: serde_json::Value =
        serde_json::from_str(&read(root, ".codeflow/manifest.json")).unwrap();
    let files = manifest["files"].as_object_mut().expect("manifest files");
    let baseline = root.join(".codeflow/.baseline").join(rel);
    if let Some(text) = older {
        std::fs::write(root.join(rel), text).unwrap();
        std::fs::write(&baseline, text).unwrap();
        files.get_mut(rel).expect("manifest record")["sha256"] =
            codeflow_core::scaffold::sha256_hex(text.as_bytes()).into();
    } else {
        std::fs::remove_file(root.join(rel)).unwrap();
        let _ = std::fs::remove_file(&baseline);
        assert!(files.remove(rel).is_some(), "{rel} had no manifest record");
    }
    std::fs::write(
        root.join(".codeflow/manifest.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()),
    )
    .unwrap();
}

#[test]
fn fresh_scaffolds_install_the_holistic_fix_doctrine_and_update_brings_it() {
    const FINDINGS_ROW: &str = "| [Review findings and repair](quality/findings.md) | when a defect is fixed, or review findings are briefed, written or acted on |\n";
    for tier in ["--standard", "--full"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let init = codeflow(&root, &["init", "--yes", tier]);
        assert!(init.status.success(), "{tier}: {}", output_text(&init));

        // Installed through the real binary, byte-identical to the assets.
        let assets = repo_root().join("assets/base");
        for (rel, src) in tsk131_installed() {
            assert_eq!(
                normalize_crlf(&read(&root, &rel)),
                normalize_crlf(&std::fs::read_to_string(assets.join(&src)).unwrap()),
                "{tier}: {rel} differs from its asset"
            );
        }
        // The doctrine an agent in this project reads.
        for (rel, sentence) in [
            (
                ".claude/skills/cf-model-orchestrator/resources/quality/findings.md",
                "Every blocker or major finding carries the smallest evidenced remedy",
            ),
            (
                ".agents/skills/cf-model-orchestrator/resources/quality/findings.md",
                "Review is one holistic pass per revision",
            ),
            (
                ".claude/skills/cf-model-orchestrator/resources/quality/blockers-and-gates.md",
                "An already approved departure is reused and not asked again.",
            ),
            (
                ".claude/skills/cf-develop/SKILL.md",
                "No cycle count decides: continue while repairs produce relevant evidence",
            ),
            (
                ".claude/agents/cf-reviewer.md",
                "remedy: <blocker and major",
            ),
            (".codeflow/rules/writing.md", "## Copy guide"),
            (".codeflow/rules/writing.md", "### Microcopy"),
        ] {
            let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(
                words(&read(&root, rel)).contains(sentence),
                "{tier}: {rel} lacks `{sentence}`"
            );
        }
        let fresh = skill_tree_snapshot(&root);
        let fresh_rules = (
            read(&root, ".codeflow/rules/writing.md"),
            read(&root, ".codeflow/rules/workflow-discipline.md"),
            read(&root, ".claude/agents/cf-reviewer.md"),
        );

        // An existing project from before TSK-131: no findings section, the
        // index without its row, the three-cycle bound, and the writing
        // reference without the copy guide, each recorded as unmodified.
        for tree in [".claude", ".agents"] {
            let base = format!("{tree}/skills/cf-model-orchestrator/resources");
            record_as_installed(&root, &format!("{base}/quality/findings.md"), None);
            let index = read(&root, &format!("{base}/quality-contract.md"));
            assert!(index.contains(FINDINGS_ROW), "{tier}: findings row");
            record_as_installed(
                &root,
                &format!("{base}/quality-contract.md"),
                Some(&index.replace(FINDINGS_ROW, "")),
            );
            let develop = format!("{tree}/skills/cf-develop/SKILL.md");
            let current = read(&root, &develop);
            let older = current.replace("cycle count decides", "Maximum 3 evidence-moving cycles");
            assert_ne!(older, current, "{tier}: cf-develop lost the progress rule");
            record_as_installed(&root, &develop, Some(&older));
        }
        let writing = read(&root, ".codeflow/rules/writing.md");
        let (older, _) = writing.split_once("\n## Copy guide\n").expect("copy guide");
        record_as_installed(&root, ".codeflow/rules/writing.md", Some(older));
        assert!(!root
            .join(".claude/skills/cf-model-orchestrator/resources/quality/findings.md")
            .exists());

        let update = codeflow(&root, &["update"]);
        let report = output_text(&update);
        assert!(update.status.success(), "{tier}: update failed: {report}");
        assert!(!report.contains("CONFLICT"), "{tier}: {report}");
        assert_eq!(
            skill_tree_snapshot(&root),
            fresh,
            "{tier}: update left the skill trees different from a fresh scaffold"
        );
        assert_eq!(
            (
                read(&root, ".codeflow/rules/writing.md"),
                read(&root, ".codeflow/rules/workflow-discipline.md"),
                read(&root, ".claude/agents/cf-reviewer.md"),
            ),
            fresh_rules,
            "{tier}: update did not bring the writing reference and reviewer"
        );
        let again = codeflow(&root, &["update"]);
        let again_report = output_text(&again);
        assert!(
            again.status.success() && !again_report.contains("CONFLICT"),
            "{tier}: second update was not clean: {again_report}"
        );
    }
}

/// Run the stub delivery cases of `evals/herdr-delivery` against the
/// `cf-herdr` installed at `skill`.
fn run_herdr_delivery_cases(skill: &Path) -> Output {
    Command::new("python3")
        .arg("-B")
        .arg(repo_root().join("evals/herdr-delivery/test_delivery.py"))
        .arg("StubDeliveryTests")
        .env("CF_HERDR_SKILL_DIR", skill)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("HERDR_ENV")
        .output()
        .expect("python3 runs the herdr delivery cases")
}

/// TSK-144 AC-5 (journey): a fresh standard and full project carries the
/// confirmed Herdr delivery, the installed script passes the AC-1 and AC-2
/// stub cases, and `codeflow update` brings it to a project installed before
/// it.
#[test]
fn fresh_scaffolds_install_the_confirmed_herdr_delivery_and_update_brings_it() {
    const SCRIPT: &str = "skills/cf-herdr/scripts/deliver.py";
    const SKILL: &str = "skills/cf-herdr/SKILL.md";
    let assets = repo_root().join("assets/base/agents");
    for tier in ["--standard", "--full"] {
        let (_tmp, root) = fresh(tier);
        for tree in [".claude", ".agents"] {
            for rel in [SCRIPT, SKILL] {
                assert_eq!(
                    normalize_crlf(&read(&root, &format!("{tree}/{rel}"))),
                    normalize_crlf(&std::fs::read_to_string(assets.join(rel)).unwrap()),
                    "{tier}: {tree}/{rel} differs from its asset"
                );
            }
            let cases = run_herdr_delivery_cases(&root.join(tree).join("skills/cf-herdr"));
            assert!(
                cases.status.success(),
                "{tier}: installed {tree} cf-herdr fails the delivery cases:\n{}",
                output_text(&cases)
            );
        }
        let fresh_skills = skill_tree_snapshot(&root);

        // A project installed before TSK-144: no script, and the skill's
        // delivery section as it read then, recorded as unmodified.
        for tree in [".claude", ".agents"] {
            record_as_installed(&root, &format!("{tree}/{SCRIPT}"), None);
            let skill = read(&root, &format!("{tree}/{SKILL}"));
            let older = skill.replace("scripts/deliver.py", "scripts/deliver-older.py");
            record_as_installed(&root, &format!("{tree}/{SKILL}"), Some(&older));
        }
        assert!(!root.join(".agents").join(SCRIPT).exists());

        let update = codeflow(&root, &["update"]);
        let report = output_text(&update);
        assert!(update.status.success(), "{tier}: update failed: {report}");
        assert!(!report.contains("CONFLICT"), "{tier}: {report}");
        assert_eq!(
            skill_tree_snapshot(&root),
            fresh_skills,
            "{tier}: update left the skill trees different from a fresh scaffold"
        );
        let cases = run_herdr_delivery_cases(&root.join(".agents/skills/cf-herdr"));
        assert!(
            cases.status.success(),
            "{tier}: updated cf-herdr fails the delivery cases:\n{}",
            output_text(&cases)
        );
    }
}

/// TSK-163 AC-4 (serves EPC-020 AC-13): fresh standard and full scaffolds
/// install the lane and the turn adapter with each delegated turn rule stated
/// once, in the adapter, before launch; `codeflow update` brings them to a
/// project installed before TSK-163 and leaves none of the lane's old copies.
#[test]
fn fresh_scaffolds_install_the_turn_rules_in_the_adapter_and_update_brings_them() {
    const LANE: &str = "skills/cf-delegate/resources/lane-lifecycle.md";
    const ADAPTER: &str = "skills/cf-delegate/resources/claude-turn-completion.md";
    // The lane's own copies before TSK-163: the launch, turn detection and a
    // preflight run "before delivery".
    const STALE: &[&str] = &[
        "tmux new-session -d -s cf-run-42",
        "**Turn detection is the lifecycle, not the pane.**",
        "**Sibling Stop-hook preflight.** Before delivery",
    ];
    let assets = repo_root().join("assets/base/claude");
    for tier in ["--standard", "--full"] {
        let (_tmp, root) = fresh(tier);
        for tree in [".claude", ".agents"] {
            for rel in [LANE, ADAPTER] {
                assert_eq!(
                    normalize_crlf(&read(&root, &format!("{tree}/{rel}"))),
                    normalize_crlf(&std::fs::read_to_string(assets.join(rel)).unwrap()),
                    "{tier}: {tree}/{rel} differs from its asset"
                );
            }
            let lane = read(&root, &format!("{tree}/{LANE}"));
            for stale in STALE {
                assert!(!lane.contains(stale), "{tier}: {tree} lane keeps {stale}");
            }
            let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(words(&lane).contains("before launching Claude, read and follow the shipped"));
            let adapter = words(&read(&root, &format!("{tree}/{ADAPTER}")));
            let preflight = adapter.find("Reject any sibling Stop hook").unwrap();
            assert!(
                preflight < adapter.find("tmux new-session").unwrap(),
                "{tier}: {tree} adapter places the preflight after launch"
            );
        }
        let fresh_skills = skill_tree_snapshot(&root);

        // A project installed before TSK-163: the lane with its own copies,
        // recorded as unmodified.
        for tree in [".claude", ".agents"] {
            let lane = read(&root, &format!("{tree}/{LANE}"));
            let older = format!(
                "{lane}\n```sh\ntmux new-session -d -s cf-run-42 -x 220 -y 50\n```\n\n\
                 - **Turn detection is the lifecycle, not the pane.** `init` wires the hooks.\n\
                 - **Sibling Stop-hook preflight.** Before delivery, enumerate the \
                 effective Stop-hook set.\n"
            );
            record_as_installed(&root, &format!("{tree}/{LANE}"), Some(&older));
        }

        let update = codeflow(&root, &["update"]);
        let report = output_text(&update);
        assert!(update.status.success(), "{tier}: update failed: {report}");
        assert!(!report.contains("CONFLICT"), "{tier}: {report}");
        assert_eq!(
            skill_tree_snapshot(&root),
            fresh_skills,
            "{tier}: update left the skill trees different from a fresh scaffold"
        );
        for tree in [".claude", ".agents"] {
            let lane = read(&root, &format!("{tree}/{LANE}"));
            for stale in STALE {
                assert!(
                    !lane.contains(stale),
                    "{tier}: update left {tree} lane with {stale}"
                );
            }
        }
    }
}

/// Run one wired hook command (`codeflow hook <name>`) in `dir` with `payload`
/// on stdin, the way a harness does, with `exe` standing in for `codeflow`.
fn run_wired_hook_with(exe: &Path, dir: &Path, command: &str, payload: &str) -> Output {
    use std::io::Write as _;
    let command = command.replacen("codeflow ", &format!("'{}' ", exe.display()), 1);
    let mut child = Command::new("sh")
        .args(["-c", &command])
        .current_dir(dir)
        .env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("hook executable runs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn run_wired_hook(dir: &Path, command: &str, payload: &str) -> Output {
    run_wired_hook_with(
        Path::new(env!("CARGO_BIN_EXE_codeflow")),
        dir,
        command,
        payload,
    )
}

/// The hook commands wired for `event` in a harness hook file, with the
/// matcher of the group that carries each.
fn wired_hooks(file: &serde_json::Value, event: &str) -> Vec<(Option<String>, String)> {
    file["hooks"][event]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| {
            let matcher = group["matcher"].as_str().map(str::to_string);
            group["hooks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|hook| hook["command"].as_str())
                .map(move |command| (matcher.clone(), command.to_string()))
        })
        .collect()
}

const ADVISORY: &str = "codeflow hook session-orient";
const CLAUDE_SOURCES: &str = "startup|resume|clear|compact|fork";
const CODEX_SOURCES: &str = "startup|resume|clear|compact";

fn start(source: &str) -> String {
    format!(r#"{{"hook_event_name":"SessionStart","source":"{source}"}}"#)
}

fn prompt(text: &str) -> String {
    serde_json::json!({"hook_event_name": "UserPromptSubmit", "prompt": text}).to_string()
}

/// A fresh scaffold of `flag` in a tempdir.
fn fresh(flag: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let init = codeflow(&root, &["init", "--yes", flag]);
    assert!(
        init.status.success(),
        "{flag}: init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    (tmp, root)
}

/// TSK-128 AC-5 (serves EPC-020 AC-13): a fresh scaffold at every tier
/// wires one advisory command, `session-orient`, on `SessionStart` and
/// `UserPromptSubmit` for Claude (sources with fork) and Codex (its four
/// documented sources), and that same wired command gives each event its
/// text through the real binary. Grok wires only the guards: its events
/// exist but carry no context to the model.
#[test]
fn fresh_scaffolds_wire_rule_reinjection_at_every_tier() {
    for flag in ["--minimal", "--standard", "--full"] {
        let (_tmp, root) = fresh(flag);
        let claude: serde_json::Value =
            serde_json::from_str(&read(&root, ".claude/settings.json")).unwrap();
        let codex: serde_json::Value =
            serde_json::from_str(&read(&root, ".codex/hooks.json")).unwrap();
        let grok: serde_json::Value =
            serde_json::from_str(&read(&root, ".grok/hooks/codeflow.json")).unwrap();

        for (harness, file, sources) in [
            ("claude", &claude, CLAUDE_SOURCES),
            ("codex", &codex, CODEX_SOURCES),
        ] {
            let session = wired_hooks(file, "SessionStart");
            assert_eq!(session.len(), 1);
            assert_eq!(session[0].0.as_deref(), Some(sources));
            assert!(session[0]
                .1
                .starts_with("codeflow hook session-orient --contract 3"));
            let prompts = wired_hooks(file, "UserPromptSubmit");
            assert_eq!(prompts.len(), 1);
            assert!(prompts[0]
                .1
                .starts_with("codeflow hook session-orient --contract 3"));

            let startup = run_wired_hook(&root, ADVISORY, &start("startup"));
            assert_eq!(startup.status.code(), Some(0));
            let digest = String::from_utf8(startup.stdout).unwrap();
            assert!(digest.contains("# orient"), "{flag} {harness}: {digest}");
            assert!(!digest.contains("## Rules after"), "{flag} {harness}");
            for source in sources
                .split('|')
                .filter(|s| *s != "startup" && *s != "clear")
            {
                let out = run_wired_hook(&root, ADVISORY, &start(source));
                assert_eq!(out.status.code(), Some(0));
                let text = String::from_utf8(out.stdout).unwrap();
                let Some((head, block)) = text.split_once("\n## Rules after compaction or resume")
                else {
                    panic!("{flag} {harness} {source}: no guidance block\n{text}");
                };
                assert_eq!(head, digest, "{flag} {harness} {source}");
                let heading = "## Rules after compaction or resume";
                println!(
                    "{flag} {harness} {source}: block {} bytes (guideline 1536)",
                    heading.len() + block.len()
                );
            }

            let reminder = run_wired_hook(
                &root,
                ADVISORY,
                &prompt("How long will the login page take?"),
            );
            assert_eq!(reminder.status.code(), Some(0));
            let line = String::from_utf8(reminder.stdout).unwrap();
            assert_eq!(line.lines().count(), 1, "{flag} {harness}: {line}");
            assert!(
                line.starts_with("codeflow reminder: When you give a duration"),
                "{flag} {harness}: {line}"
            );
            for payload in [
                prompt("Rename foo to bar"),
                r#"{"hook_event_name":"UserPromptSubmit","prompt":null}"#.to_string(),
                r#"{"hook_event_name":"SomeFutureEvent"}"#.to_string(),
            ] {
                let quiet = run_wired_hook(&root, ADVISORY, &payload);
                assert_eq!(quiet.status.code(), Some(0), "{flag} {harness} {payload}");
                assert!(quiet.stdout.is_empty(), "{flag} {harness} {payload}");
            }
        }

        let grok_events: Vec<&String> = grok["hooks"].as_object().unwrap().keys().collect();
        assert_eq!(
            grok_events,
            vec!["PreToolUse"],
            "{flag}: Grok wires only guards"
        );

        // The key is not written into the file, so an older binary can
        // still read it; the built-in default (warn) applies.
        let policy: serde_json::Value =
            serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
        assert!(policy.get("guidance").is_none(), "{flag}");
        let show = codeflow(&root, &["policy", "show"]);
        let shown = String::from_utf8_lossy(&show.stdout);
        let line = shown
            .lines()
            .find(|line| line.contains("guidance.prompt_reminders"))
            .unwrap_or_else(|| panic!("{flag}: policy show omits the key\n{shown}"));
        assert!(line.contains("warn"), "{flag}: {line}");
    }
}

/// Compile a stand-in for an older `codeflow` (public v2.1.0 and pre-change
/// 3.0.0 builds behave alike here): it knows `hook session-orient`, prints a
/// digest for it and exits 0, and exits 2 for any hook name it does not
/// know, as clap does. Built with the test's own `rustc`, so the fixture runs
/// wherever the suite does (macOS, Linux, native Windows).
fn older_binary_stub(dir: &Path) -> PathBuf {
    let source = dir.join("old.rs");
    std::fs::write(
        &source,
        r##"use std::io::Read;
fn main() {
    if std::env::args().any(|arg| arg == "--contract") { std::process::exit(2); }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    if args == ["hook", "session-orient"] {
        println!("# orient old binary");
    } else {
        eprintln!("error: invalid value for '<NAME>'");
        std::process::exit(2);
    }
}
"##,
    )
    .unwrap();
    let exe = dir.join(format!("codeflow-old{}", std::env::consts::EXE_SUFFIX));
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let built = Command::new(rustc)
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("rustc runs");
    assert!(
        built.status.success(),
        "stub build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    exe
}

/// TSK-128 F1: version skew. An updated project on a machine whose
/// `codeflow` is older must refuse the contract-3 hook. Every advisory
/// command both hosts wire, run by the older-binary stub with each event,
/// exits 0 (the old binary prints its digest; the reminder is simply
/// missing until the machine upgrades). The control shows the stub refuses
/// an unknown hook name with 2, which is what a new command name would do.
#[test]
fn older_binary_refuses_contract_three_through_either_host() {
    let stubs = tempfile::tempdir().unwrap();
    let old = older_binary_stub(stubs.path());
    let (_tmp, root) = fresh("--standard");
    let control = run_wired_hook_with(&old, &root, "codeflow hook prompt-reminder", &prompt("x"));
    assert_eq!(
        control.status.code(),
        Some(2),
        "the stub models clap's refusal"
    );

    for (host, rel) in [
        ("claude", ".claude/settings.json"),
        ("codex", ".codex/hooks.json"),
    ] {
        let file: serde_json::Value = serde_json::from_str(&read(&root, rel)).unwrap();
        for (event, payload) in [
            ("SessionStart", start("startup")),
            ("SessionStart", start("compact")),
            (
                "UserPromptSubmit",
                prompt("How long will the login page take?"),
            ),
            ("UserPromptSubmit", prompt("Rename foo to bar")),
        ] {
            let wired = wired_hooks(&file, event);
            assert!(!wired.is_empty(), "{host}: {event} not wired");
            for (_, command) in wired {
                let out = run_wired_hook_with(&old, &root, &command, &payload);
                assert_eq!(
                    out.status.code(),
                    Some(2),
                    "{host} {event}: `{command}` must refuse with an older binary: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    }
}

/// TSK-150 AC-7 (journey): in a freshly scaffolded standard-tier project
/// driven by the installed CLI, the kernel is loaded at session start (the
/// managed block of `AGENTS.md` and the session-start digest), a triggered
/// section is reachable from its index entry in the installed tree, and
/// `codeflow doctor` reports sizes without failing when a skill is over its
/// guideline. This proves structure and installation; the live load of a
/// triggered section at its moment is TSK-128's re-injection, proved there.
#[test]
fn a_fresh_standard_project_loads_the_kernel_reaches_a_trigger_and_reports_sizes() {
    use codeflow_core::reading::{self, Inventory, SkillFiles};
    use codeflow_core::scaffold::rule_map::managed_block;

    let (_tmp, root) = fresh("--standard");

    // The kernel: the managed block is installed, and the session-start hook
    // wired in the Claude settings gives the digest through the real binary.
    let agents = read(&root, "AGENTS.md");
    assert!(managed_block(&agents).is_some(), "no managed block");
    let claude: serde_json::Value =
        serde_json::from_str(&read(&root, ".claude/settings.json")).unwrap();
    let wired = wired_hooks(&claude, "SessionStart");
    assert_eq!(wired.len(), 1);
    assert_eq!(wired[0].0.as_deref(), Some(CLAUDE_SOURCES));
    assert!(wired[0]
        .1
        .starts_with("codeflow hook session-orient --contract 3"));
    let startup = run_wired_hook(&root, ADVISORY, &start("startup"));
    assert_eq!(startup.status.code(), Some(0));
    assert!(String::from_utf8(startup.stdout)
        .unwrap()
        .contains("# orient"));

    // A triggered section: the installed quality index names the findings
    // section on its trigger row, the section is installed, and the
    // installed tree holds the reading structure with the section outside
    // the per-task chain.
    let index = read(
        &root,
        ".claude/skills/cf-model-orchestrator/resources/quality-contract.md",
    );
    assert!(index.contains(
        "| [Review findings and repair](quality/findings.md) | when a defect is fixed, or review findings are briefed, written or acted on |"
    ));
    assert!(root
        .join(".claude/skills/cf-model-orchestrator/resources/quality/findings.md")
        .is_file());
    let mut installed = SkillFiles::new();
    reading::load_skill_tree(&root.join(".claude/skills"), &mut installed);
    let chain = reading::reading_chain(&installed, &Inventory::SHIPPED);
    assert!(chain.errors.is_empty(), "{:?}", chain.errors);
    assert!(!chain
        .files
        .iter()
        .any(|file| file.path.ends_with("quality/findings.md")));
    assert_eq!(reading::orphans(&installed), Vec::<String>::new());

    // Doctor reports sizes: the fresh install is within every guideline, so
    // the report is clean; a skill doubled past its guideline then warns and
    // names the step that clears it, and doctor still exits 0.
    let fresh_report = codeflow(&root, &["doctor", "--check", "reading"]);
    let clean = output_text(&fresh_report);
    assert_eq!(fresh_report.status.code(), Some(0), "{clean}");
    assert!(
        clean.starts_with("ok    reading: within guidelines:"),
        "{clean}"
    );
    assert!(!clean.contains("differ from the shipped map"), "{clean}");
    let skill = root.join(".claude/skills/cf-herdr/SKILL.md");
    let text = std::fs::read_to_string(&skill).unwrap();
    std::fs::write(&skill, format!("{text}\n{text}")).unwrap();
    let out = codeflow(&root, &["doctor", "--check", "reading"]);
    let report = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert!(
        report.contains("warn  reading: above guideline:"),
        "{report}"
    );
    assert!(report.contains("cf-herdr "), "{report}");
    assert!(report.contains("move detail behind a trigger"), "{report}");
    assert!(report.contains("per-task reading chain"), "{report}");
}

/// The `codeflow` commands a text names in code spans, each as its command
/// path (up to two lower-case words) and the long flags written with it.
fn named_commands(text: &str) -> Vec<(Vec<String>, Vec<String>)> {
    let mut commands = Vec::new();
    for (index, span) in text.split('`').enumerate() {
        if index % 2 == 0 || !span.starts_with("codeflow ") {
            continue;
        }
        let words: Vec<&str> = span.split_whitespace().skip(1).collect();
        let path: Vec<String> = words
            .iter()
            .take_while(|word| {
                !word.is_empty()
                    && word.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                    && !word.starts_with('-')
            })
            .take(2)
            .map(|word| (*word).to_string())
            .collect();
        let flags: Vec<String> = words
            .iter()
            .filter(|word| word.starts_with("--"))
            .map(|word| word.split(['=', '|']).next().unwrap().to_string())
            .collect();
        commands.push((path, flags));
    }
    commands
}

#[test]
fn a_fresh_project_installs_the_work_lifecycle_and_every_command_it_names_runs() {
    // TSK-108 AC-7 (journey, SPC-013 R-34): at each tier that ships the
    // method, the lifecycle section is installed and each command it names,
    // with each flag it writes, exists in the binary the project runs.
    let rel = "cf-method/references/project-organization.md";
    let asset = normalize_crlf(&read(
        &repo_root(),
        &format!("assets/base/claude/skills/{rel}"),
    ));
    let start = asset
        .find("\n## The work lifecycle\n")
        .expect("the asset holds the work lifecycle section");
    let rest = &asset[start + 1..];
    let section = &rest[..rest[3..].find("\n## ").map_or(rest.len(), |at| at + 3)];
    let commands = named_commands(section);
    assert!(
        commands.len() >= 10,
        "the section names the lifecycle verbs: {commands:?}"
    );
    for flag in ["--standard", "--full"] {
        let (_tmp, root) = fresh(flag);
        for tree in [".claude/skills", ".agents/skills"] {
            assert_eq!(
                normalize_crlf(&read(&root, &format!("{tree}/{rel}"))),
                asset,
                "{flag}: {tree}/{rel} is the shipped lifecycle reference"
            );
        }
        for (path, flags) in &commands {
            let mut args: Vec<&str> = path.iter().map(String::as_str).collect();
            args.push("--help");
            let out = codeflow(&root, &args);
            assert!(
                out.status.success(),
                "{flag}: `codeflow {}` named by the lifecycle section does not run: {}",
                path.join(" "),
                output_text(&out)
            );
            let help = output_text(&out);
            for named in flags {
                assert!(
                    help.contains(named.as_str()),
                    "{flag}: `codeflow {}` has no {named} flag",
                    path.join(" ")
                );
            }
        }
    }
}

#[test]
fn installed_ship_reads_release_integration_only_when_configured() {
    let dir = tempfile::tempdir().unwrap();
    let out = codeflow(dir.path(), &["init", "--yes", "--full"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for tree in [".claude", ".agents"] {
        let text =
            std::fs::read_to_string(dir.path().join(format!("{tree}/skills/cf-ship/SKILL.md")))
                .unwrap();
        assert!(text.contains("references/pr-evidence.md#release-integration-after-landing"));
        let reference = std::fs::read_to_string(
            dir.path()
                .join(format!("{tree}/skills/cf-ship/references/pr-evidence.md")),
        )
        .unwrap();
        let text = format!("{text}\n{reference}");
        for required in [
            "only when",
            "release pattern",
            "R-120",
            "after the landing",
            "gh run view",
            "no release-integration task to own it",
        ] {
            assert!(
                text.contains(required),
                "installed skill missing {required}"
            );
        }
    }
    assert!(!dir
        .path()
        .join(".github/workflows/codeflow-release.yml")
        .exists());
    assert!(!dir
        .path()
        .join(".github/workflows/codeflow-release-integration.yml")
        .exists());
}

/// The step a finding names, as printed between the backticks after `run `.
fn printed_step(output: &str, finding: &str) -> Vec<String> {
    let at = output
        .find(finding)
        .unwrap_or_else(|| panic!("no {finding:?} in: {output}"));
    let rest = &output[at..];
    let start = rest.find("run `").expect("the finding names a step to run") + "run `".len();
    let end = start + rest[start..].find('`').expect("the step closes");
    rest[start..end]
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// TSK-147 AC-5 (journey): in a freshly scaffolded standard-tier project
/// driven by the binary under test, every finding names a step that clears
/// it. A commit on a watched path prints a note pointing at the Release
/// impact fields, and `codeflow ci` given a body that declares `Breaking: no`
/// with a `Rationale` reports nothing. A pre-push with a ci finding at warn
/// and a stale task prints both with their steps and does not stop the push;
/// run as printed, the steps leave the next push without either finding.
#[test]
fn a_fresh_standard_project_prints_steps_that_clear_each_finding() {
    let (tmp, root) = fresh("--standard");
    let target = git_stdout(&root, &["branch", "--show-current"]);
    let target = target.trim().to_string();

    // The destination holds the seed: commit format reported at warn and a
    // declared contract surface. It takes the seed by fetching, so no
    // client hook runs on the protected branch.
    git_with_binary(&root, &["switch", "-q", "-c", "chore/seed"]);
    let policy_path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    policy["git"]["commit_format"] = "warn".into();
    policy["git"]["breaking_watch_paths"] = serde_json::json!(["src/api.rs"]);
    // The journey commits at its root checkout on feature branches; the
    // root-checkout rule (TSK-165) has its own journey, so it is off here.
    policy["git"]["root_checkout_commits"] = "off".into();
    std::fs::write(
        &policy_path,
        format!("{}\n", serde_json::to_string_pretty(&policy).unwrap()),
    )
    .unwrap();
    git_with_binary(&root, &["add", ".codeflow/policy.json"]);
    git_with_binary(&root, &["commit", "-q", "-m", "chore: seed the journey"]);
    let dest = tmp.path().join("dest.git");
    let dest_str = dest.to_str().unwrap();
    git_stdout(
        tmp.path(),
        &["init", "-q", "--bare", "-b", &target, dest_str],
    );
    git_with_binary(&root, &["remote", "add", "origin", dest_str]);
    seed(&root, &dest, &target);
    let base = format!("origin/{target}");

    watched_path_settles_by_release_impact(&root, tmp.path(), &base);
    stale_findings_clear_by_their_printed_steps(&root, &dest, &target);
}

/// Let the destination take `chore/seed` as `target` by fetching, so no
/// client hook runs on the protected branch, and refresh `origin`.
fn seed(root: &Path, dest: &Path, target: &str) {
    let refspec = format!("chore/seed:{target}");
    git_stdout(dest, &["fetch", "-q", root.to_str().unwrap(), &refspec]);
    git_with_binary(root, &["fetch", "-q", "origin"]);
}

fn stderr_text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

/// AC-5, first half: a commit on the watched path prints the local note,
/// and a body that settles it leaves `codeflow ci` with nothing to report.
fn watched_path_settles_by_release_impact(root: &Path, tmp: &Path, base: &str) {
    // A commit on the watched path: the local note, then a body that settles it.
    git_with_binary(root, &["switch", "-q", "-c", "feat/api", base]);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/api.rs"), "pub fn api() {}\n").unwrap();
    git_with_binary(root, &["add", "src/api.rs"]);
    let commit = git_output_with_binary(root, &["commit", "-q", "-m", "feat: add the api"]);
    let err = stderr_text(&commit);
    assert!(commit.status.success(), "{err}");
    assert!(
        err.contains(
            "codeflow commit-msg: note: commit touches a declared contract surface (src/api.rs)"
        ) && err.contains("`Breaking: no` with a `Rationale` under Release impact")
            && err.contains("codeflow commit-msg: commit not stopped")
            && !err.contains("warning"),
        "{err}"
    );
    let body = tmp.join("body.md");
    std::fs::write(
        &body,
        "Task: a new api function\n\n## Summary\n\nAdds the api.\n\n\
         ## Changes\n\n- the api\n\n## Testing\n\n- `codeflow ci` over the range\n\
         - Not tested: nothing else\n\n## Reviews\n\n- none yet\n\n\
         ## Release impact\n\n- Impact: minor\n- Breaking: no\n\
         - Rationale: a new function; nothing existing changes\n- Migration: none\n",
    )
    .unwrap();
    let settled = codeflow(
        root,
        &[
            "ci",
            "--base",
            base,
            "--head",
            "HEAD",
            "--branch",
            "feat/api",
            "--pr-body-file",
            body.to_str().unwrap(),
        ],
    );
    let err = stderr_text(&settled);
    assert_eq!(settled.status.code(), Some(0), "{err}");
    assert!(
        !err.contains("contract surface")
            && !err.contains("warning")
            && !err.contains("BLOCKED")
            && !err.contains("note:"),
        "{err}"
    );
}

/// AC-5, second half: a pre-push with a ci finding at warn and a stale task
/// prints both with their steps; run as printed, the next push has neither.
fn stale_findings_clear_by_their_printed_steps(root: &Path, dest: &Path, target: &str) {
    let base = format!("origin/{target}");
    // The destination gains a task left `in_progress` by an older release.
    git_with_binary(root, &["switch", "-q", "chore/seed"]);
    let task = codeflow(
        root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "a journey fixture",
            "a stale task",
        ],
    );
    assert!(task.status.success(), "{}", output_text(&task));
    let record = root.join("project-management/tasks/TSK-001.md");
    let text = std::fs::read_to_string(&record).unwrap();
    assert!(text.contains("\nstatus: todo "), "{text}");
    std::fs::write(
        &record,
        text.replacen("\nstatus: todo ", "\nstatus: in_progress ", 1),
    )
    .unwrap();
    git_with_binary(root, &["add", "project-management"]);
    git_with_binary(root, &["commit", "-q", "-m", "docs: record a legacy task"]);
    seed(root, dest, target);

    // A pre-push with a ci finding at warn and the stale task.
    git_with_binary(root, &["switch", "-q", "-c", "feat/x", &base]);
    std::fs::write(root.join("x.txt"), "x\n").unwrap();
    git_with_binary(root, &["add", "x.txt"]);
    git_with_binary(root, &["commit", "-q", "-m", "Not conventional."]);
    let push = git_output_with_binary(root, &["push", "-q", "origin", "feat/x"]);
    let err = stderr_text(&push);
    assert!(push.status.success(), "{err}");
    assert!(
        err.contains("warning — policy rule git.commit_format (warn)")
            && err.contains("reword it with `git commit --amend` as `type(scope): description`")
            && err.contains("stale status: in_progress with no active branch carrying TSK-001")
            && err.contains("codeflow pre-push: push not stopped")
            && !err.contains("BLOCKED"),
        "{err}"
    );

    // Each step, run as printed.
    git_with_binary(root, &["commit", "-q", "--amend", "-m", "feat: add x"]);
    let step = printed_step(&err, "stale status: in_progress");
    assert_eq!(step[..2], ["codeflow", "task"], "{step:?}");
    let args: Vec<&str> = step[1..].iter().map(String::as_str).collect();
    let status = codeflow(root, &args);
    assert!(status.status.success(), "{}", output_text(&status));
    git_with_binary(root, &["add", "project-management"]);
    git_with_binary(
        root,
        &["commit", "-q", "-m", "docs: return the stale task to todo"],
    );

    let again = git_output_with_binary(
        root,
        &["push", "-q", "--force-with-lease", "origin", "feat/x"],
    );
    let err = stderr_text(&again);
    assert!(again.status.success(), "{err}");
    assert!(
        !err.contains("git.commit_format")
            && !err.contains("stale status")
            && !err.contains("warning")
            && err.contains("codeflow pre-push: push not stopped"),
        "{err}"
    );
}

/// The refusals line of `codeflow report ceremony` over every date.
fn refusals_line(root: &Path) -> String {
    let out = codeflow(root, &["report", "ceremony", "--since", "2000-01-01"]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    text.lines()
        .find(|line| line.starts_with("Refusals hit by this clone's hooks and guards: "))
        .unwrap_or_else(|| panic!("no refusals line in: {text}"))
        .to_string()
}

/// TSK-149 AC-6 (journey): in a freshly scaffolded standard-tier project
/// driven by the binary under test, a push the pre-push hook refuses shows
/// in `codeflow report ceremony`, and a push it only warns about does not.
#[test]
fn a_refused_push_shows_in_the_ceremony_report_and_a_warned_push_does_not() {
    let (tmp, root) = fresh("--standard");
    let target = git_stdout(&root, &["branch", "--show-current"]);
    let target = target.trim().to_string();

    // The destination holds the seed, with commit format reported at warn.
    git_with_binary(&root, &["switch", "-q", "-c", "chore/seed"]);
    let policy_path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&read(&root, ".codeflow/policy.json")).unwrap();
    policy["git"]["commit_format"] = "warn".into();
    std::fs::write(
        &policy_path,
        format!("{}\n", serde_json::to_string_pretty(&policy).unwrap()),
    )
    .unwrap();
    git_with_binary(&root, &["add", ".codeflow/policy.json"]);
    git_with_binary(&root, &["commit", "-q", "-m", "chore: seed the journey"]);
    let dest = tmp.path().join("dest.git");
    let dest_str = dest.to_str().unwrap();
    git_stdout(
        tmp.path(),
        &["init", "-q", "--bare", "-b", &target, dest_str],
    );
    git_with_binary(&root, &["remote", "add", "origin", dest_str]);
    seed(&root, &dest, &target);
    let base = format!("origin/{target}");
    // The hooks that ran marked when recording began; nothing was refused.
    let line = refusals_line(&root);
    assert!(line.contains(": 0 since recording began at "), "{line}");

    // A push straight to the protected target is refused.
    git_with_binary(&root, &["switch", "-q", "-c", "feat/x", &base]);
    std::fs::write(root.join("x.txt"), "x\n").unwrap();
    git_with_binary(&root, &["add", "x.txt"]);
    git_with_binary(&root, &["commit", "-q", "-m", "feat: add x"]);
    let refused = git_output_with_binary(
        &root,
        &["push", "-q", "origin", &format!("feat/x:{target}")],
    );
    let err = stderr_text(&refused);
    assert!(!refused.status.success(), "{err}");
    assert!(
        err.contains("BLOCKED — policy rule git.push_to_protected (block)")
            && err.contains("codeflow pre-push: push stopped"),
        "{err}"
    );
    let line = refusals_line(&root);
    assert!(
        line.contains(": 1 since recording began at ") && line.ends_with(" (pre-push 1)"),
        "{line}"
    );

    // A push with a finding at warn goes through and is not a refusal.
    std::fs::write(root.join("y.txt"), "y\n").unwrap();
    git_with_binary(&root, &["add", "y.txt"]);
    git_with_binary(&root, &["commit", "-q", "-m", "Not conventional."]);
    let warned = git_output_with_binary(&root, &["push", "-q", "origin", "feat/x"]);
    let err = stderr_text(&warned);
    assert!(warned.status.success(), "{err}");
    assert!(
        err.contains("warning — policy rule git.commit_format (warn)")
            && err.contains("codeflow pre-push: push not stopped"),
        "{err}"
    );
    let line = refusals_line(&root);
    assert!(
        line.contains(": 1 since recording began at ") && line.ends_with(" (pre-push 1)"),
        "{line}"
    );
}

/// TSK-177 AC-8 (journey): through the built binary at every tier, `init`
/// installs the plain-writing rule in `AGENTS.md` and at the top of the
/// writing reference, `update` brings both (and a skill's short form) to a
/// project installed before the rule, and `doctor --check reading` reports
/// a fresh install within its guidelines.
#[test]
fn init_and_update_bring_the_plain_writing_rule_at_every_tier() {
    const RULE_LINE: &str =
        "- **Write plainly.** Everything you write, replies and status updates included";
    const LEAD: &str = "**Write plainly.** Everything you write, replies and status updates";
    const SHORT: &str = "Write the synthesis plainly:";
    for tier in ["--minimal", "--standard", "--full"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let init = codeflow(&root, &["init", "--yes", tier]);
        assert!(init.status.success(), "{tier}: {}", output_text(&init));

        let doctor = codeflow(&root, &["doctor", "--check", "reading"]);
        let reading = output_text(&doctor);
        assert_eq!(doctor.status.code(), Some(0), "{tier}: {reading}");
        assert!(!reading.contains("above guideline"), "{tier}: {reading}");

        let agents = read(&root, "AGENTS.md");
        let writing = read(&root, ".codeflow/rules/writing.md");
        assert!(
            agents.contains(RULE_LINE),
            "{tier}: AGENTS.md lacks the rule"
        );
        let lead = writing
            .find(LEAD)
            .expect("writing reference states the rule");
        assert!(
            lead < writing.find("\n## ").unwrap(),
            "{tier}: rule does not lead"
        );
        let skill_trees = tier != "--minimal";
        let consult = ".claude/skills/cf-consult/SKILL.md";
        let fresh_consult = skill_trees.then(|| read(&root, consult));
        if let Some(text) = &fresh_consult {
            assert!(
                text.contains(SHORT),
                "{tier}: cf-consult lacks the short form"
            );
        }

        // A project installed before the rule: the map without its line, the
        // writing reference without its lead, and cf-consult without its
        // short form, each recorded as unmodified.
        let line = agents
            .lines()
            .find(|line| line.starts_with(RULE_LINE))
            .unwrap()
            .to_string();
        let older_agents = agents.replace(&format!("{line}\n"), "");
        std::fs::write(root.join("AGENTS.md"), &older_agents).unwrap();
        let block = codeflow_core::scaffold::rule_map::managed_block(&older_agents)
            .expect("managed block")
            .to_string();
        std::fs::write(root.join(".codeflow/.baseline/AGENTS.md"), &block).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_str(&read(&root, ".codeflow/manifest.json")).unwrap();
        manifest["files"]["AGENTS.md"]["sha256"] =
            codeflow_core::scaffold::sha256_hex(block.as_bytes()).into();
        std::fs::write(
            root.join(".codeflow/manifest.json"),
            format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()),
        )
        .unwrap();
        let (head, tail) = writing.split_at(lead);
        let rest = &tail[tail.find("\n## ").unwrap() + 1..];
        record_as_installed(
            &root,
            ".codeflow/rules/writing.md",
            Some(&format!("{head}{rest}")),
        );
        if let Some(text) = &fresh_consult {
            let older = text.replace(SHORT, "Then");
            for tree in [".claude", ".agents"] {
                record_as_installed(
                    &root,
                    &format!("{tree}/skills/cf-consult/SKILL.md"),
                    Some(&older),
                );
            }
        }
        assert!(!read(&root, "AGENTS.md").contains(RULE_LINE));

        let update = codeflow(&root, &["update"]);
        let report = output_text(&update);
        assert!(update.status.success(), "{tier}: update failed: {report}");
        assert!(!report.contains("CONFLICT"), "{tier}: {report}");
        assert_eq!(read(&root, "AGENTS.md"), agents, "{tier}: map not brought");
        assert_eq!(
            read(&root, ".codeflow/rules/writing.md"),
            writing,
            "{tier}: writing reference not brought"
        );
        if let Some(text) = &fresh_consult {
            for tree in [".claude", ".agents"] {
                assert_eq!(
                    &read(&root, &format!("{tree}/skills/cf-consult/SKILL.md")),
                    text,
                    "{tier}: {tree} cf-consult not brought"
                );
            }
        }
    }
}

/// The spec paragraph of `project-organization.md` before TSK-169.
const TSK169_OLD_RULE: &str = "is complete. Later semantic change gets a new spec or an explicit superseding record;\ndo not rewrite history.";

/// The spec template's lifecycle comment before TSK-169.
const TSK169_OLD_TEMPLATE_COMMENT: &str =
    "<!-- Specs are optional frozen work inputs, not living requirements. `codeflow spec new --for
     EPC-NNN|TSK-NNN` allocates this file and links it from the consuming work
     item. Write one only when interfaces, formats, or behavior need pinning
     down before building; many work items need no spec. `approved` requires
     an empty `open_questions` list and freezes the criteria. `implemented` is
     derived, never written: every consumer is terminal and at least one is
     complete. A changed contract is a new spec that lists
     `supersedes: [SPC-old]`; `codeflow spec status SPC-old superseded --by
     SPC-new` records the link. Keep maintained requirements and executable
     schemas current at their declared authority. -->";

/// TSK-169 AC-6: a fresh scaffold installs the amendment rule in
/// `project-organization.md` at the standard and full tiers and in the spec
/// template at the full tier, and `codeflow update` brings both to a project
/// that has the older text, leaving none of it.
#[test]
fn fresh_scaffolds_install_the_spec_amendment_rule_and_update_brings_it() {
    let assets = repo_root().join("assets/base");
    let asset = |src: &str| normalize_crlf(&std::fs::read_to_string(assets.join(src)).unwrap());
    let organization = asset("claude/skills/cf-method/references/project-organization.md");
    let template = asset("pm/spec.md.tmpl");
    let (rule_start, _) = organization
        .split_once("While it is approved and not yet `implemented`")
        .expect("the amendment rule in the asset");
    let rule_end = organization
        .find("not rewrite history.")
        .expect("the frozen boundary in the asset")
        + "not rewrite history.".len();
    let older_organization = format!(
        "{}{}{}",
        rule_start
            .strip_suffix("is complete. ")
            .expect("rule follows its lead"),
        TSK169_OLD_RULE,
        &organization[rule_end..]
    );
    let start = template
        .find("<!-- Specs are optional")
        .expect("lifecycle comment");
    let end = template[start..].find("-->").expect("comment end") + start + "-->".len();
    let older_template = format!(
        "{}{}{}",
        &template[..start],
        TSK169_OLD_TEMPLATE_COMMENT,
        &template[end..]
    );

    for tier in ["--standard", "--full"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let init = codeflow(&root, &["init", "--yes", tier]);
        assert!(init.status.success(), "{tier}: {}", output_text(&init));
        let mut installed: Vec<(String, &str, &str)> = [".claude", ".agents"]
            .iter()
            .map(|tree| {
                (
                    format!("{tree}/skills/cf-method/references/project-organization.md"),
                    organization.as_str(),
                    older_organization.as_str(),
                )
            })
            .collect();
        let has_template = root.join(SPEC_TEMPLATE).exists();
        assert_eq!(has_template, tier == "--full", "{tier}: spec template tier");
        if has_template {
            installed.push((SPEC_TEMPLATE.to_string(), &template, &older_template));
        }
        for (rel, fresh, older) in &installed {
            assert_eq!(normalize_crlf(&read(&root, rel)), *fresh, "{tier}: {rel}");
            record_as_installed(&root, rel, Some(older));
        }
        let update = codeflow(&root, &["update"]);
        let report = output_text(&update);
        assert!(update.status.success(), "{tier}: update failed: {report}");
        assert!(!report.contains("CONFLICT"), "{tier}: {report}");
        for (rel, fresh, _) in &installed {
            let text = normalize_crlf(&read(&root, rel));
            assert_eq!(text, *fresh, "{tier}: update did not bring {rel}");
            assert!(!text.contains("freezes the criteria"), "{tier}: {rel}");
            assert!(!text.contains("Later semantic change"), "{tier}: {rel}");
        }
    }
}

/// Release canary: brownfield adoption in a linked worktree of a repository
/// whose own `.git/hooks/pre-commit` git ran until `init` set
/// `core.hooksPath`. The init report, `codeflow update` and `codeflow
/// doctor` name the hook as a choice, ignore git's `*.sample` files and a
/// file that is not executable, and leave every hook file in place.
#[test]
fn brownfield_init_names_the_git_dir_hooks_it_stops_running() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("b");
    std::fs::create_dir(&main).unwrap();
    git_stdout(&main, &["init", "-q", "-b", "main"]);
    std::fs::write(main.join("a"), "x\n").unwrap();
    git_with_binary(&main, &["add", "a"]);
    git_with_binary(&main, &["commit", "-qm", "init"]);
    let hooks = main.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::write(hooks.join("pre-commit"), "#!/bin/sh\necho OWN HOOK RAN\n").unwrap();
    std::fs::write(hooks.join("commit-msg.sample"), "#!/bin/sh\n").unwrap();
    std::fs::write(hooks.join("helper.sh"), "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Windows has no executable bit: there every hook-named file counts.
        std::fs::write(hooks.join("pre-push"), "#!/bin/sh\n").unwrap();
        for (name, mode) in [
            ("pre-commit", 0o755),
            ("commit-msg.sample", 0o755),
            ("pre-push", 0o644),
            ("helper.sh", 0o755),
        ] {
            std::fs::set_permissions(hooks.join(name), std::fs::Permissions::from_mode(mode))
                .unwrap();
        }
    }
    git_stdout(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            ".worktrees/adopt",
            "-b",
            "chore/adopt",
        ],
    );
    let adopt = main.join(".worktrees/adopt");

    let out = codeflow(&adopt, &["init", "--standard", "--yes"]);
    let report = output_text(&out);
    assert!(out.status.success(), "{report}");
    let line = report
        .lines()
        .find(|line| line.contains("git no longer runs"))
        .unwrap_or_else(|| panic!("no git-dir hooks note:\n{report}"));
    assert!(line.contains("pre-commit"), "{line}");
    assert!(line.contains("CI or a supported hook manager"), "{line}");
    assert!(line.contains("hooks folder the project owns"), "{line}");
    #[cfg(unix)]
    assert!(!line.contains("pre-push"), "not executable: {line}");
    assert!(!line.contains(".sample"), "{line}");
    assert!(!line.contains("helper.sh"), "not a hook event: {line}");
    assert!(
        hooks.join("pre-commit").exists(),
        "nothing moved or deleted"
    );

    let update = output_text(&codeflow(&adopt, &["update"]));
    assert!(
        update.contains("git no longer runs") && update.contains("pre-commit"),
        "{update}"
    );

    let doctor = output_text(&codeflow(&adopt, &["doctor", "--check", "hooks"]));
    assert!(
        doctor.contains("git does not run") && doctor.contains("pre-commit"),
        "{doctor}"
    );
    std::fs::remove_file(hooks.join("pre-commit")).unwrap();
    let cleared = output_text(&codeflow(&adopt, &["doctor", "--check", "hooks"]));
    assert!(!cleared.contains("git does not run"), "{cleared}");
}
