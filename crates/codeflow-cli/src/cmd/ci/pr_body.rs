//! Markdown-aware PR sections and advisory presentation/release checks.

use codeflow_core::hooks::{adoption, GitPolicy, PolicyLevel, Violation};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::SectionState;

struct Section<'a> {
    name: String,
    depth: HeadingLevel,
    start: usize,
    heading_start: usize,
    end: usize,
    body: &'a str,
}

/// Document headings, plus block HTML containers that never closed.
struct Outline<'a> {
    sections: Vec<Section<'a>>,
    unclosed: Vec<String>,
}

/// Only document headings count, not examples in lists, quotes, code or a
/// closed HTML container. A subsection belongs to its parent until a sibling
/// or ancestor starts.
fn sections(body: &str) -> Vec<Section<'_>> {
    outline(body).sections
}

/// A heading inside a block HTML container is an example only when that
/// container closes later. An unclosed container hides nothing: hosts render
/// the later headings, so they still count, and presentation warns instead.
fn outline(body: &str) -> Outline<'_> {
    // (depth, name, heading start, content start, containers open at the heading)
    let mut candidates: Vec<(HeadingLevel, String, usize, usize, Vec<usize>)> = Vec::new();
    let mut nesting: usize = 0;
    let mut heading: Option<(HeadingLevel, String, usize)> = None;
    let mut html = HtmlContainers::default();
    for (event, span) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. })
                if nesting == 0 && atx_heading(body, span.start) =>
            {
                heading = Some((level, String::new(), span.start));
            }
            Event::End(TagEnd::Heading(_)) if nesting == 0 => {
                if let Some((depth, name, heading_start)) = heading.take() {
                    candidates.push((depth, name, heading_start, span.end, html.open_ids()));
                }
            }
            Event::Text(text) | Event::Code(text) if heading.is_some() => {
                heading.as_mut().unwrap().1.push_str(&text);
            }
            // Only document-level HTML blocks can enclose document headings;
            // inline HTML such as `Vec<String>` or `<path>` in prose never does.
            Event::Html(value) if nesting == 1 => html.observe(&value),
            Event::End(TagEnd::HtmlBlock) => {
                nesting = nesting.saturating_sub(1);
                html.pending.clear();
            }
            Event::Start(_) => nesting += 1,
            Event::End(_) => nesting = nesting.saturating_sub(1),
            _ => {}
        }
    }
    let visible: Vec<_> = candidates
        .into_iter()
        .filter(|candidate| !candidate.4.iter().any(|id| html.closed[*id]))
        .collect();
    let sections = visible
        .iter()
        .enumerate()
        .map(|(index, (depth, name, heading_start, start, _))| Section {
            name: name.clone(),
            depth: *depth,
            heading_start: *heading_start,
            start: *start,
            end: visible[index + 1..]
                .iter()
                .find(|next| next.0 <= *depth)
                .map_or(body.len(), |next| next.2),
            body,
        })
        .collect();
    let unclosed = html.open.into_iter().map(|(name, _)| name).collect();
    Outline { sections, unclosed }
}

/// ATX section headings must start at column zero. Setext headings are prose
/// for this contract, even though `CommonMark` parses them as headings.
fn atx_heading(body: &str, offset: usize) -> bool {
    let line_start = body[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    offset == line_start && body[offset..].starts_with('#')
}

fn matching_sections<'s, 'b>(sections: &'s [Section<'b>], name: &str) -> Vec<&'s Section<'b>> {
    let depth = if sections
        .iter()
        .any(|s| s.matches(name) && s.depth == HeadingLevel::H2)
    {
        HeadingLevel::H2
    } else {
        HeadingLevel::H3
    };
    sections
        .iter()
        .filter(|s| s.matches(name) && s.depth == depth)
        .collect()
}

/// `CommonMark` ends an HTML block at a blank line. Keep the enclosing HTML
/// containers across that boundary so Markdown examples inside them cannot
/// declare top-level PR sections. Only document-level HTML blocks are
/// inspected, and only block containers or custom elements are tracked.
#[derive(Default)]
struct HtmlContainers {
    /// Open containers as (tag name, instance id).
    open: Vec<(String, usize)>,
    /// Per instance id: whether a closing tag was seen.
    closed: Vec<bool>,
    pending: String,
}

/// Block-level elements that can wrap Markdown content. Custom elements
/// (a hyphen in the name) are containers too; inline tags never are.
fn html_container(name: &str) -> bool {
    name.contains('-')
        || matches!(
            name,
            "address"
                | "article"
                | "aside"
                | "blockquote"
                | "center"
                | "dd"
                | "details"
                | "dialog"
                | "div"
                | "dl"
                | "dt"
                | "fieldset"
                | "figcaption"
                | "figure"
                | "footer"
                | "form"
                | "header"
                | "li"
                | "main"
                | "nav"
                | "ol"
                | "p"
                | "pre"
                | "section"
                | "summary"
                | "table"
                | "tbody"
                | "td"
                | "tfoot"
                | "th"
                | "thead"
                | "tr"
                | "ul"
        )
}

impl HtmlContainers {
    fn open_ids(&self) -> Vec<usize> {
        self.open.iter().map(|(_, id)| *id).collect()
    }

    fn observe(&mut self, html: &str) {
        // The Markdown parser may emit one HTML event per source line, even
        // within a single comment or tag. Keep unfinished tokens across events.
        self.pending.push_str(html);
        let pending = std::mem::take(&mut self.pending);
        let mut rest = pending.as_str();
        loop {
            let Some(start) = rest.find('<') else {
                rest = "";
                break;
            };
            rest = &rest[start..];
            if rest.starts_with("<!--") {
                let Some(end) = rest.find("-->") else { break };
                rest = &rest[end + 3..];
                continue;
            }
            let token = &rest[1..];
            // `a < b` in HTML text is not a tag.
            if !token.starts_with(|ch: char| ch == '/' || ch.is_ascii_alphabetic()) {
                rest = token;
                continue;
            }
            let mut quote = None;
            let end = token.char_indices().find_map(|(index, ch)| {
                match (quote, ch) {
                    (None, '"' | '\'') => quote = Some(ch),
                    (Some(open), close) if open == close => quote = None,
                    (None, '>') => return Some(index),
                    _ => {}
                }
                None
            });
            let Some(end) = end else { break };
            let tag = &token[..end];
            let name = tag
                .trim_start_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_end_matches('/')
                .to_ascii_lowercase();
            if tag.starts_with('/') {
                if let Some(index) = self.open.iter().rposition(|(open, _)| *open == name) {
                    // Closing an outer element also ends any inner one left open.
                    for (_, id) in self.open.drain(index..) {
                        self.closed[id] = true;
                    }
                }
            } else if !tag.trim_end().ends_with('/') && html_container(&name) {
                self.open.push((name, self.closed.len()));
                self.closed.push(false);
            }
            rest = &token[end + 1..];
        }
        self.pending = rest.to_string();
    }
}

impl Section<'_> {
    fn content(&self) -> &str {
        &self.body[self.start..self.end]
    }

    fn matches(&self, name: &str) -> bool {
        matches!(self.depth, HeadingLevel::H2 | HeadingLevel::H3)
            && self.name.trim().eq_ignore_ascii_case(name)
    }
}

/// Render visible text for content and field checks. HTML comments and heading
/// labels alone are not evidence. Code output counts as content, but cannot
/// manufacture release fields or a Not tested declaration.
fn visible_text(body: &str, include_code: bool) -> String {
    rendered_text(body, include_code, true)
}

/// The lines a Release impact field may come from: visible text outside
/// code and outside quotes, since a quoted field is an example taken from
/// elsewhere, not this change's assessment (TSK-147 F4).
fn field_text(body: &str) -> String {
    rendered_text(body, false, false)
}

fn rendered_text(body: &str, include_code: bool, include_quotes: bool) -> String {
    let mut text = String::new();
    let mut excluded = 0;
    let mut heading_excluded = false;
    for (event, span) in Parser::new(body).into_offset_iter() {
        // A block that starts inside a tight list item follows the item's
        // text with no paragraph end between them, so it ends the line
        // itself; otherwise `- Impact: minor` over `  - Breaking: no` reads
        // as one line (TSK-147 round 3, the differential corpus).
        if excluded == 0 && starts_block(&event) && !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        match event {
            Event::Start(Tag::Heading { .. }) => {
                heading_excluded = atx_heading(body, span.start);
                excluded += usize::from(heading_excluded);
            }
            Event::Start(Tag::CodeBlock(_)) if !include_code => excluded += 1,
            Event::Start(Tag::BlockQuote(_)) if !include_quotes => excluded += 1,
            Event::End(TagEnd::BlockQuote(_)) if !include_quotes => excluded -= 1,
            Event::End(TagEnd::Heading(_)) => {
                excluded -= usize::from(heading_excluded);
                if !heading_excluded {
                    text.push('\n');
                }
            }
            Event::End(TagEnd::CodeBlock) if !include_code => excluded -= 1,
            Event::Text(value) | Event::Code(value) if excluded == 0 => text.push_str(&value),
            Event::End(
                TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Superscript
                | TagEnd::Subscript
                | TagEnd::Link
                | TagEnd::Image,
            ) => {}
            // Block ends and breaks end a line; inline ends such as bold do not.
            Event::SoftBreak | Event::HardBreak | Event::End(_) if excluded == 0 => {
                text.push('\n');
            }
            _ => {}
        }
    }
    text
}

/// Whether `event` opens a block, which begins a new line of text.
fn starts_block(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::Rule
            | Event::Start(
                Tag::Paragraph
                    | Tag::Heading { .. }
                    | Tag::BlockQuote(_)
                    | Tag::CodeBlock(_)
                    | Tag::HtmlBlock
                    | Tag::List(_)
                    | Tag::Item
                    | Tag::Table(_)
                    | Tag::FootnoteDefinition(_)
                    | Tag::DefinitionList
            )
    )
}

