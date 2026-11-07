//! Convert a legacy fence catalog into one record per capability in one run.
//!
//! ```text
//!   read docs/capabilities.md --> plan every entry (span, fields, page)
//!        |                           | ambiguous or invalid: refuse, nothing written
//!        v                           v
//!   register each number with a minted uid --> write each record
//!        --> check each record's uid against its binding
//!        --> strip the converted sections from docs/capabilities.md (last write)
//! ```
//!
//! Every refusal that the input can cause happens while planning, before the
//! registry or any file changes. The rerun of an interrupted conversion and
//! its repair are not handled here: an existing record or binding for a legacy
//! number refuses the run.

use std::path::Path;

use serde::Serialize;

use super::{
    catalog_error, parse_capabilities, parse_record, top_level_headings, validate_capability_path,
    validate_record, AreaConfig, CapabilityCatalogError, CapabilityEntry,
    CAPABILITY_MAX_FILE_BYTES, CAPABILITY_MAX_VALUE_CHARS, CATALOG_DIR, LEGACY_PATH, PROJECT_PATH,
    RECORD_KEYS,
};
use crate::ids::{issue, Kind, Ledger, RegId, Standing};

/// Facet values the migration supplies when a fence does not carry them.
/// The entry's classification is later work; these keep the record valid.
pub const DEFAULT_KIND: &str = "functional";
/// The default audience: never `external`, so nothing is presented to outside
/// readers before someone classifies it.
pub const DEFAULT_AUDIENCE: &str = "internal";
/// The stage of a shipped entry that has none.
pub const DEFAULT_STAGE: &str = "unassessed";

/// One converted capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    pub id: String,
    pub uid: String,
    /// The record path, relative to the repository root.
    pub file: String,
    /// Whether the record is an existing page that gained frontmatter.
    pub page: bool,
    pub standing: Standing,
}

/// The result of one run, in catalog order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub converted: Vec<Converted>,
}

/// One entry, fully planned before anything is written.
struct Plan {
    id: RegId,
    uid: String,
    name: String,
    file: String,
    page: bool,
    text: String,
    /// The 0-based lines `[start, end)` this entry occupies in the legacy file.
    section: (usize, usize),
}

fn refuse(line: usize, message: impl Into<String>) -> CapabilityCatalogError {
    CapabilityCatalogError {
        file: LEGACY_PATH.into(),
        line,
        message: message.into(),
    }
}

fn bare(line: &str) -> &str {
    line.trim_end_matches(['\n', '\r'])
}

/// The entry's section: its heading line, the fence's closing line and the
/// end of the prose that follows the fence. Refuses when the owner of any of
/// the text is not clear.
fn section(
    lines: &[&str],
    headings: &[(usize, pulldown_cmark::HeadingLevel)],
    entry: &CapabilityEntry,
    previous_end_fence: usize,
    next_fence: Option<usize>,
) -> Result<(usize, usize, usize), CapabilityCatalogError> {
    let fence = entry.line - 1;
    let id = &entry.id;
    let Some(&(heading, level)) = headings
        .iter()
        .rev()
        .find(|(line, _)| *line < fence && *line >= previous_end_fence)
    else {
        return Err(refuse(
            entry.line,
            format!("ambiguous prose span for {id}: no heading of its own above its fence"),
        ));
    };
    if lines[heading + 1..fence]
        .iter()
        .any(|line| !bare(line).trim().is_empty())
    {
        return Err(refuse(
            heading + 1,
            format!("ambiguous prose span for {id}: text between its heading and its fence belongs to no field"),
        ));
    }
    let marker = if lines[fence].trim_start().starts_with('~') {
        "~~~"
    } else {
        "```"
    };
    let Some(close) = lines[fence + 1..]
        .iter()
        .position(|line| bare(line).trim_matches([' ', '\t']) == marker)
    else {
        return Err(refuse(
            entry.line,
            format!("{id}: the yaml fence is not closed"),
        ));
    };
    let end_fence = fence + close + 2;
    let mut end = headings
        .iter()
        .find(|(line, depth)| *line >= end_fence && *depth <= level)
        .map_or(lines.len(), |(line, _)| *line);
    if let Some(next) = next_fence {
        // The next entry's own heading closes this span even when it is deeper.
        let next_heading = headings
            .iter()
            .rev()
            .find(|(line, _)| *line < next && *line >= end_fence)
            .map(|(line, _)| *line);
        match next_heading {
            Some(line) => end = end.min(line),
            None if next < end => {
                return Err(refuse(
                    next + 1,
                    format!("ambiguous prose span for {id}: another capability fence follows in its prose without a heading of its own"),
                ));
            }
            None => {}
        }
    }
    Ok((heading, end_fence, end))
}

