//! An independent reconstruction of the grammar module's `renderFigure`
//! (`docs-portal/scripts/figure-grammar.mjs`), so the validator can prove the
//! figure a page carries is the one its pinned declaration draws, byte for
//! byte, without trusting any hash the evidence manifest supplies.
//!
//! The port follows the module operation by operation, including JavaScript's
//! number formatting and rounding, so both produce the same bytes. The shared
//! fixtures under `docs-portal/tests/fixtures/figures/rendered/` pin the two
//! together: the module's tests pin them to `renderFigure` and this module's
//! tests pin them to the reconstruction.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::float_cmp,
    clippy::manual_midpoint,
    clippy::format_collect,
    clippy::too_many_lines,
    reason = "each function mirrors one grammar module function and its JavaScript number semantics (exact equality, (a + b) / 2, counts as doubles) so both draw the same bytes; restructuring for lint shape would lose the one-to-one reading"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

const WIDE_MAX_WIDTH: f64 = 720.0;
const ELONGATION_MAX: f64 = 1.5;
const LABEL_GAP: f64 = 12.0;
const LINE: [&str; 3] = ["path", "line", "polyline"];

/// A mark of the closed vocabulary: the kit class, its legend key form, and
/// the head, hatch and cross parts it carries of its own.
struct Mark {
    class: &'static str,
    key: &'static str,
    head: Option<&'static str>,
    hatch: bool,
    cross: Option<&'static str>,
}

const fn mark(class: &'static str, key: &'static str) -> Mark {
    Mark {
        class,
        key,
        head: None,
        hatch: false,
        cross: None,
    }
}

fn mark_for(name: &str) -> Option<Mark> {
    Some(match name {
        "done" => mark("cf-m-done", "line"),
        "todo" => mark("cf-m-todo", "line"),
        "blocked" => mark("cf-m-blocked", "line"),
        "warn" => mark("cf-m-warn", "line"),
        "stop" => mark("cf-m-stop", "bar-v"),
        "limit" => mark("cf-m-limit", "bar-v"),
        "trans" => Mark {
            head: Some("cf-m-trans-head"),
            ..mark("cf-m-trans", "line")
        },
        "return" => Mark {
            head: Some("cf-m-return-head"),
            ..mark("cf-m-return", "line")
        },
        "human" => mark("cf-m-human", "ring"),
        "node" => mark("cf-m-node", "ring"),
        "agent" => mark("cf-m-agent", "disc"),
        "act" => mark("cf-m-act", "disc"),
        "merge" => mark("cf-m-merge", "diamond"),
        "cross" => mark("cf-m-cross", "cross"),
        "layer" => mark("cf-m-layer", "bar"),
        "layer-remote" => mark("cf-m-layer--remote", "bar"),
        "used" => mark("cf-m-used", "bar"),
        "state" => mark("cf-m-state", "box"),
        "optional" => mark("cf-m-optional", "box"),
        "denied" => mark("cf-m-denied", "box"),
        "cov" => mark("cf-m-cov", "cell"),
        "part" => mark("cf-m-part", "cell"),
        "notrun" => Mark {
            hatch: true,
            ..mark("cf-m-notrun", "cell")
        },
        "na" => mark("cf-m-na", "cell"),
        "nc" => Mark {
            cross: Some("cf-m-nc-cross"),
            ..mark("cf-m-nc", "cell")
        },
        _ => return None,
    })
}

fn mark_shapes(name: &str) -> &'static [&'static str] {
    match name {
        "human" | "node" | "agent" | "act" => &["circle"],
        "merge" => &["diamond", "path"],
        "cross" => &["cross"],
        "layer" | "layer-remote" | "used" | "state" | "denied" | "cov" | "part" | "notrun"
        | "na" | "nc" => &["rect"],
        "optional" => &["rect", "path", "line", "polyline"],
        _ => &LINE,
    }
}

type Rendered = Result<String, String>;

