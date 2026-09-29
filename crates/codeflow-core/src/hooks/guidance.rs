//! Rules re-injected where they were lost or where they apply (TSK-128).
//!
//! Two short texts for the project's tier, generated from the rule-map
//! kernel (`assets/base/rule-map.toml`, TSK-127) and the scaffold manifest:
//!
//! - the **guidance block** the session-orient hook adds when a session
//!   resumes, forks or restarts after compaction (source `compact`,
//!   `resume` or `fork`): the always rules by title, the moment table by its
//!   first pointer, and every skill and agent the tier installs;
//! - the **prompt reminder** the same hook adds on `UserPromptSubmit` when a
//!   prompt asks for a duration, a status or a complex explanation: exactly
//!   one rule line, or nothing.
//!
//! One stable command, `codeflow hook session-orient`, carries both events
//! and dispatches on the payload's `hook_event_name`, so an older binary
//! that only knows that command still exits 0 on a prompt (it prints its
//! digest) instead of refusing the prompt with a parse error. Both texts are
//! advisory. Neither can fail a session or a prompt: a missing or unreadable
//! kernel, project state or policy file yields no text, or the default
//! level, never an error. Their sizes are guidelines, not caps
//! ([`GUIDANCE_BLOCK_GUIDELINE_BYTES`], [`REMINDER_LINE_GUIDELINE_BYTES`]).

use std::fmt::Write as _;
use std::path::Path;

use crate::scaffold::rule_map::{is_skill_pointer, Kernel, Moment, KERNEL};
use crate::scaffold::{ScaffoldManifest, Tier};

use super::policy::{read_project_toml, Policy};

/// The size the compaction guidance block aims for. A guideline, not a
/// cap: no skill or rule is dropped to meet it, and the tests print the
/// measured size against it.
pub const GUIDANCE_BLOCK_GUIDELINE_BYTES: usize = 1536;

/// The size one prompt reminder line aims for; a guideline, not a cap.
pub const REMINDER_LINE_GUIDELINE_BYTES: usize = 300;

/// The scaffold manifest, the inventory of what each tier installs.
pub const SCAFFOLD_MANIFEST: &str = include_str!("../../../../assets/base/scaffold-manifest.toml");

/// The session-start sources after which the rules are gone or stale.
pub const GUIDANCE_SOURCES: [&str; 3] = ["compact", "resume", "fork"];

/// What a prompt asks for, when it asks for something a rule governs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// A duration, date or effort.
    Duration,
    /// A status report or summary of work.
    Status,
    /// A multi-part explanation, comparison or decision.
    Explanation,
}

impl Trigger {
    /// Every trigger in priority order: when a prompt matches several, the
    /// first one here wins, so the hook still adds exactly one line.
    pub const ALL: [Self; 3] = [Self::Duration, Self::Status, Self::Explanation];

