//! Project overrides of managed instructions (TSK-1302).
//!
//! A project replaces a managed block row or a declared skill section in
//! its own `.codeflow/overrides.toml`, which `codeflow update` never writes:
//!
//! ```toml
//! schema_version = 1
//!
//! [[override]]
//! target = "rule:outcome"          # a rule's `text` and/or `title`
//! text = "..."
//!
//! [[override]]
//! target = "moment:build"          # a moment row's `action`
//! action = "..."
//!
//! [[override]]
//! target = "cf-develop#the-loop"   # a declared skill section's body
//! body = """
//! ...
//! """
//! ```
//!
//! Names and ranges come only from the shipped text of the running binary:
//! the rule-map kernel for rows, the managed skill's `codeflow:section`
//! markers for sections. Every unit declares a class: only `overridable`
//! units can be targeted, a unit listed in `instruction-floors.toml` is
//! refused with its floor's name, and a `fixed` unit is refused with its
//! recorded reason. Any problem refuses the whole file before update writes
//! anything; nothing is applied in part.
//!
//! A row override replaces fields, so the renderer builds the line and its
//! pointers stay shipped. A section override replaces the body; the marker
//! and the heading stay shipped. After applying, the result is checked unit
//! by unit against the shipped text: only the targeted units may differ.

pub mod authority;
mod oracle;
pub mod record;
pub mod sections;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::assets::{read_shipped, AssetText};
use super::manifest::{ManifestEntry, Ownership, RegionFormat, ScaffoldManifest, Tier};
use super::rule_map::{File, Kernel, UnitClass, OUTPUTS};
use super::state::{read_beneath_root, Baseline, InstalledManifest, ScaffoldConfig};
use super::{hash, region, AssetSource, ScaffoldError, Shown};
use crate::shown;
pub use record::{AppliedUnit, Record};

/// The project's override file, repository relative.
pub const OVERRIDES_FILE: &str = ".codeflow/overrides.toml";

/// The floor list, in the binary's assets.
pub const FLOORS_ASSET: &str = "base/instruction-floors.toml";

/// The rule-map kernel, in the binary's assets.
pub const KERNEL_ASSET: &str = "base/rule-map.toml";

/// Phrases that mark floor text, including the review floors: review
/// independence, other-lineage review and the rule against headless peers
/// (TSK-1302 AC-3). The contract test fails the build when an
/// `overridable` unit's shipped text holds one. It is a regression check
/// that supplements each unit's declared class and its review: it catches
/// these phrases, not every paraphrase of a floor.
pub const FLOOR_PHRASES: &[&str] = &[
    "non-relaxable",
    "git floor",
    "[enforced]",
    "protected branch",
    "--no-verify",
    "override env",
    "headless",
    "self-review",
    "fresh-context",
    "other-lineage",
    "other lineage",
    "another lineage",
    "cross-lineage",
];

const OVERRIDES_SCHEMA: u32 = 1;
const FLOORS_SCHEMA: u32 = 1;

/// One declared override, as the project writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// `rule:<id>`, `moment:<key>` or `<skill>#<section-id>`.
    pub target: String,
    /// A rule's replacement title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A rule's replacement text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A moment row's replacement action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// A section's replacement body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverridesSource {
    schema_version: u32,
    #[serde(default, rename = "override")]
    overrides: Vec<Entry>,
}

/// A declared name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    /// `rule:<id>`.
    Rule(String),
    /// `moment:<key>`.
    Moment(String),
    /// `<skill>#<section-id>`.
    Section { skill: String, id: String },
}

impl Target {
    /// Parse a declared name; `None` when it has none of the three shapes.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        if let Some(id) = name.strip_prefix("rule:") {
            return sections::valid_id(id).then(|| Self::Rule(id.to_string()));
        }
        if let Some(key) = name.strip_prefix("moment:") {
            return sections::valid_id(key).then(|| Self::Moment(key.to_string()));
        }
        let (skill, id) = name.split_once('#')?;
        (sections::valid_id(skill) && sections::valid_id(id)).then(|| Self::Section {
            skill: skill.to_string(),
            id: id.to_string(),
        })
    }

    /// The canonical unit name.
    #[must_use]
    pub fn unit(&self) -> String {
        match self {
            Self::Rule(id) => format!("rule:{id}"),
            Self::Moment(key) => format!("moment:{key}"),
            Self::Section { skill, id } => format!("{skill}#{id}"),
        }
    }
}

/// One floor of `instruction-floors.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Floor {
    pub name: String,
    pub refusal: String,
    pub units: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FloorsSource {
    schema_version: u32,
    #[serde(rename = "floor")]
    floors: Vec<Floor>,
}

/// The parsed floor list.
#[derive(Debug, Clone)]
pub struct Floors {
    pub floors: Vec<Floor>,
}

