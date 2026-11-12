//! An independent structural check of an override result (TSK-1302).
//!
//! The section parser in [`super::sections`] and the byte comparison in
//! [`super::only_units_changed`] decide where units are and that no byte
//! outside them changed. Neither shows that Markdown still reads those bytes
//! as the same blocks: a replacement can leave a construct open that turns
//! the protected text after it into code. This check parses the shipped text
//! and the result with `pulldown_cmark`, a `CommonMark` parser that shares
//! no code with the section parser, and requires every block outside the
//! targeted units, headings and raw HTML blocks such as section markers
//! included, to be the same block at the same place. Inside a targeted
//! moment row it requires every cell but the action cell to read the same.
//! It also requires that no form whose source the rendered page does not
//! show touches a replaced unit ([`hidden_forms`]), since such a form would
//! show a reviewer less than an agent reads.

use std::ops::Range;

use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd,
};

use super::sections::{is_invisible, Hidden};
use super::{unit_ranges, AppliedUnit, Target};

/// Where a block boundary sits, in the shipped text's coordinates: at a
/// byte outside every unit, or strictly inside unit `n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    At(usize),
    In(usize),
}

/// One block as the check compares it: its nesting depth, its kind and its
/// two boundaries.
type Block = (usize, String, Place, Place);

/// Every block of `text` with its byte range: each block-level start, the
/// thematic breaks, at their nesting depth.
fn blocks(text: &str) -> Vec<(usize, String, Range<usize>)> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    for (event, range) in Parser::new_ext(text, Options::ENABLE_TABLES).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if is_block(&tag) {
                    out.push((depth, format!("{tag:?}"), range));
                }
                depth += 1;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Rule => out.push((depth, "Rule".to_string(), range)),
            _ => {}
        }
    }
    out
}

fn is_block(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::List(_)
            | Tag::Item
            | Tag::FootnoteDefinition(_)
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::MetadataBlock(_)
    )
}

/// The blocks of one side that are not wholly inside a unit, with their
/// boundaries placed by `place`.
fn outside(text: &str, spans: &[Range<usize>], place: impl Fn(usize) -> Place) -> Vec<Block> {
    blocks(text)
        .into_iter()
        .filter(|(_, _, range)| {
            !spans
                .iter()
                .any(|span| span.start <= range.start && range.end <= span.end)
        })
        .map(|(depth, kind, range)| (depth, kind, place(range.start), place(range.end)))
        .collect()
}

/// Whether `effective`, which is `pristine` with `units` applied, reads as
/// the same Markdown blocks as `pristine` everywhere outside the units.
#[must_use]
pub fn same_structure(pristine: &str, effective: &str, units: &[AppliedUnit]) -> bool {
    let Some(ranges) = unit_ranges(pristine, units) else {
        return false;
    };
    // Each unit's range in `pristine` and in `effective`: the units are in
    // order of position, and each replacement moves the ones after it.
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut removed = 0usize;
    let mut added = 0usize;
    for (range, unit) in &ranges {
        before.push(range.clone());
        let start = range.start - removed + added;
        after.push(start..start + unit.replacement.len());
        removed += range.len();
        added += unit.replacement.len();
    }
    let in_pristine = |at: usize| {
        before
            .iter()
            .position(|span| span.start < at && at < span.end)
            .map_or(Place::At(at), Place::In)
    };
    let in_effective = |at: usize| {
        if let Some(n) = after
            .iter()
            .position(|span| span.start < at && at < span.end)
        {
            return Place::In(n);
        }
        // Outside every unit: back to `pristine` by the units before `at`.
        let (mut removed, mut added) = (0usize, 0usize);
        for (n, span) in after.iter().enumerate() {
            if span.end <= at {
                removed += before[n].len();
                added += span.len();
            }
        }
        Place::At(at + removed - added)
    };
    // Inside a moment row only the action cell is the project's: every other
    // cell, the pointer cell with its `MUST OPEN` marks included, reads the
    // same on both sides.
    let rows_kept = ranges.iter().enumerate().all(|(n, (_, unit))| {
        !matches!(Target::parse(&unit.entry.target), Some(Target::Moment(_)))
            || kept_cells(&cells(pristine, &before[n]), &cells(effective, &after[n]))
    });
    // No form the rendered page does not show touches a unit: the project's
    // text is Markdown a reviewer sees as written. A form that starts in
    // shipped text and runs into a unit counts too.
    let visible = hidden_forms(effective).iter().all(|(_, range)| {
        !after
            .iter()
            .any(|span| range.start < span.end && span.start < range.end.max(range.start + 1))
    });
    rows_kept
        && visible
        && outside(pristine, &before, in_pristine) == outside(effective, &after, in_effective)
}

