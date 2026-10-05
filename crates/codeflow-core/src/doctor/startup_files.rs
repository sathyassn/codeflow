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
/// profile, as listed rules. An absent or unreadable file is named, never
/// read as a pass, and the Codex profile counts only when it is the one
/// selected with no `sandbox_mode` key to shadow it.
fn missing_rules(root: &Path) -> Vec<String> {
    let table = &actions::table().startup_paths;
    let mut missing = Vec::new();
    let claude_file = ".claude/settings.json";
    match std::fs::read_to_string(root.join(claude_file)) {
        Err(_) => missing.push(format!(
            "{claude_file} is absent or unreadable, so no Claude rule from this project protects the class"
        )),
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
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
                        "{claude_file} lacks {lost} shell startup `Edit` denies"
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
                        "{claude_file} lacks {lost} shell startup sandbox `denyWrite` entries"
                    ));
                }
            }
            Err(_) => missing.push(format!("{claude_file} does not parse")),
        },
    }
    let codex_file = ".codex/config.toml";
    match std::fs::read_to_string(root.join(codex_file)) {
        Err(_) => missing.push(format!(
            "{codex_file} is absent or unreadable, so no Codex profile from this project protects the class"
        )),
        Ok(text) => match text.parse::<toml::Value>() {
            Ok(value) => {
                let selected = value
                    .get("default_permissions")
                    .and_then(toml::Value::as_str);
                if !matches!(selected, Some("cf-guard" | "cf-builder")) {
                    missing.push(format!(
                        "{codex_file} selects `{}`, not `cf-guard` or `cf-builder`",
                        selected.unwrap_or("no profile")
                    ));
                }
                if value.get("sandbox_mode").is_some() {
                    missing.push(format!(
                        "{codex_file} sets `sandbox_mode`, which shadows the profile"
                    ));
                }
                // The selected profile, and the builder profile that builder
                // seats select at launch (review round two).
                // Another selection is already a finding above.
                let mut profiles = match selected {
                    Some(name @ ("cf-guard" | "cf-builder")) => vec![name],
                    _ => vec!["cf-guard"],
                };
                let has_builder = value
                    .get("permissions")
                    .and_then(|p| p.get("cf-builder"))
                    .is_some();
                if has_builder && !profiles.contains(&"cf-builder") {
                    profiles.push("cf-builder");
                }
                for profile in profiles {
                    missing.extend(codex_profile_gaps(&value, profile));
                }
            }
            Err(_) => missing.push(format!("{codex_file} does not parse")),
        },
    }
    missing
}

