//! The figures a generated page puts in front of a reader, read the way a
//! browser reads them. The page's Markdown is rendered to HTML with the
//! site's rule for as-is regions (raw HTML there is text; companions between
//! their markers stay markup), then parsed with an HTML parser, so character
//! references, attribute quoting and comments mean what they mean to a
//! browser. Every element that carries figure or companion markup is then
//! either inside a companion, which the caller compares with its
//! reconstruction, or counted as stray. Page content carries no CSS and no
//! executable content at all: a style element, a stylesheet link, a style
//! attribute, a script, an event handler, a `javascript:` URL, a frame or
//! embedded object, or a declarative shadow root anywhere in it is recorded,
//! so only the site's own built sheets and runtime can style or script a page.
//! A built page is read the same way before a browser consumes any template:
//! its content region carries none of these, except the exact output of a
//! code block (see `code_blocks`), and the rest of the page carries no style
//! element, event handler, `javascript:` URL, frame or shadow root.

use pulldown_cmark::{html, CowStr, Event, Parser, Tag, TagEnd};
use scraper::{ElementRef, Html, Node};

mod code_blocks;

pub(in crate::validate::portal) use code_blocks::CodeBlockAssets;

/// The companions a page renders and the figure markup outside them.
pub(super) struct RenderedFigures {
    /// Each element carrying the `cf-companion` class, in document order,
    /// with the level-two heading above it and its canonical form.
    pub companions: Vec<(Option<String>, String)>,
    /// For each companion, the text of the nearest heading of any level
    /// above it.
    pub sections: Vec<Option<String>>,
    /// Every heading outside the companions, in document order: the
    /// level-two heading it sits under (itself for a level-two heading), its
    /// level and its text.
    pub headings: Vec<(Option<String>, usize, String)>,
    /// Elements outside every companion that carry figure or companion
    /// markup: a kit class or a figure data attribute.
    pub stray: usize,
    /// The kinds of CSS the rendered content carries, each named once.
    pub css: std::collections::BTreeSet<String>,
    /// The kinds of executable content the rendered content carries.
    pub active: std::collections::BTreeSet<String>,
}

/// What a built page carries that only the runtime may: in its content
/// region, any CSS or executable content; anywhere, the carriers the runtime
/// never emits.
#[derive(Default)]
pub(super) struct BuiltCarriers {
    pub content: std::collections::BTreeSet<String>,
    pub page: std::collections::BTreeSet<String>,
    /// The text of each inline script outside the content, which must be one
    /// the runtime emits.
    pub inline_scripts: Vec<String>,
}

pub(super) fn built_page_carriers(html: &str, assets: &CodeBlockAssets) -> BuiltCarriers {
    let document = Html::parse_document(html);
    let mut found = BuiltCarriers::default();
    for element in document
        .root_element()
        .descendants()
        .filter_map(ElementRef::wrap)
    {
        let in_content = element
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(|ancestor| has_class(ancestor, |class| class == "sl-markdown-content"));
        if in_content {
            // Each carrier is judged on its own: the token exception covers
            // only the style attribute, and only the recorded asset link is
            // a link the content may hold.
            if !code_blocks::asset_link(element, assets) {
                found.content.extend(element_css(element));
            }
            if !code_blocks::token_style(element) {
                found.content.extend(attribute_css(element));
            }
            if !code_blocks::asset_script(element, assets) {
                found.content.extend(active_carrier(element));
            }
            if figure_markup(element) && code_blocks::in_sheet_scope(element) {
                found
                    .content
                    .insert("figure or companion markup inside a code block".to_string());
            }
        } else {
            if element.value().name() == "style" {
                found.page.insert("a <style> element".to_string());
            }
            if element.value().name() == "script" {
                if element.value().attr("src").is_none() {
                    found.inline_scripts.push(element.text().collect());
                }
            } else {
                found.page.extend(active_carrier(element));
            }
        }
    }
    found
}

pub(super) fn rendered_figures(markdown: &str) -> RenderedFigures {
    let document = Html::parse_document(&render_markdown(markdown));
    let mut found = RenderedFigures {
        companions: Vec::new(),
        sections: Vec::new(),
        headings: Vec::new(),
        stray: 0,
        css: std::collections::BTreeSet::new(),
        active: std::collections::BTreeSet::new(),
    };
    let mut heading = Headings::default();
    walk(document.root_element(), &mut heading, &mut found);
    for element in document
        .root_element()
        .descendants()
        .filter_map(ElementRef::wrap)
    {
        if let Some(kind) = css_carrier(element) {
            found.css.insert(kind);
        }
        if let Some(kind) = active_carrier(element) {
            found.active.insert(kind);
        }
    }
    found
}