/// Every extension the parser supports, so that a form any renderer reads
/// is seen. Smart punctuation is left out: it changes visible text and
/// hides none. Old-style footnotes replace the default footnote syntax, so
/// [`hidden_forms`] parses a second time with them.
fn every_extension() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_WIKILINKS
}

/// Every form in `text` whose source the rendered page does not show in
/// full, with its byte range, in order of position (TSK-1302).
///
/// This is the one visibility rule for override text: section bodies, row
/// values, each unit of an override result and persisted records all reach
/// it. It has two parts.
///
/// - An allow-list of forms. A form is accepted only when all of its source
///   renders as visible text, or it is plain structure: paragraphs, headings
///   without attributes, emphasis, strong, strikethrough, superscript,
///   subscript, lists, task markers, tables, block quotes (with or without a
///   GitHub alert label), definition lists, breaks, rules, code spans, fenced
///   code whose info string is empty or one short language token, indented
///   code and autolinks. Every other form is returned as its [`Hidden`]
///   class, and so is text that decodes to an invisible character.
/// - A coverage check of the source. The parser keeps no visible place for
///   some source text and emits no event for it, so a rule over events alone
///   does not see it. Every character that is not ASCII whitespace or ASCII
///   punctuation, the characters Markdown syntax is written in, must fall
///   inside the source of something the page shows ([`Places`]); any other
///   is returned as [`Hidden::Unrendered`].
///
/// A form inside a code span or a code fence is text, shown as written.
pub(super) fn hidden_forms(text: &str) -> Vec<(Hidden, Range<usize>)> {
    let mut out = Vec::new();
    let all = every_extension();
    for options in [all, all | Options::ENABLE_OLD_FOOTNOTES] {
        hidden_with(text, options, &mut out);
    }
    out.sort_by_key(|(_, range)| range.start);
    out
}

/// The first form [`hidden_forms`] finds in `text`, and the byte it starts
/// at.
pub(super) fn first_hidden(text: &str) -> Option<(Hidden, usize)> {
    hidden_forms(text)
        .first()
        .map(|(class, range)| (*class, range.start))
}

/// [`hidden_forms`] under one set of parser options, appended to `out`.
fn hidden_with(text: &str, options: Options, out: &mut Vec<(Hidden, Range<usize>)>) {
    let parser = Parser::new_ext(text, options);
    // A reference definition is read before the events and emits none.
    out.extend(
        parser
            .reference_definitions()
            .iter()
            .map(|(_, definition)| (Hidden::ReferenceDefinition, definition.span.clone())),
    );
    // The links and images open at this point: the class each takes when it
    // closes (none for an autolink, or when a title was already refused),
    // where it starts, and whether visible text has shown inside it.
    let mut open: Vec<(Option<Hidden>, Range<usize>, bool)> = Vec::new();
    let mut places = Places::new(text);
    for (event, range) in parser.into_offset_iter() {
        places.see(&event, &range);
        let found = match &event {
            Event::Start(tag) => match tag {
                Tag::Paragraph
                | Tag::BlockQuote(_)
                | Tag::CodeBlock(CodeBlockKind::Indented)
                | Tag::List(_)
                | Tag::Item
                | Tag::DefinitionList
                | Tag::DefinitionListTitle
                | Tag::DefinitionListDefinition
                | Tag::Table(_)
                | Tag::TableHead
                | Tag::TableRow
                | Tag::TableCell
                | Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Superscript
                | Tag::Subscript => None,
                Tag::Heading {
                    id, classes, attrs, ..
                } => (id.is_some() || !classes.is_empty() || !attrs.is_empty())
                    .then_some(Hidden::HeadingAttributes),
                Tag::CodeBlock(CodeBlockKind::Fenced(info)) => {
                    (!language_token(text, &range, info)).then_some(Hidden::FenceInfo)
                }
                Tag::Link {
                    link_type, title, ..
                }
                | Tag::Image {
                    link_type, title, ..
                } => {
                    if title.is_empty() {
                        let class = if matches!(tag, Tag::Image { .. }) {
                            Some(Hidden::Image)
                        } else if matches!(link_type, LinkType::Autolink | LinkType::Email) {
                            None
                        } else {
                            Some(Hidden::Link)
                        };
                        open.push((class, range.clone(), false));
                        None
                    } else {
                        open.push((None, range.clone(), true));
                        Some(Hidden::LinkTitle)
                    }
                }
                Tag::HtmlBlock => Some(Hidden::RawHtml),
                Tag::FootnoteDefinition(_) => Some(Hidden::Footnote),
                Tag::MetadataBlock(_) => Some(Hidden::Metadata),
            },
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if let Some((class, start, shown)) = open.pop() {
                    if !shown {
                        out.push((Hidden::EmptyLinkText, start));
                    } else if let Some(class) = class {
                        out.push((class, start));
                    }
                }
                None
            }
            Event::Text(text) | Event::Code(text) => {
                if !text.trim().is_empty() {
                    for (_, _, shown) in &mut open {
                        *shown = true;
                    }
                }
                text.chars()
                    .any(is_invisible)
                    .then_some(Hidden::ReferencedInvisible)
            }
            Event::InlineMath(_) | Event::DisplayMath(_) => Some(Hidden::Math),
            Event::Html(_) | Event::InlineHtml(_) => Some(Hidden::RawHtml),
            Event::FootnoteReference(_) => Some(Hidden::Footnote),
            Event::End(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::TaskListMarker(_) => None,
        };
        if let Some(class) = found {
            out.push((class, range));
        }
    }
    out.extend(places.unshown());
}

