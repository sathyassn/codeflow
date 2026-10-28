//! Body sections of a work record: listed criteria, the Blocker, the
//! cancellation lines of the Closeout and the acceptance block (SPC-013
//! R-50, R-54 and Interfaces).
//!
//! Everything here is pure text handling over untrusted input: parsers return
//! typed errors and never panic, and [`render_acceptance`] is the inverse of
//! [`parse_acceptance`] for every block the parser accepts.

use std::fmt;

/// Remove one physical Markdown line ending from a raw split-inclusive line.
/// Callers pass source lines, never a value already parsed from a line.
pub(crate) fn without_line_ending(line: &str) -> &str {
    line.strip_suffix("\r\n")
        .or_else(|| line.strip_suffix('\n'))
        .unwrap_or(line)
}

/// One acceptance criterion of a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Criterion {
    /// `AC-n`; implicit (by position) for a legacy checkbox without an id.
    pub id: String,
    /// The criterion text after the id, whitespace-collapsed.
    pub text: String,
    /// `Some(ticked)` for a legacy checkbox line, `None` for a listed line.
    pub checkbox: Option<bool>,
    /// The indented sub-bullets under the criterion, in order.
    pub sub_items: Vec<SubItem>,
}

/// An indented sub-bullet under a criterion (`  - ...`, `  * ...` or
/// `  + ...`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubItem {
    /// Where the sub-bullet starts in the criterion's `text`, in bytes.
    pub at: usize,
    /// Its first line, whitespace-collapsed, marker included.
    pub line: String,
    /// An amendment note: a dash sub-bullet whose first word is exactly
    /// `Amended` or `Moved` and whose next word is a `YYYY-MM-DD` date.
    /// The note and its wrapped lines are left out of the criterion's own
    /// text, where tags are read.
    pub note: bool,
}

impl SubItem {
    /// Read one indented line as a sub-bullet, `at` its offset in the
    /// criterion text; `None` for a wrapped line of text.
    fn read(at: usize, line: &str) -> Option<Self> {
        let marker = line.chars().next()?;
        let rest = line
            .strip_prefix(['-', '*', '+'])
            .filter(|rest| rest.starts_with([' ', '\t']))?;
        let mut words = rest.split([' ', '\t']).filter(|word| !word.is_empty());
        let first = words.next().unwrap_or_default();
        let date = words
            .next()
            .unwrap_or_default()
            .trim_end_matches([':', ',', ';', '.']);
        Some(Self {
            at,
            line: line.to_string(),
            note: marker == '-' && matches!(first, "Amended" | "Moved") && is_date(date),
        })
    }
}

/// Whether `word` is a `YYYY-MM-DD` date in form.
fn is_date(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// Whether `tag` opens or closes `text` (R-50). Sentence punctuation after
/// a closing tag (`... (journey).`) does not hide it.
fn opens_or_closes(text: &str, tag: &str) -> bool {
    let text = text.trim_matches([' ', '\t']);
    let closing = text.trim_end_matches(['.', ',', ';', ':']);
    text.starts_with(tag) || closing.ends_with(tag)
}

/// The remedy a refusal gives for a tag a sub-bullet hides
/// ([`Criterion::tag_hidden_by`]): it names the sub-bullet (its first line,
/// at most 60 characters), the criterion `name` and the tag `label`.
#[must_use]
pub fn hidden_tag_message(name: &str, label: &str, line: &str) -> String {
    const LIMIT: usize = 60;
    let mut quoted: String = line.chars().take(LIMIT).collect();
    if line.chars().count() > LIMIT {
        quoted.push_str("...");
    }
    format!(
        "the note \"{quoted}\" under {name} hides its `{label}` tag; move the tag to the start of the criterion (R-50)"
    )
}

/// The `(serves ...)` opening a criterion's serves tag.
const SERVES: &str = "(serves ";

impl Criterion {
    /// The parts of `text`: the head, then each sub-bullet with its
    /// wrapped lines, each with the sub-bullet it starts with.
    fn parts(&self) -> impl Iterator<Item = (Option<&SubItem>, &str)> {
        let head_end = self
            .sub_items
            .first()
            .map_or(self.text.len(), |item| item.at);
        let head = std::iter::once((None, &self.text[..head_end]));
        let items = self.sub_items.iter().enumerate().map(|(index, item)| {
            let end = self
                .sub_items
                .get(index + 1)
                .map_or(self.text.len(), |next| next.at);
            (Some(item), &self.text[item.at..end])
        });
        head.chain(items)
    }

    /// The criterion's own text: `text` without its amendment notes and
    /// their wrapped lines, so a note recording a change is never read as
    /// the criterion (R-50, issue #199). Every tag is read here.
    #[must_use]
    pub fn own_text(&self) -> String {
        collapse(
            &self
                .parts()
                .filter(|(item, _)| !item.is_some_and(|item| item.note))
                .map(|(_, part)| part)
                .collect::<Vec<_>>()
                .join(" "),
        )
    }

    /// A journey criterion ends with `(journey)` (R-50); a record may also
    /// open the criterion with it (`AC-7 (journey) On a fresh ...`).
    #[must_use]
    pub fn is_journey(&self) -> bool {
        self.has_tag("(journey)")
    }

    /// Whether `(journey)` sits inside the own text without opening or
    /// closing it, so it is not a tag (R-50); the journey refusal names
    /// such a criterion.
    #[must_use]
    pub fn has_inner_journey(&self) -> bool {
        !self.is_journey() && self.own_text().contains("(journey)")
    }

    /// The first line of the sub-bullet that keeps `tag` (`(journey)`,
    /// `(after release)`, or `(serves ` for the serves tag) from counting,
    /// when one does: a sub-bullet the tag closes the own text before, or
    /// an amendment note that carries the tag. A refusal names it with its
    /// remedy.
    #[must_use]
    pub fn tag_hidden_by(&self, tag: &str) -> Option<&str> {
        let serves = tag == SERVES;
        let counts = if serves {
            self.serves().is_some()
        } else {
            self.has_tag(tag)
        };
        if counts {
            return None;
        }
        let mut before = String::new();
        for (item, part) in self.parts() {
            if let Some(item) = item {
                let hides = if item.note {
                    part.contains(tag)
                } else if serves {
                    serves_tag(&before).is_some()
                } else {
                    opens_or_closes(&before, tag)
                };
                if hides {
                    return Some(item.line.as_str());
                }
            }
            if !item.is_some_and(|item| item.note) {
                before.push(' ');
                before.push_str(part);
            }
        }
        None
    }

    /// A criterion observable only after release carries `(after release)`
    /// (R-62). It is not mandatory at build time: its result is `deferred`
    /// with an owner, a measurement window and a follow-up task.
    #[must_use]
    pub fn is_after_release(&self) -> bool {
        self.has_tag("(after release)")
    }

    /// A tag opens or closes the criterion's own text (R-50). Sentence
    /// punctuation after a closing tag (`... (journey).`) does not hide
    /// it; a tag inside the text, or only in an amendment note, is not a
    /// tag.
    fn has_tag(&self, tag: &str) -> bool {
        opens_or_closes(&self.own_text(), tag)
    }

    /// What a text correction of a completed criterion keeps (ADR-0080):
    /// every tag the parser reads and the listed or checkbox form.
    #[must_use]
    pub fn tags(&self) -> CriterionTags {
        CriterionTags {
            journey: self.is_journey(),
            after_release: self.is_after_release(),
            serves: self.serves(),
            checkbox: self.checkbox.is_some(),
        }
    }

    /// The epic criterion this one serves, from a `(serves EPC-NNN AC-m)`
    /// tag that opens or closes its own text (R-50), like the other tags.
    #[must_use]
    pub fn serves(&self) -> Option<(String, String)> {
        serves_tag(&self.own_text())
    }
}

/// The epic criterion a `(serves EPC-NNN AC-m)` tag that opens or closes
/// `text` names. Sentence punctuation after a closing tag does not hide it;
/// a tag inside the text is not a tag.
fn serves_tag(text: &str) -> Option<(String, String)> {
    let text = text.trim_matches([' ', '\t']);
    let closing = text.trim_end_matches(['.', ',', ';', ':']);
    let inner = if let Some(rest) = text.strip_prefix(SERVES) {
        &rest[..rest.find(')')?]
    } else {
        let rest = &closing[closing.rfind(SERVES)? + SERVES.len()..];
        rest.strip_suffix(')')
            .filter(|inner| !inner.contains(')'))?
    };
    let mut parts = inner.split([' ', '\t']).filter(|part| !part.is_empty());
    let epic = parts.next()?;
    let criterion = parts.next()?;
    (parts.next().is_none() && is_criterion_id(criterion))
        .then(|| (epic.to_string(), criterion.to_string()))
}

/// The tags of one criterion ([`Criterion::tags`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionTags {
    /// `(journey)` opens or closes the criterion.
    pub journey: bool,
    /// `(after release)` opens or closes the criterion.
    pub after_release: bool,
    /// The epic criterion named by `(serves EPC-NNN AC-m)`.
    pub serves: Option<(String, String)>,
    /// The legacy checkbox form, ticked or not.
    pub checkbox: bool,
}

/// The parsed `## Acceptance Criteria` section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CriteriaList {
    pub items: Vec<Criterion>,
    /// Structural problems: duplicate ids, list items that are not criteria.
    pub errors: Vec<String>,
}

impl CriteriaList {
    /// Whether any criterion uses the legacy checkbox form.
    #[must_use]
    pub fn uses_checkboxes(&self) -> bool {
        self.items.iter().any(|item| item.checkbox.is_some())
    }

    /// Ids, text and the tags the parser reads, without the checkbox mark:
    /// ticking a legacy box is not a change of criteria, and a reflow that
    /// keeps the text but moves a tag past an amendment note is one.
    #[must_use]
    pub fn signature(&self) -> Vec<CriterionKey> {
        self.items.iter().map(CriterionKey::of).collect()
    }
}

/// One criterion as criteria equality reads it ([`CriteriaList::signature`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CriterionKey {
    /// `AC-n`.
    pub id: String,
    /// The whitespace-collapsed text, amendment notes included.
    pub text: String,
    /// `(journey)` counts.
    pub journey: bool,
    /// `(after release)` counts.
    pub after_release: bool,
    /// The epic criterion it serves.
    pub serves: Option<(String, String)>,
}

