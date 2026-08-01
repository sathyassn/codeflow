use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};
use sha2::{Digest, Sha256};

use crate::document::{Block, EvidenceState, PresentationDocument, TreeNode};

pub struct RenderOptions<'a> {
    pub session_id: &'a str,
    pub revision: u64,
    pub script_path: Option<&'a str>,
    pub style_path: Option<&'a str>,
    pub prepaint_source: Option<&'a str>,
    pub read_only_warning: Option<&'a str>,
    pub interactive: bool,
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
        html.push_str("<script id=\"cf-present-config\" type=\"application/json\">");
        let config = serde_json::json!({
            "schema_version": 1,
            "session_id": options.session_id,
            "revision": options.revision,
            "title": document.title,
            "shortcuts_enabled": true
        })
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
        html.push_str(&config);
        html.push_str("</script>");
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
    output.push_str("\">");
    if options.interactive {
        output.push_str("<button class=\"anchor-button\" type=\"button\" data-anchor-block=\"");
        escape_attr_to(block.id(), output);
        output.push_str("\" aria-label=\"Add feedback for this block\">+</button>");
    }
    output.push_str("<div data-cf-review-text-root>");

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
            output.push_str("<div class=\"local-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Scrollable table\"><table><thead><tr>");
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
                output.push_str(evidence_state(item.state.clone()));
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
            output.push_str("<div class=\"local-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Scrollable code\"><pre><code data-cf-language=\"");
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
            output.push_str("<div class=\"local-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Scrollable diff\"><pre class=\"diff\"><code>");
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
        Block::Diagram {
            kind,
            source,
            acc_title,
            acc_description,
            ..
        } => {
            output.push_str("<figure class=\"diagram local-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"Scrollable diagram\"><div data-cf-diagram=\"pending\" data-cf-diagram-kind=\"");
            escape_attr_to(&format!("{kind:?}").to_lowercase(), output);
            output.push_str("\" data-cf-diagram-title=\"");
            escape_attr_to(acc_title, output);
            output.push_str("\" data-cf-diagram-description=\"");
            escape_attr_to(acc_description, output);
            output.push_str("\"><template data-cf-diagram-source>");
            escape_html_to(source, output);
            output.push_str("</template><div data-cf-diagram-output><pre><code>");
            escape_html_to(source, output);
            output.push_str("</code></pre></div><p data-cf-diagram-status class=\"sr-only\" role=\"status\"></p></div><figcaption>");
            escape_html_to(acc_title, output);
            output.push_str(" — ");
            escape_html_to(acc_description, output);
            output.push_str("</figcaption></figure>");
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
        Block::Disclosure {
            summary, blocks, ..
        } => {
            output.push_str("<details><summary>");
            escape_html_to(summary, output);
            output.push_str("</summary>");
            for child in blocks {
                render_block(child, options, output);
            }
            output.push_str("</details>");
        }
        Block::Tabs { tabs, .. } => {
            output.push_str("<div class=\"tabs\">");
            for tab in tabs {
                output.push_str("<details><summary>");
                escape_html_to(&tab.label, output);
                output.push_str("</summary>");
                for child in &tab.blocks {
                    render_block(child, options, output);
                }
                output.push_str("</details>");
            }
            output.push_str("</div>");
        }
        Block::FeedbackPrompt { prompt, .. } => {
            output.push_str("<p class=\"feedback-prompt\">");
            escape_html_to(prompt, output);
            output.push_str("</p>");
        }
        Block::Html { id, html, title } => {
            output.push_str("<figure><iframe sandbox title=\"");
            escape_attr_to(title.as_deref().unwrap_or("Sandboxed content"), output);
            if options.interactive {
                output.push_str("\" src=\"/sandbox/");
                output.push_str(&sandbox_id(options.session_id, id));
                output.push_str("\"></iframe>");
            } else {
                output.push_str("\" srcdoc=\"");
                escape_attr_to(html, output);
                output.push_str("\"></iframe>");
            }
            if let Some(title) = title {
                output.push_str("<figcaption>");
                escape_html_to(title, output);
                output.push_str("</figcaption>");
            }
            output.push_str("</figure>");
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
        Block::Diagram { .. } => "diagram",
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
            Event::Html(_) | Event::InlineHtml(_) => None,
            Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => None,
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
                .then(|| Event::Html(CowStr::Borrowed("</a>"))),
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

fn evidence_state(state: EvidenceState) -> &'static str {
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
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
mod tests {
    use crate::document::{Block, PresentationDocument, Provenance};

    use super::*;

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
                script_path: None,
                style_path: None,
                prepaint_source: None,
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
}
