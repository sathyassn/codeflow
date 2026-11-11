//! Declared sections of a managed skill (TSK-1302).
//!
//! A managed skill declares an overridable (or fixed, or floor) section with
//! a marker on the line right above its heading:
//!
//! ```text
//! <!-- codeflow:section id=the-loop class=overridable -->
//! ## The loop
//! ...body...
//! ```
//!
//! The section's body runs from the line after the heading to the line
//! before the next marker, the next heading of the same or a higher level
//! outside a code fence, or the end of the file. Lower-level headings belong
//! to the body. A document parses into ordered segments: text outside any
//! declared section, and the declared sections, so a replacement can be
//! checked segment by segment against the shipped text.
//!
//! The parser is strict, because names and ranges come only from the shipped
//! text: a malformed marker, a marker not followed by a heading, a duplicate
//! id, a marker inside a code fence or a declared section nested inside
//! another is an error, never a guess.

use crate::scaffold::rule_map::UnitClass;

/// What makes a skill's section markers unusable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionError {
    /// A line that starts a section marker but does not parse as one.
    MalformedMarker { line: usize },
    /// A marker whose next line is not a heading.
    NoHeading { line: usize },
    /// Two markers with one id.
    DuplicateId { line: usize },
    /// A marker inside a fenced code block.
    MarkerInFence { line: usize },
    /// A declared section whose heading is deeper than the declared section
    /// it would sit inside.
    Nested { line: usize },
    /// A setext heading inside a declared section at the section's level or
    /// higher: declared sections end only at ATX headings.
    Setext { line: usize },
}

impl SectionError {
    /// The 1-based line the error is on.
    #[must_use]
    pub const fn line(&self) -> usize {
        match self {
            Self::MalformedMarker { line }
            | Self::NoHeading { line }
            | Self::DuplicateId { line }
            | Self::MarkerInFence { line }
            | Self::Nested { line }
            | Self::Setext { line } => *line,
        }
    }

    /// What is wrong, in compiled-in words.
    #[must_use]
    pub const fn what(&self) -> &'static str {
        match self {
            Self::MalformedMarker { .. } => "a section marker does not parse",
            Self::NoHeading { .. } => "a section marker is not followed by a heading",
            Self::DuplicateId { .. } => "two section markers share one id",
            Self::MarkerInFence { .. } => "a section marker is inside a code fence",
            Self::Nested { .. } => "a declared section is nested inside another",
            Self::Setext { .. } => {
                "a declared section holds a setext heading at its level or higher"
            }
        }
    }
}

/// One declared section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The marker's id.
    pub id: String,
    /// The marker's class; `None` when the marker states none, which is
    /// never overridden.
    pub class: Option<UnitClass>,
    /// The marker line and the heading line, line breaks included.
    pub header: String,
    /// The heading's level, 1 to 6.
    pub level: usize,
    /// The body, line breaks included.
    pub body: String,
}

/// A part of a parsed document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Text that belongs to no declared section.
    Outside(String),
    /// A declared section.
    Section(Section),
}

/// A document split into its segments, in order. Joining the segments gives
/// the document back byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    pub segments: Vec<Segment>,
}

const MARKER_START: &str = "<!-- codeflow:section";

/// Whether `line` starts a section marker (well formed or not).
fn is_marker_line(line: &str) -> bool {
    line.trim().starts_with(MARKER_START)
}

/// The level of a Markdown ATX heading, if `line` is one: up to three
/// spaces, then one to six `#`, then a space or the end of the line.
#[must_use]
pub fn heading_level(line: &str) -> Option<usize> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let hashes = rest.len() - rest.trim_start_matches('#').len();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let after = &rest[hashes..];
    let after = after.trim_end_matches(['\n', '\r']);
    (after.is_empty() || after.starts_with(' ') || after.starts_with('\t')).then_some(hashes)
}

/// An open fenced code block: its delimiter character and run length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fence {
    ch: u8,
    len: usize,
}

/// The delimiter run of a fence line: up to three spaces, then three or more
/// backticks or tildes. Returns the character, the run length and the rest
/// of the line without its line break.
fn fence_run(line: &str) -> Option<(u8, usize, &str)> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let ch = *rest.as_bytes().first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let len = rest.bytes().take_while(|&b| b == ch).count();
    (len >= 3).then(|| (ch, len, &rest[len..]))
}