/// The longest fence info string accepted, in bytes.
const LANGUAGE_TOKEN_MAX: usize = 20;

/// Whether the fence that opens at `range` of `text`, with the decoded info
/// string `info`, carries no info string or one short language token: ASCII
/// letters, digits, `+`, `#`, `-` and `.`, at most [`LANGUAGE_TOKEN_MAX`]
/// bytes, written as it decodes (no escape or character reference). A
/// renderer shows no part of the info string as text.
fn language_token(text: &str, range: &Range<usize>, info: &str) -> bool {
    fence_info_source(text, range) == info
        && info.len() <= LANGUAGE_TOKEN_MAX
        && info
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'#' | b'-' | b'.'))
}

/// The info string of the fence that opens at `range` of `text`, as written:
/// the opening line after its run of backticks or tildes, trimmed.
fn fence_info_source<'a>(text: &'a str, range: &Range<usize>) -> &'a str {
    let fence = first_line(text, range).trim_start_matches([' ', '\t']);
    let info = if fence.starts_with('~') {
        fence.trim_start_matches('~')
    } else {
        fence.trim_start_matches('`')
    };
    info.trim_matches([' ', '\t'])
}

/// The first line of `range` in `text`, without its line break.
fn first_line<'a>(text: &'a str, range: &Range<usize>) -> &'a str {
    text.get(range.clone())
        .unwrap_or_default()
        .split('\n')
        .next()
        .unwrap_or_default()
}

/// Whether `c` must fall inside a shown place: every character but ASCII
/// whitespace and ASCII punctuation, which is what Markdown syntax is
/// written in. Letters and digits always need one, and so does any other
/// character, Unicode whitespace included.
fn needs_place(c: char) -> bool {
    !c.is_ascii_whitespace() && !c.is_ascii_punctuation()
}

/// The source places the rendered page shows, collected from the parser's
/// events (TSK-1302).
///
/// The parser drops some source text without an event, so a check that
/// judges events alone cannot see it. This records, byte by byte, the
/// source of everything the page shows; [`Places::unshown`] then returns
/// each run of characters outside all of it that is not syntax.
struct Places<'a> {
    text: &'a str,
    shown: Vec<bool>,
    /// The lists open at this point: for an ordered list, its start number
    /// and how many items it has had; `None` for a bullet list.
    lists: Vec<Option<(u64, u64)>>,
}

