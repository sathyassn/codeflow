//! Body sections of a work record: listed criteria, the Blocker, the
//! cancellation lines of the Closeout and the acceptance block (SPC-013
//! R-50, R-54 and Interfaces).
//!
//! Everything here is pure text handling over untrusted input: parsers return
//! typed errors and never panic, and [`render_acceptance`] is the inverse of
//! [`parse_acceptance`] for every block the parser accepts.

use std::fmt;

/// One acceptance criterion of a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Criterion {
    /// `AC-n`; implicit (by position) for a legacy checkbox without an id.
    pub id: String,
    /// The criterion text after the id, whitespace-collapsed.
    pub text: String,
    /// `Some(ticked)` for a legacy checkbox line, `None` for a listed line.
    pub checkbox: Option<bool>,
}

impl Criterion {
    /// A journey criterion ends with `(journey)` (R-50); a record may also
    /// open the criterion with it (`AC-7 (journey) On a fresh ...`).
    #[must_use]
    pub fn is_journey(&self) -> bool {
        self.has_tag("(journey)")
    }

    /// A criterion observable only after release carries `(after release)`
    /// (R-62). It is not mandatory at build time: its result is `deferred`
    /// with an owner, a measurement window and a follow-up task.
    #[must_use]
    pub fn is_after_release(&self) -> bool {
        self.has_tag("(after release)")
    }

    fn has_tag(&self, tag: &str) -> bool {
        let text = self.text.trim();
        text.starts_with(tag) || text.ends_with(tag)
    }

    /// The epic criterion this one serves, from `(serves EPC-NNN AC-m)`.
    #[must_use]
    pub fn serves(&self) -> Option<(String, String)> {
        let start = self.text.find("(serves ")?;
        let rest = &self.text[start + "(serves ".len()..];
        let end = rest.find(')')?;
        let mut parts = rest[..end].split_whitespace();
        let epic = parts.next()?;
        let criterion = parts.next()?;
        (parts.next().is_none() && is_criterion_id(criterion))
            .then(|| (epic.to_string(), criterion.to_string()))
    }
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

    /// Ids and text, without the checkbox mark: ticking a legacy box is not a
    /// change of criteria.
    #[must_use]
    pub fn signature(&self) -> Vec<(String, String)> {
        self.items
            .iter()
            .map(|item| (item.id.clone(), item.text.clone()))
            .collect()
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
    let info = rest[length..].trim();
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

    let lines: Vec<&str> = lines
        .iter()
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let text = lines.join("\n");
    let mut starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
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
                scanned.heading = Some(text.trim().to_string());
            }
            scanned
        })
        .collect()
}

/// The number of lines the frontmatter takes, fences included (0 without
/// a closed frontmatter).
pub(crate) fn frontmatter_len(lines: &[&str]) -> usize {
    (lines.first().map(|line| line.trim_end()) == Some("---"))
        .then(|| {
            lines
                .iter()
                .skip(1)
                .position(|line| line.trim_end() == "---")
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
    let name = heading.strip_prefix("## ")?.trim();
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

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
        if line.trim().is_empty() {
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
                criterion.text = collapse(&format!("{} {}", criterion.text, line));
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
    let rest = rest.trim();
    let (first, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    if is_criterion_id(first) {
        return Ok(Some(Criterion {
            id: first.to_string(),
            text: collapse(tail),
            checkbox,
        }));
    }
    match checkbox {
        Some(ticked) if rest.is_empty() => Ok(Some(Criterion {
            id: format!("AC-{position}"),
            text: String::new(),
            checkbox: Some(ticked),
        })),
        Some(ticked) => Ok(Some(Criterion {
            id: format!("AC-{position}"),
            text: collapse(rest),
            checkbox: Some(ticked),
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
        let Some(item) = line.trim_start().strip_prefix("- ") else {
            continue;
        };
        let Some((key, value)) = item.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if keys.contains(&key.as_str()) {
            found.push((key, value.trim().to_string()));
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
    let id = id.trim();
    if !is_criterion_id(id) || entry.starts_with(' ') {
        return Err(block_error(
            number,
            format!("`{}` is not a criterion id", truncate(id, 20)),
        ));
    }
    let (outcome, evidence) = value
        .split_once('|')
        .ok_or_else(|| block_error(number, format!("{id} must read `<outcome> | <evidence>`")))?;
    let (outcome, evidence) = (outcome.trim(), evidence.trim());
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
    Ok((known, value.trim()))
}

/// Parse the inside of a fenced acceptance block (the lines between the
/// fences). Strict: two-space keys, four-space criterion lines, every key
/// once, no unknown key.
///
/// # Errors
///
/// Returns the first structural problem with its 1-based line number.
pub fn parse_acceptance(text: &str) -> Result<AcceptanceBlock, AcceptanceError> {
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line.trim_end_matches('\r')))
        .filter(|(_, line)| !line.trim().is_empty());
    let (first_line, header) = lines
        .next()
        .ok_or_else(|| block_error(1, "empty acceptance block"))?;
    let superseded = match header.trim_end() {
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
    /// Whether the block is (or, when malformed, is headed as) superseded.
    #[must_use]
    pub fn is_superseded(&self) -> bool {
        match &self.parsed {
            Ok(block) => block.superseded,
            Err(_) => self
                .inner
                .trim_start()
                .starts_with("acceptance_superseded:"),
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
                let head = inner.trim_start();
                if head.starts_with("acceptance:") || head.starts_with("acceptance_superseded:") {
                    let parsed = parse_acceptance(&inner);
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
                    &block.follow_ups,
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
                    "{} has outcome `{other}`; use verified, waived or failed",
                    criterion.id
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
    if criteria.iter().any(Criterion::is_journey) && outcome_word(&block.journey) != "verified" {
        problems.push(
            "the record has a journey criterion; `journey` must be `verified | <path exercised>`"
                .to_string(),
        );
    }
    let follow_ups = block.follow_ups.trim();
    let valid_follow_ups = follow_ups.strip_prefix("none:").map_or_else(
        || {
            follow_ups
                .split(',')
                .all(|id| crate::workgraph::is_valid_task_format_id(id.trim()))
        },
        |reason| !reason.trim().is_empty(),
    );
    if !valid_follow_ups {
        problems.push("`follow_ups` must list TSK ids or read `none: <reason>`".to_string());
    }
    if block.verdict != "approved" {
        problems.push(format!(
            "acceptance verdict is `{}`, not approved",
            block.verdict
        ));
    }
    problems
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
    let field = |name: &str| {
        result
            .evidence
            .split(';')
            .filter_map(|part| part.trim().strip_prefix(name))
            .map(|value| value.trim_start_matches(':').trim())
            .find(|value| !value.is_empty())
    };
    let mut problems = Vec::new();
    for name in ["owner", "window"] {
        if field(name).is_none() {
            problems.push(format!(
                "{id} is deferred without its {name}; record {shape}"
            ));
        }
    }
    match field("follow-up") {
        Some(task) if crate::workgraph::is_valid_task_format_id(task) => {
            if !follow_ups.split(',').any(|listed| listed.trim() == task) {
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
    value.split('|').next().unwrap_or_default().trim()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            criteria.signature(),
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
}