/// The fence `line` opens, as `CommonMark` reads it: a backtick fence's info
/// string holds no backtick.
#[must_use]
pub fn fence_opening(line: &str) -> Option<Fence> {
    let (ch, len, info) = fence_run(line)?;
    (ch == b'~' || !info.contains('`')).then_some(Fence { ch, len })
}

/// Whether `line` closes `fence`: the same character, a run at least as
/// long as the opening one, and nothing after it but spaces and tabs.
#[must_use]
pub fn closes(fence: Fence, line: &str) -> bool {
    fence_run(line).is_some_and(|(ch, len, rest)| {
        ch == fence.ch && len >= fence.len && rest.bytes().all(|b| b == b' ' || b == b'\t')
    })
}

/// The fence state after `line`, and whether `line` is a fence delimiter.
#[must_use]
pub fn step_fence(open: Option<Fence>, line: &str) -> (Option<Fence>, bool) {
    match open {
        Some(fence) if closes(fence, line) => (None, true),
        Some(fence) => (Some(fence), false),
        None => match fence_opening(line) {
            Some(fence) => (Some(fence), true),
            None => (None, false),
        },
    }
}

/// The level of a setext heading underline, if `line` is one: up to three
/// spaces, then a run of `=` (level 1) or `-` (level 2), then only spaces
/// and tabs. It underlines the line before it when that line is paragraph
/// text, which the caller decides.
#[must_use]
pub fn setext_level(line: &str) -> Option<usize> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let ch = *rest.as_bytes().first()?;
    let level = match ch {
        b'=' => 1,
        b'-' => 2,
        _ => return None,
    };
    let run = rest.bytes().take_while(|&b| b == ch).count();
    rest.as_bytes()[run..]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
        .then_some(level)
}

/// Whether `line` can be the text line a setext underline turns into a
/// heading: not blank, not an ATX heading and not a fence delimiter. This
/// reads more lines as paragraph text than `CommonMark` does, which only
/// widens what a replacement body refuses.
fn may_be_paragraph(line: &str) -> bool {
    !line.bytes().all(|b| b.is_ascii_whitespace())
        && heading_level(line).is_none()
        && fence_run(line).is_none()
        && setext_level(line).is_none()
}

/// The end condition of a raw HTML block `line` starts (`CommonMark` block
/// types 1 to 5), which runs past blank lines and headings until a line
/// holds it.
fn html_block_end(line: &str) -> Option<&'static str> {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let lower = rest.to_ascii_lowercase();
    for tag in ["script", "pre", "style", "textarea"] {
        if let Some(after) = lower.strip_prefix('<').and_then(|l| l.strip_prefix(tag)) {
            if after.is_empty() || after.starts_with([' ', '\t', '>', '\n', '\r']) {
                return Some(match tag {
                    "script" => "</script>",
                    "pre" => "</pre>",
                    "style" => "</style>",
                    _ => "</textarea>",
                });
            }
        }
    }
    if lower.starts_with("<!--") {
        return Some("-->");
    }
    if lower.starts_with("<?") {
        return Some("?>");
    }
    if lower.starts_with("<![cdata[") {
        return Some("]]>");
    }
    if lower
        .strip_prefix("<!")
        .and_then(|after| after.bytes().next())
        .is_some_and(|b| b.is_ascii_alphabetic())
    {
        return Some(">");
    }
    None
}

fn parse_marker(line: &str) -> Option<(String, Option<UnitClass>)> {
    let inner = line
        .trim()
        .strip_prefix(MARKER_START)?
        .strip_suffix("-->")?;
    let mut id = None;
    let mut class = None;
    for part in inner.split_whitespace() {
        let (key, value) = part.split_once('=')?;
        match key {
            "id" if id.is_none() && valid_id(value) => id = Some(value.to_string()),
            "class" if class.is_none() => {
                class = Some(match value {
                    "floor" => UnitClass::Floor,
                    "fixed" => UnitClass::Fixed,
                    "overridable" => UnitClass::Overridable,
                    _ => return None,
                });
            }
            _ => return None,
        }
    }
    Some((id?, class))
}

