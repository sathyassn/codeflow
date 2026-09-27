//! The progressive reading map of the shipped skills (TSK-150).
//!
//! A session reads a small always-loaded kernel: the managed block of
//! `AGENTS.md`, rendered from `assets/base/rule-map.toml`, plus the
//! session-start digest. Everything else is reached through an index entry
//! or a reviewed trigger at the moment it is needed. This module owns that
//! structure, so the `artifact_budget_contract` test and `codeflow doctor`
//! share one implementation:
//!
//! - the per-task reading chain: what a Claude-host session is told to read
//!   for one full implementation task, walked from [`CHAIN_ENTRY_POINTS`];
//! - the inventory of conditional reads, each with the trigger text of the
//!   sentence that holds it ([`CONDITIONAL_READS`]);
//! - the whole reachable graph, audited apart from the chain: every skill
//!   entry and every conditional target, with each index row classified and
//!   every link resolved to a shipped file or a recorded project reference;
//! - the orphan check: every shipped Markdown or JSON file under a skill is
//!   reachable from that skill's `SKILL.md` or another reached file;
//! - the kernel check: the kernel names each chain entry point exactly;
//! - the guideline numbers sizes are reported against.
//!
//! Only active Markdown counts as a read: fenced code (backtick or tilde
//! fences) and HTML comments are examples or retired text, never a pointer.
//!
//! Structure is the test's failure. Size is a reported measure with a
//! guideline number, never a failure: the one byte check that still fails is
//! the complete generated `AGENTS.md` against Codex's instruction limit
//! ([`crate::scaffold::rule_map::codex_overflow`]).
//!
//! Paths are keyed as `<skill>/<path inside the skill>`, the layout both
//! installed trees (`.claude/skills/`, `.agents/skills/`) share and the two
//! source trees (`assets/base/agents/skills/`, `assets/base/claude/skills/`)
//! flatten into.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

const KIB: usize = 1024;

/// Shipped skill files keyed `<skill>/<path>`, with their text.
pub type SkillFiles = BTreeMap<String, String>;

// ---------------------------------------------------------------------------
// Guideline numbers
// ---------------------------------------------------------------------------

/// Guideline for the per-task reading chain: what one full implementation
/// task reads after the kernel. Reported, never a failure.
pub const READING_CHAIN_GUIDELINE_BYTES: usize = 150 * KIB;

/// Guideline for a skill that owns cross-lineage routing or orchestration.
pub const ROUTING_SKILL_GUIDELINE_BYTES: usize = 29 * KIB;

/// Guideline for every other skill without its own number.
pub const OTHER_SKILL_GUIDELINE_BYTES: usize = 24 * KIB;

/// The skills measured against [`ROUTING_SKILL_GUIDELINE_BYTES`].
pub const ROUTING_SKILLS: &[&str] = &["cf-delegate", "cf-model-orchestrator"];

/// Each shipped skill's guideline for its `SKILL.md`, the numbers doctor
/// reports against. Every shipped file sits within its number, so a fresh
/// install reports clean; a change that moves a shipped file past its number
/// sets the new number in the same change. A skill above its number is a
/// prompt to move detail behind a trigger, never to cut a duty.
pub const SKILL_GUIDELINES: &[(&str, usize)] = &[
    ("cf-consult", 7 * KIB),
    ("cf-customize", 22 * KIB),
    ("cf-delegate", 20 * KIB + 512),
    ("cf-design", 19 * KIB + 512),
    ("cf-develop", 5 * KIB),
    ("cf-docs-portal", 9 * KIB),
    ("cf-editorial-review", 6 * KIB),
    ("cf-estimate", 6 * KIB),
    ("cf-evaluate-model", 9 * KIB + 256),
    ("cf-herdr", 9 * KIB),
    ("cf-method", 19 * KIB + 512),
    ("cf-model-orchestrator", 29 * KIB),
    ("cf-plan", 9 * KIB),
    ("cf-present", 8 * KIB),
    ("cf-ship", 6 * KIB + 896),
    ("cf-stack", 4 * KIB),
];

/// Guidelines for the other shipped instruction files, keyed by their
/// source under `assets/base/`: the former class ceilings.
pub const ARTIFACT_GUIDELINES: &[(&str, usize)] = &[
    ("CLAUDE.md.tmpl", 6 * KIB),
    ("CLAUDE.minimal.md.tmpl", 3 * KIB),
    ("claude/agents/cf-reviewer.md", 10 * KIB),
    ("claude/agents/cf-security-reviewer.md", 12 * KIB),
];

/// The guideline for `skill`'s `SKILL.md`: its own number, else its class.
#[must_use]
pub fn skill_guideline(skill: &str) -> usize {
    SKILL_GUIDELINES
        .iter()
        .find(|(name, _)| *name == skill)
        .map_or_else(
            || {
                if ROUTING_SKILLS.contains(&skill) {
                    ROUTING_SKILL_GUIDELINE_BYTES
                } else {
                    OTHER_SKILL_GUIDELINE_BYTES
                }
            },
            |(_, bytes)| *bytes,
        )
}

/// One reported size: a subject, its authored bytes and its guideline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measure {
    pub subject: String,
    pub bytes: usize,
    pub guideline: usize,
}

impl Measure {
    /// Whether the measure is above its guideline.
    #[must_use]
    pub fn over(&self) -> bool {
        self.bytes > self.guideline
    }
}

impl std::fmt::Display for Measure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} of {} bytes",
            self.subject, self.bytes, self.guideline
        )
    }
}

/// Authored bytes: LF-normalized, so a Windows checkout that materializes
/// CRLF measures the same as every other host. Lone carriage returns and all
/// other bytes count normally.
#[must_use]
pub fn authored_len(bytes: &[u8]) -> usize {
    bytes.len() - bytes.windows(2).filter(|pair| pair == b"\r\n").count()
}