impl Floors {
    /// Parse a floor list.
    ///
    /// # Errors
    ///
    /// The source is not a floor list of the known schema.
    pub fn parse(text: &str) -> Result<Self, String> {
        let source: FloorsSource = toml::from_str(text).map_err(|error| error.to_string())?;
        if source.schema_version != FLOORS_SCHEMA {
            return Err(format!(
                "unknown floor list schema {}",
                source.schema_version
            ));
        }
        Ok(Self {
            floors: source.floors,
        })
    }

    /// The floor that lists `unit`.
    #[must_use]
    pub fn floor_of(&self, unit: &str) -> Option<&Floor> {
        self.floors
            .iter()
            .find(|floor| floor.units.iter().any(|listed| listed == unit))
    }
}

/// The repository-relative `dest` as shown in a message. Every override
/// message names a file through here, so the display boundary scan lists
/// one site.
pub(crate) fn dest_shown(dest: &str) -> Shown {
    Shown::path(dest)
}

// The detail is built at each call site and used once; taking it by value
// keeps those sites plain.
#[allow(clippy::needless_pass_by_value)]
fn refused(index: usize, detail: Shown) -> ScaffoldError {
    ScaffoldError::invalid(
        shown!(
            "override {} in {}",
            Shown::count(index + 1),
            Shown::fixed(OVERRIDES_FILE)
        ),
        shown!(
            "{}; update changed nothing and no override was applied",
            detail
        ),
    )
}

#[allow(clippy::needless_pass_by_value)]
fn file_refused(detail: Shown) -> ScaffoldError {
    ScaffoldError::invalid(
        Shown::fixed(OVERRIDES_FILE),
        shown!(
            "{}; update changed nothing and no override was applied",
            detail
        ),
    )
}

/// The project's declared overrides, `None` without the file.
///
/// # Errors
///
/// The file cannot be read (a symlink, a directory, invalid UTF-8), does
/// not parse, or has another schema.
pub fn read_entries(root: &Path) -> Result<Option<Vec<Entry>>, ScaffoldError> {
    let text = match read_beneath_root(root, OVERRIDES_FILE) {
        Ok(Some(text)) => text,
        Ok(None) => return Ok(None),
        Err(_) => return Err(file_refused(Shown::fixed(
            "the override file cannot be read (it must be a regular UTF-8 file, never a symlink)",
        ))),
    };
    let source: OverridesSource = toml::from_str(&text)
        .map_err(|_| file_refused(Shown::fixed("the override file does not parse")))?;
    if source.schema_version != OVERRIDES_SCHEMA {
        return Err(file_refused(Shown::fixed(
            "the override file has an unknown schema_version",
        )));
    }
    Ok(Some(source.overrides))
}

/// The shipped text the override route resolves names against.
pub struct ShippedUnits {
    kernel_text: AssetText,
    pub kernel: Kernel,
    floors_text: AssetText,
    pub floors: Floors,
}

impl ShippedUnits {
    /// Read the kernel and the floor list from the binary's assets.
    ///
    /// # Errors
    ///
    /// Either is missing or does not parse: no override can be checked, so
    /// none is applied.
    pub fn load(source: &dyn AssetSource) -> Result<Self, ScaffoldError> {
        let missing = || {
            file_refused(Shown::fixed(
                "this build ships no rule-map kernel or floor list to check overrides against",
            ))
        };
        let kernel_text = read_shipped(source, KERNEL_ASSET)?.ok_or_else(missing)?;
        let floors_text = read_shipped(source, FLOORS_ASSET)?.ok_or_else(missing)?;
        let kernel = Kernel::parse(kernel_text.as_str()).map_err(|_| missing())?;
        let floors = Floors::parse(floors_text.as_str()).map_err(|_| missing())?;
        Ok(Self {
            kernel_text,
            kernel,
            floors_text,
            floors,
        })
    }

    fn name_in_kernel(&self, name: &str) -> Shown {
        Shown::shipped(name, self.kernel_text.shipped())
            .unwrap_or_else(|| Shown::fixed("(unnamed)"))
    }

    fn floor_refusal(&self, index: usize, floor: &Floor) -> ScaffoldError {
        let name = Shown::shipped(&floor.name, self.floors_text.shipped())
            .unwrap_or_else(|| Shown::fixed("a floor"));
        let why = Shown::shipped(&floor.refusal, self.floors_text.shipped())
            .unwrap_or_else(|| Shown::fixed("floors are never overridden"));
        refused(index, shown!("it targets a unit of {}: {}", name, why))
    }
}