    /// The kernel moment whose row the reminder carries.
    #[must_use]
    pub fn moment_key(self) -> &'static str {
        match self {
            Self::Duration => "estimate",
            Self::Status => "status",
            Self::Explanation => "explanation",
        }
    }

    /// The kernel always rule whose title heads the reminder.
    #[must_use]
    pub fn rule_id(self) -> &'static str {
        match self {
            Self::Duration => "estimate",
            Self::Status => "writing",
            Self::Explanation => "present",
        }
    }

    /// Word sequences that signal this trigger.
    fn phrases(self) -> &'static [&'static [&'static str]] {
        match self {
            Self::Duration => &[
                &["how", "long"],
                &["how", "much", "time"],
                &["how", "many", "days"],
                &["how", "many", "weeks"],
                &["how", "many", "hours"],
                &["how", "many", "months"],
                &["how", "many", "sprints"],
                &["how", "soon"],
                &["by", "when"],
                &["when", "will"],
                &["when", "can"],
                &["estimate"],
                &["estimates"],
                &["estimated"],
                &["estimation"],
                &["eta"],
                &["timeline"],
                &["timeframe"],
                &["time", "frame"],
                &["deadline"],
                &["duration"],
                &["effort"],
                &["person-days"],
                &["man-days"],
                &["story", "points"],
            ],
            Self::Status => &[
                &["status"],
                &["progress"],
                &["where", "are", "we"],
                &["where", "do", "we", "stand"],
                &["what's", "left"],
                &["what", "is", "left"],
                &["what", "remains"],
                &["what", "have", "you", "done"],
                &["what", "did", "you", "do"],
                &["what", "got", "done"],
                &["summarize"],
                &["summarise"],
                &["summary"],
                &["recap"],
                &["catch", "me", "up"],
                &["update", "me"],
                &["standup"],
                &["stand-up"],
            ],
            Self::Explanation => &[
                &["explain"],
                &["explaining"],
                &["explanation"],
                &["walk", "me", "through"],
                &["walk", "through"],
                &["compare"],
                &["comparing"],
                &["comparison"],
                &["tradeoff"],
                &["tradeoffs"],
                &["trade-off"],
                &["trade-offs"],
                &["pros", "and", "cons"],
                &["difference", "between"],
                &["differences", "between"],
                &["versus"],
                &["diagram"],
                &["overview"],
                &["break", "down"],
            ],
        }
    }

    /// Word sequences that look like a trigger but are not one (a shell
    /// command or a UI element named by the same word).
    fn exclusions(self) -> &'static [&'static [&'static str]] {
        match self {
            Self::Status => &[
                &["git", "status"],
                &["exit", "status"],
                &["status", "code"],
                &["status", "codes"],
                &["status", "line"],
                &["status", "bar"],
                &["progress", "bar"],
            ],
            Self::Duration | Self::Explanation => &[],
        }
    }
}

/// Lower-cased words of `text`. Hyphens, underscores and apostrophes stay
/// inside a word, so `cf-estimate` or `status_code` is one word and does not
/// match `estimate` or `status`; every other non-alphanumeric character
/// separates words.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('\u{2019}', "'")
        .split(|c: char| !(c.is_alphanumeric() || matches!(c, '-' | '_' | '\'')))
        .map(|word| word.trim_matches(|c| matches!(c, '-' | '_' | '\'')))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

fn phrase_at(words: &[String], at: usize, phrase: &[&str]) -> bool {
    words.len() >= at + phrase.len()
        && phrase
            .iter()
            .zip(&words[at..])
            .all(|(want, word)| *want == word)
}

/// Whether `phrase` occurs at `at` outside every exclusion that covers it.
fn counts_at(words: &[String], at: usize, phrase: &[&str], trigger: Trigger) -> bool {
    if !phrase_at(words, at, phrase) {
        return false;
    }
    !trigger.exclusions().iter().any(|exclusion| {
        (0..exclusion.len()).any(|offset| {
            at >= offset
                && phrase_at(words, at - offset, exclusion)
                && at - offset + exclusion.len() >= at + phrase.len()
        })
    })
}

fn matches(words: &[String], trigger: Trigger) -> bool {
    (0..words.len()).any(|at| {
        trigger
            .phrases()
            .iter()
            .any(|phrase| counts_at(words, at, phrase, trigger))
    })
}

/// The one trigger a prompt matches, by priority, or `None`.
#[must_use]
pub fn classify(prompt: &str) -> Option<Trigger> {
    let words = words(prompt);
    Trigger::ALL
        .into_iter()
        .find(|trigger| matches(&words, *trigger))
}

/// A pointer as plain text: a skill as its invocation, a skill-relative or
/// project path as written.
fn plain_pointer(pointer: &str) -> String {
    if is_skill_pointer(pointer) && pointer != "cf-reviewer" {
        format!("/{pointer}")
    } else {
        pointer.to_string()
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn moment<'k>(kernel: &'k Kernel, tier: Tier, key: &str) -> Option<&'k Moment> {
    kernel
        .moments_for(tier)
        .into_iter()
        .find(|moment| moment.key == key)
}

