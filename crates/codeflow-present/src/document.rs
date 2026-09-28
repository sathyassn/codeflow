use std::collections::{HashMap, HashSet};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use crate::{
    error::{PresentError, Result},
    limits,
    media::matches_declared_media,
    retired::refuse_retired_blocks,
    safe_html::{validate_sandbox_html, visible_text_from_html},
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
    /// Schema v2: a one-line summary `present list` shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
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
        /// Schema v2: required; the runtime renders "Table N · title".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Schema v2: optional.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
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
        /// Schema v2: the repository file the snippet was captured from
        /// (SPC-014 B10); `open` and `update` record its provenance.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<SnapshotSource>,
    },
    Diff {
        id: String,
        diff: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
        /// Schema v2, as on `code`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<SnapshotSource>,
    },
    Tree {
        id: String,
        label: String,
        nodes: Vec<TreeNode>,
    },
    /// A figure-grammar declaration (`figure-grammar.md`), drawn on the
    /// client by the grammar module the docs portal also runs.
    Figure {
        id: String,
        declaration: serde_json::Value,
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
        /// Schema v2: required; every v2 stage is a framed figure.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
        /// Schema v2: optional, 1 to 12 entries.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        legend: Option<Vec<LegendEntry>>,
        /// Schema v2: optional, shown in the Details disclosure.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotSource {
    /// A repository-relative path: no leading slash, no `..`, no backslash.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LegendEntry {
    pub label: String,
    pub means: String,
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
    if version > limits::MAX_DOCUMENT_SCHEMA_VERSION {
        return Ok(ParsedDocument::Unsupported {
            schema_version: version,
            raw: raw.to_string(),
        });
    }
    if version == 0 {
        return Err(PresentError::UnsupportedSchema {
            found: version,
            supported: limits::MAX_DOCUMENT_SCHEMA_VERSION,
        });
    }

    refuse_retired_blocks(&probe)?;
    let document: PresentationDocument = serde_json::from_value(probe)?;
    document.validate()?;
    Ok(ParsedDocument::Supported(document))
}

impl PresentationDocument {
    pub fn validate(&self) -> Result<()> {
        let version = self.schema_version;
        if !(1..=limits::MAX_DOCUMENT_SCHEMA_VERSION).contains(&version) {
            return Err(PresentError::UnsupportedSchema {
                found: version,
                supported: limits::MAX_DOCUMENT_SCHEMA_VERSION,
            });
        }
        require_nonempty_bounded("title", &self.title, limits::MAX_TITLE_BYTES)?;
        if let Some(language) = &self.language {
            validate_language(language)?;
        }
        validate_provenance(&self.provenance)?;
        match (&self.summary, version) {
            (None, _) => {}
            (Some(_), 1) => return Err(needs_v2("summary", "the document")),
            (Some(summary), _) => {
                let characters = summary.trim().chars().count();
                if characters == 0 || summary.chars().count() > limits::MAX_SUMMARY_CHARS {
                    return Err(invalid(format!(
                        "summary must be 1 to {} characters",
                        limits::MAX_SUMMARY_CHARS
                    )));
                }
            }
        }
        if self.blocks.is_empty() {
            return Err(invalid("blocks must not be empty"));
        }
        let mut ids = HashSet::new();
        let mut count = 0;
        let mut drawn_count = 0;
        let mut collection_items = 0;
        validate_blocks(
            &self.blocks,
            version,
            1,
            &mut count,
            &mut drawn_count,
            &mut collection_items,
            &mut ids,
        )?;
        if version >= 2 {
            validate_references(self)?;
        }
        Ok(())
    }

    /// Every block in document order, depth first through disclosures and
    /// tabs.
    #[must_use]
    pub fn walk(&self) -> Vec<&Block> {
        let mut output = Vec::new();
        walk_blocks(&self.blocks, &mut output);
        output
    }
}

fn walk_blocks<'a>(blocks: &'a [Block], output: &mut Vec<&'a Block>) {
    for block in blocks {
        output.push(block);
        match block {
            Block::Disclosure { blocks, .. } => walk_blocks(blocks, output),
            Block::Tabs { tabs, .. } => {
                for tab in tabs {
                    walk_blocks(&tab.blocks, output);
                }
            }
            _ => {}
        }
    }
}

/// What a framed block is numbered as (SPC-014 B5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Figure,
    Table,
}

impl FrameKind {
    #[must_use]
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Figure => "Figure",
            Self::Table => "Table",
        }
    }

    pub(crate) const fn reference_prefix(self) -> &'static str {
        match self {
            Self::Figure => "fig",
            Self::Table => "table",
        }
    }

    /// The frame kind a block is numbered as in a v2 document.
    #[must_use]
    pub const fn of(block: &Block) -> Option<Self> {
        match block {
            Block::Figure { .. } | Block::Html { .. } => Some(Self::Figure),
            Block::Table { .. } => Some(Self::Table),
            _ => None,
        }
    }
}

/// The document-level framing a v2 document gets: figure and table numbers
/// by a depth-first walk, and references resolved to them. A v1 document
/// has none, so its references stay literal text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Framing {
    enabled: bool,
    numbers: HashMap<String, (FrameKind, usize)>,
}

