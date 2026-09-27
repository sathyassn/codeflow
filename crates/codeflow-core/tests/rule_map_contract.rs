//! Contract for the root rule map (TSK-127): every tier's AGENTS.md and
//! CLAUDE.md template is rendered from one kernel, stays inside its budget,
//! carries the pinned always rules and moments, points only at files its tier
//! installs, and reaches existing projects through `codeflow update`.
//!
//! Regenerate the templates after a kernel edit with
//! `CODEFLOW_BLESS=1 cargo test -p codeflow-core --test rule_map_contract`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use codeflow_core::scaffold::rule_map::{
    self, is_skill_pointer, managed_block, File, Kernel, CODEX_INSTRUCTION_LIMIT_BYTES,
    MAX_MANAGED_BLOCK_BYTES, MAX_RULES, OUTPUTS,
};
use codeflow_core::scaffold::{
    self, AssetSource, DirSource, InitAnswers, InitOptions, Tier, UpdateOptions,
};

const MOMENT_KEYS: &[&str] = &[
    "estimate",
    "status",
    "explanation",
    "plan",
    "design",
    "build",
    "branch",
    "ship",
    "consult",
    "instruction-change",
    "resume",
];

const REFERENCES: &[&str] = &[
    ".codeflow/rules/workflow-discipline.md",
    ".codeflow/rules/git-rules.md",
    ".codeflow/rules/worktrees.md",
    ".codeflow/rules/writing.md",
];