/// Whether `id` is a section id: lower-case letters, digits and hyphens,
/// starting with a letter or digit.
#[must_use]
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Parse `text` into segments.
///
/// # Errors
///
/// The first [`SectionError`] found, by line.
pub fn parse(text: &str) -> Result<Doc, SectionError> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut segments = Vec::new();
    let mut outside = String::new();
    let mut ids = std::collections::BTreeSet::new();
    let mut in_fence: Option<Fence> = None;
    let mut index = 0;
    // The level of the declared section that just ended at a marker, to
    // refuse a deeper marker that would sit inside it.
    let mut ended_at_marker: Option<usize> = None;
    while index < lines.len() {
        let line = lines[index];
        if is_marker_line(line) {
            if in_fence.is_some() {
                return Err(SectionError::MarkerInFence { line: index + 1 });
            }
            let Some((id, class)) = parse_marker(line) else {
                return Err(SectionError::MalformedMarker { line: index + 1 });
            };
            let Some(heading) = lines.get(index + 1).copied() else {
                return Err(SectionError::NoHeading { line: index + 1 });
            };
            let Some(level) = heading_level(heading) else {
                return Err(SectionError::NoHeading { line: index + 1 });
            };
            if ended_at_marker.is_some_and(|outer| level > outer) {
                return Err(SectionError::Nested { line: index + 1 });
            }
            if !ids.insert(id.clone()) {
                return Err(SectionError::DuplicateId { line: index + 1 });
            }
            if !outside.is_empty() {
                segments.push(Segment::Outside(std::mem::take(&mut outside)));
            }
            let header = format!("{line}{heading}");
            index += 2;
            let mut body = String::new();
            let mut fence: Option<Fence> = None;
            // The line before, when it may be paragraph text a setext
            // underline turns into a heading; the heading line never is.
            let mut paragraph = false;
            ended_at_marker = None;
            while index < lines.len() {
                let next = lines[index];
                if is_marker_line(next) {
                    if fence.is_some() {
                        return Err(SectionError::MarkerInFence { line: index + 1 });
                    }
                    ended_at_marker = Some(level);
                    break;
                }
                let was_open = fence.is_some();
                let (after, delimiter) = step_fence(fence, next);
                fence = after;
                if !was_open && !delimiter {
                    if heading_level(next).is_some_and(|l| l <= level) {
                        break;
                    }
                    if paragraph && setext_level(next).is_some_and(|l| l <= level) {
                        return Err(SectionError::Setext { line: index + 1 });
                    }
                }
                paragraph = !was_open && !delimiter && may_be_paragraph(next);
                body.push_str(next);
                index += 1;
            }
            in_fence = fence;
            segments.push(Segment::Section(Section {
                id,
                class,
                header,
                level,
                body,
            }));
            continue;
        }
        ended_at_marker = None;
        in_fence = step_fence(in_fence, line).0;
        outside.push_str(line);
        index += 1;
    }
    if !outside.is_empty() {
        segments.push(Segment::Outside(outside));
    }
    Ok(Doc { segments })
}

impl Doc {
    /// The document text, joined back from its segments.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Outside(text) => out.push_str(text),
                Segment::Section(section) => {
                    out.push_str(&section.header);
                    out.push_str(&section.body);
                }
            }
        }
        out
    }

    /// The byte range of the body of the declared section with `id` in
    /// [`Doc::text`].
    #[must_use]
    pub fn body_range(&self, id: &str) -> Option<std::ops::Range<usize>> {
        let mut offset = 0;
        for segment in &self.segments {
            match segment {
                Segment::Outside(text) => offset += text.len(),
                Segment::Section(section) => {
                    let start = offset + section.header.len();
                    let end = start + section.body.len();
                    if section.id == id {
                        return Some(start..end);
                    }
                    offset = end;
                }
            }
        }
        None
    }

    /// The declared section with `id`.
    #[must_use]
    pub fn section(&self, id: &str) -> Option<&Section> {
        self.sections().find(|section| section.id == id)
    }

    /// Every declared section, in order.
    pub fn sections(&self) -> impl Iterator<Item = &Section> {
        self.segments.iter().filter_map(|segment| match segment {
            Segment::Section(section) => Some(section),
            Segment::Outside(_) => None,
        })
    }
}

/// Why a replacement body is refused before it is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyProblem {
    /// The body is empty.
    Empty,
    /// A line holds `codeflow:`, which every section marker and managed
    /// region marker carries.
    StructuralMarker,
    /// A heading at the section's level or higher, which would end the
    /// section early.
    EndsSection,
    /// An open code fence, which would hide the headings and markers after it.
    OpenFence,
    /// A raw HTML block (a comment, `<pre>`, `<script>` and the like) left
    /// open, which would hide the text after it in the same way.
    OpenHtmlBlock,
    /// A carriage return, which Markdown reads as a line break.
    CarriageReturn,
    /// A code fence or raw HTML block that starts inside a list item or a
    /// block quote, or under indentation.
    NestedBlock,
    /// An indented code block.
    IndentedCode,
    /// A list item or block quote still open at the end of the body.
    OpenContainer,
}