impl Framing {
    #[must_use]
    pub fn of(document: &PresentationDocument) -> Self {
        if document.schema_version < 2 {
            return Self::default();
        }
        let mut numbers = HashMap::new();
        let (mut figures, mut tables) = (0, 0);
        for block in document.walk() {
            let number = match FrameKind::of(block) {
                Some(FrameKind::Figure) => {
                    figures += 1;
                    (FrameKind::Figure, figures)
                }
                Some(FrameKind::Table) => {
                    tables += 1;
                    (FrameKind::Table, tables)
                }
                None => continue,
            };
            numbers.insert(block.id().to_string(), number);
        }
        Self {
            enabled: true,
            numbers,
        }
    }

    /// Whether this is a v2 document's framing (numbers and references).
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub fn number(&self, block_id: &str) -> Option<(FrameKind, usize)> {
        self.numbers.get(block_id).copied()
    }

    /// "Figure N" or "Table N" for a reference, when it resolves.
    #[must_use]
    pub fn reference_label(&self, kind: FrameKind, block_id: &str) -> Option<String> {
        self.number(block_id)
            .filter(|(numbered, _)| *numbered == kind)
            .map(|(kind, number)| format!("{} {number}", kind.noun()))
    }
}

/// A run of Markdown text split at `[fig:<id>]` and `[table:<id>]`
/// references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextSegment<'a> {
    Text(&'a str),
    Reference { kind: FrameKind, id: &'a str },
}

pub(crate) fn reference_segments(text: &str) -> Vec<TextSegment<'_>> {
    let mut segments = Vec::new();
    let mut literal_start = 0;
    let mut index = 0;
    while let Some(offset) = text[index..].find('[') {
        let open = index + offset;
        let found = [FrameKind::Figure, FrameKind::Table]
            .into_iter()
            .find_map(|kind| {
                let rest = text[open + 1..].strip_prefix(kind.reference_prefix())?;
                let rest = rest.strip_prefix(':')?;
                let close = rest.find(']')?;
                let id = &rest[..close];
                valid_block_id(id).then_some((kind, id))
            });
        match found {
            Some((kind, id)) => {
                if literal_start < open {
                    segments.push(TextSegment::Text(&text[literal_start..open]));
                }
                segments.push(TextSegment::Reference { kind, id });
                index = open + 1 + kind.reference_prefix().len() + 1 + id.len() + 1;
                literal_start = index;
            }
            None => index = open + 1,
        }
    }
    if literal_start < text.len() {
        segments.push(TextSegment::Text(&text[literal_start..]));
    }
    segments
}

/// Markdown events with adjacent text runs merged, so a reference the parser
/// split at its brackets reads as one run. Text inside a link is marked so
/// references there stay literal.
pub(crate) fn coalesced_markdown(markdown: &str) -> Vec<(Event<'_>, bool)> {
    let mut output: Vec<(Event<'_>, bool)> = Vec::new();
    let mut link_depth = 0_usize;
    for event in Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH) {
        match &event {
            Event::Start(Tag::Link { .. }) => link_depth += 1,
            Event::End(TagEnd::Link) => link_depth = link_depth.saturating_sub(1),
            _ => {}
        }
        let in_link = link_depth > 0;
        if let (Event::Text(next), Some((Event::Text(previous), previous_in_link))) =
            (&event, output.last_mut())
        {
            if *previous_in_link == in_link {
                *previous = CowStr::Boxed(format!("{previous}{next}").into_boxed_str());
                continue;
            }
        }
        output.push((event, in_link));
    }
    output
}

fn validate_references(document: &PresentationDocument) -> Result<()> {
    let kinds = document
        .walk()
        .into_iter()
        .map(|block| (block.id(), FrameKind::of(block)))
        .collect::<HashMap<_, _>>();
    for block in document.walk() {
        for markdown in block.markdown_fields() {
            for (event, in_link) in coalesced_markdown(markdown) {
                let Event::Text(text) = event else {
                    continue;
                };
                if in_link {
                    continue;
                }
                for segment in reference_segments(&text) {
                    let TextSegment::Reference { kind, id } = segment else {
                        continue;
                    };
                    let reference = format!("[{}:{id}]", kind.reference_prefix());
                    match kinds.get(id) {
                        None => {
                            return Err(invalid(format!(
                                "block {}: the reference {reference} names no block",
                                block.id()
                            )))
                        }
                        Some(found) if *found != Some(kind) => {
                            return Err(invalid(format!(
                            "block {}: the reference {reference} names a block that is not a {}",
                            block.id(),
                            kind.noun().to_lowercase()
                        )))
                        }
                        Some(_) => {}
                    }
                }
            }
        }
    }
    Ok(())
}