fn isolate_git() {
    static ISOLATE: std::sync::Once = std::sync::Once::new();
    ISOLATE.call_once(|| {
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
    });
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn assets() -> DirSource {
    DirSource::new(repo_root().join("assets"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn init_opts(tier: Tier) -> InitOptions {
    InitOptions {
        tier: Some(tier),
        force: false,
        binary_version: "3.0.0".to_string(),
        answers: InitAnswers {
            product_one_liner: Some("A billing service for small clinics.".to_string()),
            areas: Some(vec!["api".to_string(), "web".to_string()]),
            permission_preset: None,
        },
    }
}

fn scaffold(source: &dyn AssetSource, tier: Tier) -> tempfile::TempDir {
    isolate_git();
    let dir = tempfile::tempdir().expect("project tempdir");
    scaffold::init(source, dir.path(), &init_opts(tier)).expect("init succeeds");
    dir
}

fn agents_output(tier: Tier) -> &'static rule_map::Output {
    OUTPUTS
        .iter()
        .find(|output| output.file == File::Agents && output.tier == tier)
        .expect("an AGENTS output per tier")
}

/// AC-1: the checked-in templates are exactly the kernel's renders.
#[test]
fn every_root_template_is_rendered_from_the_one_kernel() {
    let kernel = Kernel::shipped();
    assert_eq!(kernel.schema_version, 1);
    let base = repo_root().join("assets/base");
    let bless = std::env::var_os("CODEFLOW_BLESS").is_some();
    let mut drifted = Vec::new();
    for output in OUTPUTS {
        let rendered = kernel.render(output);
        let path = base.join(output.asset);
        if bless {
            std::fs::write(&path, &rendered).expect("write rendered template");
            continue;
        }
        if read(&path) != rendered {
            drifted.push(output.asset);
        }
    }
    assert!(
        drifted.is_empty(),
        "templates drifted from assets/base/rule-map.toml: {drifted:?}; edit the kernel, then \
         regenerate with CODEFLOW_BLESS=1 cargo test -p codeflow-core --test rule_map_contract"
    );
    // The standard and full tiers share one CLAUDE template.
    assert_eq!(
        kernel.render(&rule_map::Output {
            asset: "CLAUDE.md.tmpl",
            tier: Tier::Full,
            file: File::Claude,
        }),
        kernel.render(&rule_map::Output {
            asset: "CLAUDE.md.tmpl",
            tier: Tier::Standard,
            file: File::Claude,
        }),
        "the kernel's CLAUDE text differs between standard and full, which share one template"
    );
}

/// AC-1: budget, rule count, one-line rules with pointers, and the moments.
#[test]
fn each_tier_map_fits_its_budget_with_one_line_rules_and_every_moment() {
    let kernel = Kernel::shipped();
    for tier in Kernel::tiers() {
        let rendered = kernel.render(agents_output(tier));
        let block = managed_block(&rendered).expect("managed markers");
        assert!(
            block.len() <= MAX_MANAGED_BLOCK_BYTES,
            "{tier} managed block is {} bytes, over {MAX_MANAGED_BLOCK_BYTES}",
            block.len()
        );

        let rules = kernel.rules_for(tier);
        assert!(
            !rules.is_empty() && rules.len() <= MAX_RULES,
            "{tier} carries {} always rules",
            rules.len()
        );
        let mut ids = BTreeSet::new();
        for rule in &rules {
            assert!(
                ids.insert(rule.id.as_str()),
                "{tier}: rule {} twice",
                rule.id
            );
            assert!(
                !rule.see.is_empty(),
                "{tier}: rule {} has no pointer",
                rule.id
            );
            let line = Kernel::render_rule(rule);
            assert!(!line.contains('\n'), "{tier}: rule {} spans lines", rule.id);
            assert!(
                line.len() <= 420,
                "{tier}: rule {} is {} bytes, too long for one line",
                rule.id,
                line.len()
            );
            assert!(
                block.contains(&line),
                "{tier}: rule {} is not in the map",
                rule.id
            );
        }

        let keys: Vec<&str> = kernel
            .moments_for(tier)
            .iter()
            .map(|moment| moment.key.as_str())
            .collect();
        assert_eq!(keys, MOMENT_KEYS, "{tier} moment table keys");
        for moment in kernel.moments_for(tier) {
            assert!(
                !moment.see.is_empty(),
                "{tier}: moment {} has no pointer",
                moment.key
            );
        }
        assert!(block.contains("## When you are about to"));
        assert!(block.contains("## Always rules"));
    }
}

/// AC-2: the rules that failed in practice are always loaded, as sentences
/// pinned per tier.
#[test]
fn the_failed_in_practice_rules_are_pinned_always_rules() {
    let kernel = Kernel::shipped();
    let common = [
        "never human weeks, sprints or person-days",
        "Replies, status and summaries lead with outcomes in plain words, with IDs and file names after",
        "titles name the subject in words",
        "no em or en dash in new text",
        "Evidence and honest analysis outrank agreement",
        "say what was not verified",
        "Find broadly; act by materiality.",
        "Prove it where it runs.",
        "Work to the outcome.",
        "Git floor [enforced].",
        "explicit authenticated human approval",
        "self-review is not review",
        "A change to an adopter-facing path",
    ];
    let method_tiers = [
        "Agent-delivered durations come from `/cf-estimate`",
        "goes through `/cf-present` where the harness can show it",
        "Orchestration entry is decided by touched paths",
        "starts with `/cf-model-orchestrator`",
        "when unsure, route",
        "fresh-context independent review",
        "otherwise a separate read-only pass",
        "Review verdicts require `cf-reviewer`",
    ];
    for tier in Kernel::tiers() {
        let block = normalized(
            managed_block(&kernel.render(agents_output(tier))).expect("managed markers"),
        );
        let mut missing: Vec<&str> = common
            .iter()
            .copied()
            .filter(|needle| !block.to_lowercase().contains(&needle.to_lowercase()))
            .collect();
        if tier != Tier::Minimal {
            missing.extend(method_tiers.iter().copied().filter(|needle| {
                // The review sentence lives in the reference; the
                // map carries its one-line form.
                *needle != "Review verdicts require `cf-reviewer`" && !block.contains(needle)
            }));
        }
        assert!(
            missing.is_empty(),
            "{tier} map lost pinned sentences: {missing:?}"
        );
    }
    let discipline = normalized(&read(
        &repo_root().join("assets/base/rules/workflow-discipline.md"),
    ));
    assert!(discipline.contains("Review verdicts require `cf-reviewer`"));
}

/// Backticked tokens in a managed block that name an installed file or
/// directory: a path ending in a file extension or `/`, without placeholders
/// or spaces.
fn path_tokens(block: &str) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    for (index, part) in block.split('`').enumerate() {
        if index % 2 == 0 {
            continue;
        }
        let token = part.trim();
        let placeholder = token.contains(['<', '{', ' ', '|', '*', '"']);
        let extension = Path::new(token)
            .extension()
            .and_then(|extension| extension.to_str());
        let pathlike = matches!(extension, Some("md" | "json")) || token.ends_with('/');
        if pathlike && token.contains('/') && !placeholder {
            tokens.insert(token.to_string());
        }
    }
    tokens
}

fn skill_tokens(block: &str) -> BTreeSet<String> {
    let mut skills = BTreeSet::new();
    for (index, part) in block.split('`').enumerate() {
        if index % 2 == 1 {
            let token = part.trim().trim_start_matches('/');
            if is_skill_pointer(token) && token.chars().all(|c| c.is_ascii_lowercase() || c == '-')
            {
                skills.insert(token.to_string());
            }
        }
    }
    skills
}

fn resolve(root: &Path, pointer: &str) -> Result<(), String> {
    let candidates: Vec<PathBuf> = if is_skill_pointer(pointer) {
        if pointer == "cf-reviewer" {
            vec![root.join(".claude/agents/cf-reviewer.md")]
        } else {
            vec![
                root.join(".claude/skills").join(pointer).join("SKILL.md"),
                root.join(".agents/skills").join(pointer).join("SKILL.md"),
            ]
        }
    } else if pointer.starts_with("cf-") {
        // A skill-relative path resolves in both skill trees.
        vec![
            root.join(".claude/skills").join(pointer),
            root.join(".agents/skills").join(pointer),
        ]
    } else {
        vec![root.join(pointer)]
    };
    let missing: Vec<String> = candidates
        .iter()
        .filter(|path| !path.exists())
        .map(|path| path.display().to_string())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("{pointer} -> missing {missing:?}"))
    }
}