/// The section markers' skill for a manifest `src`, when it is a managed
/// skill's `SKILL.md`.
#[must_use]
pub fn skill_of(src: &str) -> Option<&str> {
    let name = src
        .strip_prefix("agents/skills/")?
        .strip_suffix("/SKILL.md")?;
    (!name.contains('/')).then_some(name)
}

/// The tier whose rows a manifest `src` renders, when it is an `AGENTS.md`
/// template.
#[must_use]
pub fn agents_tier(src: &str) -> Option<Tier> {
    OUTPUTS
        .iter()
        .find(|output| output.asset == src && output.file == File::Agents)
        .map(|output| output.tier)
}

/// What a value of a row field may hold: one non-empty line, no marker text.
fn row_value_ok(value: &str, table_cell: bool) -> bool {
    let refused = value.trim().is_empty()
        || value.contains(['\n', '\r'])
        || value.contains("<!--")
        || value.contains("-->")
        || value.to_ascii_lowercase().contains("codeflow:")
        || (table_cell && value.contains('|'));
    !refused
}

/// A checked override: its index, entry and target.
#[derive(Debug, Clone)]
pub struct Checked {
    pub index: usize,
    pub entry: Entry,
    pub target: Target,
}

/// The managed files of the project's tier and preset that update manages
/// and the override route reads: `AGENTS.md` templates and skill files.
fn candidates<'a>(
    manifest: &'a ScaffoldManifest,
    tier: Tier,
    preset: &str,
    ignore: &ScaffoldConfig,
) -> Vec<&'a ManifestEntry> {
    manifest
        .entries
        .iter()
        .filter(|entry| entry.applies(tier, preset) && !ignore.is_ignored(&entry.dest))
        .filter(|entry| {
            (agents_tier(&entry.src).is_some() && entry.ownership == Ownership::ManagedRegion)
                || (skill_of(&entry.src).is_some() && entry.ownership == Ownership::Managed)
        })
        .collect()
}