/// The figure HTML the module renders for `declaration` with `idPrefix`
/// `id_prefix`, given the derived values of a derived figure.
pub(super) fn render_figure(
    declaration: &Value,
    id_prefix: &str,
    derived: Option<&Map<String, Value>>,
) -> Rendered {
    let figure = declaration
        .get("figure")
        .and_then(Value::as_object)
        .ok_or("the declaration has no figure")?;
    let str_field = |key: &str| figure.get(key).and_then(Value::as_str);
    let figure_id = str_field("id").ok_or("the figure has no id")?;
    let prefix = format!("{}-{figure_id}", sanitize_id(id_prefix));
    let (wide, narrow, drawn_values) = compose(figure, derived)?;
    let states = figure
        .get("states")
        .and_then(Value::as_array)
        .ok_or("the figure has no states")?;
    let state_mark = |name: &str| -> Option<String> {
        states
            .iter()
            .find(|state| state.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|state| state.get("mark").and_then(Value::as_str))
            .map(str::to_string)
    };
    let wide_drawn = drawn_states(&wide.draw);
    let narrow_drawn = drawn_states(&narrow.draw);
    let declared: Vec<&str> = states
        .iter()
        .filter_map(|state| state.get("name").and_then(Value::as_str))
        .collect();
    let unkeyed: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|name| !wide_drawn.contains(*name) && !narrow_drawn.contains(*name))
        .collect();
    if !unkeyed.is_empty() {
        return Err(format!(
            "rule 2: declared states are never drawn: {}",
            unkeyed.join(", ")
        ));
    }
    let dropped: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|name| wide_drawn.contains(*name) && !narrow_drawn.contains(*name))
        .collect();
    let mut drops: Vec<&str> = narrow.drops.iter().filter_map(Value::as_str).collect();
    let position = |name: &str| declared.iter().position(|candidate| *candidate == name);
    drops.sort_by_key(|name| position(name).map_or(-1, |index| index as i64));
    if dropped.join(" ") != drops.join(" ") {
        return Err("rule 5: the narrow composition drops differ from its declaration".into());
    }
    let description = describe(figure);
    let title = str_field("title").ok_or("the figure has no title")?;
    let facts = figure
        .get("facts")
        .and_then(Value::as_array)
        .ok_or("the figure has no facts")?;
    let fact_values: Vec<(String, Value)> = facts
        .iter()
        .map(|fact| {
            (
                fact.get("claim")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                fact.get("value").cloned().unwrap_or(Value::Null),
            )
        })
        .collect();
    let facts_json = Value::Array(
        fact_values
            .iter()
            .map(|(claim, value)| {
                let mut object = Map::new();
                object.insert("claim".into(), Value::String(claim.clone()));
                object.insert("value".into(), value.clone());
                Value::Object(object)
            })
            .collect(),
    );
    let elongation = figure
        .get("narrow")
        .and_then(|narrow| narrow.get("elongation_max"))
        .and_then(Value::as_f64)
        .unwrap_or(ELONGATION_MAX);
    let mut attributes = vec![
        ("class", "cf-fig".to_string()),
        (
            "data-cf-figure",
            str_field("family").unwrap_or_default().to_string(),
        ),
        ("data-cf-figure-id", figure_id.to_string()),
        (
            "data-cf-binding",
            str_field("binding").unwrap_or_default().to_string(),
        ),
        ("data-cf-states", declared.join(" ")),
        ("data-cf-elongation-max", js_number(elongation)),
        ("data-cf-facts", js_stringify(&facts_json)),
    ];
    if let Some(values) = &drawn_values {
        attributes.push((
            "data-cf-values",
            js_stringify(&Value::Object(values.clone())),
        ));
    }
    let attributes = attributes
        .iter()
        .map(|(name, value)| format!("{name}=\"{}\"", escape_attribute(value)))
        .collect::<Vec<_>>()
        .join(" ");
    let svg = |composition: &Composition, variant: &str| -> Rendered {
        let id = format!("{prefix}-{variant}");
        let hatch_id = format!("{id}-hatch");
        let defs = if composition.draw.iter().any(|item| {
            item.get("state")
                .and_then(Value::as_str)
                .and_then(&state_mark)
                .as_deref()
                == Some("notrun")
        }) {
            format!("<defs><pattern id=\"{hatch_id}\" width=\"4.5\" height=\"4.5\" patternUnits=\"userSpaceOnUse\" patternTransform=\"rotate(45)\"><line class=\"cf-m-hatchline\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"4.5\"/></pattern></defs>")
        } else {
            String::new()
        };
        let mut body = String::new();
        for item in &composition.draw {
            body.push_str(&draw_item(item, &state_mark, &hatch_id, &format!("{id}-"))?);
        }
        Ok(format!(
            "<svg class=\"cf-fig-svg cf-fig-svg--{variant}\" viewBox=\"0 0 {} {}\" role=\"img\" aria-labelledby=\"{id}-t {id}-d\" data-cf-variant=\"{variant}\"><title id=\"{id}-t\">{}</title><desc id=\"{id}-d\">{}</desc>{defs}{body}</svg>",
            num(composition.width),
            num(composition.height),
            escape_text(title),
            escape_text(&description)
        ))
    };
    let mut legend = String::new();
    for name in &declared {
        let mark_name = state_mark(name).unwrap_or_default();
        let means = states
            .iter()
            .find(|state| state.get("name").and_then(Value::as_str) == Some(*name))
            .and_then(|state| state.get("means").and_then(Value::as_str))
            .unwrap_or_default();
        let wide_only = wide_drawn.contains(*name) && !narrow_drawn.contains(*name);
        let sample = wide
            .draw
            .iter()
            .chain(&narrow.draw)
            .find(|item| item.get("state").and_then(Value::as_str) == Some(*name));
        let _ = write!(
            legend,
            "<li data-state=\"{}\"{}>{}{}</li>",
            escape_attribute(name),
            if wide_only { " data-cf-wide" } else { "" },
            legend_key(&mark_name, sample, &format!("{prefix}-key-{name}"))?,
            escape_text(means)
        );
    }
    let kicker = str_field("kicker").unwrap_or(title);
    Ok(format!(
        "<figure {attributes}><span class=\"cf-fig-kicker\">Figure \u{b7} {}</span>{}{}<ul class=\"cf-legend\" aria-label=\"Legend\">{legend}</ul><figcaption class=\"cf-fig-caption\">{}</figcaption>{}</figure>",
        escape_text(kicker),
        svg(&wide, "wide")?,
        svg(&narrow, "narrow")?,
        escape_text(str_field("caption").unwrap_or_default()),
        twin_table(figure, &fact_values)?
    ))
}

struct Composition {
    width: f64,
    height: f64,
    draw: Vec<Value>,
    drops: Vec<Value>,
}

