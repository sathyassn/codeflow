//! Copy checks for `present check` (TSK-1255) and for built portal pages:
//! a visible text block over the character budget, a run of text blocks with
//! no figure between them, text that may be cut to an ellipsis, and a clock
//! time without a zone. They are warnings, so `open`, `update` and
//! `validate --portal` print them and proceed.
//!
//! The length limit is a character budget, not a count of rendered lines.
//! The runtime sets prose in a column of at most `--cf-measure: 68ch`
//! (`web/src/styles.css`), so 136 characters approximate two desktop lines
//! of a paragraph, a list item or the lede, and about five lines on a phone.
//! Word wrapping, long words, wide glyphs and narrow table or comparison
//! cells all change the real line count; the same budget applies to each of
//! them. A hard break ends its line, so it counts as a full 68 characters.
use pulldown_cmark::{Event, Tag, TagEnd};
use scraper::{ElementRef, Html};

use crate::document::{
    coalesced_markdown, figure_text, reference_segments, Block, PresentationDocument, TextSegment,
};

/// Characters on one line of the default reading measure (68ch).
pub const MEASURE_CHARS: usize = 68;
/// Characters a visible text block may hold outside collapsed detail:
/// about two desktop lines at the reading measure.
pub const CHAR_BUDGET: usize = 2 * MEASURE_CHARS;
/// Characters of visible text an HTML stage may hold: about one short
/// caption per node of a small figure.
pub const STAGE_BUDGET: usize = 320;
/// Text blocks that may follow each other before a figure, table or status
/// rows must come between them.
pub const MAX_TEXT_RUN: usize = 3;

/// One copy finding: the block it names, its rule and the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyFinding {
    pub block_id: Option<String>,
    pub rule: &'static str,
    pub message: String,
}

/// One unit of a built page's content in reading order: a paragraph, a
/// list (its items are each measured) or anything that is not text, such as
/// a figure, table, code block or disclosure. Headings are left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageBlock {
    Paragraph(PageText),
    List(Vec<PageText>),
    Visual,
}

/// A paragraph's or list item's text as the page shows it, and the same
/// text with its code blanked to spaces: the length counts the shown text,
/// and the ellipsis and time checks read the blanked one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageText {
    pub text: String,
    pub prose: String,
}

impl PageText {
    fn run(&self) -> Run {
        Run {
            text: self.text.clone(),
            prose: self.prose.clone(),
            measure: Measure::Block,
        }
    }
}

/// Every copy finding for a built page, in reading order. Findings name no
/// block; the message quotes the opening of the text it is about.
#[must_use]
pub fn page_findings(blocks: &[PageBlock]) -> Vec<CopyFinding> {
    let mut findings = Vec::new();
    let mut units = Vec::new();
    for block in blocks {
        match block {
            PageBlock::Paragraph(text) => {
                check_run(&mut findings, None, &text.run(), true);
                units.push(Unit::Text(None, opening(&text.text)));
            }
            PageBlock::List(items) => {
                for item in items {
                    check_run(&mut findings, None, &item.run(), true);
                }
                let first = items
                    .first()
                    .map_or_else(String::new, |item| opening(&item.text));
                units.push(Unit::Text(None, first));
            }
            PageBlock::Visual => units.push(Unit::Break),
        }
    }
    check_text_runs(&mut findings, &units);
    findings
}

/// Every copy finding for a document, in block order, then the runs of
/// text blocks.
pub(crate) fn copy_findings(document: &PresentationDocument) -> Vec<CopyFinding> {
    let mut findings = Vec::new();
    check_run(
        &mut findings,
        None,
        &Run::plain(&document.title, Measure::None),
        true,
    );
    if let Some(summary) = &document.summary {
        check_run(
            &mut findings,
            None,
            &Run::plain(summary, Measure::Block),
            true,
        );
    }
    visit_blocks(&document.blocks, true, &mut findings);
    let mut sequences = Vec::new();
    run_units(&document.blocks, &mut sequences);
    for units in &sequences {
        check_text_runs(&mut findings, units);
    }
    findings
}

/// How a run's length counts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Measure {
    /// A label, a title or collapsed detail: its length never counts.
    None,
    /// A text block, held to [`CHAR_BUDGET`].
    Block,
    /// All the visible text of an HTML stage, held to [`STAGE_BUDGET`].
    Stage,
}

struct Run {
    /// The text as the page shows it.
    text: String,
    /// The same text with each code span blanked, for the ellipsis and time
    /// checks: a code span is literal.
    prose: String,
    measure: Measure,
}

impl Run {
    fn plain(text: &str, measure: Measure) -> Self {
        Self {
            text: text.to_string(),
            prose: text.to_string(),
            measure,
        }
    }
}

