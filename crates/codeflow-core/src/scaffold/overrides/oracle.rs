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

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag};

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
    rows_kept && outside(pristine, &before, in_pristine) == outside(effective, &after, in_effective)
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
