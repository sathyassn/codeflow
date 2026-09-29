//! The root rule map: every tier's `AGENTS.md` and `CLAUDE.md` template,
//! rendered from one kernel source (`assets/base/rule-map.toml`, TSK-127).
//!
//! The kernel holds the always rules, the moment table and the tier prose.
//! The moment table renders as two tables: the delivery stages in the order
//! work moves, then the situations that cut across stages (TSK-184). A
//! moment's pointer may be marked `MUST OPEN` with its reason, and may quote
//! the section of the pointed file to read.
//! The rendered templates are checked in under `assets/base/` so the
//! scaffold engine installs them like any other asset; the
//! `rule_map_contract` test fails when a template drifts from the kernel.
//! Keeping the rules as data also lets a later reader (the compaction
//! re-injection of TSK-128) take the same rules without parsing Markdown.

use std::fmt::Write as _;

use serde::Deserialize;

use super::Tier;

/// The kernel source, embedded so the map is available without the asset
/// tree.
pub const KERNEL: &str = include_str!("../../../../assets/base/rule-map.toml");

/// Guideline for the number of always rules one tier's map carries. A
/// reported measure, never a failure (TSK-150): a rule past it is a prompt
/// to move detail behind a moment row, not a reason to cut a duty.
pub const RULES_GUIDELINE: usize = 13;

/// Guideline for the bytes of one rendered always rule. Reported, never a
/// failure; one line per rule is the structural rule.
pub const RULE_LINE_GUIDELINE_BYTES: usize = 480;

/// Guideline for the managed block of one tier's `AGENTS.md`, markers
/// included: the always-read kernel. It leaves a realistic project section
/// room under Codex's instruction limit. `codeflow doctor` reports the kernel
/// against it, apart from the size of the complete installed `AGENTS.md`
/// (its `instructions` check); it is never a failure (TSK-150, TSK-184).
pub const MANAGED_BLOCK_GUIDELINE_BYTES: usize = 12 * 1024;

/// Codex reads at most this many bytes of project instructions
/// (`project_doc_max_bytes`) and silently cuts the rest of an `AGENTS.md`.
/// This is a host truncation point, not a reading budget: it is the one byte
/// check that stays a failure for the generated document (TSK-150).
pub const CODEX_INSTRUCTION_LIMIT_BYTES: usize = 32 * 1024;

/// The bytes by which a complete `AGENTS.md` (managed block and project
/// section together) passes Codex's instruction limit, or `None` when the
/// whole document fits. The whole document is measured because Codex cuts
/// the end, where the project section lives, even when the managed block
/// alone is small.
#[must_use]
pub fn codex_overflow(document: &str) -> Option<usize> {
    document
        .len()
        .checked_sub(CODEX_INSTRUCTION_LIMIT_BYTES)
        .filter(|over| *over > 0)
}

const ALL_TIERS: [Tier; 3] = [Tier::Minimal, Tier::Standard, Tier::Full];

/// One rendered instruction file: its asset path under `assets/base/` and
/// the tier whose kernel content it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Output {
    pub asset: &'static str,
    pub tier: Tier,
    pub file: File,
}

/// Which root file an [`Output`] renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum File {
    Agents,
    Claude,
}

/// Every template the kernel renders. The standard and full tiers share one
/// CLAUDE template, so the kernel's CLAUDE text must not differ between them.
pub const OUTPUTS: &[Output] = &[
    Output {
        asset: "AGENTS.minimal.md.tmpl",
        tier: Tier::Minimal,
        file: File::Agents,
    },
    Output {
        asset: "AGENTS.md.tmpl",
        tier: Tier::Standard,
        file: File::Agents,
    },
    Output {
        asset: "AGENTS.full.md.tmpl",
        tier: Tier::Full,
        file: File::Agents,
    },
    Output {
        asset: "CLAUDE.minimal.md.tmpl",
        tier: Tier::Minimal,
        file: File::Claude,
    },
    Output {
        asset: "CLAUDE.md.tmpl",
        tier: Tier::Standard,
        file: File::Claude,
    },
];