/// Each `<skill>/SKILL.md` in `files` measured against its guideline, in
/// skill-name order.
#[must_use]
pub fn skill_measures(files: &SkillFiles) -> Vec<Measure> {
    files
        .iter()
        .filter_map(|(path, text)| {
            let skill = path.strip_suffix("/SKILL.md")?;
            (!skill.contains('/')).then(|| Measure {
                subject: skill.to_string(),
                bytes: authored_len(text.as_bytes()),
                guideline: skill_guideline(skill),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The reading inventory
// ---------------------------------------------------------------------------

/// Where the per-task chain starts: the skills and files the kernel names
/// for one full implementation task on a Claude host, each exactly as the
/// kernel names it ([`kernel_entries`]).
/// - orient and route: the routing rule invokes `cf-model-orchestrator`, and
///   the kernel says to read and follow `cf-method`'s workflow lifecycle
///   reference (it names that file, never the `cf-method` skill itself);
/// - before launch: `CLAUDE.md` names `current-ensemble.json`, and the
///   orchestrator names `cf-delegate`;
/// - plan, build and ship: `cf-plan`, `cf-develop` and `cf-ship`.
pub const CHAIN_ENTRY_POINTS: &[(&str, &str)] = &[
    ("orient and route", "cf-model-orchestrator/SKILL.md"),
    (
        "orient and route",
        "cf-method/references/workflow-lifecycle.md",
    ),
    (
        "before launch",
        "cf-model-orchestrator/resources/current-ensemble.json",
    ),
    ("before launch", "cf-delegate/SKILL.md"),
    ("plan", "cf-plan/SKILL.md"),
    ("build", "cf-develop/SKILL.md"),
    ("ship", "cf-ship/SKILL.md"),
];

/// A reviewed conditional read: the sentence in `from` that names `to` must
/// contain `trigger`, the moment the read happens.
#[derive(Debug, Clone, Copy)]
pub struct ConditionalRead {
    pub from: &'static str,
    pub to: &'static str,
    pub trigger: &'static str,
    pub reason: &'static str,
}

const fn conditional(
    from: &'static str,
    to: &'static str,
    trigger: &'static str,
    reason: &'static str,
) -> ConditionalRead {
    ConditionalRead {
        from,
        to,
        trigger,
        reason,
    }
}

const ORCH_SKILL: &str = "cf-model-orchestrator/SKILL.md";
const QUALITY: &str = "cf-model-orchestrator/resources/quality-contract.md";
const ROUTING: &str = "cf-model-orchestrator/resources/capability-routing.md";
const DELEGATE: &str = "cf-delegate/SKILL.md";
const TURN_ADAPTER: &str = "cf-delegate/resources/claude-turn-completion.md";
const LIFECYCLE_LANE: &str = "cf-delegate/resources/lane-lifecycle.md";
const TASK_GRAPH: &str = "cf-model-orchestrator/resources/task-graph.md";
const IRREVERSIBLE: &str = "cf-model-orchestrator/resources/quality/irreversible.md";
const FINDINGS: &str = "cf-model-orchestrator/resources/quality/findings.md";
const UI_DESIGN: &str = "cf-model-orchestrator/resources/quality/ui-design.md";
const PERFORMANCE: &str = "cf-model-orchestrator/resources/quality/performance.md";
const RESEARCH: &str = "cf-model-orchestrator/resources/quality/research-planning.md";
const BLOCKERS: &str = "cf-model-orchestrator/resources/quality/blockers-and-gates.md";
const ROUTE_STATUS: &str = "cf-model-orchestrator/resources/routing/route-status.md";
const ROUTING_DESIGN: &str = "cf-model-orchestrator/resources/routing/design.md";
const OVERRIDES: &str = "cf-model-orchestrator/references/model-overrides.md";
const ORGANIZATION: &str = "cf-method/references/project-organization.md";

/// Reads outside the per-task chain, each with its trigger and reason.
pub const CONDITIONAL_READS: &[ConditionalRead] = &[
    conditional(
        ORCH_SKILL,
        OVERRIDES,
        "If `.codeflow/model-selection.json` contains project overrides",
        "only when the project has model overrides",
    ),
    conditional(
        ORCH_SKILL,
        TASK_GRAPH,
        "For a multi-task plan or a possible dependency/decision change",
        "only for a multi-task plan",
    ),
    conditional(
        ORCH_SKILL,
        TASK_GRAPH,
        "Multi-task plans use",
        "only for a multi-task plan",
    ),
    conditional(
        ORCH_SKILL,
        IRREVERSIBLE,
        "high-blast-radius action stops the host and follows",
        "only before a catastrophic or irreversible action",
    ),
    conditional(
        ORCH_SKILL,
        "cf-model-orchestrator/references/other-hosts.md",
        "| Grok Build, or another harness including Hermes |",
        "only on a Grok Build or other host",
    ),
    conditional(
        ORCH_SKILL,
        "cf-model-orchestrator/references/other-hosts.md",
        "Grok, when a Grok seat is used",
        "only when a Grok seat is used",
    ),
    // TSK-150 (audit row H24): the Grok host detail is read before a Grok
    // preflight or launch.
    conditional(
        "cf-model-orchestrator/references/other-hosts.md",
        "cf-model-orchestrator/resources/grok-host.md",
        "Before a Grok preflight or launch, also read",
        "only before a Grok preflight or launch",
    ),
    conditional(
        ORCH_SKILL,
        "cf-model-orchestrator/references/estimates.md",
        "When the brief concerns agentic estimates, capacity or deadlines",
        "only when the brief concerns estimates, capacity or deadlines",
    ),
    conditional(
        ORCH_SKILL,
        TURN_ADAPTER,
        "On a Codex, Grok or other non-Claude host, before every Claude worker",
        "the turn adapter is read on a host that launches Claude through the \
         delegated lifecycle",
    ),
    conditional(
        ORCH_SKILL,
        "cf-model-orchestrator/references/solo-fallback.md",
        "A solo `/cf-develop` run follows",
        "only when preflight leaves a required seat unavailable",
    ),
    conditional(
        ORCH_SKILL,
        "cf-model-orchestrator/references/parallel-tasks.md",
        "For independent parallel tasks",
        "only when implementation has independent parallel tasks",
    ),
    conditional(
        ORCH_SKILL,
        "cf-model-orchestrator/references/codex-host.md",
        "A Codex host runs this test-running review",
        "the test-running review detail is read on a Codex host",
    ),
    conditional(
        "cf-method/SKILL.md",
        ORGANIZATION,
        "for new-project boundary choices, brownfield adoption, monorepos",
        "only for project-organization choices; an obvious bounded task needs none",
    ),
    conditional(
        "cf-method/SKILL.md",
        "cf-method/references/skill-authoring.md",
        "When authoring or editing a skill",
        "only when authoring or editing a skill",
    ),
    conditional(
        DELEGATE,
        LIFECYCLE_LANE,
        "A Codex host follows its host and canary rules",
        "the lifecycle lane is read on a Codex host",
    ),
    conditional(
        DELEGATE,
        LIFECYCLE_LANE,
        "**From codex:**",
        "the lifecycle lane is read on a Codex host",
    ),
    conditional(
        DELEGATE,
        LIFECYCLE_LANE,
        "**From codex (Codex host):**",
        "the lifecycle lane is read on a Codex host",
    ),
    conditional(
        DELEGATE,
        "cf-delegate/resources/native-fallback.md",
        "For an incompatible or unavailable preferred lane",
        "only when the preferred lane is unavailable",
    ),
    conditional(
        DELEGATE,
        "cf-delegate/resources/edit-access.md",
        "Before any write-enabled handoff",
        "only before a write-enabled handoff",
    ),
    conditional(
        DELEGATE,
        "cf-delegate/resources/agy.md",
        "when `agy` is someone's harness",
        "only when `agy` is someone's harness",
    ),
    conditional(
        "cf-delegate/resources/lane-plugin.md",
        LIFECYCLE_LANE,
        "A Codex host uses",
        "the lifecycle lane is read on a Codex host",
    ),
    conditional(
        "cf-delegate/resources/lane-plugin.md",
        OVERRIDES,
        "With project model overrides",
        "only when the project has model overrides",
    ),
    conditional(
        "cf-plan/SKILL.md",
        ORGANIZATION,
        "When work spans areas/teams",
        "only when work spans areas, boundaries or an external method",
    ),
    conditional(
        "cf-plan/SKILL.md",
        TASK_GRAPH,
        "For CodeFlow multi-task work",
        "only for a multi-task plan",
    ),
    conditional(
        ROUTING,
        ROUTE_STATUS,
        "| when a route is being qualified, or a claim of scoped qualification, promotion or savings is made |",
        "only when a route is qualified or a qualification, promotion or savings claim is made",
    ),
    conditional(
        ROUTING,
        ROUTING_DESIGN,
        "| when the task has product, UX, UI, interaction, or visual design work |",
        "only for product, UX, UI, interaction, or visual design work",
    ),
    conditional(
        QUALITY,
        BLOCKERS,
        "| when a step is blocked or would depart from what was approved, or a check or CI job is red or did not finish |",
        "only when a step is blocked or would depart from what was approved (TSK-131 \
         added the departure rule), or a check is red or unfinished",
    ),
    conditional(
        QUALITY,
        "cf-model-orchestrator/resources/quality/parallel.md",
        "| when work fans out into parallel tasks |",
        "only when work fans out into parallel tasks",
    ),
    conditional(
        QUALITY,
        "cf-model-orchestrator/resources/quality/editorial.md",
        "| when substantial prose is written, or its presentation is reviewed |",
        "only for substantial prose or a review of its presentation",
    ),
    conditional(
        QUALITY,
        UI_DESIGN,
        "| when a user-facing surface or its design intent changes |",
        "only when a user-facing surface or its design intent changes",
    ),
    conditional(
        QUALITY,
        IRREVERSIBLE,
        "| when an action is catastrophic or irreversible |",
        "only before a catastrophic or irreversible action",
    ),
    conditional(
        QUALITY,
        PERFORMANCE,
        "| when a changed path is performance-, scale-, or concurrency-sensitive |",
        "only for a performance-, scale-, or concurrency-sensitive path",
    ),
    conditional(
        QUALITY,
        RESEARCH,
        "| when the run is research, analysis or planning only |",
        "only for a research, analysis or planning-only run",
    ),
    conditional(
        "cf-model-orchestrator/resources/routing/assignment.md",
        ROUTE_STATUS,
        "Qualifying a route, or claiming scoped qualification",
        "only when a route is qualified or a qualification, promotion or savings claim is made",
    ),
    conditional(
        "cf-model-orchestrator/resources/routing/effort.md",
        TURN_ADAPTER,
        "On a Codex, Grok or other non-Claude host, before launching a Claude worker",
        "the turn adapter is read on a host that launches Claude through the \
         delegated lifecycle",
    ),
    conditional(
        "cf-model-orchestrator/resources/routing/hosts.md",
        ROUTING_DESIGN,
        "when a task has product, UX, UI, interaction, or visual design work",
        "only for product, UX, UI, interaction, or visual design work",
    ),
    conditional(
        "cf-model-orchestrator/resources/quality/plan.md",
        TASK_GRAPH,
        "For multi-task work, `TASK_GRAPH` follows",
        "only for a multi-task plan",
    ),
    conditional(
        "cf-model-orchestrator/resources/quality/plan.md",
        UI_DESIGN,
        "holds its full rule when a user-facing surface changes",
        "only when a user-facing surface or its design intent changes",
    ),
    conditional(
        "cf-model-orchestrator/resources/quality/plan.md",
        RESEARCH,
        "Research, analysis and planning-only runs fill the remaining fields",
        "only for a research, analysis or planning-only run",
    ),
    conditional(
        "cf-model-orchestrator/resources/quality/authority.md",
        IRREVERSIBLE,
        "Before a catastrophic or irreversible action",
        "only before a catastrophic or irreversible action",
    ),
    conditional(
        "cf-model-orchestrator/resources/quality/verification.md",
        PERFORMANCE,
        "For a performance-, scale-, or concurrency-sensitive path",
        "only for a performance-, scale-, or concurrency-sensitive path",
    ),
    conditional(
        "cf-model-orchestrator/resources/quality/verification.md",
        UI_DESIGN,
        "If no user-facing surface changed",
        "only when a user-facing surface or its design intent changes",
    ),
    conditional(
        "cf-model-orchestrator/resources/quality/completion.md",
        BLOCKERS,
        "with redness classified as in",
        "only when a check is red or unfinished",
    ),
    conditional(
        "cf-model-orchestrator/resources/quality/completion.md",
        IRREVERSIBLE,
        "every catastrophic action, if any",
        "only before a catastrophic or irreversible action",
    ),
    conditional(
        "cf-ship/references/pr-evidence.md",
        "cf-ship/references/release-policy.md",
        "when the impact may be minor or major or is disputed, when the PR carries \
         version or release-note updates, when the project has no adopted release \
         process, and before publication",
        "only for a minor, major or disputed impact, release preparation, a missing \
         release process, or publication; every PR's impact rules sit in PR evidence",
    ),
    // TSK-131: the holistic-fix doctrine and the review rounds load when a
    // defect is fixed or review findings are briefed, written or acted on.
    conditional(
        QUALITY,
        FINDINGS,
        "| when a defect is fixed, or review findings are briefed, written or acted on |",
        "only when a defect is fixed or review findings are briefed, written or acted on",
    ),
    conditional(
        ORCH_SKILL,
        FINDINGS,
        "When review findings are acted on",
        "only when review findings are acted on",
    ),
    conditional(
        ORCH_SKILL,
        FINDINGS,
        "Any confirmed issue returns to its responsible primary",
        "only when review confirms an issue",
    ),
    conditional(
        "cf-develop/SKILL.md",
        FINDINGS,
        "On `changes_requested`",
        "only when a review returns changes requested",
    ),
    conditional(
        "cf-develop/SKILL.md",
        FINDINGS,
        "When the change fixes a defect",
        "only when the change fixes a defect",
    ),
];

/// References that name project files, not shipped instructions: an
/// adopter's docs, settings, records and runtime files, read as the task
/// needs them and never measured. Every other reference in the reachable
/// graph must resolve to a shipped file.
pub const PROJECT_REFERENCES: &[&str] = &[
    ".codeflow/model-selection.json",
    ".codeflow/docs-portal.json",
    "docs/product.md",
    "product.md",
    "docs/capabilities.md",
    "capabilities.md",
    "docs/capabilities/CAP-*.md",
    "docs/architecture.md",
    "docs/architecture/<area>.md",
    "docs/decisions/template.md",
    // Named by skills and references off the per-task chain (TSK-150 graph
    // audit): project settings, records and runtime files, never shipped
    // instructions.
    "AGENTS.md",
    "CLAUDE.md",
    "TASK.md",
    "index.md",
    "portal.config.json",
    "epics/EPC-NNN.md",
    "specs/SPC-NNN.md",
    "tasks/TSK-NNN.md",
    ".claude/settings.json",
    ".grok/hooks/codeflow.json",
    ".codeflow/estimate.json",
    ".codeflow/manifest.json",
    ".codeflow/policy.json",
    ".codeflow/test-config.json",
    ".codeflow/schemas/present/document-v1.schema.json",
    ".codeflow/schemas/present/utility-tokens-v1.schema.json",
    "${CODEFLOW_HOME:-$HOME/.codeflow}/herdr-runs/<repo>.json",
    "DIR/settings.json",
    "result.json",
    "turns/<turn>/continuations/<task-id>/accepted.json",
    "~/.codex/auth.json",
    "~/.gemini/config/hooks.json",
];

/// The inventory the chain walk checks against. The shipped one is
/// [`Inventory::SHIPPED`]; a test passes a changed copy to prove a fault
/// fails.
#[derive(Debug, Clone, Copy)]
pub struct Inventory<'a> {
    pub entry_points: &'a [(&'a str, &'a str)],
    pub conditional: &'a [ConditionalRead],
    pub project: &'a [&'a str],
}

impl Inventory<'static> {
    /// The shipped inventory.
    pub const SHIPPED: Self = Self {
        entry_points: CHAIN_ENTRY_POINTS,
        conditional: CONDITIONAL_READS,
        project: PROJECT_REFERENCES,
    };
}

// ---------------------------------------------------------------------------
// Loading and parsing
// ---------------------------------------------------------------------------

/// Add every UTF-8 file under the skill tree `dir` to `files`, keyed by its
/// path under `dir`. Unreadable entries and non-UTF-8 files are skipped;
/// symlinked directories are not followed.
pub fn load_skill_tree(dir: &Path, files: &mut SkillFiles) {
    load_under(dir, dir, files);
}

fn load_under(base: &Path, dir: &Path, files: &mut SkillFiles) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            load_under(base, &path, files);
        } else if kind.is_file() {
            if let (Ok(text), Ok(relative)) =
                (std::fs::read_to_string(&path), path.strip_prefix(base))
            {
                let key = relative.to_string_lossy().replace('\\', "/");
                files.insert(key, text);
            }
        }
    }
}

