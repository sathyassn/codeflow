//! `codeflow policy <explain|show>` — the consumer-facing view of
//! `.codeflow/policy.json`, rendered from the binary alone (consumers have no
//! source to read). `explain` prints the complete key schema — type, default,
//! valid values, purpose, sharp edges — from the
//! [`policy_schema`] registry; `show`
//! prints the EFFECTIVE policy: each key's current value, whether it comes from
//! the project file or the built-in default, and a loud flag on invalid values.

use std::path::Path;

use clap::{Args, Subcommand};
use codeflow_core::hooks::policy::{Policy, PolicySource};
use codeflow_core::hooks::policy_schema::{
    self, default_policy_value, lookup, render_value, schema, KeySpec, LEVEL_LEGEND,
};

#[derive(Debug, Args)]
pub struct PolicyArgs {
    #[command(subcommand)]
    pub command: PolicyCommand,
}

#[derive(Debug, Subcommand)]
pub enum PolicyCommand {
    /// Print the complete policy.json key schema: every key's type, default,
    /// valid values, and purpose.
    Explain,
    /// Print the effective policy: each key's current value, its source
    /// (project file or built-in default), and any invalid values.
    Show,
}

/// Run `codeflow policy`; returns the process exit code.
#[must_use]
pub fn run(args: &PolicyArgs) -> i32 {
    match args.command {
        PolicyCommand::Explain => explain(),
        PolicyCommand::Show => show(&super::repo_root()),
    }
}

/// The display group a key belongs to, from its dotted path.
fn group_of(spec: &KeySpec) -> &'static str {
    match spec.path.split_once('.') {
        Some(("git", _)) => "git",
        Some(("security", _)) => "security",
        Some(("guidance", _)) => "guidance",
        _ => "top-level",
    }
}

/// `codeflow policy explain` — render the whole schema, grouped
/// top-level / git / security / guidance, defaults taken live from `Policy::default()`.
fn explain() -> i32 {
    println!(".codeflow/policy.json — every key the policy file accepts.");
    println!("A missing file or key means the built-in default applies;");
    println!("`codeflow policy show` prints the values in effect for this repo.");
    println!();
    println!("Level keys accept: off | warn | allow | block");
    for line in wrap(LEVEL_LEGEND, 74) {
        println!("  {line}");
    }

    let defaults = default_policy_value();
    let mut current_group = "";
    for spec in schema() {
        let group = group_of(spec);
        if group != current_group {
            current_group = group;
            println!("\n{group}");
        }
        let default = lookup(&defaults, spec.path).map_or_else(String::new, render_value);
        println!(
            "  {}  ({}, default: {default})",
            spec.path,
            spec.kind.type_name()
        );
        for line in wrap(spec.purpose, 70) {
            println!("      {line}");
        }
        println!("      valid: {}", spec.valid);
        if !spec.notes.is_empty() {
            for (i, line) in wrap(spec.notes, 64).into_iter().enumerate() {
                let lead = if i == 0 { "note: " } else { "      " };
                println!("      {lead}{line}");
            }
        }
    }
    0
}

/// `codeflow policy show` — the effective policy: each key's current value and
/// source, with invalid values flagged loudly. Exit 1 when the file is invalid.
fn show(root: &Path) -> i32 {
    let path = root.join(".codeflow").join("policy.json");
    let errors = match policy_schema::validate_policy(root) {
        Ok(()) => Vec::new(),
        Err(errors) => errors,
    };
    // Lenient parse for display: even an invalid file's readable values are
    // shown (flagged), so the consumer sees what they wrote, not a blank.
    let file = std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| policy_schema::parse_lenient(&data));

    match Policy::source(root) {
        PolicySource::Absent => {
            println!(
                "codeflow policy: no .codeflow/policy.json — every key is at its built-in default"
            );
        }
        PolicySource::ProjectFile => {
            println!("codeflow policy: .codeflow/policy.json is in effect");
        }
        PolicySource::MalformedFile => {
            println!("codeflow policy: .codeflow/policy.json DOES NOT PARSE — enforcement is using the built-in defaults for EVERY key");
        }
    }
    for e in &errors {
        eprintln!("codeflow policy: error: {e}");
    }

    let defaults = default_policy_value();
    let mut current_group = "";
    for spec in schema() {
        let group = group_of(spec);
        if group != current_group {
            current_group = group;
            println!("\n{group}");
        }
        let file_value = file.as_ref().and_then(|f| lookup(f, spec.path));
        let (tag, value) = match file_value {
            Some(v) if errors.iter().any(|e| e.key == spec.path) => ("INVALID", render_value(v)),
            Some(v) => ("project", render_value(v)),
            None => (
                "default",
                lookup(&defaults, spec.path).map_or_else(String::new, render_value),
            ),
        };
        println!("  {tag:>7}  {:<30}  {value}", spec.path);
    }

    if errors.is_empty() {
        0
    } else {
        eprintln!(
            "\ncodeflow policy: {} invalid finding(s) — fix the key(s) above (see `codeflow policy explain` for valid values)",
            errors.len()
        );
        1
    }
}

/// Minimal greedy word-wrap: split `text` into lines of at most `width` chars
/// (a single over-long word gets its own line). Collapses the doc-string
/// continuation whitespace.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_splits_at_width_and_collapses_whitespace() {
        let lines = wrap("aa bb   cc dd", 5);
        assert_eq!(lines, vec!["aa bb", "cc dd"]);
        let long = wrap("supercalifragilistic", 5);
        assert_eq!(long, vec!["supercalifragilistic"]);
        assert!(wrap("", 10).is_empty());
    }

    #[test]
    fn groups_cover_every_section() {
        let groups: Vec<&str> = schema().iter().map(group_of).collect();
        assert!(groups.contains(&"top-level"));
        assert!(groups.contains(&"git"));
        assert!(groups.contains(&"security"));
        assert!(groups.contains(&"guidance"));
    }
}