/// The parsed kernel.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kernel {
    pub schema_version: u32,
    #[serde(default)]
    pub agents: Vec<Block>,
    #[serde(default, rename = "rule")]
    pub rules: Vec<Rule>,
    #[serde(default, rename = "moment")]
    pub moments: Vec<Moment>,
    #[serde(default)]
    pub claude: Vec<Block>,
}

/// An ordered block of a rendered file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    #[serde(default)]
    pub tiers: Option<Vec<Tier>>,
    #[serde(default)]
    pub kind: BlockKind,
    pub text: String,
}

/// What a block renders after its text.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockKind {
    #[default]
    Text,
    Rules,
    Moments,
}

/// One always rule: a single line with at least one pointer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    #[serde(default)]
    pub tiers: Option<Vec<Tier>>,
    pub title: String,
    pub text: String,
    pub see: Vec<String>,
}

/// Which of the map's two tables a moment row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Group {
    /// A delivery stage, in the order work moves.
    Delivery,
    /// A situation that cuts across stages.
    Situation,
}

/// The lead sentence of the delivery table.
pub const DELIVERY_LEAD: &str = "Delivery, in the order work moves:";

/// The lead sentence of the situations table.
pub const SITUATION_LEAD: &str = "Situations that cut across stages:";

/// One row of the "when you are about to" table.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Moment {
    pub key: String,
    pub group: Group,
    #[serde(default)]
    pub tiers: Option<Vec<Tier>>,
    pub when: String,
    pub action: String,
    pub see: Vec<Pointer>,
}

/// A pointer in a moment row: its target (a skill, an agent, a
/// skill-relative path or a project path, as for a rule's `see`), and
/// optionally the section of that file to read, a short note on when, and
/// the `MUST OPEN` mark with the reason the read is required before acting.
/// In the kernel it is either a plain string or a table with these fields.
/// It dereferences to its target, so it reads as a plain pointer wherever
/// one is expected.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "PointerSource")]
pub struct Pointer {
    pub target: String,
    pub section: Option<String>,
    pub note: Option<String>,
    pub must_open: bool,
    pub reason: Option<String>,
}

impl std::ops::Deref for Pointer {
    type Target = str;