impl<'a> Places<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            shown: vec![false; text.len()],
            lists: Vec::new(),
        }
    }

    fn show(&mut self, range: Range<usize>) {
        let end = range.end.min(self.shown.len());
        if let Some(bytes) = self.shown.get_mut(range.start.min(end)..end) {
            bytes.fill(true);
        }
    }

    /// Record what `event`, at `range`, shows on the page.
    ///
    /// - Text and code spans show their source, a character reference or an
    ///   escape included.
    /// - A task marker shows as a checkbox; its only letter is `x` or `X`.
    /// - A fence shows its opening line's info string as no text, so the
    ///   line has a place only when the string is a language token
    ///   ([`language_token`]); any other is refused as
    ///   [`Hidden::FenceInfo`].
    /// - An ordered list item's number has a place when it is the number the
    ///   page shows (the start plus the item's place) or the list's start
    ///   repeated, the two usual ways to number a list; either carries
    ///   nothing the page does not show.
    /// - A GitHub alert label is one of five fixed words, which the page
    ///   shows as the alert's title.
    ///
    /// Nothing else marks a place. The source of a form the allow-list
    /// refuses keeps that form's class, which starts no later.
    fn see(&mut self, event: &Event<'_>, range: &Range<usize>) {
        match event {
            Event::Text(_) | Event::Code(_) | Event::TaskListMarker(_) => {
                self.show(range.clone());
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                if language_token(self.text, range, info) {
                    let line = first_line(self.text, range).len();
                    self.show(range.start..range.start + line);
                }
            }
            Event::Start(Tag::List(start)) => self.lists.push(start.map(|start| (start, 0))),
            Event::End(TagEnd::List(_)) => {
                self.lists.pop();
            }
            Event::Start(Tag::Item) => {
                if let Some(Some((start, count))) = self.lists.last_mut() {
                    let (start, place) = (*start, start.checked_add(*count));
                    *count += 1;
                    if let Some(len) = item_number(self.text, range.start)
                        .filter(|(number, _)| *number == start || Some(*number) == place)
                        .map(|(_, len)| len)
                    {
                        self.show(range.start..range.start + len);
                    }
                }
            }
            Event::Start(Tag::BlockQuote(Some(kind))) => {
                if let Some(label) = alert_label(self.text, range, *kind) {
                    self.show(label);
                }
            }
            _ => {}
        }
    }

    /// Each run of characters that needs a place and has none, as
    /// [`Hidden::Unrendered`].
    fn unshown(&self) -> Vec<(Hidden, Range<usize>)> {
        let mut out = Vec::new();
        let mut run: Option<Range<usize>> = None;
        for (at, c) in self.text.char_indices() {
            if needs_place(c) && !self.shown[at] {
                let end = at + c.len_utf8();
                run = Some(run.map_or(at..end, |open| open.start..end));
            } else if let Some(open) = run.take() {
                out.push((Hidden::Unrendered, open));
            }
        }
        out.extend(run.map(|open| (Hidden::Unrendered, open)));
        out
    }
}

/// The number an ordered list item that starts at byte `at` of `text` is
/// written with, and the length of its digits.
fn item_number(text: &str, at: usize) -> Option<(u64, usize)> {
    let rest = text.get(at..)?;
    let len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=9).contains(&len) || !rest[len..].starts_with(['.', ')']) {
        return None;
    }
    Some((rest[..len].parse().ok()?, len))
}

/// The bytes of the GitHub alert label `[!WORD]` on the first line of the
/// block quote at `range`, when its word is the one `kind` names.
fn alert_label(text: &str, range: &Range<usize>, kind: BlockQuoteKind) -> Option<Range<usize>> {
    let line = first_line(text, range);
    let open = line.find("[!")?;
    let close = open + line[open..].find(']')?;
    let word = match kind {
        BlockQuoteKind::Note => "note",
        BlockQuoteKind::Tip => "tip",
        BlockQuoteKind::Important => "important",
        BlockQuoteKind::Warning => "warning",
        BlockQuoteKind::Caution => "caution",
    };
    line[open + 2..close]
        .eq_ignore_ascii_case(word)
        .then(|| range.start + open..range.start + close + 1)
}

/// The bytes of every table cell that starts inside `span`, as Markdown
/// reads the cells; padding counts, so any change to a cell shows.
fn cells<'a>(text: &'a str, span: &Range<usize>) -> Vec<&'a str> {
    Parser::new_ext(text, Options::ENABLE_TABLES)
        .into_offset_iter()
        .filter(|(event, range)| {
            matches!(event, Event::Start(Tag::TableCell)) && span.contains(&range.start)
        })
        .map(|(_, range)| &text[range])
        .collect()
}

/// Whether a row with `effective` cells keeps every cell of `pristine` but
/// the action cell, the second.
fn kept_cells(pristine: &[&str], effective: &[&str]) -> bool {
    pristine.len() >= 2
        && pristine.len() == effective.len()
        && pristine
            .iter()
            .zip(effective)
            .enumerate()
            .all(|(n, (before, after))| n == 1 || before == after)
}
