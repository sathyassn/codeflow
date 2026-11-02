//! Copy checks for `present check` (TSK-1255): a visible text block longer
//! than two lines, a sentence cut to an ellipsis, and a clock time without a
//! zone. They are warnings, so `open` and `update` print them and proceed.
//!
//! Lines are counted in characters at the reading measure. The runtime sets
//! prose in a column of `--cf-measure: 68ch` (`web/src/styles.css`), and one
//! `ch` is the width of the digit zero, which is at least the average width
//! of a character of running text in the shipped sans faces. A run of 68
//! characters therefore fits on one desktop line or wraps early, so the count
//! never reports fewer lines than the page shows at that measure. A phone
//! column is narrower, so a block within the limit can take three or four
//! lines there. A hard break starts a new line.
use pulldown_cmark::{Event, Tag, TagEnd};
use scraper::{ElementRef, Html};

use crate::document::{
    coalesced_markdown, reference_segments, Block, PresentationDocument, TextSegment,
};

/// Characters on one line of the default reading measure (68ch).
pub(crate) const MEASURE_CHARS: usize = 68;
/// Lines a visible text block may take outside collapsed detail.
pub(crate) const MAX_BLOCK_LINES: usize = 2;

/// One copy finding: the block it names, its rule and the message.
pub(crate) struct CopyFinding {
    pub block_id: Option<String>,
    pub rule: &'static str,
    pub message: String,
}

/// Every copy finding for a document, in block order.
pub(crate) fn copy_findings(document: &PresentationDocument) -> Vec<CopyFinding> {
    let mut findings = Vec::new();
    let mut visit = |block_id: Option<&str>, text: &Text<'_>, visible: bool| {
        let block_id = block_id.map(str::to_string);
        for run in text.runs() {
            if visible && run.dense {
                let lines = lines_at_measure(&run.text);
                if lines > MAX_BLOCK_LINES {
                    findings.push(CopyFinding {
                        block_id: block_id.clone(),
                        rule: "copy-density",
                        message: format!(
                            "a text block takes {lines} lines at the {MEASURE_CHARS}-character reading measure ({} characters), over the limit of {MAX_BLOCK_LINES}; keep the lead in two lines and move the rest to bullets, a figure or a disclosure: \"{}\"",
                            run.text.chars().count(),
                            opening(&run.text)
                        ),
                    });
                }
            }
            if run.prose.contains("...") || run.prose.contains('\u{2026}') {
                findings.push(CopyFinding {
                    block_id: block_id.clone(),
                    rule: "copy-ellipsis",
                    message: format!(
                        "visible text is cut to an ellipsis; keep the context whole, wrapped or behind a disclosure: \"{}\"",
                        opening(&run.text)
                    ),
                });
            }
            if let Some(time) = bare_clock_time(&run.prose) {
                findings.push(CopyFinding {
                    block_id: block_id.clone(),
                    rule: "copy-time-zone",
                    message: format!(
                        "the time {time} has no zone; write the wall-clock time with its zone, such as \"16:12 EST, 2 Nov\""
                    ),
                });
            }
        }
    };
    visit(None, &Text::Plain(&document.title, false), true);
    if let Some(summary) = &document.summary {
        visit(None, &Text::Plain(summary, true), true);
    }
    visit_blocks(&document.blocks, true, &mut visit);
    findings
}

