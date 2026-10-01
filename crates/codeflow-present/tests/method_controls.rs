//! The migration review answers in `evals/model-artifacts/method-controls`
//! (TSK-062) are real presentation documents: each one parses and validates
//! through the same code `codeflow present open` runs, so the passing and the
//! faulty answer differ only in what the case grades, never in validity.

use std::path::PathBuf;

use codeflow_present::document::{parse_document, Block, ParsedDocument};

fn answer(name: &str) -> Vec<Block> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../evals/model-artifacts/method-controls/migration")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    match parse_document(&bytes) {
        Ok(ParsedDocument::Supported(document)) => document.blocks,
        Ok(ParsedDocument::Unsupported { schema_version, .. }) => {
            panic!("{name}: unsupported schema version {schema_version}")
        }
        Err(error) => panic!("{name}: {error}"),
    }
}

fn asks(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .filter(|block| matches!(block, Block::FeedbackPrompt { .. }))
        .count()
}

#[test]
fn passing_migration_review_leads_with_a_figure_and_asks_once() {
    let blocks = answer("passing.json");
    assert!(matches!(blocks.first(), Some(Block::Figure { .. })));
    assert!(matches!(blocks.last(), Some(Block::FeedbackPrompt { .. })));
    assert_eq!(asks(&blocks), 1);
}

#[test]
fn faulty_migration_review_is_valid_but_leads_with_text_and_asks_twice() {
    let blocks = answer("faulty.json");
    assert!(matches!(blocks.first(), Some(Block::Narrative { .. })));
    assert!(!blocks
        .iter()
        .any(|block| matches!(block, Block::Figure { .. })));
    assert_eq!(asks(&blocks), 2);
}