fn needs_v2(field: &str, owner: &str) -> PresentError {
    invalid(format!(
        "{field} on {owner} is a schema_version 2 field; set schema_version 2 or remove it"
    ))
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
            | Self::Figure { id, .. }
            | Self::Media { id, .. }
            | Self::Disclosure { id, .. }
            | Self::Tabs { id, .. }
            | Self::FeedbackPrompt { id, .. }
            | Self::Html { id, .. } => id,
        }
    }

    /// The Markdown fields of this block, in which v2 references resolve.
    #[must_use]
    pub fn markdown_fields(&self) -> Vec<&str> {
        match self {
            Self::Narrative { markdown, .. }
            | Self::Callout { markdown, .. }
            | Self::Decision { markdown, .. } => vec![markdown],
            Self::Bullets { items, .. } => items.iter().map(String::as_str).collect(),
            Self::Comparison { columns, .. } => columns
                .iter()
                .map(|column| column.markdown.as_str())
                .collect(),
            Self::Table { rows, .. } => rows.iter().flatten().map(String::as_str).collect(),
            _ => Vec::new(),
        }
    }

    /// The text a text note is anchored in. In a v2 document a reference
    /// reads as its rendered label ("Figure 1"), as the page shows it.
    #[must_use]
    pub fn canonical_review_text(&self, framing: &Framing) -> String {
        let markdown_text = |markdown: &str| markdown_text(markdown, framing);
        match self {
            Self::Narrative { markdown, .. } => markdown_text(markdown),
            Self::Callout {
                title, markdown, ..
            } => join_parts([title.clone().unwrap_or_default(), markdown_text(markdown)]),
            Self::Decision {
                title,
                status,
                markdown,
                ..
            } => join_parts([
                title.clone(),
                format!("{status:?}"),
                markdown_text(markdown),
            ]),
            Self::Bullets { items, .. } => join_parts(items.iter().map(|item| markdown_text(item))),
            Self::Comparison { columns, .. } => join_parts(
                columns
                    .iter()
                    .flat_map(|column| [column.title.clone(), markdown_text(&column.markdown)]),
            ),
            Self::Table { columns, rows, .. } => join_parts(
                columns
                    .iter()
                    .cloned()
                    .chain(rows.iter().flatten().map(|cell| markdown_text(cell))),
            ),
            Self::Status { items, .. } => join_parts(
                items
                    .iter()
                    .flat_map(|item| [item.label.clone(), item.detail.clone().unwrap_or_default()]),
            ),
            Self::Code { code, caption, .. } => {
                join_parts([caption.clone().unwrap_or_default(), code.clone()])
            }
            Self::Diff { diff, caption, .. } => {
                let mut output = caption.clone().unwrap_or_default();
                if !output.is_empty() {
                    output.push('\n');
                }
                // What the reader sees and selects: a changed line without its
                // marker, and without the screen-reader label the page gives it.
                for line in diff.lines() {
                    output.push_str(diff_line_parts(line).2);
                    output.push('\n');
                }
                output
            }
            Self::Tree { label, nodes, .. } => {
                let mut output = label.clone();
                append_tree_text(nodes, &mut output);
                output
            }
            Self::Figure { declaration, .. } => {
                let (title, caption) = figure_text(declaration);
                join_parts([title, caption])
            }
            Self::Media { caption, .. } => caption.clone().unwrap_or_default(),
            Self::Disclosure { summary, .. } => summary.clone(),
            Self::Tabs { tabs, .. } => join_parts(tabs.iter().map(|tab| tab.label.as_str())),
            Self::FeedbackPrompt { prompt, .. } => prompt.clone(),
            Self::Html { title, html, .. } => [
                title.as_deref().unwrap_or_default(),
                &visible_text_from_html(html),
            ]
            .into_iter()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        }
    }

    /// A diff's review text as it was before TSK-071, when each changed line
    /// began with its screen-reader label and marker ("Added: +"), with the
    /// current review-text offset of each of its UTF-16 offsets (one more
    /// than its length). A generated label and marker map to the start of
    /// their line's text; the diff's own text maps one to one. `None` for a
    /// block that is not a diff.
    #[must_use]
    pub fn legacy_diff_review_text(&self) -> Option<(String, Vec<usize>)> {
        let Self::Diff { diff, caption, .. } = self else {
            return None;
        };
        let mut text = String::new();
        let mut offsets = Vec::new();
        let mut current = 0;
        let mut push = |part: &str, generated: bool| {
            for _ in part.encode_utf16() {
                offsets.push(current);
                if !generated {
                    current += 1;
                }
            }
            text.push_str(part);
        };
        if let Some(caption) = caption.as_deref().filter(|caption| !caption.is_empty()) {
            push(caption, false);
            push("\n", false);
        }
        for line in diff.lines() {
            let (label, marker, rest) = diff_line_parts(line);
            push(label, true);
            push(marker, true);
            push(rest, false);
            push("\n", false);
        }
        offsets.push(current);
        Some((text, offsets))
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
            | Self::Table {
                title: Some(title), ..
            }
            | Self::Decision { title, .. }
            | Self::Tree { label: title, .. } => title.clone(),
            // No raw ids in the nav (QA defect 8): a name the reader can see.
            Self::Tabs { tabs, .. } => tabs
                .iter()
                .map(|tab| tab.label.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            Self::Bullets { items, .. } => items
                .first()
                .map_or_else(|| "List".to_string(), |item| prose_nav_label(item)),
            Self::Code { .. } => "Code".to_string(),
            Self::Diff { .. } => "Diff".to_string(),
            Self::Figure { declaration, .. } => figure_text(declaration).0.to_string(),
            Self::Media { alt, .. } => alt.clone(),
            Self::Disclosure { summary, .. } => summary.clone(),
            // Full prompt is rendered in the block body; never dump it into the nav.
            Self::FeedbackPrompt { .. } => "Feedback request".to_string(),
            Self::Status { .. } => "Evidence".to_string(),
            Self::Comparison { .. } => "Compare".to_string(),
            Self::Narrative { markdown, .. } | Self::Callout { markdown, .. } => {
                prose_nav_label(markdown)
            }
            _ => self.id().to_string(),
        };
        truncate_nav_label(&raw, 40)
    }
}

