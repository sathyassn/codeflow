//! Review entities (SPC-014 B2): the named targets inside a block, their
//! server-resolved labels and, where the server can compute it, their bounds
//! in the user space of their `svg`.
//!
//! This module is the authority for the label rule; the page reads the
//! `data-cf-entity-label` the runtime renders from it.

use std::collections::HashSet;

use scraper::{ElementRef, Html, Node};
use serde::{Deserialize, Serialize};

use crate::{document::Block, limits, Result};

/// The prefix of the reserved legend entity ids.
pub const LEGEND_PREFIX: &str = "legend-";

/// SVG basic shapes whose geometry the server reads (B4).
const BASIC_SHAPES: &[&str] = &[
    "rect", "circle", "ellipse", "line", "polyline", "polygon", "path",
];

/// CSS properties that move or resize SVG geometry. A stage that sets one
/// of them where it can reach an entity leaves that entity unverified.
const GEOMETRY_PROPERTIES: &[&str] = &[
    "transform",
    "translate",
    "rotate",
    "scale",
    "zoom",
    "x",
    "y",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "width",
    "height",
    "d",
];

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn from_extent(extent: Extent) -> Self {
        Self {
            x: extent.min_x,
            y: extent.min_y,
            width: extent.max_x - extent.min_x,
            height: extent.max_y - extent.min_y,
        }
    }

    /// Whether `inner` lies inside this rectangle widened by `tolerance` on
    /// every side.
    #[must_use]
    pub fn contains_within(&self, inner: &Self, tolerance: f64) -> bool {
        inner.x >= self.x - tolerance
            && inner.y >= self.y - tolerance
            && inner.x + inner.width <= self.x + self.width + tolerance
            && inner.y + inner.height <= self.y + self.height + tolerance
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    Wide,
    Narrow,
}

impl Variant {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wide => "wide",
            Self::Narrow => "narrow",
        }
    }
}

/// One review target inside a block. A `figure` mark drawn in both
/// compositions is one entity reported once per variant, because its label
/// and geometry are read from the composition it is drawn in.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub id: String,
    pub label: String,
    pub bounds: Option<Rect>,
    pub variant: Option<Variant>,
    /// Why the server cannot compute `bounds`; `None` when it can.
    pub unverified_reason: Option<String>,
}

/// The entity id grammar, the figure grammar's `KEBAB` rule:
/// `^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$`, at most 64 characters. The reserved
/// value `none` is never an entity id.
#[must_use]
pub fn is_entity_id(value: &str) -> bool {
    value != "none"
        && value.len() <= 64
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

/// Collapse whitespace and cut to the label length (B2).
#[must_use]
pub fn finish_label(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limits::MAX_ENTITY_LABEL_CHARS)
        .collect()
}

fn collapsed(raw: &str) -> Option<String> {
    let label = finish_label(raw);
    (!label.is_empty()).then_some(label)
}

/// Every entity of a block at its revision. Blocks other than `figure` and
/// `html` have none.
pub fn block_entities(block: &Block) -> Result<Vec<Entity>> {
    match block {
        Block::Figure { declaration, .. } => Ok(figure_entities(declaration)),
        Block::Html { html, legend, .. } => {
            let mut entities = stage_entities(&Html::parse_fragment(html));
            if let Some(legend) = legend {
                for (index, entry) in legend.iter().enumerate() {
                    entities.push(Entity {
                        id: format!("{LEGEND_PREFIX}{}", index + 1),
                        label: finish_label(&legend_entry_text(&entry.label, &entry.means)),
                        bounds: None,
                        variant: None,
                        unverified_reason: Some(
                            "a legend entry is page text outside the drawing".to_string(),
                        ),
                    });
                }
            }
            Ok(entities)
        }
        _ => Ok(Vec::new()),
    }
}

/// The visible text of an `html` stage legend entry as the runtime renders it.
#[must_use]
pub fn legend_entry_text(label: &str, means: &str) -> String {
    format!("{label}: {means}")
}