impl CriterionKey {
    /// The key of one criterion.
    #[must_use]
    pub fn of(item: &Criterion) -> Self {
        Self {
            id: item.id.clone(),
            text: item.text.clone(),
            journey: item.is_journey(),
            after_release: item.is_after_release(),
            serves: item.serves(),
        }
    }

    /// The tags this key reads, as a criteria delta prints them.
    #[must_use]
    pub fn tag_list(&self) -> String {
        let mut tags = Vec::new();
        if self.journey {
            tags.push("(journey)".to_string());
        }
        if self.after_release {
            tags.push("(after release)".to_string());
        }
        if let Some((epic, criterion)) = &self.serves {
            tags.push(format!("(serves {epic} {criterion})"));
        }
        if tags.is_empty() {
            "no tag".to_string()
        } else {
            tags.join(" ")
        }
    }
}

/// Whether `value` is a criterion id `AC-n` with a positive decimal `n`.
#[must_use]
pub fn is_criterion_id(value: &str) -> bool {
    value.strip_prefix("AC-").is_some_and(|digits| {
        !digits.is_empty()
            && digits.len() <= 6
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && !digits.starts_with('0')
    })
}

/// How one line of a record reads once Markdown context is applied: a line
/// inside raw HTML or a code block is never a heading, criterion, blocker
/// field, cancellation line or acceptance fence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineKind {
    /// Visible text outside any code block, with inline HTML removed.
    Text,
    /// Inside a raw HTML block, such as a comment.
    Hidden,
    /// Opens a fenced block.
    FenceOpen,
    /// Inside a fenced block.
    FenceContent,
    /// Closes a fenced block.
    FenceClose,
}

/// One scanned line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScannedLine {
    pub kind: LineKind,
    /// The visible text: free of inline HTML for `Text`, the raw line inside a
    /// code block, empty for `Hidden`.
    pub visible: String,
    /// The fence character (`` ` `` or `~`) of a `FenceOpen` line.
    pub marker: u8,
    /// The info string of a `FenceOpen` line.
    pub info: String,
    /// The text of a level-two ATX heading the parser found on this line
    /// (`## <text>`); `None` on every other line, including a line whose
    /// `## ` is only text inside a paragraph or inline HTML.
    pub heading: Option<String>,
}

impl ScannedLine {
    fn new(kind: LineKind, visible: &str) -> Self {
        Self {
            kind,
            visible: visible.to_string(),
            marker: 0,
            info: String::new(),
            heading: None,
        }
    }
}

/// A `CommonMark` fence line: up to three spaces, then three or more backticks
/// or tildes, then the info string (no backtick in a backtick fence's info).
fn fence_marker(line: &str) -> Option<(u8, usize, &str)> {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let marker = *rest.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let length = rest.bytes().take_while(|byte| *byte == marker).count();
    let info = rest[length..].trim_matches([' ', '\t']);
    (length >= 3 && !(marker == b'`' && info.contains('`'))).then_some((marker, length, info))
}

/// Scan lines in Markdown context, as `CommonMark` reads them (the
/// `pulldown-cmark` parser, with source offsets). A line inside a raw HTML
/// block (a comment, `<pre>`, `<script>` and the like) is hidden; a line of
/// an indented code block or of a fence nested in a list or quote is fence
/// content; only a fence at the top level opens a fenced block; inline HTML
/// (an inline comment, a tag) is removed from visible text, and a code span
/// stays text, whatever it contains.
pub(crate) fn scan(lines: &[&str]) -> Vec<ScannedLine> {
    use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

    if lines.is_empty() {
        return Vec::new();
    }
    let text = lines.join("\n");
    let mut starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in lines {
        starts.push(offset);
        offset += line.len() + 1;
    }
    let line_of = |position: usize| starts.partition_point(|start| *start <= position) - 1;
    let last_line = |range: &std::ops::Range<usize>| line_of(range.end.max(range.start + 1) - 1);

    let mut kinds = vec![LineKind::Text; lines.len()];
    let mut opens: Vec<(usize, u8, String)> = Vec::new();
    let mut hidden_bytes = vec![false; text.len()];
    let mut containers = 0usize;
    let mut headings: Vec<(usize, String)> = Vec::new();
    let mut open_heading: Option<(usize, String)> = None;
    for (event, range) in Parser::new_ext(&text, Options::empty()).into_offset_iter() {
        match event {
            Event::Start(Tag::List(_) | Tag::Item | Tag::BlockQuote(_)) => containers += 1,
            Event::Start(Tag::Heading { level, .. }) => {
                let line = line_of(range.start);
                let atx = lines[line].starts_with("## ");
                open_heading = (level == HeadingLevel::H2 && containers == 0 && atx)
                    .then(|| (line, String::new()));
            }
            Event::End(TagEnd::Heading(_)) => headings.extend(open_heading.take()),
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, collected)) = open_heading.as_mut() {
                    collected.push_str(&text);
                }
            }
            Event::End(TagEnd::List(_) | TagEnd::Item | TagEnd::BlockQuote(_)) => {
                containers = containers.saturating_sub(1);
            }
            Event::Start(Tag::HtmlBlock) => {
                for kind in &mut kinds[line_of(range.start)..=last_line(&range)] {
                    *kind = LineKind::Hidden;
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let (first, last) = (line_of(range.start), last_line(&range));
                for line in &mut kinds[first..=last] {
                    *line = LineKind::FenceContent;
                }
                let top_level = containers == 0 && matches!(kind, CodeBlockKind::Fenced(_));
                if let Some((marker, length, info)) =
                    fence_marker(lines[first]).filter(|_| top_level)
                {
                    kinds[first] = LineKind::FenceOpen;
                    opens.push((first, marker, info.to_string()));
                    let closes = last > first
                        && fence_marker(lines[last]).is_some_and(|(found, count, info)| {
                            found == marker && count >= length && info.is_empty()
                        });
                    if closes {
                        kinds[last] = LineKind::FenceClose;
                    }
                }
            }
            Event::InlineHtml(_) => {
                for byte in &mut hidden_bytes[range.start..range.end.min(text.len())] {
                    *byte = true;
                }
            }
            _ => {}
        }
    }
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let kind = kinds[index];
            let visible = match kind {
                LineKind::Hidden => String::new(),
                LineKind::Text => line
                    .char_indices()
                    .filter(|(at, _)| !hidden_bytes[starts[index] + at])
                    .map(|(_, character)| character)
                    .collect(),
                _ => (*line).to_string(),
            };
            let mut scanned = ScannedLine::new(kind, &visible);
            if let Some((_, marker, info)) = opens.iter().find(|(at, _, _)| *at == index) {
                scanned.marker = *marker;
                scanned.info.clone_from(info);
            }
            if let Some((_, text)) = headings.iter().find(|(at, _)| *at == index) {
                scanned.heading = Some(text.trim_matches([' ', '\t']).to_string());
            }
            scanned
        })
        .collect()
}

/// The number of lines the frontmatter takes, fences included (0 without
/// a closed frontmatter).
pub(crate) fn frontmatter_len(lines: &[&str]) -> usize {
    (lines.first().map(|line| line.trim_end_matches([' ', '\t'])) == Some("---"))
        .then(|| {
            lines
                .iter()
                .skip(1)
                .position(|line| line.trim_end_matches([' ', '\t']) == "---")
        })
        .flatten()
        .map_or(0, |index| index + 2)
}

/// [`scan`] for a whole record: the frontmatter is hidden and never parsed
/// as Markdown.
pub(crate) fn scan_record(lines: &[&str]) -> Vec<ScannedLine> {
    let close = frontmatter_len(lines);
    let mut scanned = vec![ScannedLine::new(LineKind::Hidden, ""); close];
    scanned.extend(scan(&lines[close..]));
    scanned
}

/// Line index range `[start, end)` of the level-two section `heading`
/// (`## Closeout`), heading included. Only a heading the Markdown parser
/// reports opens or ends a section, never text that merely reads `## `.
pub(crate) fn section_span(lines: &[ScannedLine], heading: &str) -> Option<(usize, usize)> {
    let name = heading.strip_prefix("## ")?.trim_matches([' ', '\t']);
    let start = lines
        .iter()
        .position(|line| line.heading.as_deref() == Some(name))?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line.heading.is_some())
        .map_or(lines.len(), |(index, _)| index);
    Some((start, end))
}

/// The scanned lines of the section `heading`, excluding the heading line.
fn section(body: &str, heading: &str) -> Option<Vec<ScannedLine>> {
    let lines: Vec<&str> = body.lines().collect();
    let mut scanned = scan(&lines);
    let (start, end) = section_span(&scanned, heading)?;
    scanned.truncate(end);
    Some(scanned.split_off(start + 1))
}

/// The visible text lines of a section, outside fences and comments.
fn section_text(body: &str, heading: &str) -> Option<Vec<String>> {
    section(body, heading).map(|lines| {
        lines
            .into_iter()
            .filter(|line| line.kind == LineKind::Text)
            .map(|line| line.visible)
            .collect()
    })
}

/// Whether the body has the level-two section `heading` as visible text.
#[must_use]
pub fn has_section(body: &str, heading: &str) -> bool {
    section(body, heading).is_some()
}

/// Whether a task record names what it delivers (sathyassn/codeflow#40):
/// its `## Deliverables` section has a substantive entry, or its
/// `## Description` names a path. Both are read as Markdown, and comments
/// never count.
///
/// The warning this feeds is advisory, so the reading errs toward
/// silence: anything that plausibly names a path counts, and only a
/// section with no real entry fails. A token with `/` or `\` counts unless
/// it is a URL, a version or a slash word such as and/or; a file name with
/// an extension counts; a link target counts with its fragment and query
/// removed; and common extensionless files (README, CHANGELOG, LICENSE and
/// the like) count. In Deliverables, a heading is never an entry, and an
/// empty or checkbox-only item, a placeholder such as `TODO`, or the
/// template's `<output>` and `<path>` tokens fill nothing.
#[must_use]
pub fn names_deliverables(body: &str) -> bool {
    section_markdown(body, "## Deliverables")
        .is_some_and(|text| has_entry(&text, Form::Deliverables))
        || section_markdown(body, "## Description").is_some_and(|text| names_path(&text))
}