/// The first sentence of the rendered text: Markdown syntax gone, its
/// words (underscores in a name included) kept, line breaks as spaces.
fn prose_nav_label(markdown: &str) -> String {
    let text: String = Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH)
        .filter_map(|event| match event {
            Event::Text(text) | Event::Code(text) => Some(text.into_string()),
            // A heading or paragraph ends a sentence; a line break does not.
            Event::End(pulldown_cmark::TagEnd::Heading(_) | pulldown_cmark::TagEnd::Paragraph) => {
                Some(". ".to_string())
            }
            Event::SoftBreak | Event::HardBreak | Event::End(_) => Some(" ".to_string()),
            _ => None,
        })
        .collect();
    let mut collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    // A reference reads as its kind: the label has no numbering at hand.
    for (token, word) in [("[fig:", "Figure"), ("[table:", "Table")] {
        while let Some(start) = collapsed.find(token) {
            let end = collapsed[start..]
                .find(']')
                .map_or(collapsed.len(), |end| start + end + 1);
            collapsed.replace_range(start..end, word);
        }
    }
    let first = collapsed
        .split_inclusive(['.', '!', '?'])
        .next()
        .unwrap_or(collapsed.as_str())
        .trim()
        .trim_end_matches(['.', '!', '?']);
    if first.is_empty() {
        "Section".to_string()
    } else {
        first.to_string()
    }
}