/// The entity a selector names: the one drawn in `variant` when given, else
/// the first with that id.
#[must_use]
pub fn find_entity<'a>(
    entities: &'a [Entity],
    id: &str,
    variant: Option<Variant>,
) -> Option<&'a Entity> {
    entities
        .iter()
        .find(|entity| entity.id == id && (variant.is_none() || entity.variant == variant))
}

// ---------------------------------------------------------------------------
// figure blocks
// ---------------------------------------------------------------------------

fn figure_entities(declaration: &serde_json::Value) -> Vec<Entity> {
    let Some(figure) = declaration.get("figure") else {
        return Vec::new();
    };
    let states = figure
        .get("states")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let means = |state: &str| {
        states
            .iter()
            .find(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(state))
            .and_then(|entry| entry.get("means"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let mut entities = Vec::new();
    // A figure drawn from a layout exposes only its legend entities (B2).
    if figure.get("layout").is_none() {
        for variant in [Variant::Wide, Variant::Narrow] {
            let draw = figure
                .pointer(&format!("/{}/draw", variant.as_str()))
                .and_then(serde_json::Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            for item in draw {
                let (Some(state), Some(id)) = (
                    item.get("state").and_then(serde_json::Value::as_str),
                    item.get("id").and_then(serde_json::Value::as_str),
                ) else {
                    continue;
                };
                let label_texts = draw
                    .iter()
                    .filter(|text| {
                        text.get("for")
                            .and_then(serde_json::Value::as_array)
                            .is_some_and(|ids| ids.iter().any(|value| value.as_str() == Some(id)))
                    })
                    .filter_map(|text| text.get("text").and_then(serde_json::Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                let label = collapsed(&label_texts)
                    .or_else(|| means(state).as_deref().and_then(collapsed))
                    .unwrap_or_else(|| finish_label(id));
                let bounds = mark_extent(item).map(Rect::from_extent);
                entities.push(Entity {
                    id: id.to_string(),
                    label,
                    bounds,
                    variant: Some(variant),
                    unverified_reason: bounds
                        .is_none()
                        .then(|| "the mark's declared geometry is not readable".to_string()),
                });
            }
        }
    }
    for state in states {
        let Some(name) = state.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let id = format!("{LEGEND_PREFIX}{name}");
        let label = state
            .get("means")
            .and_then(serde_json::Value::as_str)
            .and_then(collapsed)
            .unwrap_or_else(|| finish_label(&id));
        entities.push(Entity {
            id,
            label,
            bounds: None,
            variant: None,
            unverified_reason: Some("a legend entry is page text outside the drawing".to_string()),
        });
    }
    entities
}

fn number(item: &serde_json::Value, key: &str) -> Option<f64> {
    item.get(key)
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite())
}

/// The extent of a declared mark's primary shape, as the grammar draws it.
fn mark_extent(item: &serde_json::Value) -> Option<Extent> {
    let shape = item.get("shape").and_then(serde_json::Value::as_str)?;
    let mut extent = Extent::default();
    match shape {
        "rect" => {
            let (x, y) = (number(item, "x")?, number(item, "y")?);
            extent.add(x, y);
            extent.add(x + number(item, "w")?, y + number(item, "h")?);
        }
        "circle" | "diamond" => {
            let (cx, cy, r) = (number(item, "cx")?, number(item, "cy")?, number(item, "r")?);
            extent.add(cx - r, cy - r);
            extent.add(cx + r, cy + r);
        }
        "cross" => {
            let (cx, cy) = (number(item, "cx")?, number(item, "cy")?);
            let half = number(item, "size")? / 2.0;
            extent.add(cx - half, cy - half);
            extent.add(cx + half, cy + half);
        }
        "line" => {
            extent.add(number(item, "x1")?, number(item, "y1")?);
            extent.add(number(item, "x2")?, number(item, "y2")?);
        }
        "polyline" => {
            for point in item.get("points")?.as_array()? {
                let pair = point.as_array()?;
                let (x, y) = (pair.first()?.as_f64()?, pair.get(1)?.as_f64()?);
                if !(x.is_finite() && y.is_finite()) {
                    return None;
                }
                extent.add(x, y);
            }
        }
        "path" => return path_extent(item.get("d")?.as_str()?),
        _ => return None,
    }
    extent.finish()
}

// ---------------------------------------------------------------------------
// html stages
// ---------------------------------------------------------------------------

/// One authored entity element of a stage and what the runtime renders on
/// it.
pub(crate) struct StageEntity {
    /// Position among the fragment's elements in document order.
    pub element_index: usize,
    pub entity: Entity,
}

/// Entities named by the closed vocabulary in a parsed stage, in document
/// order. The vocabulary itself is validated by `safe_html`.
pub(crate) fn stage_entity_nodes(fragment: &Html) -> Vec<StageEntity> {
    let stylesheet_moves_geometry = fragment
        .tree
        .nodes()
        .filter_map(ElementRef::wrap)
        .filter(|element| element.value().name() == "style")
        .any(|style| declares_geometry(&style.text().collect::<String>(), true));
    let elements = fragment
        .tree
        .nodes()
        .filter_map(ElementRef::wrap)
        .collect::<Vec<_>>();
    let mut output = Vec::new();
    for (element_index, element) in elements.iter().enumerate() {
        let Some(id) = entity_id_of(*element) else {
            continue;
        };
        let label = stage_label(*element, id, &elements);
        let (bounds, unverified_reason) = if stylesheet_moves_geometry {
            (
                None,
                Some("a stage style sheet sets transform or geometry properties".to_string()),
            )
        } else {
            match stage_bounds(*element) {
                Ok(bounds) => (Some(bounds), None),
                Err(reason) => (None, Some(reason)),
            }
        };
        output.push(StageEntity {
            element_index,
            entity: Entity {
                id: id.to_string(),
                label,
                bounds,
                variant: None,
                unverified_reason,
            },
        });
    }
    output
}

fn stage_entities(fragment: &Html) -> Vec<Entity> {
    stage_entity_nodes(fragment)
        .into_iter()
        .map(|entry| entry.entity)
        .collect()
}

/// The entity id an element declares: a target other than `none`, or a
/// group.
pub(crate) fn entity_id_of(element: ElementRef<'_>) -> Option<&str> {
    let value = element.value();
    value
        .attr("data-cf-group")
        .or_else(|| value.attr("data-cf-target").filter(|id| *id != "none"))
}

/// The label rule of B2 for a stage entity.
fn stage_label(element: ElementRef<'_>, id: &str, elements: &[ElementRef<'_>]) -> String {
    if let Some(label) = element.value().attr("data-cf-label").and_then(collapsed) {
        return label;
    }
    let for_text = elements
        .iter()
        .filter(|other| {
            other
                .value()
                .attr("data-cf-for")
                .is_some_and(|ids| ids.split_whitespace().any(|named| named == id))
        })
        .map(|other| own_visible_text(*other))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(label) = collapsed(&for_text) {
        return label;
    }
    if let Some(label) = element.value().attr("aria-label").and_then(collapsed) {
        return label;
    }
    collapsed(&own_visible_text(element)).unwrap_or_else(|| finish_label(id))
}

/// The text a reader sees inside an element: each text node collapsed,
/// joined by one space, without style, script, title or desc content.
pub(crate) fn own_visible_text(element: ElementRef<'_>) -> String {
    let mut parts = Vec::new();
    let mut pending = vec![*element];
    // Depth-first, in document order.
    while let Some(node) = pending.pop() {
        match node.value() {
            Node::Text(text) => {
                let part = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !part.is_empty() {
                    parts.push(part);
                }
            }
            Node::Element(child)
                if node.id() != element.id()
                    && matches!(child.name(), "style" | "script" | "title" | "desc") => {}
            _ => pending.extend(node.children().rev()),
        }
    }
    parts.join(" ")
}

/// Bounds for an untransformed basic shape or a group of them, in the user
/// space of its nearest `svg`, or the reason the server cannot compute them.
fn stage_bounds(element: ElementRef<'_>) -> std::result::Result<Rect, String> {
    let mut in_svg = false;
    for ancestor in std::iter::once(element).chain(element.ancestors().filter_map(ElementRef::wrap))
    {
        if ancestor.value().name() == "svg" {
            in_svg = true;
            break;
        }
        if ancestor.value().attr("transform").is_some() {
            return Err("the entity or an ancestor is transformed".to_string());
        }
        if ancestor
            .value()
            .attr("style")
            .is_some_and(|style| declares_geometry(style, false))
        {
            return Err("an inline style moves or resizes the entity".to_string());
        }
    }
    if !in_svg {
        return Err("the entity is not inside an svg".to_string());
    }
    let mut extent = Extent::default();
    add_shape_extent(element, &mut extent)?;
    extent
        .finish()
        .map(Rect::from_extent)
        .ok_or_else(|| "the entity draws no readable geometry".to_string())
}

fn add_shape_extent(
    element: ElementRef<'_>,
    extent: &mut Extent,
) -> std::result::Result<(), String> {
    let value = element.value();
    let name = value.name();
    if value.attr("transform").is_some() {
        return Err("a shape inside the entity is transformed".to_string());
    }
    if value
        .attr("style")
        .is_some_and(|style| declares_geometry(style, false))
    {
        return Err("an inline style moves or resizes a shape inside the entity".to_string());
    }
    if name == "g" {
        for child in element.children().filter_map(ElementRef::wrap) {
            if matches!(child.value().name(), "title" | "desc") {
                continue;
            }
            add_shape_extent(child, extent)?;
        }
        return Ok(());
    }
    if !BASIC_SHAPES.contains(&name) {
        return Err(format!(
            "the entity contains a {name} element, whose bounds the server does not compute"
        ));
    }
    let attr = |key: &str| -> std::result::Result<f64, String> {
        parse_length(value.attr(key).unwrap_or("0"))
            .ok_or_else(|| format!("the {name} attribute {key} is not a plain number"))
    };
    match name {
        "rect" => {
            let (x, y) = (attr("x")?, attr("y")?);
            extent.add(x, y);
            extent.add(x + attr("width")?, y + attr("height")?);
        }
        "circle" => {
            let (cx, cy, r) = (attr("cx")?, attr("cy")?, attr("r")?);
            extent.add(cx - r, cy - r);
            extent.add(cx + r, cy + r);
        }
        "ellipse" => {
            let (cx, cy, rx, ry) = (attr("cx")?, attr("cy")?, attr("rx")?, attr("ry")?);
            extent.add(cx - rx, cy - ry);
            extent.add(cx + rx, cy + ry);
        }
        "line" => {
            extent.add(attr("x1")?, attr("y1")?);
            extent.add(attr("x2")?, attr("y2")?);
        }
        "polyline" | "polygon" => {
            let numbers = value
                .attr("points")
                .unwrap_or_default()
                .split(|character: char| character == ',' || character.is_whitespace())
                .filter(|part| !part.is_empty())
                .map(parse_length)
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| format!("the {name} points are not plain numbers"))?;
            if numbers.len() < 2 || numbers.len() % 2 != 0 {
                return Err(format!("the {name} points are not coordinate pairs"));
            }
            for pair in numbers.chunks(2) {
                extent.add(pair[0], pair[1]);
            }
        }
        _ => {
            let path = path_extent(value.attr("d").unwrap_or_default()).ok_or_else(|| {
                "the path is not an absolute path of M, L, H, V, C, Q and Z commands".to_string()
            })?;
            extent.merge(path);
        }
    }
    Ok(())
}

fn parse_length(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    let number = trimmed.strip_suffix("px").unwrap_or(trimmed);
    number.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// Whether CSS declares a property that moves or resizes SVG geometry. A
/// style sheet is read block by block; an inline style is one block.
fn declares_geometry(css: &str, sheet: bool) -> bool {
    let mut text = css.to_ascii_lowercase();
    while let Some(start) = text.find("/*") {
        let end = text[start + 2..]
            .find("*/")
            .map_or(text.len(), |offset| start + 2 + offset + 2);
        text.replace_range(start..end, " ");
    }
    let blocks: Vec<&str> = if sheet {
        text.split('{')
            .skip(1)
            .map(|block| block.split('}').next().unwrap_or_default())
            .collect()
    } else {
        vec![text.as_str()]
    };
    blocks
        .iter()
        .flat_map(|block| block.split(';'))
        .any(|declaration| {
            declaration.split_once(':').is_some_and(|(property, _)| {
                let property = property.trim();
                GEOMETRY_PROPERTIES.contains(&property) || property.starts_with("offset")
            })
        })
}

/// Render-time entity attributes for a v2 stage: `data-cf-entity` and
/// `data-cf-entity-label` on every entity element, and
/// `data-cf-entity-none` on every `data-cf-target="none"` subtree root.
pub(crate) fn annotate_stage(fragment: &mut Html) {
    let entities = stage_entity_nodes(fragment);
    let nodes = fragment
        .tree
        .nodes()
        .filter_map(ElementRef::wrap)
        .map(|element| {
            (
                element.id(),
                element.value().attr("data-cf-target") == Some("none"),
            )
        })
        .collect::<Vec<_>>();
    let mut additions = Vec::new();
    for entry in entities {
        let node = nodes[entry.element_index].0;
        additions.push((node, "data-cf-entity", entry.entity.id));
        additions.push((node, "data-cf-entity-label", entry.entity.label));
    }
    for (node, none) in &nodes {
        if *none {
            additions.push((*node, "data-cf-entity-none", String::new()));
        }
    }
    for (node, name, value) in additions {
        let Some(mut node) = fragment.tree.get_mut(node) else {
            continue;
        };
        let Node::Element(element) = node.value() else {
            continue;
        };
        // Entity elements always carry a vocabulary attribute, whose
        // qualified name (no namespace, no prefix) the new one copies.
        let Some(mut qualified) = element
            .attrs
            .iter()
            .find(|(existing, _)| existing.local.as_ref().starts_with("data-cf-"))
            .map(|(existing, _)| existing.clone())
        else {
            continue;
        };
        qualified.local = name.into();
        element.attrs.retain(|(existing, _)| *existing != qualified);
        element.attrs.push((qualified, value.as_str().into()));
        element
            .attrs
            .sort_unstable_by(|left, right| left.0.cmp(&right.0));
    }
}

/// Ids of every entity a stage declares, for vocabulary checks.
pub(crate) fn declared_ids(fragment: &Html) -> HashSet<String> {
    fragment
        .tree
        .nodes()
        .filter_map(ElementRef::wrap)
        .filter_map(|element| entity_id_of(element).map(str::to_string))
        .collect()
}

// ---------------------------------------------------------------------------
// geometry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Extent {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    empty: bool,
}

impl Default for Extent {
    fn default() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
            empty: true,
        }
    }
}

impl Extent {
    fn add(&mut self, x: f64, y: f64) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
        self.empty = false;
    }

    fn merge(&mut self, other: Self) {
        if !other.empty {
            self.add(other.min_x, other.min_y);
            self.add(other.max_x, other.max_y);
        }
    }

    fn finish(self) -> Option<Self> {
        (!self.empty
            && self.min_x.is_finite()
            && self.min_y.is_finite()
            && self.max_x.is_finite()
            && self.max_y.is_finite())
        .then_some(self)
    }
}

