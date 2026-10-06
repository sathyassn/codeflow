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
//! Markdown is read by a standard Markdown parser (`pulldown_cmark`, with
//! tables), as a harness renders it. Only active content counts as a read: a link (inline,
//! reference-style, titled or angle-bracketed) or a code span naming a
//! reading file. Code blocks (fenced, indented or quoted), raw HTML and HTML
//! comments are examples or retired text, never a pointer. Index rows come
//! from the parser's table cells.
//!
//! Known limit: a code span that names a reading file counts as a pointer
//! whatever its sentence says, so "do not read `x.md`" still reaches `x.md`.
//! Backticked paths are how the shipped docs point at files, and a mention
//! of an obsolete file is a review matter, not a structure matter. The check
//! catches authoring mistakes, such as a file that lost its pointer or a read
//! without its trigger; it is not a boundary against Markdown written to
//! fool it.
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

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

const KIB: usize = 1024;

/// Shipped skill files keyed `<skill>/<path>`, with their text.
pub type SkillFiles = BTreeMap<String, String>;

// ---------------------------------------------------------------------------
// Guideline numbers
// ---------------------------------------------------------------------------

/// Guideline for the per-task reading chain: what one full implementation
/// task reads after the kernel. Reported, never a failure.
pub const READING_CHAIN_GUIDELINE_BYTES: usize = 128 * KIB;

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
    ("cf-customize", 22 * KIB + 512),
    ("cf-delegate", 20 * KIB + 512),
    ("cf-design", 20 * KIB),
    ("cf-develop", 5 * KIB + 256),
    ("cf-docs-portal", 9 * KIB + 128),
    ("cf-editorial-review", 7 * KIB),
    ("cf-estimate", 6 * KIB),
    ("cf-evaluate-model", 9 * KIB + 512),
    ("cf-herdr", 9 * KIB),
    ("cf-method", 18 * KIB),
    ("cf-model-orchestrator", 26 * KIB),
    ("cf-plan", 9 * KIB),
    ("cf-present", 8 * KIB + 256),
    ("cf-ship", 6 * KIB + 896),
    ("cf-stack", 4 * KIB),
];