/// Check every declared override against the shipped text. Any problem
/// refuses the whole file; the first refusal is returned.
///
/// # Errors
///
/// The refusal, naming the override by its position and, when the name is
/// shipped, by its unit.
#[allow(clippy::too_many_lines)] // one refusal per rule, in the order they apply
pub fn check(
    source: &dyn AssetSource,
    shipped: &ShippedUnits,
    entries: &[Entry],
    manifest: &ScaffoldManifest,
    tier: Tier,
    preset: &str,
    ignore: &ScaffoldConfig,
) -> Result<Vec<Checked>, ScaffoldError> {
    let files = candidates(manifest, tier, preset, ignore);
    let mut seen = BTreeSet::new();
    let mut checked = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(target) = Target::parse(&entry.target) else {
            return Err(refused(
                index,
                Shown::fixed(
                    "its target is not a declared name (rule:<id>, moment:<key> or <skill>#<section-id>)",
                ),
            ));
        };
        let unit = target.unit();
        if !seen.insert(unit.clone()) {
            return Err(refused(
                index,
                Shown::fixed("an earlier override already targets the same unit"),
            ));
        }
        if let Some(floor) = shipped.floors.floor_of(&unit) {
            return Err(shipped.floor_refusal(index, floor));
        }
        let unknown = || {
            refused(
                index,
                Shown::fixed(
                    "it names a unit this build does not declare for this project's tier and managed files \
                     (removed, renamed or never shipped); retarget or remove it",
                ),
            )
        };
        let (class, reason): (Option<UnitClass>, Option<Shown>) = match &target {
            Target::Rule(id) | Target::Moment(id) => {
                let is_rule = matches!(target, Target::Rule(_));
                if !files
                    .iter()
                    .any(|file| agents_tier(&file.src) == Some(tier))
                {
                    return Err(unknown());
                }
                let found: Vec<(Option<UnitClass>, Option<&String>)> = if is_rule {
                    shipped
                        .kernel
                        .rules_for(tier)
                        .into_iter()
                        .filter(|rule| &rule.id == id)
                        .map(|rule| (rule.class, rule.fixed_reason.as_ref()))
                        .collect()
                } else {
                    shipped
                        .kernel
                        .moments_for(tier)
                        .into_iter()
                        .filter(|moment| &moment.key == id)
                        .map(|moment| (moment.class, moment.fixed_reason.as_ref()))
                        .collect()
                };
                let [(class, reason)] = found.as_slice() else {
                    return Err(if found.is_empty() {
                        unknown()
                    } else {
                        refused(
                            index,
                            Shown::fixed(
                                "the shipped kernel declares this name twice for this tier",
                            ),
                        )
                    });
                };
                let fields_ok = if is_rule {
                    entry.action.is_none()
                        && entry.body.is_none()
                        && (entry.text.is_some() || entry.title.is_some())
                        && entry.text.as_deref().is_none_or(|v| row_value_ok(v, false))
                        && entry
                            .title
                            .as_deref()
                            .is_none_or(|v| row_value_ok(v, false))
                } else {
                    entry.title.is_none()
                        && entry.text.is_none()
                        && entry.body.is_none()
                        && entry
                            .action
                            .as_deref()
                            .is_some_and(|v| row_value_ok(v, true))
                };
                let reason =
                    reason.map(|reason| shipped.name_in_kernel(reason.trim_end_matches('.')));
                if fields_ok || !matches!(class, Some(UnitClass::Overridable)) {
                    (*class, reason)
                } else {
                    return Err(refused(
                        index,
                        if is_rule {
                            Shown::fixed(
                                "a rule override sets only `text` and `title`, each one line with no `<!--`, `-->` or `codeflow:`",
                            )
                        } else {
                            Shown::fixed(
                                "a moment override sets only `action`, one line with no `|`, `<!--`, `-->` or `codeflow:`",
                            )
                        },
                    ));
                }
            }
            Target::Section { skill, id } => {
                let Some(file) = files.iter().find(|file| skill_of(&file.src) == Some(skill))
                else {
                    return Err(unknown());
                };
                let text =
                    read_shipped(source, &format!("base/{}", file.src))?.ok_or_else(unknown)?;
                let doc = sections::parse(text.as_str()).map_err(|error| {
                    refused(
                        index,
                        shown!(
                            "the shipped skill's section markers do not parse (line {}: {})",
                            Shown::count(error.line()),
                            Shown::fixed(error.what())
                        ),
                    )
                })?;
                let Some(section) = doc.section(id) else {
                    return Err(unknown());
                };
                if matches!(section.class, Some(UnitClass::Overridable)) {
                    let fields_ok = entry.title.is_none()
                        && entry.text.is_none()
                        && entry.action.is_none()
                        && entry.body.is_some();
                    if !fields_ok {
                        return Err(refused(
                            index,
                            Shown::fixed("a section override sets only `body`"),
                        ));
                    }
                    let body = entry.body.as_deref().unwrap_or_default();
                    if let Err(problem) = sections::check_body(body, section.level) {
                        return Err(refused(index, Shown::fixed(problem.what())));
                    }
                }
                (section.class, None)
            }
        };
        match class {
            Some(UnitClass::Overridable) => {}
            Some(UnitClass::Floor) => {
                return Err(refused(
                    index,
                    Shown::fixed(
                        "it targets a unit that states a floor, which is never overridden",
                    ),
                ))
            }
            Some(UnitClass::Fixed) => {
                return Err(refused(
                    index,
                    shown!(
                        "it targets a fixed unit, which is not a text-override target: {}",
                        reason
                            .unwrap_or_else(|| Shown::fixed("its policy is configured elsewhere"))
                    ),
                ))
            }
            None => {
                return Err(refused(
                    index,
                    Shown::fixed(
                        "it targets a unit that declares no class, which is never overridden",
                    ),
                ))
            }
        }
        checked.push(Checked {
            index,
            entry: entry.clone(),
            target,
        });
    }
    Ok(checked)
}

/// Where each unit sits in `text`: the byte range its shipped text
/// occupies, in order of position. Rows are found as the one line whose text
/// (without its line break) is the shipped row; sections by the shipped
/// text's own markers, parsed once. `None` when a unit's shipped text is not
/// where it must be (a row line not exactly once, a section not found or its
/// body not the recorded one) or two units overlap.
fn unit_ranges<'a>(
    text: &str,
    units: &'a [AppliedUnit],
) -> Option<Vec<(std::ops::Range<usize>, &'a AppliedUnit)>> {
    let mut doc = None;
    let mut ranges = Vec::new();
    for unit in units {
        let range = match Target::parse(&unit.entry.target)? {
            Target::Rule(_) | Target::Moment(_) => {
                let mut found = None;
                let mut offset = 0;
                for line in text.split_inclusive('\n') {
                    let content = line.strip_suffix('\n').unwrap_or(line);
                    let content = content.strip_suffix('\r').unwrap_or(content);
                    if content == unit.replaced {
                        if found.is_some() {
                            return None;
                        }
                        found = Some(offset..offset + content.len());
                    }
                    offset += line.len();
                }
                found?
            }
            Target::Section { id, .. } => {
                if doc.is_none() {
                    doc = Some(sections::parse(text).ok()?);
                }
                let range = doc.as_ref()?.body_range(&id)?;
                if text.get(range.clone())? != unit.replaced {
                    return None;
                }
                range
            }
        };
        ranges.push((range, unit));
    }
    ranges.sort_by_key(|(range, _)| range.start);
    ranges
        .windows(2)
        .all(|pair| pair[0].0.end <= pair[1].0.start)
        .then_some(ranges)
}