/// AC-4: every pointer resolves in a fresh scaffold of the tier that renders
/// the map, both the kernel's declared pointers and every path or skill the
/// rendered text names.
#[test]
fn every_map_pointer_resolves_at_its_tier() {
    let kernel = Kernel::shipped();
    let source = assets();
    for tier in Kernel::tiers() {
        let project = scaffold(&source, tier);
        let root = project.path();
        let installed = read(&root.join("AGENTS.md"));
        let block = managed_block(&installed).expect("installed managed block");

        let mut pointers: BTreeSet<String> = BTreeSet::new();
        for rule in kernel.rules_for(tier) {
            pointers.extend(rule.see.iter().cloned());
        }
        for moment in kernel.moments_for(tier) {
            pointers.extend(moment.see.iter().cloned());
        }
        pointers.extend(path_tokens(block));
        pointers.extend(skill_tokens(block));
        for reference in REFERENCES {
            assert!(
                pointers.contains(*reference),
                "{tier}: map never points at {reference}"
            );
        }

        let failures: Vec<String> = pointers
            .iter()
            .filter_map(|pointer| resolve(root, pointer).err())
            .collect();
        assert!(
            failures.is_empty(),
            "{tier} map has dangling pointers: {failures:?}"
        );
        if tier == Tier::Minimal {
            assert!(
                skill_tokens(block).is_empty() && !block.contains("project-management/"),
                "the minimal map must not advertise method machinery"
            );
        }
        let claude = read(&root.join("CLAUDE.md"));
        let claude_failures: Vec<String> = path_tokens(&claude)
            .into_iter()
            .chain(skill_tokens(&claude))
            .filter(|pointer| pointer.starts_with(".codeflow/") || is_skill_pointer(pointer))
            .filter_map(|pointer| resolve(root, &pointer).err())
            .collect();
        assert!(
            claude_failures.is_empty(),
            "{tier} CLAUDE.md dangling: {claude_failures:?}"
        );
    }
}

