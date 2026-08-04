//! Architecture-fitness contract for the instruction artifacts `CodeFlow` loads
//! or ships. Byte ceilings bound context cost; semantic pins prevent a smaller
//! artifact from passing after deleting a duty.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use codeflow_core::scaffold::{DirSource, ScaffoldManifest, Tier};

const KIB: usize = 1024;
const ROOT_AGENTS_MAX_BYTES: usize = 32 * KIB;
const STANDARD_AGENTS_MAX_BYTES: usize = 30 * KIB;
const MINIMAL_AGENTS_MAX_BYTES: usize = 16 * KIB;
const STANDARD_CLAUDE_MAX_BYTES: usize = 6 * KIB;
const MINIMAL_CLAUDE_MAX_BYTES: usize = 3 * KIB;
const ROUTING_SKILL_MAX_BYTES: usize = 28 * KIB;
const OTHER_SKILL_MAX_BYTES: usize = 24 * KIB;
const REVIEWER_AGENT_MAX_BYTES: usize = 9 * KIB;
const SECURITY_REVIEWER_AGENT_MAX_BYTES: usize = 12 * KIB;

/// These skills own cross-lineage routing or orchestration mechanics and may
/// use the larger skill budget. Adding a name is a reviewed policy change, not
/// an automatic consequence of crossing the ordinary limit.
const ROUTING_SKILLS: &[&str] = &["cf-delegate", "cf-model-orchestrator"];

/// Every manifest-selected skill needs one reviewed ratchet. These values stay
/// close to the reviewed artifacts; the class limits below are backstops, not
/// growth allowances.
const SKILL_BYTE_RATCHETS: &[(&str, usize)] = &[
    ("agents/skills/cf-consult/SKILL.md", 6 * KIB),
    ("agents/skills/cf-customize/SKILL.md", 21 * KIB),
    // Raised from 15 KiB by the TSK-014 design-method recovery. The nine added
    // obligations (idea survival across applicable contexts, comprehension
    // channel separation, pre-authoring qualification, carrier feasibility,
    // recurrence harvest, non-browser platform evidence, compound-question
    // decomposition) keep only their trigger and rule here; every worked
    // explanation lives in the on-demand composition and audit references.
    // Still well inside the 24 KiB non-routing class ceiling.
    ("agents/skills/cf-design/SKILL.md", 17 * KIB),
    ("agents/skills/cf-develop/SKILL.md", 4 * KIB),
    ("agents/skills/cf-docs-portal/SKILL.md", 9 * KIB),
    ("agents/skills/cf-editorial-review/SKILL.md", 6 * KIB),
    ("agents/skills/cf-evaluate-model/SKILL.md", 9 * KIB),
    ("agents/skills/cf-model-orchestrator/SKILL.md", 27 * KIB),
    ("agents/skills/cf-plan/SKILL.md", 8 * KIB),
    ("agents/skills/cf-present/SKILL.md", 8 * KIB),
    ("agents/skills/cf-ship/SKILL.md", 6 * KIB),
    ("agents/skills/cf-stack/SKILL.md", 4 * KIB),
    ("claude/skills/cf-delegate/SKILL.md", 20 * KIB),
    ("claude/skills/cf-method/SKILL.md", 19 * KIB),
];

struct SkillBudgetException {
    source: &'static str,
    max_bytes: usize,
    evidence: &'static str,
}

/// An exception must name one exact manifest source and durable evidence for
/// why factoring content into on-demand resources would dilute the contract.
/// Keep this empty unless such evidence exists.
const SKILL_BUDGET_EXCEPTIONS: &[SkillBudgetException] = &[];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn read_text(path: &Path) -> String {
    String::from_utf8(read(path))
        .unwrap_or_else(|error| panic!("{} is not UTF-8: {error}", path.display()))
}

fn normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn authored_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"\r\n") {
            normalized.push(b'\n');
            index += 2;
        } else {
            normalized.push(bytes[index]);
            index += 1;
        }
    }
    normalized
}

fn assert_byte_budget(path: &Path, max_bytes: usize) -> usize {
    // Git may materialize tracked text with CRLF on Windows. Measure the
    // authored LF-normalized content so one reviewed budget has identical
    // meaning on every host; lone carriage returns and all other bytes still
    // count normally.
    let bytes = authored_bytes(&read(path));
    let diagnostic_lines = bytes.split_inclusive(|byte| *byte == b'\n').count();
    assert!(
        bytes.len() <= max_bytes,
        "{} is {} bytes (diagnostic: {} lines), above its {}-byte budget; \
         preserve every semantic duty and factor justified detail into an \
         on-demand resource or document a narrow exception—never game this \
         gate with dense formatting or deleted doctrine",
        path.display(),
        bytes.len(),
        diagnostic_lines,
        max_bytes
    );
    bytes.len()
}

