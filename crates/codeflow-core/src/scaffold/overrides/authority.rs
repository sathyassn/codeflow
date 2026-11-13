//! What the running binary says each unit of one managed file is, and the
//! check of a persisted override record against it (TSK-1302).
//!
//! A record is project-written state. Before update merges against it or
//! doctor reads it as evidence, every unit it holds is checked against the
//! binary's shipped text: the name is one this binary declares for the file
//! with class `overridable`, the range it replaced is the one the binary
//! finds for that name in the verified pristine baseline, and the
//! replacement is that range rebuilt with only the fields the unit lets a
//! project replace: a moment's action, a rule's title and text, a section's
//! body. A record made from an older shipped text stays valid, because the
//! range is found in the baseline the record was made from, by the unit's
//! identity: an always rule by its bold title, a moment row by its first
//! cell, a section by its marker. Its pointers stay as that baseline has
//! them.

use std::collections::{BTreeMap, BTreeSet};

use super::sections;
use super::{row_value_ok, AppliedUnit, Entry, Target};
use crate::scaffold::rule_map::{Kernel, Moment, Rule, UnitClass};
use crate::scaffold::Tier;

/// One always rule or moment row as the binary ships it for a file.
#[derive(Debug, Clone)]
enum Row {
    Rule(Rule),
    Moment(Moment),
}

/// The units of one managed file, from the binary's shipped text.
#[derive(Debug, Clone, Default)]
pub struct Authority {
    /// The shared home the file's rule lines leave out
    /// ([`Kernel::rule_home`]).
    home: Option<String>,
    /// Every row the file renders, by unit name, with its class; empty for a
    /// skill file, so no row unit is allowed there.
    rows: BTreeMap<String, (Row, Option<UnitClass>)>,
    /// The skill a skill file is, and its sections that may be overridden,
    /// with their heading levels.
    skill: Option<String>,
    sections: BTreeMap<String, usize>,
}

impl Authority {
    /// The rows of an `AGENTS.md` file of `tier`.
    #[must_use]
    pub fn for_agents(kernel: &Kernel, tier: Tier) -> Self {
        let mut rows = BTreeMap::new();
        for rule in kernel.rules_for(tier) {
            rows.insert(
                format!("rule:{}", rule.id),
                (Row::Rule(rule.clone()), rule.class),
            );
        }
        for moment in kernel.moments_for(tier) {
            rows.insert(
                format!("moment:{}", moment.key),
                (Row::Moment(moment.clone()), moment.class),
            );
        }
        Self {
            home: kernel.rule_home(tier).map(str::to_string),
            rows,
            ..Self::default()
        }
    }