fn has_extension(path: &str, extension: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|found| found.eq_ignore_ascii_case(extension))
}

/// Whether `path` is a reading file: Markdown or JSON.
fn is_reading_file(path: &str) -> bool {
    has_extension(path, "md") || has_extension(path, "json")
}

fn normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The sentences of `text`, whitespace-normalized: split on blank lines,
/// then after each ". ".
fn sentences(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for paragraph in text.split("\n\n") {
        let paragraph = normalized(paragraph);
        for sentence in paragraph.split_inclusive(". ") {
            found.push(sentence.trim().to_string());
        }
    }
    found
}

/// Link targets and backticked paths ending in `.md` or `.json`.
fn path_references(sentence: &str) -> Vec<String> {
    let is_path = |target: &str| {
        !target.is_empty()
            && !target.chars().any(char::is_whitespace)
            && !target.contains("://")
            && is_reading_file(target)
    };
    let mut found = Vec::new();
    let mut rest = sentence;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        let end = after.find(')').unwrap_or(after.len());
        let target = after[..end].split('#').next().unwrap_or_default();
        if is_path(target) {
            found.push(target.to_string());
        }
        rest = &after[end..];
    }
    for (index, span) in sentence.split('`').enumerate() {
        if index % 2 == 1 && is_path(span) {
            found.push(span.to_string());
        }
    }
    found
}