/// Whether an epic names what it delivers: its `## Deliverables` section
/// has a substantive entry, read as [`names_deliverables`] reads a task's.
/// An epic has no Description, so a path elsewhere never counts.
#[must_use]
pub fn epic_names_deliverables(body: &str) -> bool {
    section_markdown(body, "## Deliverables")
        .is_some_and(|text| has_entry(&text, Form::Deliverables))
}

/// What a record's `## Outcome` section states (TSK-435).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeLine {
    /// A line with a word beyond the template's own tokens and words.
    Stated,
    /// No line, only the template's comment, or a line holding nothing but
    /// the template's tokens and words in any formatting.
    Unfilled,
    /// A template token, `<who>` or `<do what>` in any case or spacing,
    /// outside a code span. Markdown renders it as HTML, so a reader sees
    /// nothing; quoted in a code span it shows.
    RawToken,
}

/// What a record's `## Outcome` section states (TSK-435). Two small rules
/// judge it, never the sentence's grammar: a template token outside a code
/// span is [`OutcomeLine::RawToken`] wherever it stands, and a section
/// whose every line holds nothing but the template's tokens and words
/// (after, this, merges, can), formatting, quotes and punctuation set
/// aside, is [`OutcomeLine::Unfilled`]. Comments never count.
#[must_use]
pub fn outcome_line(body: &str) -> OutcomeLine {
    let Some(text) = section_markdown(body, "## Outcome") else {
        return OutcomeLine::Unfilled;
    };
    let reading = read_entries(&text, Form::Outcome);
    if reading.raw_token {
        OutcomeLine::RawToken
    } else if reading.filled {
        OutcomeLine::Stated
    } else {
        OutcomeLine::Unfilled
    }
}

/// The Markdown of a section as written, with its comment lines blank;
/// inline HTML stays, so a placeholder such as `<path>` is still seen.
fn section_markdown(body: &str, heading: &str) -> Option<String> {
    let lines: Vec<&str> = body.lines().collect();
    let scanned = scan(&lines);
    let (start, end) = section_span(&scanned, heading)?;
    Some(
        (start + 1..end)
            .map(|index| {
                if scanned[index].kind == LineKind::Hidden {
                    ""
                } else {
                    lines[index]
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Words that hold a place rather than name an output.
const PLACEHOLDERS: &[&str] = &[
    "todo",
    "tbd",
    "tba",
    "fixme",
    "xxx",
    "placeholder",
    "output",
    "path",
];

/// Whether a Deliverables section has one substantive entry: a list item,
/// paragraph or fenced block, never a heading, with words other than
/// placeholders. An inline HTML comment, delimited by `<!--` and `-->`
/// and possibly spanning lines, never reaches the entry, so it never
/// changes the result; other inline HTML is dropped too, except the
/// section template's own tokens ([`Form`]), which empty their entry.
fn has_entry(markdown: &str, form: Form) -> bool {
    read_entries(markdown, form).filled
}

/// A section read entry by entry.
struct Reading {
    /// One entry is substantive.
    filled: bool,
    /// An Outcome template token stands outside a code span.
    raw_token: bool,
}

/// Reads a section's entries as [`has_entry`] describes. Only an Outcome
/// section looks for its tokens in text ([`token_spans`]); a Deliverables
/// code span is pushed as written and never scanned.
fn read_entries(markdown: &str, form: Form) -> Reading {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, TextMergeStream};

    let outcome = matches!(form, Form::Outcome);
    let mut entry = String::new();
    let mut reading = Reading {
        filled: false,
        raw_token: false,
    };
    let mut in_heading = false;
    let mut in_comment = false;
    let mut in_code_block = false;
    let close = |entry: &mut String| {
        let substantive = is_entry(entry, form) && !(outcome && only_template(entry));
        entry.clear();
        substantive
    };
    for event in TextMergeStream::new(Parser::new_ext(markdown, Options::empty())) {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                reading.filled |= close(&mut entry);
                in_heading = true;
            }
            Event::End(TagEnd::Heading(_)) => in_heading = false,
            _ if in_heading => {}
            Event::InlineHtml(html) => {
                if in_comment {
                    in_comment = !html.contains("-->");
                } else if let Some(rest) = html.strip_prefix("<!--") {
                    in_comment = !rest.contains("-->");
                } else if form.is_token(&html) {
                    reading.raw_token |= outcome;
                    entry.push_str(&html);
                    entry.push(' ');
                }
            }
            Event::Text(text) => {
                if outcome && !in_code_block && !token_spans(&text).is_empty() {
                    reading.raw_token = true;
                }
                entry.push_str(&text);
                entry.push(' ');
            }
            Event::Code(text) => {
                entry.push_str(&text);
                entry.push(' ');
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                entry.push_str(&dest_url);
                entry.push(' ');
            }
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::Start(Tag::Item)
            | Event::End(TagEnd::Item | TagEnd::Paragraph | TagEnd::CodeBlock) => {
                in_code_block = false;
                reading.filled |= close(&mut entry);
            }
            _ => {}
        }
    }
    reading.filled |= close(&mut entry);
    reading
}

/// The section whose template a reading follows, and so its placeholder
/// tokens. Deliverables keeps its rule that `<output>` or `<path>`
/// anywhere in an entry, quoted or not, leaves it empty. Outcome's `<who>`
/// and `<do what>` are read by [`outcome_line`]'s two rules.
#[derive(Clone, Copy)]
enum Form {
    Deliverables,
    Outcome,
}

impl Form {
    fn tokens(self) -> &'static [&'static str] {
        match self {
            Self::Deliverables => &["<output>", "<path>"],
            Self::Outcome => &["<who>", "<do what>"],
        }
    }

    /// Whether inline HTML is one of this template's placeholder tokens.
    fn is_token(self, html: &str) -> bool {
        let html = match self {
            Self::Deliverables => html.trim_matches([' ', '\t']).to_lowercase(),
            Self::Outcome => spaced(html),
        };
        self.tokens().contains(&html.as_str())
    }
}

/// `text` in lower case with each run of whitespace one space, and none
/// just inside `<` or `>`, so `< do  what >` reads `<do what>`.
fn spaced(text: &str) -> String {
    let mut text = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    for (from, to) in [("< ", "<"), (" >", ">")] {
        while text.contains(from) {
            text = text.replace(from, to);
        }
    }
    text
}

/// The template's own words, in any case.
fn template_word(word: &str) -> bool {
    ["after", "this", "merges", "can"]
        .iter()
        .any(|template| word.eq_ignore_ascii_case(template))
}

/// An Outcome's words, punctuation trimmed.
fn outcome_words(text: &str) -> Vec<&str> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .collect()
}

/// The byte range of each Outcome template token in `text`, in any case or
/// spacing, found in one forward pass. A candidate runs from the latest
/// `<` to the next `>` through letters and whitespace only; any other
/// character, another `<` included, ends it, so no character is read more
/// than twice and a long run of `<` stays linear.
fn token_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut open = None;
    for (at, c) in text.char_indices() {
        match c {
            '<' => open = Some(at),
            '>' => {
                if let Some(start) = open.take() {
                    if Form::Outcome.is_token(&text[start..=at]) {
                        spans.push((start, at + 1));
                    }
                }
            }
            c if c.is_alphabetic() || c.is_whitespace() => {}
            _ => open = None,
        }
    }
    spans
}

/// Whether an Outcome entry holds nothing but the template's tokens and
/// words once its tokens, formatting, quotes and punctuation are set aside.
fn only_template(entry: &str) -> bool {
    let mut words = String::with_capacity(entry.len());
    let mut last = 0;
    for (start, end) in token_spans(entry) {
        words.push_str(&entry[last..start]);
        words.push(' ');
        last = end;
    }
    words.push_str(&entry[last..]);
    outcome_words(&words).iter().all(|word| template_word(word))
}

/// Whether one entry is substantive: a Deliverables entry holding its
/// template's token is not, whatever else it says.
fn is_entry(text: &str, form: Form) -> bool {
    let lower = text.to_lowercase();
    if matches!(form, Form::Deliverables) && form.tokens().iter().any(|token| lower.contains(token))
    {
        return false;
    }
    let visible = text.trim_start_matches([' ', '\t']);
    let visible = ["[ ]", "[x]", "[X]"]
        .iter()
        .find_map(|checkbox| visible.strip_prefix(checkbox))
        .unwrap_or(visible);
    let words: Vec<String> = visible
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect();
    !words.is_empty()
        && !words
            .iter()
            .all(|word| PLACEHOLDERS.contains(&word.as_str()))
}

/// Whether a Description names a path, read from the parsed Markdown: a
/// word of the text, a code span or a link target.
fn names_path(markdown: &str) -> bool {
    use pulldown_cmark::{Event, Options, Parser, Tag, TextMergeStream};

    TextMergeStream::new(Parser::new_ext(markdown, Options::empty())).any(|event| match event {
        Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
            let local = dest_url.split(['#', '?']).next().unwrap_or_default();
            is_path(local)
        }
        Event::Code(text) | Event::Text(text) => text.split_whitespace().any(is_path),
        _ => false,
    })
}

/// Slash words that are prose, never paths.
const NOT_PATHS: &[&str] = &[
    "and/or",
    "either/or",
    "yes/no",
    "true/false",
    "on/off",
    "read/write",
    "input/output",
    "client/server",
    "he/she",
    "s/he",
    "his/her",
    "w/o",
    "n/a",
    "i/o",
    "pass/fail",
];

/// Dotted names that are products, never files.
const NOT_FILES: &[&str] = &[
    "node.js", "next.js", "nuxt.js", "vue.js", "react.js", "three.js", "chart.js", "d3.js",
    "asp.net",
];

/// Top-level domains: `example.com` is a host, not a file.
const DOMAINS: &[&str] = &[
    "com", "org", "net", "io", "dev", "ai", "app", "co", "gov", "edu",
];

/// Files commonly named without an extension.
const BARE_FILES: &[&str] = &[
    "README",
    "CHANGELOG",
    "LICENSE",
    "NOTICE",
    "AUTHORS",
    "CODEOWNERS",
    "Makefile",
    "Dockerfile",
    "Containerfile",
    "Justfile",
    "Procfile",
    "Gemfile",
    "Rakefile",
];