/// The tight extent of an absolute path of `M L H V C Q Z` commands, with
/// curve extrema solved exactly. Any other command is not read.
fn path_extent(d: &str) -> Option<Extent> {
    let tokens = path_tokens(d)?;
    // A path starts with a moveto; anything else is not drawn.
    if !matches!(tokens.first(), Some(PathToken::Command('M'))) {
        return None;
    }
    let mut extent = Extent::default();
    let (mut x, mut y) = (0.0_f64, 0.0_f64);
    let (mut start_x, mut start_y) = (0.0_f64, 0.0_f64);
    let mut index = 0;
    let mut command: Option<char> = None;
    let take = |index: &mut usize, count: usize| -> Option<Vec<f64>> {
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            match tokens.get(*index) {
                Some(PathToken::Number(value)) => values.push(*value),
                _ => return None,
            }
            *index += 1;
        }
        Some(values)
    };
    while index < tokens.len() {
        if let PathToken::Command(letter) = tokens[index] {
            command = Some(letter);
            index += 1;
            if letter == 'Z' {
                x = start_x;
                y = start_y;
                continue;
            }
        }
        match command? {
            'M' => {
                let values = take(&mut index, 2)?;
                (x, y) = (values[0], values[1]);
                (start_x, start_y) = (x, y);
                extent.add(x, y);
                // Further pairs after a moveto are implicit linetos.
                command = Some('L');
            }
            'L' => {
                let values = take(&mut index, 2)?;
                (x, y) = (values[0], values[1]);
                extent.add(x, y);
            }
            'H' => {
                x = take(&mut index, 1)?[0];
                extent.add(x, y);
            }
            'V' => {
                y = take(&mut index, 1)?[0];
                extent.add(x, y);
            }
            'C' => {
                let values = take(&mut index, 6)?;
                cubic_extent(
                    &mut extent,
                    [x, values[0], values[2], values[4]],
                    [y, values[1], values[3], values[5]],
                );
                (x, y) = (values[4], values[5]);
            }
            'Q' => {
                let values = take(&mut index, 4)?;
                quadratic_extent(
                    &mut extent,
                    [x, values[0], values[2]],
                    [y, values[1], values[3]],
                );
                (x, y) = (values[2], values[3]);
            }
            // A number after Z, or any command the server does not read.
            _ => return None,
        }
    }
    extent.finish()
}