/// The opening or closing fence a line carries: its character (backtick or
/// tilde) and length, at least three.
fn fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let first = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let length = trimmed.chars().take_while(|c| *c == first).count();
    (length >= 3).then_some((first, length))
}

/// The active Markdown of `text`, one output line per input line: fenced
/// code blocks and HTML comments are blanked, so an example or a retired
/// pointer is never a read. A fence closes only on the same character, at
/// least as long, with nothing after it; an unclosed fence runs to the end.
fn active_markdown(text: &str) -> Vec<String> {
    let mut active = Vec::new();
    let mut open: Option<(char, usize)> = None;
    let mut in_comment = false;
    for line in text.lines() {
        if let Some((character, length)) = open {
            let closes = fence(line).is_some_and(|(c, l)| {
                c == character
                    && l >= length
                    && line.trim_start()[l * c.len_utf8()..].trim().is_empty()
            });
            if closes {
                open = None;
            }
            active.push(String::new());
            continue;
        }
        if !in_comment {
            if let Some(found) = fence(line) {
                open = Some(found);
                active.push(String::new());
                continue;
            }
        }
        let mut kept = String::new();
        let mut rest = line;
        loop {
            if in_comment {
                let Some(end) = rest.find("-->") else {
                    break;
                };
                rest = &rest[end + 3..];
                in_comment = false;
            } else {
                let Some(start) = rest.find("<!--") else {
                    kept.push_str(rest);
                    break;
                };
                kept.push_str(&rest[..start]);
                rest = &rest[start + 4..];
                in_comment = true;
            }
        }
        active.push(kept);
    }
    active
}

