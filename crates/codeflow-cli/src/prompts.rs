//! Init's at-most-three questions (charter §4.1) — minimal stdin plumbing,
//! deliberately not a TUI (v1's init wizard is the anti-pattern, charter D22).

use std::io::{BufRead, Write};
use std::path::Path;

use codeflow_core::hooks::policy::MappingState;
use codeflow_core::scaffold::InitAnswers;

/// The permission-preset question is Claude Code-scoped: the answer selects
/// which preset lands in `.claude/settings.json` and configures nothing else.
/// Codex autonomy is a fixed per-repo posture in `.codex/config.toml`
/// (ADR-0008) — the label says so, so a codex-primary user is not left
/// believing this answer configured their harness.
const PRESET_QUESTION: &str = "Claude Code project preset (default | acceptEdits | bypassPermissions). acceptEdits pins that mode in the project file, above your user settings; default and bypassPermissions leave the mode to you, and Claude Code takes auto or bypassPermissions only from ~/.claude/settings.json or the launch flag. Codex autonomy lives in .codex/config.toml";

/// Printed after `init` when the operator chose the `bypassPermissions`
/// preset (TSK-1070, issue 451). Claude Code ignores `bypassPermissions` in
/// a project settings file and starts the session in Manual mode, so the
/// preset leaves the mode unset and this note names the routes that work.
pub const BYPASS_ROUTE_NOTE: &str = "Bypass mode: Claude Code does not take bypassPermissions from .claude/settings.json, so the project file leaves the mode unset.\n  Start one session in bypass: claude --permission-mode bypassPermissions\n  CodeFlow can keep bypass mode, a sandbox a repository cannot loosen and the host list in its own session file, ~/.codeflow/claude/session.json, and start `claude` with it inside CodeFlow projects. Each change below is shown first; bypass mode and the startup-file line are made only on your yes.\n  The guard hooks, deny rules and the fail-closed sandbox still apply in bypass mode; /cf-customize runs the canary that shows it.";

fn ask(input: &mut impl BufRead, question: &str, default: &str) -> std::io::Result<String> {
    print!("{question} [{default}]: ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    input.read_line(&mut line)?;
    let answer = line
        .strip_suffix("\r\n")
        .or_else(|| line.strip_suffix('\n'))
        .unwrap_or(&line);
    Ok(if answer.is_empty() {
        default.to_string()
    } else {
        answer.to_string()
    })
}

/// Ask the decision for a kept PR template (SPC-013 R-84) until the answer
/// is valid. End of input returns `None`, which leaves the mapping diagnosed.
pub fn decide_pr_template(
    input: &mut impl BufRead,
    path: &str,
) -> std::io::Result<Option<MappingState>> {
    use codeflow_core::scaffold::Shown;
    // `update` asks this, so the prompt is `Shown` text (TSK-1029).
    let question = codeflow_core::shown!(
        "Kept PR template {}: accept the proposed heading mapping, refuse it (append the required headings to the template), or keep a custom section list set in policy? [accept | refuse | custom]: ",
        Shown::path(path)
    );
    loop {
        crate::cmd::prompt(&question)?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            crate::cmd::say(&Shown::default());
            return Ok(None);
        }
        match line
            .trim_matches([' ', '\t', '\r', '\n'])
            .to_ascii_lowercase()
            .as_str()
        {
            "accept" | "accepted" => return Ok(Some(MappingState::Accepted)),
            "refuse" | "refused" => return Ok(Some(MappingState::Refused)),
            "custom" => return Ok(Some(MappingState::Custom)),
            _ => crate::cmd::say(&Shown::fixed(
                "unknown answer; choose accept, refuse or custom",
            )),
        }
    }
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
        |n| {
            codeflow_core::git::GitName::from_os_str(n)
                .display()
                .to_string()
        },
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
                .map(|a| a.trim_matches([' ', '\t']).to_string())
                .filter(|a| !a.is_empty())
                .collect(),
        ),
        permission_preset: Some(preset),
    })
}

#[cfg(test)]
mod tests {

    #[test]
    fn r15_prompt_preserves_answer_identity() {
        let mut input = "\u{a0}acceptEdits\u{a0}\n".as_bytes();
        assert_eq!(
            super::ask(&mut input, "preset", "default").unwrap(),
            "\u{a0}acceptEdits\u{a0}"
        );
        let mut input = "answer\r".as_bytes();
        assert_eq!(
            super::ask(&mut input, "value", "default").unwrap(),
            "answer\r"
        );
    }

    use super::*;

    fn gather(input: &str) -> InitAnswers {
        let mut reader = input.as_bytes();
        gather_answers_from(&mut reader, Path::new("/tmp/myproj")).unwrap()
    }

    #[test]
    fn pr_template_decision_reasks_and_stops_at_end_of_input() {
        let mut input = "maybe\nrefuse\n".as_bytes();
        assert_eq!(
            decide_pr_template(&mut input, "x").unwrap(),
            Some(MappingState::Refused)
        );
        let mut input = "".as_bytes();
        assert_eq!(decide_pr_template(&mut input, "x").unwrap(), None);
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
        assert!(PRESET_QUESTION.contains("only from ~/.claude/settings.json or the launch flag"));
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