/// Replace exact lines and section bodies of `text`, every unit located in
/// `text` itself before any replacement, so a replacement never moves where
/// another unit is found. `None` when a unit is not found exactly once, or
/// two units overlap.
#[must_use]
pub fn apply_units(text: &str, units: &[AppliedUnit]) -> Option<String> {
    let ranges = unit_ranges(text, units)?;
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (range, unit) in ranges {
        out.push_str(&text[at..range.start]);
        out.push_str(&unit.replacement);
        at = range.end;
    }
    out.push_str(&text[at..]);
    Some(out)
}

/// Whether `effective` is `pristine` with exactly the targeted units
/// replaced. The units are located in the shipped `pristine` text, and every
/// byte of `effective` outside them is compared directly with `pristine`;
/// `effective` is never parsed, so a replacement that would confuse a parser
/// cannot hide a change.
#[must_use]
pub fn only_units_changed(pristine: &str, effective: &str, units: &[AppliedUnit]) -> bool {
    let Some(ranges) = unit_ranges(pristine, units) else {
        return false;
    };
    let mut at = 0;
    let mut rest = effective;
    for (range, unit) in ranges {
        let Some(after) = rest
            .strip_prefix(&pristine[at..range.start])
            .and_then(|after| after.strip_prefix(unit.replacement.as_str()))
        else {
            return false;
        };
        rest = after;
        at = range.end;
    }
    rest == &pristine[at..]
}

/// Whether the declared sections of `effective` are the shipped ones, in
/// order, with the same markers and headings: a structural check on top of
/// [`only_units_changed`], with the same parser the shipped text is read by.
fn same_sections(pristine: &str, effective: &str) -> bool {
    let (Ok(before), Ok(after)) = (sections::parse(pristine), sections::parse(effective)) else {
        return false;
    };
    let shape = |doc: &sections::Doc| -> Vec<(String, Option<UnitClass>, String, usize)> {
        doc.sections()
            .map(|section| {
                (
                    section.id.clone(),
                    section.class,
                    section.header.clone(),
                    section.level,
                )
            })
            .collect()
    };
    shape(&before) == shape(&after)
}

/// The applied units of `checked` for one rendered managed file.
fn units_for(
    shipped: &ShippedUnits,
    checked: &[Checked],
    entry: &ManifestEntry,
    rendered: &str,
) -> Result<Vec<AppliedUnit>, ScaffoldError> {
    let mut units = Vec::new();
    for item in checked {
        let unit = match &item.target {
            Target::Rule(id) => {
                let Some(tier) = agents_tier(&entry.src) else {
                    continue;
                };
                let rule = shipped
                    .kernel
                    .rules_for(tier)
                    .into_iter()
                    .find(|rule| &rule.id == id)
                    .ok_or_else(|| {
                        refused(item.index, Shown::fixed("the rule is not in this tier"))
                    })?;
                let mut replaced = rule.clone();
                if let Some(title) = &item.entry.title {
                    replaced.title.clone_from(title);
                }
                if let Some(text) = &item.entry.text {
                    replaced.text.clone_from(text);
                }
                AppliedUnit {
                    entry: item.entry.clone(),
                    replaced: shipped.kernel.render_rule_at(tier, rule),
                    replacement: shipped.kernel.render_rule_at(tier, &replaced),
                }
            }
            Target::Moment(key) => {
                let Some(tier) = agents_tier(&entry.src) else {
                    continue;
                };
                let moment = shipped
                    .kernel
                    .moments_for(tier)
                    .into_iter()
                    .find(|moment| &moment.key == key)
                    .ok_or_else(|| {
                        refused(item.index, Shown::fixed("the row is not in this tier"))
                    })?;
                let mut replaced = moment.clone();
                if let Some(action) = &item.entry.action {
                    replaced.action.clone_from(action);
                }
                AppliedUnit {
                    entry: item.entry.clone(),
                    replaced: Kernel::render_moment(moment),
                    replacement: Kernel::render_moment(&replaced),
                }
            }
            Target::Section { skill, id } => {
                if skill_of(&entry.src) != Some(skill.as_str()) {
                    continue;
                }
                let doc = sections::parse(rendered).map_err(|_| {
                    refused(
                        item.index,
                        Shown::fixed("the installed skill's markers do not parse"),
                    )
                })?;
                let section = doc.section(id).ok_or_else(|| {
                    refused(
                        item.index,
                        Shown::fixed("the section is not in the rendered skill"),
                    )
                })?;
                AppliedUnit {
                    entry: item.entry.clone(),
                    replaced: section.body.clone(),
                    replacement: sections::normalized_body(
                        item.entry.body.as_deref().unwrap_or_default(),
                    ),
                }
            }
        };
        units.push(unit);
    }
    Ok(units)
}