/// Active Markdown, split into sentences within each paragraph, list item
/// and table row.
fn reference_sentences(text: &str) -> Vec<String> {
    let active = active_markdown(text);
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in &active {
        let trimmed = line.trim_start();
        let item = trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("| ")
            || trimmed
                .split_once(". ")
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if trimmed.is_empty() || item {
            blocks.push(std::mem::take(&mut current));
        }
        if !trimmed.is_empty() {
            current.push(line.as_str());
        }
    }
    blocks.push(current);
    blocks
        .iter()
        .filter(|block| !block.is_empty())
        .flat_map(|block| sentences(&block.join("\n")))
        .collect()
}

fn join_path(dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

/// The shipped file `target` names from `source`, as an installed project
/// resolves it: relative to the file, to its skill, to the skill trees
/// (with or without an installed `.claude/skills/` or `.agents/skills/`
/// prefix), then a unique path suffix. An ambiguous suffix is an error.
fn resolve_reference(
    files: &SkillFiles,
    source: &str,
    target: &str,
) -> Result<Option<String>, String> {
    let dir = source.rsplit_once('/').map_or("", |(dir, _)| dir);
    let skill = source.split('/').next().unwrap_or_default();
    let mut candidates = vec![join_path(dir, target), join_path(skill, target)];
    for installed in [".claude/skills/", ".agents/skills/"] {
        if let Some(rest) = target.strip_prefix(installed) {
            candidates.push(join_path("", rest));
        }
    }
    candidates.push(join_path("", target));
    if let Some(found) = candidates.into_iter().find(|c| files.contains_key(c)) {
        return Ok(Some(found));
    }
    let suffix = format!("/{target}");
    let matches: Vec<&String> = files.keys().filter(|k| k.ends_with(&suffix)).collect();
    match matches.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).clone())),
        several => Err(format!(
            "{source} names `{target}`, which matches several shipped files: {several:?}"
        )),
    }
}