/// One string a reader sees, and how to read it.
enum Text {
    /// Markdown: each paragraph, list item and heading is its own run; code
    /// blocks and code spans are left out.
    Markdown(String),
    /// A plain string and how its length counts.
    Plain(String, Measure),
    /// An HTML stage: the inline text of each block element outside code,
    /// scripts and styles is its own run, so a zone is never read across
    /// separate blocks or labels, and the stage's whole visible text is
    /// held to [`STAGE_BUDGET`].
    Html(String),
}

impl Text {
    fn runs(&self) -> Vec<Run> {
        match self {
            Self::Markdown(markdown) => markdown_runs(markdown),
            Self::Plain(text, measure) => vec![Run::plain(text, *measure)],
            Self::Html(html) => {
                let nodes = html_text_runs(html);
                let whole = nodes.join(" ");
                let mut runs: Vec<Run> = nodes
                    .iter()
                    .map(|node| Run::plain(node, Measure::None))
                    .collect();
                runs.push(Run {
                    prose: String::new(),
                    text: whole,
                    measure: Measure::Stage,
                });
                runs
            }
        }
    }
}

fn check_run(findings: &mut Vec<CopyFinding>, block_id: Option<&str>, run: &Run, visible: bool) {
    let block_id = block_id.map(str::to_string);
    let length = budget_chars(&run.text);
    if visible && run.measure == Measure::Block && length > CHAR_BUDGET {
        findings.push(CopyFinding {
            block_id: block_id.clone(),
            rule: "copy-density",
            message: format!(
                "a text block has {length} characters, over the {CHAR_BUDGET}-character budget (about two desktop lines at the {MEASURE_CHARS}ch reading measure, five on a phone); keep a short lead and move the rest to a figure, status rows or a table, to bullets with one point each, or behind a disclosure: \"{}\"",
                opening(&run.text)
            ),
        });
    }
    if visible && run.measure == Measure::Stage && length > STAGE_BUDGET {
        findings.push(CopyFinding {
            block_id: block_id.clone(),
            rule: "copy-density",
            message: format!(
                "the stage holds {length} characters of text, over the {STAGE_BUDGET}-character budget for a figure; text in styled boxes is not a figure (the \"Boxed prose\" smell), so draw the relation and move the sentences beside it: \"{}\"",
                opening(&run.text)
            ),
        });
    }
    if cut_ellipsis(&run.prose) {
        findings.push(CopyFinding {
            block_id: block_id.clone(),
            rule: "copy-ellipsis",
            message: format!(
                "visible text may be cut to an ellipsis; keep the context whole, wrapped or behind a disclosure, or keep an exact quote or title and say why: \"{}\"",
                opening(&run.text)
            ),
        });
    }
    if let Some(time) = bare_clock_time(&run.prose) {
        findings.push(CopyFinding {
            block_id,
            rule: "copy-time-zone",
            message: format!(
                "the time {time} has no zone; write the wall-clock time with its zone, such as \"16:12 EST, 2 Nov\""
            ),
        });
    }
}

fn visit_blocks(blocks: &[Block], visible: bool, findings: &mut Vec<CopyFinding>) {
    for block in blocks {
        let id = Some(block.id());
        let mut texts = block_labels(block);
        texts.extend(
            block
                .markdown_fields()
                .into_iter()
                .map(|markdown| Text::Markdown(markdown.to_string())),
        );
        for text in &texts {
            for run in text.runs() {
                check_run(findings, id, &run, visible);
            }
        }
        match block {
            // The description sits in the stage's Details disclosure.
            Block::Html {
                description: Some(description),
                ..
            } => check_run(
                findings,
                id,
                &Run::plain(description, Measure::Block),
                false,
            ),
            // Collapsed detail: cut context and bare times still count.
            Block::Disclosure { blocks, .. } => visit_blocks(blocks, false, findings),
            Block::Tabs { tabs, .. } => {
                for tab in tabs {
                    visit_blocks(&tab.blocks, visible, findings);
                }
            }
            _ => {}
        }
    }
}