/// Guidelines for the other shipped instruction files, keyed by their
/// source under `assets/base/`: the former class ceilings.
pub const ARTIFACT_GUIDELINES: &[(&str, usize)] = &[
    ("CLAUDE.md.tmpl", 6 * KIB),
    ("CLAUDE.minimal.md.tmpl", 3 * KIB),
    ("claude/agents/cf-reviewer.md", 10 * KIB + 256),
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
const VERIFICATION_SELECTION: &str = "cf-model-orchestrator/resources/verification-selection.md";
/// TSK-108: the moment a stage skill reads the work lifecycle section.
const LIFECYCLE_MOMENT: &str = "work item is planned, started, blocked, completed or cancelled";

/// Reads outside the per-task chain, each with its trigger and reason.
pub const CONDITIONAL_READS: &[ConditionalRead] = &[
    // The autonomy reference (EPC-018): read when a step may need the
    // operator, never on every task.
    conditional(
        ORCH_SKILL,
        "cf-method/references/autonomy.md",
        "Before deciding whether to ask the operator, escalate or stop",
        "only when deciding whether to ask, escalate or stop",
    ),
    conditional(
        ORCH_SKILL,
        "cf-method/references/autonomy.md",
        "ask the operator only what",
        "only when a question may belong to the operator",
    ),
    conditional(
        "cf-plan/SKILL.md",
        "cf-method/references/autonomy.md",
        "Ask the operator only what",
        "only when a question may belong to the operator",
    ),
    conditional(
        "cf-method/references/workflow-lifecycle.md",
        "cf-method/references/autonomy.md",
        "A request for a change selects implementation",
        "only when settling or escalating a step of a change",
    ),
    conditional(
        "cf-method/references/workflow-lifecycle.md",
        "cf-method/references/autonomy.md",
        "or record settled dissent per",
        "only when a seat dissents on the plan",
    ),
    conditional(
        "cf-method/references/workflow-lifecycle.md",
        "cf-method/references/autonomy.md",
        "Follow the PR to its readiness report",
        "only when deciding who merges",
    ),
    conditional(
        "cf-model-orchestrator/resources/quality/plan.md",
        "cf-method/references/autonomy.md",
        "settles a disagreement on a reversible choice",
        "only when the seats still disagree on a reversible item",
    ),
    conditional(
        "cf-model-orchestrator/resources/quality/blockers-and-gates.md",
        "cf-method/references/autonomy.md",
        "reserves to the operator goes to them as one question",
        "only when a blocker may be the operator choice",
    ),
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
    // TSK-184 reduction: the selection contract for property, mutation and
    // fitness techniques is read only when a changed path carries one of the
    // signals the verification section names.
    conditional(
        ORCH_SKILL,
        VERIFICATION_SELECTION,
        "When a changed path carries a candidate for a property, mutation or fitness technique",
        "only when a changed path carries a signal for an earned technique",
    ),
    conditional(
        "cf-model-orchestrator/resources/quality/verification.md",
        VERIFICATION_SELECTION,
        "When a changed path carries one of these signals, read",
        "only when a changed path carries a signal for an earned technique",
    ),
    // TSK-150 (audit row H24): the Grok host detail is read before a Grok
    // preflight or launch. TSK-184: the other-hosts reference merged into the
    // orchestrator's seat section.
    conditional(
        ORCH_SKILL,
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
    // TSK-184: the delivery process narrative is read when a body of work
    // is shaped, a batch lands or something changes midway, never per task.
    conditional(
        "cf-method/references/workflow-lifecycle.md",
        "cf-method/references/delivery-process.md",
        "When you shape a body of work, land a batch or handle a change midway, read",
        "only when shaping a body of work, landing a batch or handling a change midway",
    ),
    // TSK-240: issue handling is read when a reported defect is fixed,
    // never on every task.
    conditional(
        "cf-method/references/workflow-lifecycle.md",
        "cf-method/references/issue-handling.md",
        "a reported one first through",
        "only when a reported defect is fixed",
    ),
    conditional(
        "cf-method/SKILL.md",
        "cf-method/references/skill-authoring.md",
        "When authoring or editing a skill",
        "only when authoring or editing a skill",
    ),
    // TSK-213: lanes follow the seat being called (ADR-0077). A Claude
    // seat's lifecycle lane is read on a Codex or Grok host; the plugin is
    // an optional fallback read only while it is in use.
    conditional(
        DELEGATE,
        LIFECYCLE_LANE,
        "**A Claude seat, from a Codex or Grok host:**",
        "the lifecycle lane is read when a Codex or Grok host calls a Claude seat",
    ),
    conditional(
        DELEGATE,
        "cf-delegate/resources/lane-plugin.md",
        "**Only when the Codex plugin fallback is in use, on a Claude Code host:**",
        "only while the optional Codex plugin fallback is in use",
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
        "A Claude seat uses",
        "the lifecycle lane is read when a Codex or Grok host calls a Claude seat",
    ),
    conditional(
        "cf-model-orchestrator/resources/routing/transport.md",
        "cf-method/references/autonomy.md",
        "answers folder trust",
        "only when a new seat raises a folder trust prompt",
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
    // TSK-108: the stage skills follow the one work lifecycle section.
    conditional(
        "cf-method/SKILL.md",
        ORGANIZATION,
        LIFECYCLE_MOMENT,
        "only when a work item's record, branch or status changes",
    ),
    conditional(
        "cf-plan/SKILL.md",
        ORGANIZATION,
        LIFECYCLE_MOMENT,
        "only when a work item's record, branch or status changes",
    ),
    conditional(
        "cf-develop/SKILL.md",
        ORGANIZATION,
        LIFECYCLE_MOMENT,
        "only when a work item's record, branch or status changes",
    ),
    conditional(
        "cf-ship/SKILL.md",
        ORGANIZATION,
        LIFECYCLE_MOMENT,
        "only when a work item's record, branch or status changes",
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
    // TSK-184: the assignment record moved into the plan section.
    conditional(
        "cf-model-orchestrator/resources/quality/plan.md",
        ROUTE_STATUS,
        "Qualifying a route, or claiming scoped qualification",
        "only when a route is qualified or a qualification, promotion or savings claim is made",
    ),
    // TSK-184: the host routes merged into the orchestrator's seat section;
    // the worker effort preflight joined its preflight, whose adapter read is
    // recorded above.
    conditional(
        ORCH_SKILL,
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
    // TSK-184 reduction: the bounded wait and the redness classes load only
    // when a check is red or stuck or the policy requires hosted checks.
    conditional(
        "cf-ship/SKILL.md",
        "cf-ship/references/pr-checks.md",
        "For red or unfinished CI jobs, follow",
        "only when a CI job is red or unfinished",
    ),
    // TSK-218: release integration loads only after an epic-line landing
    // in a project that configures a release branch and its workflow.
    conditional(
        "cf-ship/SKILL.md",
        "cf-ship/references/release-integration.md",
        "only when the project configures a release branch matching its release pattern",
        "only after an epic-line landing with a configured release branch and workflow",
    ),
    conditional(
        "cf-ship/references/pr-evidence.md",
        "cf-ship/references/pr-checks.md",
        "When a required check is red or stuck, or the adopted policy requires hosted checks green before landing, follow",
        "only when a check is red or stuck, or hosted checks must be green before landing",
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
    // The approved plan the orchestrator and cf-ship read before choosing
    // a landing route (TSK-191).
    "PLAN.md",
    "TASK.md",
    "index.md",
    "portal.config.json",
    "epics/EPC-NNN.md",
    "specs/SPC-NNN.md",
    "tasks/TSK-NNN.md",
    ".claude/settings.json",
    // A retention fixture's window setting (TSK-130), in the disposable
    // fixture only.
    ".claude/settings.local.json",
    ".grok/hooks/codeflow.json",
    ".codeflow/estimate.json",
    ".codeflow/manifest.json",
    ".codeflow/policy.json",
    ".codeflow/test-config.json",
    // The writing reference the rule map loads at every tier, installed
    // beside the kernel rather than in a skill tree; skills cite it for the
    // plain-writing rule (TSK-177).
    ".codeflow/rules/writing.md",
    // The git rules file the method skill points at for the enforcement
    // planes (TSK-184 reduction).
    ".codeflow/rules/git-rules.md",
    ".codeflow/schemas/present/document-v1.schema.json",
    ".codeflow/schemas/present/utility-tokens-v1.schema.json",
    "DIR/settings.json",
    "result.json",
    "turns/<turn>/continuations/<task-id>/accepted.json",
    "~/.codex/auth.json",
    "~/.gemini/config/hooks.json",
    // The evaluation kit's per-trial runtime records (TSK-077): the pin
    // record and host pointer under the run root, and the gh stand-in's
    // state file in the trial checkout.
    "pins.json",
    "stand-in-host.json",
    "gh-stand-in.json",
    // The visual guide's portal and figure files (EPC-016): the portal
    // runtime's script list, figure fixtures and declarations, the
    // utility-presentation architecture page, and the pages the copy guide
    // cites as this repository's worked examples.
    "scripts/runtime-scripts.json",
    "tests/fixtures/figures/*.json",
    "docs/figures/present-boundary.json",
    "docs/figures/present-revisions.json",
    "docs/figures/present-limits.json",
    "docs/architecture/utility-presentation.md",
    "docs/decisions/README.md",
    ".github/pull_request_template.md",
    "docs/decisions/ADR-0058-explicit-portal-runtime-ownership.md",
    "docs/decisions/ADR-0064-portal-as-a-guide-to-the-project-as-it-stands.md",
    "docs/decisions/ADR-0067-written-content-policy.md",
    ".codeflow/schemas/present/document-v2.schema.json",
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

/// Load the complete skill tree. Any unreadable entry makes the tree unproven.
/// # Errors
/// Returns the obtaining error for a directory, entry, or text file.
pub fn load_skill_tree(dir: &Path, files: &mut SkillFiles) -> std::io::Result<()> {
    if !dir.try_exists()? {
        return Ok(());
    }
    load_under(dir, dir, files)
}
fn load_under(base: &Path, dir: &Path, files: &mut SkillFiles) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            load_under(base, &path, files)?;
        } else if kind.is_file() {
            let text = std::fs::read_to_string(&path)?;
            let relative = path.strip_prefix(base).map_err(std::io::Error::other)?;
            files.insert(crate::portable_path::slashed(relative), text);
        }
    }
    Ok(())
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

/// Whether a code span names a reading file: one token, no scheme.
fn is_code_path(span: &str) -> bool {
    !span.is_empty()
        && !span.chars().any(char::is_whitespace)
        && !span.contains("://")
        && is_reading_file(span)
}

/// The reading file a link destination names, without its fragment, or
/// `None` for an external, mail or same-page link or another kind of file.
fn link_path(destination: &str) -> Option<String> {
    if destination.contains("://") || destination.starts_with("mailto:") {
        return None;
    }
    let path = destination.split('#').next().unwrap_or_default();
    (!path.is_empty() && is_reading_file(path)).then(|| path.to_string())
}

/// The sentence of `block` that holds byte `at`, whitespace-normalized: a
/// sentence ends after a full stop followed by whitespace.
fn sentence_at(block: &str, at: usize) -> String {
    let bytes = block.as_bytes();
    let at = at.min(bytes.len());
    let ends = |i: usize| bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_whitespace);
    let start = (0..at).rev().find(|&i| ends(i)).map_or(0, |i| i + 1);
    let end = (at..bytes.len())
        .find(|&i| ends(i))
        .map_or(bytes.len(), |i| i + 1);
    normalized(&block[start..end])
}

/// A reading pointer in active Markdown: the sentence that holds it, as
/// written, and the target it names.
type Pointer = (String, String);

/// A row of an index table.
#[derive(Debug, Clone, PartialEq, Eq)]
enum IndexRow {
    /// One section link and its load classification cell, as written.
    Row {
        target: String,
        classification: String,
    },
    /// A row of a `Read` index table that is not one link and a
    /// classification, as written.
    Malformed(String),
}

/// What a Markdown file says, as the Markdown parser (with tables) reads
/// it: its reading pointers, its index rows and its code spans. Code
/// blocks (fenced, indented or quoted), raw HTML and HTML comments are
/// inactive, so an example or a retired pointer is never a read.
#[derive(Debug, Default)]
struct Scan {
    pointers: Vec<Pointer>,
    rows: Vec<IndexRow>,
    code_spans: Vec<String>,
}

/// A block whose source holds the sentences of the pointers inside it.
struct Frame {
    start: usize,
    end: usize,
    /// Where a list item's own text stops: its first nested block.
    cut: Option<usize>,
    item: bool,
    found: Vec<(usize, String)>,
}

#[derive(Default)]
struct Cell {
    raw: String,
    /// The visible text, with inline markup such as emphasis stripped.
    text: String,
    /// Each top-level link: the reading file it names, if any.
    links: Vec<Option<String>>,
    /// Content outside a link.
    other: bool,
}

#[derive(Default)]
struct TableScan {
    header: Vec<Cell>,
    row: Vec<Cell>,
    in_cell: bool,
    link_depth: usize,
}

struct Scanner<'t> {
    text: &'t str,
    frames: Vec<Frame>,
    table: Option<TableScan>,
    found: Vec<(usize, String, String)>,
    scan: Scan,
}

/// Whether `tag` is inline, so it never ends a list item's own text.
const fn is_inline(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::Link { .. }
            | Tag::Image { .. }
    )
}

const fn is_frame(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph | Tag::Heading { .. } | Tag::Item | Tag::TableHead | Tag::TableRow
    )
}

const fn ends_frame(tag: TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::Item
            | TagEnd::TableHead
            | TagEnd::TableRow
    )
}