impl BodyProblem {
    /// What is wrong, in compiled-in words.
    #[must_use]
    pub const fn what(self) -> &'static str {
        match self {
            Self::Empty => "the replacement body is empty",
            Self::StructuralMarker => {
                "a line of the replacement holds a codeflow marker, which could move a section or managed-region boundary"
            }
            Self::EndsSection => {
                "the replacement holds a heading at the section's level or higher, which would end the section early"
            }
            Self::OpenFence => {
                "the replacement leaves a code fence open, which would hide the headings and markers after it"
            }
            Self::OpenHtmlBlock => {
                "the replacement leaves a raw HTML block open, which would hide the headings and markers after it"
            }
            Self::CarriageReturn => {
                "the replacement holds a carriage return, which Markdown reads as a line break; use plain line feeds"
            }
            Self::NestedBlock => {
                "the replacement starts a code fence or raw HTML block inside a list item or block quote, or under indentation; \
                 start it at the left margin, outside any list or quote"
            }
            Self::IndentedCode => {
                "the replacement holds an indented code block; use a fenced code block at the left margin"
            }
            Self::OpenContainer => {
                "the replacement ends inside a list item or block quote; end it with a blank line"
            }
        }
    }
}

/// `line` without the list markers, block quote markers and indentation it
/// starts with, and whether it had any.
fn container_prefix(line: &str) -> (bool, &str) {
    let mut rest = line;
    loop {
        let trimmed = rest.trim_start_matches([' ', '\t']);
        let trimmed = if let Some(after) = trimmed.strip_prefix('>') {
            after
        } else if let Some(after) = trimmed
            .strip_prefix(['-', '+', '*'])
            .filter(|after| after.starts_with([' ', '\t']))
        {
            after
        } else {
            let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
            match trimmed[digits..].strip_prefix(['.', ')']) {
                Some(after) if (1..=9).contains(&digits) && after.starts_with([' ', '\t']) => after,
                _ => trimmed,
            }
        };
        if trimmed.len() == rest.len() {
            return (rest.len() != line.len(), rest);
        }
        rest = trimmed;
    }
}

/// Whether `line` is blank: nothing but spaces, tabs and its line break.
fn is_blank(line: &str) -> bool {
    line.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n'))
}

/// Check a replacement body for a section whose heading has `level`.
///
/// # Errors
///
/// The first [`BodyProblem`].
pub fn check_body(body: &str, level: usize) -> Result<(), BodyProblem> {
    if body.trim().is_empty() {
        return Err(BodyProblem::Empty);
    }
    if body.contains('\r') {
        return Err(BodyProblem::CarriageReturn);
    }
    let mut fence: Option<Fence> = None;
    let mut html: Option<&str> = None;
    // The line before may be paragraph text; the shipped heading line before
    // the body never is.
    let mut paragraph = false;
    // The line before is blank (the heading line before the body counts).
    let mut after_blank = true;
    // A list item or block quote is open.
    let mut container = false;
    for line in body.split_inclusive('\n') {
        if line.to_ascii_lowercase().contains("codeflow:") {
            return Err(BodyProblem::StructuralMarker);
        }
        // Ambiguous forms are refused wherever they stand, so that only
        // top-level fences and HTML blocks at the left margin remain, which
        // every Markdown reader agrees on.
        let (nested, rest) = container_prefix(line);
        if nested && fence_run(rest).is_some() {
            return Err(BodyProblem::NestedBlock);
        }
        if let Some(end) = html {
            if line.to_ascii_lowercase().contains(end) {
                html = None;
            }
            paragraph = false;
            after_blank = is_blank(line);
            continue;
        }
        let was_open = fence.is_some();
        let (after, delimiter) = step_fence(fence, line);
        fence = after;
        if was_open || delimiter {
            paragraph = false;
            // A fence at the left margin ends any open list or quote.
            container = false;
            after_blank = false;
            continue;
        }
        if is_blank(line) {
            paragraph = false;
            after_blank = true;
            container = false;
            continue;
        }
        if nested && html_block_end(rest).is_some() {
            return Err(BodyProblem::NestedBlock);
        }
        if after_blank && (line.starts_with('\t') || line.starts_with("    ")) && !container {
            return Err(BodyProblem::IndentedCode);
        }
        if nested && rest.len() != line.trim_start_matches([' ', '\t']).len() {
            // A list or quote marker, not only indentation.
            container = true;
        } else if !nested && (heading_level(line).is_some() || html_block_end(line).is_some()) {
            // A heading or HTML block at the left margin ends any open list
            // or quote.
            container = false;
        }
        after_blank = false;
        if heading_level(line).is_some_and(|l| l <= level)
            || (paragraph && setext_level(line).is_some_and(|l| l <= level))
        {
            return Err(BodyProblem::EndsSection);
        }
        if let Some(end) = html_block_end(line) {
            // The end condition may sit on the opening line itself, after
            // the opener.
            let opener = line.trim_start_matches(' ');
            let after = opener.get(2..).unwrap_or_default().to_ascii_lowercase();
            if !after.contains(end) {
                html = Some(end);
            }
            paragraph = false;
            continue;
        }
        paragraph = may_be_paragraph(line);
    }
    if fence.is_some() {
        return Err(BodyProblem::OpenFence);
    }
    if html.is_some() {
        return Err(BodyProblem::OpenHtmlBlock);
    }
    if container {
        return Err(BodyProblem::OpenContainer);
    }
    Ok(())
}

