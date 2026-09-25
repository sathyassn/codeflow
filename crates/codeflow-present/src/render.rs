use std::fmt::Write as _;

use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};
use sha2::{Digest, Sha256};

use crate::{
    document::{Block, EvidenceState, PresentationDocument, TreeNode},
    limits,
    state::FeedbackSnapshot,
};

pub struct RenderOptions<'a> {
    pub session_id: &'a str,
    pub revision: u64,
    pub event_sequence: u64,
    pub script_path: Option<&'a str>,
    pub style_path: Option<&'a str>,
    pub prepaint_source: Option<&'a str>,
    pub utility_style: Option<&'a str>,
    pub identity: Option<RenderIdentity<'a>>,
    pub feedback: Option<&'a FeedbackSnapshot>,
    pub read_only_warning: Option<&'a str>,
    pub interactive: bool,
}

#[derive(Clone, Copy)]
pub struct RenderIdentity<'a> {
    pub src: &'a str,
    pub alt: &'a str,
}

#[must_use]
pub fn render_document(document: &PresentationDocument, options: &RenderOptions<'_>) -> String {
    let mut body = String::new();
    for block in &document.blocks {
        render_block(block, options, &mut body);
    }

    let mut html = String::with_capacity(body.len() + 4_096);
    html.push_str("<!doctype html><html");
    if let Some(language) = &document.language {
        html.push_str(" lang=\"");
        escape_attr_to(language, &mut html);
        html.push('"');
    }
    html.push_str("><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"color-scheme\" content=\"light dark\"><title>");
    escape_html_to(&document.title, &mut html);
    html.push_str("</title>");
    if let Some(source) = options.prepaint_source {
        html.push_str("<script>");
        html.push_str(source);
        html.push_str("</script>");
    }
    if let Some(path) = options.style_path {
        html.push_str("<link rel=\"stylesheet\" href=\"");
        escape_attr_to(path, &mut html);
        html.push_str("\">");
    }
    if let Some(style) = options.utility_style {
        html.push_str("<style data-cf-project-utility-tokens");
        html.push('>');
        html.push_str(style);
        html.push_str("</style>");
    }
    html.push_str("</head><body>");
    if let Some(warning) = options.read_only_warning {
        html.push_str("<aside class=\"version-warning\" role=\"status\">");
        escape_html_to(warning, &mut html);
        html.push_str("</aside>");
    }
    if options.interactive {
        html.push_str("<div id=\"cf-present-chrome\" data-session-id=\"");
        escape_attr_to(options.session_id, &mut html);
        html.push_str("\"></div>");
    }
    html.push_str("<main id=\"cf-present-document\" class=\"document\" data-session-id=\"");
    escape_attr_to(options.session_id, &mut html);
    html.push_str("\" data-revision=\"");
    html.push_str(&options.revision.to_string());
    html.push_str("\">");
    html.push_str(&body);
    html.push_str("</main>");
    if options.interactive {
        html.push_str("<template id=\"cf-present-config\">");
        let identity = options.identity.map(|identity| {
            serde_json::json!({
                "src": identity.src,
                "alt": identity.alt
            })
        });
        let config = serde_json::json!({
            "schema_version": 1,
            "session_id": options.session_id,
            "revision": options.revision,
            "event_sequence": options.event_sequence,
            "title": document.title,
            "shortcuts_enabled": true,
            "review_limits": {
                "max_notes": limits::MAX_FEEDBACK_NOTES,
                "max_visible_feedback": limits::MAX_VISIBLE_FEEDBACK,
                "max_text_utf16": limits::MAX_FEEDBACK_TEXT_UTF16,
                "max_selector_utf16": limits::MAX_SELECTOR_EXACT_UTF16,
                "max_payload_bytes": limits::MAX_FEEDBACK_BYTES
            },
            "identity": identity,
            "feedback": options.feedback
        })
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
        html.push_str(&config);
        html.push_str("</template>");
    }
    if let Some(path) = options.script_path {
        html.push_str("<script type=\"module\" src=\"");
        escape_attr_to(path, &mut html);
        html.push_str("\"></script>");
    }
    html.push_str("</body></html>");
    html
}