/// The plain strings a block shows outside its Markdown fields, and an HTML
/// stage's markup.
fn block_labels(block: &Block) -> Vec<Text> {
    let label = |text: &str| Text::Plain(text.to_string(), Measure::None);
    let prose = |text: &str| Text::Plain(text.to_string(), Measure::Block);
    let mut texts = Vec::new();
    match block {
        Block::Callout {
            title: Some(title), ..
        } => texts.push(label(title)),
        Block::Decision { .. } | Block::Form { .. } => texts.extend(form_labels(block)),
        Block::Comparison { columns, .. } => {
            texts.extend(columns.iter().map(|column| label(&column.title)));
        }
        Block::Table {
            columns,
            title,
            caption,
            ..
        } => {
            texts.extend(columns.iter().map(|column| label(column)));
            texts.extend(title.as_deref().map(label));
            texts.extend(caption.as_deref().map(prose));
        }
        Block::Status { items, .. } => {
            for item in items {
                texts.push(label(&item.label));
                texts.extend(item.detail.as_deref().map(prose));
            }
        }
        Block::Code { caption, .. }
        | Block::Diff { caption, .. }
        | Block::Media { caption, .. } => {
            texts.extend(caption.as_deref().map(prose));
        }
        Block::Tree {
            label: name, nodes, ..
        } => {
            texts.push(label(name));
            let mut stack: Vec<_> = nodes.iter().collect();
            while let Some(node) = stack.pop() {
                texts.push(label(&node.label));
                stack.extend(node.children.iter());
            }
        }
        Block::Figure { declaration, .. } => {
            let (title, caption) = figure_text(declaration);
            texts.push(label(title));
            texts.push(prose(caption));
        }
        Block::FeedbackPrompt { prompt, .. } => texts.push(prose(prompt)),
        Block::Html {
            html,
            title,
            caption,
            legend,
            ..
        } => {
            texts.extend(title.as_deref().map(label));
            texts.extend(caption.as_deref().map(prose));
            for entry in legend.iter().flatten() {
                texts.push(label(&entry.label));
                texts.push(label(&entry.means));
            }
            texts.push(Text::Html(html.clone()));
        }
        Block::Disclosure { summary, .. } => texts.push(label(summary)),
        Block::Tabs { title, tabs, .. } => {
            texts.extend(title.as_deref().map(label));
            texts.extend(tabs.iter().map(|tab| label(&tab.label)));
        }
        _ => {}
    }
    texts
}

/// A form's or decision's title, then each field's label, description and
/// choice labels, as the page shows them. A decision's one field is
/// labelled by its title.
fn form_labels(block: &Block) -> Vec<Text> {
    let label = |text: &str| Text::Plain(text.to_string(), Measure::None);
    let mut texts = Vec::new();
    let (Block::Decision { title, .. } | Block::Form { title, .. }) = block else {
        return texts;
    };
    texts.push(label(title));
    let decision = matches!(block, Block::Decision { .. });
    if let Some(view) = crate::form::FormView::of(block) {
        for field in view.fields.iter() {
            if !decision {
                texts.push(label(&field.label));
            }
            if let Some(description) = &field.description {
                texts.push(Text::Plain(description.clone(), Measure::Block));
            }
            for choice in field.options.iter().flatten() {
                texts.push(label(&choice.label));
            }
        }
    } else if let Block::Decision {
        options: Some(options),
        ..
    } = block
    {
        texts.extend(options.iter().map(|choice| label(&choice.label)));
    }
    texts
}

/// A unit of a block sequence for the run check: a text block, named by its
/// block and opening, or anything that breaks a run.
enum Unit {
    Text(Option<String>, String),
    Break,
}

/// The run units of the top level and of each tab, one sequence each.
/// Collapsed detail is a break and is not read.
fn run_units(blocks: &[Block], sequences: &mut Vec<Vec<Unit>>) {
    let mut units = Vec::new();
    for block in blocks {
        let id = Some(block.id().to_string());
        match block {
            Block::Narrative { markdown, .. } => {
                for opening in markdown_units(markdown) {
                    units.push(match opening {
                        Some(text) => Unit::Text(id.clone(), text),
                        None => Unit::Break,
                    });
                }
            }
            Block::Callout { markdown, .. } => {
                units.push(Unit::Text(id, opening(markdown)));
            }
            Block::Bullets { items, .. } => {
                let first = items.first().map_or_else(String::new, |item| opening(item));
                units.push(Unit::Text(id, first));
            }
            Block::FeedbackPrompt { prompt, .. } => units.push(Unit::Text(id, opening(prompt))),
            Block::Tabs { tabs, .. } => {
                units.push(Unit::Break);
                for tab in tabs {
                    run_units(&tab.blocks, sequences);
                }
            }
            _ => units.push(Unit::Break),
        }
    }
    sequences.push(units);
}

/// The top-level units of a Markdown field: the opening of each paragraph,
/// list or quote, and `None` for a code block, table or HTML block. Headings
/// neither count nor break a run.
fn markdown_units(markdown: &str) -> Vec<Option<String>> {
    let mut units = Vec::new();
    let mut depth = 0_usize;
    let mut current: Option<String> = None;
    for (event, _) in coalesced_markdown(markdown) {
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    match tag {
                        Tag::Paragraph | Tag::List(_) | Tag::BlockQuote(_) => {
                            current = Some(String::new());
                        }
                        Tag::CodeBlock(_) | Tag::Table(_) | Tag::HtmlBlock => units.push(None),
                        _ => {}
                    }
                }
                depth += 1;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some(text) = current.take() {
                        units.push(Some(opening(&text)));
                    }
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(current) = current.as_mut() {
                    current.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(current) = current.as_mut() {
                    current.push(' ');
                }
            }
            _ => {}
        }
    }
    units
}