/// The reminder line for `trigger` at `tier`: the always rule's title, the
/// moment's action and its pointers. `None` when the kernel lacks the rule
/// or the moment.
#[must_use]
pub fn reminder_line(kernel: &Kernel, tier: Tier, trigger: Trigger) -> Option<String> {
    let rule = kernel
        .rules_for(tier)
        .into_iter()
        .find(|rule| rule.id == trigger.rule_id())?;
    let moment = moment(kernel, tier, trigger.moment_key())?;
    let see = moment
        .see
        .iter()
        .map(|pointer| plain_pointer(pointer))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "codeflow reminder: {} {}. See {see}.",
        rule.title,
        capitalized(&moment.action)
    ))
}

/// What a tier installs for the agent: its skills and its agents, each
/// sorted by name, read from the scaffold manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    pub skills: Vec<String>,
    pub agents: Vec<String>,
}

/// The skills (`.agents/skills/<name>/SKILL.md`) and agents
/// (`.claude/agents/<name>.md`) the manifest source installs at `tier`, or
/// an empty inventory when the source does not parse.
#[must_use]
pub fn installed_inventory(manifest: &str, tier: Tier) -> Inventory {
    let Ok(manifest) = toml::from_str::<ScaffoldManifest>(manifest) else {
        return Inventory::default();
    };
    let mut inventory = Inventory::default();
    for entry in manifest.entries.iter().filter(|e| e.tiers.contains(&tier)) {
        let dest = entry.dest.as_str();
        if let Some(name) = dest
            .strip_prefix(".agents/skills/")
            .and_then(|rest| rest.strip_suffix("/SKILL.md"))
            .filter(|name| !name.contains('/'))
        {
            inventory.skills.push(name.to_string());
        } else if let Some(name) = dest
            .strip_prefix(".claude/agents/")
            .and_then(|rest| rest.strip_suffix(".md"))
            .filter(|name| !name.contains('/'))
        {
            inventory.agents.push(name.to_string());
        }
    }
    for names in [&mut inventory.skills, &mut inventory.agents] {
        names.sort();
        names.dedup();
    }
    inventory
}

/// The guidance block for `tier`: always rules by title, the moment table
/// by its first pointer, and the tier's installed skills and agents.
#[must_use]
pub fn guidance_block(kernel: &Kernel, inventory: &Inventory, tier: Tier) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "## Rules after compaction or resume ({tier} tier)\n\
         Full rules: the AGENTS.md managed block; re-read it before acting."
    );
    let titles: Vec<&str> = kernel
        .rules_for(tier)
        .into_iter()
        .map(|rule| rule.title.as_str())
        .collect();
    let _ = writeln!(out, "Always: {}", titles.join(" "));
    let _ = writeln!(out, "When you are about to:");
    for moment in kernel.moments_for(tier) {
        let see = moment
            .see
            .first()
            .map(|pointer| plain_pointer(pointer))
            .unwrap_or_default();
        let _ = writeln!(out, "- {}: {see}", moment.when);
    }
    if inventory.skills.is_empty() {
        let _ = writeln!(out, "Skills: none at this tier.");
    } else {
        let _ = writeln!(out, "Skills: {}.", inventory.skills.join(", "));
    }
    if !inventory.agents.is_empty() {
        let _ = writeln!(out, "Agents: {}.", inventory.agents.join(", "));
    }
    out
}

/// The project's tier from `.codeflow/project.toml`, or `None` when the file
/// or the key is missing or unreadable.
#[must_use]
pub fn project_tier(root: &Path) -> Option<Tier> {
    read_project_toml(root)?.get("tier")?.as_str()?.parse().ok()
}

/// Whether a session-start source is one after which the rules are gone
/// or stale: a compaction, a resumed session or a forked one (Claude Code's
/// `fork` carries an older conversation, so it is treated as a resume).
#[must_use]
pub fn source_needs_guidance(source: &str) -> bool {
    GUIDANCE_SOURCES.contains(&source)
}

/// Which lifecycle event a hook payload reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEvent {
    /// `SessionStart`, or a payload that names no event (the legacy input
    /// of `codeflow hook session-orient`).
    SessionStart,
    /// `UserPromptSubmit`.
    PromptSubmit,
    /// Any other named event: the advisory hook adds nothing.
    Other(String),
}

