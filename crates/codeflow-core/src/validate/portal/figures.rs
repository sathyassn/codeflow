//! Figure and page-class evidence (TSK-059).
//!
//! The adapter binds grammar figures to pages from `portal.config.json`,
//! derives every figure fact from a committed file, and renders illustrated
//! and pass-through sources unchanged with companion figures inserted. This
//! module re-derives each of those claims from the commit: the page class of
//! every route, the declaration bytes behind every bound figure, every fact
//! and derived value, and, for a source rendered as it is, that the rendered
//! region with its recorded insertions removed equals the committed body.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Deserializer};

use super::{
    deserialize_bounded_sequence, git_batch_blobs, markdown_body, normalize_markdown_source,
    safe_path_text, strict_url_route, valid_sha256, Evidence, Page, PortalValidationReport,
};
use crate::capability::parse_capabilities;
use crate::scaffold::sha256_hex;
use crate::strict_json::parse_strict_json;

mod dom;
mod render;

pub(super) use dom::CodeBlockAssets;

pub(super) const PAGE_CLASSES: [&str; 3] = ["illustrated", "pass-through", "derived-lookup"];
pub(super) const PAGE_CLASS_REASONS: [&str; 3] =
    ["accepted-record", "governance", "no-relationship"];
pub(super) const DERIVED_LOOKUPS: [&str; 1] = ["capability-registry"];
const ALTITUDE_PANELS: [&str; 3] = ["concept", "architecture", "technical"];
const EXPLANATORY: &str = "explanatory";
const CAPABILITY_REGISTRY: &str = "docs/capabilities.md";
const MAX_PAGE_CLASSES: usize = 256;
const MAX_FIGURE_BINDINGS: usize = 512;
const MAX_FACTS: usize = 32;
const MAX_DECLARATION_BYTES: u64 = 256 * 1024;
const MAX_TOTAL_DECLARATION_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FACT_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_FACT_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const DRAWN_VALUE_TOLERANCE: f64 = 1e-3;
const SOURCE_END_MARKER: &str = "\n<!-- codeflow-source-end -->\n";
const COMPANION_END: &str = "\n\n<!-- codeflow-companion-end -->\n";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FigureEvidence {
    declaration_path: String,
    declaration_sha256: String,
    grammar_version: u32,
    figure_id: String,
    family: String,
    binding: String,
    #[serde(deserialize_with = "deserialize_routes")]
    routes: Vec<String>,
    #[serde(deserialize_with = "deserialize_facts")]
    facts: Vec<FactEvidence>,
    derived: Option<DerivedEvidence>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FactEvidence {
    claim: String,
    source: String,
    source_sha256: String,
    check: serde_json::Value,
    drawn: serde_json::Value,
    derived: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DerivedEvidence {
    source_path: String,
    source_sha256: String,
    select: String,
    values: serde_json::Map<String, serde_json::Value>,
    drawn: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageFigure {
    declaration_path: String,
    declaration_sha256: String,
    figure_id: String,
    placement: String,
    panel: Option<String>,
    anchor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceRegion {
    source_start_bytes: usize,
    output_offset_bytes: usize,
    region_bytes: usize,
    region_sha256: String,
    #[serde(deserialize_with = "deserialize_inserts")]
    inserts: Vec<Insert>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Insert {
    offset_bytes: usize,
    block_bytes: usize,
    block_sha256: String,
    declaration_path: String,
    placement: String,
    anchor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Lookup {
    derive: String,
    rows: usize,
}

pub(super) fn deserialize_figures<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<FigureEvidence>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_sequence(deserializer, MAX_FIGURE_BINDINGS, "portal figures").map(Some)
}

pub(super) fn deserialize_page_figures<'de, D>(deserializer: D) -> Result<Vec<PageFigure>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_sequence(deserializer, MAX_FIGURE_BINDINGS, "page figures")
}

fn deserialize_routes<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_sequence(deserializer, MAX_FIGURE_BINDINGS, "figure routes")
}

fn deserialize_facts<'de, D>(deserializer: D) -> Result<Vec<FactEvidence>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_sequence(deserializer, MAX_FACTS, "figure facts")
}

fn deserialize_inserts<'de, D>(deserializer: D) -> Result<Vec<Insert>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_sequence(deserializer, MAX_FIGURE_BINDINGS, "source region inserts")
}

/// One `page_classes` entry: a source or a prefix, its class, and the reason a
/// pass-through carries.
#[derive(Clone, Debug)]
pub(super) struct PageClassEntry {
    source: Option<String>,
    prefix: Option<String>,
    class: String,
    reason: Option<String>,
    note: Option<String>,
    derive: Option<String>,
}

impl PageClassEntry {
    /// Only a pass-through carries a reason (and, for no-relationship, the
    /// note that records the judgment); only a derived lookup names what
    /// derives it, and only for one source.
    fn allowed(&self) -> bool {
        let bounded_note = self
            .note
            .as_ref()
            .is_none_or(|note| !note.trim().is_empty() && note.encode_utf16().count() <= 300);
        match self.class.as_str() {
            "pass-through" => {
                self.reason
                    .as_deref()
                    .is_some_and(|reason| PAGE_CLASS_REASONS.contains(&reason))
                    && (self.reason.as_deref() != Some("no-relationship") || self.note.is_some())
                    && bounded_note
                    && self.derive.is_none()
            }
            "derived-lookup" => {
                self.reason.is_none()
                    && self.note.is_none()
                    && self.source.is_some()
                    && self
                        .derive
                        .as_deref()
                        .is_some_and(|derive| DERIVED_LOOKUPS.contains(&derive))
            }
            _ => self.reason.is_none() && self.note.is_none() && self.derive.is_none(),
        }
    }
}

/// One `figures` binding: a declaration, a route, and a panel or an anchor.
#[derive(Clone, Debug)]
pub(super) struct FigureBinding {
    declaration: String,
    route: String,
    panel: Option<String>,
    anchor: Option<String>,
}

impl FigureBinding {
    fn placement(&self) -> &'static str {
        if self.panel.is_some() {
            "panel"
        } else if self.anchor.is_some() {
            "anchor"
        } else {
            "head"
        }
    }
}

/// Refuse a `page_classes` table that is not the closed shape the adapter
/// accepts, so an exemption can never reach the gate through a looser reader.
pub(super) fn configured_page_classes(
    value: Option<&serde_json::Value>,
    report: &mut PortalValidationReport,
) -> Option<Vec<PageClassEntry>> {
    let Some(value) = value else {
        return Some(Vec::new());
    };
    let Some(entries) = value
        .as_array()
        .filter(|entries| entries.len() <= MAX_PAGE_CLASSES)
    else {
        report.issues.push(format!(
            "portal configuration page_classes is not an array of at most {MAX_PAGE_CLASSES} entries"
        ));
        return None;
    };
    let mut seen = BTreeSet::new();
    let mut parsed = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(object) = entry.as_object() else {
            report
                .issues
                .push("portal configuration page_classes entry is not an object".into());
            return None;
        };
        if object.keys().any(|key| {
            !["source", "prefix", "class", "reason", "note", "derive"].contains(&key.as_str())
        }) {
            report
                .issues
                .push("portal configuration page_classes keys are not closed".into());
            return None;
        }
        let text = |key: &str| {
            object
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        let source = text("source");
        let prefix = text("prefix");
        let named = match (&source, &prefix) {
            (Some(path), None) | (None, Some(path)) if safe_path_text(path) => path.clone(),
            _ => {
                report.issues.push(
                    "portal configuration page_classes entry names neither exactly one source nor one prefix".into(),
                );
                return None;
            }
        };
        if (object.contains_key("source") && source.is_none())
            || (object.contains_key("prefix") && prefix.is_none())
            || !seen.insert(super::portable_key(&named))
        {
            report.issues.push(format!(
                "portal configuration page_classes declares {named} more than once or malformed"
            ));
            return None;
        }
        let Some(class) = text("class").filter(|class| PAGE_CLASSES.contains(&class.as_str()))
        else {
            report.issues.push(format!(
                "portal configuration page_classes {named} class is not one of {}",
                PAGE_CLASSES.join(", ")
            ));
            return None;
        };
        let entry = PageClassEntry {
            source,
            prefix,
            class,
            reason: text("reason"),
            note: text("note"),
            derive: text("derive"),
        };
        if !entry.allowed() || object.values().any(|value| !value.is_string()) {
            report.issues.push(format!(
                "portal configuration page_classes {named} carries a reason, note or derive its class does not allow"
            ));
            return None;
        }
        parsed.push(entry);
    }
    Some(parsed)
}

/// Refuse a `figures` binding table that is not the closed shape.
pub(super) fn configured_figure_bindings(
    value: Option<&serde_json::Value>,
    report: &mut PortalValidationReport,
) -> Option<Vec<FigureBinding>> {
    let Some(value) = value else {
        return Some(Vec::new());
    };
    let Some(entries) = value
        .as_array()
        .filter(|entries| entries.len() <= MAX_FIGURE_BINDINGS)
    else {
        report.issues.push(format!(
            "portal configuration figures is not an array of at most {MAX_FIGURE_BINDINGS} bindings"
        ));
        return None;
    };
    let mut seen = BTreeSet::new();
    let mut parsed = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(object) = entry.as_object() else {
            report
                .issues
                .push("portal configuration figures binding is not an object".into());
            return None;
        };
        let text = |key: &str| {
            object
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        let declaration = text("declaration").filter(|path| {
            safe_path_text(path)
                && Path::new(path)
                    .extension()
                    .is_some_and(|extension| extension == "json")
        });
        let route = text("route").filter(|route| safe_path_text(route));
        let panel = text("panel");
        let anchor = text("anchor");
        let (Some(declaration), Some(route)) = (declaration, route) else {
            report.issues.push(
                "portal configuration figures binding needs a JSON declaration path and a route"
                    .into(),
            );
            return None;
        };
        let valid = object
            .keys()
            .all(|key| ["declaration", "route", "panel", "anchor"].contains(&key.as_str()))
            && object.values().all(serde_json::Value::is_string)
            && !(panel.is_some() && anchor.is_some())
            && panel
                .as_deref()
                .is_none_or(|panel| ALTITUDE_PANELS.contains(&panel))
            && anchor.as_deref().is_none_or(valid_anchor)
            && seen.insert((route.clone(), declaration.clone()));
        if !valid {
            report.issues.push(format!(
                "portal configuration figures binding {declaration} on {route} is not the closed shape"
            ));
            return None;
        }
        parsed.push(FigureBinding {
            declaration,
            route,
            panel,
            anchor,
        });
    }
    Some(parsed)
}

fn valid_anchor(anchor: &str) -> bool {
    static ANCHOR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_-]{1,200}$").expect("anchor pattern"));
    ANCHOR.is_match(anchor)
}

/// The class a source takes: its exact entry, else the longest prefix that
/// holds it, else explanatory. The adapter and the gate read the same rule.
fn class_for<'a>(entries: &'a [PageClassEntry], source_path: &str) -> Option<&'a PageClassEntry> {
    entries
        .iter()
        .find(|entry| entry.source.as_deref() == Some(source_path))
        .or_else(|| {
            entries
                .iter()
                .filter(|entry| {
                    entry.prefix.as_deref().is_some_and(|prefix| {
                        source_path.starts_with(&format!("{}/", prefix.trim_end_matches('/')))
                    })
                })
                .max_by_key(|entry| entry.prefix.as_deref().map_or(0, str::len))
        })
}

/// Everything the figure verifier needs from the rest of the validation run.
pub(super) struct FigureContext<'a> {
    pub repository: &'a Path,
    pub page_classes: &'a [PageClassEntry],
    pub bindings: &'a [FigureBinding],
    pub source_blobs: &'a BTreeMap<String, Vec<u8>>,
    pub rendered: &'a BTreeMap<String, String>,
}

