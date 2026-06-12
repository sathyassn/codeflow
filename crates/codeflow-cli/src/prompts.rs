//! Init's at-most-three questions (charter §4.1) — minimal stdin plumbing,
//! deliberately not a TUI (v1's init wizard is the anti-pattern, charter D22).

use std::io::{BufRead, Write};
use std::path::Path;

use codeflow_core::scaffold::InitAnswers;

fn ask(question: &str, default: &str) -> std::io::Result<String> {
    print!("{question} [{default}]: ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let answer = line.trim();
    Ok(if answer.is_empty() {
        default.to_string()
    } else {
        answer.to_string()
    })
}

/// Gathers the three init answers from stdin. Empty input keeps the default;
/// closed stdin behaves like `--yes`.
pub fn gather_answers(root: &Path) -> std::io::Result<InitAnswers> {
    let name = root
        .file_name()
        .map_or_else(|| "project".to_string(), |n| n.to_string_lossy().to_string());

    let one_liner = ask("Product one-liner (what is this project?)", &name)?;
    let areas = ask("Areas (comma-separated)", "core")?;
    let preset = loop {
        let answer = ask(
            "Permission preset (default | acceptEdits | bypassPermissions)",
            "default",
        )?;
        match answer.as_str() {
            "default" | "acceptEdits" | "bypassPermissions" => break answer,
            other => println!("unknown preset {other:?} — choose one of the three"),
        }
    };

    Ok(InitAnswers {
        product_one_liner: Some(one_liner),
        areas: Some(
            areas
                .split(',')
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
                .collect(),
        ),
        permission_preset: Some(preset),
    })
}
