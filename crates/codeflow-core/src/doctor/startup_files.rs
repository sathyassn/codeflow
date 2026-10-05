//! The `startup-files` check (TSK-242): whether the shell startup class is
//! in the harness settings this project ships, and which files the home's
//! startup files source from outside the class.
//!
//! The settings are read as listed rules; whether a harness enforces them
//! natively is not something this check can observe. Sourced files are
//! found by reading each home startup file once (bounded, never followed):
//! a `source` or `.` line with a literal path outside the class is a file
//! an agent could plant in that no rule protects, and a line whose path
//! the check cannot read is reported as not resolved.

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{CheckResult, Options, Status};
use crate::remedy;
use crate::security::actions;
use crate::security::startup::{class_target, StartupEnv};

/// The home startup files read for `source` lines.
const READ: &[&str] = &[
    ".bashrc",
    ".bash_profile",
    ".profile",
    ".zshenv",
    ".zprofile",
    ".zshrc",
];

/// The most bytes read from one startup file.
const READ_LIMIT: u64 = 256 * 1024;

/// What a startup file's `source` lines name.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Sourced {
    /// Literal paths outside the class, as `file: path`.
    pub(crate) unprotected: Vec<String>,
    /// Lines whose path the check cannot read, as `file: word`.
    pub(crate) unresolved: Vec<String>,
}

/// The `source` and `.` lines of the home's startup files.
pub(crate) fn sourced_files(env: &StartupEnv) -> Sourced {
    let mut found = Sourced::default();
    let Some(home) = env.home.as_deref() else {
        return found;
    };
    for name in READ {
        let file = home.join(name);
        let Ok(meta) = std::fs::metadata(&file) else {
            continue;
        };
        if !meta.is_file() || meta.len() > READ_LIMIT {
            if meta.len() > READ_LIMIT {
                found.unresolved.push(format!(
                    "~/{name}: larger than the {READ_LIMIT}-byte read limit"
                ));
            }
            continue;
        }
        let Ok(bytes) = std::fs::read(&file) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            for word in source_operands(line) {
                match literal(&word, home) {
                    Some(path) => {
                        if class_target(&path, env).is_none() {
                            found.unprotected.push(format!("~/{name}: {word}"));
                        }
                    }
                    None => found.unresolved.push(format!("~/{name}: {word}")),
                }
            }
        }
    }
    found
}

/// The operand of each `source FILE` or `. FILE` command on a line,
/// quotes removed. A comment ends the line.
fn source_operands(line: &str) -> Vec<String> {
    let code = line.split(" #").next().unwrap_or(line);
    let code = if code.trim_start().starts_with('#') {
        ""
    } else {
        code
    };
    let mut out = Vec::new();
    for command in code.split([';', '&', '|']) {
        let mut words = command.split_whitespace();
        let mut first = words.next();
        while matches!(first, Some("then" | "do" | "else" | "{" | "!")) {
            first = words.next();
        }
        if matches!(first, Some("source" | ".")) {
            if let Some(operand) = words.next() {
                out.push(operand.trim_matches(['"', '\'']).to_string());
            }
        }
    }
    out
}

/// The path a literal operand names: absolute, `~/`, `$HOME/` or
/// `${HOME}/`. `None` for anything the shell fills in otherwise.
fn literal(word: &str, home: &Path) -> Option<PathBuf> {
    let rest = word
        .strip_prefix("~/")
        .or_else(|| word.strip_prefix("$HOME/"))
        .or_else(|| word.strip_prefix("${HOME}/"));
    let path = match rest {
        Some(rest) => home.join(rest),
        None if word.starts_with('/') => PathBuf::from(word),
        None => return None,
    };
    let text = path.to_string_lossy();
    (!text.contains(['$', '`', '*', '?', '[', '{'])).then_some(path)
}

