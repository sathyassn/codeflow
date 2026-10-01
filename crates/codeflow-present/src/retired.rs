//! The `diagram` block, retired with its renderer (TSK-087).
//!
//! New `open` and `update` input that carries one is refused before typed
//! parsing, with the replacement for its `kind`. A revision that a
//! pre-release build stored with one still loads, read only: the state reader
//! asks [`legacy_document`] whether the diagram blocks are the record's only
//! departure from the current catalog, and the renderer shows each one's
//! source beside its conversion.

use serde_json::{Map, Value};

use crate::error::{PresentError, Result};
use crate::limits;

/// Where the conversion from a diagram block is taught.
pub const CONVERSION_GUIDE: &str =
    "the \"Converting a diagram block\" section of cf-present/references/document-authoring.md";

/// Each former `kind` with the block that replaces it.
pub const CONVERSIONS: [(&str, &str); 7] = [
    ("flowchart", "a flow figure"),
    ("sequence", "a sequence figure"),
    ("state", "a state figure"),
    (
        "class",
        "a structure figure, or a table where no relationship must be seen",
    ),
    (
        "entity_relationship",
        "a structure figure, or a table where no relationship must be seen",
    ),
    ("mindmap", "a tree block"),
    (
        "timeline",
        "a table, or a sequence figure when participants exchange messages",
    ),
];

const GENERIC_REPLACEMENT: &str = "a figure block, a table or a tree block";

/// The fields a diagram block carried before its removal.
const DIAGRAM_FIELDS: [&str; 6] = [
    "type",
    "id",
    "kind",
    "source",
    "acc_title",
    "acc_description",
];

/// The replacement for a former diagram `kind`, or the generic one when the
/// kind is missing or was never a diagram kind.
#[must_use]
pub fn replacement(kind: Option<&str>) -> &'static str {
    kind.and_then(|kind| CONVERSIONS.iter().find(|(name, _)| *name == kind))
        .map_or(GENERIC_REPLACEMENT, |(_, replacement)| replacement)
}

/// One diagram block found in a raw document.
#[derive(Debug)]
pub struct RetiredBlock<'a> {
    /// Its JSON position, such as `blocks[2].tabs[0].blocks[1]`.
    pub position: String,
    pub block: &'a Map<String, Value>,
}

impl RetiredBlock<'_> {
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.text("id")
    }

    #[must_use]
    pub fn kind(&self) -> Option<&str> {
        self.text("kind")
    }

    #[must_use]
    pub fn text(&self, field: &str) -> Option<&str> {
        self.block.get(field).and_then(Value::as_str)
    }
}

/// Every diagram block of a raw document, at any depth, in document order.
#[must_use]
pub fn retired_blocks(document: &Value) -> Vec<RetiredBlock<'_>> {
    let mut found = Vec::new();
    collect(document.get("blocks"), "blocks", &mut found);
    found
}

fn collect<'a>(blocks: Option<&'a Value>, path: &str, found: &mut Vec<RetiredBlock<'a>>) {
    let Some(blocks) = blocks.and_then(Value::as_array) else {
        return;
    };
    for (index, block) in blocks.iter().enumerate() {
        let Some(block) = block.as_object() else {
            continue;
        };
        let position = format!("{path}[{index}]");
        if block.get("type").and_then(Value::as_str) == Some("diagram") {
            found.push(RetiredBlock { position, block });
            continue;
        }
        collect(block.get("blocks"), &format!("{position}.blocks"), found);
        if let Some(tabs) = block.get("tabs").and_then(Value::as_array) {
            for (tab, entry) in tabs.iter().enumerate() {
                collect(
                    entry.get("blocks"),
                    &format!("{position}.tabs[{tab}].blocks"),
                    found,
                );
            }
        }
    }
}

/// Refuse new input that carries a diagram block, naming the first one and
/// its replacement.
pub fn refuse_retired_blocks(document: &Value) -> Result<()> {
    let found = retired_blocks(document);
    let Some(first) = found.first() else {
        return Ok(());
    };
    let named = first.id().map_or_else(
        || format!("the block at {}", first.position),
        |id| format!("block {id:?}"),
    );
    let conversion = match first.kind() {
        Some(kind) if CONVERSIONS.iter().any(|(name, _)| *name == kind) => {
            format!("convert its {kind} to {}", replacement(Some(kind)))
        }
        _ => format!("convert it to {GENERIC_REPLACEMENT}"),
    };
    let others = match found.len() - 1 {
        0 => String::new(),
        1 => "; 1 more diagram block needs converting".to_string(),
        more => format!("; {more} more diagram blocks need converting"),
    };
    Err(PresentError::InvalidDocument(format!(
        "{named} is a diagram block, which was removed with Mermaid; {conversion}, as {CONVERSION_GUIDE} shows{others}"
    )))
}

