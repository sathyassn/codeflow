//! The root rule map: every tier's `AGENTS.md` and `CLAUDE.md` template,
//! rendered from one kernel source (`assets/base/rule-map.toml`, TSK-127).
//!
//! The kernel holds the always rules, the moment table and the tier prose.
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
pub const RULES_GUIDELINE: usize = 12;

/// Guideline for the bytes of one rendered always rule. Reported, never a
/// failure; one line per rule is the structural rule.
pub const RULE_LINE_GUIDELINE_BYTES: usize = 450;

/// Guideline for the managed block of one tier's `AGENTS.md`, markers
/// included: the always-read kernel. It leaves a realistic project section
/// room under Codex's instruction limit. `codeflow doctor` reports the kernel
/// against it; it is never a failure (TSK-150).
pub const MANAGED_BLOCK_GUIDELINE_BYTES: usize = 10 * 1024;

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

/// One row of the "when you are about to" table.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Moment {
    pub key: String,
    #[serde(default)]
    pub tiers: Option<Vec<Tier>>,
    pub when: String,
    pub action: String,
    pub see: Vec<String>,
}

fn applies(tiers: Option<&Vec<Tier>>, tier: Tier) -> bool {
    tiers.is_none_or(|tiers| tiers.contains(&tier))
}

/// The skill tree every harness can read; the Claude tree mirrors it.
pub const SHARED_SKILL_TREE: &str = ".agents/skills";

/// A skill or agent pointer (`cf-name`) renders as its invocation; a
/// skill-relative path (`cf-name/...`) renders as its path from the
/// repository root, so a cold reader opens it in one hop; any other path
/// renders as code.
#[must_use]
pub fn render_pointer(pointer: &str) -> String {
    if is_skill_pointer(pointer) {
        if pointer == "cf-reviewer" {
            // An agent, not a skill: named, not invoked.
            return "`cf-reviewer`".to_string();
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

    /// The moment rows one tier's map carries, in kernel order.
    #[must_use]
    pub fn moments_for(&self, tier: Tier) -> Vec<&Moment> {
        self.moments
            .iter()
            .filter(|moment| applies(moment.tiers.as_ref(), tier))
            .collect()
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
        out.push_str("| When you are about to | Do this | Read |\n|---|---|---|\n");
        for moment in self.moments_for(tier) {
            let _ = writeln!(
                out,
                "| {} | {} | {} |",
                moment.when,
                moment.action,
                render_pointers(&moment.see)
            );
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