#[must_use]
pub fn render_unsupported(raw: &str, schema_version: u32) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Unsupported presentation</title></head><body><main><h1>Unsupported presentation version</h1><p>This document uses schema version ");
    html.push_str(&schema_version.to_string());
    html.push_str(". This CodeFlow build supports version 1. The complete source is shown read-only; nothing was partially rendered.</p><pre><code>");
    escape_html_to(raw, &mut html);
    html.push_str("</code></pre></main></body></html>");
    html
}

/// A revision stored with a diagram block, which was removed with Mermaid.
/// It renders read only: the conversion notice, then each diagram's source,
/// escaped, beside its replacement. Nothing is drawn and no script loads.
#[must_use]
pub fn render_retired(document: &serde_json::Value) -> String {
    let title = document
        .get("title")
        .and_then(serde_json::Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("Retired presentation revision");
    let mut html = String::new();
    html.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>");
    escape_html_to(title, &mut html);
    html.push_str("</title></head><body><main data-cf-retired-revision><h1>");
    escape_html_to(title, &mut html);
    html.push_str("</h1><p role=\"note\">This revision holds a diagram block, which was removed with Mermaid, so it is shown read only and nothing else in it is drawn. Convert each diagram below as ");
    escape_html_to(crate::retired::CONVERSION_GUIDE, &mut html);
    html.push_str(
        " shows, then run <code>codeflow present update</code> with the converted document.</p>",
    );
    for block in crate::retired::retired_blocks(document) {
        let kind = block.kind().unwrap_or_default();
        html.push_str("<section><h2>");
        escape_html_to(block.id().unwrap_or(&block.position), &mut html);
        if let Some(title) = block.text("acc_title") {
            html.push_str(": ");
            escape_html_to(title, &mut html);
        }
        html.push_str("</h2><p>Former ");
        escape_html_to(kind, &mut html);
        html.push_str(" diagram; convert it to ");
        escape_html_to(crate::retired::replacement(Some(kind)), &mut html);
        html.push_str(".</p><pre><code>");
        escape_html_to(block.text("source").unwrap_or_default(), &mut html);
        html.push_str("</code></pre></section>");
    }
    html.push_str("</main></body></html>");
    html
}

#[allow(clippy::too_many_lines)]
fn render_block(block: &Block, options: &RenderOptions<'_>, output: &mut String) {
    output.push_str("<section class=\"block block--");
    output.push_str(block_kind(block));
    output.push_str("\" id=\"");
    escape_attr_to(block.id(), output);
    output.push_str("\" data-cf-block-id=\"");
    escape_attr_to(block.id(), output);
    output.push_str("\" data-cf-block-label=\"");
    escape_attr_to(&block.review_label(), output);
    output.push_str("\" data-cf-block-digest=\"");
    output.push_str(&crate::state::block_digest(block));
    output.push_str("\">");
    if options.interactive {
        output.push_str("<button class=\"anchor-button\" type=\"button\" data-anchor-block=\"");
        escape_attr_to(block.id(), output);
        output.push_str("\" aria-label=\"Add feedback for this block\">+</button>");
    }
    if let Block::Disclosure {
        summary, blocks, ..
    } = block
    {
        output.push_str("<details><summary><span data-cf-review-text-root>");
        escape_html_to(summary, output);
        output.push_str("</span></summary>");
        for child in blocks {
            render_block(child, options, output);
        }
        output.push_str("</details></section>");
        return;
    }
    if let Block::Tabs { tabs, .. } = block {
        output
            .push_str("<div class=\"tabs\"><div class=\"tabs__labels\" data-cf-review-text-root>");
        for tab in tabs {
            output.push_str("<span>");
            escape_html_to(&tab.label, output);
            output.push_str("</span>");
        }
        output.push_str("</div>");
        for tab in tabs {
            output.push_str("<details><summary>");
            escape_html_to(&tab.label, output);
            output.push_str("</summary>");
            for child in &tab.blocks {
                render_block(child, options, output);
            }
            output.push_str("</details>");
        }
        output.push_str("</div></section>");
        return;
    }
    output.push_str("<div data-cf-review-text-root data-cf-canonical-text=\"");
    escape_attr_to(&block.canonical_review_text(), output);
    output.push_str("\">");

    match block {
        Block::Narrative { markdown, .. } => render_markdown(markdown, output),
        Block::Bullets { ordered, items, .. } => {
            let tag = if *ordered { "ol" } else { "ul" };
            output.push('<');
            output.push_str(tag);
            output.push('>');
            for item in items {
                output.push_str("<li>");
                render_markdown(item, output);
                output.push_str("</li>");
            }
            output.push_str("</");
            output.push_str(tag);
            output.push('>');
        }
        Block::Callout {
            tone,
            title,
            markdown,
            ..
        } => {
            output.push_str("<aside class=\"callout callout--");
            output.push_str(&format!("{tone:?}").to_lowercase());
            output.push_str("\">");
            if let Some(title) = title {
                output.push_str("<h2>");
                escape_html_to(title, output);
                output.push_str("</h2>");
            }
            render_markdown(markdown, output);
            output.push_str("</aside>");
        }
        Block::Comparison { columns, .. } => {
            output.push_str("<div class=\"comparison\">");
            for column in columns {
                output.push_str("<article><h2>");
                escape_html_to(&column.title, output);
                output.push_str("</h2>");
                render_markdown(&column.markdown, output);
                output.push_str("</article>");
            }
            output.push_str("</div>");
        }
        Block::Decision {
            title,
            status,
            markdown,
            ..
        } => {
            output.push_str("<article class=\"decision\"><header><h2>");
            escape_html_to(title, output);
            output.push_str("</h2><span class=\"decision__status\">");
            escape_html_to(&format!("{status:?}"), output);
            output.push_str("</span></header>");
            render_markdown(markdown, output);
            output.push_str("</article>");
        }
        Block::Table { columns, rows, .. } => {
            output.push_str("<div class=\"cf-local-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Scrollable table\"><table><thead><tr>");
            for column in columns {
                output.push_str("<th scope=\"col\">");
                escape_html_to(column, output);
                output.push_str("</th>");
            }
            output.push_str("</tr></thead><tbody>");
            for row in rows {
                output.push_str("<tr>");
                for cell in row {
                    output.push_str("<td>");
                    render_markdown(cell, output);
                    output.push_str("</td>");
                }
                output.push_str("</tr>");
            }
            output.push_str("</tbody></table></div>");
        }
        Block::Status { items, .. } => {
            output.push_str("<ul class=\"evidence-list\">");
            for item in items {
                output.push_str("<li data-state=\"");
                output.push_str(evidence_state(&item.state));
                output
                    .push_str("\"><span class=\"state-mark\" aria-hidden=\"true\"></span><strong>");
                escape_html_to(&item.label, output);
                output.push_str("</strong>");
                if let Some(detail) = &item.detail {
                    output.push_str("<span>");
                    escape_html_to(detail, output);
                    output.push_str("</span>");
                }
                output.push_str("</li>");
            }
            output.push_str("</ul>");
        }
        Block::Code {
            language,
            code,
            caption,
            ..
        } => {
            if let Some(caption) = caption {
                output.push_str("<p class=\"block-caption\">");
                escape_html_to(caption, output);
                output.push_str("</p>");
            }
            output.push_str("<div class=\"cf-local-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Scrollable code\"><pre><code data-cf-language=\"");
            escape_attr_to(language, output);
            output.push_str("\">");
            escape_html_to(code, output);
            output.push_str("</code></pre></div>");
        }
        Block::Diff { diff, caption, .. } => {
            if let Some(caption) = caption {
                output.push_str("<p class=\"block-caption\">");
                escape_html_to(caption, output);
                output.push_str("</p>");
            }
            output.push_str("<div class=\"cf-local-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Scrollable diff\"><pre class=\"diff\"><code>");
            for line in diff.lines() {
                let (tag, label) = if line.starts_with('+') && !line.starts_with("+++") {
                    ("ins", "Added: ")
                } else if line.starts_with('-') && !line.starts_with("---") {
                    ("del", "Removed: ")
                } else {
                    ("span", "")
                };
                output.push('<');
                output.push_str(tag);
                output.push('>');
                if !label.is_empty() {
                    output.push_str("<span class=\"sr-only\">");
                    output.push_str(label);
                    output.push_str("</span>");
                }
                escape_html_to(line, output);
                output.push_str("</");
                output.push_str(tag);
                output.push_str(">\n");
            }
            output.push_str("</code></pre></div>");
        }
        Block::Tree { label, nodes, .. } => {
            output.push_str("<h2>");
            escape_html_to(label, output);
            output.push_str("</h2><ul class=\"tree\">");
            render_tree(nodes, output);
            output.push_str("</ul>");
        }
        Block::Figure { declaration, .. } => {
            // The grammar module draws the figure on the client. Until
            // then, and without scripts, the placeholder shows the title and
            // the one-sentence caption.
            let title = declaration
                .pointer("/figure/title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let caption = declaration
                .pointer("/figure/caption")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            output.push_str("<div class=\"figure-block\" data-cf-figure-block=\"pending\" data-cf-figure-declaration=\"");
            escape_attr_to(&declaration.to_string(), output);
            output.push_str("\"><div data-cf-figure-output><p class=\"figure-block__title\">");
            escape_html_to(title, output);
            output.push_str("</p><p class=\"figure-block__caption\">");
            escape_html_to(caption, output);
            output.push_str(
                "</p></div><p data-cf-figure-status class=\"sr-only\" role=\"status\"></p></div>",
            );
        }
        Block::Media {
            mime_type,
            data_base64,
            alt,
            caption,
            ..
        } => {
            output.push_str("<figure>");
            let mime = mime_type.as_str();
            if mime.starts_with("image/") {
                output.push_str("<img src=\"data:");
                output.push_str(mime);
                output.push_str(";base64,");
                output.push_str(data_base64);
                output.push_str("\" alt=\"");
                escape_attr_to(alt, output);
                output.push_str("\" loading=\"lazy\">");
            } else if mime.starts_with("video/") {
                output.push_str("<video controls preload=\"metadata\" aria-label=\"");
                escape_attr_to(alt, output);
                output.push_str("\" src=\"data:");
                output.push_str(mime);
                output.push_str(";base64,");
                output.push_str(data_base64);
                output.push_str("\"></video>");
            } else {
                output.push_str("<audio controls preload=\"metadata\" aria-label=\"");
                escape_attr_to(alt, output);
                output.push_str("\" src=\"data:");
                output.push_str(mime);
                output.push_str(";base64,");
                output.push_str(data_base64);
                output.push_str("\"></audio>");
            }
            if let Some(caption) = caption {
                output.push_str("<figcaption>");
                escape_html_to(caption, output);
                output.push_str("</figcaption>");
            }
            output.push_str("</figure>");
        }
        Block::Disclosure { .. } | Block::Tabs { .. } => {
            unreachable!("nested containers return before opening the common review root")
        }
        Block::FeedbackPrompt { prompt, .. } => {
            output.push_str("<p class=\"feedback-prompt\">");
            escape_html_to(prompt, output);
            output.push_str("</p>");
        }
        Block::Html { html, title, .. } => {
            // Interactive present: validated HTML is inlined so Comment can pin
            // nodes/edges like the design-reference board. Export keeps iframe
            // sandboxing for a file that may be opened outside the session CSP.
            if options.interactive {
                let host = format!("cf-html-{}", sandbox_id(options.session_id, block.id()));
                output.push_str("<figure class=\"stage\"><div class=\"cf-stage-host\" id=\"");
                escape_attr_to(&host, output);
                output.push_str("\">");
                match crate::safe_html::scoped_html(html, &host) {
                    Ok(scoped) => output.push_str(&scoped),
                    Err(_) => output.push_str("<p>HTML content failed isolation validation.</p>"),
                }
                output.push_str("</div></figure>");
            } else {
                output.push_str("<figure class=\"stage\"><iframe sandbox title=\"");
                escape_attr_to(title.as_deref().unwrap_or("Sandboxed content"), output);
                output.push_str("\" srcdoc=\"");
                escape_attr_to(html, output);
                output.push_str("\"></iframe></figure>");
            }
        }
    }
    output.push_str("</div></section>");
}

fn block_kind(block: &Block) -> &'static str {
    match block {
        Block::Narrative { .. } => "narrative",
        Block::Bullets { .. } => "bullets",
        Block::Callout { .. } => "callout",
        Block::Comparison { .. } => "comparison",
        Block::Decision { .. } => "decision",
        Block::Table { .. } => "table",
        Block::Status { .. } => "status",
        Block::Code { .. } => "code",
        Block::Diff { .. } => "diff",
        Block::Tree { .. } => "tree",
        Block::Figure { .. } => "figure",
        Block::Media { .. } => "media",
        Block::Disclosure { .. } => "disclosure",
        Block::Tabs { .. } => "tabs",
        Block::FeedbackPrompt { .. } => "feedback",
        Block::Html { .. } => "html",
    }
}

fn render_markdown(markdown: &str, output: &mut String) {
    let mut link_stack = Vec::new();
    let parser =
        Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH).filter_map(|event| match event {
            Event::Html(_)
            | Event::InlineHtml(_)
            | Event::Start(Tag::Image { .. })
            | Event::End(TagEnd::Image) => None,
            Event::Start(Tag::Link { dest_url, .. }) => {
                let destination = dest_url.as_ref();
                let safe = safe_markdown_destination(destination);
                link_stack.push(safe);
                safe.then(|| {
                    let mut trusted = String::from("<a href=\"");
                    escape_attr_to(destination, &mut trusted);
                    trusted.push('"');
                    if !destination.starts_with('#') {
                        trusted.push_str(" target=\"_blank\" rel=\"noopener noreferrer\"");
                    }
                    trusted.push('>');
                    Event::Html(CowStr::Boxed(trusted.into_boxed_str()))
                })
            }
            Event::End(TagEnd::Link) => link_stack
                .pop()
                .unwrap_or(false)
                .then_some(Event::Html(CowStr::Borrowed("</a>"))),
            event => Some(event),
        });
    html::push_html(output, parser);
}