/// The fence's own lines as frontmatter, with the minted uid after `id:`.
fn frontmatter(
    lines: &[&str],
    fence: usize,
    end_fence: usize,
    entry: &CapabilityEntry,
    uid: &str,
) -> Result<(String, serde_yaml::Mapping), CapabilityCatalogError> {
    let body = &lines[fence + 1..end_fence - 1];
    let yaml: String = body.concat();
    let mapping: serde_yaml::Mapping = serde_yaml::from_str(&yaml).map_err(|error| {
        refuse(
            entry.line,
            format!("{}: fence is not a YAML mapping: {error}", entry.id),
        )
    })?;
    for key in mapping.keys() {
        let Some(key) = key.as_str() else {
            return Err(refuse(
                entry.line,
                format!("{}: fence has a non-string key", entry.id),
            ));
        };
        if key == "uid" {
            return Err(refuse(
                entry.line,
                format!("{}: fence already carries a uid; migrate registers a new one, so this fence is refused", entry.id),
            ));
        }
        if !RECORD_KEYS.contains(&key) {
            return Err(refuse(
                entry.line,
                format!("{}: fence key `{key}` is not a record field and would be lost; move it into the prose or remove it first", entry.id),
            ));
        }
    }
    let mut out = String::new();
    let mut inserted = false;
    for line in body {
        out.push_str(line);
        if !line.ends_with('\n') {
            out.push('\n');
        }
        if !inserted && bare(line).starts_with("id:") {
            out.push_str("uid: ");
            out.push_str(uid);
            out.push('\n');
            inserted = true;
        }
    }
    if !inserted {
        return Err(refuse(
            entry.line,
            format!(
                "{}: fence has no top-level `id:` line to place the uid after",
                entry.id
            ),
        ));
    }
    Ok((out, mapping))
}

/// The first sentence of the first plain paragraph of the prose, else `name`.
fn summary(prose: &str, name: &str) -> String {
    let paragraph: Vec<&str> = prose
        .lines()
        .skip_while(|line| line.trim().is_empty())
        .take_while(|line| !line.trim().is_empty())
        .collect();
    let plain = paragraph.first().is_some_and(|first| {
        let first = first.trim_start();
        !(first.starts_with('#')
            || first.starts_with('|')
            || first.starts_with('>')
            || first.starts_with('<')
            || first.starts_with("```")
            || first.starts_with("~~~")
            || first.starts_with("- ")
            || first.starts_with("* ")
            || first.starts_with("+ ")
            || first
                .split_once(". ")
                .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())))
    });
    if !plain {
        return name.to_string();
    }
    let text = paragraph
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ");
    let sentence = text
        .find(". ")
        .map_or(text.as_str(), |end| &text[..=end])
        .trim();
    let summary: String = sentence.chars().take(CAPABILITY_MAX_VALUE_CHARS).collect();
    if summary.is_empty() {
        name.to_string()
    } else {
        summary
    }
}

#[derive(Serialize)]
struct Defaults<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audience: Option<[&'a str; 1]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stage: Option<&'a str>,
}