/// Whether one word names a path, by the broad reading
/// [`names_deliverables`] describes.
fn is_path(raw: &str) -> bool {
    let mut word = raw
        .trim_start_matches(['(', '[', '{', '<', '"', '\''])
        .trim_end_matches([')', ']', '}', '>', '"', '\'', ',', ';', ':', '.', '!', '?']);
    // A line reference such as `lib.rs:42` or `lib.rs:42:7` names the file.
    for _ in 0..2 {
        if let Some((head, line)) = word.rsplit_once(':') {
            if !line.is_empty() && line.chars().all(|c| c.is_ascii_digit()) {
                word = head;
            }
        }
    }
    // A drive prefix such as `C:\` or `C:/` starts a Windows path.
    let bytes = word.as_bytes();
    let drive = bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let rest = if drive { &word[2..] } else { word };
    // A URL, a scheme or an anchor is no path.
    if rest.is_empty() || rest.contains(':') || rest.starts_with('#') {
        return false;
    }
    let lower = rest.to_lowercase();
    if NOT_PATHS.contains(&lower.as_str()) || NOT_FILES.contains(&lower.as_str()) {
        return false;
    }
    let unversioned = rest.strip_prefix(['v', 'V']).unwrap_or(rest);
    if unversioned
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '/' | '\\' | '-'))
    {
        return false;
    }
    if BARE_FILES.contains(&rest) {
        return true;
    }
    if rest.contains(['/', '\\']) {
        return rest
            .split(['/', '\\'])
            .any(|segment| segment.chars().any(char::is_alphanumeric));
    }
    let host = rest
        .rsplit_once('.')
        .is_some_and(|(_, tld)| DOMAINS.contains(&tld.to_lowercase().as_str()));
    names_file(rest) && !host
}

/// Whether a word is a file name with an extension: `guide.md`,
/// `a.markdown` or `.gitignore`, never an abbreviation such as e.g, a
/// version such as 2.x, or a number.
fn names_file(segment: &str) -> bool {
    let Some((stem, extension)) = segment.rsplit_once('.') else {
        return false;
    };
    !extension.is_empty()
        && !extension.eq_ignore_ascii_case("x")
        && extension.chars().all(char::is_alphanumeric)
        && extension.chars().any(char::is_alphabetic)
        && (stem.is_empty() || stem.chars().any(char::is_alphanumeric))
        && !segment.split('.').all(|part| part.chars().count() <= 1)
}

fn collapse(text: &str) -> String {
    text.split([' ', '\t', '\r', '\n'])
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse the criteria of `## Acceptance Criteria`: listed `- AC-n ...` lines
/// (R-50) and legacy `- [ ]` / `- [x]` checkboxes, with or without an id.
#[must_use]
pub fn parse_criteria(body: &str) -> CriteriaList {
    let mut list = CriteriaList::default();
    let Some(lines) = section(body, "## Acceptance Criteria") else {
        return list;
    };
    let mut current: Option<Criterion> = None;
    let mut position = 0usize;
    for scanned in lines {
        match scanned.kind {
            LineKind::Hidden => continue,
            LineKind::Text => {}
            LineKind::FenceOpen | LineKind::FenceContent | LineKind::FenceClose => {
                // A fenced block is not criterion text; it ends the item.
                if let Some(done) = current.take() {
                    list.items.push(done);
                }
                continue;
            }
        }
        let line = scanned.visible;
        if line.trim_matches([' ', '\t']).is_empty() {
            continue;
        }
        if let Some(item) = line.strip_prefix("- ") {
            if let Some(done) = current.take() {
                list.items.push(done);
            }
            position += 1;
            match parse_criterion_item(item, position) {
                Ok(Some(criterion)) => current = Some(criterion),
                Ok(None) => {}
                Err(message) => list.errors.push(message),
            }
        } else if line.starts_with([' ', '\t']) {
            if let Some(criterion) = current.as_mut() {
                let addition = collapse(&line);
                if !criterion.text.is_empty() {
                    criterion.text.push(' ');
                }
                let at = criterion.text.len();
                criterion.text.push_str(&addition);
                criterion.sub_items.extend(SubItem::read(at, &addition));
            }
        } else if let Some(done) = current.take() {
            // Prose after a list ends the item; it is not criterion text.
            list.items.push(done);
        }
    }
    if let Some(done) = current {
        list.items.push(done);
    }
    let mut seen = std::collections::BTreeSet::new();
    for item in &list.items {
        if !seen.insert(item.id.clone()) {
            list.errors
                .push(format!("criterion id {} is not unique", item.id));
        }
    }
    list
}

fn parse_criterion_item(item: &str, position: usize) -> Result<Option<Criterion>, String> {
    let (checkbox, rest) = if let Some(rest) = item.strip_prefix("[ ]") {
        (Some(false), rest)
    } else if let Some(rest) = item
        .strip_prefix("[x]")
        .or_else(|| item.strip_prefix("[X]"))
    {
        (Some(true), rest)
    } else {
        (None, item)
    };
    let rest = rest.trim_matches([' ', '\t']);
    let (first, tail) = rest.split_once([' ', '\t']).unwrap_or((rest, ""));
    if is_criterion_id(first) {
        return Ok(Some(Criterion {
            id: first.to_string(),
            text: collapse(tail),
            checkbox,
            sub_items: Vec::new(),
        }));
    }
    match checkbox {
        Some(ticked) if rest.is_empty() => Ok(Some(Criterion {
            id: format!("AC-{position}"),
            text: String::new(),
            checkbox: Some(ticked),
            sub_items: Vec::new(),
        })),
        Some(ticked) => Ok(Some(Criterion {
            id: format!("AC-{position}"),
            text: collapse(rest),
            checkbox: Some(ticked),
            sub_items: Vec::new(),
        })),
        None => Err(format!(
            "list item \"- {}\" is not a criterion; write \"- AC-n <criterion>\"",
            truncate(item, 40)
        )),
    }
}

fn truncate(text: &str, limit: usize) -> String {
    let mut out: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        out.push_str("...");
    }
    out
}

/// The `## Blocker` section required for a blocked task (Interfaces).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blocker {
    pub reason: String,
    pub owner: String,
    pub revisit: String,
}

/// Read `- key: value` bullets of one section into the named fields.
fn keyed_bullets(lines: &[String], keys: &[&str]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in lines {
        let Some(item) = line.trim_start_matches([' ', '\t']).strip_prefix("- ") else {
            continue;
        };
        let Some((key, value)) = item.split_once(':') else {
            continue;
        };
        let key = key.trim_matches([' ', '\t']).to_ascii_lowercase();
        if keys.contains(&key.as_str()) {
            found.push((key, value.trim_matches([' ', '\t']).to_string()));
        }
    }
    found
}

fn first_value(found: &[(String, String)], key: &str) -> String {
    found
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

/// Parse `## Blocker`; `None` when the section is absent.
#[must_use]
pub fn parse_blocker(body: &str) -> Option<Blocker> {
    let lines = section_text(body, "## Blocker")?;
    let found = keyed_bullets(&lines, &["reason", "owner", "revisit"]);
    Some(Blocker {
        reason: first_value(&found, "reason"),
        owner: first_value(&found, "owner"),
        revisit: first_value(&found, "revisit"),
    })
}

impl Blocker {
    /// Names of the fields that are missing or empty.
    #[must_use]
    pub fn missing(&self) -> Vec<&'static str> {
        [
            ("reason", &self.reason),
            ("owner", &self.owner),
            ("revisit", &self.revisit),
        ]
        .into_iter()
        .filter(|(_, value)| is_blank_value(value))
        .map(|(name, _)| name)
        .collect()
    }
}

fn is_blank_value(value: &str) -> bool {
    let value = value.trim();
    value.is_empty() || (value.starts_with('<') && value.ends_with('>'))
}

/// The cancellation lines of a Closeout: `- cancelled: <reason>` and
/// `- scope: <where the scope went>` (R-30).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cancellation {
    pub reason: String,
    pub scope: String,
}

impl Cancellation {
    /// Names of the fields that are missing or empty.
    #[must_use]
    pub fn missing(&self) -> Vec<&'static str> {
        [("cancelled", &self.reason), ("scope", &self.scope)]
            .into_iter()
            .filter(|(_, value)| is_blank_value(value))
            .map(|(name, _)| name)
            .collect()
    }
}

/// Parse the cancellation lines of `## Closeout` (empty when absent).
#[must_use]
pub fn parse_cancellation(body: &str) -> Cancellation {
    let Some(lines) = section_text(body, "## Closeout") else {
        return Cancellation::default();
    };
    let found = keyed_bullets(&lines, &["cancelled", "scope"]);
    Cancellation {
        reason: first_value(&found, "cancelled"),
        scope: first_value(&found, "scope"),
    }
}

/// The phrase R-101 requires where evidence is gone.
pub const HISTORICAL_EVIDENCE_UNAVAILABLE: &str = "historical evidence unavailable";

/// The landing merge named by the historical acceptance form of R-101, for
/// a task completed before the migration baseline: a Closeout item
/// `- acceptance: historical evidence unavailable; ...` (it may wrap) that
/// names the merge that landed the work as ``merge `<sha>` ``. `None` when
/// the Closeout has no such item. The form is structural, as R-60 is: it
/// does not resolve the merge.
#[must_use]
pub fn historical_acceptance(body: &str) -> Option<String> {
    let lines = section_text(body, "## Closeout")?;
    let mut items: Vec<String> = Vec::new();
    for line in &lines {
        let trimmed = line.trim_start_matches([' ', '\t']);
        if let Some(item) = trimmed.strip_prefix("- ") {
            items.push(item.to_string());
        } else if line.starts_with([' ', '\t']) && !trimmed.is_empty() {
            if let Some(last) = items.last_mut() {
                last.push(' ');
                last.push_str(trimmed);
            }
        } else {
            items.push(String::new());
        }
    }
    items.iter().find_map(|item| {
        let (key, value) = item.split_once(':')?;
        if !key
            .trim_matches([' ', '\t'])
            .eq_ignore_ascii_case("acceptance")
            || !value.contains(HISTORICAL_EVIDENCE_UNAVAILABLE)
        {
            return None;
        }
        value.split("merge `").skip(1).find_map(|rest| {
            let sha = rest.split('`').next()?;
            (rest.contains('`')
                && (7..=40).contains(&sha.len())
                && sha.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| sha.to_string())
        })
    })
}