    fn deref(&self) -> &str {
        &self.target
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PointerSource {
    Plain(String),
    Entry(PointerEntry),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PointerEntry {
    target: String,
    #[serde(default)]
    section: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    must_open: bool,
    #[serde(default)]
    reason: Option<String>,
}

impl From<PointerSource> for Pointer {
    fn from(source: PointerSource) -> Self {
        match source {
            PointerSource::Plain(target) => Self {
                target,
                section: None,
                note: None,
                must_open: false,
                reason: None,
            },
            PointerSource::Entry(entry) => Self {
                target: entry.target,
                section: entry.section,
                note: entry.note,
                must_open: entry.must_open,
                reason: entry.reason,
            },
        }
    }
}

fn applies(tiers: Option<&Vec<Tier>>, tier: Tier) -> bool {
    tiers.is_none_or(|tiers| tiers.contains(&tier))
}

/// The skill tree every harness can read; the Claude tree mirrors it.
pub const SHARED_SKILL_TREE: &str = ".agents/skills";

/// The agents a pointer may name. An agent is named, never invoked, and is
/// installed under `.claude/agents/`.
pub const AGENTS: &[&str] = &["cf-reviewer", "cf-security-reviewer"];

/// Whether a pointer names an agent rather than a skill.
#[must_use]
pub fn is_agent_pointer(pointer: &str) -> bool {
    AGENTS.contains(&pointer)
}

/// A skill pointer (`cf-name`) renders as its invocation and an agent as its
/// name; a skill-relative path (`cf-name/...`) renders as its path from the
/// repository root, so a cold reader opens it in one hop; any other path
/// renders as code.
#[must_use]
pub fn render_pointer(pointer: &str) -> String {
    if is_skill_pointer(pointer) {
        if is_agent_pointer(pointer) {
            return format!("`{pointer}`");
        }
        return format!("`/{pointer}`");
    }
    if pointer.starts_with("cf-") {
        return format!("`{SHARED_SKILL_TREE}/{pointer}`");
    }
    format!("`{pointer}`")
}

/// Whether a pointer names a skill or agent rather than a path.
#[must_use]
pub fn is_skill_pointer(pointer: &str) -> bool {
    pointer.starts_with("cf-") && !pointer.contains('/')
}

fn render_pointers(see: &[String]) -> String {
    see.iter()
        .map(|pointer| render_pointer(pointer))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One moment pointer as it reads in the table: `MUST OPEN` when the read
/// is required before acting, the target, the quoted section, the note and
/// the reason in parentheses.
#[must_use]
pub fn render_moment_pointer(pointer: &Pointer) -> String {
    let mut out = String::new();
    if pointer.must_open {
        out.push_str("MUST OPEN ");
    }
    out.push_str(&render_pointer(&pointer.target));
    if let Some(section) = &pointer.section {
        let _ = write!(out, " \"{section}\"");
    }
    if let Some(note) = &pointer.note {
        let _ = write!(out, " {note}");
    }
    if let Some(reason) = &pointer.reason {
        let _ = write!(out, " ({reason})");
    }
    out
}

/// A moment's pointers, separated by semicolons because a note or a reason
/// may hold a comma.
fn render_moment_pointers(see: &[Pointer]) -> String {
    see.iter()
        .map(render_moment_pointer)
        .collect::<Vec<_>>()
        .join("; ")
}

impl Kernel {
    /// Parse the embedded kernel.
    ///
    /// # Panics
    ///
    /// Never for the shipped kernel: its shape is pinned by the
    /// `rule_map_contract` test, so a malformed kernel fails the build's
    /// tests before it can ship.
    #[must_use]
    pub fn shipped() -> Self {
        Self::parse(KERNEL).expect("the shipped rule-map kernel parses")
    }

    /// Parse a kernel source.
    ///
    /// # Errors
    ///
    /// The TOML parse error, when the source is not a valid kernel.
    pub fn parse(source: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(source)
    }

    /// The always rules one tier's map carries, in kernel order.
    #[must_use]
    pub fn rules_for(&self, tier: Tier) -> Vec<&Rule> {
        self.rules
            .iter()
            .filter(|rule| applies(rule.tiers.as_ref(), tier))
            .collect()
    }

    /// The moment rows one tier's map carries: the delivery rows, then the
    /// situation rows, each in kernel order. This is the rendered order.
    #[must_use]
    pub fn moments_for(&self, tier: Tier) -> Vec<&Moment> {
        [Group::Delivery, Group::Situation]
            .into_iter()
            .flat_map(|group| {
                self.moments.iter().filter(move |moment| {
                    moment.group == group && applies(moment.tiers.as_ref(), tier)
                })
            })
            .collect()
    }

    /// Authoring problems the TOML shape cannot catch: a `MUST OPEN`
    /// pointer that states no reason, or a reason on a pointer that is not
    /// `MUST OPEN`. The contract test fails on any.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for moment in &self.moments {
            for pointer in &moment.see {
                match (pointer.must_open, pointer.reason.is_some()) {
                    (true, false) => problems.push(format!(
                        "moment {}: MUST OPEN {} states no reason",
                        moment.key, pointer.target
                    )),
                    (false, true) => problems.push(format!(
                        "moment {}: {} has a reason but is not MUST OPEN",
                        moment.key, pointer.target
                    )),
                    _ => {}
                }
            }
        }
        problems
    }

    /// One always rule as its rendered single line.
    #[must_use]
    pub fn render_rule(rule: &Rule) -> String {
        format!(
            "- **{}** {} See {}.",
            rule.title,
            rule.text,
            render_pointers(&rule.see)
        )
    }

    fn render_rules(&self, tier: Tier, out: &mut String) {
        for rule in self.rules_for(tier) {
            out.push_str(&Self::render_rule(rule));
            out.push('\n');
        }
    }

    fn render_moments(&self, tier: Tier, out: &mut String) {
        let rows = self.moments_for(tier);
        let mut first = true;
        for (group, lead) in [
            (Group::Delivery, DELIVERY_LEAD),
            (Group::Situation, SITUATION_LEAD),
        ] {
            let group_rows: Vec<&&Moment> =
                rows.iter().filter(|moment| moment.group == group).collect();
            if group_rows.is_empty() {
                continue;
            }
            if !first {
                out.push('\n');
            }
            first = false;
            let _ = write!(
                out,
                "{lead}\n\n| When you are about to | Do this | Read |\n|---|---|---|\n"
            );
            for moment in group_rows {
                let _ = writeln!(
                    out,
                    "| {} | {} | {} |",
                    moment.when,
                    moment.action,
                    render_moment_pointers(&moment.see)
                );
            }
        }
    }

    fn render_blocks(&self, blocks: &[Block], tier: Tier) -> String {
        let mut out = String::new();
        for block in blocks
            .iter()
            .filter(|block| applies(block.tiers.as_ref(), tier))
        {
            if !out.is_empty() {
                out.push('\n');
            }
            let text = block.text.trim_matches('\n');
            out.push_str(text);
            out.push('\n');
            match block.kind {
                BlockKind::Text => {}
                BlockKind::Rules => {
                    out.push('\n');
                    self.render_rules(tier, &mut out);
                }
                BlockKind::Moments => {
                    out.push('\n');
                    self.render_moments(tier, &mut out);
                }
            }
        }
        out
    }

    /// Render one template.
    #[must_use]
    pub fn render(&self, output: &Output) -> String {
        match output.file {
            File::Agents => self.render_blocks(&self.agents, output.tier),
            File::Claude => self.render_blocks(&self.claude, output.tier),
        }
    }

    /// Every tier the kernel renders for.
    #[must_use]
    pub fn tiers() -> [Tier; 3] {
        ALL_TIERS
    }
}

/// The managed block of a rendered `AGENTS.md`, markers included, or `None`
/// when the text carries no complete marker pair.
#[must_use]
pub fn managed_block(text: &str) -> Option<&str> {
    let start = text.find("<!-- codeflow:managed:begin")?;
    let end_marker = "<!-- codeflow:managed:end -->";
    let end = text[start..].find(end_marker)? + start + end_marker.len();
    Some(&text[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_renders_as_one_line_with_its_pointers() {
        let rule = Rule {
            id: "x".into(),
            tiers: None,
            title: "Title.".into(),
            text: "Text.".into(),
            see: vec!["cf-estimate".into(), ".codeflow/rules/writing.md".into()],
        };
        assert_eq!(
            Kernel::render_rule(&rule),
            "- **Title.** Text. See `/cf-estimate`, `.codeflow/rules/writing.md`."
        );
    }

    #[test]
    fn pointers_render_as_their_invocation_or_root_path() {
        assert_eq!(render_pointer("cf-reviewer"), "`cf-reviewer`");
        assert_eq!(
            render_pointer("cf-ship/references/release-policy.md"),
            "`.agents/skills/cf-ship/references/release-policy.md`"
        );
        assert!(!is_skill_pointer("cf-ship/references/release-policy.md"));
    }

    #[test]
    fn tier_filters_select_only_matching_blocks() {
        let kernel = Kernel::parse(
            r#"
schema_version = 1
[[agents]]
text = "all"
[[agents]]
tiers = ["minimal"]
text = "min"
"#,
        )
        .unwrap();
        let minimal = kernel.render(&OUTPUTS[0]);
        let standard = kernel.render(&OUTPUTS[1]);
        assert_eq!(minimal, "all\n\nmin\n");
        assert_eq!(standard, "all\n");
    }

    #[test]
    fn codex_overflow_measures_the_whole_document() {
        assert_eq!(
            codex_overflow(&"x".repeat(CODEX_INSTRUCTION_LIMIT_BYTES)),
            None
        );
        assert_eq!(
            codex_overflow(&"x".repeat(CODEX_INSTRUCTION_LIMIT_BYTES + 3)),
            Some(3)
        );
    }

    const TWO_GROUPS: &str = r###"
schema_version = 1
[[agents]]
kind = "moments"
text = "## When you are about to"
[[moment]]
key = "a"
group = "delivery"
when = "start a task"
action = "check first"
see = [".codeflow/rules/worktrees.md"]
[[moment]]
key = "b"
group = "situation"
when = "meet a refusal"
action = "fix the cause"
see = [{ target = "cf-plan", must_open = true, reason = "the verbs" }, ".codeflow/rules/git-rules.md"]
"###;

    /// TSK-184 WP1: delivery rows and situation rows render as two tables,
    /// each after its lead sentence.
    #[test]
    fn moments_render_as_two_tables_with_their_leads() {
        let kernel = Kernel::parse(TWO_GROUPS).unwrap();
        let header = "| When you are about to | Do this | Read |\n|---|---|---|\n";
        assert_eq!(
            kernel.render(&OUTPUTS[1]),
            format!(
                "## When you are about to\n\n{DELIVERY_LEAD}\n\n{header}\
                 | start a task | check first | `.codeflow/rules/worktrees.md` |\n\n\
                 {SITUATION_LEAD}\n\n{header}\
                 | meet a refusal | fix the cause | MUST OPEN `/cf-plan` (the verbs); \
                 `.codeflow/rules/git-rules.md` |\n"
            )
        );
        assert_eq!(kernel.moments[0].group, Group::Delivery);
        assert_eq!(kernel.moments[1].group, Group::Situation);
    }

    /// TSK-184 WP1: a pointer entry carries a section, a note, the
    /// must-open mark and its reason; a plain string stays a plain pointer.
    #[test]
    fn a_must_open_pointer_renders_its_section_note_and_reason() {
        let kernel = Kernel::parse(
            r#"
schema_version = 1
[[moment]]
key = "cleanup"
group = "delivery"
when = "clean up"
action = "prove the merge"
see = [
  { target = ".codeflow/rules/worktrees.md", section = "Cleanup", note = "before removing anything", must_open = true, reason = "the proof rules" },
  { target = "cf-method", section = "Managing a body of work" },
  "cf-ship/references/release-policy.md",
]
"#,
        )
        .unwrap();
        let see = &kernel.moments[0].see;
        assert_eq!(
            render_moment_pointer(&see[0]),
            "MUST OPEN `.codeflow/rules/worktrees.md` \"Cleanup\" before removing anything \
             (the proof rules)"
        );
        assert_eq!(
            render_moment_pointer(&see[1]),
            "`/cf-method` \"Managing a body of work\""
        );
        assert_eq!(
            render_moment_pointer(&see[2]),
            "`.agents/skills/cf-ship/references/release-policy.md`"
        );
        // A pointer reads as its target wherever a plain path is expected.
        assert_eq!(&*see[1], "cf-method");
        assert_eq!(see[0].section.as_deref(), Some("Cleanup"));
        assert!(kernel.problems().is_empty(), "{:?}", kernel.problems());
    }

    /// TSK-184 WP1: every `MUST OPEN` pointer states its reason, and a
    /// reason belongs only to a must-open pointer.
    #[test]
    fn a_must_open_pointer_without_its_reason_is_a_kernel_problem() {
        let kernel = Kernel::parse(
            r#"
schema_version = 1
[[moment]]
key = "a"
group = "delivery"
when = "w"
action = "a"
see = [{ target = "cf-plan", must_open = true }, { target = "cf-ship", reason = "stray" }]
"#,
        )
        .unwrap();
        assert_eq!(
            kernel.problems(),
            vec![
                "moment a: MUST OPEN cf-plan states no reason".to_string(),
                "moment a: cf-ship has a reason but is not MUST OPEN".to_string(),
            ]
        );
        assert!(
            Kernel::parse(
                "schema_version = 1\n[[moment]]\nkey = \"a\"\nwhen = \"w\"\naction = \"a\"\nsee = [\"x\"]\n"
            )
            .is_err(),
            "a moment without its group must not parse"
        );
    }

    #[test]
    fn agents_are_named_not_invoked() {
        assert_eq!(
            render_pointer("cf-security-reviewer"),
            "`cf-security-reviewer`"
        );
        assert!(is_agent_pointer("cf-security-reviewer"));
        assert!(!is_agent_pointer("cf-plan"));
    }

    /// The managed-block guideline is 12 KiB, reported and never failed.
    #[test]
    fn the_block_guideline_is_twelve_kib() {
        assert_eq!(MANAGED_BLOCK_GUIDELINE_BYTES, 12 * 1024);
    }

    #[test]
    fn managed_block_spans_both_markers() {
        let text =
            "head\n<!-- codeflow:managed:begin x -->\nbody\n<!-- codeflow:managed:end -->\ntail";
        assert_eq!(
            managed_block(text),
            Some("<!-- codeflow:managed:begin x -->\nbody\n<!-- codeflow:managed:end -->")
        );
        assert_eq!(managed_block("no markers"), None);
    }
}