/// Warn once for each run of more than [`MAX_TEXT_RUN`] text blocks, naming
/// its first block.
fn check_text_runs(findings: &mut Vec<CopyFinding>, units: &[Unit]) {
    let mut start = 0;
    while start < units.len() {
        let Unit::Text(block_id, first) = &units[start] else {
            start += 1;
            continue;
        };
        let length = units[start..]
            .iter()
            .take_while(|unit| matches!(unit, Unit::Text(..)))
            .count();
        if length > MAX_TEXT_RUN {
            findings.push(CopyFinding {
                block_id: block_id.clone(),
                rule: "copy-run",
                message: format!(
                    "{length} text blocks follow each other with no figure, table or status rows between them; at most {MAX_TEXT_RUN} in a row, so show the point as a figure, table or status rows before the next one: \"{first}\""
                ),
            });
        }
        start += length;
    }
}

/// The visible text runs of a Markdown field: one per paragraph, list item
/// or heading, with soft breaks as spaces and hard breaks as newlines.
fn markdown_runs(markdown: &str) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut current = (String::new(), String::new());
    let mut code_block = 0_usize;
    let mut heading = false;
    let flush = |current: &mut (String, String), runs: &mut Vec<Run>, measure: Measure| {
        if !current.0.trim().is_empty() {
            runs.push(Run {
                text: current.0.trim().to_string(),
                prose: current.1.trim().to_string(),
                measure,
            });
        }
        current.0.clear();
        current.1.clear();
    };
    let body = |heading: bool| {
        if heading {
            Measure::None
        } else {
            Measure::Block
        }
    };
    for (event, in_link) in coalesced_markdown(markdown) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code_block += 1,
            Event::End(TagEnd::CodeBlock) => code_block = code_block.saturating_sub(1),
            Event::Start(Tag::Heading { .. }) => {
                flush(&mut current, &mut runs, Measure::Block);
                heading = true;
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(&mut current, &mut runs, Measure::None);
                heading = false;
            }
            Event::Start(Tag::Paragraph | Tag::Item)
            | Event::End(TagEnd::Paragraph | TagEnd::Item) => {
                flush(&mut current, &mut runs, body(heading));
            }
            Event::Text(text) if code_block == 0 => {
                // A reference reads as its label ("Figure 1"), as the page
                // shows it; inside a link it stays literal.
                let segments = if in_link {
                    vec![TextSegment::Text(&text)]
                } else {
                    reference_segments(&text)
                };
                for segment in segments {
                    let shown = match segment {
                        TextSegment::Text(text) => text.to_string(),
                        TextSegment::Reference { kind, .. } => format!("{} 1", kind.noun()),
                    };
                    current.0.push_str(&shown);
                    current.1.push_str(&shown);
                }
            }
            // A code span takes room on the line, but its characters are
            // literal, so it never reads as a cut sentence or a bare time.
            Event::Code(code) => {
                current.0.push_str(&code);
                current.1.push_str(&" ".repeat(code.chars().count()));
            }
            Event::SoftBreak => {
                current.0.push(' ');
                current.1.push(' ');
            }
            Event::HardBreak => {
                current.0.push('\n');
                current.1.push('\n');
            }
            _ => {}
        }
    }
    flush(&mut current, &mut runs, body(heading));
    runs
}

/// Elements whose text is never drawn, or is code.
const HTML_HIDDEN: [&str; 9] = [
    "style", "script", "code", "pre", "kbd", "samp", "title", "desc", "template",
];

/// Elements that start their own text: block boxes, table cells, controls
/// and SVG text labels. Inline elements such as `strong` or `span` stay in
/// the text around them.
const HTML_BLOCKS: [&str; 41] = [
    "address",
    "article",
    "aside",
    "blockquote",
    "button",
    "caption",
    "dd",
    "details",
    "dialog",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "option",
    "p",
    "section",
    "summary",
    "svg",
    "table",
    "td",
    "th",
    "tr",
    "ul",
    "text",
    "foreignObject",
];

/// The visible text of an HTML stage as runs: the inline content of each
/// block element is one run ("Read at <strong>13:41</strong> EST"), and
/// separate blocks or SVG labels are separate runs, so a zone is never read
/// across them. Text in code, scripts, styles and the SVG `title` and `desc`
/// is left out.
fn html_text_runs(source: &str) -> Vec<String> {
    fn flush(current: &mut String, runs: &mut Vec<String>) {
        let run = current.split_whitespace().collect::<Vec<_>>().join(" ");
        if !run.is_empty() {
            runs.push(run);
        }
        current.clear();
    }
    fn walk(element: ElementRef<'_>, current: &mut String, runs: &mut Vec<String>) {
        for child in element.children() {
            if let Some(text) = child.value().as_text() {
                current.push_str(text);
            } else if let Some(child) = ElementRef::wrap(child) {
                let name = child.value().name();
                if HTML_HIDDEN.contains(&name) {
                    continue;
                }
                if name == "br" {
                    current.push(' ');
                    continue;
                }
                let block = HTML_BLOCKS.contains(&name);
                if block {
                    flush(current, runs);
                }
                walk(child, current, runs);
                if block {
                    flush(current, runs);
                }
            }
        }
    }
    let fragment = Html::parse_fragment(source);
    let mut runs = Vec::new();
    let mut current = String::new();
    walk(fragment.root_element(), &mut current, &mut runs);
    flush(&mut current, &mut runs);
    runs
}