/// A stored document whose diagram blocks all keep their pre-removal shape
/// and limits: the document with each diagram replaced by an empty narrative
/// of the same id, so typed parsing can check everything else, and the
/// diagram ids. `None` when the document has no diagram block, or one the
/// pre-removal validator would have refused: a corrupt record is never
/// presented as a retired one.
#[must_use]
pub fn legacy_document(document: &Value) -> Option<(Value, Vec<String>)> {
    let found = retired_blocks(document);
    if found.is_empty()
        || found.len() > limits::MAX_DIAGRAM_BLOCKS
        || !found.iter().all(well_formed)
    {
        return None;
    }
    let ids: Vec<String> = found
        .iter()
        .filter_map(|block| block.id().map(str::to_string))
        .collect();
    let mut substituted = document.clone();
    substitute(substituted.get_mut("blocks"));
    Some((substituted, ids))
}

/// A diagram as the pre-removal validator admitted it: its six string
/// fields, a known kind, a source within its bound, and an accessible title
/// and description that are present and within theirs.
fn well_formed(block: &RetiredBlock<'_>) -> bool {
    let within = |field: &str, max: usize, required: bool| {
        block
            .text(field)
            .is_some_and(|text| text.len() <= max && !(required && text.trim().is_empty()))
    };
    block.block.len() == DIAGRAM_FIELDS.len()
        && DIAGRAM_FIELDS
            .iter()
            .all(|field| block.block.get(*field).is_some_and(Value::is_string))
        && block
            .kind()
            .is_some_and(|kind| CONVERSIONS.iter().any(|(name, _)| *name == kind))
        && within("source", limits::MAX_DIAGRAM_BYTES, false)
        && within("acc_title", limits::MAX_TITLE_BYTES, true)
        && within("acc_description", limits::MAX_PROSE_BYTES, true)
}

