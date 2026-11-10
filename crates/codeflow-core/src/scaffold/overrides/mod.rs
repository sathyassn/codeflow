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

/// Phrases that mark floor text. The contract test fails the build when an
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
                let reason = reason.map(|reason| shipped.name_in_kernel(reason));
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

/// Replace exact lines and section bodies of `text`; `None` when a unit's
/// shipped text is not where it must be (a row line not exactly once, a
/// section not found or its body not the recorded one).
#[must_use]
pub fn apply_units(text: &str, units: &[AppliedUnit]) -> Option<String> {
    let mut out = text.to_string();
    for unit in units {
        match Target::parse(&unit.entry.target)? {
            Target::Rule(_) | Target::Moment(_) => {
                let mut lines: Vec<&str> = out.split_inclusive('\n').collect();
                let positions: Vec<usize> = lines
                    .iter()
                    .enumerate()
                    .filter(|(_, line)| line.trim_end_matches(['\n', '\r']) == unit.replaced)
                    .map(|(index, _)| index)
                    .collect();
                let [position] = positions.as_slice() else {
                    return None;
                };
                let ending =
                    &lines[*position][lines[*position].trim_end_matches(['\n', '\r']).len()..];
                let replacement = format!("{}{ending}", unit.replacement);
                lines[*position] = &replacement;
                out = lines.concat();
            }
            Target::Section { id, .. } => {
                let mut doc = sections::parse(&out).ok()?;
                let section = doc.segments.iter_mut().find_map(|segment| match segment {
                    sections::Segment::Section(section) if section.id == id => Some(section),
                    _ => None,
                })?;
                if section.body != unit.replaced {
                    return None;
                }
                section.body.clone_from(&unit.replacement);
                out = doc.text();
            }
        }
    }
    Some(out)
}

/// Whether `effective` is `pristine` with exactly the targeted units
/// changed: every other line, segment, marker and heading byte-identical.
#[must_use]
pub fn only_units_changed(pristine: &str, effective: &str, units: &[AppliedUnit]) -> bool {
    let rows: Vec<&AppliedUnit> = units
        .iter()
        .filter(|unit| {
            matches!(
                Target::parse(&unit.entry.target),
                Some(Target::Rule(_) | Target::Moment(_))
            )
        })
        .collect();
    let section_ids: BTreeMap<String, &AppliedUnit> = units
        .iter()
        .filter_map(|unit| match Target::parse(&unit.entry.target) {
            Some(Target::Section { id, .. }) => Some((id, unit)),
            _ => None,
        })
        .collect();
    if section_ids.is_empty() {
        let before: Vec<&str> = pristine.split_inclusive('\n').collect();
        let after: Vec<&str> = effective.split_inclusive('\n').collect();
        if before.len() != after.len() {
            return false;
        }
        let mut changed = 0;
        for (old, new) in before.iter().zip(&after) {
            if old == new {
                continue;
            }
            let old_text = old.trim_end_matches(['\n', '\r']);
            let new_text = new.trim_end_matches(['\n', '\r']);
            if !rows
                .iter()
                .any(|unit| unit.replaced == old_text && unit.replacement == new_text)
            {
                return false;
            }
            changed += 1;
        }
        return changed == rows.iter().filter(|u| u.replaced != u.replacement).count();
    }
    if !rows.is_empty() {
        return false;
    }
    let (Ok(before), Ok(after)) = (sections::parse(pristine), sections::parse(effective)) else {
        return false;
    };
    if before.segments.len() != after.segments.len() {
        return false;
    }
    before
        .segments
        .iter()
        .zip(&after.segments)
        .all(|pair| match pair {
            (sections::Segment::Outside(a), sections::Segment::Outside(b)) => a == b,
            (sections::Segment::Section(a), sections::Segment::Section(b)) => {
                a.id == b.id
                    && a.class == b.class
                    && a.header == b.header
                    && a.level == b.level
                    && match section_ids.get(&a.id) {
                        Some(unit) => a.body == unit.replaced && b.body == unit.replacement,
                        None => a.body == b.body,
                    }
            }
            _ => false,
        })
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
    /// The shipped rendering with the units applied (the whole file).
    pub effective: String,
    /// The record update left last time, with its effective baseline
    /// rebuilt from the pristine baseline: the merge base.
    pub old: Option<(Record, String)>,
}