impl<'t> Scanner<'t> {
    fn new(text: &'t str) -> Self {
        Self {
            text,
            frames: Vec::new(),
            table: None,
            found: Vec::new(),
            scan: Scan::default(),
        }
    }

    fn run(mut self) -> Scan {
        let parser = Parser::new_ext(self.text, Options::ENABLE_TABLES);
        for (event, range) in parser.into_offset_iter() {
            match event {
                Event::Start(tag) => self.start(&tag, range),
                Event::End(tag) => self.end(tag, range),
                Event::Code(code) => {
                    self.content(&code);
                    if is_code_path(&code) {
                        self.pointer(range, code.to_string());
                    }
                    self.scan.code_spans.push(code.to_string());
                }
                Event::Text(text) => self.content(&text),
                _ => {}
            }
        }
        self.found.sort_by_key(|(at, _, _)| *at);
        self.scan.pointers = self
            .found
            .into_iter()
            .map(|(_, sentence, target)| (sentence, target))
            .collect();
        self.scan
    }

    /// Visible text or code in a table cell: kept as the cell's text, and
    /// marked as content outside a link when it is at the top level.
    fn content(&mut self, text: &str) {
        let Some(table) = self.table.as_mut().filter(|t| t.in_cell) else {
            return;
        };
        let outside = table.link_depth == 0 && !text.trim().is_empty();
        if let Some(cell) = table.row.last_mut() {
            cell.text.push_str(text);
            cell.other |= outside;
        }
    }