/// Every reading edge out of the Markdown file `path`: the sentence that
/// holds it and the target it names, as written.
fn edges(text: &str) -> Vec<(String, String)> {
    reference_sentences(text)
        .into_iter()
        .flat_map(|sentence| {
            path_references(&sentence)
                .into_iter()
                .map(move |target| (sentence.clone(), target))
                .collect::<Vec<_>>()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The per-task chain
// ---------------------------------------------------------------------------

/// One file in the per-task chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainFile {
    pub stage: String,
    pub path: String,
    pub bytes: usize,
}

/// The per-task reading chain and every structural fault found on the way.
#[derive(Debug, Clone, Default)]
pub struct ReadingChain {
    pub files: Vec<ChainFile>,
    pub total: usize,
    /// Unreviewed or stale edges, missing files and conditional reads without
    /// a trigger. Empty when the structure holds.
    pub errors: Vec<String>,
}

impl ReadingChain {
    /// The chain total against [`READING_CHAIN_GUIDELINE_BYTES`].
    #[must_use]
    pub fn measure(&self) -> Measure {
        Measure {
            subject: "per-task reading chain".to_string(),
            bytes: self.total,
            guideline: READING_CHAIN_GUIDELINE_BYTES,
        }
    }
}

/// Faults in the inventory itself: a conditional read without a trigger or
/// without a reason.
fn inventory_faults(inventory: &Inventory<'_>) -> Vec<String> {
    let mut faults = Vec::new();
    for read in inventory.conditional {
        if read.trigger.trim().is_empty() {
            faults.push(format!(
                "conditional read {} -> {} names no trigger",
                read.from, read.to
            ));
        }
        if read.reason.trim().is_empty() {
            faults.push(format!(
                "conditional read {} -> {} gives no reason",
                read.from, read.to
            ));
        }
    }
    faults
}

/// The reviewed conditional reads for the edge `from` to `to`.
fn recorded<'a>(
    inventory: &'a Inventory<'_>,
    from: &str,
    to: &str,
) -> Vec<(usize, &'a ConditionalRead)> {
    inventory
        .conditional
        .iter()
        .enumerate()
        .filter(|(_, read)| read.from == from && read.to == to)
        .collect()
}

/// The recorded read whose trigger `sentence` carries, if any.
fn reviewed_read(recorded: &[(usize, &ConditionalRead)], sentence: &str) -> Option<usize> {
    recorded.iter().find_map(|(index, read)| {
        let trigger = normalized(read.trigger);
        (!trigger.is_empty() && sentence.contains(&trigger)).then_some(*index)
    })
}

fn untriggered(path: &str, resolved: &str, sentence: &str) -> String {
    format!(
        "{path} names {resolved} without a reviewed trigger; \
         review the read and its conditional-read entry: {sentence}"
    )
}

/// Walk the per-task chain in `files` from the inventory's entry points. An
/// edge is required unless the inventory records it as conditional with the
/// trigger text of the sentence that holds it; a required edge adds its
/// target to the chain. So a new pointer is counted and a changed trigger
/// fails until reviewed. The chain is the mandatory reading and is measured;
/// [`graph_faults`] audits everything else a session can reach.
#[must_use]
pub fn reading_chain(files: &SkillFiles, inventory: &Inventory<'_>) -> ReadingChain {
    let mut errors = inventory_faults(inventory);
    let mut conditional_targets = BTreeSet::new();
    let mut chain: Vec<(String, String)> = Vec::new();
    let mut queue: VecDeque<(String, String)> = inventory
        .entry_points
        .iter()
        .map(|(stage, path)| ((*stage).to_string(), (*path).to_string()))
        .collect();
    while let Some((stage, path)) = queue.pop_front() {
        if chain.iter().any(|(_, p)| *p == path) {
            continue;
        }
        let Some(text) = files.get(&path) else {
            errors.push(format!("{path} is in the chain but is not shipped"));
            continue;
        };
        chain.push((stage.clone(), path.clone()));
        if !has_extension(&path, "md") {
            continue;
        }
        for (sentence, target) in edges(text) {
            let resolved = match resolve_reference(files, &path, &target) {
                Ok(Some(resolved)) => resolved,
                Ok(None) => continue,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            let recorded = recorded(inventory, &path, &resolved);
            if recorded.is_empty() {
                queue.push_back((stage.clone(), resolved));
            } else if reviewed_read(&recorded, &sentence).is_some() {
                conditional_targets.insert(resolved);
            } else {
                errors.push(untriggered(&path, &resolved, &sentence));
            }
        }
    }
    for target in &conditional_targets {
        if chain.iter().any(|(_, p)| p == target) {
            errors.push(format!(
                "{target} is recorded as conditional but a required edge reaches it"
            ));
        }
    }
    let files: Vec<ChainFile> = chain
        .into_iter()
        .map(|(stage, path)| {
            let bytes = authored_len(files[&path].as_bytes());
            ChainFile { stage, path, bytes }
        })
        .collect();
    let total = files.iter().map(|file| file.bytes).sum();
    ReadingChain {
        files,
        total,
        errors,
    }
}

// ---------------------------------------------------------------------------
// The whole reachable graph
// ---------------------------------------------------------------------------

/// The load classification that makes an index row a required read.
const EVERY_TASK: &str = "every task";

/// An index row: a two-cell table row whose first cell is exactly one link
/// to a reading file. Returns the link target and the classification cell.
fn index_row(line: &str) -> Option<(String, String)> {
    let row = line.trim().strip_prefix('|')?.strip_suffix('|')?;
    let cells: Vec<&str> = row.split('|').map(str::trim).collect();
    let [section, classification] = cells.as_slice() else {
        return None;
    };
    let (_, rest) = section.strip_prefix('[')?.split_once("](")?;
    let target = rest.strip_suffix(')')?;
    let target = target.split('#').next().unwrap_or_default();
    is_reading_file(target).then(|| (target.to_string(), (*classification).to_string()))
}

/// Index rows in `path`: each needs a classification, and one other than
/// "every task" needs a reviewed conditional read, so an empty or new
/// classification is never silently read as required.
fn index_faults(files: &SkillFiles, inventory: &Inventory<'_>, path: &str) -> Vec<String> {
    let mut faults = Vec::new();
    for line in active_markdown(&files[path]) {
        let Some((target, classification)) = index_row(&line) else {
            continue;
        };
        let Ok(Some(resolved)) = resolve_reference(files, path, &target) else {
            continue;
        };
        if classification.is_empty() {
            faults.push(format!(
                "{path}: the index row for {resolved} has no load classification"
            ));
        } else if classification != EVERY_TASK && recorded(inventory, path, &resolved).is_empty() {
            faults.push(format!(
                "{path}: the index row for {resolved} is classified `{classification}` \
                 but no reviewed conditional read records it"
            ));
        }
    }
    faults
}

/// Audit everything a session can reach, apart from measuring the chain:
/// every skill entry, the chain entry points and every conditional target
/// below them. Each link resolves to a shipped file or a recorded project
/// reference, each recorded conditional read carries its trigger, each index
/// row is classified, and no recorded read or project reference is stale.
#[must_use]
pub fn graph_faults(files: &SkillFiles, inventory: &Inventory<'_>) -> Vec<String> {
    let mut faults = Vec::new();
    let mut used = vec![false; inventory.conditional.len()];
    let mut used_project = BTreeSet::new();
    let mut reached: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = skill_entries(files)
        .chain(
            inventory
                .entry_points
                .iter()
                .map(|(_, path)| (*path).to_string()),
        )
        .collect();
    while let Some(path) = queue.pop_front() {
        if !reached.insert(path.clone()) || !has_extension(&path, "md") {
            continue;
        }
        let Some(text) = files.get(&path) else {
            continue;
        };
        faults.extend(index_faults(files, inventory, &path));
        for (sentence, target) in edges(text) {
            let resolved = match resolve_reference(files, &path, &target) {
                Ok(Some(resolved)) => resolved,
                Ok(None) => {
                    if inventory.project.contains(&target.as_str()) {
                        used_project.insert(target);
                    } else {
                        faults.push(format!(
                            "{path} names `{target}`, which is no shipped file; \
                             record it as a project reference if it is a project file"
                        ));
                    }
                    continue;
                }
                Err(error) => {
                    faults.push(error);
                    continue;
                }
            };
            let recorded = recorded(inventory, &path, &resolved);
            if !recorded.is_empty() {
                match reviewed_read(&recorded, &sentence) {
                    Some(index) => used[index] = true,
                    None => faults.push(untriggered(&path, &resolved, &sentence)),
                }
            }
            queue.push_back(resolved);
        }
    }
    for (read, used) in inventory.conditional.iter().zip(&used) {
        if !used && !read.trigger.trim().is_empty() {
            faults.push(format!(
                "stale conditional read {} -> {} (trigger `{}`)",
                read.from, read.to, read.trigger
            ));
        }
    }
    for reference in inventory.project {
        if !used_project.contains(*reference) {
            faults.push(format!("stale project reference `{reference}`"));
        }
    }
    faults
}

/// Every `<skill>/SKILL.md` in `files`: the index entries the harness loads
/// by their descriptions.
fn skill_entries(files: &SkillFiles) -> impl Iterator<Item = String> + '_ {
    files
        .keys()
        .filter(|path| {
            path.strip_suffix("/SKILL.md")
                .is_some_and(|skill| !skill.contains('/'))
        })
        .cloned()
}

// ---------------------------------------------------------------------------
// Orphans
// ---------------------------------------------------------------------------

/// Every Markdown or JSON file in `files` that no active reading edge
/// reaches. Each `<skill>/SKILL.md` is an index entry the harness loads by
/// its description; from there every active link or backticked path,
/// required or conditional, reaches its target. A file nothing reaches is
/// never read at the moment it is needed, so it is either dead or missing its
/// trigger. A pointer in fenced code or an HTML comment reaches nothing.
#[must_use]
pub fn orphans(files: &SkillFiles) -> Vec<String> {
    let mut reached: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = skill_entries(files).collect();
    while let Some(path) = queue.pop_front() {
        if !reached.insert(path.clone()) || !has_extension(&path, "md") {
            continue;
        }
        for (_, target) in edges(&files[&path]) {
            if let Ok(Some(resolved)) = resolve_reference(files, &path, &target) {
                if !reached.contains(&resolved) {
                    queue.push_back(resolved);
                }
            }
        }
    }
    files
        .keys()
        .filter(|path| is_reading_file(path) && !reached.contains(*path))
        .cloned()
        .collect()
}

/// Every structural fault in `files` under `inventory`, each once: chain
/// faults, graph faults, then orphans.
#[must_use]
pub fn structure_faults(files: &SkillFiles, inventory: &Inventory<'_>) -> Vec<String> {
    let mut faults = reading_chain(files, inventory).errors;
    faults.extend(graph_faults(files, inventory));
    faults.extend(
        orphans(files)
            .into_iter()
            .map(|path| format!("{path} is orphaned: no index entry or trigger reaches it")),
    );
    let mut seen = BTreeSet::new();
    faults.retain(|fault| seen.insert(fault.clone()));
    faults
}

// ---------------------------------------------------------------------------
// The kernel
// ---------------------------------------------------------------------------

/// The entry points the kernel text names in active Markdown, keyed like
/// [`SkillFiles`]: a skill invocation `` `/cf-x` `` names `cf-x/SKILL.md`,
/// and an installed path `` `.claude/skills/<path>` `` or
/// `` `.agents/skills/<path>` `` names `<path>` exactly. A child path never
/// names its skill, and a basename names nothing.
#[must_use]
pub fn kernel_entries(kernel: &str) -> BTreeSet<String> {
    let mut named = BTreeSet::new();
    for line in active_markdown(kernel) {
        for (index, span) in line.split('`').enumerate() {
            if index % 2 == 0 {
                continue;
            }
            if let Some(skill) = span.strip_prefix('/') {
                if !skill.is_empty()
                    && skill
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                {
                    named.insert(format!("{skill}/SKILL.md"));
                }
            }
            for installed in [".claude/skills/", ".agents/skills/"] {
                if let Some(path) = span.strip_prefix(installed) {
                    named.insert(path.to_string());
                }
            }
        }
    }
    named
}

/// The chain entry points in `inventory` that `kernel` does not name.
#[must_use]
pub fn unnamed_entry_points(kernel: &str, inventory: &Inventory<'_>) -> Vec<String> {
    let named = kernel_entries(kernel);
    inventory
        .entry_points
        .iter()
        .filter(|(_, entry)| !named.contains(*entry))
        .map(|(stage, entry)| format!("the kernel does not name the {stage} entry {entry}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(entries: &[(&str, &str)]) -> SkillFiles {
        entries
            .iter()
            .map(|(path, text)| ((*path).to_string(), (*text).to_string()))
            .collect()
    }

    const EMPTY: Inventory<'static> = Inventory {
        entry_points: &[("build", "a/SKILL.md")],
        conditional: &[],
        project: &[],
    };

    #[test]
    fn authored_length_counts_crlf_as_one_byte() {
        assert_eq!(authored_len(b"a\r\nb\n"), authored_len(b"a\nb\n"));
        assert_eq!(authored_len(b"a\rb"), 3);
    }

    #[test]
    fn a_required_edge_joins_the_chain_and_counts_its_bytes() {
        let tree = files(&[
            ("a/SKILL.md", "Read [the notes](references/notes.md).\n"),
            ("a/references/notes.md", "12345"),
        ]);
        let chain = reading_chain(&tree, &EMPTY);
        assert!(chain.errors.is_empty(), "{:?}", chain.errors);
        assert_eq!(chain.files.len(), 2);
        assert_eq!(chain.total, 39 + 5);
    }

    #[test]
    fn a_conditional_read_needs_its_trigger_in_the_sentence() {
        let reads = [conditional(
            "a/SKILL.md",
            "a/references/notes.md",
            "When the build fails",
            "only on a failed build",
        )];
        let inventory = Inventory {
            conditional: &reads,
            ..EMPTY
        };
        let triggered = files(&[
            (
                "a/SKILL.md",
                "When the build fails, read [notes](references/notes.md).\n",
            ),
            ("a/references/notes.md", "x"),
        ]);
        let chain = reading_chain(&triggered, &inventory);
        assert!(chain.errors.is_empty(), "{:?}", chain.errors);
        assert_eq!(chain.files.len(), 1, "a conditional read stays outside");

        let untriggered = files(&[
            ("a/SKILL.md", "Always read [notes](references/notes.md).\n"),
            ("a/references/notes.md", "x"),
        ]);
        let chain = reading_chain(&untriggered, &inventory);
        assert!(
            chain
                .errors
                .iter()
                .any(|e| e.contains("without a reviewed trigger")),
            "{:?}",
            chain.errors
        );
    }

    #[test]
    fn an_inventory_entry_without_a_trigger_fails_and_names_the_file() {
        let reads = [conditional(
            "a/SKILL.md",
            "a/references/notes.md",
            "  ",
            "a reason",
        )];
        let inventory = Inventory {
            conditional: &reads,
            ..EMPTY
        };
        let tree = files(&[
            ("a/SKILL.md", "Read [notes](references/notes.md).\n"),
            ("a/references/notes.md", "x"),
        ]);
        let chain = reading_chain(&tree, &inventory);
        assert!(
            chain
                .errors
                .iter()
                .any(|e| e
                    == "conditional read a/SKILL.md -> a/references/notes.md names no trigger"),
            "{:?}",
            chain.errors
        );
    }

    #[test]
    fn an_ambiguous_reference_is_an_error_not_a_panic() {
        let tree = files(&[
            ("a/SKILL.md", "Read `notes.md`.\n"),
            ("b/x/notes.md", "x"),
            ("c/y/notes.md", "y"),
        ]);
        let chain = reading_chain(&tree, &EMPTY);
        assert!(
            chain
                .errors
                .iter()
                .any(|e| e.contains("several shipped files")),
            "{:?}",
            chain.errors
        );
    }

    #[test]
    fn a_file_nothing_reaches_is_an_orphan() {
        let tree = files(&[
            ("a/SKILL.md", "Read [notes](references/notes.md).\n"),
            (
                "a/references/notes.md",
                "When needed, see `a/resources/deep.md`.\n",
            ),
            ("a/resources/deep.md", "x"),
            ("a/resources/lost.md", "x"),
            ("a/scripts/tool.py", "x"),
        ]);
        assert_eq!(orphans(&tree), vec!["a/resources/lost.md".to_string()]);
    }

    #[test]
    fn fenced_code_and_comments_are_not_reads() {
        let text = "Read [a](a.md).\n~~~markdown\nRead [b](b.md).\n```\nstill fenced [c](c.md)\n~~~\n\
                    ````\n[d](d.md)\n```\n````\nx <!-- [e](e.md) --> [f](f.md)\n<!--\n[g](g.md)\n-->\n";
        let targets: Vec<String> = edges(text).into_iter().map(|(_, t)| t).collect();
        assert_eq!(targets, vec!["a.md".to_string(), "f.md".to_string()]);
    }

    #[test]
    fn an_index_row_is_one_link_and_a_classification() {
        assert_eq!(
            index_row("| [Plan](quality/plan.md) | every task |"),
            Some(("quality/plan.md".into(), "every task".into()))
        );
        assert_eq!(
            index_row("| [New](quality/new.md) | |"),
            Some(("quality/new.md".into(), String::new()))
        );
        assert_eq!(index_row("| Section | Read |"), None);
        assert_eq!(index_row("| [a](a.md) | b | c |"), None);
    }

    #[test]
    fn an_unclassified_index_row_and_a_dangling_link_are_graph_faults() {
        let tree = files(&[
            ("a/SKILL.md", "Read [the index](index.md).\n"),
            (
                "a/index.md",
                "| Section | Read |\n|---|---|\n| [One](one.md) | every task |\n\
                 | [Two](two.md) | |\n| [Three](three.md) | when it rains |\n",
            ),
            (
                "a/one.md",
                "Before a repair, read [the procedure](missing.md).\n",
            ),
            ("a/two.md", "x"),
            ("a/three.md", "x"),
        ]);
        let faults = graph_faults(&tree, &EMPTY);
        assert!(
            faults.contains(
                &"a/index.md: the index row for a/two.md has no load classification".to_string()
            ),
            "{faults:?}"
        );
        assert!(
            faults.iter().any(|f| f.starts_with(
                "a/index.md: the index row for a/three.md is classified `when it rains`"
            )),
            "{faults:?}"
        );
        assert!(
            faults
                .iter()
                .any(|f| f.starts_with("a/one.md names `missing.md`, which is no shipped file")),
            "{faults:?}"
        );
    }

    #[test]
    fn the_kernel_names_an_entry_only_by_invocation_or_exact_path() {
        let kernel =
            "Run `/cf-plan`. Read `.agents/skills/cf-method/references/workflow-lifecycle.md` \
                      and `.claude/skills/cf-x/resources/current-ensemble.json`. \
                      <!-- `/cf-ship` -->";
        let named = kernel_entries(kernel);
        assert!(named.contains("cf-plan/SKILL.md"));
        assert!(named.contains("cf-method/references/workflow-lifecycle.md"));
        assert!(
            !named.contains("cf-method/SKILL.md"),
            "a child path never names its skill"
        );
        assert!(!named.contains("cf-model-orchestrator/resources/current-ensemble.json"));
        assert!(
            !named.contains("cf-ship/SKILL.md"),
            "a commented invocation is not active"
        );
    }

    #[test]
    fn skills_measure_against_their_own_or_class_guideline() {
        assert_eq!(skill_guideline("cf-herdr"), 9 * KIB);
        assert_eq!(
            skill_guideline("cf-new-routing"),
            OTHER_SKILL_GUIDELINE_BYTES
        );
        let tree = files(&[
            ("cf-herdr/SKILL.md", "x"),
            ("cf-herdr/resources/SKILL.md", "not a skill"),
        ]);
        let measures = skill_measures(&tree);
        assert_eq!(measures.len(), 1);
        assert!(!measures[0].over());
        let big = Measure {
            subject: "cf-herdr".into(),
            bytes: 9 * KIB + 1,
            guideline: 9 * KIB,
        };
        assert!(big.over());
        assert_eq!(big.to_string(), "cf-herdr 9217 of 9216 bytes");
    }
}
