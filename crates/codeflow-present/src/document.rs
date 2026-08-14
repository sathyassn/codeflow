use std::collections::HashSet;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use pulldown_cmark::{Event, Options, Parser};
use serde::{Deserialize, Serialize};

use crate::{
    error::{PresentError, Result},
    limits,
    media::matches_declared_media,
    safe_html::validate_sandbox_html,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PresentationDocument {
    pub schema_version: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub provenance: Provenance,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adr_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Block {
    Narrative {
        id: String,
        markdown: String,
    },
    Bullets {
        id: String,
        #[serde(default)]
        ordered: bool,
        items: Vec<String>,
    },
    Callout {
        id: String,
        tone: CalloutTone,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        markdown: String,
    },
    Comparison {
        id: String,
        columns: Vec<ComparisonColumn>,
    },
    Decision {
        id: String,
        title: String,
        status: DecisionStatus,
        markdown: String,
    },
    Table {
        id: String,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Status {
        id: String,
        items: Vec<StatusItem>,
    },
    Code {
        id: String,
        language: String,
        code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
    },
    Diff {
        id: String,
        diff: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
    },
    Tree {
        id: String,
        label: String,
        nodes: Vec<TreeNode>,
    },
    Diagram {
        id: String,
        kind: DiagramKind,
        source: String,
        acc_title: String,
        acc_description: String,
    },
    Media {
        id: String,
        mime_type: MediaMime,
        data_base64: String,
        alt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
    },
    Disclosure {
        id: String,
        summary: String,
        blocks: Vec<Block>,
    },
    Tabs {
        id: String,
        tabs: Vec<Tab>,
    },
    FeedbackPrompt {
        id: String,
        prompt: String,
    },
    Html {
        id: String,
        html: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalloutTone {
    Note,
    Info,
    Success,
    Warning,
    Danger,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Proposed,
    Accepted,
    Rejected,
    Open,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagramKind {
    Flowchart,
    Sequence,
    Timeline,
    State,
    Class,
    EntityRelationship,
    Mindmap,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MediaMime {
    #[serde(rename = "image/png")]
    ImagePng,
    #[serde(rename = "image/jpeg")]
    ImageJpeg,
    #[serde(rename = "image/gif")]
    ImageGif,
    #[serde(rename = "image/webp")]
    ImageWebp,
    #[serde(rename = "video/mp4")]
    VideoMp4,
    #[serde(rename = "video/webm")]
    VideoWebm,
    #[serde(rename = "audio/mpeg")]
    AudioMpeg,
    #[serde(rename = "audio/ogg")]
    AudioOgg,
}

impl MediaMime {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ImagePng => "image/png",
            Self::ImageJpeg => "image/jpeg",
            Self::ImageGif => "image/gif",
            Self::ImageWebp => "image/webp",
            Self::VideoMp4 => "video/mp4",
            Self::VideoWebm => "video/webm",
            Self::AudioMpeg => "audio/mpeg",
            Self::AudioOgg => "audio/ogg",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComparisonColumn {
    pub title: String,
    pub markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StatusItem {
    pub label: String,
    pub state: EvidenceState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    Pass,
    Fail,
    Pending,
    NotRun,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TreeNode {
    pub label: String,
    #[serde(default)]
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Tab {
    pub label: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedDocument {
    Supported(PresentationDocument),
    Unsupported { schema_version: u32, raw: String },
}

pub fn parse_document(bytes: &[u8]) -> Result<ParsedDocument> {
    if bytes.len() > limits::MAX_DOCUMENT_BYTES {
        return Err(PresentError::DocumentTooLarge {
            limit: limits::MAX_DOCUMENT_BYTES,
        });
    }
    let raw = std::str::from_utf8(bytes)
        .map_err(|_| PresentError::InvalidDocument("input must be UTF-8 JSON".to_string()))?;
    let probe: serde_json::Value = serde_json::from_str(raw)?;
    let version = probe
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            PresentError::InvalidDocument(
                "schema_version must be a non-negative 32-bit integer".to_string(),
            )
        })?;
    if version > limits::SCHEMA_VERSION {
        return Ok(ParsedDocument::Unsupported {
            schema_version: version,
            raw: raw.to_string(),
        });
    }
    if version != limits::SCHEMA_VERSION {
        return Err(PresentError::UnsupportedSchema {
            found: version,
            supported: limits::SCHEMA_VERSION,
        });
    }

    let document: PresentationDocument = serde_json::from_value(probe)?;
    document.validate()?;
    Ok(ParsedDocument::Supported(document))
}

impl PresentationDocument {
    pub fn validate(&self) -> Result<()> {
        require_nonempty_bounded("title", &self.title, limits::MAX_TITLE_BYTES)?;
        if let Some(language) = &self.language {
            validate_language(language)?;
        }
        validate_provenance(&self.provenance)?;
        if self.blocks.is_empty() {
            return Err(invalid("blocks must not be empty"));
        }
        let mut ids = HashSet::new();
        let mut count = 0;
        let mut diagram_count = 0;
        let mut collection_items = 0;
        validate_blocks(
            &self.blocks,
            1,
            &mut count,
            &mut diagram_count,
            &mut collection_items,
            &mut ids,
        )
    }
}

impl Block {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Narrative { id, .. }
            | Self::Bullets { id, .. }
            | Self::Callout { id, .. }
            | Self::Comparison { id, .. }
            | Self::Decision { id, .. }
            | Self::Table { id, .. }
            | Self::Status { id, .. }
            | Self::Code { id, .. }
            | Self::Diff { id, .. }
            | Self::Tree { id, .. }
            | Self::Diagram { id, .. }
            | Self::Media { id, .. }
            | Self::Disclosure { id, .. }
            | Self::Tabs { id, .. }
            | Self::FeedbackPrompt { id, .. }
            | Self::Html { id, .. } => id,
        }
    }

    #[must_use]
    pub fn canonical_review_text(&self) -> String {
        match self {
            Self::Narrative { markdown, .. } => markdown_text(markdown),
            Self::Callout {
                title, markdown, ..
            } => format!(
                "{}{}",
                title.as_deref().unwrap_or_default(),
                markdown_text(markdown)
            ),
            Self::Decision {
                title,
                status,
                markdown,
                ..
            } => format!("{title}{status:?}{}", markdown_text(markdown)),
            Self::Bullets { items, .. } => items.iter().map(|item| markdown_text(item)).collect(),
            Self::Comparison { columns, .. } => {
                let mut output = String::new();
                for column in columns {
                    output.push_str(&column.title);
                    output.push_str(&markdown_text(&column.markdown));
                }
                output
            }
            Self::Table { columns, rows, .. } => columns
                .iter()
                .cloned()
                .chain(rows.iter().flatten().map(|cell| markdown_text(cell)))
                .collect(),
            Self::Status { items, .. } => {
                let mut output = String::new();
                for item in items {
                    output.push_str(&item.label);
                    output.push_str(item.detail.as_deref().unwrap_or_default());
                }
                output
            }
            Self::Code { code, caption, .. } => {
                format!("{}{}", caption.as_deref().unwrap_or_default(), code)
            }
            Self::Diff { diff, caption, .. } => {
                let mut output = caption.clone().unwrap_or_default();
                for line in diff.lines() {
                    if line.starts_with('+') && !line.starts_with("+++") {
                        output.push_str("Added: ");
                    } else if line.starts_with('-') && !line.starts_with("---") {
                        output.push_str("Removed: ");
                    }
                    output.push_str(line);
                    output.push('\n');
                }
                output
            }
            Self::Tree { label, nodes, .. } => {
                let mut output = label.clone();
                append_tree_text(nodes, &mut output, false);
                output
            }
            Self::Diagram {
                acc_title,
                acc_description,
                source,
                ..
            } => format!("{source}{acc_title} — {acc_description}"),
            Self::Media { caption, .. } => caption.clone().unwrap_or_default(),
            Self::Disclosure { summary, .. } => summary.clone(),
            Self::Tabs { tabs, .. } => tabs.iter().map(|tab| tab.label.as_str()).collect(),
            Self::FeedbackPrompt { prompt, .. } => prompt.clone(),
            Self::Html { title, .. } => title.clone().unwrap_or_default(),
        }
    }

    /// Short, scannable label for TOC / feedback notes. Not the full prose of
    /// long prompts or captions — those remain in the block body.
    #[must_use]
    pub fn review_label(&self) -> String {
        let raw = match self {
            Self::Callout {
                title: Some(title), ..
            }
            | Self::Code {
                caption: Some(title),
                ..
            }
            | Self::Diff {
                caption: Some(title),
                ..
            }
            | Self::Html {
                title: Some(title), ..
            }
            | Self::Decision { title, .. }
            | Self::Tree { label: title, .. } => title.clone(),
            Self::Diagram { acc_title, .. } => acc_title.clone(),
            Self::Media { alt, .. } => alt.clone(),
            Self::Disclosure { summary, .. } => summary.clone(),
            // Full prompt is rendered in the block body; never dump it into the nav.
            Self::FeedbackPrompt { .. } => "Feedback request".to_string(),
            _ => self.id().to_string(),
        };
        truncate_nav_label(&raw, 40)
    }
}

/// Nav / TOC labels stay one short line so long titles cannot collapse the rail.
fn truncate_nav_label(label: &str, max_chars: usize) -> String {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return "Section".to_string();
    }
    let count = trimmed.chars().count();
    if count <= max_chars {
        return trimmed.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let mut out: String = trimmed.chars().take(keep).collect();
    // Avoid orphan spaces before the ellipsis.
    while out.ends_with(char::is_whitespace) {
        out.pop();
    }
    out.push('…');
    out
}

fn validate_blocks(
    blocks: &[Block],
    depth: usize,
    count: &mut usize,
    diagram_count: &mut usize,
    collection_items: &mut usize,
    ids: &mut HashSet<String>,
) -> Result<()> {
    if depth > limits::MAX_NESTING {
        return Err(invalid(format!(
            "block nesting exceeds {}",
            limits::MAX_NESTING
        )));
    }
    for block in blocks {
        *count += 1;
        if *count > limits::MAX_BLOCKS {
            return Err(invalid(format!(
                "block count exceeds {}",
                limits::MAX_BLOCKS
            )));
        }
        if matches!(block, Block::Diagram { .. }) {
            *diagram_count += 1;
            if *diagram_count > limits::MAX_DIAGRAM_BLOCKS {
                return Err(invalid(format!(
                    "diagram count exceeds {}",
                    limits::MAX_DIAGRAM_BLOCKS
                )));
            }
        }
        validate_id(block.id())?;
        if !ids.insert(block.id().to_string()) {
            return Err(invalid(format!("duplicate block id {}", block.id())));
        }
        validate_block(block, depth, count, diagram_count, collection_items, ids)?;
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn validate_block(
    block: &Block,
    depth: usize,
    count: &mut usize,
    diagram_count: &mut usize,
    collection_items: &mut usize,
    ids: &mut HashSet<String>,
) -> Result<()> {
    match block {
        Block::Narrative { markdown, .. } => bounded("markdown", markdown, limits::MAX_PROSE_BYTES),
        Block::Bullets { items, .. } => {
            validate_collection_count("bullets", items.len(), collection_items)?;
            for item in items {
                bounded("bullet", item, limits::MAX_PROSE_BYTES)?;
            }
            Ok(())
        }
        Block::Callout {
            title, markdown, ..
        } => {
            optional_bounded("callout title", title.as_deref(), limits::MAX_TITLE_BYTES)?;
            bounded("callout markdown", markdown, limits::MAX_PROSE_BYTES)
        }
        Block::Comparison { columns, .. } => {
            if !(2..=4).contains(&columns.len()) {
                return Err(invalid("comparison must contain two to four columns"));
            }
            add_collection_items(columns.len(), collection_items)?;
            for column in columns {
                require_nonempty_bounded(
                    "comparison title",
                    &column.title,
                    limits::MAX_TITLE_BYTES,
                )?;
                bounded(
                    "comparison markdown",
                    &column.markdown,
                    limits::MAX_PROSE_BYTES,
                )?;
            }
            Ok(())
        }
        Block::Decision {
            title, markdown, ..
        } => {
            require_nonempty_bounded("decision title", title, limits::MAX_TITLE_BYTES)?;
            bounded("decision markdown", markdown, limits::MAX_PROSE_BYTES)
        }
        Block::Table { columns, rows, .. } => {
            if columns.is_empty() || columns.len() > limits::MAX_TABLE_COLUMNS {
                return Err(invalid("table column count is outside the allowed range"));
            }
            if rows.len() > limits::MAX_TABLE_ROWS {
                return Err(invalid("table has too many rows"));
            }
            let cells = rows
                .len()
                .checked_mul(columns.len())
                .and_then(|count| count.checked_add(columns.len()))
                .ok_or_else(|| invalid("table item count overflow"))?;
            add_collection_items(cells, collection_items)?;
            for column in columns {
                require_nonempty_bounded("table column", column, limits::MAX_TITLE_BYTES)?;
            }
            for row in rows {
                if row.len() != columns.len() {
                    return Err(invalid("every table row must match the column count"));
                }
                for cell in row {
                    bounded("table cell", cell, limits::MAX_PROSE_BYTES)?;
                }
            }
            Ok(())
        }
        Block::Status { items, .. } => {
            validate_collection_count("status", items.len(), collection_items)?;
            for item in items {
                require_nonempty_bounded("status label", &item.label, limits::MAX_LABEL_BYTES)?;
                optional_bounded(
                    "status detail",
                    item.detail.as_deref(),
                    limits::MAX_PROSE_BYTES,
                )?;
            }
            Ok(())
        }
        Block::Code {
            language,
            code,
            caption,
            ..
        } => {
            require_nonempty_bounded("code language", language, 64)?;
            bounded("code", code, limits::MAX_CODE_BYTES)?;
            optional_bounded("code caption", caption.as_deref(), limits::MAX_TITLE_BYTES)
        }
        Block::Diff { diff, caption, .. } => {
            bounded("diff", diff, limits::MAX_CODE_BYTES)?;
            optional_bounded("diff caption", caption.as_deref(), limits::MAX_TITLE_BYTES)
        }
        Block::Tree { label, nodes, .. } => {
            require_nonempty_bounded("tree label", label, limits::MAX_TITLE_BYTES)?;
            if nodes.is_empty() {
                return Err(invalid("tree must contain at least one node"));
            }
            let mut tree_items = 0;
            validate_tree(nodes, depth + 1, &mut tree_items, collection_items)
        }
        Block::Diagram {
            source,
            acc_title,
            acc_description,
            ..
        } => {
            bounded("diagram source", source, limits::MAX_DIAGRAM_BYTES)?;
            require_nonempty_bounded(
                "diagram accessible title",
                acc_title,
                limits::MAX_TITLE_BYTES,
            )?;
            require_nonempty_bounded(
                "diagram accessible description",
                acc_description,
                limits::MAX_PROSE_BYTES,
            )
        }
        Block::Media {
            mime_type,
            data_base64,
            alt,
            caption,
            ..
        } => {
            require_nonempty_bounded("media alternative text", alt, limits::MAX_LABEL_BYTES)?;
            optional_bounded("media caption", caption.as_deref(), limits::MAX_PROSE_BYTES)?;
            let decoded = STANDARD
                .decode(data_base64)
                .map_err(|_| invalid("media data_base64 is not valid base64"))?;
            if decoded.len() > limits::MAX_MEDIA_BYTES {
                return Err(invalid("decoded media exceeds the allowed size"));
            }
            if !matches_declared_media(mime_type, &decoded) {
                return Err(invalid(
                    "media bytes do not match the declared closed MIME family",
                ));
            }
            Ok(())
        }
        Block::Disclosure {
            summary, blocks, ..
        } => {
            require_nonempty_bounded("disclosure summary", summary, limits::MAX_TITLE_BYTES)?;
            validate_blocks(
                blocks,
                depth + 1,
                count,
                diagram_count,
                collection_items,
                ids,
            )
        }
        Block::Tabs { tabs, .. } => {
            if tabs.is_empty() || tabs.len() > 12 {
                return Err(invalid("tabs must contain one to twelve entries"));
            }
            add_collection_items(tabs.len(), collection_items)?;
            for tab in tabs {
                require_nonempty_bounded("tab label", &tab.label, limits::MAX_TITLE_BYTES)?;
                validate_blocks(
                    &tab.blocks,
                    depth + 1,
                    count,
                    diagram_count,
                    collection_items,
                    ids,
                )?;
            }
            Ok(())
        }
        Block::FeedbackPrompt { prompt, .. } => {
            require_nonempty_bounded("feedback prompt", prompt, limits::MAX_LABEL_BYTES)
        }
        Block::Html { html, title, .. } => {
            bounded("sandboxed html", html, limits::MAX_HTML_BYTES)?;
            validate_sandbox_html(html)?;
            optional_bounded(
                "sandboxed html title",
                title.as_deref(),
                limits::MAX_TITLE_BYTES,
            )
        }
    }
}

fn validate_language(language: &str) -> Result<()> {
    require_nonempty_bounded("language", language, limits::MAX_LANGUAGE_BYTES)?;
    let mut parts = language.split('-');
    let Some(primary) = parts.next() else {
        return Err(invalid("language must be a compact BCP-47-like tag"));
    };
    if !(2..=8).contains(&primary.len()) || !primary.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return Err(invalid("language must be a compact BCP-47-like tag"));
    }
    if parts.any(|part| {
        part.is_empty() || part.len() > 8 || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
    }) {
        return Err(invalid("language must be a compact BCP-47-like tag"));
    }
    Ok(())
}

fn validate_collection_count(name: &str, items: usize, document_items: &mut usize) -> Result<()> {
    if items == 0 || items > limits::MAX_COLLECTION_ITEMS_PER_BLOCK {
        return Err(invalid(format!(
            "{name} item count is outside the allowed range"
        )));
    }
    add_collection_items(items, document_items)
}

fn add_collection_items(items: usize, document_items: &mut usize) -> Result<()> {
    *document_items = document_items
        .checked_add(items)
        .ok_or_else(|| invalid("document collection item count overflow"))?;
    if *document_items > limits::MAX_DOCUMENT_COLLECTION_ITEMS {
        return Err(invalid(format!(
            "document collection item count exceeds {}",
            limits::MAX_DOCUMENT_COLLECTION_ITEMS
        )));
    }
    Ok(())
}

fn validate_tree(
    nodes: &[TreeNode],
    depth: usize,
    tree_items: &mut usize,
    document_items: &mut usize,
) -> Result<()> {
    if depth > limits::MAX_NESTING {
        return Err(invalid("tree nesting exceeds the allowed depth"));
    }
    *tree_items = tree_items
        .checked_add(nodes.len())
        .ok_or_else(|| invalid("tree item count overflow"))?;
    if *tree_items > limits::MAX_COLLECTION_ITEMS_PER_BLOCK {
        return Err(invalid("tree has too many nodes"));
    }
    add_collection_items(nodes.len(), document_items)?;
    for node in nodes {
        require_nonempty_bounded("tree node", &node.label, limits::MAX_TITLE_BYTES)?;
        validate_tree(&node.children, depth + 1, tree_items, document_items)?;
    }
    Ok(())
}

fn append_tree_text(nodes: &[TreeNode], output: &mut String, separated: bool) {
    for node in nodes {
        if separated {
            output.push('\n');
        }
        output.push_str(&node.label);
        append_tree_text(&node.children, output, separated);
    }
}

fn markdown_text(markdown: &str) -> String {
    Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH)
        .filter_map(|event| match event {
            Event::Text(text) | Event::Code(text) => Some(text.into_string()),
            Event::SoftBreak | Event::HardBreak => Some("\n".to_string()),
            _ => None,
        })
        .collect()
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || !id.as_bytes()[0].is_ascii_alphanumeric()
    {
        return Err(invalid(format!("invalid block id {id:?}")));
    }
    Ok(())
}

fn validate_provenance(provenance: &Provenance) -> Result<()> {
    if provenance
        .task_id
        .as_deref()
        .is_some_and(|value| !valid_task_id(value))
    {
        return Err(invalid("provenance task_id must be TSK-NNN or TSK-NNN-NNN"));
    }
    for (name, value, prefix, digits) in [
        ("spec_id", provenance.spec_id.as_deref(), "SPC-", 3),
        ("adr_id", provenance.adr_id.as_deref(), "ADR-", 4),
    ] {
        if value.is_some_and(|value| !valid_fixed_id(value, prefix, digits)) {
            return Err(invalid(format!(
                "provenance {name} must use its canonical numeric ID"
            )));
        }
    }
    Ok(())
}

fn valid_task_id(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix("TSK-") else {
        return false;
    };
    let mut groups = suffix.split('-');
    let first = groups.next().is_some_and(valid_three_digits);
    let second = groups.next().is_none_or(valid_three_digits);
    first && second && groups.next().is_none()
}

fn valid_fixed_id(value: &str, prefix: &str, digits: usize) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == digits && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn valid_three_digits(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn require_nonempty_bounded(name: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{name} must not be empty")));
    }
    bounded(name, value, max)
}

fn optional_bounded(name: &str, value: Option<&str>, max: usize) -> Result<()> {
    value.map_or(Ok(()), |value| bounded(name, value, max))
}

fn bounded(name: &str, value: &str, max: usize) -> Result<()> {
    if value.len() > max {
        return Err(invalid(format!("{name} exceeds {max} bytes")));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> PresentError {
    PresentError::InvalidDocument(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(blocks: Vec<Block>) -> PresentationDocument {
        PresentationDocument {
            schema_version: 1,
            title: "Review".to_string(),
            language: None,
            provenance: Provenance::default(),
            blocks,
        }
    }

    #[test]
    fn rejects_duplicate_nested_block_ids() {
        let value = document(vec![
            Block::Narrative {
                id: "same".to_string(),
                markdown: "one".to_string(),
            },
            Block::Disclosure {
                id: "details".to_string(),
                summary: "More".to_string(),
                blocks: vec![Block::Narrative {
                    id: "same".to_string(),
                    markdown: "two".to_string(),
                }],
            },
        ]);
        assert!(value
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate"));
    }

    #[test]
    fn newer_schema_is_complete_raw_fallback() {
        let raw = br#"{"schema_version":2,"title":"Future","blocks":[],"new":true}"#;
        assert_eq!(
            parse_document(raw).unwrap(),
            ParsedDocument::Unsupported {
                schema_version: 2,
                raw: String::from_utf8(raw.to_vec()).unwrap()
            }
        );
    }

    #[test]
    fn rejects_paths_and_urls_as_media_shape() {
        let raw = br#"{
          "schema_version": 1,
          "title": "Unsafe",
          "blocks": [{"type":"media","id":"m","mime_type":"image/png","path":"/tmp/x","alt":"x"}]
        }"#;
        assert!(parse_document(raw).is_err());
    }

    #[test]
    fn utf16_selector_basis_preserves_emoji_units() {
        let text = "a😀b";
        assert_eq!(text.encode_utf16().count(), 4);
    }

    #[test]
    fn provenance_accepts_canonical_ids_and_rejects_lookalikes() {
        let mut value = document(vec![Block::Narrative {
            id: "intro".to_string(),
            markdown: "hello".to_string(),
        }]);
        value.provenance = Provenance {
            task_id: Some("TSK-002-001".to_string()),
            spec_id: Some("SPC-004".to_string()),
            adr_id: Some("ADR-0050".to_string()),
        };
        assert!(value.validate().is_ok());

        for invalid in ["TSK-2", "TSK-002-1", "tsk-002", "TSK-002-001-extra"] {
            value.provenance.task_id = Some(invalid.to_string());
            assert!(value.validate().is_err(), "accepted {invalid}");
        }
        value.provenance.task_id = None;
        value.provenance.spec_id = Some("SPC-4".to_string());
        assert!(value.validate().is_err());
        value.provenance.spec_id = None;
        value.provenance.adr_id = Some("ADR-50".to_string());
        assert!(value.validate().is_err());
    }

    #[test]
    fn diagram_sources_and_document_counts_are_bounded_before_rendering() {
        let diagram = |index: usize, source: String| Block::Diagram {
            id: format!("diagram-{index}"),
            kind: DiagramKind::Flowchart,
            source,
            acc_title: format!("Diagram {index}"),
            acc_description: "A bounded test diagram.".to_string(),
        };
        let oversized = document(vec![diagram(0, "x".repeat(limits::MAX_DIAGRAM_BYTES + 1))]);
        assert!(oversized.validate().is_err());

        let diagrams = (0..=limits::MAX_DIAGRAM_BLOCKS)
            .map(|index| diagram(index, "flowchart LR\nA-->B".to_string()))
            .collect();
        assert!(document(diagrams).validate().is_err());
    }

    #[test]
    fn collection_cardinality_is_bounded_per_block_and_across_the_document() {
        let oversized_bullets = document(vec![Block::Bullets {
            id: "bullets".to_string(),
            ordered: false,
            items: vec!["x".to_string(); limits::MAX_COLLECTION_ITEMS_PER_BLOCK + 1],
        }]);
        assert!(oversized_bullets.validate().is_err());

        let status_blocks = (0..9)
            .map(|index| Block::Status {
                id: format!("status-{index}"),
                items: (0..limits::MAX_COLLECTION_ITEMS_PER_BLOCK)
                    .map(|item| StatusItem {
                        label: format!("item-{item}"),
                        state: EvidenceState::Pending,
                        detail: None,
                    })
                    .collect(),
            })
            .collect();
        assert!(document(status_blocks).validate().is_err());

        let oversized_tree = document(vec![Block::Tree {
            id: "tree".to_string(),
            label: "Tree".to_string(),
            nodes: (0..=limits::MAX_COLLECTION_ITEMS_PER_BLOCK)
                .map(|index| TreeNode {
                    label: format!("node-{index}"),
                    children: Vec::new(),
                })
                .collect(),
        }]);
        assert!(oversized_tree.validate().is_err());
    }

    #[test]
    fn review_label_keeps_nav_scannable_for_long_feedback_and_titles() {
        let long_prompt = "Give the exact release decision: Ship current build (macOS/Linux only; no Windows claim), Hold until native Windows is qualified, or Request changes with anchored notes.";
        let feedback = Block::FeedbackPrompt {
            id: "verdict".to_string(),
            prompt: long_prompt.to_string(),
        };
        assert_eq!(feedback.review_label(), "Feedback request");
        assert!(feedback.review_label().chars().count() <= 40);

        let diagram = Block::Diagram {
            id: "fig".to_string(),
            kind: DiagramKind::Flowchart,
            source: "flowchart LR\n  A-->B".to_string(),
            acc_title: "Verified native paths versus open Windows gap and more words".to_string(),
            acc_description: "Long description for accessibility only.".to_string(),
        };
        let label = diagram.review_label();
        assert!(label.chars().count() <= 40, "{label}");
        assert!(label.ends_with('…'), "{label}");
        // Full prompt still available for body rendering via prompt field, not review_label.
        assert_ne!(feedback.review_label(), long_prompt);
    }

    #[test]
    fn language_media_alt_and_feedback_prompt_match_public_byte_boundaries() {
        let mut value = document(vec![Block::FeedbackPrompt {
            id: "prompt".to_string(),
            prompt: "x".repeat(limits::MAX_LABEL_BYTES),
        }]);
        value.language = Some("en-Latn-CA".to_string());
        assert!(value.validate().is_ok());

        let Block::FeedbackPrompt { prompt, .. } = &mut value.blocks[0] else {
            unreachable!()
        };
        prompt.push('x');
        assert!(value.validate().is_err());

        value.language =
            Some("en-abcdef12-abcdef12-abcdef12-abcdef12-abcdef12-abcdef12-abcdef12".to_string());
        assert!(value.validate().is_err());
        value.language = Some("e-US".to_string());
        assert!(value.validate().is_err());

        let media = document(vec![Block::Media {
            id: "media".to_string(),
            mime_type: MediaMime::ImagePng,
            data_base64: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".to_string(),
            alt: "x".repeat(limits::MAX_LABEL_BYTES + 1),
            caption: None,
        }]);
        assert!(media.validate().is_err());
    }
}