/// The characters a run spends of the budget: its characters, with each
/// hard break filling the rest of its line at the reading measure.
pub(crate) fn budget_chars(text: &str) -> usize {
    let segments: Vec<&str> = text.split('\n').map(str::trim).collect();
    let last = segments.len().saturating_sub(1);
    segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            let count = segment.chars().count();
            if index < last {
                count.div_ceil(MEASURE_CHARS).max(1) * MEASURE_CHARS
            } else {
                count
            }
        })
        .sum()
}

/// The first words of a run, whole, for a message that names the block's
/// text without cutting it mid-word.
fn opening(text: &str) -> String {
    let mut out = String::new();
    for word in text.split_whitespace() {
        if !out.is_empty() && out.chars().count() + word.chars().count() > 60 {
            out.push_str(" (and more)");
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Whether the text holds an ellipsis that may cut it short. An ellipsis
/// inside quotation marks (an exact quote or title) or between two numbers
/// (a range such as "1...5") is literal.
pub(crate) fn cut_ellipsis(prose: &str) -> bool {
    let chars: Vec<char> = blank_quoted(prose).chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let width = if chars[index] == '\u{2026}' {
            1
        } else if chars[index..].starts_with(&['.', '.', '.']) {
            3
        } else {
            index += 1;
            continue;
        };
        let before = chars[..index].iter().rev().find(|c| !c.is_whitespace());
        let after = chars[index + width..].iter().find(|c| !c.is_whitespace());
        let range =
            before.is_some_and(char::is_ascii_digit) && after.is_some_and(char::is_ascii_digit);
        if !range {
            return true;
        }
        index += width;
    }
    false
}

/// The text with every closed quotation blanked: straight double quotes,
/// curly double quotes, curly single quotes and guillemets.
fn blank_quoted(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let close = match chars[index] {
            '"' => '"',
            '\u{201c}' => '\u{201d}',
            '\u{2018}' => '\u{2019}',
            '\u{ab}' => '\u{bb}',
            _ => {
                index += 1;
                continue;
            }
        };
        let Some(offset) = chars[index + 1..].iter().position(|c| *c == close) else {
            index += 1;
            continue;
        };
        let end = index + 1 + offset;
        for c in &mut chars[index + 1..end] {
            *c = ' ';
        }
        index = end + 1;
    }
    chars.into_iter().collect()
}

/// Zone names a time may carry, in capitals.
const ZONES: &[&str] = &[
    "UTC", "GMT", "EST", "EDT", "CST", "CDT", "MST", "MDT", "PST", "PDT", "AKST", "AKDT", "HST",
    "BST", "IST", "CET", "CEST", "EET", "EEST", "WET", "WEST", "MSK", "JST", "KST", "AEST", "AEDT",
    "ACST", "ACDT", "AWST", "NZST", "NZDT", "SGT", "HKT",
];

/// A word right after a number pair that makes it a ratio or a duration.
const NOT_CLOCK_AFTER: &[&str] = &[
    "aspect",
    "ratio",
    "ratios",
    "min",
    "mins",
    "minute",
    "minutes",
    "sec",
    "secs",
    "second",
    "seconds",
    "s",
    "m",
    "h",
    "hr",
    "hrs",
    "hour",
    "hours",
    "long",
    "split",
    "pace",
    "lap",
    "contrast",
    "odds",
    "scale",
    "zoom",
    "duration",
    "elapsed",
    "remaining",
    "left",
    "runtime",
];

/// A word shortly before a number pair that makes it a ratio or a duration.
const NOT_CLOCK_BEFORE: &[&str] = &[
    "ratio", "aspect", "contrast", "scale", "odds", "duration", "lasted", "took", "runtime",
    "elapsed", "pace", "verse", "chapter", "length", "timer", "lap", "split",
];

/// The first clock time in `text` that names no zone, such as "13:41" in
/// "read at 13:41", or a range such as "09:00 to 10:00". A time is zoned by a
/// known zone name ("EST", "UTC+1"), a numeric offset ("+05:00", "-0500",
/// spaced or attached) or `Z` right after it. Ratios and durations ("16:10
/// aspect ratio", "1:30 min"), versions and out-of-range values are not
/// times; when the match is unsure there is no finding.
///
/// An attached "-HH:MM" can be an offset ("08:15-05:00") or the end of a
/// range ("09:00-10:00"). Whenever it is a valid offset (up to 14 hours),
/// it is read as the offset and there is no finding, so a bare hyphenated
/// range draws no warning. A hyphenated range whose end cannot be an offset
/// ("09:00-17:00"), a spaced or dashed range ("09:00 - 10:00", "09:00 to
/// 10:00") and an en dash range are unzoned without one.
pub(crate) fn bare_clock_time(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let Some(clock) = clock_at(text, index, false) else {
            index += 1;
            continue;
        };
        if not_a_clock(text, index, clock.end) {
            index = clock.end;
            continue;
        }
        let attached_offset = offset_len(&text[clock.end..], false).is_some();
        let end = if clock.seconds || clock.iso || attached_offset {
            clock.end
        } else {
            range_end(text, clock.end).unwrap_or(clock.end)
        };
        if !zone_follows(&text[end..]) {
            return Some(text[index..end].to_string());
        }
        index = end;
    }
    None
}