/// Nav / TOC labels stay one short line so long titles cannot collapse the rail.
fn truncate_nav_label(label: &str, max_chars: usize) -> String {
    let collapsed = label.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.as_str();
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
    version: u32,
    depth: usize,
    count: &mut usize,
    drawn_count: &mut usize,
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
        if matches!(block, Block::Figure { .. }) {
            *drawn_count += 1;
            if *drawn_count > limits::MAX_FIGURE_BLOCKS {
                return Err(invalid(format!(
                    "figure count exceeds {}",
                    limits::MAX_FIGURE_BLOCKS
                )));
            }
        }
        validate_id(block.id())?;
        if !ids.insert(block.id().to_string()) {
            return Err(invalid(format!("duplicate block id {}", block.id())));
        }
        validate_block(
            block,
            version,
            depth,
            count,
            drawn_count,
            collection_items,
            ids,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn validate_block(
    block: &Block,
    version: u32,
    depth: usize,
    count: &mut usize,
    drawn_count: &mut usize,
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
        Block::Table {
            id,
            columns,
            rows,
            title,
            caption,
        } => {
            if version == 1 {
                if title.is_some() {
                    return Err(needs_v2("title", &format!("table block {id}")));
                }
                if caption.is_some() {
                    return Err(needs_v2("caption", &format!("table block {id}")));
                }
            } else {
                let Some(title) = title else {
                    return Err(invalid(format!(
                        "table block {id} needs a title: a schema_version 2 table is framed as \"Table N · title\""
                    )));
                };
                require_nonempty_bounded(
                    &format!("table block {id} title"),
                    title,
                    limits::MAX_TITLE_BYTES,
                )?;
                if let Some(caption) = caption {
                    require_nonempty_bounded(
                        &format!("table block {id} caption"),
                        caption,
                        limits::MAX_TITLE_BYTES,
                    )?;
                }
            }
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
            id,
            language,
            code,
            caption,
            source,
        } => {
            require_nonempty_bounded("code language", language, 64)?;
            bounded("code", code, limits::MAX_CODE_BYTES)?;
            validate_snapshot_source(id, source.as_ref(), version)?;
            optional_bounded("code caption", caption.as_deref(), limits::MAX_TITLE_BYTES)
        }
        Block::Diff {
            id,
            diff,
            caption,
            source,
        } => {
            bounded("diff", diff, limits::MAX_CODE_BYTES)?;
            validate_snapshot_source(id, source.as_ref(), version)?;
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
        Block::Figure { id, declaration } => {
            validate_figure_declaration(declaration)?;
            if version >= 2 {
                validate_figure_marks(id, declaration)?;
            }
            Ok(())
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
                version,
                depth + 1,
                count,
                drawn_count,
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
                    version,
                    depth + 1,
                    count,
                    drawn_count,
                    collection_items,
                    ids,
                )?;
            }
            Ok(())
        }
        Block::FeedbackPrompt { prompt, .. } => {
            require_nonempty_bounded("feedback prompt", prompt, limits::MAX_LABEL_BYTES)
        }
        Block::Html {
            id,
            html,
            title,
            caption,
            legend,
            description,
        } => {
            bounded("sandboxed html", html, limits::MAX_HTML_BYTES)?;
            if version == 1 {
                validate_sandbox_html(html, version)?;
                for (field, present) in [
                    ("caption", caption.is_some()),
                    ("legend", legend.is_some()),
                    ("description", description.is_some()),
                ] {
                    if present {
                        return Err(needs_v2(field, &format!("html block {id}")));
                    }
                }
                return optional_bounded(
                    "sandboxed html title",
                    title.as_deref(),
                    limits::MAX_TITLE_BYTES,
                );
            }
            validate_sandbox_html(html, version).map_err(|error| {
                invalid(format!("html block {id}: {}", document_message(error)))
            })?;
            validate_stage_framing(
                id,
                title.as_deref(),
                caption.as_deref(),
                legend.as_deref(),
                description.as_deref(),
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

fn append_tree_text(nodes: &[TreeNode], output: &mut String) {
    for node in nodes {
        output.push('\n');
        output.push_str(&node.label);
        append_tree_text(&node.children, output);
    }
}

/// A diff line as the page draws it: the screen-reader label ("Added: ",
/// "Removed: " or none), the marker the reader sees but never quotes ("+",
/// "-" or none) and the text that is the line's review text. File headers
/// ("+++", "---") and context lines have no marker.
#[must_use]
pub fn diff_line_parts(line: &str) -> (&'static str, &str, &str) {
    if line.starts_with('+') && !line.starts_with("+++") {
        ("Added: ", &line[..1], &line[1..])
    } else if line.starts_with('-') && !line.starts_with("---") {
        ("Removed: ", &line[..1], &line[1..])
    } else {
        ("", "", line)
    }
}

/// Joins a block's parts with one line break, so a quote or its context that
/// crosses two parts (two status items, a tree's nodes, two table cells) keeps
/// a separator (QA defect 8). Empty parts add none.
fn join_parts<S: AsRef<str>>(parts: impl IntoIterator<Item = S>) -> String {
    let mut output = String::new();
    for part in parts {
        let part = part.as_ref();
        if part.is_empty() {
            continue;
        }
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(part);
    }
    output
}

/// Ends a Markdown paragraph, heading or list item with one line break, so
/// adjacent blocks of text do not run together in the review text.
fn end_markdown_block(output: &mut String) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
}

fn markdown_text(markdown: &str, framing: &Framing) -> String {
    if !framing.enabled() {
        let mut output = String::new();
        for event in Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH) {
            match event {
                Event::Text(text) | Event::Code(text) => output.push_str(&text),
                Event::SoftBreak | Event::HardBreak => output.push('\n'),
                Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item) => {
                    end_markdown_block(&mut output);
                }
                _ => {}
            }
        }
        output.truncate(output.trim_end_matches('\n').len());
        return output;
    }
    let mut output = String::new();
    for (event, in_link) in coalesced_markdown(markdown) {
        match event {
            Event::Text(text) if !in_link => {
                for segment in reference_segments(&text) {
                    match segment {
                        TextSegment::Text(text) => output.push_str(text),
                        TextSegment::Reference { kind, id } => {
                            if let Some(label) = framing.reference_label(kind, id) {
                                output.push_str(&label);
                            } else {
                                output.push('[');
                                output.push_str(kind.reference_prefix());
                                output.push(':');
                                output.push_str(id);
                                output.push(']');
                            }
                        }
                    }
                }
            }
            Event::Text(text) | Event::Code(text) => output.push_str(&text),
            Event::SoftBreak | Event::HardBreak => output.push('\n'),
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item) => {
                end_markdown_block(&mut output);
            }
            _ => {}
        }
    }
    output.truncate(output.trim_end_matches('\n').len());
    output
}

/// The message of a document error without its "invalid presentation
/// document" prefix, to nest it under a block.
fn document_message(error: PresentError) -> String {
    match error {
        PresentError::InvalidDocument(message) => message,
        other => other.to_string(),
    }
}

/// A v2 snapshot source names a repository-relative file; `open` and
/// `update` resolve it inside the work tree (SPC-014 B10).
fn validate_snapshot_source(id: &str, source: Option<&SnapshotSource>, version: u32) -> Result<()> {
    let Some(source) = source else {
        return Ok(());
    };
    if version == 1 {
        return Err(needs_v2("source", &format!("block {id}")));
    }
    let path = &source.path;
    let relative = !path.is_empty()
        && path.len() <= 1024
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if !relative {
        return Err(invalid(format!(
            "block {id}: source.path {path:?} must be a repository-relative path without . or .. parts"
        )));
    }
    Ok(())
}

