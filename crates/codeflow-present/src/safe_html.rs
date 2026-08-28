use scraper::{ElementRef, Html};

use crate::{PresentError, Result};

const FORBIDDEN_ELEMENTS: &[&str] = &[
    "a",
    "animate",
    "animatemotion",
    "animatetransform",
    "applet",
    "area",
    "base",
    "embed",
    "form",
    "frame",
    "frameset",
    "iframe",
    "link",
    "meta",
    "object",
    "portal",
    "script",
    "set",
];

const URL_ATTRIBUTES: &[&str] = &[
    "action",
    "background",
    "cite",
    "classid",
    "codebase",
    "data",
    "formaction",
    "href",
    "longdesc",
    "manifest",
    "ping",
    "poster",
    "profile",
    "src",
    "srcdoc",
    "srcset",
    "usemap",
    "xlink:href",
];

/// Validate the v1 sandboxed-HTML subset before any browser sees it.
///
/// The sandbox remains defense in depth; this parser-level boundary rejects
/// document navigation, active content, and remote resource references even if
/// browser navigation semantics or a future CSP interpretation changes.
/// Visible text a reviewer can select in the rendered sandbox, including SVG
/// `<text>` labels. Must stay aligned with `captureSelection` so a Text pin
/// survives server-side selector validation.
pub(crate) fn visible_text_from_html(source: &str) -> String {
    Html::parse_fragment(source)
        .root_element()
        .text()
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn validate_sandbox_html(source: &str) -> Result<()> {
    let fragment = Html::parse_fragment(source);
    for element in fragment.tree.nodes().filter_map(ElementRef::wrap) {
        validate_element(element)?;
    }
    Ok(())
}

fn validate_element(element: ElementRef<'_>) -> Result<()> {
    let tag = element.value().name().to_ascii_lowercase();
    if FORBIDDEN_ELEMENTS.contains(&tag.as_str()) {
        return Err(invalid(format!(
            "sandboxed html element <{tag}> can navigate, embed, submit, or execute content"
        )));
    }

    if tag == "style" {
        let css = element.text().collect::<String>();
        validate_inline_css(&css)?;
    }

    for (name, value) in &element.value().attrs {
        let local = name.local.as_ref().to_ascii_lowercase();
        if local.starts_with("on") {
            return Err(invalid(format!(
                "sandboxed html event attribute {local} is not allowed"
            )));
        }
        if local == "style" {
            validate_inline_css(value)?;
            continue;
        }
        let qualified = if name
            .prefix
            .as_ref()
            .is_some_and(|prefix| prefix.as_ref() == "xlink")
        {
            format!("xlink:{local}")
        } else {
            local.clone()
        };
        if URL_ATTRIBUTES.contains(&qualified.as_str())
            && !allowed_fragment_reference(&tag, &qualified, value)
        {
            return Err(invalid(format!(
                "sandboxed html attribute {qualified} on <{tag}> can navigate or load a resource"
            )));
        }
        let lower_value = value.to_ascii_lowercase();
        if lower_value.contains("url(") && !is_css_fragment_reference(&lower_value) {
            return Err(invalid(format!(
                "sandboxed html attribute {qualified} on <{tag}> references an external resource"
            )));
        }
    }
    Ok(())
}

fn allowed_fragment_reference(tag: &str, attribute: &str, value: &str) -> bool {
    let value = value.trim();
    if matches!(attribute, "href" | "xlink:href") && tag != "a" && tag != "area" {
        return is_safe_fragment(value);
    }
    false
}

fn is_safe_fragment(value: &str) -> bool {
    value
        .strip_prefix('#')
        .is_some_and(|fragment| !fragment.is_empty() && fragment.bytes().all(is_fragment_byte))
}

fn is_fragment_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
}

fn is_css_fragment_reference(value: &str) -> bool {
    let compact = value
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b'\'' | b'"'))
        .collect::<Vec<_>>();
    compact
        .strip_prefix(b"url(#")
        .and_then(|value| value.strip_suffix(b")"))
        .is_some_and(|fragment| {
            !fragment.is_empty() && fragment.iter().copied().all(is_fragment_byte)
        })
}

fn validate_inline_css(css: &str) -> Result<()> {
    let lower = css.to_ascii_lowercase();
    if lower.contains('@')
        || lower.contains('\\')
        || lower.contains("url")
        || lower.contains("image-set")
        || lower.contains("cross-fade")
        || lower.contains("element(")
        || lower.contains("paint(")
        || lower.contains("expression")
        || lower.contains("behavior")
        || lower.contains("binding")
        || lower.contains("javascript:")
        || lower.contains("data:")
        || lower.contains("http:")
        || lower.contains("https:")
    {
        return Err(invalid(
            "sandboxed html CSS may not import, navigate, or reference a resource",
        ));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> PresentError {
    PresentError::InvalidDocument(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_static_html_inline_style_and_local_svg_references() {
        let html = r#"<style>.card{color:#123;background:#fff}</style>
          <section class="card"><svg><defs><linearGradient id="g"/></defs><rect fill="url(#g)"/></svg></section>"#;
        assert!(validate_sandbox_html(html).is_ok());
    }

    #[test]
    fn rejects_navigation_execution_submission_embedding_and_remote_resources() {
        for html in [
            r#"<meta http-equiv="refresh" content="0;url=https://example.test">"#,
            r#"<a href="https://example.test">leave</a>"#,
            r#"<form action="https://example.test"><button>send</button></form>"#,
            r#"<iframe src="https://example.test"></iframe>"#,
            r#"<applet code="x.class"></applet>"#,
            r"<script>location='https://example.test'</script>",
            r#"<img src="https://example.test/pixel.png">"#,
            r#"<img src="data:image/png;base64,iVBORw0KGgo=">"#,
            r#"<div style="background:url(https://example.test/pixel.png)"></div>"#,
            r#"<div style="background-image:image-set('relative.png' 1x)"></div>"#,
            r"<style>@import 'https://example.test/a.css'</style>",
            r#"<button formaction="https://example.test">send</button>"#,
            r#"<div onclick="location='https://example.test'">go</div>"#,
            r#"<svg><a xlink:href="https://example.test"><text>go</text></a></svg>"#,
            r#"<svg><animate attributeName="href" to="https://example.test"/></svg>"#,
            r#"<svg><set attributeName="href" to="https://example.test"/></svg>"#,
        ] {
            assert!(validate_sandbox_html(html).is_err(), "accepted {html}");
        }
    }
}
