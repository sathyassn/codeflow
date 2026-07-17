//! Init's at-most-three questions (charter §4.1) — minimal stdin plumbing,
//! deliberately not a TUI (v1's init wizard is the anti-pattern, charter D22).

use std::io::{BufRead, Write};
use std::path::Path;

use codeflow_core::scaffold::InitAnswers;

/// The permission-preset question is Claude Code-scoped: the answer selects
/// which preset lands in `.claude/settings.json` and configures nothing else.
/// Codex autonomy is a fixed per-repo posture in `.codex/config.toml`
/// (ADR-0008) — the label says so, so a codex-primary user is not left
/// believing this answer configured their harness.
const PRESET_QUESTION: &str = "Claude Code project preset (default | acceptEdits | bypassPermissions; bypass only for an externally isolated container/VM) — auto mode is selected at CLI/user scope, and codex autonomy lives in .codex/config.toml";

fn ask(input: &mut impl BufRead, question: &str, default: &str) -> std::io::Result<String> {
    print!("{question} [{default}]: ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    input.read_line(&mut line)?;
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
    gather_answers_from(&mut std::io::stdin().lock(), root)
}

/// [`gather_answers`] over any reader — the testable core.
fn gather_answers_from(input: &mut impl BufRead, root: &Path) -> std::io::Result<InitAnswers> {
    let name = root.file_name().map_or_else(
        || "project".to_string(),
        |n| n.to_string_lossy().to_string(),
    );

    let one_liner = ask(input, "Product one-liner (what is this project?)", &name)?;
    let areas = ask(input, "Areas (comma-separated)", "core")?;
    let preset = loop {
        // Default to acceptEdits: the owner autonomy posture (ADR-0008) makes
        // promptless project-scoped work the out-of-the-box experience. The
        // choice stays the user's — the deny/ask tiers and the guard hooks are
        // the protections, not the permission mode.
        let answer = ask(input, PRESET_QUESTION, "acceptEdits")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn gather(input: &str) -> InitAnswers {
        let mut reader = input.as_bytes();
        gather_answers_from(&mut reader, Path::new("/tmp/myproj")).unwrap()
    }

    #[test]
    fn piped_answers_fill_all_three_questions() {
        let answers = gather("a discipline layer\nengine, scaffold, docs\nacceptEdits\n");
        assert_eq!(
            answers.product_one_liner.as_deref(),
            Some("a discipline layer")
        );
        assert_eq!(
            answers.areas,
            Some(vec![
                "engine".to_string(),
                "scaffold".to_string(),
                "docs".to_string()
            ])
        );
        assert_eq!(answers.permission_preset.as_deref(), Some("acceptEdits"));
    }

    #[test]
    fn empty_input_keeps_every_default() {
        // Closed stdin (EOF on every question) behaves like --yes. The preset
        // default is acceptEdits — the owner autonomy posture (ADR-0008).
        let answers = gather("");
        assert_eq!(answers.product_one_liner.as_deref(), Some("myproj"));
        assert_eq!(answers.areas, Some(vec!["core".to_string()]));
        assert_eq!(answers.permission_preset.as_deref(), Some("acceptEdits"));
    }

    #[test]
    fn blank_lines_keep_defaults_too() {
        let answers = gather("\n\n\n");
        assert_eq!(answers.product_one_liner.as_deref(), Some("myproj"));
        assert_eq!(answers.areas, Some(vec!["core".to_string()]));
        assert_eq!(answers.permission_preset.as_deref(), Some("acceptEdits"));
    }

    #[test]
    fn unknown_preset_is_reasked_until_valid() {
        let answers = gather("p\ncore\nyolo\nbypassPermissions\n");
        assert_eq!(
            answers.permission_preset.as_deref(),
            Some("bypassPermissions")
        );
    }

    #[test]
    fn preset_question_is_claude_code_scoped() {
        // The preset writes .claude/settings.json only; the label must name
        // the harness it binds and point codex users at their real knob.
        assert!(PRESET_QUESTION.starts_with("Claude Code project preset"));
        assert!(PRESET_QUESTION.contains("auto mode is selected at CLI/user scope"));
        assert!(PRESET_QUESTION.contains("externally isolated container/VM"));
        assert!(PRESET_QUESTION.contains(".codex/config.toml"));
    }

    #[test]
    fn areas_are_trimmed_and_empties_dropped() {
        let answers = gather("p\n core ,, cli , \ndefault\n");
        assert_eq!(
            answers.areas,
            Some(vec!["core".to_string(), "cli".to_string()])
        );
    }
}
