//! The present `diagram` block and its Mermaid renderer were removed
//! (TSK-087). No skill tree, evaluation kit resource or recorded model
//! artifact may teach either again: a present block typed `diagram` fails,
//! and every line that names Mermaid must be one this file lists, where the
//! line says Mermaid is unsupported or uses it as a faulty control.

use std::path::{Path, PathBuf};

use regex::Regex;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// The trees scanned, each with the prefix its relative paths are listed under.
const ROOTS: [(&str, &str); 4] = [
    ("assets/base/agents/skills", ""),
    (".claude/skills", ""),
    (".agents/skills", ""),
    ("evals/model-artifacts", "evals/model-artifacts/"),
];

/// Every permitted line that names Mermaid, by path relative to its root.
/// Each says Mermaid is unsupported; a faulty control that names it belongs
/// here too, with the case that fails on it.
const ALLOWED: [(&str, &str); 12] = [
    (
        "cf-present/references/document-authoring.md",
        "The `diagram` block was removed with its Mermaid renderer, and Mermaid is",
    ),
    (
        "cf-present/references/document-authoring.md",
        "A figure declaration is longer than a Mermaid line, so start from a complete",
    ),
    (
        "cf-present/resources/how-presentation-works.md",
        "Mermaid is unsupported on both surfaces: present refuses a `diagram` block and",
    ),
    (
        "cf-present/resources/how-presentation-works.md",
        "the portal shows a Mermaid fence as code.",
    ),
    (
        "cf-present/resources/utility-presentation-system.md",
        "| Mermaid | **unsupported**: a Mermaid fence renders as plain code | **unsupported**: a `diagram` block is refused with its conversion named |",
    ),
    (
        "cf-present/resources/utility-presentation-system.md",
        "Unsupported on both surfaces: Mermaid and any other diagram syntax beyond the",
    ),
    (
        "cf-docs-portal/resources/utility-presentation-system.md",
        "| Mermaid | **unsupported**: a Mermaid fence renders as plain code | **unsupported**: a `diagram` block is refused with its conversion named |",
    ),
    (
        "cf-docs-portal/resources/utility-presentation-system.md",
        "Unsupported on both surfaces: Mermaid and any other diagram syntax beyond the",
    ),
    (
        "cf-present/resources/explanation-method.md",
        "file and a Mermaid fence are not README figures: the portal rejects SVG",
    ),
    (
        "cf-present/resources/explanation-method.md",
        "media and shows a Mermaid fence as code, and GitHub shows a `cf-stage`",
    ),
    (
        "cf-docs-portal/resources/explanation-method.md",
        "file and a Mermaid fence are not README figures: the portal rejects SVG",
    ),
    (
        "cf-docs-portal/resources/explanation-method.md",
        "media and shows a Mermaid fence as code, and GitHub shows a `cf-stage`",
    ),
];

/// A present block typed `diagram`, in a JSON file or quoted inside a string
/// (an evaluation prompt or a Markdown example).
fn diagram_block() -> Regex {
    Regex::new(r#"\\?"type\\?"\s*:\s*\\?"diagram\\?""#).expect("valid pattern")
}

fn violations(relative: &str, text: &str, diagram: &Regex) -> Vec<String> {
    let mut found = Vec::new();
    if diagram.is_match(text) {
        found.push(format!(
            "{relative}: carries a present \"type\": \"diagram\" block"
        ));
    }
    for (number, line) in text.lines().enumerate() {
        if !line.to_ascii_lowercase().contains("mermaid") {
            continue;
        }
        let trimmed = line.trim();
        if !ALLOWED
            .iter()
            .any(|(path, allowed)| *path == relative && *allowed == trimmed)
        {
            found.push(format!(
                "{relative}:{}: names Mermaid outside the allowed list: {trimmed}",
                number + 1
            ));
        }
    }
    found
}

fn files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("tree is readable") {
            let path = entry.expect("entry is readable").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn no_skill_or_evaluation_resource_teaches_the_removed_diagram_block() {
    let root = repo_root();
    let diagram = diagram_block();
    let mut problems = Vec::new();
    let mut scanned = 0;
    let mut allowed_seen = std::collections::HashSet::new();
    for (tree, prefix) in ROOTS {
        let base = root.join(tree);
        assert!(base.is_dir(), "{tree} is missing");
        for path in files(&base) {
            // Fonts and images are not text an agent is taught from.
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            scanned += 1;
            let relative = format!(
                "{prefix}{}",
                path.strip_prefix(&base)
                    .expect("inside its tree")
                    .to_string_lossy()
                    .replace('\\', "/")
            );
            for (allowed_path, line) in ALLOWED {
                if allowed_path == relative
                    && text.lines().any(|candidate| candidate.trim() == line)
                {
                    allowed_seen.insert((allowed_path, line));
                }
            }
            problems.extend(violations(&relative, &text, &diagram));
        }
    }
    assert!(scanned > 100, "only {scanned} text files were scanned");
    assert!(
        problems.is_empty(),
        "the removed diagram block or Mermaid is taught:\n  {}",
        problems.join("\n  ")
    );
    let stale: Vec<_> = ALLOWED
        .iter()
        .filter(|entry| !allowed_seen.contains(entry))
        .collect();
    assert!(
        stale.is_empty(),
        "allowed Mermaid lines no longer present; remove them from the list: {stale:?}"
    );
}

#[test]
fn the_guard_fails_a_diagram_block_and_an_unlisted_mermaid_line() {
    let diagram = diagram_block();
    let case = r#"{"id": "case-1", "prompt": "Present it with {\"type\":\"diagram\",\"kind\":\"flowchart\"}"}"#;
    assert_eq!(
        violations("cf-evaluate-model/resources/cases.json", case, &diagram).len(),
        1
    );
    let block = "{\n  \"type\": \"diagram\",\n  \"id\": \"flow\"\n}\n";
    assert_eq!(
        violations("cf-present/assets/new.example.json", block, &diagram).len(),
        1
    );
    let instruction = "Draw the flow as a Mermaid flowchart in a diagram block.\n";
    let found = violations("cf-present/SKILL.md", instruction, &diagram);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("SKILL.md:1: names Mermaid"), "{found:?}");

    // A listed line passes only in its own file.
    let (path, line) = ALLOWED[2];
    assert!(violations(path, &format!("{line}\n"), &diagram).is_empty());
    assert_eq!(
        violations("cf-present/SKILL.md", &format!("{line}\n"), &diagram).len(),
        1
    );
    // Figure blocks and the diagram-line token are not the removed block.
    let figure = "{\"type\": \"figure\", \"id\": \"flow\"} --cf-diagram-line\n";
    assert!(violations("cf-present/assets/x.json", figure, &diagram).is_empty());
}
