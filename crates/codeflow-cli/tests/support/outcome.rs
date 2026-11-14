//! Fill the `## Outcome` section of records a fixture made with `task new`
//! or `epic new` (TSK-435), as their author does before committing them: a
//! record made after a project adopted the section is refused until it
//! states one line.

// Each test file uses the part it needs.
#![allow(dead_code)]

use std::path::Path;

/// The line written under each empty `## Outcome` section.
pub const OUTCOME_LINE: &str = "after this merges, a fixture user can do the work";

/// Write [`OUTCOME_LINE`] into every task and epic record under
/// `root/project-management` whose `## Outcome` section states nothing yet.
/// Records without the section, or with a line already, are left alone.
pub fn state_outcomes(root: &Path) {
    let pm = root.join("project-management");
    let mut dirs = vec![pm.join("tasks"), pm.join("epics")];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                state_outcome(&path);
            }
        }
    }
}

/// Write [`OUTCOME_LINE`] into the record at `path` when its `## Outcome`
/// section holds only its template comment.
pub fn state_outcome(path: &Path) {
    let text = std::fs::read_to_string(path).unwrap();
    let filled = with_outcome(&text);
    if filled != text {
        std::fs::write(path, filled).unwrap();
    }
}

/// `text` with [`OUTCOME_LINE`] under its `## Outcome` section when the
/// section holds only its template comment; otherwise `text` unchanged.
#[must_use]
pub fn with_outcome(text: &str) -> String {
    let Some(start) = text.find("\n## Outcome\n") else {
        return text.to_string();
    };
    let body_start = start + "\n## Outcome\n".len();
    let end = text[body_start..]
        .find("\n## ")
        .map_or(text.len(), |at| body_start + at);
    let section = &text[body_start..end];
    let without_comments = section
        .split("<!--")
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part
            } else {
                part.split_once("-->").map_or("", |(_, rest)| rest)
            }
        })
        .collect::<String>();
    if !without_comments.trim().is_empty() {
        return text.to_string();
    }
    format!(
        "{}{}\n{OUTCOME_LINE}\n{}",
        &text[..body_start],
        section.trim_end(),
        &text[end..]
    )
}