fn substitute(blocks: Option<&mut Value>) {
    let Some(blocks) = blocks.and_then(Value::as_array_mut) else {
        return;
    };
    for block in blocks {
        let Some(object) = block.as_object_mut() else {
            continue;
        };
        if object.get("type").and_then(Value::as_str) == Some("diagram") {
            let id = object.get("id").cloned().unwrap_or(Value::Null);
            *block = serde_json::json!({ "type": "narrative", "id": id, "markdown": "" });
            continue;
        }
        substitute(object.get_mut("blocks"));
        if let Some(tabs) = object.get_mut("tabs").and_then(Value::as_array_mut) {
            for tab in tabs {
                substitute(tab.get_mut("blocks"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagram(id: Option<&str>, kind: &str) -> Value {
        let mut block = serde_json::json!({
            "type": "diagram",
            "kind": kind,
            "source": "flowchart LR\n  A --> B",
            "acc_title": "Flow",
            "acc_description": "A reaches B."
        });
        if let Some(id) = id {
            block["id"] = id.into();
        }
        block
    }

    #[allow(clippy::needless_pass_by_value, reason = "tests build blocks inline")]
    fn document(blocks: Vec<Value>) -> Value {
        serde_json::json!({ "schema_version": 1, "title": "Review", "blocks": blocks })
    }

    fn refusal(document: &Value) -> String {
        refuse_retired_blocks(document)
            .expect_err("a diagram block is refused")
            .to_string()
    }

    /// A stored diagram is retired only within the pre-removal admission
    /// limits (T114-2); at the limits it still is.
    #[test]
    fn a_legacy_diagram_is_admitted_only_within_the_old_limits() {
        use crate::limits::{
            MAX_DIAGRAM_BLOCKS, MAX_DIAGRAM_BYTES, MAX_PROSE_BYTES, MAX_TITLE_BYTES,
        };
        let with = |field: &str, value: String| {
            let mut block = diagram(Some("d"), "flowchart");
            block[field] = value.into();
            document(vec![block])
        };
        let many = |count: usize| {
            document(
                (0..count)
                    .map(|index| diagram(Some(&format!("d{index}")), "flowchart"))
                    .collect(),
            )
        };
        for admitted in [
            with("acc_title", "t".repeat(MAX_TITLE_BYTES)),
            with("acc_description", "d".repeat(MAX_PROSE_BYTES)),
            with("source", "s".repeat(MAX_DIAGRAM_BYTES)),
            many(MAX_DIAGRAM_BLOCKS),
        ] {
            assert!(legacy_document(&admitted).is_some());
        }
        for (name, refused) in [
            ("empty title", with("acc_title", String::new())),
            ("blank title", with("acc_title", " \t".to_string())),
            (
                "long title",
                with("acc_title", "t".repeat(MAX_TITLE_BYTES + 1)),
            ),
            (
                "empty description",
                with("acc_description", "  ".to_string()),
            ),
            (
                "long description",
                with("acc_description", "d".repeat(MAX_PROSE_BYTES + 1)),
            ),
            (
                "long source",
                with("source", "s".repeat(MAX_DIAGRAM_BYTES + 1)),
            ),
            ("too many", many(MAX_DIAGRAM_BLOCKS + 1)),
        ] {
            assert!(legacy_document(&refused).is_none(), "{name}");
        }
    }

    #[test]
    fn the_refusal_finds_a_diagram_at_every_nesting_level() {
        let top = document(vec![diagram(Some("top"), "flowchart")]);
        let disclosed = document(vec![serde_json::json!({
            "type": "disclosure", "id": "more", "summary": "More",
            "blocks": [diagram(Some("inner"), "flowchart")]
        })]);
        let tabbed = document(vec![serde_json::json!({
            "type": "tabs", "id": "views",
            "tabs": [
                {"label": "One", "blocks": []},
                {"label": "Two", "blocks": [{
                    "type": "disclosure", "id": "deep", "summary": "Deep",
                    "blocks": [diagram(Some("deepest"), "flowchart")]
                }]}
            ]
        })]);
        for (value, id, position) in [
            (&top, "top", "blocks[0]"),
            (&disclosed, "inner", "blocks[0].blocks[0]"),
            (&tabbed, "deepest", "blocks[0].tabs[1].blocks[0].blocks[0]"),
        ] {
            let message = refusal(value);
            assert!(message.contains(&format!("block {id:?}")), "{message}");
            assert!(message.contains("removed with Mermaid"), "{message}");
            assert!(
                message.contains("convert its flowchart to a flow figure"),
                "{message}"
            );
            assert!(message.contains(CONVERSION_GUIDE), "{message}");
            assert_eq!(retired_blocks(value)[0].position, position);
        }
    }

    #[test]
    fn the_refusal_names_the_replacement_for_every_former_kind() {
        for (kind, replacement) in CONVERSIONS {
            let message = refusal(&document(vec![diagram(Some("d"), kind)]));
            assert!(
                message.contains(&format!("convert its {kind} to {replacement}")),
                "{message}"
            );
        }
        for kind in ["gantt", ""] {
            let message = refusal(&document(vec![diagram(Some("d"), kind)]));
            assert!(
                message.contains("convert it to a figure block, a table or a tree block"),
                "{message}"
            );
        }
    }

    #[test]
    fn a_diagram_without_an_id_is_named_by_its_position_and_extras_are_counted() {
        let value = document(vec![
            serde_json::json!({"type": "narrative", "id": "intro", "markdown": "Hi"}),
            diagram(None, "sequence"),
            diagram(Some("second"), "state"),
            diagram(Some("third"), "mindmap"),
        ]);
        let message = refusal(&value);
        assert!(
            message.starts_with(
                "invalid presentation document: the block at blocks[1] is a diagram block"
            ),
            "{message}"
        );
        assert!(
            message.contains("convert its sequence to a sequence figure"),
            "{message}"
        );
        assert!(
            message.ends_with("; 2 more diagram blocks need converting"),
            "{message}"
        );
        let pair = document(vec![
            diagram(Some("first"), "flowchart"),
            diagram(Some("second"), "timeline"),
        ]);
        let message = refusal(&pair);
        assert!(
            message.ends_with("; 1 more diagram block needs converting"),
            "{message}"
        );
        assert!(refuse_retired_blocks(&document(vec![])).is_ok());
    }

    #[test]
    fn a_legacy_document_keeps_its_ids_and_refuses_malformed_diagrams() {
        let value = document(vec![serde_json::json!({
            "type": "tabs", "id": "views",
            "tabs": [{"label": "One", "blocks": [diagram(Some("flow"), "flowchart")]}]
        })]);
        let (substituted, ids) = legacy_document(&value).expect("legacy document");
        assert_eq!(ids, ["flow"]);
        assert_eq!(
            substituted.pointer("/blocks/0/tabs/0/blocks/0"),
            Some(&serde_json::json!({"type": "narrative", "id": "flow", "markdown": ""}))
        );

        assert!(legacy_document(&document(vec![])).is_none());
        let mut extra = diagram(Some("flow"), "flowchart");
        extra["theme"] = "dark".into();
        let mut numeric = diagram(Some("flow"), "flowchart");
        numeric["source"] = 7.into();
        for malformed in [
            extra,
            numeric,
            diagram(None, "flowchart"),
            diagram(Some("flow"), "gantt"),
        ] {
            assert!(
                legacy_document(&document(vec![malformed.clone()])).is_none(),
                "{malformed}"
            );
        }
    }
}