/// The page that already holds this capability's detail, if any. Two pages,
/// a page under another name, or a page that is already a record refuse.
fn page(
    tree: &crate::contained::Tree,
    pages: &[String],
    entry: &CapabilityEntry,
    file: &str,
) -> Result<Option<String>, CapabilityCatalogError> {
    let prefix = format!("{}-", entry.id);
    let own: Vec<&String> = pages
        .iter()
        .filter(|name| name.starts_with(&prefix))
        .collect();
    let Some(first) = own.first() else {
        return Ok(None);
    };
    let path = format!("{CATALOG_DIR}/{first}");
    if own.len() > 1 || path != file {
        let names: Vec<&str> = own.iter().map(|name| name.as_str()).collect();
        return Err(refuse(
            entry.line,
            format!(
                "ambiguous page for {}: found {} under {CATALOG_DIR}/, expected only {file}",
                entry.id,
                names.join(", ")
            ),
        ));
    }
    let bytes = tree
        .read(&path, CAPABILITY_MAX_FILE_BYTES as u64)
        .map_err(|error| catalog_error(&path, error.to_string()))?;
    let text = String::from_utf8(bytes)
        .map_err(|error| catalog_error(&path, format!("invalid UTF-8: {error}")))?;
    if text.lines().next().is_some_and(super::frontmatter_fence) {
        return Err(catalog_error(
            &path,
            format!(
                "{} is already a record; rerunning a migration is not supported by this command",
                entry.id
            ),
        ));
    }
    Ok(Some(text))
}

/// What every entry's plan reads, loaded once.
struct Inputs<'a> {
    tree: &'a crate::contained::Tree,
    lines: Vec<&'a str>,
    headings: Vec<(usize, pulldown_cmark::HeadingLevel)>,
    pages: Vec<String>,
    areas: Vec<String>,
    vocabulary: Option<super::CapabilityVocabulary>,
}

fn plan_entry(
    inputs: &Inputs<'_>,
    entry: &CapabilityEntry,
    previous_end_fence: usize,
    next_fence: Option<usize>,
) -> Result<(Plan, usize), CapabilityCatalogError> {
    use std::fmt::Write as _;
    let (heading, end_fence, end) = section(
        &inputs.lines,
        &inputs.headings,
        entry,
        previous_end_fence,
        next_fence,
    )?;
    let id = RegId::parse(&entry.id)
        .filter(|id| id.kind() == Kind::Cap && super::numeric_capability_id(&entry.id))
        .ok_or_else(|| {
            refuse(
                entry.line,
                format!("{}: expected CAP- and at least three digits", entry.id),
            )
        })?;
    let file = format!("{CATALOG_DIR}/{}-{}.md", entry.id, entry.name);
    validate_capability_path(&file).map_err(|error| refuse(entry.line, error.message))?;
    let uid = crate::ids::new_uid();
    let (mut yaml, fields) = frontmatter(&inputs.lines, entry.line - 1, end_fence, entry, &uid)?;
    let prose: String = inputs.lines[end_fence..end].concat();
    let has = |key: &str| fields.contains_key(key);
    let summary_text = summary(&prose, &entry.name);
    let defaults = Defaults {
        summary: (!has("summary")).then_some(summary_text.as_str()),
        kind: (!has("kind")).then_some(DEFAULT_KIND),
        audience: (!has("audience")).then_some([DEFAULT_AUDIENCE]),
        stage: (!has("stage") && entry.status == "shipped").then_some(DEFAULT_STAGE),
    };
    let extra =
        serde_yaml::to_string(&defaults).map_err(|error| refuse(entry.line, error.to_string()))?;
    if extra.trim() != "{}" {
        yaml.push_str(&extra);
    }
    let page = page(inputs.tree, &inputs.pages, entry, &file)?;
    let mut text = format!("---\n{yaml}---\n");
    if let Some(page) = &page {
        text.push_str(page);
        if !page.ends_with('\n') {
            text.push('\n');
        }
    } else {
        if inputs.tree.metadata(&file).is_ok() {
            return Err(catalog_error(&file, "already exists"));
        }
        let _ = write!(text, "\n# {}: {}\n", entry.id, entry.name);
    }
    text.push_str(&prose);
    let record = parse_record(&file, &text)?;
    validate_record(&record, &inputs.areas, inputs.vocabulary.as_ref())?;
    let plan = Plan {
        id,
        uid,
        name: entry.name.clone(),
        file,
        page: page.is_some(),
        text,
        section: (heading, end),
    };
    Ok((plan, end_fence))
}

