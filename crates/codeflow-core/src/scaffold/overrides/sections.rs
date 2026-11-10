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
            | Self::Nested { line } => *line,
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

/// Whether `line` opens or closes a fenced code block.
#[must_use]
pub fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start_matches(' ');
    line.len() - trimmed.len() <= 3 && (trimmed.starts_with("```") || trimmed.starts_with("~~~"))
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
    let mut in_fence = false;
    let mut index = 0;
    // The level of the declared section that just ended at a marker, to
    // refuse a deeper marker that would sit inside it.
    let mut ended_at_marker: Option<usize> = None;
    while index < lines.len() {
        let line = lines[index];
        if is_marker_line(line) {
            if in_fence {
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
            let mut fence = false;
            ended_at_marker = None;
            while index < lines.len() {
                let next = lines[index];
                if is_marker_line(next) {
                    if fence {
                        return Err(SectionError::MarkerInFence { line: index + 1 });
                    }
                    ended_at_marker = Some(level);
                    break;
                }
                if is_fence(next) {
                    fence = !fence;
                } else if !fence && heading_level(next).is_some_and(|l| l <= level) {
                    break;
                }
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
        if is_fence(line) {
            in_fence = !in_fence;
        }
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
        }
    }
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
    let mut fence = false;
    for line in body.split_inclusive('\n') {
        if line.to_ascii_lowercase().contains("codeflow:") {
            return Err(BodyProblem::StructuralMarker);
        }
        if is_fence(line) {
            fence = !fence;
        } else if !fence && heading_level(line).is_some_and(|l| l <= level) {
            return Err(BodyProblem::EndsSection);
        }
    }
    if fence {
        return Err(BodyProblem::OpenFence);
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