pub(super) fn verify_figures(
    evidence: &Evidence,
    context: &FigureContext<'_>,
    report: &mut PortalValidationReport,
) {
    let Some(figures) = &evidence.figures else {
        // Evidence from a runtime before figures carries no class or binding
        // claims, and is only valid while the configuration declares none.
        if !context.page_classes.is_empty() || !context.bindings.is_empty() {
            report.issues.push(
                "evidence records no figures, but the configuration declares page classes or figure bindings".into(),
            );
        }
        if evidence.pages.iter().any(|page| {
            page.class.is_some()
                || !page.figures.is_empty()
                || page.source_region.is_some()
                || page.lookup.is_some()
        }) {
            report
                .issues
                .push("evidence claims page classes without its figure inventory".into());
        }
        return;
    };
    let pages_by_route: BTreeMap<&str, &Page> = evidence
        .pages
        .iter()
        .map(|page| (page.route.as_str(), page))
        .collect();
    for entry in context.page_classes {
        let named = entry
            .source
            .as_deref()
            .or(entry.prefix.as_deref())
            .unwrap_or_default();
        let matched = evidence
            .pages
            .iter()
            .any(|page| class_for(std::slice::from_ref(entry), &page.source_path).is_some());
        if !matched {
            report.issues.push(format!(
                "portal configuration page_classes declares {} for {named}, which matches no published source",
                entry.class
            ));
        }
    }
    for binding in context.bindings {
        if !pages_by_route.contains_key(binding.route.as_str()) {
            report.issues.push(format!(
                "portal configuration binds {} to {}, which is not a published route",
                binding.declaration, binding.route
            ));
        }
    }
    let declarations = verify_declarations(evidence, figures, context, report);
    for page in &evidence.pages {
        verify_page_class(page, context, &declarations, report);
    }
}

/// The configured declaration paths, after checking the evidence records each
/// one exactly once.
fn verify_inventory<'a>(
    figures: &[FigureEvidence],
    context: &FigureContext<'a>,
    report: &mut PortalValidationReport,
) -> Vec<&'a str> {
    let expected: BTreeSet<&str> = context
        .bindings
        .iter()
        .map(|binding| binding.declaration.as_str())
        .collect();
    let claimed: Vec<&str> = figures
        .iter()
        .map(|figure| figure.declaration_path.as_str())
        .collect();
    if claimed.iter().copied().collect::<BTreeSet<_>>() != expected
        || claimed.len() != expected.len()
    {
        report.issues.push(
            "evidence figure inventory does not match the configured figure declarations".into(),
        );
    }
    expected.into_iter().collect()
}

/// The declaration bytes behind every bound figure, read from the commit and
/// compared with the evidence, with every fact and derived value re-derived.
fn verify_declarations(
    evidence: &Evidence,
    figures: &[FigureEvidence],
    context: &FigureContext<'_>,
    report: &mut PortalValidationReport,
) -> BTreeMap<String, Pinned> {
    let paths = verify_inventory(figures, context, report);
    let blobs = match git_batch_blobs(
        context.repository,
        &evidence.repository.commit,
        &paths,
        MAX_DECLARATION_BYTES,
        MAX_TOTAL_DECLARATION_BYTES,
    ) {
        Ok(blobs) => blobs,
        Err(error) => {
            report.issues.push(format!(
                "authoritative Git figure declarations are unreadable: {error}"
            ));
            return BTreeMap::new();
        }
    };
    let mut hashes = BTreeMap::new();
    let mut declared_figures = Vec::new();
    for figure in figures {
        let path = &figure.declaration_path;
        let Some(bytes) = blobs.get(path) else {
            report.issues.push(format!(
                "figure declaration {path} is absent at the evidenced commit"
            ));
            continue;
        };
        let actual = sha256_hex(bytes);
        if !valid_sha256(&figure.declaration_sha256) || actual != figure.declaration_sha256 {
            report.issues.push(format!(
                "figure declaration {path} does not match its Git blob"
            ));
            continue;
        }
        let Ok(declaration) = parse_strict_json::<serde_json::Value>(bytes) else {
            report
                .issues
                .push(format!("figure declaration {path} is not strict JSON"));
            continue;
        };
        let Some(body) = declaration
            .get("figure")
            .and_then(serde_json::Value::as_object)
        else {
            report
                .issues
                .push(format!("figure declaration {path} has no figure object"));
            continue;
        };
        let field = |key: &str| {
            body.get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        };
        let mut routes: Vec<&str> = context
            .bindings
            .iter()
            .filter(|binding| &binding.declaration == path)
            .map(|binding| binding.route.as_str())
            .collect();
        routes.sort_unstable();
        if figure.grammar_version != 1
            || field("id") != figure.figure_id
            || field("family") != figure.family
            || field("binding") != figure.binding
            || routes != figure.routes.iter().map(String::as_str).collect::<Vec<_>>()
        {
            report.issues.push(format!(
                "figure {path} identity, family, binding or routes differ from its declaration"
            ));
        }
        declared_figures.push((figure, body.clone()));
        hashes.insert(
            path.clone(),
            Pinned {
                sha256: actual,
                declaration,
                derived: None,
            },
        );
    }
    for (path, derived) in verify_facts(evidence, &declared_figures, context, report) {
        if let Some(pinned) = hashes.get_mut(&path) {
            pinned.derived = derived;
        }
    }
    hashes
}

/// A declaration as the commit holds it, with the values its derived binding
/// re-derives from the committed source, so its figure can be reconstructed.
pub(super) struct Pinned {
    sha256: String,
    declaration: serde_json::Value,
    derived: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Pinned {
    /// The companion block the adapter writes for this declaration: the
    /// wrapper, the figure reconstructed from the declaration, and the line
    /// that attributes it to the declaration.
    fn companion(&self, path: &str, placement: &str, index: usize) -> Result<String, String> {
        let figure = render::render_figure(
            &self.declaration,
            &format!("cf-fig-{index}"),
            self.derived.as_ref(),
        )?;
        Ok(format!(
            "{}{figure}<p class=\"cf-companion-source\">Figure declared in <code>{}</code>, not part of the page source.</p></div>",
            companion_opening(path, placement, &self.sha256),
            super::escape_html_attribute(path)
        ))
    }
}

type DerivedValues = Option<serde_json::Map<String, serde_json::Value>>;

fn verify_facts(
    evidence: &Evidence,
    figures: &[(&FigureEvidence, serde_json::Map<String, serde_json::Value>)],
    context: &FigureContext<'_>,
    report: &mut PortalValidationReport,
) -> BTreeMap<String, DerivedValues> {
    let mut derived_values = BTreeMap::new();
    let mut sources = BTreeSet::new();
    for (_, body) in figures {
        for fact in body
            .get("facts")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some((path, _)) = fact
                .get("source")
                .and_then(serde_json::Value::as_str)
                .and_then(parse_fact_source)
            {
                sources.insert(path);
            }
        }
        if let Some(path) = body
            .get("source")
            .and_then(|source| source.get("path"))
            .and_then(serde_json::Value::as_str)
            .filter(|path| safe_path_text(path))
        {
            sources.insert(path.to_string());
        }
    }
    let paths: Vec<&str> = sources.iter().map(String::as_str).collect();
    let blobs = match git_batch_blobs(
        context.repository,
        &evidence.repository.commit,
        &paths,
        MAX_FACT_SOURCE_BYTES,
        MAX_TOTAL_FACT_SOURCE_BYTES,
    ) {
        Ok(blobs) => blobs,
        Err(error) => {
            report.issues.push(format!(
                "authoritative Git figure fact sources are unreadable: {error}"
            ));
            return derived_values;
        }
    };
    for (figure, body) in figures {
        let path = &figure.declaration_path;
        let declared = body
            .get("facts")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        if declared.len() != figure.facts.len() {
            report.issues.push(format!(
                "figure {path} evidence records a different fact count"
            ));
            continue;
        }
        for (fact, claimed) in declared.iter().zip(&figure.facts) {
            verify_fact(path, fact, claimed, &blobs, report);
        }
        derived_values.insert(
            path.clone(),
            verify_derived_binding(figure, body, &blobs, report),
        );
    }
    derived_values
}

/// One declared fact against its evidence and its committed source.
fn verify_fact(
    path: &str,
    fact: &serde_json::Value,
    claimed: &FactEvidence,
    blobs: &BTreeMap<String, Vec<u8>>,
    report: &mut PortalValidationReport,
) {
    let source = fact
        .get("source")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let value = fact
        .get("value")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let check = fact
        .get("check")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if fact.get("claim").and_then(serde_json::Value::as_str) != Some(claimed.claim.as_str())
        || source != claimed.source
        || check != claimed.check
        || !json_equal(&value, &claimed.drawn)
    {
        report.issues.push(format!(
            "figure {path} fact \"{}\" differs from its declaration",
            claimed.claim
        ));
        return;
    }
    let Some((file, _)) = parse_fact_source(source) else {
        report.issues.push(format!(
            "figure {path} fact source {source} is not a safe path"
        ));
        return;
    };
    let Some(bytes) = blobs.get(&file) else {
        report.issues.push(format!(
            "figure {path} fact source {file} is absent at the evidenced commit"
        ));
        return;
    };
    if sha256_hex(bytes) != claimed.source_sha256 {
        report.issues.push(format!(
            "figure {path} fact source {file} does not match its recorded hash"
        ));
    }
    match derive_fact(source, &check, bytes) {
            Ok(derived) if json_equal(&derived, &claimed.derived) && json_equal(&derived, &value) => {}
            Ok(derived) => report.issues.push(format!(
                "figure {path} rule 6 (fidelity): fact \"{}\" draws {value} but {source} gives {derived}",
                claimed.claim
            )),
            Err(error) => report.issues.push(format!(
                "figure {path} rule 6 (fidelity): fact \"{}\" cannot be derived: {error}",
                claimed.claim
            )),
        }
}

/// The derived values re-derived from the committed source, or `None` for an
/// authored figure or a source that does not resolve.
fn verify_derived_binding(
    figure: &FigureEvidence,
    body: &serde_json::Map<String, serde_json::Value>,
    blobs: &BTreeMap<String, Vec<u8>>,
    report: &mut PortalValidationReport,
) -> DerivedValues {
    let path = &figure.declaration_path;
    let binding = body.get("binding").and_then(serde_json::Value::as_str);
    let Some(claimed) = &figure.derived else {
        if binding == Some("derived") {
            report
                .issues
                .push(format!("derived figure {path} records no derived values"));
        }
        return None;
    };
    let source = |key: &str| {
        body.get("source")
            .and_then(|source| source.get(key))
            .and_then(serde_json::Value::as_str)
    };
    if binding != Some("derived")
        || source("path") != Some(claimed.source_path.as_str())
        || source("select") != Some(claimed.select.as_str())
    {
        report.issues.push(format!(
            "figure {path} derived evidence does not match its declared source"
        ));
        return None;
    }
    let Some(bytes) = blobs.get(&claimed.source_path) else {
        report.issues.push(format!(
            "figure {path} derived source {} is absent",
            claimed.source_path
        ));
        return None;
    };
    if sha256_hex(bytes) != claimed.source_sha256 {
        report.issues.push(format!(
            "figure {path} derived source {} does not match its recorded hash",
            claimed.source_path
        ));
    }
    let model = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|value| select_json(&value, &claimed.select).cloned());
    let Some(model) = model else {
        report.issues.push(format!(
            "figure {path} derived source does not resolve {}",
            claimed.select
        ));
        return None;
    };
    let layout = body.get("layout");
    let mut values = serde_json::Map::new();
    for group in ["rows", "limits"] {
        for entry in layout
            .and_then(|layout| layout.get(group))
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(selector) = entry.get("value").and_then(serde_json::Value::as_str) else {
                continue;
            };
            match select_json(&model, selector)
                .filter(|value| value.as_f64().is_some_and(f64::is_finite))
            {
                Some(value) => {
                    values.insert(selector.to_string(), value.clone());
                }
                None => report.issues.push(format!(
                    "figure {path} derived value {selector} is not a number in its source"
                )),
            }
        }
    }
    let derived = serde_json::Value::Object(values);
    if !json_equal(&derived, &serde_json::Value::Object(claimed.values.clone())) {
        report.issues.push(format!(
            "figure {path} rule 6 (fidelity): recorded derived values differ from {}",
            claimed.source_path
        ));
    }
    let drawn_keys: BTreeSet<&String> = claimed.drawn.keys().collect();
    let derived_keys: BTreeSet<&String> = claimed.values.keys().collect();
    let within = drawn_keys == derived_keys
        && claimed.values.iter().all(|(key, value)| {
            match (
                value.as_f64(),
                claimed.drawn.get(key).and_then(serde_json::Value::as_f64),
            ) {
                (Some(derived), Some(drawn)) => {
                    (drawn - derived).abs() <= DRAWN_VALUE_TOLERANCE * derived.abs().max(1.0)
                }
                _ => false,
            }
        });
    if !within {
        report.issues.push(format!(
            "figure {path} rule 6 (fidelity): drawn values differ from the values derived from the source"
        ));
    }
    derived.as_object().cloned()
}