fn validate_stage_framing(
    id: &str,
    title: Option<&str>,
    caption: Option<&str>,
    legend: Option<&[LegendEntry]>,
    description: Option<&str>,
) -> Result<()> {
    let Some(title) = title else {
        return Err(invalid(format!(
            "html block {id} needs a title: every schema_version 2 stage is a figure framed as \"Figure N · title\""
        )));
    };
    require_nonempty_bounded(
        &format!("html block {id} title"),
        title,
        limits::MAX_TITLE_BYTES,
    )?;
    let Some(caption) = caption else {
        return Err(invalid(format!(
            "html block {id} needs a caption: every schema_version 2 stage states its takeaway"
        )));
    };
    require_nonempty_bounded(
        &format!("html block {id} caption"),
        caption,
        limits::MAX_TITLE_BYTES,
    )?;
    if let Some(legend) = legend {
        if legend.is_empty() || legend.len() > limits::MAX_LEGEND_ENTRIES {
            return Err(invalid(format!(
                "html block {id} legend must list 1 to {} entries",
                limits::MAX_LEGEND_ENTRIES
            )));
        }
        for entry in legend {
            require_nonempty_bounded(
                &format!("html block {id} legend label"),
                &entry.label,
                limits::MAX_TITLE_BYTES,
            )?;
            require_nonempty_bounded(
                &format!("html block {id} legend means"),
                &entry.means,
                limits::MAX_TITLE_BYTES,
            )?;
        }
    }
    if let Some(description) = description {
        if description.trim().is_empty()
            || description.chars().count() > limits::MAX_DESCRIPTION_CHARS
        {
            return Err(invalid(format!(
                "html block {id} description must be 1 to {} characters",
                limits::MAX_DESCRIPTION_CHARS
            )));
        }
    }
    Ok(())
}

