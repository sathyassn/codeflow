//! Prompt abstraction for the setup wizard.
//!
//! Provides an injectable trait so the wizard can be tested without a live
//! terminal. Production code uses [`TerminalPromptProvider`]; tests inject
//! [`ScriptedPromptProvider`].

use std::io::{self, BufRead, Write};

/// Abstraction over user prompts.
pub trait PromptProvider {
    /// Ask the user a question and return their answer (trimmed).
    ///
    /// # Errors
    ///
    /// Returns `io::Error` on I/O failure.
    fn prompt(&self, question: &str) -> io::Result<String>;

    /// Ask a yes/no question. Returns the default when the user presses Enter.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` on I/O failure.
    fn confirm(&self, question: &str, default: bool) -> io::Result<bool> {
        let suffix = if default { "[Y/n]" } else { "[y/N]" };
        let answer = self.prompt(&format!("{question} {suffix}"))?;
        if answer.is_empty() {
            return Ok(default);
        }
        Ok(matches!(answer.to_lowercase().as_str(), "y" | "yes"))
    }

    /// Ask a question with three choices: y / N / skip. Returns the choice.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` on I/O failure.
    fn prompt_three_way(
        &self,
        question: &str,
        default: ThreeWayChoice,
    ) -> io::Result<ThreeWayChoice> {
        let suffix = match default {
            ThreeWayChoice::Yes => "[Y/n/skip]",
            ThreeWayChoice::No => "[y/N/skip]",
            ThreeWayChoice::Skip => "[y/n/SKIP]",
        };
        let answer = self.prompt(&format!("{question} {suffix}"))?;
        if answer.is_empty() {
            return Ok(default);
        }
        match answer.to_lowercase().as_str() {
            "y" | "yes" => Ok(ThreeWayChoice::Yes),
            "n" | "no" => Ok(ThreeWayChoice::No),
            "skip" | "s" => Ok(ThreeWayChoice::Skip),
            _ => Ok(default),
        }
    }
}

/// Three-way prompt result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreeWayChoice {
    Yes,
    No,
    Skip,
}

/// Existing config action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingConfigAction {
    Edit,
    Replace,
    Abort,
}

/// Terminal-based prompt provider for production use.
pub struct TerminalPromptProvider;

impl PromptProvider for TerminalPromptProvider {
    fn prompt(&self, question: &str) -> io::Result<String> {
        print!("{question} ");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line)?;
        Ok(line.trim().to_string())
    }
}

/// Scripted prompt provider for testing.
///
/// Pops answers from a queue in order. Panics if the queue is exhausted.
pub struct ScriptedPromptProvider {
    answers: std::cell::RefCell<Vec<String>>,
}

impl ScriptedPromptProvider {
    /// Create from a list of answers (first answer is returned first).
    pub fn new(answers: Vec<&str>) -> Self {
        Self {
            answers: std::cell::RefCell::new(answers.into_iter().map(String::from).collect()),
        }
    }
}

impl PromptProvider for ScriptedPromptProvider {
    fn prompt(&self, _question: &str) -> io::Result<String> {
        let mut answers = self.answers.borrow_mut();
        assert!(
            !answers.is_empty(),
            "ScriptedPromptProvider: no more answers in queue"
        );
        Ok(answers.remove(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_provider_returns_answers_in_order() {
        let p = ScriptedPromptProvider::new(vec!["first", "second"]);
        assert_eq!(p.prompt("q1").unwrap(), "first");
        assert_eq!(p.prompt("q2").unwrap(), "second");
    }

    #[test]
    #[should_panic(expected = "no more answers")]
    fn scripted_provider_panics_on_exhaustion() {
        let p = ScriptedPromptProvider::new(vec![]);
        let _ = p.prompt("q");
    }

    #[test]
    fn confirm_default_yes() {
        let p = ScriptedPromptProvider::new(vec![""]);
        assert!(p.confirm("Continue?", true).unwrap());
    }

    #[test]
    fn confirm_default_no() {
        let p = ScriptedPromptProvider::new(vec![""]);
        assert!(!p.confirm("Continue?", false).unwrap());
    }

    #[test]
    fn confirm_explicit_yes() {
        let p = ScriptedPromptProvider::new(vec!["y"]);
        assert!(p.confirm("Continue?", false).unwrap());
    }

    #[test]
    fn confirm_explicit_no() {
        let p = ScriptedPromptProvider::new(vec!["n"]);
        assert!(!p.confirm("Continue?", true).unwrap());
    }

    #[test]
    fn three_way_default_no() {
        let p = ScriptedPromptProvider::new(vec![""]);
        assert_eq!(
            p.prompt_three_way("Set up?", ThreeWayChoice::No).unwrap(),
            ThreeWayChoice::No
        );
    }

    #[test]
    fn three_way_explicit_skip() {
        let p = ScriptedPromptProvider::new(vec!["skip"]);
        assert_eq!(
            p.prompt_three_way("Set up?", ThreeWayChoice::No).unwrap(),
            ThreeWayChoice::Skip
        );
    }

    #[test]
    fn three_way_explicit_yes() {
        let p = ScriptedPromptProvider::new(vec!["y"]);
        assert_eq!(
            p.prompt_three_way("Set up?", ThreeWayChoice::No).unwrap(),
            ThreeWayChoice::Yes
        );
    }
}