    fn pointer(&mut self, range: std::ops::Range<usize>, target: String) {
        if let Some(frame) = self.frames.last_mut() {
            frame.found.push((range.start, target));
        } else {
            let sentence = normalized(&self.text[range.clone()]);
            self.found.push((range.start, sentence, target));
        }
    }

    fn start(&mut self, tag: &Tag<'_>, range: std::ops::Range<usize>) {
        if !is_inline(tag) {
            if let Some(top) = self.frames.last_mut() {
                if top.item && top.cut.is_none() {
                    top.cut = Some(range.start);
                }
            }
        }
        if is_frame(tag) {
            self.frames.push(Frame {
                start: range.start,
                end: range.end,
                cut: None,
                item: matches!(tag, Tag::Item),
                found: Vec::new(),
            });
        }
        match tag {
            Tag::Table(_) => self.table = Some(TableScan::default()),
            Tag::TableCell => {
                if let Some(table) = self.table.as_mut() {
                    let raw = self.text[range].trim().trim_matches('|').trim();
                    table.row.push(Cell {
                        raw: raw.to_string(),
                        ..Cell::default()
                    });
                    table.in_cell = true;
                }
            }
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                let target = if matches!(link_type, LinkType::Email) {
                    None
                } else {
                    link_path(dest_url)
                };
                if let Some(table) = self.table.as_mut().filter(|t| t.in_cell) {
                    if table.link_depth == 0 {
                        if let Some(cell) = table.row.last_mut() {
                            cell.links.push(target.clone());
                        }
                    }
                    table.link_depth += 1;
                }
                if let Some(target) = target {
                    self.pointer(range, target);
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd, range: std::ops::Range<usize>) {
        if ends_frame(tag) {
            if let Some(frame) = self.frames.pop() {
                let block = &self.text[frame.start..frame.cut.unwrap_or(frame.end)];
                for (at, target) in frame.found {
                    let sentence = sentence_at(block, at.saturating_sub(frame.start));
                    self.found.push((at, sentence, target));
                }
            }
        }
        let Some(table) = self.table.as_mut() else {
            return;
        };
        match tag {
            TagEnd::Link if table.in_cell => table.link_depth = table.link_depth.saturating_sub(1),
            TagEnd::TableCell => table.in_cell = false,
            TagEnd::TableHead => table.header = std::mem::take(&mut table.row),
            TagEnd::TableRow => {
                let row = std::mem::take(&mut table.row);
                // The header matches on its visible text, so `**Read**` is Read.
                let read_table = table.header.len() == 2
                    && table.header[1].text.trim().eq_ignore_ascii_case("read");
                let target = row
                    .first()
                    .filter(|cell| !cell.other && cell.links.len() == 1)
                    .and_then(|cell| cell.links[0].clone());
                match (target, row.as_slice()) {
                    (Some(target), [_, classification]) => self.scan.rows.push(IndexRow::Row {
                        target,
                        classification: classification.raw.clone(),
                    }),
                    _ if read_table => self
                        .scan
                        .rows
                        .push(IndexRow::Malformed(normalized(&self.text[range]))),
                    _ => {}
                }
            }
            TagEnd::Table => self.table = None,
            _ => {}
        }
    }
}

/// Parse `text` as standard Markdown with tables.
fn scan(text: &str) -> Scan {
    Scanner::new(text).run()
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
/// prefix), then a unique path suffix, preferring the one match in the
/// source's own skill. An ambiguous suffix is an error.
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
    let mut matches: Vec<&String> = files.keys().filter(|k| k.ends_with(&suffix)).collect();
    // A skill reads its own copy first: when several skills ship a file of
    // this name and exactly one match is in the source's skill, that is it.
    let own: Vec<&String> = matches
        .iter()
        .copied()
        .filter(|k| k.starts_with(&format!("{skill}/")))
        .collect();
    if matches.len() > 1 && own.len() == 1 {
        matches = own;
    }
    match matches.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).clone())),
        several => Err(format!(
            "{source} names `{target}`, which matches several shipped files: {several:?}"
        )),
    }
}