/// The override work for one managed file.
#[derive(Debug, Clone)]
pub struct DestPlan {
    /// The units to apply now, in declaration order.
    pub applied: Vec<AppliedUnit>,
    /// The shipped text the pristine baseline holds: the whole rendering of
    /// a managed file, the managed block of a managed-region file.
    pub pristine: String,
    /// The shipped rendering with the units applied (the whole file).
    pub effective: String,
    /// The block format of a managed-region file; `None` for a managed file.
    pub region: Option<RegionFormat>,
    /// What the binary's shipped text says each unit of this file is: what
    /// a persisted record of the file is checked against.
    pub authority: authority::Authority,
    /// The record update left last time, with its effective baseline
    /// rebuilt from the pristine baseline: the merge base.
    pub old: Option<(Record, String)>,
}

impl DestPlan {
    /// The part of `text` an override record hashes: the whole file, or the
    /// managed block of a managed-region file.
    #[must_use]
    pub fn unit_of(&self, text: &str) -> Option<String> {
        match self.region {
            None => Some(text.to_string()),
            Some(format) => region::extract_block(text, format),
        }
    }

    /// The effective text the record of this plan hashes.
    #[must_use]
    pub fn effective_unit(&self) -> Option<String> {
        self.unit_of(&self.effective)
    }
}

/// The override work of one update, by destination.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub dests: BTreeMap<String, DestPlan>,
}

/// Metadata an update is about to write, which the checks before the first
/// write read in place of what is on disk: pristine baselines and override
/// records (`None` removes one), by destination.
#[derive(Debug, Clone, Default)]
pub struct Projected {
    pub baselines: BTreeMap<String, String>,
    pub records: BTreeMap<String, Option<Record>>,
}

/// Check the declared overrides and compute every affected file's effective
/// rendering, before update writes anything. `render` renders a manifest
/// entry as update would. Files with a record or a journal but no override
/// now get a plan too, so an override removed from the file restores the
/// shipped text and an interrupted write can be checked against it.
///
/// # Errors
///
/// Any refusal of [`check`], or a result that changes more than the
/// targeted units.
pub fn prepare(
    source: &dyn AssetSource,
    root: &Path,
    manifest: &ScaffoldManifest,
    tier: Tier,
    preset: &str,
    ignore: &ScaffoldConfig,
    render: &mut dyn FnMut(&ManifestEntry) -> Result<Option<String>, ScaffoldError>,
) -> Result<Plan, ScaffoldError> {
    let entries = read_entries(root)?.unwrap_or_default();
    let files = candidates(manifest, tier, preset, ignore);
    let mut recorded = BTreeSet::new();
    for file in &files {
        for path in [
            record::record_path(&file.dest),
            record::journal_path(&file.dest),
        ] {
            if read_beneath_root(root, &path)
                .map_err(|_| record_unreadable(&file.dest))?
                .is_some()
            {
                recorded.insert(file.dest.clone());
            }
        }
    }
    if entries.is_empty() && recorded.is_empty() {
        return Ok(Plan::default());
    }
    let shipped = ShippedUnits::load(source)?;
    let checked = check(source, &shipped, &entries, manifest, tier, preset, ignore)?;
    let mut plan = Plan::default();
    for file in files {
        let targeted = checked.iter().any(|item| match &item.target {
            Target::Rule(_) | Target::Moment(_) => agents_tier(&file.src).is_some(),
            Target::Section { skill, .. } => skill_of(&file.src) == Some(skill.as_str()),
        });
        if !targeted && !recorded.contains(&file.dest) {
            continue;
        }
        let Some(rendered) = render(file)? else {
            continue;
        };
        let applied = units_for(&shipped, &checked, file, &rendered)?;
        if applied.is_empty() && !recorded.contains(&file.dest) {
            continue;
        }
        let more_than_targeted = || {
            file_refused(shown!(
                "applying the overrides to {} would change more than the targeted units",
                dest_shown(&file.dest)
            ))
        };
        let effective = apply_units(&rendered, &applied)
            .filter(|effective| only_units_changed(&rendered, effective, &applied))
            .filter(|effective| same_sections(&rendered, effective))
            .filter(|effective| oracle::same_structure(&rendered, effective, &applied))
            .ok_or_else(more_than_targeted)?;
        let authority = match (agents_tier(&file.src), skill_of(&file.src)) {
            (Some(tier), _) => authority::Authority::for_agents(&shipped.kernel, tier),
            (None, Some(skill)) => authority::Authority::for_skill(skill, &rendered),
            (None, None) => authority::Authority::default(),
        };
        let (pristine, region) = if file.ownership == Ownership::ManagedRegion {
            let format = file.region.unwrap_or(RegionFormat::Markdown);
            let both = region::extract_block(&rendered, format)
                .zip(region::extract_block(&effective, format))
                .filter(|(before, after)| only_units_changed(before, after, &applied));
            let Some((pristine, _)) = both else {
                return Err(file_refused(shown!(
                    "an overridden row of {} is not inside its managed block",
                    dest_shown(&file.dest)
                )));
            };
            (pristine, Some(format))
        } else {
            (rendered.clone(), None)
        };
        plan.dests.insert(
            file.dest.clone(),
            DestPlan {
                applied,
                pristine,
                effective,
                region,
                authority,
                old: None,
            },
        );
    }
    Ok(plan)
}