/// Schema v2: every mark an authored figure draws names itself, because an
/// unnamed mark could only be told apart by its position (B2). A figure
/// drawn from a layout keys its marks from the layout instead.
fn validate_figure_marks(id: &str, declaration: &serde_json::Value) -> Result<()> {
    let Some(figure) = declaration.get("figure") else {
        return Ok(());
    };
    if figure.get("layout").is_some() {
        return Ok(());
    }
    for composition in ["wide", "narrow"] {
        let Some(draw) = figure
            .pointer(&format!("/{composition}/draw"))
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        let mut seen = HashSet::new();
        for (index, item) in draw.iter().enumerate() {
            let Some(state) = item.get("state") else {
                continue;
            };
            let Some(mark) = item.get("id").and_then(serde_json::Value::as_str) else {
                return Err(invalid(format!(
                    "figure block {id}: {composition}.draw[{index}] draws the state {state} without an id; a schema_version 2 authored figure names every mark"
                )));
            };
            if !crate::entity::is_entity_id(mark) {
                return Err(invalid(format!(
                    "figure block {id}: mark id {mark:?} does not match the entity id grammar (lower-case kebab-case, at most 64 characters)"
                )));
            }
            if mark.starts_with(crate::entity::LEGEND_PREFIX) {
                return Err(invalid(format!(
                    "figure block {id}: mark id {mark:?} uses the prefix legend-, which is reserved for legend entries"
                )));
            }
            if !seen.insert(mark) {
                return Err(invalid(format!(
                    "figure block {id}: mark id {mark:?} is used twice in the {composition} composition"
                )));
            }
        }
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    if !valid_block_id(id) {
        return Err(invalid(format!("invalid block id {id:?}")));
    }
    Ok(())
}

fn valid_block_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && id.as_bytes()[0].is_ascii_alphanumeric()
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

/// The figure families of `figure-grammar.md`, in the grammar module's order.
pub const FIGURE_FAMILIES: [&str; 9] = [
    "flow",
    "structure",
    "layering",
    "sequence",
    "state",
    "coverage",
    "extent",
    "derivation",
    "graph",
];

/// The title and caption a figure placeholder shows before the grammar module
/// draws it; they are also the block's canonical review text.
fn figure_text(declaration: &serde_json::Value) -> (&str, &str) {
    let field = |name: &str| {
        declaration
            .pointer(&format!("/figure/{name}"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    };
    (field("title"), field("caption"))
}

/// The envelope the service can check. The client validates the rest with the
/// grammar module before it draws, and shows the failure in place of the
/// figure. Present has no repository source to bind, so it draws authored
/// figures only; a derived figure binds its data in the docs portal.
fn validate_figure_declaration(declaration: &serde_json::Value) -> Result<()> {
    let encoded = serde_json::to_vec(declaration)
        .map_err(|_| invalid("figure declaration is not serializable"))?;
    if encoded.len() > limits::MAX_FIGURE_DECLARATION_BYTES {
        return Err(invalid(format!(
            "figure declaration exceeds {} bytes",
            limits::MAX_FIGURE_DECLARATION_BYTES
        )));
    }
    let Some(root) = declaration.as_object() else {
        return Err(invalid("figure declaration must be an object"));
    };
    if root
        .keys()
        .any(|key| key != "schema_version" && key != "figure")
        || root.get("schema_version") != Some(&serde_json::Value::from(1))
    {
        return Err(invalid(
            "figure declaration must be { schema_version: 1, figure: { ... } }",
        ));
    }
    let Some(figure) = root.get("figure").and_then(serde_json::Value::as_object) else {
        return Err(invalid("figure declaration must carry a figure object"));
    };
    let family = figure
        .get("family")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if !FIGURE_FAMILIES.contains(&family) {
        return Err(invalid(format!(
            "figure family must be one of {}",
            FIGURE_FAMILIES.join(", ")
        )));
    }
    if figure.get("binding").and_then(serde_json::Value::as_str) != Some("authored") {
        return Err(invalid(
            "present draws authored figures; a derived figure binds its data in the docs portal",
        ));
    }
    let (title, caption) = figure_text(declaration);
    require_nonempty_bounded("figure title", title, limits::MAX_TITLE_BYTES)?;
    require_nonempty_bounded("figure caption", caption, limits::MAX_TITLE_BYTES)
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
            summary: None,
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
        let raw = br#"{"schema_version":3,"title":"Future","blocks":[],"new":true}"#;
        assert_eq!(
            parse_document(raw).unwrap(),
            ParsedDocument::Unsupported {
                schema_version: 3,
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
    fn html_canonical_text_includes_visible_stage_labels() {
        let block = Block::Html {
            caption: None,
            legend: None,
            description: None,
            id: "stage".to_string(),
            title: Some("Stage title".to_string()),
            html: "<figure><svg><text>Element · click a figure</text></svg></figure>".to_string(),
        };
        let canonical = block.canonical_review_text(&crate::document::Framing::default());
        assert!(canonical.contains("Stage title"));
        assert!(
            canonical.contains("Element · click a figure"),
            "visible SVG labels must be selectable as Text: {canonical:?}"
        );
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
    fn diagram_blocks_are_refused_before_typed_parsing() {
        let raw = br#"{
          "schema_version": 1,
          "title": "Retired",
          "blocks": [{"type":"tabs","id":"views","tabs":[{"label":"One","blocks":[
            {"type":"diagram","id":"flow","kind":"flowchart","source":"flowchart LR","acc_title":"Flow","acc_description":"A flow."}
          ]}]}]
        }"#;
        let message = parse_document(raw).unwrap_err().to_string();
        assert!(
            message.contains("block \"flow\" is a diagram block, which was removed with Mermaid"),
            "{message}"
        );
        assert!(
            message.contains("convert its flowchart to a flow figure"),
            "{message}"
        );

        // Refused before typed parsing: a diagram whose shape would never
        // parse still names the conversion instead of a serde error.
        let malformed =
            br#"{"schema_version":1,"title":"T","blocks":[{"type":"diagram","kind":"mindmap"}]}"#;
        let message = parse_document(malformed).unwrap_err().to_string();
        assert!(
            message.contains("the block at blocks[0] is a diagram block"),
            "{message}"
        );
        assert!(
            message.contains("convert its mindmap to a tree block"),
            "{message}"
        );
    }

    fn figure(id: &str, declaration: serde_json::Value) -> Block {
        Block::Figure {
            id: id.to_string(),
            declaration,
        }
    }

    fn authored_declaration() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "figure": {
                "id": "review-path",
                "family": "flow",
                "binding": "authored",
                "title": "One change on its review path",
                "caption": "A change passes review before it lands."
            }
        })
    }

    #[test]
    fn figure_declarations_are_checked_at_the_envelope_and_share_the_drawn_budget() {
        let valid = authored_declaration();
        assert!(document(vec![figure("fig", valid.clone())])
            .validate()
            .is_ok());
        let block = figure("fig", valid.clone());
        assert_eq!(block.review_label(), "One change on its review path");
        assert_eq!(
            block.canonical_review_text(&crate::document::Framing::default()),
            "One change on its review path\nA change passes review before it lands."
        );

        let refused = |change: &dyn Fn(&mut serde_json::Value), expected: &str| {
            let mut declaration = valid.clone();
            change(&mut declaration);
            let error = document(vec![figure("fig", declaration)])
                .validate()
                .expect_err(expected)
                .to_string();
            assert!(error.contains(expected), "{error}");
        };
        refused(
            &|value| value["figure"]["binding"] = "derived".into(),
            "present draws authored figures",
        );
        refused(
            &|value| value["figure"]["family"] = "chart".into(),
            "figure family must be one of",
        );
        refused(
            &|value| value["figure"]["caption"] = " ".into(),
            "figure caption must not be empty",
        );
        refused(
            &|value| value["extra"] = true.into(),
            "schema_version: 1, figure",
        );
        refused(
            &|value| value["schema_version"] = 2.into(),
            "schema_version: 1, figure",
        );
        refused(
            &|value| {
                value["figure"]["description"] =
                    "x".repeat(limits::MAX_FIGURE_DECLARATION_BYTES).into();
            },
            "figure declaration exceeds",
        );
        refused(&|value| *value = serde_json::json!([]), "must be an object");

        // The drawn budget counts figure blocks across the nested tree.
        let drawn = |count: usize| -> Vec<Block> {
            (0..count)
                .map(|index| figure(&format!("fig-{index}"), valid.clone()))
                .collect()
        };
        assert!(document(drawn(limits::MAX_FIGURE_BLOCKS))
            .validate()
            .is_ok());
        let over = document(vec![
            Block::Disclosure {
                id: "more".to_string(),
                summary: "More".to_string(),
                blocks: drawn(limits::MAX_FIGURE_BLOCKS),
            },
            figure("one-more", valid.clone()),
        ]);
        let error = over
            .validate()
            .expect_err("over the drawn budget")
            .to_string();
        assert!(error.contains("figure count exceeds 24"), "{error}");
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

    /// QA defect 8: a quote across two parts of a block keeps a separator.
    #[test]
    fn review_text_separates_a_blocks_parts() {
        let text = |value: serde_json::Value| {
            serde_json::from_value::<Block>(value)
                .unwrap()
                .canonical_review_text(&crate::document::Framing::default())
        };
        assert_eq!(
            text(serde_json::json!({"type": "status", "id": "s", "items": [
                {"label": "Linux Chrome run", "state": "pending", "detail": "one crop was a sliver"},
                {"label": "macOS run", "state": "pending"}
            ]})),
            "Linux Chrome run\none crop was a sliver\nmacOS run"
        );
        assert_eq!(
            text(
                serde_json::json!({"type": "tree", "id": "t", "label": "crates", "nodes": [
                    {"label": "state.rs", "children": [{"label": "limits.rs"}]}, {"label": "lib.rs"}
                ]})
            ),
            "crates\nstate.rs\nlimits.rs\nlib.rs"
        );
        assert_eq!(
            text(
                serde_json::json!({"type": "narrative", "id": "n", "markdown": "# Why\n\nOne.\n\n- a\n- b"})
            ),
            "Why\nOne.\na\nb"
        );
    }

    /// TSK-071 round 3: a diff's review text is what the reader sees and
    /// selects, a changed line without its marker and without the screen
    /// reader label; file headers and context lines are unchanged.
    #[test]
    fn diff_review_text_holds_no_label_or_marker() {
        let block: Block = serde_json::from_value(serde_json::json!({
            "type": "diff", "id": "change", "caption": "One change.",
            "diff": "--- a/x.rs\n+++ b/x.rs\n fn f() {\n-    let n = 1;\n+    let n = 2;\n }"
        }))
        .unwrap();
        let text = block.canonical_review_text(&crate::document::Framing::default());
        assert_eq!(
            text,
            "One change.\n--- a/x.rs\n+++ b/x.rs\n fn f() {\n    let n = 1;\n    let n = 2;\n }\n"
        );
        assert!(!text.contains("Added") && !text.contains("Removed"));
        assert_eq!(diff_line_parts("+x"), ("Added: ", "+", "x"));
        assert_eq!(diff_line_parts("-x"), ("Removed: ", "-", "x"));
        assert_eq!(diff_line_parts("+++ b/x"), ("", "", "+++ b/x"));
        assert_eq!(diff_line_parts(" x"), ("", "", " x"));
    }

    #[test]
    fn review_labels_are_readable_names_never_raw_ids() {
        // QA defect 8: underscores kept, no Markdown syntax, a heading ends
        // the label, references read as their kind, and no block shows its id.
        let block = |value: serde_json::Value| -> Block { serde_json::from_value(value).unwrap() };
        for (value, expected) in [
            (
                serde_json::json!({"type": "narrative", "id": "a", "markdown": "## Why `snake_case` wins\n\nMore text."}),
                "Why snake_case wins",
            ),
            (
                serde_json::json!({"type": "narrative", "id": "b", "markdown": "See [fig:flow] for\nthe path. Then more."}),
                "See Figure for the path",
            ),
            (
                serde_json::json!({"type": "table", "id": "t", "title": "Review limits", "columns": ["A"], "rows": [["1"]]}),
                "Review limits",
            ),
            (
                serde_json::json!({"type": "tabs", "id": "views", "tabs": [{"label": "Plan", "blocks": []}, {"label": "Risks", "blocks": []}]}),
                "Plan, Risks",
            ),
            (
                serde_json::json!({"type": "bullets", "id": "points", "items": ["Select **words** to quote them."]}),
                "Select words to quote them",
            ),
            (
                serde_json::json!({"type": "code", "id": "snippet", "language": "rust", "code": "fn main() {}"}),
                "Code",
            ),
            (
                serde_json::json!({"type": "diff", "id": "change", "diff": "+a"}),
                "Diff",
            ),
        ] {
            assert_eq!(block(value).review_label(), expected);
        }
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

        let mut declaration = authored_declaration();
        declaration["figure"]["title"] =
            "Verified native paths versus open Windows gap and more words".into();
        let label = figure("fig", declaration).review_label();
        assert!(label.chars().count() <= 40, "{label}");
        assert!(label.ends_with('…'), "{label}");
        // Full prompt still available for body rendering via prompt field, not review_label.
        assert_ne!(feedback.review_label(), long_prompt);

        let narrative = Block::Narrative {
            id: "frame".to_string(),
            markdown: "How to use this surface. Agents author this session.".to_string(),
        };
        assert_eq!(narrative.review_label(), "How to use this surface");

        let status = Block::Status {
            id: "verification".to_string(),
            items: Vec::new(),
        };
        assert_eq!(status.review_label(), "Evidence");

        // A v2 table is named by its title; a v1 table has none and keeps
        // its id.
        let table = |title: Option<&str>| Block::Table {
            id: "gates".to_string(),
            columns: vec!["Check".to_string()],
            rows: Vec::new(),
            title: title.map(str::to_string),
            caption: None,
        };
        assert_eq!(
            table(Some("Where each check runs")).review_label(),
            "Where each check runs"
        );
        assert_eq!(table(None).review_label(), "gates");
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
