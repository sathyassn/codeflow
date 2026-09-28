//! Markdown-aware PR sections and advisory presentation/release checks.

use codeflow_core::hooks::{GitPolicy, PolicyLevel, Violation};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

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
    let mut text = String::new();
    let mut excluded = 0;
    let mut heading_excluded = false;
    for (event, span) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                heading_excluded = atx_heading(body, span.start);
                excluded += usize::from(heading_excluded);
            }
            Event::Start(Tag::CodeBlock(_)) if !include_code => excluded += 1,
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

pub(super) fn presentation(git: &GitPolicy, body: &str, epic_into_main: bool) -> Vec<Violation> {
    if !git.pr_sections.is_active() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut warn = |message: String| {
        out.push(Violation::new(
            "git.pr_sections",
            PolicyLevel::Warn,
            message,
            "keep the body concise and link detailed evidence; retain necessary verification"
                .into(),
        ));
    };
    let outline = outline(body);
    for tag in &outline.unclosed {
        warn(format!(
            "PR body opens an HTML <{tag}> block that never closes; its later headings still count as sections, but close it with </{tag}>"
        ));
    }
    // ADR-0071 rule 7: a Summary is judged by whether it anchors the reader,
    // which review and evaluation grade; no count stands in for that.
    for section in outline.sections {
        if section.matches("Testing")
            && !visible_text(section.content(), false)
                .lines()
                .any(|line| line.trim().to_ascii_lowercase().starts_with("not tested:"))
        {
            warn("PR Testing has no Not tested: line".into());
        }
    }
    let mut fenced = false;
    let mut code_lines = 0;
    for event in Parser::new(body) {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))) => {
                fenced = true;
                code_lines = 0;
            }
            Event::Text(text) if fenced => code_lines += text.lines().count(),
            Event::End(TagEnd::CodeBlock) if fenced => {
                if code_lines > 12 {
                    warn(format!(
                        "PR fenced block has {code_lines} lines; aim for at most 12"
                    ));
                }
                fenced = false;
            }
            _ => {}
        }
    }
    if long_prose_line(body) {
        warn("PR prose line exceeds about 160 characters; wrap or shorten it".into());
    }
    // A portable approximation to wrapped Markdown at 100 columns. Comments
    // consume no rows; source blank lines and Markdown syntax remain conservative.
    let visible = super::strip_html_comments(body, false);
    let rows: usize = visible.lines().map(wrapped_rows).sum();
    let budget = if epic_into_main { 90 } else { 65 };
    if rows > budget {
        warn(format!(
            "PR body is about {rows} rendered rows at 100 columns; aim for {budget}"
        ));
    }
    out
}

/// Source lines of prose, list items and headings; code blocks, HTML blocks
/// and table rows are exempt. Tight list items have no paragraph, so lines are
/// read from the source rather than from paragraph spans.
fn long_prose_line(body: &str) -> bool {
    let exempt: Vec<_> = Parser::new(body)
        .into_offset_iter()
        .filter(|(event, _)| matches!(event, Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock)))
        .map(|(_, span)| span)
        .collect();
    let mut offset = 0;
    body.split_inclusive('\n').any(|line| {
        let range = offset..offset + line.len();
        offset = range.end;
        line.trim_end().chars().count() > 160
            && !line.trim_start().starts_with('|')
            && !exempt
                .iter()
                .any(|span| span.start < range.end && range.start < span.end)
    })
}

fn wrapped_rows(line: &str) -> usize {
    let mut rows = 1;
    let mut width = 0;
    for word in line.split_whitespace() {
        let length = word.chars().count();
        if width > 0 && width + 1 + length > 100 {
            rows += 1;
            width = 0;
        }
        if width > 0 {
            width += 1;
        }
        width += length;
        rows += width.saturating_sub(1) / 100;
        width = width.saturating_sub(1) % 100 + 1;
    }
    rows
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
            "declare Impact, Breaking, Rationale and Migration under Release impact using the project's breaking level".into(),
        ));
    };
    let parsed = sections(body);
    let matched = matching_sections(&parsed, "Release impact");
    let [section] = matched.as_slice() else {
        issue("PR body needs exactly one Release impact section".into());
        return out;
    };
    // Do not consume sibling/subsection migration fields as release fields.
    let content = section.content();
    let end = sections(content)
        .first()
        .map_or(content.len(), |s| s.heading_start);
    let text = visible_text(&content[..end], false);
    let mut fields = std::collections::BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.trim().split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            if ["impact", "breaking", "rationale", "migration"].contains(&key.as_str())
                && fields.insert(key.clone(), value.trim()).is_some()
            {
                issue(format!("PR Release impact has duplicate {key} fields"));
            }
        }
    }
    for key in ["impact", "breaking", "rationale", "migration"] {
        if fields.get(key).is_none_or(|value| value.is_empty()) {
            issue(format!(
                "PR Release impact requires a non-empty {key} field"
            ));
        }
    }
    // The same field rules as `scripts/release.py`; both parsers pass
    // `scripts/fixtures/release_impact_cases.json`.
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
    let breaking = fields
        .get("breaking")
        .copied()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let levels = ["none", "patch", "minor", "major"];
    if !levels.contains(&impact.as_str()) {
        issue("PR Impact must be none, patch, minor or major".into());
    }
    if !["yes", "no"].contains(&breaking.as_str()) {
        issue("PR Breaking must be yes or no".into());
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
        for reason in ["Not tested:", "13 lines", "rendered rows", "160 characters"] {
            assert!(
                findings.iter().any(|v| v.message.contains(reason)),
                "missing {reason}: {findings:?}"
            );
        }
        // ADR-0071 rule 7: a key file name or code span may anchor the
        // Summary, and its length is judgment, so none draws a warning.
        for retired in ["code span", "contains a path", "sentences"] {
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
    fn presentation_accepts_short_evidence_and_uses_epic_row_budget() {
        let body = "## Summary\nImproves the installer.\n## Testing\nNot tested: Windows.\n```\n12 passed\n```";
        assert!(presentation(&GitPolicy::default(), body, false).is_empty());
        let body = format!("{body}\n{}", "evidence\n".repeat(66));
        assert!(presentation(&GitPolicy::default(), &body, false)
            .iter()
            .any(|v| v.message.contains("aim for 65")));
        assert!(presentation(&GitPolicy::default(), &body, true).is_empty());
        assert_eq!(wrapped_rows(&"word ".repeat(40)), 2);
        assert_eq!(wrapped_rows(&"x".repeat(201)), 3);
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
    fn long_line_check_reads_tight_items_but_not_code_html_or_tables() {
        let long = "word ".repeat(40);
        for flagged in [
            format!("- {long}"),
            format!("{long}\n"),
            format!("1. {long}"),
            format!("> {long}"),
        ] {
            assert!(long_prose_line(&flagged), "{flagged}");
        }
        for exempt in [
            format!("```\n{long}\n```"),
            format!("    {long}"),
            format!("| {long} |"),
            format!("<!-- {long} -->"),
            format!("- item\n\n  ```\n  {long}\n  ```"),
        ] {
            assert!(!long_prose_line(&exempt), "{exempt}");
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

    /// TSK-106 AC-8: the Release impact parsers share one fixture set;
    /// `scripts/test_release.py` runs the same cases through `release.py`.
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