struct Clock {
    end: usize,
    seconds: bool,
    iso: bool,
}

fn digits_at(bytes: &[u8], from: usize) -> usize {
    bytes
        .get(from..)
        .unwrap_or_default()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count()
}

/// A clock time starting at byte `start`: one or two hour digits, a colon,
/// two minute digits and optional seconds, standing on its own.
fn clock_at(text: &str, start: usize, after_connector: bool) -> Option<Clock> {
    let bytes = text.as_bytes();
    if !bytes.get(start)?.is_ascii_digit() {
        return None;
    }
    let mut iso = false;
    if start > 0 && !after_connector {
        let before = bytes[start - 1];
        // "T" joins a date to its time in ISO 8601 ("2026-11-02T13:41").
        iso = before == b'T' && start > 1 && bytes[start - 2].is_ascii_digit();
        if !iso && (before.is_ascii_alphanumeric() || b".:,+-/_#@".contains(&before)) {
            return None;
        }
    }
    let hour_len = digits_at(bytes, start);
    if !(1..=2).contains(&hour_len) || bytes.get(start + hour_len) != Some(&b':') {
        return None;
    }
    let minute = start + hour_len + 1;
    if digits_at(bytes, minute) != 2 {
        return None;
    }
    let mut end = minute + 2;
    let mut seconds = false;
    if bytes.get(end) == Some(&b':') && digits_at(bytes, end + 1) == 2 {
        let value: u32 = text[end + 1..end + 3].parse().ok()?;
        if value > 59 {
            return None;
        }
        end += 3;
        seconds = true;
        if bytes.get(end) == Some(&b'.') && digits_at(bytes, end + 1) > 0 {
            end += 1 + digits_at(bytes, end + 1);
        }
    }
    // More digits, letters or a version-like tail: not a clock.
    let next = bytes.get(end).copied();
    if next.is_some_and(|byte| byte.is_ascii_alphanumeric() && byte != b'Z')
        || (matches!(next, Some(b':' | b'.')) && digits_at(bytes, end + 1) > 0)
    {
        return None;
    }
    let hour: u32 = text[start..start + hour_len].parse().ok()?;
    let minutes: u32 = text[minute..minute + 2].parse().ok()?;
    (hour <= 23 && minutes <= 59).then_some(Clock { end, seconds, iso })
}

/// Whether the words around a number pair make it a ratio or a duration.
fn not_a_clock(text: &str, start: usize, end: usize) -> bool {
    let word = |raw: &str| {
        raw.trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    };
    let after = text[end..].split_whitespace().next().map(word);
    if after.is_some_and(|after| NOT_CLOCK_AFTER.contains(&after.as_str())) {
        return true;
    }
    text[..start]
        .split_whitespace()
        .rev()
        .take(3)
        .map(word)
        .any(|before| NOT_CLOCK_BEFORE.contains(&before.as_str()))
}

/// The end of a range's second time, when a range connector and another
/// clock follow the first ("09:00-17:00", "9:00 to 10:30").
fn range_end(text: &str, end: usize) -> Option<usize> {
    let rest = &text[end..];
    for connector in [
        "-",
        "\u{2013}",
        "\u{2014}",
        " - ",
        " \u{2013} ",
        " \u{2014} ",
        " to ",
        " until ",
    ] {
        if let Some(tail) = rest.strip_prefix(connector) {
            let start = text.len() - tail.len();
            if let Some(second) = clock_at(text, start, true) {
                return Some(second.end);
            }
        }
    }
    None
}

/// A numeric offset at the start of `text` and its length: `+HH:MM`,
/// `-HHMM` or, when `short` is allowed (after a zone name), `+H` or `-HH`.
fn offset_len(text: &str, short: bool) -> Option<usize> {
    let bytes = text.as_bytes();
    if !matches!(bytes.first(), Some(b'+' | b'-')) {
        return None;
    }
    let hours = digits_at(bytes, 1);
    let (len, hour, minute) =
        if hours == 2 && bytes.get(3) == Some(&b':') && digits_at(bytes, 4) == 2 {
            (6, &text[1..3], &text[4..6])
        } else if hours == 4 {
            (5, &text[1..3], &text[3..5])
        } else if short && (1..=2).contains(&hours) {
            (1 + hours, &text[1..=hours], "00")
        } else {
            return None;
        };
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    let clean_end = !bytes.get(len).is_some_and(u8::is_ascii_alphanumeric);
    (hour <= 14 && minute <= 59 && clean_end).then_some(len)
}