#[derive(Debug, Clone, Copy)]
enum PathToken {
    Command(char),
    Number(f64),
}

fn path_tokens(d: &str) -> Option<Vec<PathToken>> {
    let bytes = d.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() || byte == b',' {
            index += 1;
            continue;
        }
        if matches!(byte, b'M' | b'L' | b'H' | b'V' | b'C' | b'Q' | b'Z' | b'z') {
            tokens.push(PathToken::Command(if byte == b'z' {
                'Z'
            } else {
                byte as char
            }));
            index += 1;
            continue;
        }
        if byte.is_ascii_alphabetic() && !matches!(byte, b'e' | b'E') {
            return None;
        }
        let start = index;
        index += 1;
        let mut seen_dot = byte == b'.';
        while index < bytes.len() {
            let next = bytes[index];
            let exponent_sign =
                matches!(next, b'-' | b'+') && matches!(bytes[index - 1], b'e' | b'E');
            if next.is_ascii_digit() || matches!(next, b'e' | b'E') || exponent_sign {
                index += 1;
            } else if next == b'.' && !seen_dot {
                seen_dot = true;
                index += 1;
            } else {
                break;
            }
        }
        let value = d[start..index].parse::<f64>().ok()?;
        if !value.is_finite() {
            return None;
        }
        tokens.push(PathToken::Number(value));
    }
    Some(tokens)
}