/// The event a hook payload names in `hook_event_name`; a payload that
/// names none, or does not parse, is a session start, as before.
#[must_use]
pub fn payload_event(payload: &str) -> HookEvent {
    let name = serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("hook_event_name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        });
    match name.as_deref() {
        None | Some("SessionStart") => HookEvent::SessionStart,
        Some("UserPromptSubmit") => HookEvent::PromptSubmit,
        Some(other) => HookEvent::Other(other.to_string()),
    }
}

/// The `source` field of a session-start hook payload, read leniently: an
/// unreadable payload has no source.
#[must_use]
pub fn payload_source(payload: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    value.get("source")?.as_str().map(str::to_string)
}

/// The guidance block for the project at `root` after `source`, from the
/// kernel source `kernel`; `None` for any other source, or when the tier or
/// the kernel cannot be read.
#[must_use]
pub fn session_guidance_with(root: &Path, source: &str, kernel: &str) -> Option<String> {
    if !source_needs_guidance(source) {
        return None;
    }
    let tier = project_tier(root)?;
    let kernel = Kernel::parse(kernel).ok()?;
    Some(guidance_block(
        &kernel,
        &installed_inventory(SCAFFOLD_MANIFEST, tier),
        tier,
    ))
}

/// [`session_guidance_with`] over the shipped kernel.
#[must_use]
pub fn session_guidance(root: &Path, source: &str) -> Option<String> {
    session_guidance_with(root, source, KERNEL)
}

/// The prompt text of a prompt-submit hook payload, read leniently.
#[must_use]
pub fn payload_prompt(payload: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    value.get("prompt")?.as_str().map(str::to_string)
}

/// The reminder for the project at `root` and the prompt-submit `payload`,
/// from the kernel source `kernel`: one line when the prompt matches a
/// trigger and `guidance.prompt_reminders` is active, else `None`. A missing
/// policy file means the default level (`warn`); a missing tier or kernel
/// means no reminder.
#[must_use]
pub fn prompt_reminder_with(root: &Path, payload: &str, kernel: &str) -> Option<String> {
    if !Policy::load(root).guidance.prompt_reminders.is_active() {
        return None;
    }
    let trigger = classify(&payload_prompt(payload)?)?;
    let tier = project_tier(root)?;
    let kernel = Kernel::parse(kernel).ok()?;
    reminder_line(&kernel, tier, trigger)
}

/// [`prompt_reminder_with`] over the shipped kernel.
#[must_use]
pub fn prompt_reminder(root: &Path, payload: &str) -> Option<String> {
    prompt_reminder_with(root, payload, KERNEL)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_project(root: &Path, tier: &str) {
        std::fs::create_dir_all(root.join(".codeflow")).unwrap();
        std::fs::write(
            root.join(".codeflow/project.toml"),
            format!("tier = \"{tier}\"\n"),
        )
        .unwrap();
    }

    #[test]
    fn words_keep_hyphenated_names_whole() {
        assert_eq!(
            words("Run /cf-estimate, then git-status_code it's"),
            vec!["run", "cf-estimate", "then", "git-status_code", "it's"]
        );
    }

    #[test]
    fn exclusions_cover_only_their_own_words() {
        assert_eq!(classify("run git status please"), None);
        assert_eq!(
            classify("git status is clean, what is the status of the release?"),
            Some(Trigger::Status)
        );
    }

    #[test]
    fn priority_picks_one_trigger() {
        assert_eq!(
            classify("explain the status and how long it takes"),
            Some(Trigger::Duration)
        );
    }

    /// One line per trigger at every tier; the measured size is printed
    /// against the guideline, not asserted.
    #[test]
    fn every_trigger_has_one_line_at_every_tier() {
        let kernel = Kernel::shipped();
        for tier in Kernel::tiers() {
            for trigger in Trigger::ALL {
                let line = reminder_line(&kernel, tier, trigger)
                    .unwrap_or_else(|| panic!("{tier}: {trigger:?} has no line"));
                assert!(!line.contains('\n'), "{line}");
                assert!(line.starts_with("codeflow reminder: "), "{line}");
                println!(
                    "reminder {tier} {trigger:?}: {} bytes (guideline {REMINDER_LINE_GUIDELINE_BYTES})",
                    line.len()
                );
            }
        }
    }

    /// The block holds every always rule, every moment and the tier's whole
    /// inventory; its size is printed against the guideline.
    #[test]
    fn guidance_holds_the_kernel_and_the_whole_inventory_at_every_tier() {
        let kernel = Kernel::shipped();
        for tier in Kernel::tiers() {
            let inventory = installed_inventory(SCAFFOLD_MANIFEST, tier);
            let block = guidance_block(&kernel, &inventory, tier);
            for rule in kernel.rules_for(tier) {
                assert!(block.contains(&rule.title), "{tier}: {}", rule.title);
            }
            for moment in kernel.moments_for(tier) {
                assert!(
                    block.contains(&format!("- {}: ", moment.when)),
                    "{tier}: {}",
                    moment.when
                );
            }
            for name in inventory.skills.iter().chain(&inventory.agents) {
                assert!(block.contains(name.as_str()), "{tier}: {name}");
            }
            println!(
                "guidance {tier}: {} bytes (guideline {GUIDANCE_BLOCK_GUIDELINE_BYTES})",
                block.len()
            );
        }
    }

    #[test]
    fn session_guidance_follows_the_source() {
        let dir = tempfile::tempdir().unwrap();
        write_project(dir.path(), "standard");
        for source in ["compact", "resume", "fork"] {
            assert!(session_guidance(dir.path(), source).is_some(), "{source}");
        }
        for source in ["startup", "clear", ""] {
            assert!(session_guidance(dir.path(), source).is_none(), "{source}");
        }
    }

    #[test]
    fn missing_kernel_or_tier_yields_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let payload = r#"{"prompt":"how long will this take?"}"#;
        assert!(prompt_reminder(dir.path(), payload).is_none(), "no tier");
        assert!(session_guidance(dir.path(), "compact").is_none(), "no tier");
        write_project(dir.path(), "full");
        for kernel in ["", "not = [toml", "schema_version = 1\n"] {
            assert!(prompt_reminder_with(dir.path(), payload, kernel).is_none());
        }
        assert!(session_guidance_with(dir.path(), "compact", "").is_none());
        assert!(prompt_reminder(dir.path(), payload).is_some());
    }

    #[test]
    fn unreadable_payloads_yield_nothing() {
        let dir = tempfile::tempdir().unwrap();
        write_project(dir.path(), "full");
        for payload in ["", "not json", "{}", r#"{"prompt":7}"#] {
            assert!(prompt_reminder(dir.path(), payload).is_none(), "{payload}");
        }
        assert_eq!(
            payload_source(r#"{"source":"compact"}"#).as_deref(),
            Some("compact")
        );
        assert_eq!(payload_source("garbage"), None);
    }

    #[test]
    fn inventory_keeps_agents_apart_from_skills() {
        let minimal = installed_inventory(SCAFFOLD_MANIFEST, Tier::Minimal);
        assert!(minimal.skills.is_empty() && minimal.agents.is_empty());
        let standard = installed_inventory(SCAFFOLD_MANIFEST, Tier::Standard);
        assert!(standard.agents.iter().any(|a| a == "cf-reviewer"));
        assert!(!standard.skills.iter().any(|s| s == "cf-reviewer"));
        for name in [
            "cf-customize",
            "cf-docs-portal",
            "cf-editorial-review",
            "cf-herdr",
            "cf-stack",
        ] {
            assert!(standard.skills.iter().any(|s| s == name), "{name}");
        }
        assert_eq!(
            installed_inventory("not toml", Tier::Full),
            Inventory::default()
        );
    }

    #[test]
    fn events_dispatch_by_name_and_default_to_session_start() {
        assert_eq!(
            payload_event(r#"{"hook_event_name":"UserPromptSubmit"}"#),
            HookEvent::PromptSubmit
        );
        for payload in [
            "",
            "not json",
            "{}",
            r#"{"hook_event_name":"SessionStart"}"#,
        ] {
            assert_eq!(payload_event(payload), HookEvent::SessionStart, "{payload}");
        }
        assert_eq!(
            payload_event(r#"{"hook_event_name":"PreCompact"}"#),
            HookEvent::Other("PreCompact".into())
        );
    }
}