/// The reopen reasons of a Closeout: `- reopened: <reason>` lines, written
/// when a task completed before the migration (no acceptance block) reopens.
#[must_use]
pub fn reopen_reasons(body: &str) -> Vec<String> {
    section_text(body, "## Closeout")
        .map(|lines| keyed_bullets(&lines, &["reopened"]))
        .unwrap_or_default()
        .into_iter()
        .map(|(_, value)| value)
        .filter(|value| !is_blank_value(value))
        .collect()
}

/// One criterion result of an acceptance block: `<outcome> | <evidence>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionResult {
    /// `verified`, `waived` or `failed` for a well-formed block.
    pub outcome: String,
    pub evidence: String,
}

/// An acceptance block (Interfaces, R-54). A superseded block carries the
/// reopen `reason`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceBlock {
    pub superseded: bool,
    pub reason: Option<String>,
    pub reviewed: String,
    pub review: String,
    pub criteria: Vec<(String, CriterionResult)>,
    pub journey: String,
    pub not_verified: String,
    pub follow_ups: String,
    pub verdict: String,
}

/// Why an acceptance block did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for AcceptanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "acceptance block line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for AcceptanceError {}

const BLOCK_KEYS: [&str; 8] = [
    "reason",
    "reviewed",
    "review",
    "criteria",
    "journey",
    "not_verified",
    "follow_ups",
    "verdict",
];

fn block_error(line: usize, message: impl Into<String>) -> AcceptanceError {
    AcceptanceError {
        line,
        message: message.into(),
    }
}

/// One `AC-n: <outcome> | <evidence>` line of the criteria map.
fn parse_result_line(
    number: usize,
    entry: &str,
) -> Result<(String, CriterionResult), AcceptanceError> {
    let (id, value) = entry.split_once(':').ok_or_else(|| {
        block_error(
            number,
            "criterion line must be `AC-n: <outcome> | <evidence>`",
        )
    })?;
    let id = id.trim_matches([' ', '\t']);
    if !is_criterion_id(id) || entry.starts_with(' ') {
        return Err(block_error(
            number,
            format!("`{}` is not a criterion id", truncate(id, 20)),
        ));
    }
    let (outcome, evidence) = value
        .split_once('|')
        .ok_or_else(|| block_error(number, format!("{id} must read `<outcome> | <evidence>`")))?;
    let (outcome, evidence) = (
        outcome.trim_matches([' ', '\t']),
        evidence.trim_matches([' ', '\t']),
    );
    if outcome.is_empty() || evidence.is_empty() {
        return Err(block_error(
            number,
            format!("{id} needs an outcome and evidence"),
        ));
    }
    Ok((
        id.to_string(),
        CriterionResult {
            outcome: outcome.to_string(),
            evidence: evidence.to_string(),
        },
    ))
}

/// One two-space `key: value` line; the key must be a block key.
fn parse_key_line(number: usize, line: &str) -> Result<(&'static str, &str), AcceptanceError> {
    let entry = line
        .strip_prefix("  ")
        .filter(|entry| !entry.starts_with(' '))
        .ok_or_else(|| block_error(number, "block keys are indented by exactly two spaces"))?;
    let (key, value) = entry
        .split_once(':')
        .ok_or_else(|| block_error(number, "expected `key: value`"))?;
    let known = BLOCK_KEYS
        .iter()
        .find(|known| **known == key)
        .ok_or_else(|| block_error(number, format!("unknown key `{}`", truncate(key, 30))))?;
    Ok((known, value.trim_matches([' ', '\t'])))
}

/// Parse the inside of a fenced acceptance block (the lines between the
/// fences). Strict: two-space keys, four-space criterion lines, every key
/// once, no unknown key.
///
/// # Errors
///
/// Returns the first structural problem with its 1-based line number.
pub fn parse_acceptance(text: &str) -> Result<AcceptanceBlock, AcceptanceError> {
    parse_acceptance_lines(text.lines())
}

fn parse_acceptance_lines<'a>(
    lines: impl Iterator<Item = &'a str>,
) -> Result<AcceptanceBlock, AcceptanceError> {
    let mut lines = lines
        .enumerate()
        .map(|(index, line)| (index + 1, line))
        .filter(|(_, line)| !line.trim_matches([' ', '\t']).is_empty());
    let (first_line, header) = lines
        .next()
        .ok_or_else(|| block_error(1, "empty acceptance block"))?;
    let superseded = match header.trim_end_matches([' ', '\t']) {
        "acceptance:" => false,
        "acceptance_superseded:" => true,
        other => {
            return Err(block_error(
                first_line,
                format!(
                    "expected `acceptance:` or `acceptance_superseded:`, found `{}`",
                    truncate(other, 40)
                ),
            ))
        }
    };
    let mut values: Vec<(&str, String)> = Vec::new();
    let mut criteria: Vec<(String, CriterionResult)> = Vec::new();
    let mut in_criteria = false;
    for (number, line) in lines {
        if line.contains('\t') {
            return Err(block_error(number, "tabs are not allowed"));
        }
        if let Some(entry) = line.strip_prefix("    ") {
            if !in_criteria {
                return Err(block_error(number, "unexpected indented line"));
            }
            let (id, result) = parse_result_line(number, entry)?;
            if criteria.iter().any(|(seen, _)| *seen == id) {
                return Err(block_error(number, format!("{id} appears more than once")));
            }
            criteria.push((id, result));
            continue;
        }
        let (key, value) = parse_key_line(number, line)?;
        if values.iter().any(|(seen, _)| *seen == key) {
            return Err(block_error(number, format!("{key} appears more than once")));
        }
        in_criteria = key == "criteria";
        if in_criteria && !value.is_empty() {
            return Err(block_error(
                number,
                "criteria entries go on their own lines",
            ));
        }
        if !in_criteria && value.is_empty() {
            return Err(block_error(number, format!("{key} is empty")));
        }
        values.push((key, value.to_string()));
    }
    let get = |key: &str| -> Result<String, AcceptanceError> {
        values
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| block_error(first_line, format!("missing `{key}`")))
    };
    get("criteria")?;
    if criteria.is_empty() {
        return Err(block_error(first_line, "criteria lists no criterion"));
    }
    let reason = get("reason").ok();
    if superseded != reason.is_some() {
        return Err(block_error(
            first_line,
            "a superseded block, and only a superseded block, records the reopen `reason`",
        ));
    }
    Ok(AcceptanceBlock {
        superseded,
        reason,
        reviewed: get("reviewed")?,
        review: get("review")?,
        criteria,
        journey: get("journey")?,
        not_verified: get("not_verified")?,
        follow_ups: get("follow_ups")?,
        verdict: get("verdict")?,
    })
}

/// Render the inside of a fenced block; the exact inverse of
/// [`parse_acceptance`] for any block it accepts.
#[must_use]
pub fn render_acceptance(block: &AcceptanceBlock) -> String {
    let header = if block.superseded {
        "acceptance_superseded:"
    } else {
        "acceptance:"
    };
    let mut lines = vec![header.to_string()];
    if let Some(reason) = &block.reason {
        lines.push(format!("  reason: {reason}"));
    }
    lines.push(format!("  reviewed: {}", block.reviewed));
    lines.push(format!("  review: {}", block.review));
    lines.push("  criteria:".to_string());
    for (id, result) in &block.criteria {
        lines.push(format!(
            "    {id}: {} | {}",
            result.outcome, result.evidence
        ));
    }
    lines.push(format!("  journey: {}", block.journey));
    lines.push(format!("  not_verified: {}", block.not_verified));
    lines.push(format!("  follow_ups: {}", block.follow_ups));
    lines.push(format!("  verdict: {}", block.verdict));
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// A fenced acceptance block found in `## Closeout`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FencedAcceptance {
    /// The text between the fences.
    pub inner: String,
    pub parsed: Result<AcceptanceBlock, AcceptanceError>,
}

impl FencedAcceptance {
    /// Whether a valid block is superseded. Malformed blocks remain active so
    /// the lifecycle validator refuses their parse error.
    #[must_use]
    pub fn is_superseded(&self) -> bool {
        match &self.parsed {
            Ok(block) => block.superseded,
            Err(_) => false,
        }
    }
}

/// Every closed backtick `yaml` fence of `## Closeout` whose first line opens
/// an acceptance or superseded acceptance block. A fence inside an HTML
/// comment or inside another fence is not a block.
#[must_use]
pub fn acceptance_blocks(body: &str) -> Vec<FencedAcceptance> {
    let Some(lines) = section(body, "## Closeout") else {
        return Vec::new();
    };
    let mut blocks = Vec::new();
    let mut current: Option<Vec<String>> = None;
    for line in lines {
        match line.kind {
            LineKind::FenceOpen => {
                current = (line.marker == b'`' && line.info == "yaml").then(Vec::new);
            }
            LineKind::FenceContent => {
                if let Some(collected) = current.as_mut() {
                    collected.push(line.visible);
                }
            }
            LineKind::FenceClose => {
                let inner = current.take().unwrap_or_default().join("\n");
                let head = inner.trim_start_matches([' ', '\t', '\r', '\n']);
                if head.starts_with("acceptance:") || head.starts_with("acceptance_superseded:") {
                    // section already removed physical CRLF framing; a content CR remains content.
                    let parsed = parse_acceptance_lines(inner.split('\n'));
                    blocks.push(FencedAcceptance { inner, parsed });
                }
            }
            LineKind::Text | LineKind::Hidden => {}
        }
    }
    blocks
}