/// The class rules missing from the project's Claude settings and Codex
/// profile, as listed rules.
fn missing_rules(root: &Path) -> Vec<String> {
    let table = &actions::table().startup_paths;
    let mut missing = Vec::new();
    let claude = root.join(".claude/settings.json");
    if let Ok(text) = std::fs::read_to_string(&claude) {
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value) => {
                let has = |list: &serde_json::Value, rule: &str| {
                    list.as_array()
                        .is_some_and(|a| a.iter().any(|r| r.as_str() == Some(rule)))
                };
                let deny = &value["permissions"]["deny"];
                let lost = table
                    .claude_edit_denies()
                    .iter()
                    .filter(|rule| !has(deny, rule))
                    .count();
                if lost > 0 {
                    missing.push(format!(
                        ".claude/settings.json lacks {lost} shell startup `Edit` denies"
                    ));
                }
                let write = &value["sandbox"]["filesystem"]["denyWrite"];
                let lost = table
                    .sandbox_write_denies()
                    .iter()
                    .filter(|path| !has(write, path))
                    .count();
                if lost > 0 {
                    missing.push(format!(
                        ".claude/settings.json lacks {lost} shell startup sandbox `denyWrite` entries"
                    ));
                }
            }
            Err(_) => missing.push(".claude/settings.json does not parse".to_string()),
        }
    }
    let codex = root.join(".codex/config.toml");
    if let Ok(text) = std::fs::read_to_string(&codex) {
        match text.parse::<toml::Value>() {
            Ok(value) => {
                let fs = value
                    .get("permissions")
                    .and_then(|p| p.get("cf-guard"))
                    .and_then(|p| p.get("filesystem"));
                let lost = table
                    .home
                    .iter()
                    .map(|e| format!("~/{}", e.trim_end_matches('/')))
                    .chain(
                        table
                            .absolute
                            .iter()
                            .map(|e| e.trim_end_matches('/').to_string()),
                    )
                    .filter(|path| {
                        fs.and_then(|fs| fs.get(path)).and_then(toml::Value::as_str) != Some("read")
                    })
                    .count();
                if lost > 0 {
                    missing.push(format!(
                        ".codex/config.toml's cf-guard profile leaves {lost} shell startup paths writable"
                    ));
                }
            }
            Err(_) => missing.push(".codex/config.toml does not parse".to_string()),
        }
    }
    missing
}

pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let env = StartupEnv::from_process();
    let missing = missing_rules(&root);
    let sourced = sourced_files(&env);
    let (status, message) = if !missing.is_empty() {
        (
            Status::Warn(remedy::DOCTOR_STARTUP_PRESETS.remedy()),
            format!(
                "{}; the harness sandboxes do not deny every shell startup write (listed rules; native enforcement is not observed here)",
                missing.join("; ")
            ),
        )
    } else if !sourced.unprotected.is_empty() || !sourced.unresolved.is_empty() {
        let mut parts = Vec::new();
        if !sourced.unprotected.is_empty() {
            parts.push(format!(
                "sourced files outside the protected class: {}",
                sourced.unprotected.join(", ")
            ));
        }
        if !sourced.unresolved.is_empty() {
            parts.push(format!(
                "sourced files not resolved: {}",
                sourced.unresolved.join(", ")
            ));
        }
        (
            Status::Warn(remedy::DOCTOR_STARTUP_SOURCED.remedy()),
            format!(
                "{}; an agent could plant a definition there that no rule or sandbox entry protects",
                parts.join("; ")
            ),
        )
    } else {
        (
            Status::Pass,
            "the shell startup class is in the project's harness settings (listed rules), and the home's startup files source nothing outside it".to_string(),
        )
    };
    CheckResult {
        name: "startup-files".into(),
        status,
        message,
        duration: start.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sourced_files_outside_the_class_are_listed_and_variables_unresolved() {
        let dir = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(
            home.join(".zshrc"),
            "source ~/.zsh/aliases.zsh\n. $HOME/work.zsh\nsource \"$ZSH/oh-my-zsh.sh\"\n# source ~/commented.sh\nif true; then . /opt/tool/env.sh; fi\n",
        )
        .unwrap();
        let env = StartupEnv {
            home: Some(home.clone()),
            zdotdir: None,
            xdg_config: None,
        };
        let found = sourced_files(&env);
        assert_eq!(
            found.unprotected,
            ["~/.zshrc: $HOME/work.zsh", "~/.zshrc: /opt/tool/env.sh"]
        );
        assert_eq!(found.unresolved, ["~/.zshrc: $ZSH/oh-my-zsh.sh"]);
    }

    #[test]
    fn missing_rules_name_each_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::fs::write(root.join(".claude/settings.json"), "{}").unwrap();
        std::fs::write(root.join(".codex/config.toml"), "").unwrap();
        let missing = missing_rules(root);
        assert_eq!(missing.len(), 3, "{missing:?}");
        let shipped = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/base");
        std::fs::copy(
            shipped.join("settings/default.json"),
            root.join(".claude/settings.json"),
        )
        .unwrap();
        std::fs::copy(
            shipped.join("codex/config.toml"),
            root.join(".codex/config.toml"),
        )
        .unwrap();
        assert!(missing_rules(root).is_empty(), "{:?}", missing_rules(root));
    }
}