/// Whether the text right after a clock time names its zone.
fn zone_follows(after: &str) -> bool {
    let bytes = after.as_bytes();
    if bytes.first() == Some(&b'Z') {
        return !bytes.get(1).is_some_and(u8::is_ascii_alphanumeric);
    }
    if offset_len(after, false).is_some() {
        return true;
    }
    let mut rest = after.trim_start_matches(' ');
    if offset_len(rest, false).is_some() {
        return true;
    }
    for meridiem in ["a.m.", "p.m.", "am", "pm", "AM", "PM"] {
        if let Some(tail) = rest.strip_prefix(meridiem) {
            if !tail.starts_with(|c: char| c.is_ascii_alphanumeric()) {
                rest = tail.trim_start_matches(' ');
                break;
            }
        }
    }
    let rest = rest.strip_prefix('(').unwrap_or(rest);
    let letters = rest.bytes().take_while(u8::is_ascii_uppercase).count();
    if !ZONES.contains(&&rest[..letters]) {
        return false;
    }
    let tail = &rest[letters..];
    !tail.starts_with(|c: char| c.is_ascii_alphanumeric()) || offset_len(tail, true).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare(text: &str) -> Option<String> {
        bare_clock_time(text)
    }

    #[test]
    fn bare_clock_time_finds_times_without_a_zone() {
        assert_eq!(bare("read at 13:41"), Some("13:41".into()));
        assert_eq!(bare("at 7:05 today"), Some("7:05".into()));
        assert_eq!(bare("4:30 pm today"), Some("4:30".into()));
        assert_eq!(bare("2026-11-02T13:41"), Some("13:41".into()));
        assert_eq!(bare("13:41 ESTIMATE"), Some("13:41".into()));
        // A capitalized word that is not a zone does not zone the time.
        assert_eq!(bare("Read at 13:41 TODO"), Some("13:41".into()));
        assert_eq!(bare("13:41 API check"), Some("13:41".into()));
        // A range is one time, and it needs a zone after its second end.
        assert_eq!(bare("09:00-17:00"), Some("09:00-17:00".into()));
        assert_eq!(bare("Meet 09:00 - 10:00"), Some("09:00 - 10:00".into()));
        assert_eq!(bare("9:00 to 10:30 today"), Some("9:00 to 10:30".into()));
        assert_eq!(
            bare("09:00 \u{2013} 10:00"),
            Some("09:00 \u{2013} 10:00".into())
        );
        // An offset that is out of range, or too short without a zone name.
        assert_eq!(bare("13:41 +25:00"), Some("13:41".into()));
        assert_eq!(bare("13:41 +5"), Some("13:41".into()));
        // The second of two times.
        assert_eq!(bare("13:41 EST and 14:00"), Some("14:00".into()));
        assert_eq!(bare("Meet 09:00 to 10:00"), Some("09:00 to 10:00".into()));
    }

    #[test]
    fn bare_clock_time_accepts_zones_and_offsets() {
        for zoned in [
            "13:41 EST, 2 Nov",
            "13:41:07 UTC",
            "08:15Z",
            "2026-11-02T13:41:07Z",
            "2026-11-02T13:41:07+05:00",
            "2026-11-02T13:41-05:00",
            "08:15+01:00",
            "09:00-0500",
            "13:41 +05:00",
            "13:41 -0500",
            "13:41 UTC+1",
            "13:41 GMT-05:00",
            "4:30 pm EST",
            "4:30 PM (PDT)",
            "09:00-10:00 EST",
            "9:00 to 10:30 CET",
            // An attached negative offset, whatever its size against the
            // clock, with and without seconds or a date; a bare hyphenated
            // range reads the same way, so it draws no finding.
            "Read at 01:00-05:00.",
            "Read at 05:00-05:00.",
            "Read at 01:00:00-05:00",
            "Read on 2026-11-02 01:00-05:00",
            "09:00-10:00",
            "Meet 08:15-09:00 daily",
            "Read at 08:15-05:00 today",
            "Read at 08:15:30-05:00",
            "Read on 2026-11-02 08:15-05:00",
            "Read at 2026-11-02T08:15-05:00",
            "Read at 08:15-0500",
            "Read at 13:41-03:30",
        ] {
            assert_eq!(bare(zoned), None, "{zoned}");
        }
    }

    #[test]
    fn bare_clock_time_skips_ratios_durations_versions_and_code() {
        for not_a_time in [
            "a floor of 4.5:1 and 3:1",
            "a 16:9 frame and 4:3",
            "the 16:10 aspect ratio",
            "an aspect ratio of 16:10",
            "a contrast of 21:00 is odd",
            "the run took 1:30 min",
            "1:30 minutes long",
            "99:00 is not a time",
            "12:75 is not a time",
            "version 1.12:30 build",
            "port localhost:13:41",
            "node:22 and 10:30:99",
            "v2:13:41",
            "a1:30 b",
        ] {
            assert_eq!(bare(not_a_time), None, "{not_a_time}");
        }
        // Code spans reach this check blanked.
        let runs = markdown_runs("Run `at 13:41` now.");
        assert_eq!(bare(&runs[0].prose), None);
    }

    #[test]
    fn ellipsis_skips_quotes_and_numeric_ranges() {
        assert!(cut_ellipsis(
            "the milestone fixes and the quality defaults that..."
        ));
        assert!(cut_ellipsis("cut\u{2026} here"));
        assert!(!cut_ellipsis(
            "the episode \u{201c}Wait... What?\u{201d} aired"
        ));
        assert!(!cut_ellipsis("the title \"Wait... What?\" stays"));
        assert!(!cut_ellipsis("pages 1...5 and 3 \u{2026} 9"));
        assert!(!cut_ellipsis("no ellipsis here."));
        // An unclosed quote blanks nothing.
        assert!(cut_ellipsis("he said \"wait..."));
    }

    #[test]
    fn budget_counts_characters_and_hard_breaks_fill_their_line() {
        assert_eq!(budget_chars(&"a".repeat(CHAR_BUDGET)), CHAR_BUDGET);
        assert_eq!(budget_chars("one\ntwo\nthree"), 2 * MEASURE_CHARS + 5);
        assert_eq!(
            budget_chars(&format!("{}\nb", "a".repeat(70))),
            2 * MEASURE_CHARS + 1
        );
        // Long words count by characters: the budget is not a line count.
        let long_words = "internationalization ".repeat(6);
        assert_eq!(budget_chars(long_words.trim()), 125);
    }

    #[test]
    fn markdown_runs_split_blocks_and_leave_out_code() {
        let runs = markdown_runs(
            "## Heading\n\nFirst `a...b` para.\n\n- one\n- [table:t] two\n\n```\n13:41 ...\n```\n",
        );
        let texts: Vec<_> = runs
            .iter()
            .map(|run| {
                (
                    run.text.as_str(),
                    run.prose.as_str(),
                    run.measure == Measure::Block,
                )
            })
            .collect();
        assert_eq!(
            texts,
            [
                ("Heading", "Heading", false),
                ("First a...b para.", "First       para.", true),
                ("one", "one", true),
                ("Table 1 two", "Table 1 two", true)
            ]
        );
    }

    #[test]
    fn markdown_units_count_paragraphs_and_lists_and_break_on_code() {
        let units =
            markdown_units("## Head\n\nOne.\n\nTwo.\n\n- a\n- b\n\n```\nx\n```\n\nThree.\n");
        let shape: Vec<bool> = units.iter().map(Option::is_some).collect();
        assert_eq!(shape, [true, true, true, false, true]);
    }

    #[test]
    fn html_text_runs_join_inline_text_and_keep_blocks_apart() {
        let runs = html_text_runs(
            "<svg><title>Hidden 13:41</title><text>Read 13:41</text><text>EST</text><code>9:00</code></svg>",
        );
        assert_eq!(runs, ["Read 13:41", "EST"]);
        assert_eq!(bare(&runs[0]), Some("13:41".into()));
        // Inline formatting stays in its sentence, so the zone is read.
        let runs = html_text_runs("<div><p>Read at <strong>13:41</strong> EST</p></div>");
        assert_eq!(runs, ["Read at 13:41 EST"]);
        assert_eq!(bare(&runs[0]), None);
        // Separate boxes are separate runs; a hard break is a space.
        let runs = html_text_runs("<div><div>Read 13:41</div><div>EST</div>one<br>two</div>");
        assert_eq!(runs, ["Read 13:41", "EST", "one two"]);
        assert_eq!(bare(&runs[0]), Some("13:41".into()));
    }

    #[test]
    fn page_findings_measure_paragraphs_items_and_runs() {
        let text = |value: &str| PageText {
            text: value.to_string(),
            prose: value.to_string(),
        };
        let long = "word ".repeat(30);
        let blocks = [
            PageBlock::Paragraph(text("One short lead at 13:41.")),
            PageBlock::Paragraph(text(long.trim())),
            PageBlock::List(vec![text("a"), text("items cut to that...")]),
            PageBlock::Paragraph(PageText {
                text: "Run `at 13:41 ...` now.".to_string(),
                prose: "Run                   now.".to_string(),
            }),
            PageBlock::Visual,
            PageBlock::Paragraph(text("After a figure.")),
        ];
        let rules: Vec<_> = page_findings(&blocks).iter().map(|f| f.rule).collect();
        assert_eq!(
            rules,
            [
                "copy-time-zone",
                "copy-density",
                "copy-ellipsis",
                "copy-run"
            ]
        );
        let clean = [
            PageBlock::Paragraph(text("Lead.")),
            PageBlock::Visual,
            PageBlock::List(vec![text("one")]),
        ];
        assert!(page_findings(&clean).is_empty());
    }
}
