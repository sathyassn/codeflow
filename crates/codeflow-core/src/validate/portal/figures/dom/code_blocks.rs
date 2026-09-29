//! Code blocks in a built page. Starlight renders every fenced code block with
//! Expressive Code, which writes three things into the page content that the
//! content rule otherwise refuses: at the head of the page's first block, a
//! stylesheet link and a module script for its own built assets; and on the
//! highlighted tokens, style attributes holding only CSS custom properties.
//! This module recognises exactly that output and nothing else:
//!
//! - The link and the script are children of a `div.expressive-code` block,
//!   carry exactly the attributes Expressive Code writes, and point at
//!   `<base>_astro/ec.<hash>.css` or `.js`, a built artifact the evidence
//!   records with its hash (the validator checks those bytes like any other).
//! - A style attribute sits on the `pre` of a block's frame
//!   (`div.expressive-code > figure > pre`) or inside it, and every
//!   declaration in it is a custom property Expressive Code writes, whose
//!   value is a hex colour, a fixed keyword or a whole number of `ch`. Any
//!   other property, `url()`, `var()`, comment or escape fails.
//! - Each carrier on an element is judged on its own: an allowed token style
//!   attribute never excuses a style element, a link that is not the
//!   recorded asset link, or any executable content on the same element.
//! - Figure or companion markup on or inside any element with the
//!   `expressive-code` class fails, whatever its tag, because the sheet
//!   scopes its rules on the class alone. With that exclusion, neither those
//!   properties nor the Expressive Code sheet can reach a figure: every rule
//!   of that sheet that styles an element is scoped under `.expressive-code`,
//!   and its only other rules declare its `--ec-*` theme properties on
//!   `:root`, which nothing outside a block reads.
//!
//! Raw HTML can imitate a block. That is harmless only because of the rules
//! above: a custom property reaches just the element that declares it and its
//! descendants, no other carrier rides on an allowed attribute, and no figure
//! may sit in the sheet's class scope. The generated-page check
//! still refuses every style attribute, link and script, so the imitation
//! cannot come from a Markdown source in the first place.

use std::collections::BTreeSet;

use scraper::ElementRef;

use super::has_class;

/// The Expressive Code assets a built page may load: the URL of each
/// `_astro/ec.<hash>` stylesheet and script among the recorded artifacts,
/// under the configured base.
#[derive(Default)]
pub(in crate::validate::portal) struct CodeBlockAssets {
    sheets: BTreeSet<String>,
    scripts: BTreeSet<String>,
}

impl CodeBlockAssets {
    /// The assets among `artifacts` (evidence paths under `dist/`), served
    /// under `base`. Without a valid base no asset is allowed.
    pub(in crate::validate::portal) fn recorded<'a>(
        base: Option<&str>,
        artifacts: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let mut assets = Self::default();
        let Some(base) = base else {
            return assets;
        };
        for path in artifacts {
            let Some(name) = path.strip_prefix("dist/_astro/") else {
                continue;
            };
            let url = format!("{base}_astro/{name}");
            match asset_kind(name) {
                Some("css") => {
                    assets.sheets.insert(url);
                }
                Some("js") => {
                    assets.scripts.insert(url);
                }
                _ => {}
            }
        }
        assets
    }
}

/// `css` or `js` for a file named `ec.<hash>.<extension>` as Expressive Code
/// names its assets.
fn asset_kind(name: &str) -> Option<&str> {
    let (stem, extension) = name.strip_prefix("ec.")?.rsplit_once('.')?;
    let hash = !stem.is_empty()
        && stem.len() <= 32
        && stem
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    (hash && matches!(extension, "css" | "js")).then_some(extension)
}

fn is_block(element: ElementRef<'_>) -> bool {
    element.value().name() == "div" && has_class(element, |class| class == "expressive-code")
}

