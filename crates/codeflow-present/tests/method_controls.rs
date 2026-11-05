//! The present review answers in `evals/model-artifacts/method-controls`
//! (TSK-062, TSK-445) are real presentation documents: each one parses and
//! validates through the same code `codeflow present open` runs, so the
//! passing and the faulty answers differ only in what the case grades, never
//! in validity.

use std::path::PathBuf;

use codeflow_present::document::{parse_document, Block, ParsedDocument};

fn answer(name: &str) -> Vec<Block> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../evals/model-artifacts/method-controls")
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

/// Every block that asks the reader, nested ones included.
fn asks(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::FeedbackPrompt { .. } | Block::Decision { .. } | Block::Form { .. } => 1,
            Block::Disclosure { blocks, .. } => asks(blocks),
            Block::Tabs { tabs, .. } => tabs.iter().map(|tab| asks(&tab.blocks)).sum(),
            _ => 0,
        })
        .sum()
}

/// An anchor: a narrative that opens with a `## ` heading and lists its lines.
fn is_anchor(block: &Block) -> bool {
    matches!(block, Block::Narrative { markdown, .. }
        if markdown.starts_with("## ") && markdown.lines().filter(|line| line.starts_with("- ")).count() >= 4)
}

#[test]
fn passing_migration_review_opens_with_an_anchor_then_the_figure_and_asks_once() {
    let blocks = answer("migration/passing.json");
    assert!(is_anchor(&blocks[0]));
    assert!(matches!(&blocks[1], Block::Narrative { markdown, .. } if markdown.starts_with("## ")));
    assert!(matches!(blocks[2], Block::Figure { .. }));
    assert!(matches!(blocks.last(), Some(Block::FeedbackPrompt { .. })));
    assert_eq!(asks(&blocks), 1);
}

#[test]
fn faulty_migration_review_without_an_anchor_is_valid_and_leads_with_the_figure() {
    let blocks = answer("migration/faulty-no-anchor.json");
    assert!(matches!(blocks.first(), Some(Block::Figure { .. })));
    assert!(!blocks.iter().any(is_anchor));
    assert_eq!(asks(&blocks), 1);
}

#[test]
fn retry_review_answers_are_valid_and_the_passing_one_is_anchored() {
    for name in [
        "retry/passing.json",
        "retry/faulty.json",
        "retry/faulty-no-figure.json",
        "retry/faulty-no-requeue.json",
        "retry/faulty-no-timeout.json",
        "retry/passing-compact-durations.json",
        "retry/faulty-backoff-90s.json",
        "retry/faulty-second-failure.json",
        "retry/faulty-outage-measured.json",
        "retry/passing-30-sec.json",
        "retry/passing-half-minute.json",
        "retry/passing-one-minute.json",
        "retry/passing-per-failure-backoffs.json",
        "retry/faulty-no-operator-requeue.json",
        "retry/faulty-not-unmeasured.json",
        "retry/faulty-cache-unmeasured.json",
        "retry/faulty-max-attempts.json",
    ] {
        let blocks = answer(name);
        assert_eq!(asks(&blocks), 1, "{name}");
        assert_eq!(is_anchor(&blocks[0]), name != "retry/faulty.json", "{name}");
    }
}

#[test]
fn faulty_migration_review_is_valid_but_leads_with_text_and_asks_twice() {
    let blocks = answer("migration/faulty.json");
    assert!(matches!(blocks.first(), Some(Block::Narrative { .. })));
    assert!(!blocks
        .iter()
        .any(|block| matches!(block, Block::Figure { .. })));
    assert_eq!(asks(&blocks), 2);
}