#[test]
fn byte_budgets_are_independent_of_checkout_newlines() {
    let lf = b"first line\nsecond line\n";
    let crlf = b"first line\r\nsecond line\r\n";
    assert_eq!(authored_bytes(lf), authored_bytes(crlf));
    assert_eq!(authored_bytes(b"first\rline"), b"first\rline");
}

fn manifest() -> ScaffoldManifest {
    ScaffoldManifest::load(&DirSource::new(repo_root().join("assets")))
        .expect("shipped manifest loads")
}

fn selected_source(manifest: &ScaffoldManifest, dest: &str, tier: Tier) -> String {
    let selected: Vec<_> = manifest
        .entries
        .iter()
        .filter(|entry| entry.dest == dest && entry.applies(tier, "default"))
        .collect();
    assert_eq!(
        selected.len(),
        1,
        "exactly one {dest} source must apply to the {tier} tier"
    );
    selected[0].src.clone()
}

fn assert_contains_all(path: &Path, clauses: &[(&str, &str)]) {
    let content = normalized(&read_text(path));
    let missing: Vec<_> = clauses
        .iter()
        .filter_map(|(duty, needle)| (!content.contains(&normalized(needle))).then_some(*duty))
        .collect();
    assert!(
        missing.is_empty(),
        "{} is within its byte budget but lost required semantic duties: {}",
        path.display(),
        missing.join(", ")
    );
}

fn reviewed_skill_ratchets(manifest_sources: &BTreeSet<&str>) -> BTreeMap<&'static str, usize> {
    let ratchets: BTreeMap<_, _> = SKILL_BYTE_RATCHETS.iter().copied().collect();
    assert_eq!(
        ratchets.len(),
        SKILL_BYTE_RATCHETS.len(),
        "skill byte ratchets must name unique manifest sources"
    );
    let ratchet_sources: BTreeSet<_> = ratchets.keys().copied().collect();
    assert_eq!(
        &ratchet_sources, manifest_sources,
        "every manifest SKILL.md source needs exactly one reviewed byte ratchet; remove stale ratchets when a skill is removed"
    );
    ratchets
}

fn validated_skill_exceptions(
    root: &Path,
    ratchets: &BTreeMap<&'static str, usize>,
) -> BTreeMap<&'static str, &'static SkillBudgetException> {
    let exceptions: BTreeMap<_, _> = SKILL_BUDGET_EXCEPTIONS
        .iter()
        .map(|exception| {
            assert!(
                exception.evidence.starts_with("docs/"),
                "{} needs a durable docs/ evidence reference",
                exception.source
            );
            assert!(
                root.join(exception.evidence).is_file(),
                "{} evidence does not resolve to a repository file: {}",
                exception.source,
                exception.evidence
            );
            (exception.source, exception)
        })
        .collect();
    assert_eq!(
        exceptions.len(),
        SKILL_BUDGET_EXCEPTIONS.len(),
        "skill budget exceptions must name unique sources"
    );
    for source in exceptions.keys() {
        assert!(
            ratchets.contains_key(source),
            "stale skill budget exception for {source}"
        );
    }
    exceptions
}

#[test]
fn root_and_manifest_selected_contracts_obey_byte_budgets() {
    let root = repo_root();
    let manifest = manifest();
    let base = root.join("assets/base");

    assert_byte_budget(&root.join("AGENTS.md"), ROOT_AGENTS_MAX_BYTES);
    assert_byte_budget(&root.join("CLAUDE.md"), STANDARD_CLAUDE_MAX_BYTES);
    assert_byte_budget(
        &root.join(".codeflow/.baseline/AGENTS.md"),
        STANDARD_AGENTS_MAX_BYTES,
    );
    assert_byte_budget(
        &root.join(".codeflow/.baseline/CLAUDE.md"),
        STANDARD_CLAUDE_MAX_BYTES,
    );

    let standard_agents = selected_source(&manifest, "AGENTS.md", Tier::Standard);
    let full_agents = selected_source(&manifest, "AGENTS.md", Tier::Full);
    let minimal_agents = selected_source(&manifest, "AGENTS.md", Tier::Minimal);
    assert_eq!(
        standard_agents, full_agents,
        "standard and full must share the standard AGENTS contract"
    );
    assert_byte_budget(&base.join(standard_agents), STANDARD_AGENTS_MAX_BYTES);
    assert_byte_budget(&base.join(minimal_agents), MINIMAL_AGENTS_MAX_BYTES);

    let standard_claude = selected_source(&manifest, "CLAUDE.md", Tier::Standard);
    let full_claude = selected_source(&manifest, "CLAUDE.md", Tier::Full);
    let minimal_claude = selected_source(&manifest, "CLAUDE.md", Tier::Minimal);
    assert_eq!(
        standard_claude, full_claude,
        "standard and full must share the standard CLAUDE contract"
    );
    assert_byte_budget(&base.join(standard_claude), STANDARD_CLAUDE_MAX_BYTES);
    assert_byte_budget(&base.join(minimal_claude), MINIMAL_CLAUDE_MAX_BYTES);
}