/// The class, reason and bound figures a page claims, and for a source
/// rendered as it is, the unchanged region.
fn verify_page_class(
    page: &Page,
    context: &FigureContext<'_>,
    declarations: &BTreeMap<String, Pinned>,
    report: &mut PortalValidationReport,
) {
    let route = &page.route;
    if page.stale {
        if page.class.is_some()
            || page.class_reason.is_some()
            || page.class_note.is_some()
            || !page.figures.is_empty()
            || page.source_region.is_some()
            || page.lookup.is_some()
        {
            report
                .issues
                .push(format!("{route} stale stub claims a page class or figures"));
        }
        return;
    }
    let entry = class_for(context.page_classes, &page.source_path);
    let class = entry.map_or(EXPLANATORY, |entry| entry.class.as_str());
    if page.class.as_deref() != Some(class)
        || page.class_reason.as_deref() != entry.and_then(|entry| entry.reason.as_deref())
        || page.class_note.as_deref() != entry.and_then(|entry| entry.note.as_deref())
    {
        report.issues.push(format!(
            "{route} page class does not match the configuration ({class})"
        ));
        return;
    }
    let bindings: Vec<&FigureBinding> = context
        .bindings
        .iter()
        .filter(|binding| &binding.route == route)
        .collect();
    let allowed = match class {
        EXPLANATORY => bindings.iter().all(|binding| binding.panel.is_some()),
        "illustrated" => bindings.iter().all(|binding| binding.panel.is_none()),
        _ => bindings.is_empty(),
    };
    if !allowed {
        report.issues.push(format!(
            "{route} binds a figure to a place its {class} class does not allow"
        ));
    }
    let matches = page.figures.len() == bindings.len()
        && page.figures.iter().zip(&bindings).all(|(figure, binding)| {
            figure.declaration_path == binding.declaration
                && figure.placement == binding.placement()
                && figure.panel == binding.panel
                && figure.anchor == binding.anchor
                && declarations
                    .get(&binding.declaration)
                    .is_some_and(|pinned| pinned.sha256 == figure.declaration_sha256)
                && !figure.figure_id.is_empty()
        });
    if !matches {
        report.issues.push(format!(
            "{route} bound figures do not match the configuration and the pinned declarations"
        ));
    }
    let rendered = context.rendered.get(route);
    match class {
        EXPLANATORY => {
            if let Some(output) = rendered {
                verify_rendered_figures(page, output, true, declarations, report);
            }
            if page.source_region.is_some() || page.lookup.is_some() {
                report.issues.push(format!(
                    "{route} explanatory page claims a source region or lookup"
                ));
            }
        }
        "derived-lookup" => {
            verify_lookup(page, entry, context, report);
            if let Some(output) = rendered {
                verify_rendered_figures(page, output, false, declarations, report);
            }
        }
        _ => match (&page.source_region, rendered) {
            (Some(region), Some(output)) => {
                verify_source_region(page, region, output, context, declarations, report);
                verify_rendered_figures(page, output, false, declarations, report);
            }
            (None, _) => report.issues.push(format!(
                "{route} {class} source records no unchanged source region"
            )),
            (Some(_), None) => {}
        },
    }
}

/// Every figure a page puts in front of a reader must be a companion that
/// equals the reconstruction of a declaration bound to that page, each
/// binding exactly once, compared as parsed HTML. On an explanatory page each
/// sits under its declared panel heading. Figure or companion markup anywhere
/// else fails, however it is spelled; a comment renders nothing and text that
/// names a kit class is not markup. The page content carries no CSS and no
/// executable content: only the site's own built sheets and runtime may style
/// or script what a reader sees.
fn verify_rendered_figures(
    page: &Page,
    output: &str,
    panels: bool,
    declarations: &BTreeMap<String, Pinned>,
    report: &mut PortalValidationReport,
) {
    let route = &page.route;
    let rendered = dom::rendered_figures(markdown_body(output));
    let mut expected = Vec::new();
    for (index, figure) in page.figures.iter().enumerate() {
        let path = &figure.declaration_path;
        match declarations
            .get(path)
            .ok_or_else(|| "its declaration is not pinned".to_string())
            .and_then(|pinned| pinned.companion(path, &figure.placement, index))
            .and_then(|block| {
                dom::canonical_fragment(&block).ok_or_else(|| "it is not one element".to_string())
            }) {
            Ok(canonical) => expected.push((figure.panel.clone(), path, Some(canonical))),
            Err(why) => {
                report.issues.push(format!(
                    "{route} figure {path} cannot be reconstructed: {why}"
                ));
                expected.push((figure.panel.clone(), path, None));
            }
        }
    }
    let mut matched = vec![false; expected.len()];
    for (panel, companion) in &rendered.companions {
        let found = expected
            .iter()
            .enumerate()
            .position(|(index, (want, _, text))| {
                !matched[index]
                    && (!panels || want == panel)
                    && text.as_deref() == Some(companion.as_str())
            });
        match found {
            Some(index) => matched[index] = true,
            None if panels => report.issues.push(format!(
                "{route} renders a companion that no bound declaration draws in the {} panel",
                panel.as_deref().unwrap_or("page")
            )),
            None => report.issues.push(format!(
                "{route} renders a companion that no bound declaration draws"
            )),
        }
    }
    for ((_, path, text), found) in expected.iter().zip(&matched) {
        if text.is_some() && !found {
            report.issues.push(format!(
                "{route} does not render the figure its declaration {path} draws"
            ));
        }
    }
    if rendered.stray > 0 {
        report.issues.push(format!(
            "{route} renders companion or figure markup outside its companion blocks"
        ));
    }
    if !rendered.css.is_empty() {
        let kinds: Vec<&str> = rendered.css.iter().map(String::as_str).collect();
        report.issues.push(format!(
            "{route} renders page CSS, which only the site's own sheets may carry: {}",
            kinds.join(", ")
        ));
    }
    if !rendered.active.is_empty() {
        let kinds: Vec<&str> = rendered.active.iter().map(String::as_str).collect();
        report.issues.push(format!(
            "{route} renders executable content, which only the site's runtime may carry: {}",
            kinds.join(", ")
        ));
    }
}

/// The committed list of the runtime's fixed inline scripts, in the portal.
pub(super) const RUNTIME_SCRIPTS_FILE: &str = "scripts/runtime-scripts.json";
const REGENERATE: &str = "npm run build && node scripts/runtime-scripts.mjs dist";

/// The hashes an inline script outside the page content may have: the
/// runtime's fixed scripts as committed, and the pre-paint display script the
/// configuration's theme writes into the committed template.
pub(super) fn runtime_inline_scripts(
    list: &[u8],
    config: &[u8],
) -> Result<BTreeSet<String>, String> {
    let list: serde_json::Value =
        serde_json::from_slice(list).map_err(|error| error.to_string())?;
    let config: serde_json::Value =
        serde_json::from_slice(config).map_err(|error| error.to_string())?;
    let theme = config
        .get("theme")
        .and_then(serde_json::Value::as_str)
        .ok_or("the configuration names no theme")?;
    let template = list
        .get("pre_paint_template")
        .and_then(serde_json::Value::as_str)
        .ok_or("it has no pre-paint template")?;
    let mut allowed: BTreeSet<String> = list
        .get("scripts")
        .and_then(serde_json::Value::as_array)
        .ok_or("it lists no scripts")?
        .iter()
        .map(|script| {
            script
                .get("sha256")
                .and_then(serde_json::Value::as_str)
                .filter(|sha| valid_sha256(sha))
                .map(str::to_string)
                .ok_or_else(|| "a script entry has no sha256".to_string())
        })
        .collect::<Result<_, _>>()?;
    let theme = serde_json::to_string(theme).map_err(|error| error.to_string())?;
    allowed.insert(sha256_hex(
        template.replacen("__THEME__", &theme, 1).as_bytes(),
    ));
    Ok(allowed)
}