/// The class paths the selected Codex profile leaves writable, read
/// through its `extends` chain with the nearest profile's entry winning,
/// and any `write` grant in that chain on a path above a class entry, which
/// may reopen it (review round two).
fn codex_profile_gaps(value: &toml::Value, selected: &str) -> Vec<String> {
    let table = &actions::table().startup_paths;
    let profiles = value.get("permissions");
    let mut chain = Vec::new();
    let mut name = selected.to_string();
    while !name.starts_with(':') && !chain.contains(&name) && chain.len() < 8 {
        let Some(profile) = profiles.and_then(|p| p.get(&name)) else {
            break;
        };
        chain.push(name.clone());
        match profile.get("extends").and_then(toml::Value::as_str) {
            Some(parent) => name = parent.to_string(),
            None => break,
        }
    }
    let filesystem = |profile: &str| {
        profiles
            .and_then(|p| p.get(profile))
            .and_then(|p| p.get("filesystem"))
    };
    let entry = |path: &str| {
        chain
            .iter()
            .find_map(|profile| filesystem(profile).and_then(|fs| fs.get(path)))
            .and_then(toml::Value::as_str)
    };
    let paths: Vec<String> = table
        .home
        .iter()
        .map(|e| format!("~/{}", e.trim_end_matches('/')))
        .chain(
            table
                .absolute
                .iter()
                .map(|e| e.trim_end_matches('/').to_string()),
        )
        .collect();
    let mut gaps = Vec::new();
    let open = paths
        .iter()
        .filter(|path| !matches!(entry(path), Some("read" | "deny" | "none")))
        .count();
    if open > 0 {
        gaps.push(format!(
            ".codex/config.toml's `{selected}` profile leaves {open} shell startup paths writable"
        ));
    }
    let roots = chain.iter().find_map(|profile| {
        filesystem(profile)
            .and_then(|fs| fs.get(":workspace_roots"))
            .and_then(|r| r.get(".envrc"))
    });
    if roots.and_then(toml::Value::as_str) != Some("read") {
        gaps.push(format!(
            ".codex/config.toml's `{selected}` profile leaves the workspace root's `.envrc` writable"
        ));
    }
    // A write grant on, above or below a class path, in any spelling, can
    // reopen it: Codex applies narrower grants, and a write wins over a
    // read at the same path (review rounds two and three).
    let home = std::env::var("HOME").ok();
    let normal = |path: &str| -> Option<String> {
        let expanded = match (path.strip_prefix('~'), &home) {
            (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
                format!("{}{rest}", home.trim_end_matches('/'))
            }
            (Some(_), _) => return None,
            (None, _) => path.to_string(),
        };
        expanded
            .starts_with('/')
            .then(|| expanded.trim_end_matches('/').to_lowercase())
    };
    let class: Vec<String> = paths.iter().filter_map(|p| normal(p)).collect();
    for profile in &chain {
        let Some(fs) = filesystem(profile).and_then(toml::Value::as_table) else {
            continue;
        };
        for (path, mode) in fs {
            if mode.as_str() != Some("write") || path.starts_with(':') {
                continue;
            }
            let Some(grant) = normal(path) else {
                gaps.push(format!(
                    "the `{profile}` profile grants write on `{path}`, which doctor cannot place"
                ));
                continue;
            };
            let near = |a: &str, b: &str| a == b || a.starts_with(&format!("{b}/"));
            if class
                .iter()
                .any(|c| near(c, &grant) || near(&grant, c) || grant.is_empty())
            {
                gaps.push(format!(
                    "the `{profile}` profile grants write on `{path}`, on, above or below shell startup paths"
                ));
            }
        }
    }
    gaps
}

/// `ZDOTDIR` or `XDG_CONFIG_HOME` set away from the defaults the generated
/// rules name, so the relocated startup files have no native deny; the
/// guards still follow them.
fn relocated(env: &StartupEnv) -> Vec<String> {
    let Some(home) = env.home.as_deref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(z) = env.zdotdir.as_deref().filter(|z| *z != home) {
        out.push(format!("ZDOTDIR is {}", z.display()));
    }
    if let Some(x) = env
        .xdg_config
        .as_deref()
        .filter(|x| *x != home.join(".config"))
    {
        out.push(format!("XDG_CONFIG_HOME is {}", x.display()));
    }
    out
}

pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let env = StartupEnv::from_process();
    let missing = missing_rules(&root);
    let moved = relocated(&env);
    let sourced = sourced_files(&env);
    // Every finding is reported together; the remedy is the first one's.
    let mut parts = Vec::new();
    let mut remedy = None;
    if !missing.is_empty() {
        parts.push(format!(
            "{}; the harness sandboxes do not deny every shell startup write (listed rules; native enforcement is not observed here)",
            missing.join("; ")
        ));
        remedy.get_or_insert(remedy::DOCTOR_STARTUP_PRESETS.remedy());
    }
    if !moved.is_empty() {
        parts.push(format!(
            "{}, so the startup files there have no generated deny; the guards still refuse visible writes to them",
            moved.join(" and ")
        ));
        remedy.get_or_insert(remedy::DOCTOR_STARTUP_RELOCATED.remedy());
    }
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
    if !sourced.unprotected.is_empty() || !sourced.unresolved.is_empty() {
        parts.push(
            "an agent could plant a definition in a sourced file that no rule or sandbox entry protects"
                .to_string(),
        );
        remedy.get_or_insert(remedy::DOCTOR_STARTUP_SOURCED.remedy());
    }
    let (status, message) = match remedy {
        Some(remedy) => (Status::Warn(remedy), parts.join("; ")),
        None => (
            Status::Pass,
            "the shell startup class is in the project's Claude settings and selected Codex profile (listed rules; native enforcement is not observed here), and the home's startup files source nothing outside it (direct `source` lines only)".to_string(),
        ),
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
        // Absent files are findings, never a pass (review round one).
        let missing = missing_rules(root);
        assert_eq!(missing.len(), 2, "{missing:?}");
        assert!(missing.iter().all(|m| m.contains("absent")), "{missing:?}");
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::fs::write(root.join(".claude/settings.json"), "{}").unwrap();
        std::fs::write(root.join(".codex/config.toml"), "").unwrap();
        let missing = missing_rules(root);
        // Edit denies, denyWrite, no profile selected, cf-guard
        // entries, the workspace root `.envrc`.
        assert_eq!(missing.len(), 5, "{missing:?}");
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
        // The shipped profile's entries do not count when another profile,
        // or a `sandbox_mode` key, is what Codex runs.
        let shipped_text = std::fs::read_to_string(root.join(".codex/config.toml")).unwrap();
        for (from, to) in [
            (
                "default_permissions = \"cf-guard\"",
                "default_permissions = \":danger-full-access\"",
            ),
            (
                "default_permissions = \"cf-guard\"",
                "default_permissions = \"cf-guard\"\nsandbox_mode = \"danger-full-access\"",
            ),
        ] {
            assert!(shipped_text.contains(from));
            std::fs::write(
                root.join(".codex/config.toml"),
                shipped_text.replacen(from, to, 1),
            )
            .unwrap();
            assert_eq!(missing_rules(root).len(), 1, "{to}");
        }
        // A builder override that reopens a startup file, and a write grant
        // above one, are reported even with cf-guard selected, since builder
        // seats select cf-builder at launch (review round two).
        let anchor = "[permissions.cf-builder.filesystem.\":workspace_roots\"]";
        assert!(shipped_text.contains(anchor));
        let home = std::env::var("HOME").unwrap();
        let absolute = format!("\"{home}/.zshrc\" = \"write\"");
        let absolute_expect = format!("grants write on `{home}/.zshrc`");
        for (grant, expect) in [
            ("\"~/.zshrc\" = \"write\"", "`cf-builder` profile leaves 1"),
            ("\"~/.config\" = \"write\"", "grants write on `~/.config`"),
            ("\"~\" = \"write\"", "grants write on `~`"),
            // Below a class directory, and the home spelled absolutely
            // (review round three).
            (
                "\"~/.config/fish/conf.d/evil.fish\" = \"write\"",
                "grants write on `~/.config/fish/conf.d/evil.fish`",
            ),
            (absolute.as_str(), absolute_expect.as_str()),
            ("\"relative/x\" = \"write\"", "doctor cannot place"),
        ] {
            let reopened = shipped_text.replacen(
                anchor,
                &format!("[permissions.cf-builder.filesystem]\n{grant}\n\n{anchor}"),
                1,
            );
            std::fs::write(root.join(".codex/config.toml"), reopened).unwrap();
            let gaps = missing_rules(root);
            assert!(gaps.iter().any(|g| g.contains(expect)), "{grant}: {gaps:?}");
        }
    }

    #[test]
    fn relocated_startup_directories_are_reported() {
        let home = PathBuf::from("/fixture/home");
        let at_defaults = StartupEnv {
            home: Some(home.clone()),
            zdotdir: Some(home.clone()),
            xdg_config: Some(home.join(".config")),
        };
        assert!(relocated(&at_defaults).is_empty());
        let moved = StartupEnv {
            home: Some(home),
            zdotdir: Some(PathBuf::from("/fixture/work/zdir")),
            xdg_config: Some(PathBuf::from("/fixture/xdg")),
        };
        assert_eq!(relocated(&moved).len(), 2);
    }
}