#[test]
fn shipped_skills_and_managed_copies_obey_ratchets_and_class_ceilings() {
    let root = repo_root();
    let manifest = manifest();
    let base = root.join("assets/base");
    let skill_sources: BTreeSet<_> = manifest
        .entries
        .iter()
        .filter(|entry| entry.src.ends_with("/SKILL.md"))
        .map(|entry| entry.src.as_str())
        .collect();
    assert!(!skill_sources.is_empty(), "manifest must ship skills");

    let ratchets = reviewed_skill_ratchets(&skill_sources);
    let exceptions = validated_skill_exceptions(&root, &ratchets);

    for source in skill_sources {
        let skill_name = Path::new(source)
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .expect("skill source has a UTF-8 parent name");
        let class_max = if ROUTING_SKILLS.contains(&skill_name) {
            ROUTING_SKILL_MAX_BYTES
        } else {
            OTHER_SKILL_MAX_BYTES
        };
        let max_bytes = *ratchets
            .get(source)
            .expect("manifest and ratchet sources were proven equal");
        if let Some(exception) = exceptions.get(source) {
            assert_eq!(
                exception.max_bytes, max_bytes,
                "{source} exception and reviewed ratchet disagree"
            );
            assert!(
                max_bytes > class_max,
                "{source} does not need an exception at {max_bytes} bytes"
            );
        } else {
            assert!(
                max_bytes <= class_max,
                "{source} exceeds its {class_max}-byte class ceiling without a durable exception"
            );
        }
        let canonical = base.join(source);
        let canonical_bytes = read(&canonical);
        assert_byte_budget(&canonical, max_bytes);

        let destinations: BTreeSet<_> = manifest
            .entries
            .iter()
            .filter(|entry| entry.src == source)
            .map(|entry| entry.dest.as_str())
            .collect();
        assert_eq!(
            destinations.len(),
            2,
            "{source} must have exactly the Claude and Codex destinations"
        );
        for prefix in [".claude/skills/", ".agents/skills/"] {
            assert!(
                destinations.iter().any(|dest| dest.starts_with(prefix)),
                "{source} does not ship to {prefix}"
            );
        }

        for destination in destinations {
            for copy in [
                root.join(destination),
                root.join(".codeflow/.baseline").join(destination),
            ] {
                let copy_bytes = read(&copy);
                assert_eq!(
                    copy_bytes,
                    canonical_bytes,
                    "{} drifted from {}",
                    copy.display(),
                    canonical.display()
                );
                assert_byte_budget(&copy, max_bytes);
            }
        }
    }
}

#[test]
fn shipped_agent_definitions_obey_role_specific_budgets_and_match_managed_copies() {
    let root = repo_root();
    let manifest = manifest();
    let base = root.join("assets/base");
    let contracts = [
        (
            "claude/agents/cf-reviewer.md",
            REVIEWER_AGENT_MAX_BYTES,
            [
                "never write or fix code",
                "against its stated acceptance criteria",
                "whether a smaller, clearer solution meets the same criteria",
                "Require one faithful vertical run through every",
                "Material avoidable complexity or brittleness is major",
            ],
        ),
        (
            "claude/agents/cf-security-reviewer.md",
            SECURITY_REVIEWER_AGENT_MAX_BYTES,
            [
                "two independent-vendor models",
                "deterministic-scanner output",
                "Assume breach",
                "BLOCK when any finding",
                "Cross-vendor divergence escalates to the human at merge",
            ],
        ),
    ];

    for (source, max_bytes, clauses) in contracts {
        let entries: Vec<_> = manifest
            .entries
            .iter()
            .filter(|entry| entry.src == source)
            .collect();
        assert_eq!(entries.len(), 1, "{source} must have one manifest entry");
        let destination = entries[0].dest.as_str();
        assert!(
            destination.starts_with(".claude/agents/"),
            "{source} must install as a Claude agent definition"
        );

        let canonical = base.join(source);
        let canonical_bytes = read(&canonical);
        assert_byte_budget(&canonical, max_bytes);
        for copy in [
            root.join(destination),
            root.join(".codeflow/.baseline").join(destination),
        ] {
            assert_eq!(
                read(&copy),
                canonical_bytes,
                "{} drifted from {}",
                copy.display(),
                canonical.display()
            );
            assert_byte_budget(&copy, max_bytes);
        }

        let semantic_clauses: Vec<_> = clauses.iter().map(|clause| (*clause, *clause)).collect();
        assert_contains_all(&canonical, &semantic_clauses);
    }
}