/// A built page, read before a browser consumes its templates: its content
/// region carries no CSS or executable content beyond the exact output of a
/// code block (the recorded Expressive Code assets and its token custom
/// properties), the rest of the page carries none of what the runtime never
/// emits, and every inline script outside the content is one the runtime
/// emits.
pub(super) fn verify_built_page(
    path: &str,
    html: &str,
    runtime_scripts: Option<&BTreeSet<String>>,
    code_blocks: &CodeBlockAssets,
    report: &mut PortalValidationReport,
) {
    let found = dom::built_page_carriers(html, code_blocks);
    if let Some(allowed) = runtime_scripts {
        let unknown: BTreeSet<String> = found
            .inline_scripts
            .iter()
            .map(|text| sha256_hex(text.as_bytes()))
            .filter(|sha| !allowed.contains(sha))
            .map(|sha| sha[..12].to_string())
            .collect();
        if !unknown.is_empty() {
            report.issues.push(format!(
                "built page {path} carries inline scripts the site's runtime does not emit (sha256 {}); if the runtime changed, regenerate {RUNTIME_SCRIPTS_FILE}: {REGENERATE}",
                unknown.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    if !found.content.is_empty() {
        let kinds: Vec<&str> = found.content.iter().map(String::as_str).collect();
        report.issues.push(format!(
            "built page {path} carries CSS or executable content in its content: {}",
            kinds.join(", ")
        ));
    }
    if !found.page.is_empty() {
        let kinds: Vec<&str> = found.page.iter().map(String::as_str).collect();
        report.issues.push(format!(
            "built page {path} carries what the site's runtime never emits: {}",
            kinds.join(", ")
        ));
    }
}

fn verify_lookup(
    page: &Page,
    entry: Option<&PageClassEntry>,
    context: &FigureContext<'_>,
    report: &mut PortalValidationReport,
) {
    let route = &page.route;
    let derive = entry.and_then(|entry| entry.derive.as_deref());
    let rows = context
        .source_blobs
        .get(&page.source_path)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .map(|text| parse_capabilities(text).0.len());
    let valid = page.source_path == CAPABILITY_REGISTRY
        && page.source_region.is_none()
        && page.lookup.as_ref().is_some_and(|lookup| {
            Some(lookup.derive.as_str()) == derive && Some(lookup.rows) == rows
        });
    if !valid {
        report.issues.push(format!(
            "{route} derived lookup does not match its source and configured derivation"
        ));
    }
}

fn companion_opening(declaration: &str, placement: &str, sha256: &str) -> String {
    format!(
        "<div class=\"cf-companion not-content\" data-cf-companion=\"{}\" data-cf-placement=\"{placement}\" data-cf-declaration-sha256=\"{sha256}\">",
        super::escape_html_attribute(declaration)
    )
}

/// A source rendered as it is: the region between the markers, less every
/// recorded insertion, equals the committed body after its title, and every
/// insertion is one bound companion figure at the place its binding names.
fn verify_source_region(
    page: &Page,
    region: &SourceRegion,
    output: &str,
    context: &FigureContext<'_>,
    declarations: &BTreeMap<String, Pinned>,
    report: &mut PortalValidationReport,
) {
    let route = &page.route;
    let fail = |report: &mut PortalValidationReport, why: &str| {
        report.issues.push(format!(
            "{route} rendered source region does not equal the committed source: {why}"
        ));
    };
    let Some(source) = context
        .source_blobs
        .get(&page.source_path)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
    else {
        return fail(report, "the committed source is unreadable");
    };
    let normalized = normalize_markdown_source(source);
    let body = markdown_body(&normalized);
    if region.source_start_bytes != as_is_region_start(body, &page.title) {
        return fail(report, "the region does not start after the page title");
    }
    let committed = &body.as_bytes()[region.source_start_bytes..];
    let bytes = output.as_bytes();
    let class = page.class.as_deref().unwrap_or_default();
    let begin = format!(
        "<!-- codeflow-source-begin route={} source_sha256={} class={class} -->\n",
        strict_url_route(route),
        page.source_sha256
    );
    let end = region.output_offset_bytes.checked_add(region.region_bytes);
    let Some(end) = end.filter(|end| *end <= bytes.len()) else {
        return fail(report, "the recorded region lies outside the rendered page");
    };
    let rendered_region = &bytes[region.output_offset_bytes..end];
    if !bytes[..region.output_offset_bytes].ends_with(begin.as_bytes())
        || !bytes[end..].starts_with(SOURCE_END_MARKER.as_bytes())
        || output.matches("<!-- codeflow-source-begin").count() != 1
        || sha256_hex(rendered_region) != region.region_sha256
    {
        return fail(report, "the region markers or hash do not match");
    }
    match strip_inserts(
        page,
        region,
        rendered_region,
        &anchor_offsets(body),
        declarations,
    ) {
        Err(why) => fail(report, &why),
        Ok(remaining) if remaining != committed => {
            fail(
                report,
                "the page body no longer equals the committed source",
            );
        }
        Ok(_) => {}
    }
}

/// The rendered region with every recorded insertion removed, after proving
/// each one is the companion its pinned declaration draws, byte for byte, at
/// the place its binding names.
fn strip_inserts(
    page: &Page,
    region: &SourceRegion,
    rendered_region: &[u8],
    anchors: &BTreeMap<String, usize>,
    declarations: &BTreeMap<String, Pinned>,
) -> Result<Vec<u8>, String> {
    let mut remaining = Vec::with_capacity(rendered_region.len());
    let mut cursor = 0;
    let mut consumed_source = 0;
    let mut placed = Vec::new();
    for insert in &region.inserts {
        let Some(stop) = insert.offset_bytes.checked_add(insert.block_bytes) else {
            return Err("an insertion overflows".into());
        };
        if insert.offset_bytes < cursor || stop > rendered_region.len() {
            return Err("insertions overlap or leave the region".into());
        }
        remaining.extend_from_slice(&rendered_region[cursor..insert.offset_bytes]);
        consumed_source += insert.offset_bytes - cursor;
        let block = &rendered_region[insert.offset_bytes..stop];
        let Some((index, sha)) = page
            .figures
            .iter()
            .enumerate()
            .find(|(_, figure)| figure.declaration_path == insert.declaration_path)
            .map(|(index, figure)| (index, figure.declaration_sha256.as_str()))
        else {
            return Err("an insertion names a figure the page does not bind".into());
        };
        let expected_offset = match insert.placement.as_str() {
            "head" if insert.anchor.is_none() => Some(0),
            "anchor" => insert
                .anchor
                .as_deref()
                .and_then(|anchor| anchors.get(anchor))
                .and_then(|after| after.checked_sub(region.source_start_bytes))
                .filter(|offset| *offset > 0),
            _ => None,
        };
        let companion = declarations
            .get(&insert.declaration_path)
            .filter(|pinned| pinned.sha256 == sha)
            .ok_or_else(|| "its declaration is not pinned".to_string())
            .and_then(|pinned| pinned.companion(&insert.declaration_path, &insert.placement, index))
            .map_err(|why| {
                format!(
                    "the companion {} cannot be reconstructed: {why}",
                    insert.declaration_path
                )
            })?;
        let expected = format!(
            "\n<!-- codeflow-companion-begin declaration={} sha256={sha} -->\n\n{companion}{COMPANION_END}",
            insert.declaration_path
        );
        if expected_offset != Some(consumed_source)
            || sha256_hex(block) != insert.block_sha256
            || block != expected.as_bytes()
        {
            return Err(format!(
                "the companion {} is not the recorded insertion at its bound place",
                insert.declaration_path
            ));
        }
        placed.push((
            insert.declaration_path.as_str(),
            insert.placement.as_str(),
            insert.anchor.as_deref(),
        ));
        cursor = stop;
    }
    remaining.extend_from_slice(&rendered_region[cursor..]);
    let mut bound: Vec<(&str, &str, Option<&str>)> = page
        .figures
        .iter()
        .map(|figure| {
            (
                figure.declaration_path.as_str(),
                figure.placement.as_str(),
                figure.anchor.as_deref(),
            )
        })
        .collect();
    bound.sort_unstable();
    placed.sort_unstable();
    if bound != placed {
        return Err("the insertions are not exactly the bound figures".into());
    }
    Ok(remaining)
}

// ---------------------------------------------------------------------------
// Markdown rules shared with the adapter (figure-grammar.mjs, lib.mjs)
// ---------------------------------------------------------------------------

/// The byte offset where an as-is region starts: after leading blank lines, one
/// level-one heading that repeats the title, and the blank lines after it.
fn as_is_region_start(body: &str, title: &str) -> usize {
    static HEADING: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^ {0,3}#[ \t]+(.*?)(?:[ \t]+#+)?[ \t]*$").expect("title heading pattern")
    });
    let lines: Vec<&str> = body.split('\n').collect();
    let blank = |line: &str| {
        line.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
            .is_empty()
    };
    let mut index = 0;
    let mut offset = 0;
    while index + 1 < lines.len() && blank(lines[index]) {
        offset += lines[index].len() + 1;
        index += 1;
    }
    let Some(heading) = lines.get(index).and_then(|line| HEADING.captures(line)) else {
        return 0;
    };
    if index + 1 >= lines.len() || !title_matches(&heading[1], title) {
        return 0;
    }
    offset += lines[index].len() + 1;
    index += 1;
    while index + 1 < lines.len() && blank(lines[index]) {
        offset += lines[index].len() + 1;
        index += 1;
    }
    offset
}

fn title_matches(raw: &str, title: &str) -> bool {
    static PREFIX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:ADR|EPC|SPC|TSK|CAP)-[0-9]+(?:-[0-9]+)?\s*[\u{2014}\u{2013}:-]\s*")
            .expect("record prefix pattern")
    });
    let clean = |value: &str| {
        value
            .chars()
            .filter(|c| !matches!(c, '`' | '*' | '_'))
            .collect::<String>()
            .trim()
            .to_string()
    };
    let visible = clean(raw);
    let wanted = clean(title);
    let unprefixed = PREFIX.replace(&visible, "").to_string();
    let rest = |value: &str| value.chars().skip(1).collect::<String>();
    let first = |value: &str| {
        value
            .chars()
            .next()
            .map(|c| c.to_lowercase().collect::<String>())
    };
    visible == wanted
        || unprefixed == wanted
        || (rest(&unprefixed) == rest(&wanted) && first(&unprefixed) == first(&wanted))
}

/// One ATX heading outside fenced code: its line, level, anchor and the lines
/// of its section.
struct Section {
    line: usize,
    anchor: String,
    body: String,
}

fn markdown_sections(text: &str) -> Vec<Section> {
    static FENCE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^ {0,3}(`{3,}|~{3,})(.*)$").expect("fence pattern"));
    static HEADING: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?[ \t]*$").expect("heading pattern")
    });
    static CLOSING: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[ \t]+#+[ \t]*$").expect("closing hashes pattern"));
    let normalized = normalize_markdown_source(text);
    let normalized = normalized.strip_prefix('\u{feff}').unwrap_or(&normalized);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut headings: Vec<(usize, usize, String)> = Vec::new();
    let mut seen = BTreeMap::<String, usize>::new();
    let mut fence: Option<(char, usize)> = None;
    for (index, line) in lines.iter().enumerate() {
        let marker = FENCE.captures(line);
        if let Some((character, length)) = fence {
            if let Some(marker) = &marker {
                let run = &marker[1];
                if run.starts_with(character) && run.len() >= length && marker[2].trim().is_empty()
                {
                    fence = None;
                }
            }
            continue;
        }
        if let Some(marker) = &marker {
            let run = &marker[1];
            if !(run.starts_with('`') && marker[2].contains('`')) {
                fence = Some((run.chars().next().unwrap_or('`'), run.len()));
                continue;
            }
        }
        let Some(heading) = HEADING.captures(line) else {
            continue;
        };
        let level = heading[1].len();
        let raw = heading.get(2).map_or("", |value| value.as_str());
        let raw = CLOSING.replace(raw, "");
        let raw = if raw.chars().all(|c| c == '#') {
            ""
        } else {
            raw.as_ref()
        };
        let base = slug_heading(raw);
        let count = seen.entry(base.clone()).or_insert(0);
        let anchor = if *count == 0 {
            base.clone()
        } else {
            format!("{base}-{count}")
        };
        *count += 1;
        headings.push((index, level, anchor));
    }
    headings
        .iter()
        .enumerate()
        .map(|(position, (line, level, anchor))| {
            let next = headings[position + 1..]
                .iter()
                .find(|(_, candidate, _)| candidate <= level)
                .map_or(lines.len(), |(next, _, _)| *next);
            Section {
                line: *line,
                anchor: anchor.clone(),
                body: lines[line + 1..next].join("\n"),
            }
        })
        .collect()
}

fn slug_heading(raw: &str) -> String {
    static IMAGE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"!\[([^\]]*)\]\([^)]*\)").expect("image pattern"));
    static LINK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").expect("link pattern"));
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").expect("tag pattern"));
    static KEEP: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}\p{M}_\- ]$").expect("slug pattern"));
    let visible = IMAGE.replace_all(raw, "$1");
    let visible = LINK.replace_all(&visible, "$1");
    let visible: String = visible
        .chars()
        .filter(|c| !matches!(c, '`' | '*' | '~'))
        .collect();
    let visible = TAG.replace_all(&visible, "");
    let mut buffer = [0; 4];
    visible
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| KEEP.is_match(c.encode_utf8(&mut buffer)))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// The byte offset just after each anchored heading line, in the body.
fn anchor_offsets(body: &str) -> BTreeMap<String, usize> {
    let mut starts = Vec::new();
    let mut cursor = 0;
    for line in body.split('\n') {
        starts.push(cursor);
        cursor += line.len() + 1;
    }
    markdown_sections(body)
        .into_iter()
        .map(|section| {
            let after = starts
                .get(section.line + 1)
                .copied()
                .unwrap_or(body.len())
                .min(body.len());
            (section.anchor, after)
        })
        .collect()
}

fn parse_fact_source(value: &str) -> Option<(String, Option<String>)> {
    let (path, anchor) = match value.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor.to_string())),
        None => (value, None),
    };
    (safe_path_text(path) && anchor.as_deref().is_none_or(valid_anchor))
        .then(|| (path.to_string(), anchor))
}

/// One fact re-derived from its committed source: `contains`, `count-items`
/// or `json`, the closed set the grammar declares.
fn derive_fact(
    source: &str,
    check: &serde_json::Value,
    bytes: &[u8],
) -> Result<serde_json::Value, String> {
    static ITEM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?:[-*+]|[0-9]{1,9}[.)])[ \t]").expect("item pattern"));
    let (_, anchor) = parse_fact_source(source).ok_or("the source is not a safe path")?;
    let text = std::str::from_utf8(bytes).map_err(|_| "the source is not UTF-8".to_string())?;
    let kind = check
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if kind == "json" {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|error| error.to_string())?;
        let selector = check
            .get("select")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        return select_json(&value, selector)
            .cloned()
            .ok_or_else(|| format!("selector {selector} does not resolve"));
    }
    let normalized = normalize_markdown_source(text);
    let normalized = normalized
        .strip_prefix('\u{feff}')
        .unwrap_or(&normalized)
        .to_string();
    let scope = match anchor {
        None => normalized,
        Some(anchor) => markdown_sections(&normalized)
            .into_iter()
            .find(|section| section.anchor == anchor)
            .map(|section| section.body)
            .ok_or_else(|| format!("fact anchor does not exist: {source}"))?,
    };
    match kind {
        "contains" => {
            let needle = check
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            Ok(serde_json::Value::Bool(
                !needle.is_empty() && scope.contains(needle),
            ))
        }
        "count-items" => Ok(serde_json::Value::from(
            scope.split('\n').filter(|line| ITEM.is_match(line)).count(),
        )),
        other => Err(format!("unknown fact check {other}")),
    }
}