fn drawn_states(draw: &[Value]) -> BTreeSet<String> {
    draw.iter()
        .filter_map(|item| item.get("state").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn composition(value: Option<&Value>) -> Result<Composition, String> {
    let value = value.ok_or("a composition is missing")?;
    Ok(Composition {
        width: number(value, "width")?,
        height: number(value, "height")?,
        draw: value
            .get("draw")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        drops: value
            .get("drops")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    })
}

type Composed = (Composition, Composition, Option<Map<String, Value>>);

fn compose(
    figure: &Map<String, Value>,
    derived: Option<&Map<String, Value>>,
) -> Result<Composed, String> {
    let Some(layout) = figure.get("layout") else {
        return Ok((
            composition(figure.get("wide"))?,
            composition(figure.get("narrow"))?,
            None,
        ));
    };
    if figure.get("binding").and_then(Value::as_str) == Some("derived") && derived.is_none() {
        return Err("a derived figure needs its bound data".into());
    }
    let empty = Map::new();
    let derived = derived.unwrap_or(&empty);
    let drops = figure
        .get("narrow")
        .and_then(|narrow| narrow.get("drops"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let (wide, mut narrow, values) = match layout.get("kind").and_then(Value::as_str) {
        Some("extent") => extent_layout(layout, derived)?,
        Some("coverage") => coverage_layout(layout)?,
        _ => return Err("the layout kind is not extent or coverage".into()),
    };
    narrow.drops = drops;
    Ok((wide, narrow, values))
}

fn extent_layout(layout: &Value, derived: &Map<String, Value>) -> Result<Composed, String> {
    let value_of = |entry: &Value| -> Result<f64, String> {
        match entry.get("value") {
            Some(Value::String(selector)) => derived
                .get(selector)
                .and_then(Value::as_f64)
                .ok_or_else(|| format!("derived value {selector} is missing")),
            Some(value) => value
                .as_f64()
                .ok_or_else(|| "a layout value is not a number".into()),
            None => Err("a layout entry has no value".into()),
        }
    };
    let list = |key: &str| {
        layout
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let rows: Vec<(Value, f64)> = list("rows")
        .into_iter()
        .map(|row| value_of(&row).map(|number| (row, number)))
        .collect::<Result<_, _>>()?;
    let limits: Vec<(Value, f64)> = list("limits")
        .into_iter()
        .map(|limit| value_of(&limit).map(|number| (limit, number)))
        .collect::<Result<_, _>>()?;
    let unit = layout
        .get("unit")
        .and_then(Value::as_str)
        .filter(|unit| !unit.is_empty())
        .map_or(String::new(), |unit| format!(" {unit}"));
    let max = number(layout, "max")?;
    let mut drawn_values = Map::new();
    let mut compose = |width: f64, narrow: bool| -> Composition {
        let label_width = if narrow { 0.0 } else { 150.0 };
        let left = label_width;
        let right = width - 40.0;
        let scale = |value: f64| left + (value / max) * (right - left);
        let invert = |position: f64| {
            if right == left {
                0.0
            } else {
                ((position - left) / (right - left)) * max
            }
        };
        let mut draw = Vec::new();
        let row_height = if narrow { 58.0 } else { 40.0 };
        let top = 30.0;
        for (index, (row, number)) in rows.iter().enumerate() {
            let y = top + index as f64 * row_height + if narrow { 22.0 } else { 0.0 };
            let label_y = if narrow { y - 8.0 } else { y + 10.0 };
            let length = scale(*number) - left;
            let id = format!("row-{index}");
            let label = row.get("label").and_then(Value::as_str).unwrap_or_default();
            draw.push(serde_json::json!({ "text": label, "x": if narrow { left } else { 0.0 }, "y": label_y, "style": "strong", "for": [id] }));
            draw.push(serde_json::json!({ "state": row.get("state").cloned().unwrap_or(Value::Null), "shape": "rect", "x": left, "y": y, "w": round(length, 0.01), "h": 12, "rx": 3, "id": id, "value": number }));
            if let Some(selector) = row.get("value").and_then(Value::as_str) {
                drawn_values.insert(
                    selector.to_string(),
                    json_number(round(invert(left + round(length, 0.01)), 0.0001)),
                );
            }
            let value_text = format!("{}{unit}", format_number(*number));
            let value_width = value_text.encode_utf16().count() as f64 * 8.5;
            let mut value_x = left + length + LABEL_GAP;
            let mut limit_xs: Vec<f64> = limits.iter().map(|(_, number)| scale(*number)).collect();
            limit_xs.sort_by(f64::total_cmp);
            for limit_x in limit_xs {
                if limit_x > value_x - LABEL_GAP && limit_x < value_x + value_width + LABEL_GAP {
                    value_x = limit_x + LABEL_GAP;
                }
            }
            draw.push(serde_json::json!({ "text": value_text, "x": round(value_x, 0.01), "y": y + 11.0, "style": ["mono", "mute"], "for": [id] }));
        }
        let axis_y = top + rows.len() as f64 * row_height + if narrow { 22.0 } else { 4.0 };
        draw.push(serde_json::json!({ "deco": "axis", "shape": "line", "x1": left, "y1": axis_y, "x2": right, "y2": axis_y }));
        for (index, (limit, number)) in limits.iter().enumerate() {
            let x = round(scale(*number), 0.01);
            let id = format!("limit-{index}");
            draw.push(serde_json::json!({ "state": limit.get("state").cloned().unwrap_or(Value::Null), "shape": "line", "x1": x, "y1": top - 6.0, "x2": x, "y2": axis_y, "id": id, "value": number }));
            if let Some(selector) = limit.get("value").and_then(Value::as_str) {
                drawn_values.insert(selector.to_string(), json_number(round(invert(x), 0.0001)));
            }
            let label = limit
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or_default();
            draw.push(serde_json::json!({ "text": format!("{label} {}", format_number(*number)), "x": x, "y": axis_y + 22.0 + if narrow { index as f64 * 20.0 } else { 0.0 }, "anchor": if narrow { "end" } else { "middle" }, "style": "mute", "for": [id] }));
        }
        let height = axis_y
            + 30.0
            + if narrow {
                (limits.len().max(1) - 1) as f64 * 20.0
            } else {
                0.0
            };
        Composition {
            width,
            height,
            draw,
            drops: Vec::new(),
        }
    };
    let wide = compose(WIDE_MAX_WIDTH, false);
    let narrow = compose(360.0, true);
    Ok((wide, narrow, Some(drawn_values)))
}

fn coverage_layout(layout: &Value) -> Result<Composed, String> {
    let columns: Vec<String> = layout
        .get("columns")
        .and_then(Value::as_array)
        .ok_or("a coverage layout has no columns")?
        .iter()
        .map(|column| column.as_str().unwrap_or_default().to_string())
        .collect();
    let rows = layout
        .get("rows")
        .and_then(Value::as_array)
        .ok_or("a coverage layout has no rows")?;
    let cell = 18.0;
    let label_width = 240.0;
    let col_step = f64::min(80.0, (WIDE_MAX_WIDTH - label_width) / columns.len() as f64);
    let mut wide = Vec::new();
    for (index, column) in columns.iter().enumerate() {
        wide.push(serde_json::json!({ "text": column, "x": round(label_width + col_step * index as f64 + col_step / 2.0, 0.01), "y": 24, "anchor": "middle", "style": "head" }));
    }
    let cells = |row: &Value| {
        row.get("cells")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let label = |row: &Value| row.get("label").cloned().unwrap_or(Value::Null);
    for (row_index, row) in rows.iter().enumerate() {
        let y = 48.0 + row_index as f64 * 36.0;
        wide.push(serde_json::json!({ "text": label(row), "x": 0, "y": y + 14.0 }));
        for (column_index, state) in cells(row).into_iter().enumerate() {
            wide.push(serde_json::json!({ "state": state, "shape": "rect", "x": round(label_width + col_step * column_index as f64 + col_step / 2.0 - cell / 2.0, 0.01), "y": y, "w": cell, "h": cell, "rx": 2 }));
        }
    }
    let mut narrow = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        let y = 20.0 + row_index as f64 * 64.0;
        narrow.push(serde_json::json!({ "text": label(row), "x": 0, "y": y, "style": "strong" }));
        let step = f64::min(88.0, 360.0 / columns.len() as f64);
        for (column_index, state) in cells(row).into_iter().enumerate() {
            let x = column_index as f64 * step;
            narrow.push(serde_json::json!({ "state": state, "shape": "rect", "x": x, "y": y + 14.0, "w": 16, "h": 16, "rx": 2 }));
            narrow.push(serde_json::json!({ "text": columns.get(column_index).cloned().unwrap_or_default(), "x": x + 22.0, "y": y + 27.0, "style": "mute" }));
        }
    }
    Ok((
        Composition {
            width: WIDE_MAX_WIDTH,
            height: 48.0 + rows.len() as f64 * 36.0 + 4.0,
            draw: wide,
            drops: Vec::new(),
        },
        Composition {
            width: 360.0,
            height: 20.0 + rows.len() as f64 * 64.0,
            draw: narrow,
            drops: Vec::new(),
        },
        None,
    ))
}

/// The accessible description: the declared one or the idea, then the key
/// and the facts in words.
fn describe(figure: &Map<String, Value>) -> String {
    let text = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let lead = figure
        .get("description")
        .and_then(Value::as_str)
        .map_or_else(|| text(figure.get("idea")), str::to_string);
    let list = |key: &str| {
        figure
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let key = list("states")
        .iter()
        .map(|state| format!("{} ({})", text(state.get("means")), text(state.get("name"))))
        .collect::<Vec<_>>()
        .join("; ");
    let facts = list("facts")
        .iter()
        .map(|fact| text(fact.get("claim")))
        .collect::<Vec<_>>()
        .join("; ");
    format!("{lead} Key: {key}. Facts: {facts}.")
}

fn draw_item(
    item: &Value,
    state_mark: &dyn Fn(&str) -> Option<String>,
    hatch_id: &str,
    id_prefix: &str,
) -> Rendered {
    if let Some(text) = item.get("text") {
        let styles: Vec<&str> = match item.get("style") {
            None => Vec::new(),
            Some(Value::Array(styles)) => styles.iter().filter_map(Value::as_str).collect(),
            Some(style) => style.as_str().into_iter().collect(),
        };
        let mut classes = vec!["cf-t"];
        for style in styles {
            classes.push(match style {
                "strong" => "cf-t--strong",
                "mute" => "cf-t--mute",
                "head" => "cf-t--head",
                "mono" => "cf-t--mono",
                _ => return Err(format!("unknown text style {style}")),
            });
        }
        let anchor = item
            .get("anchor")
            .and_then(Value::as_str)
            .filter(|anchor| !anchor.is_empty() && *anchor != "start")
            .map_or(String::new(), |anchor| format!(" text-anchor=\"{anchor}\""));
        let labels = item
            .get("for")
            .and_then(Value::as_array)
            .filter(|ids| !ids.is_empty())
            .map_or(String::new(), |ids| {
                let joined = ids
                    .iter()
                    .map(|id| format!("{id_prefix}{}", id.as_str().unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(" data-cf-for=\"{}\"", escape_attribute(&joined))
            });
        return Ok(format!(
            "<text class=\"{}\" x=\"{}\" y=\"{}\"{anchor}{labels}>{}</text>",
            classes.join(" "),
            num(number(item, "x")?),
            num(number(item, "y")?),
            escape_text(&js_string(text))
        ));
    }
    if let Some(deco) = item.get("deco").and_then(Value::as_str) {
        let class = match deco {
            "rule" => "cf-f-rule",
            "axis" => "cf-f-axis",
            "tick" => "cf-f-tick",
            _ => return Err(format!("unknown decoration {deco}")),
        };
        return shape_element(item, class, "");
    }
    let state = item
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mark_name = state_mark(state).ok_or_else(|| format!("state {state} is not declared"))?;
    let mark = mark_for(&mark_name).ok_or_else(|| format!("unknown mark {mark_name}"))?;
    let shape = item
        .get("shape")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !mark_shapes(&mark_name).contains(&shape) {
        return Err(format!(
            "state {state} draws the {mark_name} mark, not {shape}"
        ));
    }
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .map_or(String::new(), |id| {
            format!(" id=\"{}\"", escape_attribute(&format!("{id_prefix}{id}")))
        });
    let extra = if mark.hatch {
        format!(" fill=\"url(#{hatch_id})\"")
    } else {
        String::new()
    };
    let value = item.get("value").map_or(String::new(), |value| {
        format!(" data-cf-value=\"{}\"", escape_attribute(&js_string(value)))
    });
    let primary = shape_element(item, mark.class, &extra)?;
    let parts = mark_parts(item, &mark_name, &mark)?;
    Ok(format!(
        "<g data-state=\"{}\"{id}{value}>{primary}{parts}</g>",
        escape_attribute(state)
    ))
}

fn shape_element(item: &Value, class: &str, extra: &str) -> Rendered {
    let cls = format!("class=\"{class}\"");
    let n = |key: &str| number(item, key);
    Ok(
        match item
            .get("shape")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "path" => format!(
                "<path {cls} d=\"{}\"{extra}/>",
                normalize_path(item.get("d").and_then(Value::as_str).unwrap_or_default())
            ),
            "line" => format!(
                "<line {cls} x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{extra}/>",
                num(n("x1")?),
                num(n("y1")?),
                num(n("x2")?),
                num(n("y2")?)
            ),
            "polyline" => format!(
                "<polyline {cls} points=\"{}\"{extra}/>",
                points(item)?
                    .iter()
                    .map(|(x, y)| format!("{},{}", num(*x), num(*y)))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            "rect" => format!(
                "<rect {cls} x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{extra}/>",
                num(n("x")?),
                num(n("y")?),
                num(n("w")?),
                num(n("h")?),
                item.get("rx").map_or(Ok(String::new()), |_| n("rx")
                    .map(|rx| format!(" rx=\"{}\"", num(rx))))?
            ),
            "circle" => format!(
                "<circle {cls} cx=\"{}\" cy=\"{}\" r=\"{}\"{extra}/>",
                num(n("cx")?),
                num(n("cy")?),
                num(n("r")?)
            ),
            "diamond" => {
                let (cx, cy, r) = (n("cx")?, n("cy")?, n("r")?);
                format!(
                    "<path {cls} d=\"M{} {}L{} {}L{} {}L{} {}Z\"{extra}/>",
                    num(cx),
                    num(cy - r),
                    num(cx + r),
                    num(cy),
                    num(cx),
                    num(cy + r),
                    num(cx - r),
                    num(cy)
                )
            }
            "cross" => format!(
                "<path {cls} d=\"{}\"{extra}/>",
                cross_path(n("cx")?, n("cy")?, n("size")?)
            ),
            other => return Err(format!("unknown shape {other}")),
        },
    )
}

fn points(item: &Value) -> Result<Vec<(f64, f64)>, String> {
    item.get("points")
        .and_then(Value::as_array)
        .ok_or("a polyline has no points")?
        .iter()
        .map(|point| {
            let pair = point
                .as_array()
                .filter(|pair| pair.len() == 2)
                .ok_or("a point is not a pair")?;
            match (pair[0].as_f64(), pair[1].as_f64()) {
                (Some(x), Some(y)) => Ok((x, y)),
                _ => Err("a point is not numeric".into()),
            }
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy)]
struct Terminal {
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
}

fn ends(value: Option<&Value>) -> Vec<&'static str> {
    match value.and_then(Value::as_str) {
        Some("both") => vec!["start", "end"],
        Some("start") => vec!["start"],
        Some("end") => vec!["end"],
        _ => Vec::new(),
    }
}

fn mark_parts(item: &Value, mark_name: &str, mark: &Mark) -> Rendered {
    let mut parts = String::new();
    let shape = item
        .get("shape")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let terminals = if LINE.contains(&shape) {
        Some(line_terminals(item)?)
    } else {
        None
    };
    let terminal = |end: &str| -> Result<Terminal, String> {
        let (start, stop) =
            terminals.ok_or_else(|| format!("a part needs a line mark, not {shape}"))?;
        Ok(if end == "start" { start } else { stop })
    };
    for end in ends(item.get("head")) {
        let _ = write!(
            parts,
            "<path class=\"{}\" d=\"{}\"/>",
            mark.head.unwrap_or("cf-m-trans-head"),
            head_path(terminal(end)?, 11.0, 5.5)
        );
    }
    for end in ends(item.get("open")) {
        let _ = write!(
            parts,
            "<path class=\"cf-m-state\" d=\"{}\"/>",
            head_path(terminal(end)?, 11.0, 5.5)
        );
    }
    for end in ends(item.get("square")) {
        let Terminal { x, y, .. } = terminal(end)?;
        let _ = write!(
            parts,
            "<rect class=\"cf-m-state\" x=\"{}\" y=\"{}\" width=\"10\" height=\"10\"/>",
            num(x - 5.0),
            num(y - 5.0)
        );
    }
    if item.get("cap").and_then(Value::as_str) == Some("end") {
        if shape != "rect" {
            return Err("a cap closes a bar".into());
        }
        let (x, y, w, h) = (
            number(item, "x")?,
            number(item, "y")?,
            number(item, "w")?,
            number(item, "h")?,
        );
        let rx = item
            .get("rx")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .min(h / 2.0)
            .min(6.0);
        let start = x + w - 14.0;
        let _ = write!(
            parts,
            "<path class=\"cf-m-cap\" d=\"M{} {}H{}Q{} {} {} {}V{}Q{} {} {} {}H{}Z\"/>",
            num(start),
            num(y),
            num(x + w - rx),
            num(x + w),
            num(y),
            num(x + w),
            num(y + rx),
            num(y + h - rx),
            num(x + w),
            num(y + h),
            num(x + w - rx),
            num(y + h),
            num(start)
        );
    }
    if item.get("cross").is_some() || mark.cross.is_some() {
        let (cx, cy, size) = match item.get("cross") {
            Some(Value::Object(spec)) => (
                spec.get("cx").and_then(Value::as_f64).ok_or("cross cx")?,
                spec.get("cy").and_then(Value::as_f64).ok_or("cross cy")?,
                spec.get("size")
                    .and_then(Value::as_f64)
                    .ok_or("cross size")?,
            ),
            _ => default_cross(item, mark_name)?,
        };
        let _ = write!(
            parts,
            "<path class=\"{}\" d=\"{}\"/>",
            mark.cross.unwrap_or("cf-m-cross"),
            cross_path(cx, cy, size)
        );
    }
    if item.get("tick") == Some(&Value::Bool(true)) {
        if shape != "circle" {
            return Err("a tick sits inside a ring".into());
        }
        let (cx, cy) = (number(item, "cx")?, number(item, "cy")?);
        let _ = write!(
            parts,
            "<path class=\"cf-m-done\" d=\"M{} {}L{} {}L{} {}\"/>",
            num(cx - 6.0),
            num(cy),
            num(cx - 2.0),
            num(cy + 5.0),
            num(cx + 7.0),
            num(cy - 5.0)
        );
    }
    Ok(parts)
}

fn default_cross(item: &Value, mark_name: &str) -> Result<(f64, f64, f64), String> {
    let shape = item
        .get("shape")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if shape == "rect" {
        let (x, y, w, h) = (
            number(item, "x")?,
            number(item, "y")?,
            number(item, "w")?,
            number(item, "h")?,
        );
        let size = if mark_name == "nc" {
            w.min(h) * 0.88
        } else {
            12.0
        };
        return Ok((x + w / 2.0, y + h / 2.0, size));
    }
    if LINE.contains(&shape) {
        let points = path_points(item)?;
        let half = points.len() / 2;
        let middle = points.get(half).copied().unwrap_or(points[0]);
        let previous = points[half.saturating_sub(1)];
        return Ok((
            (middle.x + previous.x) / 2.0,
            (middle.y + previous.y) / 2.0,
            12.0,
        ));
    }
    if shape == "circle" {
        return Ok((number(item, "cx")?, number(item, "cy")?, 12.0));
    }
    Err(format!("no default cross for {shape}"))
}

fn cross_path(cx: f64, cy: f64, size: f64) -> String {
    let half = size / 2.0;
    format!(
        "M{} {}L{} {}M{} {}L{} {}",
        num(cx - half),
        num(cy - half),
        num(cx + half),
        num(cy + half),
        num(cx + half),
        num(cy - half),
        num(cx - half),
        num(cy + half)
    )
}

fn head_path(terminal: Terminal, length: f64, half: f64) -> String {
    let Terminal { x, y, dx, dy } = terminal;
    let hypot = js_hypot(dx, dy);
    let norm = if hypot == 0.0 || hypot.is_nan() {
        1.0
    } else {
        hypot
    };
    let ux = dx / norm;
    let uy = dy / norm;
    let bx = x - ux * length;
    let by = y - uy * length;
    format!(
        "M{} {}L{} {}L{} {}Z",
        num(x),
        num(y),
        num(bx - uy * half),
        num(by + ux * half),
        num(bx + uy * half),
        num(by - ux * half)
    )
}

/// `Math.hypot` for two finite arguments: V8 scales by the larger magnitude
/// and sums with a compensated addition.
fn js_hypot(a: f64, b: f64) -> f64 {
    let (a, b) = (a.abs(), b.abs());
    let max = a.max(b);
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    let mut compensation = 0.0_f64;
    for value in [a, b] {
        let ratio = value / max;
        let summand = ratio * ratio - compensation;
        let prelim = sum + summand;
        compensation = (prelim - sum) - summand;
        sum = prelim;
    }
    sum.sqrt() * max
}

fn line_terminals(item: &Value) -> Result<(Terminal, Terminal), String> {
    match item
        .get("shape")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "line" => {
            let (x1, y1, x2, y2) = (
                number(item, "x1")?,
                number(item, "y1")?,
                number(item, "x2")?,
                number(item, "y2")?,
            );
            let (dx, dy) = (x2 - x1, y2 - y1);
            Ok((
                Terminal {
                    x: x1,
                    y: y1,
                    dx: -dx,
                    dy: -dy,
                },
                Terminal {
                    x: x2,
                    y: y2,
                    dx,
                    dy,
                },
            ))
        }
        "polyline" => {
            let points = points(item)?;
            if points.len() < 2 {
                return Err("a polyline needs two points".into());
            }
            let (a, b) = (points[0], points[1]);
            let (c, d) = (points[points.len() - 2], points[points.len() - 1]);
            Ok((
                Terminal {
                    x: a.0,
                    y: a.1,
                    dx: a.0 - b.0,
                    dy: a.1 - b.1,
                },
                Terminal {
                    x: d.0,
                    y: d.1,
                    dx: d.0 - c.0,
                    dy: d.1 - c.1,
                },
            ))
        }
        _ => {
            let segments = parse_path(item.get("d").and_then(Value::as_str).unwrap_or_default())?;
            let first = segments[0];
            let last = segments[segments.len() - 1];
            Ok((
                Terminal {
                    x: first.from.x,
                    y: first.from.y,
                    dx: first.from.x - first.start_control.x,
                    dy: first.from.y - first.start_control.y,
                },
                Terminal {
                    x: last.to.x,
                    y: last.to.y,
                    dx: last.to.x - last.end_control.x,
                    dy: last.to.y - last.end_control.y,
                },
            ))
        }
    }
}

fn path_points(item: &Value) -> Result<Vec<Point>, String> {
    match item
        .get("shape")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "line" => Ok(vec![
            Point {
                x: number(item, "x1")?,
                y: number(item, "y1")?,
            },
            Point {
                x: number(item, "x2")?,
                y: number(item, "y2")?,
            },
        ]),
        "polyline" => Ok(points(item)?
            .into_iter()
            .map(|(x, y)| Point { x, y })
            .collect()),
        _ => {
            let segments = parse_path(item.get("d").and_then(Value::as_str).unwrap_or_default())?;
            let mut points = vec![segments[0].from];
            points.extend(segments.iter().map(|segment| segment.to));
            Ok(points)
        }
    }
}

#[derive(Clone, Copy)]
struct Segment {
    from: Point,
    to: Point,
    start_control: Point,
    end_control: Point,
}

/// `parsePath`: absolute M, L, H, V, C, Q and Z only.
fn parse_path(d: &str) -> Result<Vec<Segment>, String> {
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)[MLHVCQZ]|-?[0-9]*\.?[0-9]+(?:e-?[0-9]+)?").expect("path token pattern")
    });
    let tokens: Vec<&str> = TOKEN.find_iter(d).map(|found| found.as_str()).collect();
    let mut segments = Vec::new();
    let mut index = 0;
    let mut command: Option<char> = None;
    let mut current = Point { x: 0.0, y: 0.0 };
    let mut start = current;
    let is_command = |token: &str| token.len() == 1 && "MLHVCQZmlhvcqz".contains(token);
    let number = |index: &mut usize| -> Result<f64, String> {
        let value = tokens
            .get(*index)
            .map_or(f64::NAN, |token| js_to_number(token));
        *index += 1;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(format!("invalid path data: {d}"))
        }
    };
    while index < tokens.len() {
        if is_command(tokens[index]) {
            command = tokens[index].chars().next().map(|c| c.to_ascii_uppercase());
            index += 1;
        }
        let Some(active) = command else {
            return Err(format!("path data must start with a command: {d}"));
        };
        let from = current;
        match active {
            'M' => {
                current = Point {
                    x: number(&mut index)?,
                    y: number(&mut index)?,
                };
                start = current;
                command = Some('L');
                continue;
            }
            'Z' => {
                if current != start {
                    segments.push(Segment {
                        from,
                        to: start,
                        start_control: start,
                        end_control: from,
                    });
                }
                current = start;
                command = None;
                continue;
            }
            _ => {}
        }
        let mut start_control = None;
        let mut end_control = None;
        match active {
            'L' => {
                current = Point {
                    x: number(&mut index)?,
                    y: number(&mut index)?,
                }
            }
            'H' => {
                current = Point {
                    x: number(&mut index)?,
                    y: current.y,
                }
            }
            'V' => {
                current = Point {
                    x: current.x,
                    y: number(&mut index)?,
                }
            }
            'C' => {
                let c1 = Point {
                    x: number(&mut index)?,
                    y: number(&mut index)?,
                };
                let c2 = Point {
                    x: number(&mut index)?,
                    y: number(&mut index)?,
                };
                current = Point {
                    x: number(&mut index)?,
                    y: number(&mut index)?,
                };
                start_control = Some(if c1 == from { c2 } else { c1 });
                end_control = Some(if c2 == current { c1 } else { c2 });
            }
            'Q' => {
                let c = Point {
                    x: number(&mut index)?,
                    y: number(&mut index)?,
                };
                current = Point {
                    x: number(&mut index)?,
                    y: number(&mut index)?,
                };
                start_control = Some(c);
                end_control = Some(c);
            }
            other => return Err(format!("unsupported path command {other}")),
        }
        segments.push(Segment {
            from,
            to: current,
            start_control: start_control.unwrap_or(current),
            end_control: end_control.unwrap_or(from),
        });
    }
    if segments.is_empty() {
        return Err(format!("path draws nothing: {d}"));
    }
    Ok(segments)
}

fn legend_key(mark_name: &str, sample: Option<&Value>, id: &str) -> Rendered {
    let mark = mark_for(mark_name).ok_or_else(|| format!("no legend key for {mark_name}"))?;
    let class = mark.class;
    let svg = |inner: String| {
        format!("<svg class=\"cf-key\" viewBox=\"0 0 28 16\" aria-hidden=\"true\">{inner}</svg>")
    };
    let has = |part: &str| sample.is_some_and(|sample| sample.get(part).is_some());
    let arrow = Terminal {
        x: 26.0,
        y: 8.0,
        dx: 1.0,
        dy: 0.0,
    };
    Ok(match mark.key {
        "line" => {
            let head = if has("head") {
                format!(
                    "<path class=\"{}\" d=\"{}\"/>",
                    mark.head.unwrap_or("cf-m-trans-head"),
                    head_path(arrow, 9.0, 4.5)
                )
            } else {
                String::new()
            };
            let open = if has("open") {
                format!(
                    "<path class=\"cf-m-state\" d=\"{}\"/>",
                    head_path(arrow, 9.0, 4.5)
                )
            } else {
                String::new()
            };
            let square = if has("square") {
                "<rect class=\"cf-m-state\" x=\"19\" y=\"4\" width=\"8\" height=\"8\"/>".to_string()
            } else {
                String::new()
            };
            let cross = if has("cross") {
                format!(
                    "<path class=\"cf-m-cross\" d=\"{}\"/>",
                    cross_path(14.0, 8.0, 9.0)
                )
            } else {
                String::new()
            };
            let stop = if head.is_empty() && open.is_empty() && square.is_empty() {
                25
            } else {
                20
            };
            svg(format!(
                "<path class=\"{class}\" d=\"M3 8H{stop}\"/>{head}{open}{square}{cross}"
            ))
        }
        "bar-v" => svg(format!(
            "<line class=\"{class}\" x1=\"14\" y1=\"1\" x2=\"14\" y2=\"15\"/>"
        )),
        "ring" => svg(format!(
            "<circle class=\"{class}\" cx=\"14\" cy=\"8\" r=\"6\"/>{}",
            if has("tick") {
                "<path class=\"cf-m-done\" d=\"M10.5 8L13 11L18 5\"/>"
            } else {
                ""
            }
        )),
        "disc" => svg(format!(
            "<circle class=\"{class}\" cx=\"14\" cy=\"8\" r=\"6.5\"/>"
        )),
        "diamond" => svg(format!(
            "<path class=\"{class}\" d=\"M14 1.5L20.5 8L14 14.5L7.5 8Z\"/>"
        )),
        "cross" => svg(format!(
            "<path class=\"{class}\" d=\"{}\"/>",
            cross_path(14.0, 8.0, 10.0)
        )),
        "bar" => svg(format!(
            "<rect class=\"{class}\" x=\"2\" y=\"3\" width=\"24\" height=\"10\" rx=\"3\"/>{}{}",
            if has("cap") {
                "<path class=\"cf-m-cap\" d=\"M19 3H23Q26 3 26 6V10Q26 13 23 13H19Z\"/>"
            } else {
                ""
            },
            if has("cross") {
                format!(
                    "<path class=\"cf-m-cross\" d=\"{}\"/>",
                    cross_path(14.0, 8.0, 9.0)
                )
            } else {
                String::new()
            }
        )),
        "box" => svg(format!(
            "<rect class=\"{class}\" x=\"2\" y=\"2\" width=\"24\" height=\"12\" rx=\"3\"/>{}",
            if mark_name == "denied" || has("cross") {
                format!(
                    "<path class=\"cf-m-cross\" d=\"{}\"/>",
                    cross_path(14.0, 8.0, 9.0)
                )
            } else {
                String::new()
            }
        )),
        "cell" => {
            let hatch = if mark.hatch {
                format!("<defs><pattern id=\"{id}\" width=\"4.5\" height=\"4.5\" patternUnits=\"userSpaceOnUse\" patternTransform=\"rotate(45)\"><line class=\"cf-m-hatchline\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"4.5\"/></pattern></defs>")
            } else {
                String::new()
            };
            let fill = if mark.hatch {
                format!(" fill=\"url(#{id})\"")
            } else {
                String::new()
            };
            let cross = mark.cross.map_or(String::new(), |cross| {
                format!(
                    "<path class=\"{cross}\" d=\"{}\"/>",
                    cross_path(14.0, 8.0, 10.5)
                )
            });
            svg(format!("{hatch}<rect class=\"{class}\" x=\"8\" y=\"2\" width=\"12\" height=\"12\" rx=\"2\"{fill}/>{cross}"))
        }
        other => return Err(format!("no legend key form {other}")),
    })
}

fn twin_table(figure: &Map<String, Value>, fact_values: &[(String, Value)]) -> Rendered {
    let (columns, rows): (Vec<String>, Vec<Vec<String>>) = match figure.get("twin") {
        Some(Value::String(twin)) if twin == "facts" => (
            vec!["Fact".into(), "Source".into(), "Value".into()],
            figure
                .get("facts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .zip(fact_values)
                .map(|(fact, (_, value))| {
                    vec![
                        fact.get("claim")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        format!(
                            "`{}`",
                            fact.get("source")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                        ),
                        value
                            .as_str()
                            .map_or_else(|| js_stringify(value), str::to_string),
                    ]
                })
                .collect(),
        ),
        Some(Value::Object(twin)) => {
            let strings = |value: &Value| -> Vec<String> {
                value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(js_string)
                    .collect()
            };
            (
                twin.get("columns").map(strings).unwrap_or_default(),
                twin.get("rows")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(strings)
                    .collect(),
            )
        }
        _ => return Err("rule 12: the twin is neither facts nor a table".into()),
    };
    let head: String = columns
        .iter()
        .map(|column| format!("<th scope=\"col\">{}</th>", inline_text(column)))
        .collect();
    let body: String = rows
        .iter()
        .map(|row| {
            format!(
                "<tr>{}</tr>",
                row.iter()
                    .map(|cell| format!("<td>{}</td>", inline_text(cell)))
                    .collect::<String>()
            )
        })
        .collect();
    Ok(format!("<details class=\"cf-twin\"><summary>Table twin</summary><div class=\"cf-twin-scroll\"><table><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table></div></details>"))
}

/// Backticks mark code in a twin cell; everything else is text. The parts
/// are what `String.split` with a capturing pattern yields.
fn inline_text(value: &str) -> String {
    static CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`]+`").expect("code pattern"));
    let mut parts = Vec::new();
    let mut cursor = 0;
    for found in CODE.find_iter(value) {
        parts.push(&value[cursor..found.start()]);
        parts.push(found.as_str());
        cursor = found.end();
    }
    parts.push(&value[cursor..]);
    parts
        .into_iter()
        .map(|part| {
            if part.starts_with('`') && part.ends_with('`') && part.encode_utf16().count() > 2 {
                format!("<code>{}</code>", escape_text(&part[1..part.len() - 1]))
            } else {
                escape_text(part)
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// JavaScript semantics the module relies on
// ---------------------------------------------------------------------------

fn number(value: &Value, key: &str) -> Result<f64, String> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite())
        .ok_or_else(|| format!("{key} is not a number"))
}

/// `String(value)` of a JSON value the module interpolates into a template.
fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number
            .as_f64()
            .map_or_else(|| number.to_string(), js_number),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".into(),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                if item.is_null() {
                    String::new()
                } else {
                    js_string(item)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

fn json_number(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

/// `Number(token)` for a path token the pattern matched.
fn js_to_number(token: &str) -> f64 {
    token.parse::<f64>().unwrap_or(f64::NAN)
}

/// `Math.round`: the nearest integer, ties toward positive infinity.
fn js_round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `num`: two decimals, printed as JavaScript prints a number.
fn num(value: f64) -> String {
    js_number(js_round(value * 100.0) / 100.0)
}

/// `formatNumber`: integers as they are, other values to two decimals.
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.is_finite() {
        js_number(value)
    } else {
        js_number(js_round(value * 100.0) / 100.0)
    }
}

/// `round`: to the step, then through `toFixed(6)` back to a number.
fn round(value: f64, step: f64) -> f64 {
    to_fixed_6(js_round(value / step) * step)
}

/// `Number(x.toFixed(6))`: `toFixed` picks the nearest six-decimal value of
/// the exact binary number, and the larger one on a tie.
fn to_fixed_6(value: f64) -> f64 {
    if !value.is_finite() || value.abs() >= 1e21 {
        return value;
    }
    let exact = format!("{:.1100}", value.abs());
    let (whole, fraction) = exact.split_once('.').unwrap_or((&exact, ""));
    let kept = &fraction[..6];
    let rest = &fraction[6..];
    let mut digits: Vec<u8> = format!("{whole}{kept}").into_bytes();
    let round_up = match rest.as_bytes().first() {
        Some(b'5'..=b'9') => {
            // Exactly half rounds up as well: toFixed takes the larger n.
            true
        }
        _ => false,
    };
    if round_up {
        let mut index = digits.len();
        loop {
            if index == 0 {
                digits.insert(0, b'1');
                break;
            }
            index -= 1;
            if digits[index] == b'9' {
                digits[index] = b'0';
            } else {
                digits[index] += 1;
                break;
            }
        }
    }
    let text = String::from_utf8(digits).unwrap_or_default();
    let split = text.len() - 6;
    let magnitude: f64 = format!("{}.{}", &text[..split], &text[split..])
        .parse()
        .unwrap_or(0.0);
    if value.is_sign_negative() {
        -magnitude
    } else {
        magnitude
    }
}

/// `String(number)`: the shortest round-trip digits, in JavaScript's layout.
pub(super) fn js_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if value == 0.0 {
        return "0".into();
    }
    let magnitude = value.abs();
    if (1e-6..1e21).contains(&magnitude) {
        let text = format!("{value}");
        return text;
    }
    let text = format!("{value:e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    if exponent.starts_with('-') {
        format!("{mantissa}e{exponent}")
    } else {
        format!("{mantissa}e+{exponent}")
    }
}

/// `JSON.stringify` of a parsed JSON value.
pub(super) fn js_stringify(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number
            .as_f64()
            .filter(|number| number.is_finite())
            .map_or_else(|| "null".into(), js_number),
        Value::String(text) => serde_json::to_string(text).unwrap_or_default(),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(js_stringify).collect::<Vec<_>>().join(",")
        ),
        Value::Object(object) => {
            // Integer-like keys come first in ascending order, then the rest
            // in insertion order, as a JavaScript object orders its keys.
            let index = |key: &str| {
                key.parse::<u32>()
                    .ok()
                    .filter(|number| *number < u32::MAX && number.to_string() == key)
            };
            let mut keys: Vec<&String> = object.keys().filter(|key| index(key).is_some()).collect();
            keys.sort_by_key(|key| index(key));
            keys.extend(object.keys().filter(|key| index(key).is_none()));
            format!(
                "{{{}}}",
                keys.iter()
                    .map(|key| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        js_stringify(&object[key.as_str()])
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

fn normalize_path(d: &str) -> String {
    d.split(|character: char| {
        (character.is_whitespace() && character != '\u{85}') || character == '\u{feff}'
    })
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attribute(value: &str) -> String {
    escape_text(value).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// The portal commits the HTML its grammar draws for every specimen
    /// (docs-portal/tests/figure-grammar.test.mjs pins it to renderFigure), so
    /// reconstructing each one here proves the validator draws the same bytes.
    #[test]
    fn reconstruction_matches_the_portal_rendered_specimens() {
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs-portal/tests/fixtures/figures");
        let mut names: Vec<_> = std::fs::read_dir(&fixtures)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".json").map(str::to_owned)
            })
            .collect();
        names.sort();
        assert_eq!(names.len(), 10, "{names:?}");
        let derived = serde_json::json!({"commit_desc_max_len": 50, "commit_subject_max_len": 72});
        for name in names {
            let declaration: serde_json::Value = serde_json::from_slice(
                &std::fs::read(fixtures.join(format!("{name}.json"))).unwrap(),
            )
            .unwrap();
            let bound = (declaration["figure"]["binding"] == "derived")
                .then(|| derived.as_object().unwrap());
            let expected =
                std::fs::read_to_string(fixtures.join(format!("rendered/{name}.html"))).unwrap();
            let actual = super::render_figure(&declaration, "cf-fig-0", bound)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(format!("{actual}\n"), expected, "{name}");
        }
    }
}