/// Visible text or raw HTML other than comments; template placeholders are
/// comments, so an unfilled section stays empty.
pub(super) fn has_content(body: &str) -> bool {
    if !visible_text(body, true).trim().is_empty() {
        return true;
    }
    let html: String = Parser::new(body)
        .filter_map(|event| match event {
            Event::Html(html) | Event::InlineHtml(html) => Some(html.into_string()),
            _ => None,
        })
        .collect();
    let mut rest = html.as_str();
    while let Some(start) = rest.find("<!--") {
        if !rest[..start].trim().is_empty() {
            return true;
        }
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return false,
        }
    }
    !rest.trim().is_empty()
}

pub(super) fn find_section(body: &str, name: &str) -> SectionState {
    let sections = sections(body);
    let matches = matching_sections(&sections, name);
    match matches.as_slice() {
        [] => SectionState::Missing,
        [section] if has_content(section.content()) => SectionState::Present,
        [_] => SectionState::Empty,
        _ => SectionState::Duplicate,
    }
}

/// A current review row names the exact revision and an approving verdict.
/// Fences, quotes, examples and HTML comments cannot supply the evidence.
pub(crate) fn review_names_revision(body: &str, heading: &str, sha: &str) -> bool {
    let outline = sections(body);
    let matching = matching_sections(&outline, heading);
    let [section] = matching.as_slice() else {
        return false;
    };
    rendered_text(section.content(), false, false)
        .lines()
        .any(|line| {
            let cells: Vec<_> = line
                .split('|')
                .map(str::trim)
                .filter(|cell| !cell.is_empty())
                .collect();
            cells.len() >= 3
                && cells.last().is_some_and(|verdict| {
                    matches!(
                        verdict.to_ascii_lowercase().as_str(),
                        "approved" | "approve"
                    )
                })
                && cells[1..cells.len() - 1].iter().any(|scope| {
                    scope
                        .split(|c: char| !c.is_ascii_hexdigit())
                        .any(|token| token == sha)
                })
        })
}

/// A body over this many words draws the length warning (TSK-228).
const BODY_WORD_LIMIT: usize = 1000;

/// HTML elements that sit inside a line of text; any other tag, `<br>` and
/// `<p>` included, separates the words around it.
const INLINE_TAGS: &[&str] = &[
    "a", "abbr", "b", "bdo", "big", "cite", "code", "del", "dfn", "em", "font", "i", "img", "ins",
    "kbd", "mark", "q", "s", "samp", "small", "span", "strike", "strong", "sub", "sup", "tt", "u",
    "var",
];

/// The text a reader sees in raw HTML: comments and tags are gone, a tag
/// that is not inline separates its neighbours, and entities are decoded.
fn html_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if let Some(after) = rest.strip_prefix("<!--") {
            // An unterminated comment hides the rest of the block.
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            continue;
        }
        let tag_like = rest[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '/');
        if let Some(end) = tag_end(rest).filter(|_| tag_like) {
            let name = rest[1..end]
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !INLINE_TAGS.contains(&name.as_str()) {
                out.push('\n');
            }
            rest = &rest[end + 1..];
        } else {
            out.push('<');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    decode_entities(&out)
}

/// The offset of the `>` that ends the tag `html` opens with, skipping any
/// `>` inside a quoted attribute value; `None` when the tag never ends.
fn tag_end(html: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (at, ch) in html.char_indices() {
        match (quote, ch) {
            (None, '>') => return Some(at),
            (None, '"' | '\'') => quote = Some(ch),
            (Some(open), _) if ch == open => quote = None,
            _ => {}
        }
    }
    None
}

/// What the HTML character reference at the start of `rest` (which begins
/// with `&`) stands for, and the bytes it takes. The named references cover
/// the markup characters and every space the HTML specification names, so
/// each separates words as it does on the page; any other reference stays
/// as written.
fn reference(rest: &str) -> Option<(&'static str, usize)> {
    let end = rest.bytes().take(33).position(|byte| byte == b';');
    if let Some(end) = end {
        let named = match &rest[1..end] {
            "amp" => Some("&"),
            "lt" => Some("<"),
            "gt" => Some(">"),
            "quot" => Some("\""),
            "apos" => Some("'"),
            "nbsp" | "NonBreakingSpace" => Some("\u{a0}"),
            "ensp" => Some("\u{2002}"),
            "emsp" => Some("\u{2003}"),
            "emsp13" => Some("\u{2004}"),
            "emsp14" => Some("\u{2005}"),
            "numsp" => Some("\u{2007}"),
            "puncsp" => Some("\u{2008}"),
            "thinsp" | "ThinSpace" => Some("\u{2009}"),
            "hairsp" | "VeryThinSpace" => Some("\u{200a}"),
            "MediumSpace" => Some("\u{205f}"),
            "ThickSpace" => Some("\u{205f}\u{200a}"),
            "Tab" => Some("\t"),
            "NewLine" => Some("\n"),
            _ => None,
        };
        if let Some(text) = named {
            return Some((text, end + 1));
        }
        // A numeric reference, padded with zeros or not, that stands for a
        // space; any other character stays as written.
        let code = rest[1..end].strip_prefix('#').and_then(|digits| {
            match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => digits.parse().ok(),
            }
        });
        let spaced = code
            .and_then(char::from_u32)
            .filter(|ch| ch.is_whitespace());
        if let Some(ch) = spaced {
            return Some((
                match ch {
                    '\t' => "\t",
                    '\n' => "\n",
                    '\u{a0}' => "\u{a0}",
                    _ => " ",
                },
                end + 1,
            ));
        }
    }
    // HTML also reads `&nbsp` with no semicolon.
    rest.starts_with("&nbsp").then_some(("\u{a0}", 5))
}

/// Decode the character references an HTML block can carry.
fn decode_entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if let Some((decoded, taken)) = reference(rest) {
            out.push_str(decoded);
            rest = &rest[taken..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// The text of a body as a reader sees it rendered, with the source offset
/// each stretch of it came from, so a section's words are read from the
/// one parse of the whole body: a reference link or footnote defined in
/// another section still resolves.
struct ReaderText {
    text: String,
    /// `(source offset, text length so far)`, in order.
    marks: Vec<(usize, usize)>,
}

impl ReaderText {
    /// Headings, tables and fenced blocks count; HTML comments, tag markup,
    /// image alt text and footnote definitions nothing refers to do not.
    fn of(source: &str) -> Self {
        let options = github_options();
        let referenced: std::collections::HashSet<String> = Parser::new_ext(source, options)
            .filter_map(|event| match event {
                Event::FootnoteReference(label) => Some(label.into_string()),
                _ => None,
            })
            .collect();
        let mut reader = Self {
            text: String::new(),
            marks: Vec::new(),
        };
        let mut html = String::new();
        let mut html_at = 0;
        let mut image = 0usize;
        let mut hidden_definition = false;
        for (event, span) in Parser::new_ext(source, options).into_offset_iter() {
            match event {
                Event::Html(value) | Event::InlineHtml(value) => {
                    if html.is_empty() {
                        html_at = span.start;
                    }
                    html.push_str(&value);
                    continue;
                }
                Event::SoftBreak | Event::HardBreak if !html.is_empty() => {
                    html.push('\n');
                    continue;
                }
                _ => {}
            }
            if !html.is_empty() {
                if !hidden_definition {
                    reader.mark(html_at);
                    reader.text.push_str(&html_text(&html));
                }
                html.clear();
            }
            match event {
                Event::Start(Tag::FootnoteDefinition(label)) => {
                    hidden_definition = !referenced.contains(label.as_ref());
                }
                Event::End(TagEnd::FootnoteDefinition) => hidden_definition = false,
                _ if hidden_definition => {}
                event => {
                    // An End event spans its whole element; its end keeps
                    // the offsets in order.
                    reader.mark(if matches!(event, Event::End(_)) {
                        span.end
                    } else {
                        span.start
                    });
                    reader.event(event, &mut image);
                }
            }
        }
        if !html.is_empty() && !hidden_definition {
            reader.mark(html_at);
            reader.text.push_str(&html_text(&html));
        }
        reader
    }

    fn mark(&mut self, offset: usize) {
        self.marks.push((offset, self.text.len()));
    }

    fn event(&mut self, event: Event<'_>, image: &mut usize) {
        if starts_block(&event) && !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
        match event {
            Event::Start(Tag::Image { .. }) => *image += 1,
            Event::End(TagEnd::Image) => *image = image.saturating_sub(1),
            Event::Text(value) | Event::Code(value) if *image == 0 => {
                self.text.push_str(&value);
            }
            // Inline ends join the words either side of them; block and
            // table ends and breaks separate them.
            Event::End(
                TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Superscript
                | TagEnd::Subscript
                | TagEnd::Link,
            ) => {}
            Event::SoftBreak | Event::HardBreak | Event::End(_) => self.text.push('\n'),
            _ => {}
        }
    }

    /// The words of the whole body.
    fn words(&self) -> usize {
        self.text.split_whitespace().count()
    }

    /// The words that came from source offsets `start..end`.
    fn words_in(&self, start: usize, end: usize) -> usize {
        let at = |offset: usize| {
            let index = self.marks.partition_point(|mark| mark.0 < offset);
            self.marks.get(index).map_or(self.text.len(), |mark| mark.1)
        };
        self.text[at(start)..at(end)].split_whitespace().count()
    }
}

/// The words of `source` as a reader sees them.
#[cfg(test)]
fn word_count(source: &str) -> usize {
    ReaderText::of(source).words()
}

/// The length warning's message for an over-long body: the count, the limit
/// and the three largest `##` sections with their word counts. `None` at or
/// under the limit.
fn length_message(body: &str, sections: &[Section<'_>]) -> Option<String> {
    let reader = ReaderText::of(body);
    let total = reader.words();
    if total <= BODY_WORD_LIMIT {
        return None;
    }
    let mut largest: Vec<(&str, usize)> = sections
        .iter()
        .filter(|section| section.depth == HeadingLevel::H2)
        .map(|section| {
            (
                section.name.trim(),
                reader.words_in(section.start, section.end),
            )
        })
        .filter(|(_, words)| *words > 0)
        .collect();
    // Stable, so equal sections keep their order in the body.
    largest.sort_by(|a, b| b.1.cmp(&a.1));
    let named = largest
        .iter()
        .take(3)
        .map(|(name, words)| format!("## {name} ({words} words)"))
        .collect::<Vec<_>>()
        .join(", ");
    let sections = if named.is_empty() {
        String::new()
    } else {
        format!("; largest sections: {named}")
    };
    Some(format!(
        "PR body is {total} words, over the {BODY_WORD_LIMIT}-word limit (HTML comments not counted){sections}"
    ))
}

pub(super) fn presentation(git: &GitPolicy, body: &str, _protected: bool) -> Vec<Violation> {
    if !git.pr_sections.is_active() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut warn = |message: String| {
        out.push(Violation::new(
            "git.pr_sections",
            PolicyLevel::Warn,
            message,
            codeflow_core::remedy::PR_PRESENTATION.remedy(),
        ));
    };
    let outline = outline(body);
    // TSK-228: a body over the word limit warns and never blocks; this check
    // raises every finding at warn, whatever `git.pr_sections` says.
    let length = length_message(body, &outline.sections);
    for tag in &outline.unclosed {
        warn(format!(
            "PR body opens an HTML <{tag}> block that never closes; its later headings still count as sections, but close it with </{tag}>"
        ));
    }
    // ADR-0071 rule 7: whether a Summary anchors the reader is judged by
    // review and evaluation; no count stands in for that. Its block shape is
    // checked mechanically under `git.pr_summary` (the note of 2026-10-03).
    for section in outline.sections {
        if section.matches("Testing")
            && !visible_text(section.content(), false)
                .lines()
                .any(|line| line.trim().to_ascii_lowercase().starts_with("not tested:"))
        {
            warn("PR Testing has no Not tested: line".into());
        }
    }
    if let Some(message) = length {
        out.push(Violation::new(
            "git.pr_sections",
            PolicyLevel::Warn,
            message,
            codeflow_core::remedy::PR_BODY_LENGTH.remedy(),
        ));
    }
    out
}

/// One top-level block of a Summary as a reader sees it rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Paragraph,
    List,
    Table,
    Heading,
    Code,
    Quote,
    Html,
    Rule,
}

impl Block {
    fn name(self) -> &'static str {
        match self {
            Self::Paragraph => "paragraph",
            Self::List => "list",
            Self::Table => "table",
            Self::Heading => "heading",
            Self::Code => "code block",
            Self::Quote => "quote",
            Self::Html => "HTML block",
            Self::Rule => "rule",
        }
    }

    /// The name with its article, for a sentence.
    fn with_article(self) -> String {
        let article = if self == Self::Html { "an" } else { "a" };
        format!("{article} {}", self.name())
    }

    fn details(self) -> bool {
        matches!(self, Self::List | Self::Table)
    }
}

/// The Markdown options GitHub renders a pull request body with: tables,
/// footnotes, strikethrough, task lists and alert quotes.
fn github_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
}