/// The body as it is written: with a final line break.
#[must_use]
pub fn normalized_body(body: &str) -> String {
    if body.ends_with('\n') {
        body.to_string()
    } else {
        format!("{body}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Skill\n\nintro\n\n<!-- codeflow:section id=loop class=overridable -->\n## The loop\n\nbody one\n\n### Detail\n\nmore\n\n## Next\n\nafter\n";

    #[test]
    fn a_section_runs_to_the_next_heading_of_its_level() {
        let doc = parse(DOC).unwrap();
        let section = doc.section("loop").unwrap();
        assert_eq!(section.level, 2);
        assert_eq!(section.class, Some(UnitClass::Overridable));
        assert_eq!(section.body, "\nbody one\n\n### Detail\n\nmore\n\n");
        assert_eq!(doc.text(), DOC);
    }

    #[test]
    fn a_heading_inside_a_fence_does_not_end_the_section() {
        let text = "<!-- codeflow:section id=a class=fixed -->\n## A\n```\n## not a heading\n```\nkept\n## B\n";
        let doc = parse(text).unwrap();
        assert_eq!(
            doc.section("a").unwrap().body,
            "```\n## not a heading\n```\nkept\n"
        );
        assert_eq!(doc.text(), text);
    }

    #[test]
    fn malformed_duplicate_fenced_and_nested_markers_are_errors() {
        let malformed = "<!-- codeflow:section id=a class=maybe -->\n## A\n";
        assert_eq!(
            parse(malformed),
            Err(SectionError::MalformedMarker { line: 1 })
        );
        let no_heading = "<!-- codeflow:section id=a class=fixed -->\ntext\n";
        assert_eq!(parse(no_heading), Err(SectionError::NoHeading { line: 1 }));
        let duplicate = "<!-- codeflow:section id=a class=fixed -->\n## A\n<!-- codeflow:section id=a class=fixed -->\n## B\n";
        assert_eq!(parse(duplicate), Err(SectionError::DuplicateId { line: 3 }));
        let fenced = "```\n<!-- codeflow:section id=a class=fixed -->\n## A\n```\n";
        assert_eq!(parse(fenced), Err(SectionError::MarkerInFence { line: 2 }));
        let nested = "<!-- codeflow:section id=a class=overridable -->\n## A\ntext\n<!-- codeflow:section id=b class=floor -->\n### B\n";
        assert_eq!(parse(nested), Err(SectionError::Nested { line: 4 }));
    }

    #[test]
    fn replacement_bodies_cannot_move_boundaries() {
        assert_eq!(check_body("fine\n### sub\n", 2), Ok(()));
        assert_eq!(check_body("  \n", 2), Err(BodyProblem::Empty));
        assert_eq!(
            check_body("x\n<!-- codeflow:managed:end -->\n", 2),
            Err(BodyProblem::StructuralMarker)
        );
        assert_eq!(
            check_body("x\n# codeflow:managed:begin\n", 2),
            Err(BodyProblem::StructuralMarker)
        );
        assert_eq!(
            check_body("x\n## Same level\n", 2),
            Err(BodyProblem::EndsSection)
        );
        assert_eq!(check_body("```\nopen\n", 2), Err(BodyProblem::OpenFence));
        assert_eq!(check_body("```\n## fenced\n```\n", 2), Ok(()));
    }
}