/// One string a reader sees, and how to read it.
enum Text<'a> {
    /// Markdown: each paragraph, list item and heading is its own run; code
    /// blocks and code spans are left out.
    Markdown(&'a str),
    /// A plain string; `true` when it is a text block whose length counts.
    Plain(&'a str, bool),
    /// An HTML stage: its text outside code, scripts and styles. A stage is
    /// a figure, so its length never counts.
    Html(&'a str),
}

struct Run {
    /// The text as the page shows it.
    text: String,
    /// The same text with each code span blanked, for the ellipsis and time
    /// checks: a code span is literal.
    prose: String,
    dense: bool,
}

impl Run {
    fn plain(text: String, dense: bool) -> Self {
        Self {
            prose: text.clone(),
            text,
            dense,
        }
    }
}

impl Text<'_> {
    fn runs(&self) -> Vec<Run> {
        match self {
            Self::Markdown(markdown) => markdown_runs(markdown),
            Self::Plain(text, dense) => vec![Run::plain((*text).to_string(), *dense)],
            Self::Html(html) => vec![Run::plain(html_text(html), false)],
        }
    }
}

fn visit_blocks(
    blocks: &[Block],
    visible: bool,
    visit: &mut impl FnMut(Option<&str>, &Text<'_>, bool),
) {
    for block in blocks {
        let id = Some(block.id());
        let mut plain = |text: &str, dense: bool| visit(id, &Text::Plain(text, dense), visible);
        match block {
            Block::Callout { title, .. } => {
                if let Some(title) = title {
                    plain(title, false);
                }
            }
            Block::Decision { title, .. } | Block::Form { title, .. } => plain(title, false),
            Block::Comparison { columns, .. } => {
                for column in columns {
                    plain(&column.title, false);
                }
            }
            Block::Table {
                columns,
                title,
                caption,
                ..
            } => {
                for column in columns {
                    plain(column, false);
                }
                if let Some(title) = title {
                    plain(title, false);
                }
                if let Some(caption) = caption {
                    plain(caption, true);
                }
            }
            Block::Status { items, .. } => {
                for item in items {
                    plain(&item.label, false);
                    if let Some(detail) = &item.detail {
                        plain(detail, true);
                    }
                }
            }
            Block::Code { caption, .. }
            | Block::Diff { caption, .. }
            | Block::Media { caption, .. } => {
                if let Some(caption) = caption {
                    plain(caption, true);
                }
            }
            Block::Tree { label, nodes, .. } => {
                plain(label, false);
                let mut stack: Vec<_> = nodes.iter().collect();
                while let Some(node) = stack.pop() {
                    plain(&node.label, false);
                    stack.extend(node.children.iter());
                }
            }
            Block::FeedbackPrompt { prompt, .. } => plain(prompt, true),
            Block::Html {
                title,
                caption,
                legend,
                ..
            } => {
                if let Some(title) = title {
                    plain(title, false);
                }
                if let Some(caption) = caption {
                    plain(caption, true);
                }
                for entry in legend.iter().flatten() {
                    plain(&entry.label, false);
                    plain(&entry.means, false);
                }
            }
            Block::Disclosure { summary, .. } => plain(summary, false),
            Block::Tabs { title, tabs, .. } => {
                if let Some(title) = title {
                    plain(title, false);
                }
                for tab in tabs {
                    plain(&tab.label, false);
                }
            }
            _ => {}
        }
        for markdown in block.markdown_fields() {
            visit(id, &Text::Markdown(markdown), visible);
        }
        match block {
            Block::Html {
                html, description, ..
            } => {
                visit(id, &Text::Html(html), visible);
                // The description sits in the stage's Details disclosure.
                if let Some(description) = description {
                    visit(id, &Text::Plain(description, true), false);
                }
            }
            // Collapsed detail: cut context and bare times still count.
            Block::Disclosure { blocks, .. } => visit_blocks(blocks, false, visit),
            Block::Tabs { tabs, .. } => {
                for tab in tabs {
                    visit_blocks(&tab.blocks, visible, visit);
                }
            }
            _ => {}
        }
    }
}

/// The visible text runs of a Markdown field: one per paragraph, list item
/// or heading, with soft breaks as spaces and hard breaks as newlines.
fn markdown_runs(markdown: &str) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut current = (String::new(), String::new());
    let mut code_block = 0_usize;
    let mut heading = false;
    let flush = |current: &mut (String, String), runs: &mut Vec<Run>, dense: bool| {
        if !current.0.trim().is_empty() {
            runs.push(Run {
                text: current.0.trim().to_string(),
                prose: current.1.trim().to_string(),
                dense,
            });
        }
        current.0.clear();
        current.1.clear();
    };
    for (event, in_link) in coalesced_markdown(markdown) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code_block += 1,
            Event::End(TagEnd::CodeBlock) => code_block = code_block.saturating_sub(1),
            Event::Start(Tag::Heading { .. }) => {
                flush(&mut current, &mut runs, true);
                heading = true;
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(&mut current, &mut runs, false);
                heading = false;
            }
            Event::Start(Tag::Paragraph | Tag::Item)
            | Event::End(TagEnd::Paragraph | TagEnd::Item) => {
                flush(&mut current, &mut runs, !heading);
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
    flush(&mut current, &mut runs, !heading);
    runs
}

/// Visible text of an HTML stage outside `code`, `pre`, `script` and
/// `style`.
fn html_text(source: &str) -> String {
    let fragment = Html::parse_fragment(source);
    fragment
        .tree
        .nodes()
        .filter(|node| {
            !node.ancestors().filter_map(ElementRef::wrap).any(|parent| {
                matches!(
                    parent.value().name(),
                    "style" | "script" | "code" | "pre" | "kbd" | "samp"
                )
            })
        })
        .filter_map(|node| node.value().as_text())
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Lines a run takes at the reading measure: each hard-break segment wraps
/// on its own.
fn lines_at_measure(text: &str) -> usize {
    text.split('\n')
        .map(|segment| {
            segment
                .trim()
                .chars()
                .count()
                .div_ceil(MEASURE_CHARS)
                .max(1)
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

/// The first clock time in `text` that names no zone, such as "13:41" in
/// "read at 13:41". A time followed by `Z`, a numeric offset or an
/// uppercase zone word ("EST", "UTC+1") is zoned; ratios ("4.5:1", "3:1")
/// and out-of-range values are not times.
pub(crate) fn bare_clock_time(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let Some(found) = clock_at(text, index) else {
            index += 1;
            continue;
        };
        if !found.zoned {
            return Some(text[index..found.end].to_string());
        }
        index = found.end;
    }
    None
}

struct Clock {
    end: usize,
    zoned: bool,
}

fn clock_at(text: &str, start: usize) -> Option<Clock> {
    let bytes = text.as_bytes();
    if !bytes[start].is_ascii_digit() {
        return None;
    }
    if start > 0 {
        let before = bytes[start - 1];
        // "T" joins a date to its time in ISO 8601 ("2026-11-02T13:41").
        let iso = before == b'T' && start > 1 && bytes[start - 2].is_ascii_digit();
        // An offset ("+01:00") is part of the time before it, never a time.
        if !iso && (before.is_ascii_alphanumeric() || matches!(before, b'.' | b':' | b',' | b'+')) {
            return None;
        }
        if before == b'-' && start > 1 && bytes[start - 2].is_ascii_digit() {
            return None;
        }
    }
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let hour_len = digits(start);
    if !(1..=2).contains(&hour_len) || bytes.get(start + hour_len) != Some(&b':') {
        return None;
    }
    let minute = start + hour_len + 1;
    if digits(minute) != 2 {
        return None;
    }
    let mut end = minute + 2;
    if bytes.get(end) == Some(&b':') && digits(end + 1) == 2 {
        end += 3;
        if bytes.get(end) == Some(&b'.') {
            end += 1 + digits(end + 1);
        }
    }
    let hour: u32 = text[start..start + hour_len].parse().ok()?;
    let minutes: u32 = text[minute..minute + 2].parse().ok()?;
    if hour > 23 || minutes > 59 {
        return None;
    }
    Some(Clock {
        end,
        zoned: zone_follows(&text[end..]),
    })
}

/// Whether the text right after a clock time names its zone.
fn zone_follows(after: &str) -> bool {
    let bytes = after.as_bytes();
    match bytes.first() {
        Some(b'Z') => return !bytes.get(1).is_some_and(u8::is_ascii_alphanumeric),
        Some(b'+' | b'-') => return bytes.get(1).is_some_and(u8::is_ascii_digit),
        _ => {}
    }
    let mut rest = after.trim_start();
    for meridiem in ["a.m.", "p.m.", "am", "pm", "AM", "PM"] {
        if let Some(tail) = rest.strip_prefix(meridiem) {
            if !tail.starts_with(|c: char| c.is_ascii_alphanumeric()) {
                rest = tail.trim_start();
                break;
            }
        }
    }
    let rest = rest.strip_prefix('(').unwrap_or(rest);
    let letters = rest.bytes().take_while(u8::is_ascii_uppercase).count();
    (1..=5).contains(&letters) && !rest[letters..].starts_with(|c: char| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_clock_time_reads_zones_and_skips_ratios() {
        assert_eq!(bare_clock_time("read at 13:41"), Some("13:41".to_string()));
        assert_eq!(bare_clock_time("at 7:05 today"), Some("7:05".to_string()));
        assert_eq!(bare_clock_time("13:41 EST, 2 Nov"), None);
        assert_eq!(bare_clock_time("13:41:07 UTC"), None);
        assert_eq!(bare_clock_time("08:15Z"), None);
        assert_eq!(bare_clock_time("08:15+01:00 and 09:00-0500"), None);
        assert_eq!(bare_clock_time("a floor of 4.5:1 and 3:1"), None);
        assert_eq!(bare_clock_time("99:00 is not a time"), None);
        assert_eq!(bare_clock_time("13:41 ESTIMATE"), Some("13:41".to_string()));
        assert_eq!(bare_clock_time("4:30 pm EST"), None);
        assert_eq!(bare_clock_time("4:30 pm today"), Some("4:30".to_string()));
        assert_eq!(bare_clock_time("2026-11-02T13:41:07Z"), None);
        assert_eq!(
            bare_clock_time("2026-11-02T13:41"),
            Some("13:41".to_string())
        );
        assert_eq!(bare_clock_time("a 16:9 frame"), None);
    }

    #[test]
    fn lines_count_characters_at_the_measure_and_hard_breaks() {
        assert_eq!(lines_at_measure(&"a".repeat(MEASURE_CHARS)), 1);
        assert_eq!(lines_at_measure(&"a".repeat(MEASURE_CHARS + 1)), 2);
        assert_eq!(lines_at_measure(&"a".repeat(MEASURE_CHARS * 2 + 1)), 3);
        assert_eq!(lines_at_measure("one\ntwo\nthree"), 3);
    }

    #[test]
    fn markdown_runs_split_blocks_and_leave_out_code() {
        let runs = markdown_runs(
            "## Heading\n\nFirst `a...b` para.\n\n- one\n- [table:t] two\n\n```\n13:41 ...\n```\n",
        );
        let texts: Vec<_> = runs
            .iter()
            .map(|run| (run.text.as_str(), run.prose.as_str(), run.dense))
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
}