#[test]
fn agents_byte_efficiency_cannot_delete_semantic_duties() {
    let root = repo_root();
    let standard_agents = [
        root.join("AGENTS.md"),
        root.join("assets/base/AGENTS.md.tmpl"),
        root.join(".codeflow/.baseline/AGENTS.md"),
    ];
    let standard_agent_clauses = [
        (
            "non-trivial orchestration route",
            "Every non-trivial repository task **must begin with**",
        ),
        (
            "independent dual planning",
            "both independently research/analyze/plan",
        ),
        ("legible peer degradation", "Missing seats degrade legibly"),
        ("worktree isolation", "Develop in a worktree per session"),
        ("materiality", "Find broadly; act by materiality"),
        (
            "durable implementation quality",
            "Make the smallest clear, idiomatic, durable",
        ),
        (
            "whole-surface verification",
            "integration, end-to-end, and user-facing behavior",
        ),
        (
            "evidence honesty",
            "Unverifiable or fabricated claims are defects (zero tolerance).",
        ),
        (
            "human safety authority",
            "explicit authenticated human approval",
        ),
        (
            "independent review",
            "Review verdicts come from an independent pass",
        ),
    ];
    for path in &standard_agents {
        assert_contains_all(path, &standard_agent_clauses);
    }

    assert_contains_all(
        &root.join("assets/base/AGENTS.minimal.md.tmpl"),
        &[
            ("secret protection", "**Secrets:** never stage credentials"),
            ("work-start identity", "**Work-start check:**"),
            ("proven cleanup", "**Post-landing cleanup:**"),
            ("materiality", "Find broadly; act by materiality"),
            (
                "durable implementation quality",
                "Make the smallest clear, idiomatic, durable",
            ),
            (
                "whole-surface verification",
                "integration, end-to-end, user-facing",
            ),
            (
                "evidence honesty",
                "Unverifiable or fabricated claims are defects (zero tolerance).",
            ),
            (
                "human safety authority",
                "explicit authenticated human approval",
            ),
        ],
    );
}

#[test]
fn claude_byte_efficiency_cannot_delete_semantic_duties() {
    let root = repo_root();
    for path in [
        root.join("CLAUDE.md"),
        root.join("assets/base/CLAUDE.md.tmpl"),
        root.join(".codeflow/.baseline/CLAUDE.md"),
    ] {
        assert_contains_all(
            &path,
            &[
                ("common authority", "@AGENTS.md"),
                ("routing gate", "`/cf-model-orchestrator`"),
                ("native Codex peer", "official Codex plugin"),
                ("qualified model bindings", "current-ensemble.json"),
                ("bounded parallelism", "bounded fan-out"),
            ],
        );
    }
    assert_contains_all(
        &root.join("assets/base/CLAUDE.minimal.md.tmpl"),
        &[
            ("common authority", "@AGENTS.md"),
            ("tier honesty", "no method machinery"),
            ("fail-closed sandbox", "fail-closed sandbox"),
            ("cross-harness boundary", "one harness's settings do not"),
            ("hook enforcement", "hook-enforced"),
        ],
    );
}

#[test]
fn orchestration_byte_efficiency_cannot_delete_semantic_duties() {
    let root = repo_root();
    assert_contains_all(
        &root.join("assets/base/agents/skills/cf-model-orchestrator/SKILL.md"),
        &[
            ("independent discovery", "**Both think independently.**"),
            ("Claude design lead", "**Claude leads design.**"),
            ("capability routing", "**Host routes execution.**"),
            (
                "producer-relative review",
                "**Review is producer-relative.**",
            ),
            (
                "evidence over consensus",
                "**Evidence outranks agreement.**",
            ),
            (
                "human safety authority",
                "**Catastrophic actions remain human-gated.**",
            ),
            ("native sessions", "**Native interactive sessions only.**"),
            ("bounded parallelism", "**Bounded parallelism.**"),
        ],
    );

    assert_contains_all(
        &root.join("docs/plan/v2/00-charter.md"),
        &[
            ("byte-gate authority", "Bytes are the deterministic gate"),
            (
                "line-count diagnostic only",
                "Line counts are diagnostic only",
            ),
            (
                "no semantic dilution",
                "A smaller artifact is not a better artifact",
            ),
            (
                "exception discipline",
                "evidence-backed exception naming the exact skill",
            ),
        ],
    );
}