/// The override work of one update, by destination.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub dests: BTreeMap<String, DestPlan>,
}

/// Check the declared overrides and compute every affected file's effective
/// rendering, before update writes anything. `render` renders a manifest
/// entry as update would. Files with a record but no override now get a
/// plan too, so an override removed from the file restores the shipped text.
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
        if read_beneath_root(root, &record::record_path(&file.dest))
            .map_err(|_| record_unreadable(&file.dest))?
            .is_some()
        {
            recorded.insert(file.dest.clone());
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
        let effective = apply_units(&rendered, &applied)
            .filter(|effective| only_units_changed(&rendered, effective, &applied))
            .ok_or_else(|| {
                file_refused(shown!(
                    "applying the overrides to {} would change more than the targeted units",
                    dest_shown(&file.dest)
                ))
            })?;
        if file.ownership == Ownership::ManagedRegion {
            let format = file.region.unwrap_or(RegionFormat::Markdown);
            let both = region::extract_block(&rendered, format)
                .zip(region::extract_block(&effective, format));
            let rows_inside =
                both.is_some_and(|(before, after)| only_units_changed(&before, &after, &applied));
            if !rows_inside {
                return Err(file_refused(shown!(
                    "an overridden row of {} is not inside its managed block",
                    dest_shown(&file.dest)
                )));
            }
        }
        plan.dests.insert(
            file.dest.clone(),
            DestPlan {
                applied,
                effective,
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

impl Plan {
    /// Load each planned file's record and rebuild its effective baseline
    /// from the pristine baseline, refusing a record that does not match the
    /// installed state. `pending` maps a destination to the pristine hash a
    /// finished conflict resolution just accepted.
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
    ) -> Result<(), ScaffoldError> {
        for (dest, plan) in &mut self.dests {
            let Some(record) = record::Record::read(root, dest)? else {
                continue;
            };
            // The baseline is the pristine text: the whole file for a managed
            // file, the managed block for a managed-region file.
            let baseline = Baseline::read(root, dest)?.unwrap_or_default();
            let pristine = baseline.as_str();
            let pristine_hash = hash::sha256_hex(pristine.as_bytes());
            let installed_hash = installed.files.get(dest).map(|file| file.sha256.as_str());
            let rebuilt = apply_units(pristine, &record.applied)
                .filter(|_| record.pristine_sha256 == pristine_hash)
                .filter(|_| installed_hash == Some(pristine_hash.as_str()))
                .filter(|effective| {
                    hash::sha256_hex(effective.as_bytes()) == record.effective_sha256
                });
            let Some(effective) = rebuilt else {
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

/// Resolve every journal an interrupted update left, before update writes
/// anything else: roll forward a write that landed, drop one that did not,
/// and refuse any other state without writing. Every journal is classified
/// before the first one is acted on.
///
/// # Errors
///
/// A journal that does not parse or matches neither generation.
pub fn recover(
    root: &Path,
    manifest: &ScaffoldManifest,
    installed: &mut InstalledManifest,
) -> Result<usize, ScaffoldError> {
    let candidates = journal_candidates(manifest, installed);
    let mut actions = Vec::new();
    for dest in &candidates {
        let Some(journal) = record::Journal::read(root, dest)? else {
            continue;
        };
        let now = record::Journal::observe(
            root,
            dest,
            installed.files.get(dest).map(|file| file.sha256.as_str()),
        )?;
        match record::classify(&journal, &now) {
            record::Recovery::Refuse => return Err(record::refusal(dest)),
            recovery => actions.push((dest.clone(), journal, recovery)),
        }
    }
    let count = actions.len();
    for (dest, journal, recovery) in &actions {
        if *recovery == record::Recovery::RollForward {
            Baseline::write(root, dest, &journal.baseline_text)?;
            match &journal.record {
                Some(record) => record.write(root, dest)?,
                None => record::remove_record(root, dest)?,
            }
            if let Some(sha) = &journal.new.installed {
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
    }
    if count > 0 {
        installed.store(root)?;
        for (dest, _, _) in &actions {
            record::remove_journal(root, dest)?;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests;
