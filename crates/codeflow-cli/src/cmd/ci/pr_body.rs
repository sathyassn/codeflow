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

/// Only document headings count, not examples in lists, quotes, HTML or code.
/// A subsection belongs to its parent until a sibling or ancestor starts.
fn sections(body: &str) -> Vec<Section<'_>> {
    let mut result: Vec<Section<'_>> = Vec::new();
    let mut nesting: usize = 0;
    let mut heading: Option<(HeadingLevel, String, usize)> = None;
    let mut html = HtmlContainers::default();
    for (event, span) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. })
                if nesting == 0 && html.open.is_empty() && atx_heading(body, span.start) =>
            {
                for section in &mut result {
                    if section.end == body.len() && section.depth >= level {
                        section.end = span.start;
                    }
                }
                heading = Some((level, String::new(), span.start));
            }
            Event::End(TagEnd::Heading(_)) if nesting == 0 => {
                if let Some((depth, name, heading_start)) = heading.take() {
                    result.push(Section {
                        name,
                        depth,
                        heading_start,
                        start: span.end,
                        end: body.len(),
                        body,
                    });
                }
            }
            Event::Text(text) | Event::Code(text) if heading.is_some() => {
                heading.as_mut().unwrap().1.push_str(&text);
            }
            Event::Html(value) | Event::InlineHtml(value) => html.observe(&value),
            Event::Start(_) => nesting += 1,
            Event::End(_) => nesting = nesting.saturating_sub(1),
            _ => {}
        }
    }
    result
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
/// declare top-level PR sections. Only parser-emitted HTML is inspected.
#[derive(Default)]
struct HtmlContainers {
    open: Vec<String>,
    pending: String,
}

impl HtmlContainers {
    fn observe(&mut self, html: &str) {
        // The Markdown parser may emit one HTML event per source line, even
        // within a single comment or tag. Keep unfinished tokens across events.
        self.pending.push_str(html);
        let mut rest = self.pending.as_str();
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
            let closing = tag.starts_with('/');
            let name = tag
                .trim_start_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_end_matches('/')
                .to_ascii_lowercase();
            if closing {
                if let Some(index) = self.open.iter().rposition(|open| *open == name) {
                    self.open.truncate(index);
                }
            } else if !tag.trim_end().ends_with('/')
                && name.starts_with(|ch: char| ch.is_ascii_alphabetic())
                && !matches!(
                    name.as_str(),
                    "area"
                        | "base"
                        | "br"
                        | "col"
                        | "embed"
                        | "hr"
                        | "img"
                        | "input"
                        | "link"
                        | "meta"
                        | "param"
                        | "source"
                        | "track"
                        | "wbr"
                )
            {
                self.open.push(name);
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
            Event::SoftBreak | Event::HardBreak | Event::End(_) if excluded == 0 => {
                text.push('\n');
            }
            _ => {}
        }
    }
    text
}

pub(super) fn has_content(body: &str) -> bool {
    !visible_text(body, true).trim().is_empty()
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
    for section in sections(body) {
        if section.matches("Summary") {
            let text = visible_text(section.content(), false);
            // Advisory heuristic: punctuation ending a word, not dots inside paths.
            let sentences = text
                .split_whitespace()
                .filter(|word| {
                    word.trim_end_matches(['\'', '"', ')'])
                        .ends_with(['.', '!', '?'])
                })
                .count();
            if sentences > 3 {
                warn(format!(
                    "PR Summary has about {sentences} sentences; aim for at most three"
                ));
            }
            if Parser::new(section.content()).any(|event| matches!(event, Event::Code(_))) {
                warn(
                    "PR Summary contains a code span; put implementation details in Changes".into(),
                );
            }
            if text.split_whitespace().any(looks_like_path) {
                warn("PR Summary contains a path; put file details in Changes".into());
            }
        }
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
    for (event, span) in Parser::new(body).into_offset_iter() {
        if matches!(event, Event::Start(Tag::Paragraph))
            && body[span]
                .lines()
                .any(|line| !line.trim_start().starts_with('|') && line.chars().count() > 160)
        {
            warn("PR prose line exceeds about 160 characters; wrap or shorten it".into());
        }
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

fn looks_like_path(word: &str) -> bool {
    let word = word.trim_matches(['(', ')', ',', '.', ';', '"', '\'']);
    !word.contains("://")
        && (word.contains('/') || word.contains('\\'))
        && word.chars().any(char::is_alphabetic)
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
    if breaking == "yes" {
        let migration = fields
            .get("migration")
            .copied()
            .unwrap_or_default()
            .trim_matches(['"', '\'']);
        let substantive = if migration.eq_ignore_ascii_case("see Breaking change") {
            let guidance = matching_sections(&parsed, "Breaking change");
            guidance.len() == 1 && substantive(&visible_text(guidance[0].content(), true))
        } else {
            substantive(migration)
        };
        if !substantive {
            issue("PR Breaking: yes requires substantive Migration steps or a populated Breaking change reference".into());
        }
    }
    out
}

fn substantive(value: &str) -> bool {
    let value = value.trim();
    !matches!(
        value.to_ascii_lowercase().as_str(),
        "" | "none" | "n/a" | "na" | "tbd" | "todo" | "steps" | "see breaking change"
    ) && !value.contains(['<', '>', '|'])
        && value.chars().filter(|c| c.is_alphanumeric()).count() >= 8
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
        for reason in [
            "sentences",
            "code span",
            "path",
            "Not tested:",
            "13 lines",
            "rendered rows",
            "160 characters",
        ] {
            assert!(
                findings.iter().any(|v| v.message.contains(reason)),
                "missing {reason}: {findings:?}"
            );
        }
        assert!(findings.iter().all(|v| v.level == PolicyLevel::Warn));
        let git = GitPolicy {
            pr_sections: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        assert!(presentation(&git, &body, false).is_empty());
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