fn record_unreadable(dest: &str) -> ScaffoldError {
    ScaffoldError::invalid(
        shown!("override state of {}", dest_shown(dest)),
        Shown::fixed("the override record cannot be read; update changed nothing"),
    )
}

/// Rebuild the effective baseline a record claims: the pristine baseline
/// with the record's applied set and nothing else. `None` unless the
/// baseline hashes to the record's pristine hash and to `installed`, the
/// installed manifest's hash; its generation can still advance; every unit
/// is one `authority`, the binary's shipped text for the file, lets a
/// project override, at the place the binary finds that unit in the
/// baseline, with a replacement that changes only the unit's fields;
/// the result changes only those units and reads as the same Markdown blocks
/// outside them; and it hashes to the record's effective hash. A record
/// that is consistent with itself but names a unit it may not, or a range
/// that is not that unit's, is refused.
#[must_use]
pub fn rebuild(
    authority: &authority::Authority,
    record: &Record,
    baseline: &str,
    installed: Option<&str>,
) -> Option<String> {
    let pristine_hash = hash::sha256_hex(baseline.as_bytes());
    if record.pristine_sha256 != pristine_hash || installed != Some(pristine_hash.as_str()) {
        return None;
    }
    // No digest covers the generation; a record whose next generation does
    // not exist cannot be advanced by the write that replaces it.
    record.generation.checked_add(1)?;
    if !authority.allows(baseline, &record.applied) {
        return None;
    }
    apply_units(baseline, &record.applied)
        .filter(|effective| only_units_changed(baseline, effective, &record.applied))
        .filter(|effective| oracle::same_structure(baseline, effective, &record.applied))
        .filter(|effective| hash::sha256_hex(effective.as_bytes()) == record.effective_sha256)
}

impl Plan {
    /// Load each planned file's record and rebuild its effective baseline
    /// from the pristine baseline, refusing a record that does not match the
    /// installed state. `installed` and `projected` are the metadata as the
    /// update is about to leave it: what is on disk, with the writes this
    /// update has decided but not yet made.
    ///
    /// # Errors
    ///
    /// A record that does not parse, names a pristine text other than the
    /// installed one, or whose applied set does not rebuild its effective
    /// hash: unverifiable, refused.
    pub fn load_old(
        &mut self,
        root: &Path,
        installed: &InstalledManifest,
        projected: &Projected,
    ) -> Result<(), ScaffoldError> {
        for (dest, plan) in &mut self.dests {
            let record = match projected.records.get(dest) {
                Some(record) => record.clone(),
                None => record::Record::read(root, dest)?,
            };
            let Some(record) = record else {
                continue;
            };
            // The baseline is the pristine text: the whole file for a managed
            // file, the managed block for a managed-region file.
            let baseline = match projected.baselines.get(dest) {
                Some(text) => text.clone(),
                None => Baseline::read(root, dest)?.unwrap_or_default(),
            };
            let installed_hash = installed.files.get(dest).map(|file| file.sha256.as_str());
            let Some(effective) = rebuild(&plan.authority, &record, &baseline, installed_hash)
            else {
                return Err(ScaffoldError::invalid(
                    shown!("override state of {}", dest_shown(dest)),
                    Shown::fixed(
                        "the override record does not match the installed baseline and manifest; update changed nothing",
                    ),
                ));
            };
            plan.old = Some((record, effective));
        }
        Ok(())
    }
}

/// Every destination a journal may exist for: the installed and the shipped
/// managed files.
#[must_use]
pub fn journal_candidates(
    manifest: &ScaffoldManifest,
    installed: &InstalledManifest,
) -> BTreeSet<String> {
    installed
        .files
        .keys()
        .cloned()
        .chain(manifest.entries.iter().map(|entry| entry.dest.clone()))
        .collect()
}

