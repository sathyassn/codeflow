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
                agents.contains(&Kernel::render_rule(rule)),
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
                read(&repo_root(), &format!("assets/base/rules/{reference}")),
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
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    let out = Command::new("git")
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
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
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
            assert!(
                added.len() >= 3 + 16 + 8 + 6,
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
    git_with_binary(&root, &["switch", "-q", "-c", "docs/guide"]);
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/guide.md"), "# Guide\n\nHow to start.\n").unwrap();
    git_with_binary(&root, &["add", "docs/guide.md"]);
    git_with_binary(&root, &["commit", "-q", "-m", "docs: add a starting guide"]);
    let body = "Task: none: a new guide\n\n## Summary\n\nAdds a starting guide.\n\n\
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
            "docs/guide",
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
    let body = "Task: none: a new guide\n\n## Description\n\nAdds a guide.\n\n\
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
            "docs/guide",
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
                "Docs and records: two review rounds per submitted version",
            ),
            (
                ".claude/skills/cf-model-orchestrator/resources/quality/blockers-and-gates.md",
                "An already approved departure is reused and not asked again.",
            ),
            (
                ".claude/skills/cf-develop/SKILL.md",
                "Maximum 2 evidence-moving cycles for code",
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
            let older = read(&root, &develop).replace(
                "Maximum 2 evidence-moving cycles for code",
                "Maximum 3 evidence-moving cycles",
            );
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

/// Run one wired hook command (`codeflow hook <name>`) in `dir` with `payload`
/// on stdin, the way a harness does, with `exe` standing in for `codeflow`.
fn run_wired_hook_with(exe: &Path, dir: &Path, command: &str, payload: &str) -> Output {
    use std::io::Write as _;
    let args: Vec<&str> = command
        .strip_prefix("codeflow ")
        .unwrap_or_else(|| panic!("not a codeflow hook: {command}"))
        .split_whitespace()
        .collect();
    let mut child = Command::new(exe)
        .args(&args)
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
            assert_eq!(
                wired_hooks(file, "SessionStart"),
                vec![(Some(sources.to_string()), ADVISORY.to_string())],
                "{flag} {harness}: SessionStart wiring"
            );
            assert_eq!(
                wired_hooks(file, "UserPromptSubmit"),
                vec![(None, ADVISORY.to_string())],
                "{flag} {harness}: UserPromptSubmit wiring"
            );

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
                line.starts_with("codeflow reminder: Durations"),
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
/// `codeflow` is older must not have its prompts refused. Every advisory
/// command both hosts wire, run by the older-binary stub with each event,
/// exits 0 (the old binary prints its digest; the reminder is simply
/// missing until the machine upgrades). The control shows the stub refuses
/// an unknown hook name with 2, which is what a new command name would do.
#[test]
fn older_binary_never_refuses_a_prompt_through_either_host() {
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
                    Some(0),
                    "{host} {event}: `{command}` would refuse the prompt with an older binary: {}",
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
    assert_eq!(
        wired_hooks(&claude, "SessionStart"),
        vec![(Some(CLAUDE_SOURCES.to_string()), ADVISORY.to_string())]
    );
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

    // Doctor reports sizes: a skill doubled past its guideline warns and
    // names the step that clears it, and doctor still exits 0.
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