    /// The overridable sections of `skill`, as its shipped `rendered` text
    /// declares them.
    #[must_use]
    pub fn for_skill(skill: &str, rendered: &str) -> Self {
        let sections = sections::parse(rendered)
            .map(|doc| {
                doc.sections()
                    .filter(|section| section.class == Some(UnitClass::Overridable))
                    .map(|section| (section.id.clone(), section.level))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            skill: Some(skill.to_string()),
            sections,
            ..Self::default()
        }
    }

    /// Whether every unit of a record is one this binary lets a project
    /// override in this file, at the place the binary finds it in
    /// `baseline`, with a replacement that changes only the unit's declared
    /// fields. `baseline` is the
    /// verified pristine baseline the record was made from.
    #[must_use]
    pub fn allows(&self, baseline: &str, units: &[AppliedUnit]) -> bool {
        let mut seen = BTreeSet::new();
        let mut doc = None;
        units.iter().all(|unit| {
            let Some(target) = Target::parse(&unit.entry.target) else {
                return false;
            };
            if !seen.insert(target.unit()) {
                return false;
            }
            match target {
                Target::Rule(_) | Target::Moment(_) => self.allows_row(baseline, unit),
                Target::Section { skill, id } => {
                    if self.skill.as_deref() != Some(skill.as_str()) {
                        return false;
                    }
                    let Some(&level) = self.sections.get(&id) else {
                        return false;
                    };
                    if doc.is_none() {
                        doc = sections::parse(baseline).ok();
                    }
                    let Some(doc) = doc.as_ref() else {
                        return false;
                    };
                    let shipped = doc.section(&id).is_some_and(|section| {
                        section.class == Some(UnitClass::Overridable) && section.level == level
                    });
                    shipped
                        && doc
                            .body_range(&id)
                            .and_then(|range| baseline.get(range))
                            .is_some_and(|body| body == unit.replaced)
                        && unit.entry.title.is_none()
                        && unit.entry.text.is_none()
                        && unit.entry.action.is_none()
                        && unit
                            .entry
                            .body
                            .as_deref()
                            .is_some_and(|body| sections::normalized_body(body) == unit.replacement)
                        && sections::check_body(&unit.replacement, level).is_ok()
                }
            }
        })
    }

    fn allows_row(&self, baseline: &str, unit: &AppliedUnit) -> bool {
        let home = self.home.as_deref();
        let Some((row, class)) = self.rows.get(&unit.entry.target) else {
            return false;
        };
        if *class != Some(UnitClass::Overridable) {
            return false;
        }
        let entry = &unit.entry;
        let (identity, current, replacement) = match row {
            Row::Rule(rule) => {
                if entry.action.is_some()
                    || entry.body.is_some()
                    || (entry.text.is_none() && entry.title.is_none())
                    || !entry.text.as_deref().is_none_or(|v| row_value_ok(v, false))
                    || !entry
                        .title
                        .as_deref()
                        .is_none_or(|v| row_value_ok(v, false))
                {
                    return false;
                }
                let mut replaced = rule.clone();
                if let Some(title) = &entry.title {
                    replaced.title.clone_from(title);
                }
                if let Some(text) = &entry.text {
                    replaced.text.clone_from(text);
                }
                (
                    format!("- **{}** ", rule.title),
                    Kernel::render_rule_with_home(home, rule),
                    Kernel::render_rule_with_home(home, &replaced),
                )
            }
            Row::Moment(moment) => {
                if entry.title.is_some()
                    || entry.text.is_some()
                    || entry.body.is_some()
                    || !entry
                        .action
                        .as_deref()
                        .is_some_and(|v| row_value_ok(v, true))
                {
                    return false;
                }
                let mut replaced = moment.clone();
                if let Some(action) = &entry.action {
                    replaced.action.clone_from(action);
                }
                (
                    format!("| {} | ", moment.when),
                    Kernel::render_moment(moment),
                    Kernel::render_moment(&replaced),
                )
            }
        };
        // The range the binary finds for this name: the one line of the
        // baseline that starts with the unit's identity.
        let mut found = baseline
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .filter(|line| line.starts_with(&identity));
        let (Some(line), None) = (found.next(), found.next()) else {
            return false;
        };
        if line != unit.replaced {
            return false;
        }
        if unit.replaced == current {
            // The baseline is this binary's own rendering: the replacement is
            // exactly what it renders for the declared fields.
            return unit.replacement == replacement;
        }
        // An older shipped row: rebuilt from the baseline line's own parts,
        // with only the fields the entry declares replaced.
        rebuild_older(row, entry, &identity, line)
            .is_some_and(|rebuilt| unit.replacement == rebuilt)
    }
}

/// An older shipped row, `line`, rebuilt with the fields `entry` declares.
/// Every other part stays byte for byte as the baseline has it: a moment
/// keeps its first cell and its pointer cell, `MUST OPEN` marks included, and
/// takes `entry.action`; a rule keeps its pointer part (` See ...`) and takes
/// the entry's title and text, each defaulting to the baseline's own.
/// `None` when the line does not split into those parts.
fn rebuild_older(row: &Row, entry: &Entry, identity: &str, line: &str) -> Option<String> {
    let rest = line.strip_prefix(identity)?;
    match row {
        Row::Moment(_) => {
            // The action cell holds no `|`, so the first separator ends it;
            // the pointer cell and the row's closing `|` follow unchanged.
            let action = entry.action.as_deref()?;
            let (_, pointers) = rest.split_once(" | ")?;
            Some(format!("{identity}{action} | {pointers}"))
        }
        Row::Rule(rule) => {
            let title = entry.title.as_deref().unwrap_or(&rule.title);
            let Some(text) = entry.text.as_deref() else {
                return Some(format!("- **{title}** {rest}"));
            };
            // A rule's text holds no " See ", so the last one starts the
            // pointer part; a rule under the shared home has none.
            let pointers = rest.rfind(" See ").map_or("", |at| &rest[at..]);
            Some(format!("- **{title}** {text}{pointers}"))
        }
    }
}