fn read_text(
    tree: &crate::contained::Tree,
    path: &str,
    missing: &str,
) -> Result<String, CapabilityCatalogError> {
    let bytes = tree
        .read(path, CAPABILITY_MAX_FILE_BYTES as u64)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                catalog_error(path, missing)
            } else {
                catalog_error(path, error.to_string())
            }
        })?;
    String::from_utf8(bytes).map_err(|error| catalog_error(path, format!("invalid UTF-8: {error}")))
}

fn plan(
    root: &Path,
    tree: &crate::contained::Tree,
) -> Result<(String, Vec<Plan>), CapabilityCatalogError> {
    let catalog = super::load_capability_catalog(root)?;
    let content = read_text(tree, LEGACY_PATH, "no legacy catalog to migrate")?;
    let (entries, issues) = parse_capabilities(&content);
    if let Some(issue) = issues.into_iter().next() {
        return Err(refuse(issue.line, issue.message));
    }
    if entries.is_empty() {
        return Err(catalog_error(
            LEGACY_PATH,
            "no capability fences to migrate",
        ));
    }
    let project = read_text(tree, PROJECT_PATH, "project configuration is required")?;
    let areas = toml::from_str::<AreaConfig>(&project)
        .map_err(|error| catalog_error(PROJECT_PATH, format!("areas: {error}")))?
        .areas;
    let pages: Vec<String> = match tree.entries(CATALOG_DIR, super::CAPABILITY_MAX_TREE_FILES) {
        Ok(names) => names
            .into_iter()
            .filter_map(|name| name.into_string().ok())
            .filter(|name| {
                name.starts_with("CAP-")
                    && Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            })
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(catalog_error(CATALOG_DIR, error.to_string())),
    };
    let inputs = Inputs {
        tree,
        lines: content.split_inclusive('\n').collect(),
        headings: top_level_headings(&content),
        pages,
        areas,
        vocabulary: catalog.vocabulary,
    };
    let mut plans = Vec::new();
    let mut previous_end_fence = 0;
    for (index, entry) in entries.iter().enumerate() {
        let next_fence = entries.get(index + 1).map(|next| next.line - 1);
        let (plan, end_fence) = plan_entry(&inputs, entry, previous_end_fence, next_fence)?;
        previous_end_fence = end_fence;
        plans.push(plan);
    }
    let git = crate::ids::Git::new(root);
    let ledgers = [
        Ledger::read(&git, crate::ids::REGISTRY_REF),
        Ledger::read(&git, &crate::ids::tracking_ref(crate::ids::AUTHORITY)),
    ];
    for ledger in ledgers {
        let ledger = ledger.map_err(|error| catalog_error("<registry>", error.to_string()))?;
        if let Some(held) = plans.iter().find(|plan| ledger.holds(&plan.id)) {
            return Err(catalog_error(
                "<registry>",
                format!("{} is already registered; rerunning or repairing a migration is not supported by this command", held.id),
            ));
        }
    }
    drop(inputs);
    Ok((content, plans))
}

/// The legacy file without the converted sections.
fn stripped(content: &str, plans: &[Plan]) -> String {
    content
        .split_inclusive('\n')
        .enumerate()
        .filter(|(line, _)| {
            !plans
                .iter()
                .any(|plan| (plan.section.0..plan.section.1).contains(line))
        })
        .map(|(_, text)| text)
        .collect()
}