/// A realistic project section of `bytes` bytes: prose rules of the kind
/// adopters keep below the managed block.
fn project_section(bytes: usize) -> String {
    let paragraph = "- **Payments:** every change under `services/billing/` keeps the \
                     ledger double-entry invariant; run `make ledger-check` before push \
                     and attach its output to the PR. Card data never leaves the vault \
                     service, and fixtures use the provider's published test numbers.\n";
    let mut section = String::from("\n## Service conventions\n\n");
    while section.len() + paragraph.len() <= bytes {
        section.push_str(paragraph);
    }
    section
}

/// AC-5: with a 16 KiB project section the whole AGENTS.md stays within
/// Codex's 32 KiB limit at every tier.
#[test]
fn a_sixteen_kib_project_section_fits_under_the_codex_limit_at_every_tier() {
    let source = assets();
    for tier in Kernel::tiers() {
        let project = scaffold(&source, tier);
        let path = project.path().join("AGENTS.md");
        let mut text = read(&path);
        text.push_str(&project_section(16 * 1024));
        std::fs::write(&path, &text).unwrap();
        assert!(
            text.len() <= CODEX_INSTRUCTION_LIMIT_BYTES,
            "{tier}: AGENTS.md with a 16 KiB project section is {} bytes",
            text.len()
        );
    }
}

/// An asset source that serves the previous release's root contracts and
/// manifest shape over the current tree, so a project can be scaffolded from
/// the templates this task replaces.
struct Legacy {
    current: DirSource,
    overrides: BTreeMap<String, Option<Vec<u8>>>,
}

impl AssetSource for Legacy {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        match self.overrides.get(path) {
            Some(value) => value.clone(),
            None => self.current.read(path),
        }
    }
}

fn legacy_source() -> Legacy {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy-root-contracts");
    let current = assets();
    let manifest = String::from_utf8(current.read("base/scaffold-manifest.toml").unwrap()).unwrap();
    // The previous manifest: one AGENTS entry for standard and full, and no
    // references.
    let full_entry = "[[entry]]\nsrc = \"AGENTS.full.md.tmpl\"\ndest = \"AGENTS.md\"\nownership = \"managed-region\"\nregion = \"markdown\"\ntiers = [\"full\"]\ntemplate = true\n\n";
    assert!(
        manifest.contains(full_entry),
        "manifest shape changed; update the legacy shim"
    );
    let mut legacy = manifest.replace(full_entry, "").replace(
        "src = \"AGENTS.md.tmpl\"\ndest = \"AGENTS.md\"\nownership = \"managed-region\"\nregion = \"markdown\"\ntiers = [\"standard\"]",
        "src = \"AGENTS.md.tmpl\"\ndest = \"AGENTS.md\"\nownership = \"managed-region\"\nregion = \"markdown\"\ntiers = [\"standard\", \"full\"]",
    );
    for reference in REFERENCES {
        let name = reference.trim_start_matches(".codeflow/rules/");
        let entry = format!(
            "\n[[entry]]\nsrc = \"rules/{name}\"\ndest = \"{reference}\"\nownership = \"managed\"\ntiers = [\"minimal\", \"standard\", \"full\"]\n"
        );
        assert!(
            legacy.contains(&entry),
            "reference entry for {name} not found"
        );
        legacy = legacy.replace(&entry, "");
    }
    let mut overrides = BTreeMap::new();
    overrides.insert(
        "base/scaffold-manifest.toml".to_string(),
        Some(legacy.into_bytes()),
    );
    for name in [
        "AGENTS.md.tmpl",
        "AGENTS.minimal.md.tmpl",
        "CLAUDE.md.tmpl",
        "CLAUDE.minimal.md.tmpl",
    ] {
        let bytes = std::fs::read(fixtures.join(name)).expect("legacy fixture");
        overrides.insert(format!("base/{name}"), Some(bytes));
    }
    overrides.insert("base/AGENTS.full.md.tmpl".to_string(), None);
    Legacy { current, overrides }
}

fn split_at_block(text: &str) -> (&str, &str, &str) {
    let block = managed_block(text).expect("managed block");
    let start = text.find(block).unwrap();
    (&text[..start], block, &text[start + block.len()..])
}