/// One top-level block of the Summary while the walk is inside it.
struct OpenBlock {
    /// `None` for a footnote definition, which is not a block of the shape.
    kind: Option<Block>,
    /// The visible text: `Text` and `Code` outside images, code blocks,
    /// quotes, HTML and footnote definitions; breaks end a line.
    text: String,
    /// The raw HTML of an HTML block.
    html: String,
    /// How deep the walk is inside content a reader does not see as prose.
    hidden: usize,
}

/// The visible top-level blocks of the Summary whose content spans
/// `range` of `body`, in order, read from one parse of the whole body with
/// GitHub's options. Reference definitions anywhere in the body resolve, so
/// a reference image or an empty reference link shows no text wherever it
/// is defined; reference and footnote definitions are never blocks. Left
/// out, because a reader sees no prose in them: an HTML block of only
/// comments or tags, a paragraph, list or table with no visible text, and a
/// paragraph that is only the `Task:` line, a field the PR template puts
/// above the Summary.
fn summary_blocks(body: &str, range: std::ops::Range<usize>) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut depth = 0usize;
    let mut open: Option<OpenBlock> = None;
    for (event, span) in Parser::new_ext(body, github_options()).into_offset_iter() {
        let at_top = depth == 0 && range.contains(&span.start);
        match event {
            Event::Start(tag) => {
                if at_top {
                    open = Some(OpenBlock {
                        kind: block_kind(&tag),
                        text: String::new(),
                        html: String::new(),
                        hidden: 0,
                    });
                }
                if let Some(block) = open.as_mut() {
                    if hides_text(&tag) {
                        block.hidden += 1;
                    }
                }
                depth += 1;
            }
            Event::End(tag) => {
                depth = depth.saturating_sub(1);
                if let Some(block) = open.as_mut() {
                    if ends_hidden(tag) {
                        block.hidden = block.hidden.saturating_sub(1);
                    }
                }
                if depth == 0 {
                    if let Some(block) = open.take() {
                        if let Some(kind) = block.kind.filter(|kind| visible_block(*kind, &block)) {
                            blocks.push(kind);
                        }
                    }
                }
            }
            Event::Rule if at_top => blocks.push(Block::Rule),
            Event::Html(value) => {
                if let Some(block) = open.as_mut() {
                    block.html.push_str(&value);
                }
            }
            Event::Text(value) | Event::Code(value) => {
                if let Some(block) = open.as_mut().filter(|block| block.hidden == 0) {
                    block.text.push_str(&value);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(block) = open.as_mut().filter(|block| block.hidden == 0) {
                    block.text.push('\n');
                }
            }
            _ => {}
        }
    }
    blocks
}

/// The shape kind of a top-level tag; `None` for a footnote definition.
fn block_kind(tag: &Tag<'_>) -> Option<Block> {
    Some(match tag {
        Tag::Paragraph => Block::Paragraph,
        Tag::List(_) => Block::List,
        Tag::Table(_) => Block::Table,
        Tag::Heading { .. } => Block::Heading,
        Tag::CodeBlock(_) => Block::Code,
        Tag::BlockQuote(_) => Block::Quote,
        Tag::FootnoteDefinition(_) => return None,
        _ => Block::Html,
    })
}

/// Content whose text a reader does not see as prose: an image's
/// description (an `alt` attribute), code, a quote, raw HTML and a footnote
/// definition, which renders at the end of the page.
fn hides_text(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Image { .. }
            | Tag::CodeBlock(_)
            | Tag::BlockQuote(_)
            | Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
    )
}

fn ends_hidden(tag: TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Image
            | TagEnd::CodeBlock
            | TagEnd::BlockQuote(_)
            | TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
    )
}

fn visible_block(kind: Block, block: &OpenBlock) -> bool {
    let text = block.text.trim();
    match kind {
        Block::Paragraph => {
            let task_line = text.starts_with("Task:") && !text.contains('\n');
            !text.is_empty() && !task_line
        }
        Block::List | Block::Table => !text.is_empty(),
        Block::Html => !strip_markup(&block.html).trim().is_empty(),
        Block::Heading | Block::Code | Block::Quote | Block::Rule => true,
    }
}

/// The text of an HTML block a reader sees: without its comments, where an
/// unclosed comment hides the rest as a browser does, and without its tags.
/// A block of only markup, such as `<p align="center">`, shows no text.
fn strip_markup(source: &str) -> String {
    let mut text = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("<!--") {
        text.push_str(&rest[..start]);
        let Some(end) = rest[start + 4..].find("-->") else {
            rest = "";
            break;
        };
        rest = &rest[start + 4 + end + 3..];
    }
    text.push_str(rest);
    let mut out = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// What is wrong with a Summary of these blocks, or `None` when it has the
/// shape: one prose paragraph, then a list or table, then only lists,
/// tables and at most one closing paragraph, which comes last.
fn summary_problem(blocks: &[Block]) -> Option<String> {
    let Some((lead, rest)) = blocks.split_first() else {
        return Some("it has no visible prose lead".into());
    };
    if *lead != Block::Paragraph {
        return Some(format!(
            "it opens with {}, not a prose paragraph",
            lead.with_article()
        ));
    }
    let Some(first_details) = rest.iter().position(|block| block.details()) else {
        return Some(
            match rest.iter().find(|block| **block != Block::Paragraph) {
                Some(other) => format!(
                    "{} follows the lead and no list or table",
                    other.with_article()
                ),
                None if rest.is_empty() => "no list or table follows the lead".into(),
                None => format!(
                    "it is {} prose paragraphs with no list or table",
                    rest.len() + 1
                ),
            },
        );
    };
    if let Some(between) = rest[..first_details].first() {
        return Some(if *between == Block::Paragraph {
            "a second prose paragraph comes before the first list or table".into()
        } else {
            format!(
                "{} comes between the lead and the first list or table",
                between.with_article()
            )
        });
    }
    let after = &rest[first_details + 1..];
    if let Some(other) = after
        .iter()
        .find(|block| !block.details() && **block != Block::Paragraph)
    {
        return Some(format!(
            "{} follows the list; only lists, tables and one closing paragraph may",
            other.with_article()
        ));
    }
    let prose = after
        .iter()
        .filter(|block| **block == Block::Paragraph)
        .count();
    if prose > 1 {
        return Some(format!(
            "{prose} prose paragraphs follow the list; at most one closing paragraph may"
        ));
    }
    if prose == 1 && after.last() != Some(&Block::Paragraph) {
        return Some(
            "a prose paragraph sits between lists; the one closing paragraph comes last".into(),
        );
    }
    None
}

/// The Summary shape check (`git.pr_summary`, ADR-0071 note of 2026-10-03):
/// the body's one Summary section, under its mapped heading, opens with one
/// prose paragraph that anchors the reader, then the details as a list or
/// table, then at most one closing paragraph. Only the visible blocks
/// count, so text hidden in an HTML comment or a code block never supplies
/// the lead or the list. A missing, empty or repeated Summary is left to
/// `git.pr_sections`.
pub(super) fn summary_shape(git: &GitPolicy, body: &str) -> Vec<Violation> {
    if !git.pr_summary.is_active() {
        return Vec::new();
    }
    let heading = adoption::mapped_sections(git, &["Summary".to_string()])
        .pop()
        .unwrap_or_else(|| "Summary".to_string());
    let parsed = sections(body);
    let matched = matching_sections(&parsed, &heading);
    let [section] = matched.as_slice() else {
        return Vec::new();
    };
    if !has_content(section.content()) {
        return Vec::new();
    }
    let blocks = summary_blocks(body, section.start..section.end);
    let Some(problem) = summary_problem(&blocks) else {
        return Vec::new();
    };
    let found = if blocks.is_empty() {
        "nothing visible".to_string()
    } else {
        blocks
            .iter()
            .map(|block| block.name())
            .collect::<Vec<_>>()
            .join(", ")
    };
    vec![Violation::new(
        "git.pr_summary",
        git.pr_summary,
        format!(
            "PR '## {heading}' is not a prose lead then bullets: {problem} (found: {found}); expected one prose paragraph, then a list or table, then at most one closing paragraph"
        ),
        codeflow_core::remedy::PR_SUMMARY_SHAPE.remedy(),
    )]
}

/// The body's one Release impact section's own fields, each `(key,
/// value)` with the key in lower case; `None` without exactly one section.
/// Fields come only from the section's own text outside code and quotes,
/// never from a subsection; both the release check and the watched-path
/// settlement read them here.
fn release_fields(body: &str) -> Option<Vec<(String, String)>> {
    release_fields_under(body, "Release impact")
}

fn release_fields_under(body: &str, heading: &str) -> Option<Vec<(String, String)>> {
    let parsed = sections(body);
    let matched = matching_sections(&parsed, heading);
    let [section] = matched.as_slice() else {
        return None;
    };
    let content = section.content();
    let end = sections(content)
        .first()
        .map_or(content.len(), |s| s.heading_start);
    Some(
        field_text(&content[..end])
            .lines()
            .filter_map(|line| line.trim().split_once(':'))
            .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_string()))
            .collect(),
    )
}