fn select_json<'a>(value: &'a serde_json::Value, selector: &str) -> Option<&'a serde_json::Value> {
    selector
        .split('.')
        .try_fold(value, |cursor, segment| match cursor {
            serde_json::Value::Array(items)
                if segment.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                segment
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| items.get(index))
            }
            serde_json::Value::Object(map) => map.get(segment),
            _ => None,
        })
}

/// JSON equality as the grammar's canonical form sees it: numbers by value,
/// objects without regard to key order.
fn json_equal(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    match (left, right) {
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => a.as_f64() == b.as_f64(),
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| json_equal(x, y))
        }
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, x)| b.get(key).is_some_and(|y| json_equal(x, y)))
        }
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_and_slugs_match_the_grammar_module() {
        let source = "# Guide\n\n## Install\n\n1. Run the installer.\n2. Check the result.\n\n```md\n## Not a heading\n```\n\n## Install\n\n- again\n";
        let anchors: Vec<String> = markdown_sections(source)
            .into_iter()
            .map(|section| section.anchor)
            .collect();
        assert_eq!(anchors, ["guide", "install", "install-1"]);
        assert_eq!(
            slug_heading("Plan graph (Plan v4.3)"),
            "plan-graph-plan-v43"
        );
        assert_eq!(
            slug_heading("4. Preserve evidence and safety"),
            "4-preserve-evidence-and-safety"
        );
        assert_eq!(
            slug_heading("The `cf-stage` [fence](x.md) <em>now</em>"),
            "the-cf-stage-fence-now"
        );
        let check = serde_json::json!({ "kind": "count-items" });
        assert_eq!(
            derive_fact("guide.md#install", &check, source.as_bytes()),
            Ok(serde_json::json!(2))
        );
        assert_eq!(
            derive_fact("guide.md#install-1", &check, source.as_bytes()),
            Ok(serde_json::json!(1))
        );
        assert!(derive_fact("guide.md#uninstall", &check, source.as_bytes())
            .unwrap_err()
            .contains("fact anchor does not exist"));
        let contains = serde_json::json!({ "kind": "contains", "text": "Check the result" });
        assert_eq!(
            derive_fact("guide.md#install", &contains, source.as_bytes()),
            Ok(serde_json::json!(true))
        );
        let json = serde_json::json!({ "kind": "json", "select": "git.limits.1" });
        assert_eq!(
            derive_fact("policy.json", &json, br#"{"git":{"limits":[50,72.0]}}"#),
            Ok(serde_json::json!(72.0))
        );
        assert!(json_equal(&serde_json::json!(72.0), &serde_json::json!(72)));
        assert!(json_equal(
            &serde_json::json!({"a": 1, "b": 2}),
            &serde_json::json!({"b": 2, "a": 1})
        ));
    }

    #[test]
    fn the_region_starts_after_a_title_heading_only() {
        assert_eq!(
            as_is_region_start("# Install guide\n\nBody.\n", "Install guide"),
            17
        );
        assert_eq!(
            as_is_region_start("\n# ADR-0001: first decision\n\nBody.\n", "First decision"),
            29
        );
        assert_eq!(
            as_is_region_start("# Another title\n\nBody.\n", "Install guide"),
            0
        );
        assert_eq!(as_is_region_start("Body first.\n", "Install guide"), 0);
        assert_eq!(as_is_region_start("# Install guide", "Install guide"), 0);
    }

    #[test]
    fn page_class_and_binding_tables_are_closed() {
        let mut report = PortalValidationReport::default();
        let accepted = serde_json::json!([
            { "source": "docs/guide.md", "class": "illustrated" },
            { "prefix": "docs/decisions", "class": "pass-through", "reason": "accepted-record" },
            { "prefix": "docs/misc", "class": "pass-through", "reason": "no-relationship", "note": "A list of links." },
            { "source": "docs/capabilities.md", "class": "derived-lookup", "derive": "capability-registry" }
        ]);
        let entries = configured_page_classes(Some(&accepted), &mut report).expect("closed table");
        assert!(report.is_clean(), "{:?}", report.issues);
        assert_eq!(
            class_for(&entries, "docs/decisions/ADR-0001.md").map(|entry| entry.class.as_str()),
            Some("pass-through")
        );
        assert_eq!(
            class_for(&entries, "docs/guide.md").map(|entry| entry.class.as_str()),
            Some("illustrated")
        );
        assert!(class_for(&entries, "docs/other.md").is_none());
        for rejected in [
            serde_json::json!([{ "prefix": "docs/decisions", "class": "pass-through" }]),
            serde_json::json!([{ "prefix": "docs/decisions", "class": "pass-through", "reason": "tidy" }]),
            serde_json::json!([{ "prefix": "docs/misc", "class": "pass-through", "reason": "no-relationship" }]),
            serde_json::json!([{ "source": "docs/a.md", "prefix": "docs", "class": "illustrated" }]),
            serde_json::json!([{ "source": "docs/a.md", "class": "illustrated", "reason": "governance" }]),
            serde_json::json!([{ "prefix": "docs", "class": "derived-lookup", "derive": "capability-registry" }]),
            serde_json::json!([{ "source": "docs/a.md", "class": "decorative" }]),
            serde_json::json!([{ "source": "docs/a.md", "class": "illustrated" }, { "source": "docs/a.md", "class": "illustrated" }]),
        ] {
            let mut report = PortalValidationReport::default();
            assert!(
                configured_page_classes(Some(&rejected), &mut report).is_none(),
                "{rejected} was accepted"
            );
            assert!(!report.is_clean());
        }
        let mut report = PortalValidationReport::default();
        let bindings = serde_json::json!([
            { "declaration": "figures/a.json", "route": "orient/product", "panel": "concept" },
            { "declaration": "figures/b.json", "route": "reference/guide", "anchor": "install" },
            { "declaration": "figures/c.json", "route": "reference/guide" }
        ]);
        let parsed =
            configured_figure_bindings(Some(&bindings), &mut report).expect("closed bindings");
        assert_eq!(
            parsed
                .iter()
                .map(FigureBinding::placement)
                .collect::<Vec<_>>(),
            ["panel", "anchor", "head"]
        );
        for rejected in [
            serde_json::json!([{ "declaration": "figures/a.md", "route": "x" }]),
            serde_json::json!([{ "declaration": "figures/a.json", "route": "x", "panel": "summary" }]),
            serde_json::json!([{ "declaration": "figures/a.json", "route": "x", "panel": "concept", "anchor": "a" }]),
            serde_json::json!([{ "declaration": "../a.json", "route": "x" }]),
            serde_json::json!([{ "declaration": "figures/a.json", "route": "x", "anchor": "not an anchor" }]),
            serde_json::json!([{ "declaration": "figures/a.json", "route": "x" }, { "declaration": "figures/a.json", "route": "x" }]),
        ] {
            let mut report = PortalValidationReport::default();
            assert!(
                configured_figure_bindings(Some(&rejected), &mut report).is_none(),
                "{rejected} was accepted"
            );
        }
    }

    /// A whole illustrated portal, written the way the adapter writes one: a
    /// committed source, a committed declaration with a derived fact, and a
    /// rendered page carrying the source unchanged with one companion figure
    /// at its head.
    struct IllustratedPortal {
        temp: tempfile::TempDir,
        evidence: serde_json::Value,
        rendered: String,
    }

    const GUIDE: &str = "# Guide\n\nRead this first.\n\n## Install\n\n1. Run it.\n2. Check it.\n";
    const DECLARATION: &str = r#"{"schema_version":1,"figure":{"id":"steps","family":"sequence","binding":"authored","question":"What runs first?","idea":"Running comes before checking.","title":"Run, then check","caption":"Run it, then check it.","twin":"facts","states":[{"name":"trans","mark":"trans","means":"Step"}],"facts":[{"claim":"Two steps","source":"docs/guide.md#install","derive":"numbered items","check":{"kind":"count-items"},"value":2}],"wide":{"width":240,"height":60,"draw":[{"state":"trans","shape":"path","d":"M20 36H200","head":"end","id":"m1"},{"text":"run, then check","x":110,"y":24,"anchor":"middle","for":["m1"]}]},"narrow":{"recompose":"stack","drops":[],"marks":"same","width":200,"height":60,"draw":[{"state":"trans","shape":"path","d":"M20 36H180","head":"end","id":"m1"},{"text":"run, then check","x":20,"y":24,"for":["m1"]}]}}}"#;

    fn run_git(root: &Path, args: &[&str]) {
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap()
            .success());
    }

    #[allow(clippy::too_many_lines)] // One fixture carries every file the validator reads, so each tamper case below differs from it by one claim.
    fn illustrated_portal() -> IllustratedPortal {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for directory in [
            ".codeflow",
            "docs",
            "figures",
            "portal/.portal/generated",
            "portal/src/content/docs/reference",
            "portal/public/markdown/reference",
            "portal/dist/reference/guide",
        ] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        let config = serde_json::json!({
            "schema_version": 1,
            "title": "Fixture guide",
            "description": "Illustrated source fixture",
            "theme": "signal",
            "repository_url": null,
            "repository_root": "..",
            "release_version": null,
            "primitive_tokens": null,
            "source_roots": ["docs"],
            "exclude": [],
            "layers": [
                {"id":"orient","label":"Orient","description":"Start","paths":[]},
                {"id":"system","label":"System","description":"Decisions","prefixes":["docs/decisions"]},
                {"id":"reference","label":"Reference","description":"Other","fallback":true}
            ],
            "page_classes": [{"source": "docs/guide.md", "class": "illustrated"}],
            "figures": [{"declaration": "figures/steps.json", "route": "reference/guide"}],
            "base": "/"
        });
        let config_bytes = serde_json::to_vec_pretty(&config).unwrap();
        std::fs::write(root.join("portal/portal.config.json"), &config_bytes).unwrap();
        std::fs::create_dir_all(root.join("portal/scripts")).unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs-portal/scripts/runtime-scripts.json"),
            root.join("portal/scripts/runtime-scripts.json"),
        )
        .unwrap();

        std::fs::write(root.join("docs/guide.md"), GUIDE).unwrap();
        std::fs::write(root.join("figures/steps.json"), DECLARATION).unwrap();
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "portal-tests@codeflow.invalid"][..],
            &["config", "user.name", "Portal tests"][..],
            &["config", "core.autocrlf", "false"][..],
            &[
                "add",
                "docs",
                "figures",
                "portal/portal.config.json",
                "portal/scripts/runtime-scripts.json",
            ][..],
            &["commit", "-q", "-m", "fixture"][..],
        ] {
            run_git(root, args);
        }
        let commit = super::super::git_text_bounded(root, &["rev-parse", "HEAD"], 1024)
            .unwrap()
            .trim()
            .to_owned();
        let source_hash = sha256_hex(GUIDE.as_bytes());
        let declaration_hash = sha256_hex(DECLARATION.as_bytes());
        let pinned = Pinned {
            sha256: declaration_hash.clone(),
            declaration: serde_json::from_str(DECLARATION).unwrap(),
            derived: None,
        };
        let block = format!(
            "\n<!-- codeflow-companion-begin declaration=figures/steps.json sha256={declaration_hash} -->\n\n{}{COMPANION_END}",
            pinned.companion("figures/steps.json", "head", 0).unwrap()
        );
        let start = "# Guide\n\n".len();
        let region = format!("{block}{}", &GUIDE[start..]);
        let opening = format!(
            "---\ntitle: \"Guide\"\n---\n\n<!-- codeflow-page-provenance source_sha256={source_hash} built_from_commit={commit} portal_version=1.0.0 release_version=none -->\n<div class=\"portal-provenance\">Source <code>docs/guide.md</code> at <code>{commit}</code></div>\n\n<div data-pagefind-body data-codeflow-search-root=\"reference/guide\">\n\n<div class=\"portal-source\" data-cf-source-region data-cf-page-class=\"illustrated\">\n\n<!-- codeflow-source-begin route=reference/guide source_sha256={source_hash} class=illustrated -->\n"
        );
        let rendered = format!("{opening}{region}{SOURCE_END_MARKER}\n</div>\n\n</div>\n");
        let llms = format!(
            "# Fixture guide\n\nIllustrated source fixture\n\nRepository commit: {commit}\n\n- [reference/guide](./markdown/reference/guide.md) \u{2014} docs/guide\\.md\n"
        );
        std::fs::write(root.join("portal/public/llms.txt"), &llms).unwrap();
        let built = "<main data-pagefind-body data-codeflow-search-root=\"reference/guide\"><h1>Guide</h1></main>\n";
        std::fs::write(root.join("portal/dist/reference/guide/index.html"), built).unwrap();
        let adoption = serde_json::json!({
            "schema_version": 1,
            "root": "portal",
            "starter_version": "1.0.0",
            "files": {"portal.config.json": {"ownership": "user-owned", "pristine_sha256": sha256_hex(&config_bytes)}}
        });
        std::fs::write(
            root.join(".codeflow/docs-portal.json"),
            serde_json::to_vec(&adoption).unwrap(),
        )
        .unwrap();
        let figures = serde_json::json!([{
            "declaration_path": "figures/steps.json",
            "declaration_sha256": declaration_hash,
            "grammar_version": 1,
            "figure_id": "steps",
            "family": "sequence",
            "binding": "authored",
            "routes": ["reference/guide"],
            "facts": [{
                "claim": "Two steps",
                "source": "docs/guide.md#install",
                "source_sha256": source_hash,
                "check": {"kind": "count-items"},
                "drawn": 2,
                "derived": 2
            }],
            "derived": null
        }]);
        let source_region = serde_json::json!({
            "source_start_bytes": start,
            "output_offset_bytes": opening.len(),
            "region_bytes": region.len(),
            "region_sha256": sha256_hex(region.as_bytes()),
            "inserts": [{
                "offset_bytes": 0,
                "block_bytes": block.len(),
                "block_sha256": sha256_hex(block.as_bytes()),
                "declaration_path": "figures/steps.json",
                "placement": "head",
                "anchor": null
            }]
        });
        let pages = serde_json::json!([{
            "source_path": "docs/guide.md",
            "source_sha256": source_hash,
            "built_from_commit": commit,
            "route": "reference/guide",
            "title": "Guide",
            "status": null,
            "output_markdown": "src/content/docs/reference/guide.md",
            "output_markdown_sha256": "",
            "markdown_twin": "public/markdown/reference/guide.md",
            "markdown_twin_sha256": "",
            "stale": false,
            "searchable": true,
            "ids": [],
            "relationships": [],
            "backlinks": [],
            "snippets": [],
            "stale_reason": null,
            "page_class": "illustrated",
            "class_reason": null,
            "class_note": null,
            "figures": [{
                "declaration_path": "figures/steps.json",
                "declaration_sha256": declaration_hash,
                "figure_id": "steps",
                "placement": "head",
                "panel": null,
                "anchor": null
            }],
            "source_region": source_region,
            "lookup": null
        }]);
        let evidence = serde_json::json!({
            "schema_version": 1,
            "generator": {"name": "@codeflow/docs-portal", "version": "1.0.0"},
            "repository": {"root": "..", "commit": commit, "release_version": null},
            "config_sha256": sha256_hex(&config_bytes),
            "primitive_tokens": null,
            "media": [],
            "figures": figures,
            "pages": pages,
            "llms": {"path": "public/llms.txt", "sha256": sha256_hex(llms.as_bytes())},
            "artifacts": [{"path": "dist/reference/guide/index.html", "sha256": sha256_hex(built.as_bytes())}]
        });
        let portal = IllustratedPortal {
            temp,
            evidence,
            rendered,
        };
        portal.write(&portal.rendered, &portal.evidence);
        portal
    }

    impl IllustratedPortal {
        /// Write one rendered page and its evidence, with the page hashes the
        /// evidence claims kept consistent, so only the claim under test fails.
        fn write(&self, rendered: &str, evidence: &serde_json::Value) {
            let root = self.temp.path();
            for path in [
                "portal/src/content/docs/reference/guide.md",
                "portal/public/markdown/reference/guide.md",
            ] {
                std::fs::write(root.join(path), rendered).unwrap();
            }
            let mut evidence = evidence.clone();
            let hash = sha256_hex(rendered.as_bytes());
            evidence["pages"][0]["output_markdown_sha256"] = hash.clone().into();
            evidence["pages"][0]["markdown_twin_sha256"] = hash.into();
            std::fs::write(
                root.join("portal/.portal/generated/evidence.json"),
                serde_json::to_vec(&evidence).unwrap(),
            )
            .unwrap();
        }

        fn issues(&self, rendered: &str, evidence: &serde_json::Value) -> Vec<String> {
            self.write(rendered, evidence);
            super::super::validate_portal(self.temp.path(), Path::new("portal")).issues
        }
    }

    #[test]
    fn validate_accepts_an_inserted_figure_and_rejects_a_tampered_source() {
        let portal = illustrated_portal();
        let clean = portal.issues(&portal.rendered, &portal.evidence);
        assert!(clean.is_empty(), "{clean:?}");

        // The rendered route no longer carries the committed body: one byte of
        // the source changes and every hash the evidence records is rewritten
        // to match, so only the equality with the committed source can fail.
        let tampered = portal.rendered.replace("2. Check it.", "2. Chuck it.");
        let region = &portal.evidence["pages"][0]["source_region"];
        let offset = usize::try_from(region["output_offset_bytes"].as_u64().unwrap()).unwrap();
        let length = usize::try_from(region["region_bytes"].as_u64().unwrap()).unwrap();
        let mut evidence = portal.evidence.clone();
        evidence["pages"][0]["source_region"]["region_sha256"] =
            sha256_hex(&tampered.as_bytes()[offset..offset + length]).into();
        let issues = portal.issues(&tampered, &evidence);
        assert!(
            issues.iter().any(|issue| issue.contains("reference/guide rendered source region does not equal the committed source: the page body no longer equals the committed source")),
            "{issues:?}"
        );

        // A companion moved off the page head, a region that swallows the
        // title, and an insertion the page does not bind all fail.
        let mut moved = portal.evidence.clone();
        moved["pages"][0]["source_region"]["inserts"][0]["placement"] = "anchor".into();
        moved["pages"][0]["source_region"]["inserts"][0]["anchor"] = "install".into();
        assert!(portal
            .issues(&portal.rendered, &moved)
            .iter()
            .any(|issue| issue.contains("is not the recorded insertion at its bound place")));
        let mut early = portal.evidence.clone();
        early["pages"][0]["source_region"]["source_start_bytes"] = 0.into();
        assert!(portal
            .issues(&portal.rendered, &early)
            .iter()
            .any(|issue| issue.contains("does not start after the page title")));
    }

    /// The issues for a portal whose companion text `original` is replaced by
    /// `edited`, with the block, region and page hashes all rewritten to
    /// match, so only the bound reconstruction of the pinned declaration can
    /// notice the change.
    fn companion_tamper_issues(original: &str, edited: &str) -> Vec<String> {
        let portal = illustrated_portal();
        assert!(portal.rendered.contains(original), "{original}");
        let tampered = portal.rendered.replace(original, edited);
        let region = &portal.evidence["pages"][0]["source_region"];
        let offset = usize::try_from(region["output_offset_bytes"].as_u64().unwrap()).unwrap();
        let region_len = usize::try_from(region["region_bytes"].as_u64().unwrap()).unwrap();
        let block_len =
            usize::try_from(region["inserts"][0]["block_bytes"].as_u64().unwrap()).unwrap();
        let count = portal.rendered[offset..offset + block_len]
            .matches(original)
            .count();
        assert!(count > 0);
        let growth = (edited.len() - original.len()) * count;
        let block = &tampered.as_bytes()[offset..offset + block_len + growth];
        let whole = &tampered.as_bytes()[offset..offset + region_len + growth];
        let mut evidence = portal.evidence.clone();
        let source_region = &mut evidence["pages"][0]["source_region"];
        source_region["region_bytes"] = (region_len + growth).into();
        source_region["region_sha256"] = sha256_hex(whole).into();
        source_region["inserts"][0]["block_bytes"] = (block_len + growth).into();
        source_region["inserts"][0]["block_sha256"] = sha256_hex(block).into();
        portal.issues(&tampered, &evidence)
    }

    fn assert_companion_refused(issues: &[String]) {
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("is not the recorded insertion at its bound place")),
            "{issues:?}"
        );
        assert!(
            issues.iter().any(|issue| issue
                == "reference/guide does not render the figure its declaration figures/steps.json draws"),
            "{issues:?}"
        );
    }

    #[test]
    fn validate_rejects_companion_text_its_declaration_does_not_draw() {
        // The visible caption inside the companion changes.
        assert_companion_refused(&companion_tamper_issues(
            "Run it, then check it.",
            "Run it, then skip the check.",
        ));
    }

    #[test]
    fn validate_rejects_a_tampered_figure_title_or_entity() {
        // The visible title line (SPC-014 B5) and a review entity's label
        // (B2) are part of the reconstruction, so neither can be edited.
        assert_companion_refused(&companion_tamper_issues(
            "<span class=\"cf-fig-name\">Run, then check</span>",
            "<span class=\"cf-fig-name\">Run, then skip it</span>",
        ));
        assert_companion_refused(&companion_tamper_issues(
            "data-cf-entity-label=\"run, then check\"",
            "data-cf-entity-label=\"run, then skip it\"",
        ));
        assert_companion_refused(&companion_tamper_issues(
            "data-cf-entity=\"m1\"",
            "data-cf-entity=\"m12\"",
        ));
    }

    #[test]
    fn rendered_figures_are_read_as_a_browser_parses_them() {
        let companion = "<div class=\"cf-companion\" data-cf-companion=\"f.json\"><figure class=\"cf-fig\"><figcaption>Drawn.</figcaption></figure></div>";
        let canonical = dom::canonical_fragment(companion).unwrap();
        // Positive control: a comment, a code span, a path and plain text that
        // name a kit class render no figure markup (R2-1, R3-4).
        let clean = format!(
            "<section data-altitude=\"concept\">\n\n## Concept\n\n<!-- {companion} -->\n\n{companion}\n\nThe cf-fig class is documented here, with `class=\"cf-fig\"` in code.\n\n<div class=\"portal-provenance\">Source <code>docs/cf-figures.md</code></div>\n\n<div data-codeflow-search-root=\"reference/cf-figures\">x</div>\n\n</section>\n"
        );
        let found = dom::rendered_figures(&clean);
        assert_eq!(
            found.companions,
            vec![(Some("concept".to_string()), canonical.clone())]
        );
        assert_eq!(found.stray, 0);
        // A companion spelled with character references is still a
        // companion, and one inside other markup is stray (R3-1).
        let encoded = format!(
            "## Concept\n\n{companion}\n\n<div class=\"cf&#45;companion\"><figure class=\"cf&#45;fig\"><figcaption>FALSE UNBOUND CONTENT.</figcaption></figure></div>\n"
        );
        let found = dom::rendered_figures(&encoded);
        assert_eq!(found.companions.len(), 2);
        assert_ne!(found.companions[1].1, canonical);
        for smuggled in [
            "## Concept\n\n<div class=\"note\"><figure class=\"cf&#x2D;fig\"></figure></div>\n"
                .to_string(),
            "## Concept\n\n- item <span class=\"cf-fig\">x</span>\n".to_string(),
            "## Concept\n\n> <svg class=\"cf-fig-svg\"></svg>\n".to_string(),
        ] {
            assert!(dom::rendered_figures(&smuggled).stray > 0, "{smuggled}");
        }
        // Inside an as-is region raw HTML is text; only the companion between
        // its markers is markup.
        let region = format!(
            "<!-- codeflow-source-begin route=r source_sha256=x class=illustrated -->\n\n<!-- codeflow-companion-begin declaration=f.json sha256=x -->\n\n{companion}\n\n<!-- codeflow-companion-end -->\n\n<div class=\"cf-fig\">raw source markup stays text</div>\n\n<!-- codeflow-source-end -->\n"
        );
        let found = dom::rendered_figures(&region);
        assert_eq!((found.companions.len(), found.stray), (1, 0));
        assert!(found.css.is_empty(), "{:?}", found.css);
        // Page content carries no CSS (R4-1, R4-2): each rule the review used
        // as a style block, a style attribute, a stylesheet link, an SVG style
        // and a declarative shadow root are all found; the clean page and a
        // style block inside an as-is region, which renders as text, are not.
        assert!(dom::rendered_figures(&clean).css.is_empty());
        for rule in [
            ".cf-fig { opacity: 0; }",
            ".cf-companion { opacity: 0; }",
            ".cf-fig { clip-path: inset(50%); }",
            ".cf-fig { filter: opacity(0); }",
            ".cf-fig-caption { visibility: hidden; }",
            ".cf-fig { translate: 0 1000px; }",
            ".cf-fig-svg .cf-m-trans { stroke-dasharray: 0 100000; }",
        ] {
            let styled = format!("## Concept\n\n<style>{rule}</style>\n\n{companion}\n");
            let css = dom::rendered_figures(&styled).css;
            assert_eq!(
                css.into_iter().collect::<Vec<_>>(),
                ["a <style> element"],
                "{rule}"
            );
        }
        for (page, kind) in [
            ("## Concept\n\n<div style=\"opacity:0\">\n\nx\n\n</div>\n", "a style attribute on <div>"),
            ("## Concept\n\nText <span style=\"opacity:0\">inline</span>.\n", "a style attribute on <span>"),
            ("## Concept\n\n<link rel=\"stylesheet\" href=\"/x.css\">\n", "a <link> element"),
            // A code block imitated in a Markdown source: the built-page
            // allowance never applies to what the adapter generates.
            ("## Concept\n\n<div class=\"expressive-code\"><figure><pre><span style=\"--0:#82AAFF\">x</span></pre></figure></div>\n", "a style attribute on <span>"),
            ("## Concept\n\n<div class=\"expressive-code\"><link rel=\"stylesheet\" href=\"/_astro/ec.w36nc.css\"></div>\n", "a <link> element"),
            ("## Concept\n\n<svg><style>.cf-fig{opacity:0}</style></svg>\n", "a <style> element"),
            ("## Concept\n\n<div><template shadowrootmode=\"open\"><style>p{}</style></template></div>\n", "a <style> element"),
        ] {
            assert!(dom::rendered_figures(page).css.contains(kind), "{page}");
        }
        let inert = "<!-- codeflow-source-begin route=r source_sha256=x class=illustrated -->\n\n<style>.cf-fig{opacity:0}</style>\n\n<script>alert(1)</script>\n\n<!-- codeflow-source-end -->\n";
        assert!(dom::rendered_figures(inert).css.is_empty());
        assert!(dom::rendered_figures(inert).active.is_empty());
        // Nor executable content (R5-1, R5-2): the review's CSSOM insertion,
        // handlers, script URLs however spelled, frames and shadow roots of
        // either mode. The clean page carries none.
        assert!(dom::rendered_figures(&clean).active.is_empty());
        for (page, kind) in [
            ("## Concept\n\n<script>\ndocument.styleSheets[0].insertRule(\".cf-fig {opacity:0}\", document.styleSheets[0].cssRules.length);\n</script>\n", "a <script> element"),
            ("## Concept\n\n<svg><script>void 0</script></svg>\n", "a <script> element"),
            ("## Concept\n\n<img src=\"x.png\" onerror=\"void 0\">\n", "an event-handler attribute on <img>"),
            ("## Concept\n\nText <span ONMOUSEOVER=\"void 0\">here</span>.\n", "an event-handler attribute on <span>"),
            ("## Concept\n\n<a href=\" java\tscript:void(0)\">x</a>\n", "a script URL on <a>"),
            ("## Concept\n\n<svg><a xlink:href=\"JavaScript:void(0)\"><text>x</text></a></svg>\n", "a script URL on <a>"),
            ("## Concept\n\n<svg><a><set attributeName=\"href\" to=\"javascript:void(0)\"/></a></svg>\n", "a script URL on <set>"),
            ("## Concept\n\n<iframe srcdoc=\"x\"></iframe>\n", "an <iframe> element"),
            ("## Concept\n\n<object data=\"x.svg\"></object>\n", "an <object> element"),
            ("## Concept\n\n<embed src=\"x.svg\">\n", "an <embed> element"),
            ("## Concept\n\n<div><template shadowrootmode=\"closed\"><style>:host{opacity:0}</style>Shadow</template></div>\n", "a declarative shadow root"),
            ("## Concept\n\n<div><template shadowrootmode=\"open\"><style>:host{opacity:0}</style>Shadow</template></div>\n", "a declarative shadow root"),
        ] {
            assert!(dom::rendered_figures(page).active.contains(kind), "{page}");
        }
        // A plain link, and text that mentions a script URL, run nothing.
        assert!(dom::rendered_figures("## Concept\n\n<a href=\"/guide/\" title=\"javascript: tips\">guide</a> about javascript: URLs\n").active.is_empty());
    }

    /// A built page is read before a browser consumes its templates (R5-2).
    /// The runtime's head scripts and its icon template are allowed; its
    /// content region carries nothing, and nothing anywhere carries a shadow
    /// root, a handler or a frame.
    #[test]
    fn built_pages_carry_only_what_the_runtime_emits() {
        let carriers = |html: &str| dom::built_page_carriers(html, &CodeBlockAssets::default());
        let page = |content: &str, chrome: &str| {
            format!("<!doctype html><html><head><script>(function(){{}})();</script><script type=\"module\" src=\"/_astro/page.js\"></script></head><body><template id=\"theme-icons\"><svg></svg></template><nav style=\"--depth: 0;\">{chrome}</nav><main><div class=\"sl-markdown-content\"><p>Text.</p>{content}</div></main></body></html>")
        };
        let clean = carriers(&page("", ""));
        assert!(
            clean.content.is_empty() && clean.page.is_empty(),
            "{:?} {:?}",
            clean.content,
            clean.page
        );
        let closed = "<div><template shadowrootmode=\"closed\"><style>:host{opacity:0}</style>Shadow</template></div>";
        let open = closed.replace("closed", "open");
        for shadow in [closed.to_string(), open] {
            assert!(carriers(&page(&shadow, ""))
                .content
                .contains("a declarative shadow root"));
            assert!(carriers(&page("", &shadow))
                .page
                .contains("a declarative shadow root"));
        }
        let script = carriers(&page(
            "<script>document.styleSheets[0].insertRule(\".cf-fig{opacity:0}\")</script>",
            "",
        ));
        assert!(script.content.contains("a <script> element"));
        let styled = carriers(&page("<p style=\"opacity:0\">x</p>", ""));
        assert!(styled.content.contains("a style attribute on <p>"));
        let handler = carriers(&page("", "<button onclick=\"void 0\">x</button>"));
        assert!(handler
            .page
            .contains("an event-handler attribute on <button>"));
        let style = carriers(&page("", "<style>.cf-fig{opacity:0}</style>"));
        assert!(style.page.contains("a <style> element"));
        assert_eq!(clean.inline_scripts, ["(function(){})();"]);
    }

    const CODE_BLOCK_ASSETS: [&str; 5] = [
        "dist/_astro/ec.w36nc.css",
        "dist/_astro/ec.0vx5m.js",
        "dist/_astro/common.DP3CCCu_.css",
        "dist/_astro/page.LAbJoB63.js",
        "dist/index.html",
    ];
    const CODE_BLOCK_HEAD: &str = "<link rel=\"stylesheet\" href=\"/_astro/ec.w36nc.css\"><script type=\"module\" src=\"/_astro/ec.0vx5m.js\"></script>";
    const CODE_BLOCK_TOKENS: &str = "<span style=\"--0:#82AAFF;--1:#3B61B0\">curl</span><span style=\"--0:#D6DEEB;--1:#403F53\"> </span><span style=\"--0:#637777;--0fs:italic;--1:#5F636FE3;--1fs:italic\">x</span><span style=\"--0bg:#1D3B53;--0fw:bold;--0td:underline line-through\">y</span>";

    /// A code block as Expressive Code writes it, with `head` before its
    /// frame, `pre` in the pre tag and `tokens` as its one line.
    fn code_block(head: &str, pre: &str, tokens: &str) -> String {
        format!("<div class=\"expressive-code\">{head}<figure class=\"frame is-terminal not-content\"><figcaption class=\"header\"><span class=\"title\"></span></figcaption><pre data-language=\"sh\"{pre}><code><div class=\"ec-line\"><div class=\"code\">{tokens}</div></div></code></pre><div class=\"copy\"><button data-code=\"curl\"><div></div></button></div></figure></div>")
    }

    /// What a built page with `html` in its content carries there.
    fn code_block_carriers(html: &str, assets: &CodeBlockAssets) -> Vec<String> {
        let page = format!("<!doctype html><html><head></head><body><main><div class=\"sl-markdown-content\"><p>Text.</p>{html}</div></main></body></html>");
        dom::built_page_carriers(&page, assets)
            .content
            .into_iter()
            .collect()
    }

    /// A fenced code block renders through Expressive Code, which writes a
    /// link and a script for its recorded assets and custom properties on its
    /// tokens into the content. Exactly that passes, under the configured
    /// base; a block imitated in raw HTML with only those properties is
    /// harmless and passes too.
    #[test]
    fn code_blocks_pass_as_expressive_code_writes_them() {
        let assets = CodeBlockAssets::recorded(Some("/"), CODE_BLOCK_ASSETS);
        let content = code_block_carriers;
        let (block, head, tokens) = (code_block, CODE_BLOCK_HEAD, CODE_BLOCK_TOKENS);
        // Every form Expressive Code writes: token colours for two theme
        // variants, background, italic, bold and decoration, and the wrapped
        // block's longest line and a wrapped line's indent.
        let wrapped = block(
            head,
            " class=\"wrap\" style=\"--ecMaxLine:42ch\"",
            &format!("<div class=\"ec-line\" style=\"--ecIndent:4ch\">{tokens}</div>"),
        );
        for html in [block(head, "", tokens), wrapped, block("", "", tokens)] {
            assert_eq!(content(&html, &assets), Vec::<String>::new(), "{html}");
        }
        // The same block under a base path links its assets under that base.
        let guide = CodeBlockAssets::recorded(Some("/guide/"), CODE_BLOCK_ASSETS);
        let under_base = block(
            "<link rel=\"stylesheet\" href=\"/guide/_astro/ec.w36nc.css\"><script type=\"module\" src=\"/guide/_astro/ec.0vx5m.js\"></script>",
            "",
            tokens,
        );
        assert!(content(&under_base, &guide).is_empty());
        assert_eq!(
            content(&block(head, "", tokens), &guide),
            ["a <link> element", "a <script> element"]
        );
        assert_eq!(
            content(
                &block(head, "", tokens),
                &CodeBlockAssets::recorded(None, CODE_BLOCK_ASSETS)
            ),
            ["a <link> element", "a <script> element"]
        );
    }

    /// A style attribute passes only on a block's frame and only with the
    /// custom properties Expressive Code writes: a real property, a value
    /// that loads or computes, another declaration, or an allowed property
    /// outside a block's frame fails.
    #[test]
    fn code_block_styles_hold_only_expressive_code_properties() {
        let assets = CodeBlockAssets::recorded(Some("/"), CODE_BLOCK_ASSETS);
        let content = code_block_carriers;
        let (block, head) = (code_block, CODE_BLOCK_HEAD);
        // A real property, a value that loads or computes, or a declaration
        // Expressive Code does not write, inside the frame.
        for style in [
            "color:red",
            "--0:#fff;color:red",
            "opacity:0",
            "transform:scale(2)",
            "display:none",
            "background:url(/x.png)",
            "--0:url(/x.png)",
            "--0bg:var(--x)",
            "--0:#fff/*;*/",
            "--0:#ff\\66",
            "--0:#fff !important",
            "--0fs:oblique",
            "--0fw:900",
            "--ecIndent:1em",
            "--tmLabel:\"x\"",
            "--cf-fig:#fff",
            "--123:#fff",
            "",
        ] {
            let html = block(head, "", &format!("<span style=\"{style}\">x</span>"));
            assert_eq!(
                content(&html, &assets),
                ["a style attribute on <span>"],
                "{style}"
            );
        }
        // An allowed property outside a block's frame: in plain content, on
        // the block, in its caption, or in a block with no figure frame.
        for html in [
            "<p style=\"--0:#82AAFF\">x</p>".to_string(),
            "<div class=\"expressive-code\" style=\"--0:#82AAFF\"></div>".to_string(),
            "<div class=\"expressive-code\"><figure><figcaption><span style=\"--0:#82AAFF\">t</span></figcaption></figure></div>".to_string(),
            "<div class=\"expressive-code\"><pre><span style=\"--0:#82AAFF\">t</span></pre></div>".to_string(),
            "<div class=\"note\"><figure><pre><span style=\"--0:#82AAFF\">t</span></pre></figure></div>".to_string(),
        ] {
            let found = content(&html, &assets);
            assert_eq!(found.len(), 1, "{html}");
            assert!(found[0].starts_with("a style attribute on <"), "{html}");
        }
    }

    /// A link or script that is not a recorded asset as Expressive Code
    /// writes it at the head of a block, a style element, and a figure inside
    /// a block, real or imitated, all fail.
    #[test]
    fn code_block_assets_and_figures_are_refused_otherwise() {
        let assets = CodeBlockAssets::recorded(Some("/"), CODE_BLOCK_ASSETS);
        let content = code_block_carriers;
        let (block, head, tokens) = (code_block, CODE_BLOCK_HEAD, CODE_BLOCK_TOKENS);
        // A link or script that is not the recorded asset, not as Expressive
        // Code writes it, or not at the head of a block.
        for (asset, kind) in [
            (
                "<link rel=\"stylesheet\" href=\"/_astro/ec.zzzzz.css\">",
                "a <link> element",
            ),
            (
                "<link rel=\"stylesheet\" href=\"/_astro/common.DP3CCCu_.css\">",
                "a <link> element",
            ),
            (
                "<link rel=\"stylesheet\" href=\"/_astro/ec.w36nc.css\" media=\"print\">",
                "a <link> element",
            ),
            (
                "<link rel=\"preload\" href=\"/_astro/ec.w36nc.css\">",
                "a <link> element",
            ),
            (
                "<link rel=\"stylesheet\" href=\"https://elsewhere.test/_astro/ec.w36nc.css\">",
                "a <link> element",
            ),
            (
                "<script type=\"module\" src=\"/_astro/ec.zzzzz.js\"></script>",
                "a <script> element",
            ),
            (
                "<script type=\"module\" src=\"/_astro/page.LAbJoB63.js\"></script>",
                "a <script> element",
            ),
            (
                "<script src=\"/_astro/ec.0vx5m.js\"></script>",
                "a <script> element",
            ),
            (
                "<script type=\"module\" src=\"/_astro/ec.0vx5m.js\">void 0</script>",
                "a <script> element",
            ),
            ("<style>.cf-fig{opacity:0}</style>", "a <style> element"),
        ] {
            assert_eq!(
                content(&block(asset, "", tokens), &assets),
                [kind],
                "{asset}"
            );
        }
        for asset in [
            "<link rel=\"stylesheet\" href=\"/_astro/ec.w36nc.css\">",
            "<script type=\"module\" src=\"/_astro/ec.0vx5m.js\"></script>",
        ] {
            assert_eq!(content(asset, &assets).len(), 1, "{asset}");
            assert_eq!(
                content(
                    &format!(
                        "<div><div class=\"expressive-code\"><figure>{asset}</figure></div></div>"
                    ),
                    &assets
                )
                .len(),
                1,
                "{asset}"
            );
        }
        // An allowed token attribute excuses only itself: a style element, a
        // link that is not the recorded asset link, or a script on the same
        // element inside the frame still fails (Codex CB-1, Grok F2).
        for (payload, kind) in [
            (
                "<style style=\"--0:#82AAFF\">.cf-fig{opacity:0}</style>",
                "a <style> element",
            ),
            (
                "<link style=\"--0:#82AAFF\" rel=\"stylesheet\" href=\"/evil.css\">",
                "a <link> element",
            ),
            (
                "<link style=\"--0:#82AAFF\" rel=\"stylesheet\" href=\"/_astro/ec.w36nc.css\">",
                "a <link> element",
            ),
            (
                "<script style=\"--0:#82AAFF\">void 0</script>",
                "a <script> element",
            ),
        ] {
            assert_eq!(
                content(&block(head, "", &format!("{tokens}{payload}")), &assets),
                [kind],
                "{payload}"
            );
        }
        let styled_head = CODE_BLOCK_HEAD.replace("<link ", "<link style=\"--0:#82AAFF\" ");
        assert_eq!(
            content(&block(&styled_head, "", tokens), &assets),
            ["a <link> element", "a style attribute on <link>"]
        );
    }

    /// The Expressive Code sheet scopes its rules on the `expressive-code`
    /// class whatever the tag, so no figure or companion may sit on or inside
    /// any element with that class (Codex CB-2, Grok F1). A real block beside
    /// a figure passes.
    #[test]
    fn no_figure_sits_in_the_code_block_sheet_scope() {
        let assets = CodeBlockAssets::recorded(Some("/"), CODE_BLOCK_ASSETS);
        let content = code_block_carriers;
        let companion = "<div class=\"cf-companion not-content\" data-cf-companion=\"figures/a.json\"><figure class=\"cf-fig\"><figcaption class=\"cf-fig-caption\">c</figcaption><ul class=\"cf-legend\"></ul></figure></div>";
        let inside = "figure or companion markup inside a code block".to_string();
        let mut wrapped: Vec<String> = ["div", "section", "article", "span", "aside", "main"]
            .iter()
            .map(|tag| format!("<{tag} class=\"note expressive-code\">{companion}</{tag}>"))
            .collect();
        wrapped.push(code_block(
            CODE_BLOCK_HEAD,
            "",
            &format!("<span style=\"--0:#82AAFF\">{companion}</span>"),
        ));
        wrapped.push(companion.replace(
            "cf-companion not-content",
            "cf-companion not-content expressive-code",
        ));
        wrapped.push(companion.replace("class=\"cf-fig\"", "class=\"cf-fig expressive-code\""));
        wrapped
            .push(companion.replace("class=\"cf-legend\"", "class=\"cf-legend expressive-code\""));
        for html in &wrapped {
            assert!(content(html, &assets).contains(&inside), "{html}");
        }
        let beside = format!(
            "{}{companion}",
            code_block(CODE_BLOCK_HEAD, "", CODE_BLOCK_TOKENS)
        );
        assert!(content(&beside, &assets).is_empty());
    }

    /// Every inline script outside the content is one the runtime emits: the
    /// committed fixed scripts, and the pre-paint script the configured theme
    /// writes into the committed template. Any other names the regeneration.
    #[test]
    fn inline_scripts_are_held_to_the_runtime_list() {
        let list = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs-portal/scripts/runtime-scripts.json"),
        )
        .unwrap();
        let committed: serde_json::Value = serde_json::from_slice(&list).unwrap();
        let allowed = runtime_inline_scripts(&list, br#"{"theme": "signal"}"#).unwrap();
        assert_eq!(
            allowed.len(),
            committed["scripts"].as_array().unwrap().len() + 1
        );
        let pre_paint = committed["pre_paint_template"].as_str().unwrap().replacen(
            "__THEME__",
            "\"signal\"",
            1,
        );
        assert!(allowed.contains(&sha256_hex(pre_paint.as_bytes())));
        let folio = runtime_inline_scripts(&list, br#"{"theme": "folio"}"#).unwrap();
        assert!(!folio.contains(&sha256_hex(pre_paint.as_bytes())));
        let built = |head: &str| {
            format!("<!doctype html><html><head><script>{pre_paint}</script>{head}</head><body><main><div class=\"sl-markdown-content\"><p>Text.</p></div></main></body></html>")
        };
        let mut report = PortalValidationReport::default();
        verify_built_page(
            "dist/a/index.html",
            &built(""),
            Some(&allowed),
            &CodeBlockAssets::default(),
            &mut report,
        );
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        verify_built_page(
            "dist/a/index.html",
            &built("<script>document.styleSheets[0].insertRule(\".cf-fig{opacity:0}\")</script>"),
            Some(&allowed),
            &CodeBlockAssets::default(),
            &mut report,
        );
        assert_eq!(report.issues.len(), 1, "{:?}", report.issues);
        assert!(report.issues[0].starts_with("built page dist/a/index.html carries inline scripts the site's runtime does not emit (sha256 "));
        assert!(report.issues[0].ends_with("if the runtime changed, regenerate scripts/runtime-scripts.json: npm run build && node scripts/runtime-scripts.mjs dist"));
    }

    #[test]
    fn validate_rederives_facts_classes_and_bindings_from_the_commit() {
        let portal = illustrated_portal();
        let expect = |evidence: &serde_json::Value, needle: &str| {
            let issues = portal.issues(&portal.rendered, evidence);
            assert!(
                issues.iter().any(|issue| issue.contains(needle)),
                "expected {needle:?} in {issues:?}"
            );
        };
        let mut wrong_fact = portal.evidence.clone();
        wrong_fact["figures"][0]["facts"][0]["derived"] = 3.into();
        expect(
            &wrong_fact,
            "rule 6 (fidelity): fact \"Two steps\" draws 2 but docs/guide.md#install gives 2",
        );
        let mut drawn = portal.evidence.clone();
        drawn["figures"][0]["facts"][0]["drawn"] = 3.into();
        expect(&drawn, "fact \"Two steps\" differs from its declaration");
        let mut declaration = portal.evidence.clone();
        declaration["figures"][0]["declaration_sha256"] = "0".repeat(64).into();
        expect(
            &declaration,
            "figure declaration figures/steps.json does not match its Git blob",
        );
        let mut class = portal.evidence.clone();
        class["pages"][0]["page_class"] = "explanatory".into();
        expect(
            &class,
            "reference/guide page class does not match the configuration (illustrated)",
        );
        let mut unbound = portal.evidence.clone();
        unbound["pages"][0]["figures"] = serde_json::json!([]);
        expect(
            &unbound,
            "reference/guide bound figures do not match the configuration",
        );
        let mut legacy = portal.evidence.clone();
        legacy.as_object_mut().unwrap().remove("figures");
        expect(&legacy, "evidence records no figures, but the configuration declares page classes or figure bindings");
    }

    #[test]
    fn a_derived_figure_is_compared_with_its_source_and_its_drawing() {
        let declaration: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../docs-portal/tests/fixtures/figures/10-extent-derived.json"
        ))
        .unwrap();
        let body = declaration["figure"].as_object().unwrap().clone();
        let policy = b"{\"git\":{\"commit_desc_max_len\":50,\"commit_subject_max_len\":72}}\n";
        let blobs = BTreeMap::from([("policy.json".to_string(), policy.to_vec())]);
        let evidence = |edit: &dyn Fn(&mut serde_json::Value)| {
            let mut value = serde_json::json!({
                "declaration_path": "figures/commit-limits.json",
                "declaration_sha256": "0".repeat(64),
                "grammar_version": 1,
                "figure_id": "commit-limits",
                "family": "extent",
                "binding": "derived",
                "routes": [],
                "facts": [],
                "derived": {
                    "source_path": "policy.json",
                    "source_sha256": sha256_hex(policy),
                    "select": "git",
                    "values": { "commit_desc_max_len": 50, "commit_subject_max_len": 72 },
                    "drawn": { "commit_desc_max_len": 50.02, "commit_subject_max_len": 72 }
                }
            });
            edit(&mut value);
            let figure: FigureEvidence = serde_json::from_value(value).unwrap();
            let mut report = PortalValidationReport {
                checked_pages: 0,
                issues: Vec::new(),
            };
            verify_derived_binding(&figure, &body, &blobs, &mut report);
            report.issues
        };
        assert_eq!(evidence(&|_| {}), Vec::<String>::new());
        let drawn = "figure figures/commit-limits.json rule 6 (fidelity): drawn values differ from the values derived from the source";
        assert_eq!(
            evidence(&|value| value["derived"]["drawn"]["commit_desc_max_len"] = 60.into()),
            [drawn]
        );
        assert_eq!(
            evidence(&|value| {
                value["derived"]["values"]["commit_desc_max_len"] = 60.into();
                value["derived"]["drawn"]["commit_desc_max_len"] = 60.into();
            }),
            ["figure figures/commit-limits.json rule 6 (fidelity): recorded derived values differ from policy.json"]
        );
        assert_eq!(
            evidence(&|value| value["derived"]["source_path"] = "other.json".into()),
            ["figure figures/commit-limits.json derived evidence does not match its declared source"]
        );
    }
}