/// Every reading edge out of the Markdown `text`: the sentence that holds it
/// and the target it names, as written.
fn edges(text: &str) -> Vec<Pointer> {
    scan(text).pointers
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

/// Index rows in `path` (see [`Scan`]): a row of a `Read` table must be one
/// section link and its classification; each needs a classification, and
/// one other than "every task" needs a reviewed conditional read, so an
/// empty, malformed or new classification is never silently read as
/// required.
fn index_faults(
    files: &SkillFiles,
    inventory: &Inventory<'_>,
    path: &str,
    rows: &[IndexRow],
) -> Vec<String> {
    let mut faults = Vec::new();
    for row in rows {
        let (target, classification) = match row {
            IndexRow::Row {
                target,
                classification,
            } => (target, classification),
            IndexRow::Malformed(row) => {
                faults.push(format!(
                    "{path}: the index row `{row}` is not one section link and its \
                     load classification"
                ));
                continue;
            }
        };
        let Ok(Some(resolved)) = resolve_reference(files, path, target) else {
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
        let scanned = scan(text);
        faults.extend(index_faults(files, inventory, &path, &scanned.rows));
        for (sentence, target) in scanned.pointers {
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
/// trigger. A pointer in a code block, raw HTML or an HTML comment reaches
/// nothing.
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

/// The entry points the kernel text names in active code spans, keyed like
/// [`SkillFiles`]: a skill invocation `` `/cf-x` `` names `cf-x/SKILL.md`,
/// and an installed path `` `.claude/skills/<path>` `` or
/// `` `.agents/skills/<path>` `` names `<path>` exactly. A child path never
/// names its skill, and a basename names nothing.
#[must_use]
pub fn kernel_entries(kernel: &str) -> BTreeSet<String> {
    let mut named = BTreeSet::new();
    for span in scan(kernel).code_spans {
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

    fn targets(text: &str) -> Vec<String> {
        edges(text).into_iter().map(|(_, target)| target).collect()
    }

    #[test]
    fn code_blocks_raw_html_and_comments_are_not_reads() {
        let text = "Read [a](a.md).\n~~~markdown\nRead [b](b.md).\n```\nstill fenced [c](c.md)\n~~~\n\
                    ````\n[d](d.md)\n```\n````\nx <!-- [e](e.md) --> [f](f.md)\n<!--\n[g](g.md)\n-->\n\
                    \n    [h](h.md) indented code\n\n> ~~~md\n> [i](i.md)\n> ~~~\n\n\
                    <div>\n[j](j.md)\n</div>\n\nA `k.md` span and ``l.md`` too.\n";
        assert_eq!(targets(text), ["a.md", "f.md", "k.md", "l.md"]);
    }

    #[test]
    fn every_link_form_names_its_target() {
        let text = "Read [ref][r], [collapsed][], [shortcut], [titled](t.md \"Guide\"), \
                    [angled](<a b.md>), [anchored](x.md#part) and `code/path.md`.\n\
                    Not [external](https://example.com/x.md), [mail](mailto:x@y.md), \
                    [same page](#x), `two words.md` or [a script](run.sh).\n\n\
                    [r]: ref.md\n[collapsed]: collapsed.md\n[shortcut]: <short.md> \"t\"\n\
                    [unused]: unused.md\n";
        assert_eq!(
            targets(text),
            [
                "ref.md",
                "collapsed.md",
                "short.md",
                "t.md",
                "a b.md",
                "x.md",
                "code/path.md"
            ]
        );
    }

    #[test]
    fn a_pointer_keeps_the_sentence_that_holds_it() {
        let text = "First. When it fails, read\n[notes](n.md). Last.\n\n\
                    - Before a launch, read [launch](l.md)\n  - Nested, see [child](c.md).\n\n\
                    | Section | Read |\n|---|---|\n| [Plan](p.md) | when planning |\n";
        let found = edges(text);
        let sentence = |target: &str| {
            found
                .iter()
                .find(|(_, t)| t == target)
                .map(|(s, _)| s.clone())
                .unwrap_or_default()
        };
        assert_eq!(sentence("n.md"), "When it fails, read [notes](n.md).");
        assert_eq!(sentence("l.md"), "- Before a launch, read [launch](l.md)");
        assert_eq!(sentence("c.md"), "- Nested, see [child](c.md).");
        assert_eq!(sentence("p.md"), "| [Plan](p.md) | when planning |");
    }

    #[test]
    fn an_index_row_is_one_link_and_a_classification() {
        let text = "| Section | Read |\n|---|---|\n\
                    | [Plan](quality/plan.md) | every task |\n\
                    | [New](quality/new.md) | |\n\
                    | [Versioned \\| plan](quality/v.md) | |\n\
                    | [Extra](quality/e.md) | | stray |\n\
                    | [Short](quality/s.md) |\n\
                    | Plain text | every task |\n\
                    | [Two](a.md) [links](b.md) | every task |\n\n\
                    | File | Purpose | When |\n|---|---|---|\n| [a](a.md) | b | c |\n\n\
                    | Section | **Read** |\n|---|---|\n| See [it](quality/x.md) too | every task |\n";
        let row = |target: &str, classification: &str| IndexRow::Row {
            target: target.into(),
            classification: classification.into(),
        };
        assert_eq!(
            scan(text).rows,
            [
                row("quality/plan.md", "every task"),
                row("quality/new.md", ""),
                row("quality/v.md", ""),
                row("quality/e.md", ""),
                row("quality/s.md", ""),
                IndexRow::Malformed("| Plain text | every task |".into()),
                IndexRow::Malformed("| [Two](a.md) [links](b.md) | every task |".into()),
                IndexRow::Malformed("| See [it](quality/x.md) too | every task |".into()),
            ]
        );
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
    /// Shipped files frozen as they stood when the scanner moved to the
    /// Markdown parser, keyed as they install.
    const CORPUS: &[(&str, &str)] = &[
        (
            "cf-model-orchestrator/SKILL.md",
            include_str!("../tests/fixtures/reading-corpus/cf-model-orchestrator--SKILL.md"),
        ),
        (
            "cf-model-orchestrator/resources/quality-contract.md",
            include_str!(
                "../tests/fixtures/reading-corpus/cf-model-orchestrator--resources--quality-contract.md"
            ),
        ),
        (
            "cf-model-orchestrator/resources/capability-routing.md",
            include_str!(
                "../tests/fixtures/reading-corpus/cf-model-orchestrator--resources--capability-routing.md"
            ),
        ),
        (
            "cf-model-orchestrator/resources/grok-host.md",
            include_str!(
                "../tests/fixtures/reading-corpus/cf-model-orchestrator--resources--grok-host.md"
            ),
        ),
        (
            "cf-model-orchestrator/resources/routing/hosts.md",
            include_str!(
                "../tests/fixtures/reading-corpus/cf-model-orchestrator--resources--routing--hosts.md"
            ),
        ),
    ];

    /// A sample of the switch to the parser: on five frozen shipped files and
    /// the standard kernel template it finds every edge with the sentence
    /// that holds it, every index row and every kernel entry that the line
    /// scanner it replaced found (`expected.tsv`, recorded from that
    /// scanner). Its inputs are frozen, so it never needs updating; the
    /// whole-tree comparison at the switch is recorded as evidence in
    /// `docs/verification/tsk-150-byte-cut-audit.md`.
    #[test]
    fn the_parser_reads_the_frozen_corpus_as_the_line_scanner_did() {
        let kernel = include_str!("../tests/fixtures/reading-corpus/AGENTS.md.tmpl");
        let expected = include_str!("../tests/fixtures/reading-corpus/expected.tsv");
        let mut found = Vec::new();
        for (path, text) in CORPUS {
            let scanned = scan(text);
            for (sentence, target) in scanned.pointers {
                found.push(format!("EDGE\t{path}\t{target}\t{sentence}"));
            }
            for row in scanned.rows {
                let IndexRow::Row {
                    target,
                    classification,
                } = row
                else {
                    panic!("{path}: malformed index row {row:?}");
                };
                found.push(format!("ROW\t{path}\t{target}\t{classification}"));
            }
        }
        for entry in kernel_entries(kernel) {
            found.push(format!("KERNEL\t{entry}"));
        }
        // Order carries no meaning: the line scanner listed a sentence's links
        // before its code spans, the parser lists them by position.
        found.sort();
        let mut expected: Vec<&str> = expected.lines().collect();
        expected.sort_unstable();
        assert_eq!(found, expected);
    }
}

#[cfg(test)]
mod r16_obtaining_regressions {

    #[test]
    fn r16_reading_refuses_unreadable_skill_inventory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skills");
        std::fs::write(&path, "not a directory").unwrap();
        let mut files = std::collections::BTreeMap::new();
        assert!(super::load_skill_tree(&path, &mut files).is_err());
    }
}
