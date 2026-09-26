//! This repository's own guide keeps each Technical altitude under 1500 words
//! of prose outside its tables (TSK-061). The count is the one the portal
//! adapter records and `validate --portal` recounts; the limit is this
//! repository's editorial rule, so it is a test here and never a portal
//! failure for a project that adopts the starter.

use std::path::Path;

use codeflow_core::validate::portal::altitude_words;

const TECHNICAL_WORD_LIMIT: usize = 1500;

#[test]
fn every_explanatory_page_keeps_technical_under_the_limit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("docs-portal/portal.config.json")).unwrap(),
    )
    .unwrap();
    let classed: Vec<&str> = config["page_classes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["source"].as_str())
        .collect();
    let pages: Vec<&str> = config["layers"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|layer| layer["paths"].as_array().into_iter().flatten())
        .filter_map(serde_json::Value::as_str)
        .filter(|path| !classed.contains(path))
        .collect();
    assert!(pages.len() >= 10, "too few explanatory pages: {pages:?}");
    let mut over = Vec::new();
    for page in &pages {
        let words = altitude_words(&std::fs::read_to_string(root.join(page)).unwrap());
        assert!(
            words.concept > 0 && words.technical > 0,
            "{page} has no Concept or Technical prose: {words:?}"
        );
        if words.technical > TECHNICAL_WORD_LIMIT {
            over.push(format!("{page}: {} words", words.technical));
        }
    }
    assert!(
        over.is_empty(),
        "Technical altitudes over {TECHNICAL_WORD_LIMIT} words of prose: {over:?}"
    );
}