fn safe_markdown_destination(destination: &str) -> bool {
    let lower = destination.to_ascii_lowercase();
    destination.starts_with('#')
        || lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
}

fn render_tree(nodes: &[TreeNode], output: &mut String) {
    for node in nodes {
        output.push_str("<li><span>");
        escape_html_to(&node.label, output);
        output.push_str("</span>");
        if !node.children.is_empty() {
            output.push_str("<ul>");
            render_tree(&node.children, output);
            output.push_str("</ul>");
        }
        output.push_str("</li>");
    }
}

fn evidence_state(state: &EvidenceState) -> &'static str {
    match state {
        EvidenceState::Pass => "pass",
        EvidenceState::Fail => "fail",
        EvidenceState::Pending => "pending",
        EvidenceState::NotRun => "not-run",
    }
}

#[must_use]
pub fn sandbox_id(session_id: &str, block_id: &str) -> String {
    let digest = Sha256::digest(format!("cf-present-sandbox\0{session_id}\0{block_id}"));
    let mut output = String::with_capacity(32);
    for byte in &digest[..16] {
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

pub fn escape_html_to(value: &str, output: &mut String) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(character),
        }
    }
}

fn escape_attr_to(value: &str, output: &mut String) {
    escape_html_to(value, output);
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::document::{Block, PresentationDocument, Provenance, Tab};
    use scraper::{ElementRef, Html};

    use super::*;

    /// What a retired revision page must show and must never carry.
    pub(crate) fn assert_retired_page(html: &str) {
        assert!(
            html.contains("This revision holds a diagram block, which was removed with Mermaid"),
            "{html}"
        );
        assert!(html.contains(
            crate::retired::CONVERSION_GUIDE
                .replace('"', "&quot;")
                .as_str()
        ));
        for (heading, source, conversion) in [
            (
                "flow: Qualification flow",
                "flowchart LR\n  Input --&gt; Review --&gt; Evidence",
                "Former flowchart diagram; convert it to a flow figure.",
            ),
            (
                "handshake: Open handshake",
                "sequenceDiagram\n  Agent-&gt;&gt;Service: open\n  Service--&gt;&gt;Agent: ready",
                "Former sequence diagram; convert it to a sequence figure.",
            ),
        ] {
            assert!(html.contains(&format!("<h2>{heading}</h2>")), "{html}");
            assert!(
                html.contains(&format!("<pre><code>{source}</code></pre>")),
                "{html}"
            );
            assert!(html.contains(conversion), "{html}");
        }
        for absent in [
            "data-cf-diagram",
            "pending",
            "<script",
            "<template",
            "type=\"module\"",
        ] {
            assert!(
                !html.contains(absent),
                "retired page carries {absent}: {html}"
            );
        }
    }

    #[test]
    fn a_retired_revision_renders_its_notice_and_escaped_sources_and_draws_nothing() {
        let stored: serde_json::Value =
            serde_json::from_str(crate::state::retired_fixture::REVISION).unwrap();
        let html = render_retired(&stored["content"]["document"]);
        assert_retired_page(&html);
        assert!(html.contains("<title>Qualification review</title>"));

        let hostile = serde_json::json!({"title": "<b>x</b>", "blocks": [{
            "type": "diagram", "id": "d", "kind": "flowchart",
            "source": "</code><script>alert(1)</script>", "acc_title": "<i>t</i>", "acc_description": "d"
        }]});
        let html = render_retired(&hostile);
        assert!(!html.contains("<script>") && !html.contains("<b>") && !html.contains("<i>"));
        assert!(html.contains("&lt;/code&gt;&lt;script&gt;alert(1)&lt;/script&gt;"));
    }

    #[test]
    fn markdown_drops_raw_html() {
        let document = PresentationDocument {
            schema_version: 1,
            title: "Safe".to_string(),
            language: None,
            provenance: Provenance::default(),
            blocks: vec![Block::Narrative {
                id: "intro".to_string(),
                markdown: "Hello <script>alert(1)</script> world".to_string(),
            }],
        };
        let rendered = render_document(
            &document,
            &RenderOptions {
                session_id: "00000000-0000-0000-0000-000000000000",
                revision: 1,
                event_sequence: 0,
                script_path: None,
                style_path: None,
                prepaint_source: None,
                utility_style: None,
                identity: None,
                feedback: None,
                read_only_warning: None,
                interactive: true,
            },
        );
        assert!(!rendered.contains("<script>alert"));
        assert!(rendered.contains("Hello alert(1) world"));
    }

    #[test]
    fn markdown_links_and_images_cannot_navigate_to_unsafe_resources() {
        let mut rendered = String::new();
        render_markdown(
            "[safe](https://example.com) [bad](JaVaScRiPt:alert(1)) ![remote](https://example.com/a.png)",
            &mut rendered,
        );
        assert!(rendered.contains(
            "href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\""
        ));
        assert!(!rendered.to_ascii_lowercase().contains("javascript:"));
        assert!(!rendered.contains("<img"));
        assert!(!rendered.contains("a.png"));
        assert!(rendered.contains("bad"));
        assert!(rendered.contains("remote"));
    }

    #[test]
    fn attributes_escape_quotes_and_markup() {
        let mut output = String::new();
        escape_attr_to("\"<&", &mut output);
        assert_eq!(output, "&quot;&lt;&amp;");
    }

    #[test]
    fn nested_review_roots_are_valid_and_match_their_own_unicode_canonical_text() {
        let blocks = vec![
            Block::Disclosure {
                id: "details".to_string(),
                summary: "Résumé 🧭".to_string(),
                blocks: vec![Block::Narrative {
                    id: "detail-body".to_string(),
                    markdown: "Nested body".to_string(),
                }],
            },
            Block::Figure {
                id: "figure".to_string(),
                declaration: serde_json::json!({
                    "schema_version": 1,
                    "figure": {
                        "id": "review-path",
                        "family": "flow",
                        "binding": "authored",
                        "title": "Résumé <path>",
                        "caption": "A change passes review before it lands."
                    }
                }),
            },
            Block::Tabs {
                id: "tabs".to_string(),
                tabs: vec![
                    Tab {
                        label: "Café".to_string(),
                        blocks: vec![Block::Narrative {
                            id: "cafe-body".to_string(),
                            markdown: "One".to_string(),
                        }],
                    },
                    Tab {
                        label: "東京".to_string(),
                        blocks: vec![Block::Narrative {
                            id: "tokyo-body".to_string(),
                            markdown: "Two".to_string(),
                        }],
                    },
                ],
            },
        ];
        let document = PresentationDocument {
            schema_version: 1,
            title: "Nested".to_string(),
            language: Some("en-CA".to_string()),
            provenance: Provenance::default(),
            blocks: blocks.clone(),
        };
        let rendered = render_document(
            &document,
            &RenderOptions {
                session_id: "00000000-0000-0000-0000-000000000000",
                revision: 1,
                event_sequence: 0,
                script_path: None,
                style_path: None,
                prepaint_source: None,
                utility_style: None,
                identity: None,
                feedback: None,
                read_only_warning: None,
                interactive: true,
            },
        );
        let parsed = Html::parse_document(&rendered);
        for block in blocks {
            let section = parsed
                .tree
                .nodes()
                .filter_map(ElementRef::wrap)
                .find(|element| element.value().attr("data-cf-block-id") == Some(block.id()))
                .unwrap();
            let roots = section
                .descendants()
                .filter_map(ElementRef::wrap)
                .filter(|element| {
                    element.value().attr("data-cf-review-text-root").is_some()
                        && element
                            .ancestors()
                            .filter_map(ElementRef::wrap)
                            .find_map(|ancestor| ancestor.value().attr("data-cf-block-id"))
                            == Some(block.id())
                })
                .collect::<Vec<_>>();
            assert_eq!(roots.len(), 1);
            assert_eq!(
                roots[0].text().collect::<String>(),
                block.canonical_review_text()
            );
        }
    }

    #[test]
    fn figure_placeholder_carries_its_declaration_for_the_client() {
        let declaration = serde_json::json!({
            "schema_version": 1,
            "figure": {
                "id": "review-path",
                "family": "flow",
                "binding": "authored",
                "title": "Quotes \" and <tags> stay text",
                "caption": "A change passes review before it lands."
            }
        });
        let document = PresentationDocument {
            schema_version: 1,
            title: "Figure".to_string(),
            language: None,
            provenance: Provenance::default(),
            blocks: vec![Block::Figure {
                id: "figure".to_string(),
                declaration: declaration.clone(),
            }],
        };
        let rendered = render_document(
            &document,
            &RenderOptions {
                session_id: "00000000-0000-0000-0000-000000000000",
                revision: 1,
                event_sequence: 0,
                script_path: None,
                style_path: None,
                prepaint_source: None,
                utility_style: None,
                identity: None,
                feedback: None,
                read_only_warning: None,
                interactive: false,
            },
        );
        let parsed = Html::parse_document(&rendered);
        // The client draws from the declaration attribute, which parses back
        // to the declaration the document carried.
        let placeholder = parsed
            .tree
            .nodes()
            .filter_map(ElementRef::wrap)
            .find(|element| element.value().attr("data-cf-figure-block") == Some("pending"))
            .expect("figure placeholder");
        assert!(placeholder
            .descendants()
            .filter_map(ElementRef::wrap)
            .any(|element| element.value().attr("data-cf-figure-status").is_some()));
        let carried = placeholder
            .value()
            .attr("data-cf-figure-declaration")
            .expect("declaration attribute");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(carried).unwrap(),
            declaration
        );
        assert!(!rendered.contains("<tags>"));
    }
}