/// Whether `value` is a full commit id (40 or 64 lowercase hex digits).
#[must_use]
pub fn is_full_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_short_or_full_sha(value: &str) -> bool {
    (7..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The structural acceptance rules of R-60 for one active block against the
/// record's criteria. Binding the block to the reviewed commit and checking a
/// waiver's commit on the target are CI checks outside this function; this
/// proves structure only and does not prove the evidence is honest.
#[must_use]
pub fn check_acceptance(block: &AcceptanceBlock, criteria: &[Criterion]) -> Vec<String> {
    check_block(block, criteria, criteria)
}

/// [`check_acceptance`] for a block that proves only the `required` subset of
/// the record's `known` criteria, as an epic's own block does for the
/// criteria no task serves. Listing another known criterion is allowed.
#[must_use]
pub fn check_block(
    block: &AcceptanceBlock,
    required: &[Criterion],
    known: &[Criterion],
) -> Vec<String> {
    let criteria = required;
    let mut problems = Vec::new();
    if !is_full_sha(&block.reviewed) {
        problems.push("acceptance `reviewed` must be a full commit sha".to_string());
    }
    for criterion in criteria {
        match block.criteria.iter().find(|(id, _)| *id == criterion.id) {
            None => problems.push(format!("acceptance block omits {}", criterion.id)),
            Some((_, result)) if criterion.is_after_release() => {
                problems.extend(after_release_problems(
                    &criterion.id,
                    result,
                    follow_ups_value(&block.follow_ups),
                ));
            }
            Some((_, result)) => match result.outcome.as_str() {
                "verified" => {}
                "waived" => {
                    if !is_short_or_full_sha(&result.evidence) {
                        problems.push(format!(
                            "{} is waived without the planning amendment commit",
                            criterion.id
                        ));
                    }
                }
                "failed" => problems.push(format!("{} failed: {}", criterion.id, result.evidence)),
                other => problems.push(format!(
                    "{} has outcome `{other}`; use verified, waived or failed{}",
                    criterion.id,
                    criterion
                        .tag_hidden_by("(after release)")
                        .map(|line| format!(
                            "; {}",
                            hidden_tag_message(&criterion.id, "(after release)", line)
                        ))
                        .unwrap_or_default()
                )),
            },
        }
    }
    for (id, _) in &block.criteria {
        if !known.iter().any(|criterion| criterion.id == *id) {
            problems.push(format!(
                "acceptance block lists {id}, which the record does not have"
            ));
        }
    }
    // A journey criterion that is also after-release is deferred because it
    // is observable only after release (R-62): the journey has not run.
    if criteria
        .iter()
        .any(|criterion| criterion.is_journey() && !criterion.is_after_release())
        && outcome_word(&block.journey) != "verified"
    {
        problems.push(
            "the record has a journey criterion; `journey` must be `verified | <path exercised>`"
                .to_string(),
        );
    }
    let follow_ups = follow_ups_value(&block.follow_ups);
    let valid_follow_ups = follow_ups.strip_prefix("none:").map_or_else(
        || {
            follow_ups
                .split(',')
                .all(|id| crate::workgraph::is_valid_task_format_id(id.trim_matches([' ', '\t'])))
        },
        |reason| !reason.trim().is_empty(),
    );
    if !valid_follow_ups {
        problems.push(
            "`follow_ups` must list TSK ids or read `none: <reason>`, quoted or not".to_string(),
        );
    }
    if block.verdict != "approved" {
        problems.push(format!(
            "acceptance verdict is `{}`, not approved",
            block.verdict
        ));
    }
    problems
}

/// The non-empty value of `name` in a `deferred` result's evidence, which
/// reads `owner: <who>; window: <when>; follow-up: TSK-NNN`.
fn deferral_field<'r>(result: &'r CriterionResult, name: &str) -> Option<&'r str> {
    result
        .evidence
        .split(';')
        // Spaces and tabs separate the fields (issue 79): another whitespace
        // character is part of the value.
        .filter_map(|part| part.trim_matches([' ', '\t']).strip_prefix(name))
        .map(|value| value.trim_start_matches(':').trim_matches([' ', '\t']))
        .find(|value| !value.trim().is_empty())
}

/// The follow-up task a `deferred` result names (R-62), when it reads
/// `follow-up: <value>`. The value is not checked to be a task id here.
#[must_use]
pub fn deferred_follow_up(result: &CriterionResult) -> Option<&str> {
    deferral_field(result, "follow-up")
}

/// The value of `follow_ups` with one pair of matching YAML quotes removed
/// (issue #69): `none: <reason>` is not a valid YAML plain scalar, so a
/// block that a YAML parser must load writes it quoted, as
/// `"none: <reason>"` or `'none: <reason>'`. Both spellings mean the same,
/// and a record written either way stays valid.
fn follow_ups_value(raw: &str) -> &str {
    let value = raw.trim_matches([' ', '\t']);
    ['"', '\'']
        .into_iter()
        .find_map(|quote| {
            value
                .strip_prefix(quote)
                .and_then(|inner| inner.strip_suffix(quote))
        })
        .map_or(value, |inner| inner.trim_matches([' ', '\t']))
}

/// A criterion observable only after release (R-62) is `deferred` with an
/// owner, a measurement window and a follow-up task listed in `follow_ups`;
/// it is never verified at build time.
fn after_release_problems(id: &str, result: &CriterionResult, follow_ups: &str) -> Vec<String> {
    let shape = "`deferred | owner: <who>; window: <when>; follow-up: TSK-NNN`";
    if result.outcome != "deferred" {
        return vec![format!(
            "{id} is observable only after release and is never `{}` at build time; record {shape}",
            result.outcome
        )];
    }
    let mut problems = Vec::new();
    for name in ["owner", "window"] {
        if deferral_field(result, name).is_none() {
            problems.push(format!(
                "{id} is deferred without its {name}; record {shape}"
            ));
        }
    }
    match deferred_follow_up(result) {
        Some(task) if crate::workgraph::is_valid_task_format_id(task) => {
            if !follow_ups
                .split(',')
                .any(|listed| listed.trim_matches([' ', '\t']) == task)
            {
                problems.push(format!(
                    "{id}'s follow-up {task} is not listed in `follow_ups`"
                ));
            }
        }
        _ => problems.push(format!(
            "{id} is deferred without a follow-up task; record {shape}"
        )),
    }
    problems
}