/// What `scripts/release.py` reads from a PR body, through
/// `codeflow ci --read-release-impact`: the Release impact fields, the
/// visible text of each Breaking change section (a `Migration: see Breaking
/// change` reference), and this check's findings under the default policy.
pub(super) fn reading(body: &str) -> serde_json::Value {
    let parsed = sections(body);
    let breaking_change: Vec<String> = matching_sections(&parsed, "Breaking change")
        .iter()
        .map(|section| visible_text(section.content(), true))
        .collect();
    let findings: Vec<String> = release(&GitPolicy::default(), body, false)
        .into_iter()
        .map(|violation| violation.message)
        .collect();
    serde_json::json!({
        "release_impact": release_fields(body),
        "breaking_change": breaking_change,
        "findings": findings,
    })
}

/// The legacy three-state `Contract` field and the `Breaking` value each
/// state means (`CONTRACT_BREAKING` in `scripts/release.py`). A block may
/// state Contract alone during the transition, or beside Breaking when the
/// two agree (`docs/releasing.md`).
const CONTRACT_BREAKING: [(&str, &str); 3] = [
    ("not-applicable", "no"),
    ("compatible", "no"),
    ("breaking", "yes"),
];

fn contract_breaking(contract: &str) -> Option<&'static str> {
    CONTRACT_BREAKING
        .iter()
        .find(|(state, _)| state.eq_ignore_ascii_case(contract))
        .map(|(_, breaking)| *breaking)
}

/// What a block declares about breaking: `Breaking` lower-cased, or, when
/// only the legacy `Contract` appears, the Breaking value its state means
/// (empty for an unknown state). `legacy` is that Contract-only case.
struct Declared {
    breaking: String,
    legacy: bool,
}

fn declared(fields: &std::collections::BTreeMap<String, &str>) -> Declared {
    match (fields.get("breaking"), fields.get("contract")) {
        (None, Some(contract)) => Declared {
            breaking: contract_breaking(contract).unwrap_or_default().to_string(),
            legacy: true,
        },
        (breaking, _) => Declared {
            breaking: breaking.copied().unwrap_or_default().to_ascii_lowercase(),
            legacy: false,
        },
    }
}

/// Whether the body's one Release impact section declares no break, by
/// `Breaking: no`, a legacy `Contract: compatible`, or both, and has a
/// `Rationale` that gives a reason (TSK-147 AC-4). A legacy
/// `not-applicable` says no contract was touched, which a watched surface
/// contradicts, so `scripts/release.py check-pr` refuses it there and it
/// settles nothing here.
pub(super) fn declares_no_break(body: &str) -> bool {
    let Some(fields) = release_fields(body) else {
        return false;
    };
    let field = |name: &str| {
        let values: Vec<&str> = fields
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
            .collect();
        match values.as_slice() {
            [value] => Some(*value),
            _ => None,
        }
    };
    let stated = |name: &str| fields.iter().any(|(key, _)| key == name);
    let holds = |name: &str, meaning: &str| {
        !stated(name) || field(name).is_some_and(|value| value.eq_ignore_ascii_case(meaning))
    };
    (stated("breaking") || stated("contract"))
        && holds("breaking", "no")
        && holds("contract", "compatible")
        && field("rationale").is_some_and(|value| !value.is_empty() && !placeholder(value))
}

pub(super) fn release_heading(git: &GitPolicy) -> String {
    codeflow_core::hooks::adoption::mapped_sections(git, &["Release impact".into()]).remove(0)
}

pub(super) fn release(git: &GitPolicy, body: &str, breaking_commit: bool) -> Vec<Violation> {
    if !git.pr_release_impact.is_active() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut issue = |message: String| {
        out.push(Violation::new(
            "git.pr_release_impact",
            git.pr_release_impact,
            message,
            codeflow_core::remedy::PR_RELEASE_IMPACT.remedy(),
        ));
    };
    let parsed = sections(body);
    // Do not consume sibling/subsection migration fields as release fields.
    let Some(lines) = release_fields_under(body, &release_heading(git)) else {
        issue("PR body needs exactly one Release impact section".into());
        return out;
    };
    let mut fields = std::collections::BTreeMap::new();
    for (key, value) in &lines {
        if ["impact", "breaking", "contract", "rationale", "migration"].contains(&key.as_str())
            && fields.insert(key.clone(), value.as_str()).is_some()
        {
            issue(format!("PR Release impact has duplicate {key} fields"));
        }
    }
    let declared = declared(&fields);
    // A legacy Contract-only block states its break through Contract, and
    // Migration is required with Breaking only, as `scripts/release.py` reads it.
    let required: &[&str] = if declared.legacy {
        &["impact", "rationale"]
    } else {
        &["impact", "breaking", "rationale", "migration"]
    };
    for key in required {
        if fields.get(*key).is_none_or(|value| value.is_empty()) {
            issue(format!(
                "PR Release impact requires a non-empty {key} field"
            ));
        }
    }
    // The same field rules as `scripts/release.py`, which reads the body
    // through this reader; both pass `scripts/fixtures/release_impact_cases.json`.
    if fields
        .get("rationale")
        .is_some_and(|value| !value.is_empty() && placeholder(value))
    {
        issue("PR Rationale must give the reason, not a placeholder".into());
    }
    if fields
        .get("migration")
        .is_some_and(|value| TEMPLATE_ALTERNATIVES.contains(&guidance(value).as_str()))
    {
        issue("PR Migration still holds the template's alternatives; choose one".into());
    }
    let impact = fields
        .get("impact")
        .copied()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let breaking = declared.breaking;
    let levels = ["none", "patch", "minor", "major"];
    if !levels.contains(&impact.as_str()) {
        issue("PR Impact must be none, patch, minor or major".into());
    }
    let contract = fields.get("contract").copied();
    if contract.is_some_and(|value| contract_breaking(value).is_none()) {
        issue("PR Contract must be not-applicable, compatible or breaking".into());
    } else if !declared.legacy && !["yes", "no"].contains(&breaking.as_str()) {
        issue("PR Breaking must be yes or no".into());
    } else if contract
        .and_then(contract_breaking)
        .is_some_and(|meant| meant != breaking)
    {
        issue("PR Contract disagrees with Breaking".into());
    }
    let below_floor = levels.iter().position(|level| *level == impact)
        < levels
            .iter()
            .position(|level| *level == git.pr_breaking_level);
    if breaking_commit && breaking != "yes" {
        issue("breaking commit marker requires Breaking: yes".into());
    }
    if (breaking_commit || breaking == "yes") && below_floor {
        issue(format!(
            "{} requires Impact of at least {}",
            if breaking_commit {
                "breaking commit marker"
            } else {
                "Breaking: yes"
            },
            git.pr_breaking_level
        ));
    }
    let migration = fields.get("migration").copied().unwrap_or_default();
    if breaking == "yes" && !migration_guidance(&parsed, migration) {
        issue("PR Breaking: yes requires substantive Migration steps or a populated Breaking change reference".into());
    }
    out
}

