//! The figures a generated page puts in front of a reader, read the way a
//! browser reads them. The page's Markdown is rendered to HTML with the
//! site's rule for as-is regions (raw HTML there is text; companions between
//! their markers stay markup), then parsed with an HTML parser, so character
//! references, attribute quoting and comments mean what they mean to a
//! browser. Every element that carries figure or companion markup is then
//! either inside a companion, which the caller compares with its
//! reconstruction, or counted as stray.

use pulldown_cmark::{html, CowStr, Event, Parser, Tag, TagEnd};
use scraper::{ElementRef, Html, Node};

/// The companions a page renders and the figure markup outside them.
pub(super) struct RenderedFigures {
    /// Each element carrying the `cf-companion` class, in document order,
    /// with the level-two heading above it and its canonical form.
    pub companions: Vec<(Option<String>, String)>,
    /// Elements outside every companion that carry figure or companion
    /// markup: a kit class or a figure data attribute.
    pub stray: usize,
}

pub(super) fn rendered_figures(markdown: &str) -> RenderedFigures {
    let document = Html::parse_document(&render_markdown(markdown));
    let mut found = RenderedFigures {
        companions: Vec::new(),
        stray: 0,
    };
    let mut heading = None;
    walk(document.root_element(), &mut heading, &mut found);
    found
}

/// The canonical form of the one element an HTML fragment holds, the form
/// `rendered_figures` records for a companion.
pub(super) fn canonical_fragment(fragment: &str) -> Option<String> {
    let parsed = Html::parse_fragment(fragment);
    let mut elements = parsed.root_element().child_elements();
    let first = elements.next()?;
    elements.next().is_none().then(|| canonical(first))
}

fn walk(element: ElementRef<'_>, heading: &mut Option<String>, found: &mut RenderedFigures) {
    let name = element.value().name();
    if name == "h2" {
        *heading = Some(element.text().collect::<String>().trim().to_lowercase());
    }
    if has_class(element, |class| class == "cf-companion") {
        found.companions.push((heading.clone(), canonical(element)));
        return;
    }
    if figure_markup(element) {
        found.stray += 1;
    }
    for child in element.child_elements() {
        walk(child, heading, found);
    }
}

fn has_class(element: ElementRef<'_>, test: impl Fn(&str) -> bool) -> bool {
    element
        .value()
        .attr("class")
        .is_some_and(|classes| classes.split_ascii_whitespace().any(test))
}

/// A class from the figure kit or a figure data attribute. Text, paths and
/// other attribute values that merely mention a kit name are not markup.
fn figure_markup(element: ElementRef<'_>) -> bool {
    has_class(element, |class| {
        class.starts_with("cf-companion")
            || class.starts_with("cf-fig")
            || class.starts_with("cf-m-")
            || class.starts_with("cf-f-")
            || class == "cf-t"
            || class.starts_with("cf-t--")
            || matches!(class, "cf-legend" | "cf-key" | "cf-twin" | "cf-twin-scroll")
    }) || element.value().attrs().any(|(name, _)| {
        name.starts_with("data-cf-companion") || name.starts_with("data-cf-figure")
    })
}

/// Namespace, name, sorted attributes and children, text as decoded; comments
/// render nothing and are left out.
fn canonical(element: ElementRef<'_>) -> String {
    let value = element.value();
    let mut attributes: Vec<String> = value
        .attrs()
        .map(|(name, text)| format!("{name}={}", serde_json::Value::from(text)))
        .collect();
    attributes.sort();
    let mut out = format!(
        "<{}|{} {}>",
        value.name.ns,
        value.name(),
        attributes.join(" ")
    );
    for child in element.children() {
        match child.value() {
            Node::Text(text) => out.push_str(&serde_json::Value::from(&**text).to_string()),
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(child) {
                    out.push_str(&canonical(child));
                }
            }
            _ => {}
        }
    }
    out.push_str("</>");
    out
}

/// The page's HTML as the site renders it. Inside an as-is region (between
/// the source markers) raw HTML is text, except a companion between its own
/// markers; everywhere else generated HTML is markup.
fn render_markdown(markdown: &str) -> String {
    let mut events: Vec<Event<'_>> = Vec::new();
    let mut depth = 0_usize;
    let mut in_region = false;
    let mut in_companion = false;
    let mut block: Option<Vec<Event<'_>>> = None;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::HtmlBlock) if depth == 0 => {
                block = Some(Vec::new());
                depth += 1;
            }
            Event::End(TagEnd::HtmlBlock) if depth == 1 && block.is_some() => {
                depth -= 1;
                let chunks = block.take().unwrap_or_default();
                let text: String = chunks
                    .iter()
                    .filter_map(|chunk| match chunk {
                        Event::Html(html) => Some(html.as_ref()),
                        _ => None,
                    })
                    .collect();
                let marker = text.trim();
                let was_active =
                    in_region && !in_companion && !marker.starts_with("<!-- codeflow-");
                if marker.starts_with("<!-- codeflow-source-begin") {
                    in_region = true;
                } else if marker.starts_with("<!-- codeflow-source-end") {
                    in_region = false;
                } else if marker.starts_with("<!-- codeflow-companion-begin") {
                    in_companion = true;
                } else if marker.starts_with("<!-- codeflow-companion-end") {
                    in_companion = false;
                }
                if was_active {
                    events.push(Event::Start(Tag::Paragraph));
                    events.push(Event::Text(CowStr::from(text)));
                    events.push(Event::End(TagEnd::Paragraph));
                } else {
                    events.push(Event::Start(Tag::HtmlBlock));
                    events.extend(chunks);
                    events.push(Event::End(TagEnd::HtmlBlock));
                }
            }
            Event::Html(html) | Event::InlineHtml(html) if in_region && !in_companion => {
                if let Some(block) = block.as_mut() {
                    block.push(Event::Html(html));
                } else {
                    events.push(Event::Text(html));
                }
            }
            Event::Start(tag) => {
                depth += 1;
                events.push(Event::Start(tag));
            }
            Event::End(tag) => {
                depth = depth.saturating_sub(1);
                events.push(Event::End(tag));
            }
            other => match block.as_mut() {
                Some(block) => block.push(other),
                None => events.push(other),
            },
        }
    }
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out
}