/// Whether a journal's payload is exactly what this update writes for
/// `dest`: the shipped pristine text this binary renders, and a record whose
/// applied set is the declared overrides as this binary checks them (so it
/// covers only authorized, overridable units), which rebuilds from that
/// pristine text to the effective text and names the bytes the journal says
/// were written.
fn payload_is_this_update(journal: &record::Journal, plan: Option<&DestPlan>) -> bool {
    let Some(plan) = plan else {
        return false;
    };
    let pristine_hash = hash::sha256_hex(journal.baseline_text.as_bytes());
    if journal.baseline_text != plan.pristine
        || journal.new.installed.as_deref() != Some(pristine_hash.as_str())
    {
        return false;
    }
    match &journal.record {
        None => plan.applied.is_empty(),
        Some(record) => {
            record.applied == plan.applied
                && Some(&record.written_sha256) == journal.new.file.as_ref()
                && rebuild(
                    &plan.authority,
                    record,
                    &journal.baseline_text,
                    Some(&pristine_hash),
                )
                .is_some_and(|effective| plan.effective_unit().as_ref() == Some(&effective))
        }
    }
}

/// What the journals an interrupted update left resolve to, decided before
/// anything is written: each journal and what to do with it.
#[derive(Debug, Clone, Default)]
pub struct Recovery {
    actions: Vec<(String, record::Journal, record::Recovery)>,
}

impl Recovery {
    /// How many journals this recovery resolves.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Whether there is no journal to resolve.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    fn rolled_forward(&self) -> impl Iterator<Item = (&String, &record::Journal)> {
        self.actions
            .iter()
            .filter(|(_, _, recovery)| *recovery == record::Recovery::RollForward)
            .map(|(dest, journal, _)| (dest, journal))
    }

    /// Apply the metadata a roll forward writes to `installed` and
    /// `projected`, in memory only, so every check before the first write
    /// reads the state this update leaves.
    pub fn project(
        &self,
        manifest: &ScaffoldManifest,
        installed: &mut InstalledManifest,
        projected: &mut Projected,
    ) {
        for (dest, journal) in self.rolled_forward() {
            projected
                .baselines
                .insert(dest.clone(), journal.baseline_text.clone());
            projected
                .records
                .insert(dest.clone(), journal.record.clone());
            let Some(sha) = &journal.new.installed else {
                continue;
            };
            if let Some(file) = installed.files.get_mut(dest) {
                file.sha256.clone_from(sha);
            } else if let Some(entry) = manifest.entries.iter().find(|e| &e.dest == dest) {
                installed.files.insert(
                    dest.clone(),
                    super::state::InstalledFile {
                        src: entry.src.clone(),
                        ownership: entry.ownership,
                        sha256: sha.clone(),
                        exec: entry.exec,
                    },
                );
            }
        }
    }

    /// Write the baselines and records of every roll forward. The installed
    /// manifest is stored by the caller, then [`Recovery::finish`] removes
    /// the journals.
    ///
    /// # Errors
    ///
    /// IO failures, or a symlinked path.
    pub fn commit(&self, root: &Path) -> Result<(), ScaffoldError> {
        for (dest, journal) in self.rolled_forward() {
            Baseline::write(root, dest, &journal.baseline_text)?;
            match &journal.record {
                Some(record) => record.write(root, dest)?,
                None => record::remove_record(root, dest)?,
            }
        }
        Ok(())
    }

    /// Remove every resolved journal, once its metadata is stored.
    ///
    /// # Errors
    ///
    /// IO failures, or a symlinked journal path.
    pub fn finish(&self, root: &Path) -> Result<(), ScaffoldError> {
        for (dest, _, _) in &self.actions {
            record::remove_journal(root, dest)?;
        }
        Ok(())
    }
}

/// Decide what every journal an interrupted update left resolves to, writing
/// nothing: roll forward a write that landed and whose payload is exactly
/// what this update writes, drop one that never landed, and refuse any other
/// state. Every journal is decided before any is acted on, and the caller
/// checks the whole projected state before it commits.
///
/// # Errors
///
/// A journal that does not parse, whose payload does not match its digests
/// or this update, or that matches neither generation.
pub fn plan_recovery(
    root: &Path,
    manifest: &ScaffoldManifest,
    installed: &InstalledManifest,
    plan: &Plan,
) -> Result<Recovery, ScaffoldError> {
    let mut recovery = Recovery::default();
    for dest in journal_candidates(manifest, installed) {
        let Some(journal) = record::Journal::read(root, &dest)? else {
            continue;
        };
        let now = record::Journal::observe(
            root,
            &dest,
            installed.files.get(&dest).map(|file| file.sha256.as_str()),
        )?;
        let action = record::classify(&journal, &now);
        match action {
            record::Recovery::Refuse => return Err(record::refusal(&dest)),
            record::Recovery::RollForward
                if !payload_is_this_update(&journal, plan.dests.get(&dest)) =>
            {
                return Err(record::payload_refusal(&dest));
            }
            _ => recovery.actions.push((dest, journal, action)),
        }
    }
    Ok(recovery)
}

#[cfg(test)]
mod tests;