/// Convert every legacy fence entry into a record.
///
/// Registers each legacy number with a minted `uid` in the local registry
/// (pending until the next push or `ids sync` when an authority exists),
/// writes one record per entry (an existing page gains frontmatter and keeps
/// its text), checks each written record against its binding, then removes
/// the converted sections from `docs/capabilities.md` in one last write.
///
/// # Errors
/// Refuses an ambiguous prose span, an invalid or already migrated catalog
/// and a registered number before anything is written; reports registry and
/// write failures with what was already done.
pub fn migrate(root: &Path) -> Result<Migration, CapabilityCatalogError> {
    let tree = crate::contained::Tree::open(root)
        .map_err(|error| catalog_error("<checkout>", error.to_string()))?;
    let (content, plans) = plan(root, &tree)?;
    let mut converted = Vec::new();
    for plan in &plans {
        let standing = issue::reserve_exact_offline(root, &plan.id, &plan.uid, &plan.name)
            .map_err(|error| {
                catalog_error(
                    "<registry>",
                    format!(
                        "registering {}: {error}; {} of {} numbers registered, no record written",
                        plan.id,
                        converted.len(),
                        plans.len()
                    ),
                )
            })?
            .ok_or_else(|| {
                catalog_error(
                    "<registry>",
                    format!("{}: its new uid is already bound", plan.id),
                )
            })?;
        converted.push(Converted {
            id: plan.id.to_string(),
            uid: plan.uid.clone(),
            file: plan.file.clone(),
            page: plan.page,
            standing,
        });
    }
    for (written, plan) in plans.iter().enumerate() {
        let result = if plan.page {
            tree.write(&plan.file, plan.text.as_bytes())
        } else {
            tree.create_new(&plan.file, plan.text.as_bytes())
        };
        result.map_err(|error| {
            catalog_error(
                &plan.file,
                format!(
                    "{error}; {written} of {} records written, fences left in {LEGACY_PATH}",
                    plans.len()
                ),
            )
        })?;
    }
    let git = crate::ids::Git::new(root);
    let ledger = Ledger::read(&git, crate::ids::REGISTRY_REF)
        .map_err(|error| catalog_error("<registry>", error.to_string()))?;
    for plan in &plans {
        let bytes = tree
            .read(&plan.file, CAPABILITY_MAX_FILE_BYTES as u64)
            .map_err(|error| catalog_error(&plan.file, error.to_string()))?;
        let record = parse_record(&plan.file, &String::from_utf8_lossy(&bytes))?;
        let bound = ledger.entry(&plan.id).map(|entry| entry.uid.as_str());
        if bytes != plan.text.as_bytes()
            || record.uid.as_deref() != Some(plan.uid.as_str())
            || bound != Some(plan.uid.as_str())
        {
            return Err(catalog_error(
                &plan.file,
                format!(
                    "{} does not match its registry binding; fences left in {LEGACY_PATH}",
                    plan.id
                ),
            ));
        }
    }
    tree.write(LEGACY_PATH, stripped(&content, &plans).as_bytes())
        .map_err(|error| {
            catalog_error(
                LEGACY_PATH,
                format!("{error}; every record was written but the fences remain"),
            )
        })?;
    Ok(Migration { converted })
}

#[cfg(test)]
mod tests {
    use super::summary;

    #[test]
    fn summary_takes_the_first_sentence_of_a_plain_paragraph() {
        assert_eq!(
            summary("\nRuns the gate. Then more.\n\nNext.\n", "gate"),
            "Runs the gate."
        );
        assert_eq!(
            summary("\n`codeflow x` lays\nit out (ADR-1).\n", "x"),
            "`codeflow x` lays it out (ADR-1)."
        );
    }

    #[test]
    fn summary_falls_back_to_the_name_for_lists_tables_and_headings() {
        for prose in ["\n- item\n", "\n| a |\n", "\n### Sub\n", "\n1. step\n", ""] {
            assert_eq!(summary(prose, "name"), "name", "{prose:?}");
        }
    }
}