/// CSS in rendered content, in any namespace and inside template contents:
/// a style element, a link element or a style attribute.
fn css_carrier(element: ElementRef<'_>) -> Option<String> {
    element_css(element).or_else(|| attribute_css(element))
}

/// A style element or a link element.
fn element_css(element: ElementRef<'_>) -> Option<String> {
    match element.value().name() {
        "style" => Some("a <style> element".to_string()),
        "link" => Some("a <link> element".to_string()),
        _ => None,
    }
}

/// A style attribute, on any element.
fn attribute_css(element: ElementRef<'_>) -> Option<String> {
    let value = element.value();
    value
        .attr("style")
        .map(|_| format!("a style attribute on <{}>", value.name()))
}

/// Executable content, in any namespace and inside template contents: a
/// script, a frame or embedded object, a declarative shadow root (open or
/// closed, which can carry styles and scripts of its own), an event-handler
/// attribute, or a URL a browser would run as script.
fn active_carrier(element: ElementRef<'_>) -> Option<String> {
    let value = element.value();
    let name = value.name();
    match name {
        "script" => return Some("a <script> element".to_string()),
        "iframe" | "frame" | "frameset" | "object" | "embed" => {
            return Some(format!("an <{name}> element"));
        }
        "template"
            if value.attrs().any(|(attribute, _)| {
                attribute == "shadowrootmode" || attribute == "shadowroot"
            }) =>
        {
            return Some("a declarative shadow root".to_string());
        }
        _ => {}
    }
    for (attribute, text) in value.attrs() {
        if attribute.len() > 2
            && attribute
                .get(..2)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("on"))
        {
            return Some(format!("an event-handler attribute on <{name}>"));
        }
        let url = matches!(
            attribute,
            "href"
                | "src"
                | "action"
                | "formaction"
                | "xlink:href"
                | "data"
                | "poster"
                | "background"
                | "srcdoc"
        ) || (matches!(name, "animate" | "set")
            && matches!(attribute, "to" | "from" | "by" | "values"));
        if url && (attribute == "srcdoc" || runs_script(text)) {
            return Some(format!("a script URL on <{name}>"));
        }
    }
    None
}

/// A URL a browser runs as script: after the leading and trailing control
/// characters and spaces it trims and the tabs and newlines it removes, the
/// scheme is `javascript:` in any case. A list of values runs if any does.
fn runs_script(text: &str) -> bool {
    text.split(';').any(|part| {
        let cleaned: String = part
            .trim_matches(|c: char| c <= ' ')
            .chars()
            .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
            .collect();
        cleaned
            .get(..11)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("javascript:"))
    })
}

/// The canonical form of the one element an HTML fragment holds, the form
/// `rendered_figures` records for a companion.
pub(super) fn canonical_fragment(fragment: &str) -> Option<String> {
    let parsed = Html::parse_fragment(fragment);
    let mut elements = parsed.root_element().child_elements();
    let first = elements.next()?;
    elements.next().is_none().then(|| canonical(first))
}

/// The level-two heading and the nearest heading of any level seen so far.
#[derive(Default)]
struct Headings {
    panel: Option<String>,
    nearest: Option<String>,
}

fn walk(element: ElementRef<'_>, heading: &mut Headings, found: &mut RenderedFigures) {
    let name = element.value().name();
    if let Some(level) = name
        .strip_prefix('h')
        .and_then(|digit| digit.parse::<usize>().ok())
        .filter(|level| (1..=6).contains(level))
    {
        let text = element.text().collect::<String>();
        if level == 2 {
            heading.panel = Some(text.trim().to_lowercase());
        }
        heading.nearest = Some(text.trim().to_string());
        found
            .headings
            .push((heading.panel.clone(), level, text.trim().to_string()));
    }
    if has_class(element, |class| class == "cf-companion") {
        found
            .companions
            .push((heading.panel.clone(), canonical(element)));
        found.sections.push(heading.nearest.clone());
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