/// The outcome word before `|` in a `<outcome> | <detail>` value.
#[must_use]
pub fn outcome_word(value: &str) -> &str {
    value
        .split('|')
        .next()
        .unwrap_or_default()
        .trim_matches([' ', '\t'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acceptance_syntax_preserves_unicode_whitespace_in_values() {
        let criteria = parse_criteria(
            "## Acceptance Criteria\n\n- AC-1 A result (serves EPC-001\u{a0}AC-1)\n",
        );
        assert!(criteria.items[0].serves().is_none());
        assert_eq!(
            parse_key_line(1, "  reviewed: abc\u{a0}").unwrap().1,
            "abc\u{a0}"
        );
        assert!(parse_result_line(1, "AC-1\u{a0}: verified | evidence").is_err());
        assert_eq!(outcome_word("approved\u{a0} | evidence"), "approved\u{a0}");
        assert!(fence_marker("```yaml\u{a0}").is_some_and(|(_, _, info)| info == "yaml\u{a0}"));
    }

    const RECORD: &str = "---\nid: TSK-001\n---\n\n## Acceptance Criteria\n\n\
<!-- guidance -->\n\n- AC-1 When a thing happens, the system shall act.\n  More text.\n\
- AC-2 When run on a fresh init, the system shall work (journey)\n\
- AC-3 When served, the system shall serve (serves EPC-001 AC-2)\n\n## Closeout\n\nDone.\n";

    fn block() -> AcceptanceBlock {
        AcceptanceBlock {
            superseded: false,
            reason: None,
            reviewed: "a".repeat(40),
            review: "https://example.test/pr/1#review".into(),
            criteria: vec![
                (
                    "AC-1".into(),
                    CriterionResult {
                        outcome: "verified".into(),
                        evidence: "cargo test lifecycle | 12 passed".into(),
                    },
                ),
                (
                    "AC-2".into(),
                    CriterionResult {
                        outcome: "waived".into(),
                        evidence: "abcdef1".into(),
                    },
                ),
            ],
            journey: "verified | fresh init".into(),
            not_verified: "none".into(),
            follow_ups: "none: nothing left".into(),
            verdict: "approved".into(),
        }
    }

    /// Issue #69: `follow_ups: none: <reason>` is no YAML; the quoted
    /// spelling loads as YAML and both pass, as do quoted task ids.
    #[test]
    fn follow_ups_may_be_quoted_so_the_block_loads_as_yaml() {
        let criteria = parse_criteria(
            "## Acceptance Criteria\n\n- AC-1 When run, the system shall act.\n- AC-2 When rerun, the system shall act again.\n",
        )
        .items;
        for (follow_ups, yaml) in [
            ("none: nothing left", false),
            ("\"none: nothing left\"", true),
            ("'none: nothing left'", true),
            ("\"TSK-001, TSK-002\"", true),
        ] {
            let mut block = block();
            block.follow_ups = follow_ups.into();
            assert!(
                check_acceptance(&block, &criteria).is_empty(),
                "{follow_ups}: {:?}",
                check_acceptance(&block, &criteria)
            );
            let text = render_acceptance(&block);
            assert_eq!(parse_acceptance(&text).as_ref(), Ok(&block), "{follow_ups}");
            assert_eq!(
                serde_yaml::from_str::<serde_yaml::Value>(&text).is_ok(),
                yaml,
                "{follow_ups}: {text}"
            );
        }
        for invalid in ["\"none:\"", "\"later\"", "'none: unclosed"] {
            let mut block = block();
            block.follow_ups = invalid.into();
            assert!(
                !check_acceptance(&block, &criteria).is_empty(),
                "{invalid} passed"
            );
        }
    }

    #[test]
    fn listed_criteria_parse_with_ids_journey_and_serves() {
        let list = parse_criteria(RECORD);
        assert!(list.errors.is_empty(), "{:?}", list.errors);
        assert_eq!(list.items.len(), 3);
        assert_eq!(list.items[0].id, "AC-1");
        assert_eq!(
            list.items[0].text,
            "When a thing happens, the system shall act. More text."
        );
        assert!(list.items[1].is_journey());
        assert_eq!(
            list.items[2].serves(),
            Some(("EPC-001".to_string(), "AC-2".to_string()))
        );
        assert!(!list.uses_checkboxes());
    }

    fn criterion(text: &str) -> Criterion {
        Criterion {
            id: "AC-1".into(),
            text: text.into(),
            checkbox: None,
            sub_items: Vec::new(),
        }
    }

    #[test]
    fn a_tag_reads_with_sentence_punctuation_only_at_either_end() {
        for text in [
            "When run on a fresh init, the system shall work (journey)",
            "When run on a fresh init, the system shall work (journey).",
            "When run on a fresh init, the system shall work (journey),",
            "When run on a fresh init, the system shall work (journey);",
            "When run on a fresh init, the system shall work (journey):",
            "(journey) On a fresh project, the command shall succeed.",
            "(journey): the seed and both runs are recorded.",
        ] {
            assert!(criterion(text).is_journey(), "{text}");
        }
        // TSK-152 AC-4 as the parser collapses it: the tag, then a period.
        let tsk152 = "When a fresh repository is scaffolded with the built binary, a \
minimal-tier `AGENTS.md` names the `.codex/` starter as for interactive Codex and \
`.grok/hooks/` as for Grok Build, and a standard-tier install carries the restored \
present method byte for byte; evidence: the scaffold run's output files compared with \
their sources, or the `init_e2e` case that renders them (journey).";
        assert!(criterion(tsk152).is_journey());
        for text in [
            "When run, the (journey) path shall pass.",
            "When run, the system shall work (journey). Then it shall stop.",
            "When run, the system shall work (journey)!",
            "When run, the system shall work (journeys).",
        ] {
            assert!(!criterion(text).is_journey(), "{text}");
        }
        assert!(
            criterion("Adopters shall report fewer failed installs (after release).")
                .is_after_release()
        );
        assert!(!criterion("The (after release) window shall be a month.").is_after_release());
        assert_eq!(
            criterion("When served, the system shall serve (serves EPC-001 AC-2).").serves(),
            Some(("EPC-001".to_string(), "AC-2".to_string()))
        );
    }

    fn listed(section: &str) -> Vec<Criterion> {
        let list = parse_criteria(&format!("## Acceptance Criteria\n\n{section}"));
        assert!(list.errors.is_empty(), "{:?}", list.errors);
        list.items
    }

    /// Issue #199 (TSK-382 AC-1): an indented `- Amended ...` or
    /// `- Moved ...` note under a criterion is not criterion text, so each
    /// tag closing the head line still counts.
    #[test]
    fn an_amendment_note_keeps_each_closing_tag() {
        let items = listed(
            "- AC-1 On a fresh project, the command shall succeed (journey)\n  \
             - Amended 2026-10-14 (plan version 5): narrowed to Linux.\n\
             - AC-2 The command shall run (serves EPC-001 AC-3)\n  \
             - Moved 2026-10-13 from TSK-009 AC-2.\n\
             - AC-3 Adopters shall report fewer failed installs (after release).\n  \
             - Amended 2026-10-14: the window is a month.\n    \
             It starts at publication.\n  \
             - Moved 2026-10-15 from AC-9.\n\
             - [ ] AC-4 On a fresh project, init shall pass (journey).\n  \
             - Moved 2026-10-16, from AC-8.\n\
             - AC-5 The command shall pass (journey)\n  \
             - Amended 2026-10-18: split the proof.\n  \
             - Print the plan first.\n",
        );
        assert!(items[0].is_journey(), "{:?}", items[0]);
        assert_eq!(
            items[1].serves(),
            Some(("EPC-001".to_string(), "AC-3".to_string()))
        );
        assert!(items[2].is_after_release(), "{:?}", items[2]);
        assert!(items[3].is_journey(), "{:?}", items[3]);
        // A note is left out, and a later sub-bullet that is no note stays
        // criterion text, so here it hides the tag and is named.
        assert_eq!(
            items[4].own_text(),
            "The command shall pass (journey) - Print the plan first."
        );
        assert!(!items[4].is_journey(), "{:?}", items[4]);
        assert_eq!(
            items[4].tag_hidden_by("(journey)"),
            Some("- Print the plan first.")
        );
        // The note stays part of the recorded text, so adding or changing
        // it is still a criteria change.
        assert!(
            items[0].text.ends_with("narrowed to Linux."),
            "{:?}",
            items[0]
        );
        // A tag only inside a note is not the criterion's tag.
        let noted = listed(
            "- AC-1 The command shall run.\n  - Moved 2026-10-13 from (serves EPC-001 AC-2) to none.\n",
        );
        assert_eq!(noted[0].serves(), None);
        assert_eq!(
            noted[0].tag_hidden_by("(serves "),
            Some("- Moved 2026-10-13 from (serves EPC-001 AC-2) to none.")
        );
    }

    /// TSK-382 review round 1: a note is a dash sub-bullet whose first word
    /// is exactly `Amended` or `Moved`, followed by a `YYYY-MM-DD` date.
    /// Any other sub-bullet is criterion text, and a requirement written
    /// after a note still counts.
    #[test]
    fn only_a_dated_dash_note_is_left_out_of_the_criterion() {
        for (line, note) in [
            ("- Amended 2026-10-14: reworded.", true),
            ("- Moved 2026-10-13 from AC-4.", true),
            ("- Amended 2026-10-12 (plan version 4): reworded.", true),
            ("- Moved files shall stay listed in the report.", false),
            ("- **Amended** 2026-10-14: bold word.", false),
            ("- amended 2026-10-14: lower case.", false),
            ("- Amended: 2026-10-14 reworded.", false),
            ("- Amended 2026-10: no day.", false),
            ("* Amended 2026-10-14: a star marker.", false),
            ("+ Moved 2026-10-14: a plus marker.", false),
        ] {
            let items = listed(&format!(
                "- AC-1 On a fresh project, the command shall succeed (journey)\n  {line}\n"
            ));
            assert_eq!(items[0].is_journey(), note, "{line}");
            assert_eq!(items[0].sub_items[0].note, note, "{line}");
            if !note {
                assert_eq!(items[0].tag_hidden_by("(journey)"), Some(line), "{line}");
            }
        }
        // TSK-009 of the review: the closing `(journey)` after the note
        // counts, and the head's `(after release)` no longer closes the
        // criterion, so a requirement written after a note still binds.
        let items = listed(
            "- AC-1 Look fine (after release)\n  - Amended 2026-10-18: split the proof.\n  \
             - The command shall pass the suite before release (journey).\n",
        );
        assert!(items[0].is_journey(), "{:?}", items[0]);
        assert!(!items[0].is_after_release(), "{:?}", items[0]);
        assert_eq!(
            items[0].tag_hidden_by("(after release)"),
            Some("- The command shall pass the suite before release (journey).")
        );
        // A wrapped line of a note is part of the note.
        let items = listed(
            "- AC-1 The command shall pass (journey)\n  - Amended 2026-10-18: one\n    more line of the note.\n",
        );
        assert_eq!(items[0].own_text(), "The command shall pass (journey)");
    }

    /// TSK-382 review round 2: a serves tag counts only where it opens or
    /// closes the criterion's own text, like the other tags.
    #[test]
    fn a_serves_tag_reads_where_it_opens_or_closes() {
        let served = Some(("EPC-001".to_string(), "AC-1".to_string()));
        for (section, counts, hidden) in [
            (
                "- AC-1 The installer shall run (serves EPC-001 AC-1)\n  - Amended 2026-10-14: dated note.\n  - The command shall print the plan.\n",
                false,
                Some("- The command shall print the plan."),
            ),
            ("- AC-1 The installer (serves EPC-001 AC-1) shall run.\n", false, None),
            (
                "- AC-1 The installer shall run (serves EPC-001 AC-1)\n  - Amended 2026-10-14: dated note.\n",
                true,
                None,
            ),
            ("- AC-1 (serves EPC-001 AC-1) The installer shall run.\n", true, None),
            ("- AC-1 The installer shall run (serves EPC-001 AC-1).\n", true, None),
        ] {
            let items = listed(section);
            assert_eq!(items[0].serves().is_some(), counts, "{section}");
            if counts {
                assert_eq!(items[0].serves(), served, "{section}");
            }
            assert_eq!(items[0].tag_hidden_by("(serves "), hidden, "{section}");
        }
    }

    /// TSK-382 review round 1: the signature reads the tags, so a reflow
    /// that keeps the collapsed text but moves a tag past a note is a
    /// criteria change.
    #[test]
    fn a_reflow_that_moves_a_tag_changes_the_signature() {
        for (one_line, split) in [
            (
                "- AC-1 The command shall pass (journey) - Amended 2026-10-14: keep the log.\n",
                "- AC-1 The command shall pass (journey)\n  - Amended 2026-10-14: keep the log.\n",
            ),
            (
                "- AC-1 Prove the metric now (after release) - Amended 2026-10-14: the rest of the sentence.\n",
                "- AC-1 Prove the metric now (after release)\n  - Amended 2026-10-14: the rest of the sentence.\n",
            ),
        ] {
            let (before, after) = (listed(one_line), listed(split));
            assert_eq!(before[0].text, after[0].text);
            assert_ne!(
                parse_criteria(&format!("## Acceptance Criteria\n\n{one_line}")).signature(),
                parse_criteria(&format!("## Acceptance Criteria\n\n{split}")).signature(),
                "{split}"
            );
        }
    }

    /// TSK-382 AC-2: wrapped lines and substantive sub-bullets stay criterion
    /// text, and a tag inside the text still does not count.
    #[test]
    fn a_wrapped_criterion_keeps_its_whole_text() {
        let items = listed(
            "- AC-1 When a fresh repository is scaffolded,\n  the command shall pass\n  \
             and print its plan (journey).\n\
             - AC-2 When run, the (journey) path\n  shall pass on Linux.\n\
             - AC-3 The command shall:\n  - print its plan;\n  - pass (journey).\n\
             - AC-4 When run, the system shall work (journey)\n  and then stop.\n",
        );
        assert_eq!(
            items[0].text,
            "When a fresh repository is scaffolded, the command shall pass and print its plan (journey)."
        );
        assert!(items[0].is_journey());
        assert_eq!(
            items[1].text,
            "When run, the (journey) path shall pass on Linux."
        );
        assert!(!items[1].is_journey() && items[1].has_inner_journey());
        assert_eq!(
            items[2].text,
            "The command shall: - print its plan; - pass (journey)."
        );
        assert!(items[2].is_journey());
        assert!(!items[3].is_journey() && items[3].has_inner_journey());
    }

    #[test]
    fn legacy_checkboxes_read_as_before_with_or_without_ids() {
        let body = "## Acceptance Criteria\n\n- [ ] first\n- [x] AC-7 second\n- [ ]\n";
        let list = parse_criteria(body);
        assert!(list.errors.is_empty(), "{:?}", list.errors);
        assert_eq!(list.items[0].id, "AC-1");
        assert_eq!(list.items[0].checkbox, Some(false));
        assert_eq!(list.items[1].id, "AC-7");
        assert_eq!(list.items[1].checkbox, Some(true));
        assert_eq!(list.items[2].id, "AC-3");
        assert!(list.uses_checkboxes());
        let ticked = parse_criteria(&body.replace("[ ] first", "[x] first"));
        assert_eq!(list.signature(), ticked.signature());
    }

    #[test]
    fn malformed_criteria_are_reported_not_panicked() {
        let body = "## Acceptance Criteria\n- AC-1 one\n- AC-1 two\n- plain bullet\n- AC-01 zero\n";
        let list = parse_criteria(body);
        assert!(list.errors.iter().any(|e| e.contains("AC-1 is not unique")));
        assert!(list.errors.iter().any(|e| e.contains("plain bullet")));
        assert!(list.errors.iter().any(|e| e.contains("AC-01")));
    }

    #[test]
    fn blocker_and_cancellation_report_missing_fields() {
        let body = "## Blocker\n- reason: waiting\n- owner: <person>\n\n## Closeout\n- cancelled: dropped\n";
        let blocker = parse_blocker(body).unwrap();
        assert_eq!(blocker.missing(), vec!["owner", "revisit"]);
        assert_eq!(parse_cancellation(body).missing(), vec!["scope"]);
        assert!(parse_blocker("## Closeout\n").is_none());
    }

    #[test]
    fn acceptance_block_round_trips_and_is_found_in_closeout() {
        let original = block();
        let text = render_acceptance(&original);
        assert_eq!(parse_acceptance(&text), Ok(original.clone()));
        let body = format!("## Closeout\n\nText\n\n```yaml\n{text}```\n\n```yaml\nother: 1\n```\n");
        let found = acceptance_blocks(&body);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].parsed, Ok(original));
    }

    #[test]
    fn superseded_block_needs_reason() {
        let mut superseded = block();
        superseded.superseded = true;
        let without = render_acceptance(&superseded);
        assert!(parse_acceptance(&without)
            .unwrap_err()
            .message
            .contains("reason"));
        superseded.reason = Some("reopened for a regression".into());
        let with = render_acceptance(&superseded);
        assert_eq!(parse_acceptance(&with), Ok(superseded));
    }

    #[test]
    fn malformed_blocks_are_rejected_with_a_line() {
        for (text, needle) in [
            ("", "empty"),
            ("nope:\n", "expected"),
            (
                "acceptance:\n  reviewed: x\n  reviewed: y\n",
                "more than once",
            ),
            ("acceptance:\n  bogus: 1\n", "unknown key"),
            (
                "acceptance:\n    AC-1: verified | x\n",
                "unexpected indented",
            ),
            ("acceptance:\n  criteria:\n    AC-1: verified\n", "outcome"),
            (
                "acceptance:\n  criteria:\n    AC-1: verified | a\n    AC-1: verified | b\n",
                "more than once",
            ),
            ("acceptance:\n\treviewed: x\n", "tabs"),
            ("acceptance:\n  reviewed:\n", "empty"),
        ] {
            let error = parse_acceptance(text).unwrap_err();
            assert!(error.message.contains(needle), "{text:?}: {error}");
        }
    }

    #[test]
    fn structural_acceptance_rules_follow_r60() {
        let criteria = parse_criteria(RECORD).items;
        let mut candidate = block();
        let problems = check_acceptance(&candidate, &criteria);
        assert!(
            problems.iter().any(|p| p.contains("omits AC-3")),
            "{problems:?}"
        );
        candidate.criteria.push((
            "AC-3".into(),
            CriterionResult {
                outcome: "verified".into(),
                evidence: "x".into(),
            },
        ));
        assert!(check_acceptance(&candidate, &criteria).is_empty());
        candidate.verdict = "rejected".into();
        candidate.reviewed = "abc".into();
        candidate.journey = "none | n/a".into();
        candidate.criteria[1].1.evidence = "planning PR".into();
        candidate.criteria.push((
            "AC-9".into(),
            CriterionResult {
                outcome: "verified".into(),
                evidence: "x".into(),
            },
        ));
        candidate.follow_ups = "later".into();
        let problems = check_acceptance(&candidate, &criteria).join("\n");
        for needle in [
            "not approved",
            "full commit sha",
            "journey",
            "waived without",
            "AC-9",
            "follow_ups",
        ] {
            assert!(problems.contains(needle), "{needle}: {problems}");
        }
    }

    #[test]
    fn markdown_context_hides_comments_and_enclosed_fences() {
        let inner = render_acceptance(&block());
        let visible = format!("```yaml\n{inner}```");
        let body = format!(
            "## Closeout\n\n<!--\n{visible}\n-->\n\n````markdown\n{visible}\n## Not a heading\n````\n\n\
~~~\n{visible}\n~~~\n\n```yaml\n{inner}```\n"
        );
        let found = acceptance_blocks(&body);
        assert_eq!(found.len(), 1, "only the visible top-level fence counts");
        assert_eq!(found[0].parsed, Ok(block()));

        // An unclosed fence runs to the end of the document and is no block.
        assert!(acceptance_blocks(&format!("## Closeout\n\n```yaml\n{inner}")).is_empty());
        // A fence closes only on its own character, at its length or longer.
        let lines = ["````yaml", "```", "~~~~", "`````", "after"];
        let kinds: Vec<LineKind> = scan(&lines).into_iter().map(|line| line.kind).collect();
        assert_eq!(
            kinds,
            [
                LineKind::FenceOpen,
                LineKind::FenceContent,
                LineKind::FenceContent,
                LineKind::FenceClose,
                LineKind::Text
            ]
        );
        // Raw HTML hides a fence; a code span holding `<!--` hides nothing.
        let pre = format!("## Closeout\n\n<pre>\n{visible}\n</pre>\n");
        assert!(acceptance_blocks(&pre).is_empty());
        let span = format!("## Closeout\n\nSee `<!--` here.\n\n{visible}\n");
        assert_eq!(acceptance_blocks(&span).len(), 1);
        let listed = format!("## Closeout\n\n- item\n\n  {visible}\n");
        assert!(
            acceptance_blocks(&listed).is_empty(),
            "a nested fence is no block"
        );
        // A commented heading opens no section; an inline comment keeps one.
        assert!(!has_section("<!--\n## Blocker\n-->\n", "## Blocker"));
        assert!(!has_section("<!-- ## Blocker -->\n", "## Blocker"));
        assert!(has_section("## Blocker <!-- note -->\n", "## Blocker"));
        // Inline HTML never manufactures a heading, nor does text in a list.
        assert!(!has_section(
            "<span hidden>## Blocker</span>\n",
            "## Blocker"
        ));
        assert!(!has_section("- ## Blocker\n", "## Blocker"));
        assert!(
            !has_section("Blocker\n--\n", "## Blocker"),
            "ATX headings only"
        );
        let criteria = parse_criteria(
            "## Acceptance Criteria\n\n<!--\n- AC-9 hidden\n-->\n- AC-1 shown\n```text\n- AC-8 example\n```\n",
        );
        assert_eq!(
            criteria
                .signature()
                .into_iter()
                .map(|key| (key.id, key.text))
                .collect::<Vec<_>>(),
            vec![("AC-1".to_string(), "shown".to_string())]
        );
        assert_eq!(
            reopen_reasons(
                "## Closeout\n\n- reopened: one\n<!-- - reopened: two -->\n- reopened: <reason>\n"
            ),
            vec!["one".to_string()]
        );
    }

    /// A small deterministic generator (xorshift) standing in for a
    /// property-testing crate: the parsers must never panic on arbitrary
    /// input, and every rendered valid block must parse back to itself.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
            let index = usize::try_from(self.next() % items.len() as u64).unwrap_or(0);
            items[index]
        }

        fn word(&mut self) -> String {
            let words = [
                "alpha", "b|c", "x: y", "AC-1", "   ", "é", "verified", "`q`", "--", "#",
                "none: r", "0", "\u{2014}",
            ];
            let count = 1 + self.next() % 4;
            (0..count)
                .map(|_| self.pick(&words))
                .collect::<Vec<_>>()
                .join(" ")
        }

        fn value(&mut self) -> String {
            let word = self.word();
            let trimmed = word.trim();
            if trimmed.is_empty() {
                "v".to_string()
            } else {
                trimmed.to_string()
            }
        }
    }

    #[test]
    fn parsers_never_panic_on_generated_input() {
        let fragments = [
            "acceptance:",
            "acceptance_superseded:",
            "  ",
            "    ",
            "\t",
            "AC-",
            "AC-1",
            ":",
            "|",
            "\n",
            "```yaml",
            "```",
            "````",
            "~~~",
            "## Closeout",
            "## Acceptance Criteria",
            "- ",
            "- [ ]",
            "- [x]",
            "<!--",
            "-->",
            "reviewed",
            "criteria",
            "reason",
            "é",
            "\r\n",
            "## Blocker",
            "owner:",
        ];
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..4000 {
            let count = rng.next() % 40;
            let text: String = (0..count).map(|_| rng.pick(&fragments)).collect();
            let _ = parse_acceptance(&text);
            let _ = acceptance_blocks(&text);
            let _ = parse_criteria(&text);
            let _ = parse_blocker(&text);
            let _ = parse_cancellation(&text);
            let _ = reopen_reasons(&text);
        }
    }

    #[test]
    fn generated_valid_blocks_round_trip() {
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        for _ in 0..2000 {
            let superseded = rng.next().is_multiple_of(2);
            let criteria = (1..=1 + rng.next() % 5)
                .map(|n| {
                    (
                        format!("AC-{n}"),
                        CriterionResult {
                            outcome: rng
                                .pick(&["verified", "waived", "failed", "odd"])
                                .to_string(),
                            evidence: rng.value(),
                        },
                    )
                })
                .collect();
            let original = AcceptanceBlock {
                superseded,
                reason: superseded.then(|| rng.value()),
                reviewed: rng.value(),
                review: rng.value(),
                criteria,
                journey: rng.value(),
                not_verified: rng.value(),
                follow_ups: rng.value(),
                verdict: rng.value(),
            };
            let rendered = render_acceptance(&original);
            assert_eq!(parse_acceptance(&rendered), Ok(original), "{rendered}");
        }
    }
    #[test]
    fn r16_line_framing_is_removed_once() {
        assert_eq!(without_line_ending("value\r\n"), "value");
        assert_eq!(without_line_ending("value\n\n"), "value\n");
        assert_eq!(without_line_ending("value\r\r\n"), "value\r");
        let lines: Vec<_> = "---\r\r\nid: TSK-001\r\n---\r\n".lines().collect();
        assert_eq!(
            frontmatter_len(&lines),
            0,
            "a content CR is not another line delimiter"
        );
    }
}