/// Whether `migration` gives real steps, or points at exactly one populated
/// Breaking change section.
fn migration_guidance(parsed: &[Section<'_>], migration: &str) -> bool {
    if guidance(migration) == "see breaking change" {
        let sections = matching_sections(parsed, "Breaking change");
        sections.len() == 1 && substantive(&visible_text(sections[0].content(), true))
    } else {
        substantive(migration)
    }
}

/// Lowercased text without Markdown quoting or repeated whitespace, as
/// `scripts/release.py` compares guidance.
fn guidance(value: &str) -> String {
    value
        .replace(['`', '"', '\''], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// The PR template's Migration choice text, left in place instead of a
/// chosen value (`UNRESOLVED_ALTERNATIVES` in `scripts/release.py`).
const TEMPLATE_ALTERNATIVES: [&str; 3] = [
    "none, steps, or see breaking change",
    "none | steps | see breaking change",
    "steps",
];

/// A value that says nothing: empty of letters and digits, a whole
/// `<placeholder>`, or a placeholder word (`PLACEHOLDERS` in
/// `scripts/release.py`).
fn placeholder(value: &str) -> bool {
    let value = guidance(value);
    let whole_placeholder = value.starts_with('<')
        && value.ends_with('>')
        && !value[1..value.len() - 1].contains(['<', '>']);
    whole_placeholder
        || !value.chars().any(char::is_alphanumeric)
        || matches!(value.as_str(), "none" | "n/a" | "na" | "tbd" | "todo")
}

/// Real migration guidance, not a placeholder or the template's unresolved
/// alternatives. Only whole-field forms are rejected, so a step containing a
/// pipe or angle brackets in a command still counts.
fn substantive(value: &str) -> bool {
    let normalized = guidance(value);
    !placeholder(value)
        && normalized != "see breaking change"
        && !TEMPLATE_ALTERNATIVES.contains(&normalized.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body whose Summary is `summary`, followed by a Changes section.
    fn with_summary(summary: &str) -> String {
        format!("Task: TSK-001\n\n## Summary\n\n{summary}\n\n## Changes\n\n- one change\n")
    }

    fn shape(summary: &str) -> Vec<Violation> {
        summary_shape(&GitPolicy::default(), &with_summary(summary))
    }

    #[test]
    fn summary_shape_passes_a_lead_then_details() {
        for (case, summary) in [
            (
                "lead and bullets",
                "Adds the shape check so a reader is anchored first.\n\n- one\n- two",
            ),
            (
                "lead and a numbered list",
                "Adds the shape check.\n\n1. first\n2. second",
            ),
            (
                "lead and a table",
                "Compares the two checks.\n\n| Check | Level |\n|---|---|\n| shape | block |",
            ),
            (
                "lead, bullets and a closing line",
                "Adds the shape check.\n\n- one\n- two\n\nMinor for 3.1.0.",
            ),
            (
                "a list that interrupts the lead with no blank line",
                "This PR closes that gap:\n- one\n- two\n\nA patch fix for 3.1.0.",
            ),
            (
                "lead, bullets, table and a closing line",
                "Adds the check.\n\n- one\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nMinor.",
            ),
            (
                "comments and the Task line are skipped",
                "<!-- the template comment -->\n\nTask: TSK-001\n\nAdds the check.\n\n<!-- note -->\n\n- one",
            ),
            (
                "prose and details that carry an image",
                "Adds the check ![diagram](https://example.com/d.png) shown here.\n\n- one ![icon](https://example.com/i.png) change\n\n| A | B |\n|---|---|\n| ![x](https://example.com/x.png) cell | 2 |",
            ),
            (
                "a tag-only HTML block shows no text",
                "<p align=\"center\">\n\nAdds the check.\n\n- one\n\n</p>",
            ),
            (
                "a long single lead is not counted",
                &format!("{}\n\n- one", "A sentence that goes on. ".repeat(40)),
            ),
        ] {
            assert!(shape(summary).is_empty(), "{case}: {:?}", shape(summary));
        }
    }

    /// `summary` draws exactly one blocking `git.pr_summary` finding that
    /// says `said` and names the expected shape.
    fn assert_refused(case: &str, summary: &str, said: &str) {
        let found = shape(summary);
        assert_eq!(found.len(), 1, "{case}: {found:?}");
        assert_eq!(found[0].rule, "git.pr_summary", "{case}");
        assert_eq!(found[0].level, PolicyLevel::Block, "{case}");
        assert!(
            found[0].message.contains(said),
            "{case}: {}",
            found[0].message
        );
        assert!(
            found[0].message.contains("expected one prose paragraph"),
            "{case}: {}",
            found[0].message
        );
    }

    #[test]
    fn summary_shape_refuses_other_shapes() {
        for (case, summary, said) in [
            (
                "prose only",
                "Adds the check.",
                "no list or table follows the lead",
            ),
            (
                "a prose wall",
                "One.\n\nTwo.\n\nThree.",
                "it is 3 prose paragraphs with no list or table",
            ),
            (
                "two lead paragraphs",
                "Adds the check.\n\nIt also does more.\n\n- one",
                "a second prose paragraph comes before the first list or table",
            ),
            (
                "list first",
                "- one\n- two\n\nAdds the check.",
                "it opens with a list, not a prose paragraph",
            ),
            (
                "table first",
                "| A | B |\n|---|---|\n| 1 | 2 |",
                "it opens with a table, not a prose paragraph",
            ),
            (
                "prose wall after the list",
                "Adds the check.\n\n- one\n\nMore.\n\nAnd more.",
                "2 prose paragraphs follow the list",
            ),
            (
                "prose between lists",
                "Adds the check.\n\n- one\n\nAlso:\n\n- two",
                "a prose paragraph sits between lists",
            ),
            (
                "a heading in the Summary",
                "Adds the check.\n\n- one\n\n### Detail\n\n- two",
                "a heading follows the list",
            ),
            (
                "a quote as the lead",
                "> Adds the check.\n\n- one",
                "it opens with a quote",
            ),
            (
                "a code block after the list",
                "Adds the check.\n\n- one\n\n```text\nout\n```",
                "a code block follows the list",
            ),
            (
                "prose inside an HTML block",
                "Adds the check.\n\n- one\n\n<details>\n<summary>More</summary>\n\nA wall.\n</details>",
                "follows the list",
            ),
            (
                "only the Task line",
                "Task: TSK-001",
                "it has no visible prose lead",
            ),
        ] {
            assert_refused(case, summary, said);
        }
    }

    /// Footnotes and reference definitions resolve across the whole body, as
    /// GitHub renders it: a definition is never a block of the shape, and a
    /// reference image or empty reference link shows no prose, wherever its
    /// definition sits.
    #[test]
    fn summary_shape_reads_footnotes_and_references_as_rendered() {
        const DEF: &str = "[ref]: https://example.com/p.png";
        let body = |summary: &str, inside: bool| {
            if inside {
                format!("Task: TSK-001\n\n## Summary\n\n{summary}\n\n{DEF}\n\n## Changes\n\n- one change\n")
            } else {
                format!("Task: TSK-001\n\n## Summary\n\n{summary}\n\n## Changes\n\n- one change\n\n{DEF}\n")
            }
        };
        let notes = "This fixes startup[^a] and shutdown[^b].\n\n- Handles both cases.\n\n[^a]: Startup details belong here.\n\n[^b]: Shutdown details belong here.";
        assert!(
            summary_shape(&GitPolicy::default(), &with_summary(notes)).is_empty(),
            "footnotes inside the Summary"
        );
        let elsewhere = "Task: TSK-001\n\n## Summary\n\nThis fixes startup[^a].\n\n- Handles it.\n\n## Changes\n\n- one change\n\n[^a]: Startup details.\n";
        assert!(
            summary_shape(&GitPolicy::default(), elsewhere).is_empty(),
            "a footnote defined in another section"
        );
        let unreferenced = shape("[^a]: This definition is never referenced.\n\n- one");
        assert_eq!(unreferenced.len(), 1, "{unreferenced:?}");
        assert!(
            unreferenced[0].message.contains("it opens with a list"),
            "{}",
            unreferenced[0].message
        );
        for (case, summary, said) in [
            (
                "a reference image lead",
                "![Description][ref]\n\n- one",
                "it opens with a list",
            ),
            (
                "an empty reference link lead",
                "[][ref]\n\n- one",
                "it opens with a list",
            ),
            (
                "a reference-image table",
                "Adds the check.\n\n| ![a][ref] |  |\n|---|---|\n|  |  |",
                "no list or table follows the lead",
            ),
            (
                "an empty-reference-link list",
                "Adds the check.\n\n- [][ref]\n- [][ref]",
                "no list or table follows the lead",
            ),
        ] {
            for inside in [true, false] {
                let found = summary_shape(&GitPolicy::default(), &body(summary, inside));
                assert_eq!(
                    found.len(),
                    1,
                    "{case} (definition inside: {inside}): {found:?}"
                );
                assert!(
                    found[0].message.contains(said),
                    "{case}: {}",
                    found[0].message
                );
            }
        }
        // A reference link with text and a reference image beside prose
        // still read as prose.
        for inside in [true, false] {
            let text = body(
                "See [the guide][ref] ![d][ref] for the steps.\n\n- one [step][ref]",
                inside,
            );
            assert!(
                summary_shape(&GitPolicy::default(), &text).is_empty(),
                "{inside}"
            );
        }
    }

    /// Text in a comment, a code fence or an empty cell never supplies the
    /// lead or the list.
    #[test]
    fn summary_shape_ignores_hidden_details() {
        for (case, summary, said) in [
            (
                "a lead hidden in a comment",
                "<!-- Adds the check. -->\n\n- one",
                "it opens with a list",
            ),
            (
                "a list hidden in a code fence",
                "Adds the check.\n\n```\n- one\n- two\n```",
                "a code block follows the lead and no list or table",
            ),
            (
                "a list hidden in a comment",
                "Adds the check.\n\n<!--\n- one\n-->",
                "no list or table follows the lead",
            ),
            (
                "a list of bare template bullets",
                "Adds the check.\n\n-\n-",
                "no list or table follows the lead",
            ),
            (
                "a list whose items are only code",
                "Adds the check.\n\n- ```\n  hidden\n  ```",
                "no list or table follows the lead",
            ),
            (
                "a table with only empty cells",
                "Adds the check.\n\n|  |  |\n|---|---|\n|  |  |",
                "no list or table follows the lead",
            ),
            (
                "a lead that is only an image description",
                "![This text is only an image description](https://example.com/picture.png)\n\n- one change",
                "it opens with a list",
            ),
            (
                "a list whose items are only images",
                "Adds the check.\n\n- ![one change](https://example.com/a.png)\n- ![two](https://example.com/b.png)",
                "no list or table follows the lead",
            ),
            (
                "a table whose only content is an image",
                "Adds the check.\n\n|  |  |\n|---|---|\n| ![a description](https://example.com/a.png) |  |",
                "no list or table follows the lead",
            ),
            (
                "a table whose cells hold only comments",
                "Adds the check.\n\n| <!-- a --> | <!-- b --> |\n|---|---|\n| <!-- c --> | <!-- d --> |",
                "no list or table follows the lead",
            ),
        ] {
            assert_refused(case, summary, said);
        }
    }

    #[test]
    fn summary_shape_names_what_it_found() {
        let found = shape("One.\n\nTwo.\n\n- three");
        assert!(
            found[0]
                .message
                .contains("(found: paragraph, paragraph, list)"),
            "{}",
            found[0].message
        );
        assert!(found[0].remedy.contains("writing.md` \"Summaries\""));
        assert!(found[0].remedy.contains("codeflow ci --pr-body-file"));
    }

    #[test]
    fn summary_shape_follows_its_level_and_heading() {
        let body = with_summary("Adds the check.");
        let mut git = GitPolicy {
            pr_summary: PolicyLevel::Warn,
            ..GitPolicy::default()
        };
        let found = summary_shape(&git, &body);
        assert_eq!(found[0].level, PolicyLevel::Warn);
        for level in [PolicyLevel::Off, PolicyLevel::Allow] {
            git.pr_summary = level;
            assert!(summary_shape(&git, &body).is_empty(), "{level}");
        }
        // A missing, empty or repeated Summary is the section check's.
        let git = GitPolicy::default();
        assert!(summary_shape(&git, "## Changes\n\n- one\n").is_empty());
        assert!(summary_shape(&git, "## Summary\n\n<!-- c -->\n\n## Changes\n").is_empty());
        assert!(summary_shape(&git, "## Summary\n\nOne.\n\n## Summary\n\nTwo.\n").is_empty());
        // An example Summary inside a fence or a closed HTML block is not
        // the section.
        assert!(summary_shape(&git, "## Changes\n\n```md\n## Summary\n\nOne.\n```\n").is_empty());
        // An accepted mapping checks the template's own heading.
        let mapped = GitPolicy {
            pr_section_mapping: Some(codeflow_core::hooks::policy::PrSectionMapping {
                state: codeflow_core::hooks::policy::MappingState::Accepted,
                headings: [("Summary".to_string(), "Description".to_string())]
                    .into_iter()
                    .collect(),
                decided: "2026-10-03".into(),
            }),
            ..GitPolicy::default()
        };
        let found = summary_shape(&mapped, "## Description\n\nOne.\n\nTwo.\n");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].message.contains("'## Description'"));
    }

    fn declaration(impact: &str, breaking: &str, migration: &str) -> String {
        format!("## Release impact\n- Impact: {impact}\n- Breaking: {breaking}\n- Rationale: Preserve the public behavior.\n- Migration: {migration}\n- Unit: another-project\n- Evidence: manual assessment\n")
    }

    #[test]
    fn markdown_sections_reject_examples_comments_duplicates_and_empty_content() {
        for fake in [
            "```md\n## Summary\nexample\n```",
            "~~~\n## Summary\nexample\n~~~",
            "<!--\n## Summary\ncomment\n-->",
            "> ## Summary\n> quoted example",
            "- ## Summary\n  listed example",
            "    ## Summary\n    indented example",
        ] {
            assert_eq!(
                find_section(fake, "Summary"),
                SectionState::Missing,
                "{fake}"
            );
        }
        for empty in [
            "<!-- hint -->",
            "-\n",
            "### Detail\n<!-- hint -->",
            "```\n```",
            "<!-- unclosed",
        ] {
            assert_eq!(
                find_section(&format!("## Summary\n{empty}"), "Summary"),
                SectionState::Empty,
                "{empty}"
            );
        }
        assert_eq!(
            find_section("## Summary\none\n## Summary\ntwo", "Summary"),
            SectionState::Duplicate
        );
        assert_eq!(
            find_section("## Summary\n<!--\n## Summary\n-->\nreal", "Summary"),
            SectionState::Present
        );
    }

    #[test]
    fn markdown_sections_keep_nested_evidence_without_borrowing_siblings() {
        assert_eq!(
            find_section(
                "## Testing\n### Unit checks\n```\n3 passed\n```\n## Changes\nx",
                "Testing"
            ),
            SectionState::Present
        );
        assert_eq!(
            find_section(
                "## Testing\n### Unit checks\n<!-- todo -->\n## Changes\nx",
                "Testing"
            ),
            SectionState::Empty
        );
        assert_eq!(
            find_section("### sUmMaRy ###\nWorks.", "Summary"),
            SectionState::Present
        );
        assert_eq!(
            find_section("Summary\n-------\nWorks.", "Summary"),
            SectionState::Missing
        );
        assert!(has_content("## Testing\n```\n<!-- literal output -->\n```"));
    }

    #[test]
    fn setext_paragraphs_do_not_end_atx_sections() {
        // Exact body from Grok finding 2.
        let body = "## Testing\nAll good\n---\n## Changes\nx\n";
        assert_eq!(find_section(body, "Testing"), SectionState::Present);
        for underline in ["---", "==="] {
            for name in ["Summary", "Changes", "Reviews", "Release impact", "Testing"] {
                let body = format!("## {name}\nAll good\n{underline}\n## Next\nx\n");
                assert_eq!(find_section(&body, name), SectionState::Present, "{body}");
            }
        }
        assert_eq!(
            find_section("Summary\n===\ntext", "Summary"),
            SectionState::Missing
        );
    }

    #[test]
    fn only_unindented_headings_outside_html_declare_sections() {
        for fake in [
            " ## Summary\nexample", "  ## Summary\nexample", "   ## Summary\nexample",
            "<details>\n\n## Summary\nexample\n\n</details>",
            "<details\n class='example'>\n\n## Summary\nexample\n\n</details>",
            "<custom-panel>\n\n## Summary\nexample\n\n</custom-panel>",
            "<DIV class='example'>\n\n## Summary\nexample\n\n</DIV>",
            "<details data-label='a > b'><summary>Example</summary>\n\n## Summary\nexample\n\n</details>",
            "<details>\n<div>\n\n## Summary\nexample\n\n</div>\n</details>",
        ] {
            assert_eq!(find_section(fake, "Summary"), SectionState::Missing, "{fake}");
            let real = format!("{fake}\n\n## Summary\nReal summary.");
            assert_eq!(find_section(&real, "Summary"), SectionState::Present, "{real}");
        }
        let commented = "<!--\n<details>\n<head>\n-->\n## Summary\nReal summary.";
        assert_eq!(find_section(commented, "Summary"), SectionState::Present);
    }

    const FULL_BODY: &str = "## Summary\nMake the command easier to use.\n\n## Changes\n- Explain the result.\n\n## Testing\nRan 3 tests.\nNot tested: Windows.\n\n## Reviews\nNone: pending.\n\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Clarify output.\n- Migration: none\n";
    const NAMES: [&str; 5] = ["Summary", "Changes", "Testing", "Reviews", "Release impact"];

    #[test]
    fn raw_html_is_content_and_comments_are_not() {
        for content in [
            "<p>Ran the suite.</p>",
            "<img src=\"shot.png\" alt=\"Result\">",
            "<details><pre>test result: ok</pre></details>",
            "<p align=\"center\">Ran the suite.",
            "<!-- note -->\n<p>Ran the suite.</p>",
        ] {
            let body = format!("## Testing\n\n{content}\n");
            assert_eq!(
                find_section(&body, "Testing"),
                SectionState::Present,
                "{content}"
            );
        }
        for content in [
            "<!-- Name the tested revision. -->",
            "<!--\n  Name the tested revision,\n  and what was not tested.\n-->",
            "<!-- one -->\n\n<!-- two -->",
        ] {
            let body = format!("## Testing\n\n{content}\n\n## Reviews\n\nNone: docs.\n");
            assert_eq!(
                find_section(&body, "Testing"),
                SectionState::Empty,
                "{content}"
            );
        }
    }

    #[test]
    fn inline_html_and_unclosed_blocks_never_hide_later_sections() {
        for (from, to, unclosed) in [
            (
                "- Explain the result.",
                "- Return Vec<String> instead of a joined string.",
                None,
            ),
            (
                "Make the command easier to use.",
                "Replace <path> with the real file.",
                None,
            ),
            (
                "Make the command easier to use.",
                "Use <details> inline, then prose.",
                None,
            ),
            (
                "Make the command easier to use.",
                "<p align=\"center\">\n\nMake it easier.",
                Some("p"),
            ),
            (
                "- Explain the result.",
                "- Explain the result.\n\n<details>\n\nMore detail.",
                Some("details"),
            ),
            (
                "- Explain the result.",
                "- Explain.\n\n<div>\n<details>\n\n## Example\n\n</details>",
                Some("div"),
            ),
        ] {
            let body = FULL_BODY.replace(from, to);
            for name in NAMES {
                assert_eq!(
                    find_section(&body, name),
                    SectionState::Present,
                    "{name}: {body}"
                );
            }
            assert!(
                release(&GitPolicy::default(), &body, false).is_empty(),
                "{body}"
            );
            let warnings: Vec<_> = presentation(&GitPolicy::default(), &body, false)
                .into_iter()
                .filter(|v| v.message.contains("never closes"))
                .collect();
            match unclosed {
                None => assert!(warnings.is_empty(), "{body}: {warnings:?}"),
                Some(tag) => {
                    assert_eq!(warnings.len(), 1, "{body}: {warnings:?}");
                    assert!(warnings[0].message.contains(&format!("<{tag}>")));
                    assert_eq!(warnings[0].level, PolicyLevel::Warn);
                }
            }
        }
        // A closed container inside an unclosed one still hides its example.
        let nested = "<div>\n<details>\n\n## Summary\nexample\n\n</details>\n\n## Summary\nReal.";
        assert_eq!(find_section(nested, "Summary"), SectionState::Present);
    }

    #[test]
    fn depth_two_sections_take_precedence_over_same_name_subsections() {
        for body in [
            "## Testing\nParent content.\n### Testing\nNested content.",
            "## Testing\n### Testing\nNested content.",
            "### Testing\nEarlier example.\n## Testing\nReal evidence.",
        ] {
            assert_eq!(
                find_section(body, "Testing"),
                SectionState::Present,
                "{body}"
            );
        }
        assert_eq!(
            find_section(
                "### Testing\nExample.\n## Testing\n<!-- empty -->",
                "Testing"
            ),
            SectionState::Empty
        );
        assert_eq!(
            find_section("## Testing\nFirst.\n## Testing\nSecond.", "Testing"),
            SectionState::Duplicate
        );
        let body = format!(
            "### Release impact\nEarlier example.\n{}",
            declaration("Minor", "No", "None")
        );
        assert!(release(&GitPolicy::default(), &body, false).is_empty());
    }

    #[test]
    fn presentation_warnings_are_advisory_and_respect_off() {
        let body = format!("## Summary\nOne. Two. Three. Four. Update `thing` in src/thing.py.\n## Testing\nPassed.\n```\n{}```\n{}", "output\n".repeat(13), "long word ".repeat(800));
        let findings = presentation(&GitPolicy::default(), &body, false);
        {
            let reason = "Not tested:";
            assert!(
                findings.iter().any(|v| v.message.contains(reason)),
                "missing {reason}: {findings:?}"
            );
        }
        // ADR-0071 rule 7: a key file name or code span may anchor the
        // Summary, and its length is judgment, so none draws a warning.
        for retired in [
            "code span",
            "contains a path",
            "sentences",
            "13 lines",
            "rendered rows",
            "160 characters",
        ] {
            assert!(
                !findings.iter().any(|v| v.message.contains(retired)),
                "retired Summary warning {retired}: {findings:?}"
            );
        }
        assert!(findings.iter().all(|v| v.level == PolicyLevel::Warn));
        let git = GitPolicy {
            pr_sections: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        assert!(presentation(&git, &body, false).is_empty());
    }

    fn words(n: usize) -> String {
        "word ".repeat(n)
    }

    /// TSK-228: the count is what a reader sees. Comments, tag markup and
    /// image alt text are left out; code blocks, tables, headings and link
    /// text are in.
    #[test]
    fn word_count_reads_what_a_reader_sees() {
        assert_eq!(word_count("one two three"), 3);
        assert_eq!(word_count("one <!-- hidden words here --> two"), 2);
        assert_eq!(
            word_count("one\n\n<!--\nhidden\n\nmore hidden\n-->\n\ntwo"),
            2
        );
        assert_eq!(word_count("a <!-- split\nacross lines --> b"), 2);
        assert_eq!(word_count("## Heading of three\ntext"), 4);
        assert_eq!(word_count("```text\nfenced output of five words\n```"), 5);
        assert_eq!(
            word_count("```\n<!-- shown in a fence -->\n```"),
            6,
            "a comment inside a fenced block is shown, so it counts"
        );
        assert_eq!(
            word_count("| a | b |\n|---|---|\n| one two | three |"),
            5,
            "cell text counts; pipes and the delimiter row do not"
        );
        assert_eq!(
            word_count("see [the guide](https://example.com/a/b) now"),
            4,
            "a link counts by its text, never its address"
        );
        assert_eq!(word_count("look ![alt text of image](x.png) here"), 2);
        assert_eq!(word_count("a <b>bold</b> c"), 3);
        assert_eq!(word_count("foo**bar**baz"), 1, "inline marks join words");
        assert_eq!(word_count("- one\n- two\n- three"), 3);
        assert_eq!(word_count(""), 0);
        assert_eq!(word_count("<!-- only a comment -->"), 0);
    }

    /// Review round 1 (F1): raw HTML keeps its block, cell and line-break
    /// boundaries, decodes entities and hides what GitHub hides.
    #[test]
    fn word_count_reads_raw_html_as_rendered() {
        assert_eq!(word_count("<p>one</p><p>two</p><p>three</p>"), 3);
        assert_eq!(word_count("one<br>two<br/>three"), 3);
        assert_eq!(
            word_count("<table><tr><td>a</td><td>b</td></tr></table>"),
            2
        );
        assert_eq!(
            word_count("<details><summary>Round</summary>one two</details>"),
            3
        );
        assert_eq!(word_count("a&nbsp;b &amp; c"), 4);
        assert_eq!(word_count("&nbsp; &#160; &#xA0;"), 0);
        assert_eq!(word_count("<p>foo<b>bar</b>baz</p>"), 1, "inline tags join");
        assert_eq!(word_count("<p>a < b</p>"), 3, "a bare < is text");
        assert_eq!(
            word_count("<!-- unterminated comment runs to the end\n\nhidden"),
            0
        );
        let long = "<p>word</p>".repeat(BODY_WORD_LIMIT + 100);
        assert!(
            length_message(&long, &sections(&long)).is_some(),
            "adjacent HTML elements are separate words and pass the limit"
        );
    }

    /// Review round 2: a `>` inside a quoted attribute does not end the
    /// tag, and the named whitespace references separate words.
    #[test]
    fn word_count_reads_quoted_attributes_and_named_spaces() {
        assert_eq!(
            word_count("<div><img src=\"x.png\" alt=\"a > hidden words\"></div>"),
            0
        );
        assert_eq!(
            word_count("<div><img src='x.png' alt='a > hidden words'>shown</div>"),
            1
        );
        assert_eq!(word_count("<p title=\"x\">a < b</p>"), 3);
        assert_eq!(
            word_count("<div>one&ensp;two&emsp;three&Tab;four&NewLine;five&thinsp;six</div>"),
            6
        );
        let at = format!(
            "{}<div><img src=\"x.png\" alt=\"a > hidden image alternative words here\"></div>",
            words(BODY_WORD_LIMIT - 2)
        );
        assert!(length_message(&at, &sections(&at)).is_none());
        let over = format!(
            "{}<div>one&ensp;two&emsp;three&Tab;four&NewLine;five</div>",
            words(BODY_WORD_LIMIT - 2)
        );
        assert!(length_message(&over, &sections(&over)).is_some());
    }

    /// Review round 3: every space the HTML specification names, however it
    /// is written, separates words at the 1,000-word boundary.
    #[test]
    fn spaces_written_as_references_separate_words_at_the_boundary() {
        let spaces = [
            "&nbsp;",
            "&NonBreakingSpace;",
            "&ensp;",
            "&emsp;",
            "&emsp13;",
            "&emsp14;",
            "&numsp;",
            "&puncsp;",
            "&thinsp;",
            "&ThinSpace;",
            "&hairsp;",
            "&VeryThinSpace;",
            "&MediumSpace;",
            "&ThickSpace;",
            "&Tab;",
            "&NewLine;",
            "&#32;",
            "&#x20;",
            "&#x00000020;",
            "&#00032;",
            "&#9;",
            "&#xA0;",
            "&#160;",
        ];
        for space in spaces {
            let body = format!(
                "{}\n\n<div>one{space}two{space}three</div>",
                words(BODY_WORD_LIMIT - 2)
            );
            assert_eq!(word_count(&body), BODY_WORD_LIMIT + 1, "{space}");
            assert!(length_message(&body, &sections(&body)).is_some(), "{space}");
        }
        assert_eq!(word_count("<div>one&nbsptwo</div>"), 2, "legacy form");
        assert_eq!(
            word_count("<div>R&D &amp; Q&A</div>"),
            3,
            "a bare & is text"
        );
    }

    #[test]
    fn word_count_hides_footnote_definitions_nothing_refers_to() {
        assert_eq!(word_count("text[^1]\n\n[^1]: used note words"), 4);
        assert_eq!(word_count("text\n\n[^1]: unused note words here"), 1);
        let at = format!(
            "{}\n\n[^9]: unused four word note",
            words(BODY_WORD_LIMIT - 2)
        );
        assert!(
            length_message(&at, &sections(&at)).is_none(),
            "an unused definition does not push a body over the limit"
        );
    }

    /// Review round 1 (F2): a section's words come from the one parse of
    /// the whole body, so a reference defined in another section resolves.
    #[test]
    fn section_counts_keep_references_defined_elsewhere() {
        let body = format!(
            "## Images\n![alt text of an image][img] and [a link here][lnk]\n\n## Big\n{}\n## Links\n[img]: x.png\n[lnk]: https://example.com\n",
            words(BODY_WORD_LIMIT + 100),
        );
        let message = length_message(&body, &sections(&body)).unwrap();
        let named = message.split("largest sections: ").nth(1).unwrap();
        assert!(
            named.starts_with("## Big (1100 words), ## Images (4 words)"),
            "{message}"
        );
        assert!(!named.contains("## Links"), "{message}");
        assert!(message.contains("is 1107 words"), "{message}");
    }

    /// Review round 1 (nit): equal sections keep their order in the body.
    #[test]
    fn length_message_keeps_body_order_between_equal_sections() {
        let body = format!(
            "## First\n{}\n## Second\n{}\n## Third\n{}\n## Fourth\n{}\n",
            words(400),
            words(400),
            words(400),
            words(400),
        );
        let message = length_message(&body, &sections(&body)).unwrap();
        assert!(
            message.ends_with("## First (400 words), ## Second (400 words), ## Third (400 words)"),
            "{message}"
        );
    }

    #[test]
    fn length_message_fires_only_over_the_limit() {
        let at = format!("## Summary\n{}", words(BODY_WORD_LIMIT - 1));
        assert_eq!(word_count(&at), BODY_WORD_LIMIT);
        assert!(length_message(&at, &sections(&at)).is_none());
        let over = format!("{at}word");
        let message = length_message(&over, &sections(&over)).unwrap();
        assert!(
            message.starts_with("PR body is 1001 words, over the 1000-word limit"),
            "{message}"
        );
        let commented = format!("{at}\n<!-- {} -->", words(500));
        assert!(length_message(&commented, &sections(&commented)).is_none());
    }

    /// TSK-228: the three largest `##` sections are named with their words;
    /// a subsection belongs to its parent and text before the first heading
    /// belongs to none.
    #[test]
    fn length_message_names_the_three_largest_sections() {
        let body = format!(
            "Task: TSK-001 {}\n## Summary\n{}\n## Changes\n{}\n## Testing\n{}\n### Detail\n{}\n## Reviews\n{}\n",
            words(5),
            words(10),
            words(300),
            words(100),
            words(400),
            words(450),
        );
        let message = length_message(&body, &sections(&body)).unwrap();
        let named = message.split("largest sections: ").nth(1).unwrap();
        assert_eq!(
            named, "## Testing (501 words), ## Reviews (450 words), ## Changes (300 words)",
            "{message}"
        );
        assert!(!message.contains("Detail"), "{message}");
        assert!(!message.contains("Summary"), "{message}");
    }

    #[test]
    fn length_message_without_sections_names_none() {
        let body = words(1200);
        let message = length_message(&body, &sections(&body)).unwrap();
        assert!(message.contains("1200 words"), "{message}");
        assert!(!message.contains("largest sections"), "{message}");
    }

    /// TSK-228: the warning rides the presentation check, so it is always a
    /// warning, names its remedy, and follows `git.pr_sections` being active.
    #[test]
    fn long_body_warns_at_warn_whatever_the_section_level() {
        let body = format!(
            "## Summary\nShort.\n## Testing\nNot tested: x.\n{}",
            words(1100)
        );
        for level in [PolicyLevel::Block, PolicyLevel::Warn] {
            let git = GitPolicy {
                pr_sections: level,
                ..GitPolicy::default()
            };
            let findings = presentation(&git, &body, true);
            let long: Vec<_> = findings
                .iter()
                .filter(|v| v.message.contains("-word limit"))
                .collect();
            assert_eq!(long.len(), 1, "{level}: {findings:?}");
            assert_eq!(long[0].level, PolicyLevel::Warn);
            assert!(long[0].remedy.to_string().contains("link records"));
        }
        for level in [PolicyLevel::Allow, PolicyLevel::Off] {
            let git = GitPolicy {
                pr_sections: level,
                ..GitPolicy::default()
            };
            assert!(presentation(&git, &body, true).is_empty(), "{level}");
        }
        let short = "## Summary\nShort.\n## Testing\nNot tested: x.\n";
        assert!(presentation(&GitPolicy::default(), short, true).is_empty());
    }

    /// ADR-0071 rule 7 (Codex TSK-108 review, R108-1): a Summary is judged
    /// by whether it anchors the reader, never by counting its sentences.
    #[test]
    fn presentation_never_counts_summary_sentences() {
        let body = "Task: none: isolated summary-warning review probe\n\n## Summary\n\n\
            The installer now preserves local settings. Existing projects can update safely. \
            Fresh projects keep the standard defaults. The change is ready for review.\n\n\
            ## Changes\n\n- Preserve local settings during updates.\n\n## Testing\n\n\
            Docs-only review fixture.\nNew tests: none.\nNot tested: live model behavior.\n";
        let findings = presentation(&GitPolicy::default(), body, false);
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn presentation_accepts_evidence_without_row_budget() {
        let body = "## Summary\nImproves the installer.\n## Testing\nNot tested: Windows.\n```\n12 passed\n```";
        assert!(presentation(&GitPolicy::default(), body, false).is_empty());
        let body = format!("{body}\n{}", "evidence\n".repeat(66));
        assert!(presentation(&GitPolicy::default(), &body, false).is_empty());
        assert!(presentation(&GitPolicy::default(), &body, true).is_empty());
        let comment = format!("{body}\n<!-- {} -->", "hidden\n".repeat(100));
        assert!(presentation(&GitPolicy::default(), &comment, true).is_empty());
        assert!(presentation(
            &GitPolicy::default(),
            "## Testing\n```\nNot tested: forged\n```",
            false
        )
        .iter()
        .any(|v| v.message.contains("Not tested:")));
        for label in [
            "**Not tested**: Windows.",
            "**Not tested:** Windows.",
            "_Not tested_: Windows.",
        ] {
            let body = format!("## Testing\nRan 3 tests.\n{label}");
            assert!(
                presentation(&GitPolicy::default(), &body, false).is_empty(),
                "{label}"
            );
        }
    }

    #[test]
    fn release_checks_minimum_impact_and_case_insensitive_values() {
        let levels = ["none", "patch", "minor", "major"];
        for (floor, level) in levels.iter().enumerate().skip(1) {
            let git = GitPolicy {
                pr_breaking_level: (*level).into(),
                ..GitPolicy::default()
            };
            for (rank, impact) in levels.iter().enumerate() {
                for breaking in ["No", "YES"] {
                    let body = declaration(
                        &impact.to_ascii_uppercase(),
                        breaking,
                        "Replace the old call.",
                    );
                    let findings = release(&git, &body, false);
                    assert_eq!(
                        findings.is_empty(),
                        breaking == "No" || rank >= floor,
                        "{level}: {body}: {findings:?}"
                    );
                    let marked = release(&git, &body, true);
                    assert_eq!(
                        marked.is_empty(),
                        breaking == "YES" && rank >= floor,
                        "{level}: {body}: {marked:?}"
                    );
                }
            }
        }
        for (impact, breaking, reason) in [
            ("huge", "no", "Impact must"),
            ("patch", "maybe", "Breaking must"),
            ("none | patch | minor | major", "yes | no", "Impact must"),
        ] {
            assert!(release(
                &GitPolicy::default(),
                &declaration(impact, breaking, "none"),
                false
            )
            .iter()
            .any(|v| v.message.contains(reason)));
        }
    }

    #[test]
    fn release_requires_real_fields_and_substantive_migration() {
        let git = GitPolicy::default();
        for body in [
            "<!-- ## Release impact -->",
            "```\n## Release impact\n```",
            "## Release impact\n<!-- - Impact: none -->",
        ] {
            assert!(!release(&git, body, false).is_empty());
        }
        let body = declaration("major", "yes", "none");
        for migration in [
            "none",
            "N/A",
            "TODO",
            "steps",
            "<steps>",
            "see Breaking change",
            "\"see Breaking change\"",
        ] {
            assert!(release(
                &git,
                &body.replace("Migration: none", &format!("Migration: {migration}")),
                false
            )
            .iter()
            .any(|v| v.message.contains("substantive Migration")));
        }
        let reference = declaration("major", "yes", "see Breaking change");
        assert!(release(
            &git,
            &format!("{reference}\n## Breaking change\nReplace the old call with the new call."),
            false
        )
        .is_empty());
        for extra in ["- Impact: patch", "## Release impact\n- Impact: patch"] {
            assert!(release(&git, &format!("{body}\n{extra}"), false)
                .iter()
                .any(|v| v.message.contains("duplicate") || v.message.contains("exactly one")));
        }
        let fake_fields = "## Release impact\n```\n- Impact: none\n- Breaking: no\n- Rationale: example\n- Migration: none\n```";
        assert!(release(&git, fake_fields, false)
            .iter()
            .any(|v| v.message.contains("requires a non-empty impact")));
        let clean = declaration("patch", "no", "none");
        assert!(release(
            &git,
            &format!("{clean}\n### Migration details\n- Migration: example"),
            false
        )
        .is_empty());
        let bold = "## Release impact\n- **Impact:** major\n- **Breaking**: yes\n- __Rationale:__ Remove the old flag.\n- *Migration*: Use the new flag.\n";
        assert!(release(&git, bold, false).is_empty(), "{bold}");
    }

    #[test]
    fn migration_guidance_matches_the_release_script() {
        // scripts/test_release.py SUBSTANTIVE_MIGRATIONS, minus the reference
        // form, which needs a Breaking change section here.
        for migration in [
            "Run the new command to convert saved records.",
            "\"docs/migrate.md\"",
            "Run `cat old.json | tool migrate` to convert saved records.",
            "Replace `<name>` with `--name <value>`.",
        ] {
            assert!(substantive(migration), "{migration}");
            let body = declaration("major", "yes", migration);
            assert!(
                release(&GitPolicy::default(), &body, false).is_empty(),
                "{body}"
            );
        }
        // QUOTED_MIGRATION_PLACEHOLDERS and the template's unresolved choice.
        for placeholder in [
            "\"none\"",
            "'TODO'",
            "`\" N/A \"`",
            "\"  TBD  \"",
            "'-'",
            "\"\"",
            "'   '",
            "<steps>",
            "`none`, steps, or \"see Breaking change\"",
            "none | steps | \"see Breaking change\"",
        ] {
            assert!(!substantive(placeholder), "{placeholder}");
        }
    }

    /// TSK-147 round 6: a legacy `Contract: compatible` settles a watched
    /// surface as `Breaking: no` does; `not-applicable` there is refused by
    /// `scripts/release.py check-pr`, so it settles nothing.
    #[test]
    fn a_legacy_contract_declares_no_break_as_breaking_would() {
        let block = |lines: &str| {
            format!("## Release impact\n{lines}- Rationale: Preserve the public behavior.\n")
        };
        for no_break in [
            "- Contract: compatible\n",
            "- Contract: Compatible\n",
            "- Contract: compatible\n- Breaking: no\n",
        ] {
            assert!(declares_no_break(&block(no_break)), "{no_break}");
        }
        for not_settled in [
            "- Contract: breaking\n",
            "- Contract: breaking\n- Breaking: no\n",
            "- Contract: compatible\n- Breaking: yes\n",
            "- Contract: maybe\n",
            "- Contract: not-applicable\n",
            "- Contract: not-applicable\n- Breaking: no\n",
            "- Contract: compatible\n- Contract: compatible\n",
            "- Breaking: no\n- Breaking: no\n",
        ] {
            assert!(!declares_no_break(&block(not_settled)), "{not_settled}");
        }
    }

    /// TSK-106 AC-8: the shared fixture set; `scripts/test_release.py` runs
    /// the same cases through `release.py`, which reads them with this
    /// reader (`codeflow ci --read-release-impact`), and the seeded corpus
    /// in `tests/release_impact_corpus.rs` compares the two verdicts.
    #[test]
    fn release_impact_block_passes_the_shared_fixture_set() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../scripts/fixtures/release_impact_cases.json"
        ))
        .unwrap();
        let cases = fixtures["cases"].as_array().unwrap();
        assert!(cases.len() >= 20, "the shared set covers the block");
        let mut disagreements = Vec::new();
        for case in cases {
            let body = case["body"].as_str().unwrap();
            let valid = case["valid"].as_bool().unwrap();
            let findings = release(&GitPolicy::default(), body, false);
            if findings.is_empty() != valid {
                disagreements.push(format!(
                    "{}: expected valid={valid}, got {:?}",
                    case["name"],
                    findings
                        .iter()
                        .map(|f| f.message.clone())
                        .collect::<Vec<_>>()
                ));
            }
        }
        assert!(disagreements.is_empty(), "{disagreements:#?}");
    }

    #[test]
    fn release_commit_floor_and_severity_are_independent_of_structure() {
        let body = declaration("patch", "no", "none");
        for level in [
            PolicyLevel::Warn,
            PolicyLevel::Block,
            PolicyLevel::Off,
            PolicyLevel::Allow,
        ] {
            let git = GitPolicy {
                pr_sections: PolicyLevel::Off,
                pr_release_impact: level,
                ..GitPolicy::default()
            };
            let findings = release(&git, &body, true);
            assert_eq!(findings.is_empty(), !level.is_active());
            assert!(findings.iter().all(|v| v.level == level));
            if level.is_active() {
                assert!(findings
                    .iter()
                    .any(|v| v.message.contains("breaking commit marker")));
            }
        }
        let git = GitPolicy {
            pr_breaking_level: "minor".into(),
            ..GitPolicy::default()
        };
        assert!(release(
            &git,
            &declaration("minor", "yes", "Rename the removed option."),
            true
        )
        .is_empty());
    }
}