fn cubic_at(p: [f64; 4], t: f64) -> f64 {
    let u = 1.0 - t;
    u * u * u * p[0] + 3.0 * u * u * t * p[1] + 3.0 * u * t * t * p[2] + t * t * t * p[3]
}

fn cubic_extent(extent: &mut Extent, xs: [f64; 4], ys: [f64; 4]) {
    extent.add(xs[0], ys[0]);
    extent.add(xs[3], ys[3]);
    let mut roots = Vec::new();
    for p in [xs, ys] {
        // Derivative: a t^2 + b t + c.
        let a = -p[0] + 3.0 * p[1] - 3.0 * p[2] + p[3];
        let b = 2.0 * (p[0] - 2.0 * p[1] + p[2]);
        let c = p[1] - p[0];
        if a.abs() < 1e-12 {
            if b.abs() > 1e-12 {
                roots.push(-c / b);
            }
            continue;
        }
        let discriminant = b * b - 4.0 * a * c;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            roots.push((-b + root) / (2.0 * a));
            roots.push((-b - root) / (2.0 * a));
        }
    }
    for t in roots.into_iter().filter(|t| *t > 0.0 && *t < 1.0) {
        extent.add(cubic_at(xs, t), cubic_at(ys, t));
    }
}

fn quadratic_extent(extent: &mut Extent, xs: [f64; 3], ys: [f64; 3]) {
    extent.add(xs[0], ys[0]);
    extent.add(xs[2], ys[2]);
    let at = |p: [f64; 3], t: f64| {
        let u = 1.0 - t;
        u * u * p[0] + 2.0 * u * t * p[1] + t * t * p[2]
    };
    for p in [xs, ys] {
        let denominator = p[0] - 2.0 * p[1] + p[2];
        if denominator.abs() > 1e-12 {
            let t = (p[0] - p[1]) / denominator;
            if t > 0.0 && t < 1.0 {
                extent.add(at(xs, t), at(ys, t));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn close(left: Rect, right: Rect) -> bool {
        (left.x - right.x).abs() < 1e-9
            && (left.y - right.y).abs() < 1e-9
            && (left.width - right.width).abs() < 1e-9
            && (left.height - right.height).abs() < 1e-9
    }

    #[test]
    fn a_crop_may_pass_each_edge_by_the_tolerance_and_no_further() {
        let bounds = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let tolerance = crate::limits::ENTITY_CROP_TOLERANCE;
        let inside = Rect {
            x: 2.0,
            y: 12.0,
            width: 46.0,
            height: 56.0,
        };
        assert!(bounds.contains_within(&inside, tolerance));
        for outside in [
            Rect { x: 1.5, ..inside },
            Rect { y: 11.5, ..inside },
            Rect {
                width: 46.5,
                ..inside
            },
            Rect {
                height: 56.5,
                ..inside
            },
        ] {
            assert!(!bounds.contains_within(&outside, tolerance), "{outside:?}");
        }
    }

    #[test]
    fn entity_ids_follow_the_grammar() {
        for valid in ["a", "m15", "submit-edge", "a1-2b", &"a".repeat(64)] {
            assert!(is_entity_id(valid), "{valid}");
        }
        for invalid in [
            "",
            "none",
            "Service",
            "-a",
            "a-",
            "a--b",
            "0-x",
            "a_b",
            "a.b",
            "é",
            &"a".repeat(65),
        ] {
            assert!(!is_entity_id(invalid), "{invalid}");
        }
    }

    #[test]
    fn labels_collapse_and_cut_to_120_characters() {
        assert_eq!(finish_label("  Two\n  words "), "Two words");
        assert_eq!(finish_label(&"é".repeat(130)).chars().count(), 120);
    }

    #[test]
    fn path_extent_reads_absolute_commands_with_exact_curve_extrema() {
        let straight = path_extent("M99 100H261").unwrap();
        assert!(close(
            Rect::from_extent(straight),
            rect(99.0, 100.0, 162.0, 0.0)
        ));
        let diamond = path_extent("M270 169L281 180L270 191L259 180Z").unwrap();
        assert!(close(
            Rect::from_extent(diamond),
            rect(259.0, 169.0, 22.0, 22.0)
        ));
        // The control points reach y 150 but the curve does not.
        let curve = Rect::from_extent(path_extent("M459 180H540C590 180 630 150 630 111").unwrap());
        assert!((curve.x - 459.0).abs() < 1e-9 && (curve.y - 111.0).abs() < 1e-9);
        assert!((curve.width - 171.0).abs() < 1e-9 && (curve.height - 69.0).abs() < 1e-9);
        let quadratic = Rect::from_extent(path_extent("M0 0Q50 100 100 0").unwrap());
        assert!((quadratic.height - 50.0).abs() < 1e-9);
        let implicit = Rect::from_extent(path_extent("M0,0 10,10 20,-5").unwrap());
        assert!(close(implicit, rect(0.0, -5.0, 20.0, 15.0)));
        for unread in [
            "m0 0h10",
            "M0 0A5 5 0 0 1 10 10",
            "M0 0S1 1 2 2",
            "L0 0",
            "M0",
            "",
        ] {
            assert!(path_extent(unread).is_none(), "{unread}");
        }
    }

    #[test]
    fn stage_bounds_cover_basic_shapes_and_refuse_what_the_server_cannot_see() {
        let bounds = |html: &str| {
            let fragment = Html::parse_fragment(html);
            stage_entities(&fragment)
                .into_iter()
                .map(|entity| (entity.id, entity.bounds, entity.unverified_reason))
                .collect::<Vec<_>>()
        };
        let found = bounds(
            "<svg><g data-cf-group='a'><rect x='10' y='20' width='30' height='40'/><circle cx='0' cy='0' r='5'/></g>\
             <ellipse data-cf-target='b' cx='10' cy='10' rx='4' ry='2'/>\
             <polygon data-cf-target='c' points='0,0 10,0 5,8'/>\
             <line data-cf-target='d' x1='1' y1='2' x2='3' y2='4'/></svg>",
        );
        assert!(close(found[0].1.unwrap(), rect(-5.0, -5.0, 45.0, 65.0)));
        assert!(close(found[1].1.unwrap(), rect(6.0, 8.0, 8.0, 4.0)));
        assert!(close(found[2].1.unwrap(), rect(0.0, 0.0, 10.0, 8.0)));
        assert!(close(found[3].1.unwrap(), rect(1.0, 2.0, 2.0, 2.0)));

        for (html, reason) in [
            ("<svg><g data-cf-target='t' transform='translate(1 1)'><rect width='1' height='1'/></g></svg>", "transformed"),
            ("<svg><g transform='scale(2)'><rect data-cf-target='t' width='1' height='1'/></g></svg>", "transformed"),
            ("<svg><g data-cf-target='t'><rect width='1' height='1'/><text>Label</text></g></svg>", "text element"),
            ("<svg><rect data-cf-target='t' width='50%' height='1'/></svg>", "not a plain number"),
            ("<div data-cf-target='t'>HTML</div>", "not inside an svg"),
            ("<svg><rect data-cf-target='t' style='transform: rotate(4deg)' width='1' height='1'/></svg>", "inline style"),
            ("<style>rect { x: 40px }</style><svg><rect data-cf-target='t' width='1' height='1'/></svg>", "style sheet"),
            ("<svg><path data-cf-target='t' d='m0 0 l5 5'/></svg>", "absolute path"),
        ] {
            let found = bounds(html);
            assert_eq!(found.len(), 1, "{html}");
            assert!(found[0].1.is_none(), "{html}");
            assert!(found[0].2.as_deref().unwrap().contains(reason), "{html}: {:?}", found[0].2);
        }
        // Harmless styles keep bounds: presentation, not geometry.
        let styled = bounds("<style>.box { fill: red; stroke-width: 2 }</style><svg style='width:100%;height:auto'><rect data-cf-target='t' style='font-family:serif' width='1' height='1'/></svg>");
        assert!(styled[0].1.is_some(), "{styled:?}");
    }

    #[test]
    fn stage_labels_follow_the_b2_order_and_are_never_a_tag_name() {
        let fragment = Html::parse_fragment(
            "<svg><g data-cf-group='browser' data-cf-label='Browser page' aria-label='ignored'><rect/><text>Page</text></g>\
             <line data-cf-target='submit-edge'/><text data-cf-for='submit-edge'>submit</text><text data-cf-for='submit-edge service'>call</text>\
             <g data-cf-target='service'><rect/><text>Service</text><title>hidden title</title></g>\
             <g data-cf-target='aria' aria-label=' Named  by aria '><rect/><text>visible</text></g>\
             <rect data-cf-target='bare-box'/></svg>",
        );
        let labels = stage_entities(&fragment)
            .into_iter()
            .map(|entity| (entity.id, entity.label))
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            [
                ("browser", "Browser page"),
                ("submit-edge", "submit call"),
                ("service", "call"),
                ("aria", "Named by aria"),
                ("bare-box", "bare-box"),
            ]
            .map(|(id, label)| (id.to_string(), label.to_string()))
        );
    }
}