/// AC-6: `codeflow update` on a project scaffolded from the previous
/// templates replaces the managed block, installs the references, and keeps
/// the project section and explicit values, at every tier.
#[test]
fn update_moves_an_existing_project_onto_the_map_at_every_tier() {
    let legacy = legacy_source();
    let current = assets();
    for tier in Kernel::tiers() {
        let project = scaffold(&legacy, tier);
        let root = project.path();
        let agents_path = root.join("AGENTS.md");
        let before = read(&agents_path);
        assert!(
            before.len() > 12 * 1024,
            "{tier}: the legacy fixture should be the old large contract"
        );
        assert!(!root.join(".codeflow/rules").exists());

        // The adopter's own section and explicit values.
        let section = project_section(16 * 1024);
        let header_edit = before.replacen(
            "A billing service for small clinics.",
            "A billing service for small clinics, run by the payments team.",
            1,
        );
        std::fs::write(&agents_path, format!("{header_edit}{section}")).unwrap();
        let policy_path = root.join(".codeflow/policy.json");
        let mut policy: serde_json::Value = serde_json::from_str(&read(&policy_path)).unwrap();
        policy["git"]["product_paths"] = serde_json::json!(["services/**"]);
        std::fs::write(&policy_path, serde_json::to_string_pretty(&policy).unwrap()).unwrap();
        let project_toml_before = read(&root.join(".codeflow/project.toml"));
        let (head_before, tail_before) = {
            let text = read(&agents_path);
            let (head, _, tail) = split_at_block(&text);
            (head.to_string(), tail.to_string())
        };

        let report = scaffold::update(
            &current,
            root,
            &UpdateOptions {
                force: false,
                binary_version: "3.0.0".to_string(),
                diff_out: None,
            },
        )
        .expect("update succeeds");
        let report_text = report.to_string();
        assert!(!report_text.contains("CONFLICT"), "{tier}: {report_text}");

        let after = read(&agents_path);
        let (head, block, tail) = split_at_block(&after);
        let fresh = scaffold(&current, tier);
        let fresh_agents = read(&fresh.path().join("AGENTS.md"));
        assert_eq!(
            block,
            managed_block(&fresh_agents).unwrap(),
            "{tier}: the managed block is not the new map"
        );
        assert_eq!(
            head, head_before,
            "{tier}: header outside the block changed"
        );
        assert_eq!(tail, tail_before, "{tier}: project section changed");
        assert!(tail.ends_with(&section), "{tier}: project section lost");
        assert!(
            after.len() <= CODEX_INSTRUCTION_LIMIT_BYTES,
            "{tier}: updated AGENTS.md is {} bytes",
            after.len()
        );
        for reference in REFERENCES {
            let installed = root.join(reference);
            assert!(
                installed.exists(),
                "{tier}: update did not install {reference}"
            );
            assert_eq!(
                read(&installed),
                read(
                    &repo_root()
                        .join("assets/base/rules")
                        .join(reference.trim_start_matches(".codeflow/rules/"))
                ),
                "{tier}: {reference} is not the shipped reference"
            );
        }
        assert_eq!(
            read(&root.join("CLAUDE.md")),
            read(&fresh.path().join("CLAUDE.md")),
            "{tier}: CLAUDE.md is not the new render"
        );
        let policy_after: serde_json::Value = serde_json::from_str(&read(&policy_path)).unwrap();
        assert_eq!(
            policy_after["git"]["product_paths"],
            serde_json::json!(["services/**"]),
            "{tier}: an explicit policy value was lost"
        );
        let project_toml = read(&root.join(".codeflow/project.toml"));
        for key in ["product_one_liner", "areas", "tier"] {
            let line = |text: &str| {
                text.lines()
                    .find(|line| line.starts_with(key))
                    .map(str::to_string)
            };
            assert_eq!(
                line(&project_toml),
                line(&project_toml_before),
                "{tier}: project.toml {key} changed"
            );
        }
        let rerun = scaffold::update(
            &current,
            root,
            &UpdateOptions {
                force: false,
                binary_version: "3.0.0".to_string(),
                diff_out: None,
            },
        )
        .expect("second update succeeds");
        assert_eq!(
            read(&agents_path),
            after,
            "{tier}: a second update changed AGENTS.md"
        );
        drop(rerun);
    }
}
