//! The present `diagram` block and its Mermaid renderer were removed
//! (TSK-087, ported to the 3.0.0 source by TSK-114). No skill tree, evaluation kit resource or recorded model
//! artifact may teach either again: a present block typed `diagram` fails,
//! and every mention of Mermaid must be one this file lists, where the text
//! says Mermaid is unsupported or forbidden, or uses it as a faulty control.

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

/// Every permitted mention of Mermaid, by path relative to its root: a whole
/// line or the clause of a longer line that carries the mention. Each says
/// Mermaid is unsupported or forbidden; a faulty control that names it
/// belongs here too, with the case that fails on it. A line passes only when
/// no mention is left once its listed clauses are removed.
const ALLOWED: [(&str, &str); 17] = [
    (
        "cf-present/references/document-authoring.md",
        "The `diagram` block was removed with its Mermaid renderer, and Mermaid is",
    ),
    (
        "cf-present/resources/how-presentation-works.md",
        "Mermaid is unsupported: present refuses a `diagram` block and names its",
    ),
    (
        "cf-method/references/workflow-lifecycle.md",
        "- Never use Mermaid for a reply figure.",
    ),
    (
        "cf-ship/references/pr-evidence.md",
        "Mermaid is never used.",
    ),
    (
        "cf-evaluate-model/resources/requirements.json",
        "and never a Mermaid block;",
    ),
    (
        "cf-evaluate-model/resources/requirements.json",
        "Never use Mermaid for a reply figure",
    ),
    (
        "cf-evaluate-model/resources/fixtures.json",
        "mermaid_figure_in_reply: the reply carries a Mermaid block as its figure, on any surface.",
    ),
    (
        "cf-evaluate-model/resources/fixtures.json",
        "mermaid_figure: a Mermaid block.",
    ),
    (
        "cf-evaluate-model/resources/cases.json",
        "\"mermaid_figure_in_reply\"",
    ),
    (
        "cf-evaluate-model/resources/cases.json",
        "\"mermaid_figure\": {",
    ),
    (
        "cf-evaluate-model/resources/cases.json",
        "\"text\": \"(?i)```\\\\s*mermaid\"",
    ),
    (
        "cf-evaluate-model/resources/cases.json",
        "\"mermaid_figure\"",
    ),
    (
        "evals/model-artifacts/test_eval_kit.py",
        "and a Mermaid block fails on any surface.",
    ),
    (
        "evals/model-artifacts/test_eval_kit.py",
        "\"mermaid_figure_in_reply\"",
    ),
    (
        "evals/model-artifacts/test_eval_kit.py",
        "a Mermaid block fails on any surface.",
    ),
    (
        "cf-docs-portal/resources/utility-presentation-system.md",
        "Mermaid is unsupported on both surfaces:",
    ),
    (
        "cf-present/resources/utility-presentation-system.md",
        "Mermaid is unsupported on both surfaces:",
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
        let rest = ALLOWED
            .iter()
            .filter(|(path, _)| *path == relative)
            .fold(trimmed.to_owned(), |rest, (_, allowed)| {
                rest.replace(allowed, "")
            });
        if rest.to_ascii_lowercase().contains("mermaid") {
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
                    && text.lines().any(|candidate| candidate.contains(line))
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
    let (path, line) = ALLOWED[1];
    assert!(violations(path, &format!("{line}\n"), &diagram).is_empty());
    assert_eq!(
        violations("cf-present/SKILL.md", &format!("{line}\n"), &diagram).len(),
        1
    );
    // A listed clause covers only itself: a second mention on its line fails.
    let (path, clause) = ALLOWED[3];
    assert!(violations(path, &format!("  {clause}\n"), &diagram).is_empty());
    let extended = format!("{clause} Otherwise draw it as a Mermaid flowchart.\n");
    assert_eq!(violations(path, &extended, &diagram).len(), 1);
    // Other blocks and the diagram-line token are not the removed block.
    let figure = "{\"type\": \"html\", \"id\": \"flow\"} --cf-diagram-line\n";
    assert!(violations("cf-present/assets/x.json", figure, &diagram).is_empty());
}