fn parent_element(element: ElementRef<'_>) -> Option<ElementRef<'_>> {
    element.parent().and_then(ElementRef::wrap)
}

/// Whether the Expressive Code sheet can reach the element: the sheet scopes
/// its rules on the `expressive-code` class whatever the tag, so this is any
/// element carrying that class, or inside one. The asset and style allowance
/// above keeps to the exact `div` structure Expressive Code writes.
pub(super) fn in_sheet_scope(element: ElementRef<'_>) -> bool {
    std::iter::once(element)
        .chain(element.ancestors().filter_map(ElementRef::wrap))
        .any(|candidate| has_class(candidate, |class| class == "expressive-code"))
}

/// The element's attributes are exactly `fixed` plus one URL attribute
/// `url`, whose value is returned; it has no children.
fn exact_attributes<'a>(
    element: ElementRef<'a>,
    fixed: (&str, &str),
    url: &str,
) -> Option<&'a str> {
    let value = element.value();
    let exact = value.attrs().count() == 2
        && value.attr(fixed.0) == Some(fixed.1)
        && element.children().next().is_none();
    exact.then(|| value.attr(url)).flatten()
}

/// The stylesheet link Expressive Code puts at the head of a block.
pub(super) fn asset_link(element: ElementRef<'_>, assets: &CodeBlockAssets) -> bool {
    element.value().name() == "link"
        && parent_element(element).is_some_and(is_block)
        && exact_attributes(element, ("rel", "stylesheet"), "href")
            .is_some_and(|href| assets.sheets.contains(href))
}

/// The module script Expressive Code puts at the head of a block.
pub(super) fn asset_script(element: ElementRef<'_>, assets: &CodeBlockAssets) -> bool {
    element.value().name() == "script"
        && parent_element(element).is_some_and(is_block)
        && exact_attributes(element, ("type", "module"), "src")
            .is_some_and(|src| assets.scripts.contains(src))
}

/// A style attribute Expressive Code writes: on the `pre` of a block's frame
/// or inside it, holding only the custom properties it declares. This judges
/// the attribute alone; the element's own kind is checked separately.
pub(super) fn token_style(element: ElementRef<'_>) -> bool {
    let Some(style) = element.value().attr("style") else {
        return false;
    };
    let pre = std::iter::once(element)
        .chain(element.ancestors().filter_map(ElementRef::wrap))
        .find(|candidate| candidate.value().name() == "pre");
    let framed = pre
        .and_then(parent_element)
        .filter(|frame| frame.value().name() == "figure")
        .and_then(parent_element)
        .is_some_and(is_block);
    framed && custom_properties(style)
}

/// Every declaration is one Expressive Code writes: a token's colour,
/// background, italic, bold or decoration for a theme variant (`--0`,
/// `--0bg`, `--0fs`, `--0fw`, `--0td`), or a wrapped line's indent and a
/// wrapped block's longest line in `ch`. Empty declarations are allowed.
pub(super) fn custom_properties(style: &str) -> bool {
    let mut declared = false;
    for declaration in style.split(';') {
        let declaration = declaration.trim_ascii();
        if declaration.is_empty() {
            continue;
        }
        let Some((name, value)) = declaration.split_once(':') else {
            return false;
        };
        let (name, value) = (name.trim_ascii(), value.trim_ascii());
        let allowed = match name {
            "--ecIndent" | "--ecMaxLine" => whole_ch(value),
            _ => token_property(name, value),
        };
        if !allowed {
            return false;
        }
        declared = true;
    }
    declared
}

fn token_property(name: &str, value: &str) -> bool {
    let Some(rest) = name.strip_prefix("--") else {
        return false;
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=2).contains(&digits) {
        return false;
    }
    match &rest[digits..] {
        "" | "bg" => hex_colour(value),
        "fs" => value == "italic",
        "fw" => value == "bold",
        "td" => matches!(
            value,
            "underline" | "line-through" | "underline line-through"
        ),
        _ => false,
    }
}

fn hex_colour(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn whole_ch(value: &str) -> bool {
    value.strip_suffix("ch").is_some_and(|digits| {
        (1..=4).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}
