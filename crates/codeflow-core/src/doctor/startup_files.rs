//! The `startup-files` check (TSK-242): whether the shell startup class is
//! in the harness settings this project ships, whether its Claude settings
//! send native edits to edit-guard (TSK-1461), and which files the home's
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
use crate::security::startup::saved_state::Places;
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
        // Only a file proven absent is skipped; one the check cannot read
        // is named, never read as sourcing nothing (issue 79).
        let meta = match crate::absence::metadata_optional(&file) {
            Ok(Some(meta)) => meta,
            Ok(None) => continue,
            Err(error) => {
                found
                    .unresolved
                    .push(format!("~/{name}: cannot read: {error}"));
                continue;
            }
        };
        if !meta.is_file() || meta.len() > READ_LIMIT {
            if meta.len() > READ_LIMIT {
                found.unresolved.push(format!(
                    "~/{name}: larger than the {READ_LIMIT}-byte read limit"
                ));
            }
            continue;
        }
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(error) => {
                found
                    .unresolved
                    .push(format!("~/{name}: cannot read: {error}"));
                continue;
            }
        };
        // The shell reads lines at a newline. A line that is not text is
        // named as unresolved, never read through a lossy spelling.
        for (number, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
            let Ok(line) = std::str::from_utf8(line) else {
                // A comment line runs nothing, whatever its bytes.
                let first = line.iter().find(|byte| !matches!(byte, b' ' | b'\t'));
                if first != Some(&b'#') {
                    found
                        .unresolved
                        .push(format!("~/{name}: line {} is not UTF-8", number + 1));
                }
                continue;
            };
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
    // The shell's blanks only (issue 79): another whitespace character is
    // part of its word.
    let blank = crate::hooks::git_guard::shell_blank;
    let code = line.split(" #").next().unwrap_or(line);
    let code = if code.trim_start_matches(blank).starts_with('#') {
        ""
    } else {
        code
    };
    let mut out = Vec::new();
    for command in code.split([';', '&', '|']) {
        let mut words = command.split(blank).filter(|word| !word.is_empty());
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
/// `${HOME}/`. `None` for anything the shell fills in otherwise. Only the
/// part the file spells is read for expansions: the home is a path,
/// whatever characters it holds (a Windows `\\?\` home included).
fn literal(word: &str, home: &Path) -> Option<PathBuf> {
    let rest = word
        .strip_prefix("~/")
        .or_else(|| word.strip_prefix("$HOME/"))
        .or_else(|| word.strip_prefix("${HOME}/"));
    if rest
        .unwrap_or(word)
        .contains(['$', '`', '*', '?', '[', '{'])
    {
        return None;
    }
    match rest {
        Some(rest) => Some(home.join(rest)),
        None if word.starts_with('/') => Some(PathBuf::from(word)),
        None => None,
    }
}

/// The protected classes the presets carry, with the words a finding uses:
/// the shell startup class, the saved shell state (TSK-1461) and the
/// default git configuration files.
fn classes() -> [(&'static str, &'static actions::StartupPaths); 3] {
    let table = actions::table();
    [
        ("shell startup", &table.startup_paths),
        ("saved shell state", &table.saved_state_paths),
        ("git configuration", &table.git_config_paths),
    ]
}

/// The class rules missing from the project's Claude settings and Codex
/// profile, as listed rules. An absent or unreadable file is named, never
/// read as a pass, and the Codex profile counts only when it is the one
/// selected with no `sandbox_mode` key to shadow it.
fn missing_rules(root: &Path) -> Vec<String> {
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
                let write = &value["sandbox"]["filesystem"]["denyWrite"];
                for (label, table) in classes() {
                    let lost = table
                        .claude_edit_denies()
                        .iter()
                        .filter(|rule| !has(deny, rule))
                        .count();
                    if lost > 0 {
                        missing.push(format!("{claude_file} lacks {lost} {label} `Edit` denies"));
                    }
                    let lost = table
                        .sandbox_write_denies()
                        .iter()
                        .filter(|path| !has(write, path))
                        .count();
                    if lost > 0 {
                        missing.push(format!(
                            "{claude_file} lacks {lost} {label} sandbox `denyWrite` entries"
                        ));
                    }
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

/// The native edit tools of Claude Code that the shipped presets send to
/// edit-guard (TSK-1461 AC-11).
const CLAUDE_EDIT_TOOLS: [&str; 4] = ["Edit", "MultiEdit", "Write", "NotebookEdit"];

/// The Claude Code native edit tools that the project's
/// `.claude/settings.json` sends to no edit-guard hook, as a finding that
/// names the file; `None` when every one reaches edit-guard, or when the
/// file is absent or does not parse, which [`missing_rules`] already
/// reports. A group counts when one of its hooks runs edit-guard and its
/// matcher takes the tool: an absent, empty or `*` matcher takes every
/// tool, and any other is read as a pattern that must match the whole tool
/// name. That is the stricter of the readings a harness may apply, so a
/// matcher that passes here takes the tool under either; one that is not a
/// valid pattern takes nothing.
fn claude_edit_guard_gap(root: &Path) -> Option<String> {
    let claude_file = ".claude/settings.json";
    let text = std::fs::read_to_string(root.join(claude_file)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let groups = value["hooks"]["PreToolUse"].as_array();
    let takes = |group: &serde_json::Value, tool: &str| {
        let runs_guard = group["hooks"].as_array().is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook["command"]
                    .as_str()
                    .and_then(crate::hooks::guard_deadline::guard_of)
                    == Some("edit-guard")
            })
        });
        let matches = match group.get("matcher") {
            None | Some(serde_json::Value::Null) => true,
            Some(serde_json::Value::String(m)) if m.is_empty() || m == "*" => true,
            Some(serde_json::Value::String(m)) => {
                regex::Regex::new(&format!("^(?:{m})$")).is_ok_and(|pattern| pattern.is_match(tool))
            }
            Some(_) => false,
        };
        runs_guard && matches
    };
    let open: Vec<String> = CLAUDE_EDIT_TOOLS
        .iter()
        .filter(|tool| !groups.is_some_and(|g| g.iter().any(|group| takes(group, tool))))
        .map(|tool| format!("`{tool}`"))
        .collect();
    let (last, rest) = open.split_last()?;
    let tools = if rest.is_empty() {
        last.clone()
    } else {
        format!("{} and {last}", rest.join(", "))
    };
    Some(format!(
        "{claude_file} sends Claude Code's native {tools} calls to no edit-guard hook"
    ))
}

/// The class paths the selected Codex profile leaves writable, read
/// through its `extends` chain with the nearest profile's entry winning,
/// and any `write` grant in that chain on a path above a class entry, which
/// may reopen it (review round two).
fn codex_profile_gaps(value: &toml::Value, selected: &str) -> Vec<String> {
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
    let mut gaps = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    for (label, table) in classes() {
        let class: Vec<String> = table
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
        let open = class
            .iter()
            .filter(|path| !matches!(entry(path), Some("read" | "deny" | "none")))
            .count();
        if open > 0 {
            gaps.push(format!(
                ".codex/config.toml's `{selected}` profile leaves {open} {label} paths writable"
            ));
        }
        paths.extend(class);
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
    let home = StartupEnv::from_process().home;
    let normal = |path: &str| normal_path(path, home.as_deref());
    let class: Vec<String> = paths.iter().filter_map(|p| normal(p)).collect();
    for profile in &chain {
        let Some(fs) = filesystem(profile).and_then(toml::Value::as_table) else {
            continue;
        };
        let grants = profile_grants(profile, fs, &mut gaps);
        for (path, mode) in &grants {
            if mode != "write" {
                continue;
            }
            // A glob grants everything below its literal directory.
            let literal: String = path
                .split('/')
                .take_while(|part| !part.contains(['*', '?', '[']))
                .collect::<Vec<_>>()
                .join("/");
            let Some(grant) = normal(if literal.is_empty() { path } else { &literal })
                .filter(|_| !path.starts_with(':') && !literal.is_empty())
            else {
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
                    "the `{profile}` profile grants write on `{path}`, on, above or below shell startup or git configuration paths"
                ));
            }
        }
    }
    gaps
}

/// A Codex filesystem path as doctor compares it: `~` expanded with
/// `home`, `.` and `..` components folded, the longest existing ancestor
/// read through its symbolic links, and the result keyed as the guard's
/// class compares paths (slashes, lower case, on Windows no drive), with no
/// trailing slash, so equivalent spellings compare equal (review round
/// five); `None` for a path it cannot place (relative, or `~user`). A path
/// spelled from the root (`/etc/profile`) is placed on Windows too.
fn normal_path(path: &str, home: Option<&Path>) -> Option<String> {
    let expanded = match (path.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        (Some(_), _) => return None,
        (None, _) => PathBuf::from(path),
    };
    if !expanded.has_root() {
        return None;
    }
    let mut lexical = PathBuf::new();
    for component in expanded.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                lexical.pop();
            }
            other => lexical.push(other),
        }
    }
    let mut base = lexical.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let real = loop {
        if let Ok(real) = crate::portable_path::canonicalize(&base) {
            break rest.iter().rev().fold(real, |acc, part| acc.join(part));
        }
        match (base.file_name(), base.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                base = parent.to_path_buf();
            }
            _ => break lexical,
        }
    };
    Some(
        crate::security::startup::key(&real)
            .trim_end_matches('/')
            .to_string(),
    )
}

/// One profile's filesystem entries, direct and scoped
/// (`[filesystem."~/dir"]` with `"name" = "write"`), as one list of spelled
/// paths and modes (review round four). Settings such as
/// `glob_scan_max_depth` are not paths; a scoped entry that is not a mode
/// is a finding.
fn profile_grants(
    profile: &str,
    fs: &toml::map::Map<String, toml::Value>,
    gaps: &mut Vec<String>,
) -> Vec<(String, String)> {
    let mut grants = Vec::new();
    for (path, value) in fs {
        match value {
            toml::Value::String(mode) => grants.push((path.clone(), mode.clone())),
            toml::Value::Table(scoped) if path != ":workspace_roots" => {
                for (sub, mode) in scoped {
                    let spelled = if sub == "." {
                        path.clone()
                    } else {
                        format!("{}/{sub}", path.trim_end_matches('/'))
                    };
                    match mode.as_str() {
                        Some(mode) => grants.push((spelled, mode.to_string())),
                        None => gaps.push(format!(
                            "the `{profile}` profile has an entry under `{path}` doctor cannot read"
                        )),
                    }
                }
            }
            _ => {}
        }
    }
    grants
}

/// `ZDOTDIR`, `XDG_CONFIG_HOME`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME` or
/// `APPDATA` set away from the defaults the generated rules name, and a
/// `CLAUDE_ENV_FILE` outside the default Claude folder they name, so the
/// relocated startup files and saved shell state have no native deny; the
/// guards still follow them.
fn relocated(places: &Places) -> Vec<String> {
    let env = &places.startup;
    let Some(home) = env.home.as_deref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(z) = env.zdotdir.as_deref().filter(|z| *z != home) {
        out.push(format!("ZDOTDIR is {}", crate::git::display_path(z)));
    }
    for (name, value, default) in [
        ("XDG_CONFIG_HOME", &env.xdg_config, ".config"),
        ("CLAUDE_CONFIG_DIR", &places.claude_config, ".claude"),
        ("CODEX_HOME", &places.codex_home, ".codex"),
        ("APPDATA", &places.appdata, "AppData/Roaming"),
    ] {
        if let Some(moved) = value.as_deref().filter(|v| *v != home.join(default)) {
            out.push(format!("{name} is {}", crate::git::display_path(moved)));
        }
    }
    if let Some(file) = places
        .claude_env_file
        .as_deref()
        .filter(|file| !file.starts_with(home.join(".claude/session-env")))
    {
        out.push(format!(
            "CLAUDE_ENV_FILE is {}",
            crate::git::display_path(file)
        ));
    }
    out
}

/// `CLAUDE_CONFIG_DIR` or `CODEX_HOME` set to a folder inside the project.
/// A command that changes the project's files, such as `git checkout` or
/// `tar`, can then place a saved shell state file, so the guards refuse it
/// there; the way through is to keep the configuration folder outside the
/// project.
fn inside_project(places: &Places, root: &Path) -> Vec<String> {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    [
        ("CLAUDE_CONFIG_DIR", &places.claude_config),
        ("CODEX_HOME", &places.codex_home),
    ]
    .into_iter()
    .filter_map(|(name, value)| {
        let folder = value.as_deref()?;
        let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        folder
            .starts_with(&root)
            .then(|| format!("{name} is {}", crate::git::display_path(&folder)))
    })
    .collect()
}

pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let places = Places::from_process();
    let missing = missing_rules(&root);
    let unguarded = claude_edit_guard_gap(&root);
    let moved = relocated(&places);
    let sourced = sourced_files(&places.startup);
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
    if let Some(gap) = &unguarded {
        parts.push(format!(
            "{gap}, so such an edit of a shell startup file or saved shell state is refused only where a preset `Edit` deny names it"
        ));
        remedy.get_or_insert(remedy::DOCTOR_STARTUP_PRESETS.remedy());
    }
    if !moved.is_empty() {
        parts.push(format!(
            "{}, so the startup files and saved shell state there have no generated deny; the guards still refuse visible writes to them",
            moved.join(" and ")
        ));
        remedy.get_or_insert(remedy::DOCTOR_STARTUP_RELOCATED.remedy());
    }
    let inside = inside_project(&places, &root);
    if !inside.is_empty() {
        parts.push(format!(
            "{}, inside the project, so a command that changes the project's files, such as `git checkout` or `tar`, is refused where it can place saved shell state; keep the harness configuration folder outside the project",
            inside.join(" and ")
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
        // The startup files that hold the named lines, each once.
        let mut files: Vec<&str> = sourced
            .unprotected
            .iter()
            .chain(&sourced.unresolved)
            .map(|entry| {
                entry
                    .split_once(": ")
                    .map_or(entry.as_str(), |(file, _)| file)
            })
            .collect();
        files.sort_unstable();
        files.dedup();
        remedy.get_or_insert(remedy::DOCTOR_STARTUP_SOURCED.with(&[("path", &files.join(", "))]));
    }
    let (status, message) = match remedy {
        Some(remedy) => (Status::Warn(remedy), parts.join("; ")),
        None => (
            Status::Pass,
            "the shell startup class and the default git configuration files are in the project's Claude settings and selected Codex profile (listed rules; native enforcement is not observed here), the Claude settings send native `Edit`, `MultiEdit`, `Write` and `NotebookEdit` calls to edit-guard, and the home's startup files source nothing outside it (direct `source` lines only)".to_string(),
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
            other_homes: Vec::new(),
            zdotdir: None,
            xdg_config: None,
            etc_roots: Vec::new(),
        };
        let found = sourced_files(&env);
        assert_eq!(
            found.unprotected,
            ["~/.zshrc: $HOME/work.zsh", "~/.zshrc: /opt/tool/env.sh"]
        );
        assert_eq!(found.unresolved, ["~/.zshrc: $ZSH/oh-my-zsh.sh"]);
    }

    /// Issue 79: a `source` line that is not UTF-8 and a startup file the
    /// check cannot read are named, never read as sourcing nothing; a
    /// comment line runs nothing whatever its bytes.
    #[cfg(unix)]
    #[test]
    fn unreadable_startup_lines_and_files_are_unresolved() {
        let dir = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(
            home.join(".zshrc"),
            b"# caf\xe9\nsource ~/caf\xe9.zsh\n".as_slice(),
        )
        .unwrap();
        std::os::unix::fs::symlink(home.join("missing"), home.join(".bashrc")).unwrap();
        let env = StartupEnv {
            home: Some(home.clone()),
            other_homes: Vec::new(),
            zdotdir: None,
            xdg_config: None,
            etc_roots: Vec::new(),
        };
        let found = sourced_files(&env);
        assert!(found.unprotected.is_empty(), "{:?}", found.unprotected);
        assert!(
            found
                .unresolved
                .iter()
                .any(|entry| entry.starts_with("~/.bashrc: cannot read")),
            "{:?}",
            found.unresolved
        );
        assert!(
            found
                .unresolved
                .contains(&"~/.zshrc: line 2 is not UTF-8".to_string()),
            "{:?}",
            found.unresolved
        );
        assert_eq!(found.unresolved.len(), 2, "{:?}", found.unresolved);
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
        // Edit denies, denyWrite and cf-guard entries of each class (the
        // shell startup files, the saved shell state and the default git
        // configuration files), no profile selected, the workspace root
        // `.envrc`.
        assert_eq!(missing.len(), 11, "{missing:?}");
        assert!(
            missing
                .iter()
                .any(|m| m.contains("saved shell state `Edit` denies")),
            "{missing:?}"
        );
        assert!(
            missing
                .iter()
                .any(|m| m.contains("git configuration `Edit` denies")),
            "{missing:?}"
        );
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
        // The home doctor reads (`HOME`, else `USERPROFILE`), spelled with
        // `/` as a TOML string can hold it on every platform.
        let home = crate::portable_path::slashed(&StartupEnv::from_process().home.unwrap());
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
            // A scoped grant and a glob (review round four).
            (
                "[permissions.cf-builder.filesystem.\"~/.config/fish/conf.d\"]\n\"evil.fish\" = \"write\"",
                "grants write on `~/.config/fish/conf.d/evil.fish`",
            ),
            ("\"~/**\" = \"write\"", "grants write on `~/**`"),
            // Equivalent spellings (review round five).
            ("\"~/./.zshrc\" = \"write\"", "grants write on `~/./.zshrc`"),
            (
                "\"~/work/../.zshrc\" = \"write\"",
                "grants write on `~/work/../.zshrc`",
            ),
        ] {
            let reopened = shipped_text.replacen(
                anchor,
                // The shipped `[permissions.cf-builder.filesystem]` table
                // (TSK-1003) ends right above the anchor, so a plain grant
                // placed there joins it.
                &format!("{grant}\n\n{anchor}"),
                1,
            );
            std::fs::write(root.join(".codex/config.toml"), reopened).unwrap();
            let gaps = missing_rules(root);
            assert!(gaps.iter().any(|g| g.contains(expect)), "{grant}: {gaps:?}");
        }
        // Scoped reads and writes away from the class stay clean.
        let benign = shipped_text.replacen(
            anchor,
            &format!(
                "[permissions.cf-builder.filesystem.\"~/.config/fish/conf.d\"]\n\"x.fish\" = \"read\"\n\n[permissions.cf-builder.filesystem.\"~/./work\"]\n\"notes\" = \"write\"\n\n{anchor}"
            ),
            1,
        );
        std::fs::write(root.join(".codex/config.toml"), benign).unwrap();
        assert!(missing_rules(root).is_empty(), "{:?}", missing_rules(root));
    }

    /// TSK-1461 AC-11: Claude settings that send a native edit tool to no
    /// edit-guard hook are named, tool by tool; the shipped presets and the
    /// matchers that take all four tools pass.
    #[test]
    fn claude_settings_without_the_edit_hook_are_named() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // No file is `missing_rules`' finding, not this one.
        assert_eq!(claude_edit_guard_gap(root), None);
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        let shipped = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/settings");
        for name in ["default.json", "acceptEdits.json", "bypass-sandboxed.json"] {
            std::fs::copy(shipped.join(name), root.join(".claude/settings.json")).unwrap();
            assert_eq!(claude_edit_guard_gap(root), None, "{name}");
        }
        let guard = "codeflow hook edit-guard --contract 3 || exit 2";
        let settings = |groups: serde_json::Value| {
            std::fs::write(
                root.join(".claude/settings.json"),
                serde_json::json!({"hooks": {"PreToolUse": groups}}).to_string(),
            )
            .unwrap();
            claude_edit_guard_gap(root)
        };
        let group = |matcher: serde_json::Value, command: &str| {
            let mut group = serde_json::json!({"hooks": [{"type": "command", "command": command}]});
            if !matcher.is_null() {
                group["matcher"] = matcher;
            }
            group
        };
        let both = Some(
            ".claude/settings.json sends Claude Code's native `Edit`, `MultiEdit`, `Write` and `NotebookEdit` calls to no edit-guard hook"
                .to_string(),
        );
        assert_eq!(settings(serde_json::json!([])), both);
        std::fs::write(root.join(".claude/settings.json"), "{}").unwrap();
        assert_eq!(claude_edit_guard_gap(root), both);
        // A guard under the shell tools' matcher, or another program under
        // the edit tools' matcher, takes no edit.
        assert_eq!(
            settings(serde_json::json!([group(
                "^(Bash|PowerShell)$".into(),
                guard
            )])),
            both
        );
        assert_eq!(
            settings(serde_json::json!([group(
                "^(Edit|Write)$".into(),
                "./check.sh"
            )])),
            both
        );
        assert_eq!(
            settings(serde_json::json!([group("Edit".into(), guard)])),
            Some(
                ".claude/settings.json sends Claude Code's native `MultiEdit`, `Write` and `NotebookEdit` calls to no edit-guard hook"
                    .to_string()
            )
        );
        // The matcher part 5 first shipped leaves two tools out.
        assert_eq!(
            settings(serde_json::json!([group("^(Edit|Write)$".into(), guard)])),
            Some(
                ".claude/settings.json sends Claude Code's native `MultiEdit` and `NotebookEdit` calls to no edit-guard hook"
                    .to_string()
            )
        );
        // A pattern read as matching the whole name: `Edi` and `Writer` take
        // neither tool, and an invalid pattern takes nothing.
        for matcher in ["Edi|Writer", "(Edit"] {
            assert_eq!(
                settings(serde_json::json!([group(matcher.into(), guard)])),
                both,
                "{matcher}"
            );
        }
        for matcher in [
            serde_json::Value::Null,
            "".into(),
            "*".into(),
            "^(Edit|MultiEdit|Write|NotebookEdit)$".into(),
            "NotebookEdit|Write|MultiEdit|Edit".into(),
            ".*".into(),
        ] {
            assert_eq!(
                settings(serde_json::json!([group(matcher.clone(), guard)])),
                None,
                "{matcher}"
            );
        }
        // Two groups that together take every tool pass.
        assert_eq!(
            settings(serde_json::json!([
                group("Edit|MultiEdit".into(), guard),
                group("Write|NotebookEdit".into(), guard)
            ])),
            None
        );
    }

    #[test]
    fn relocated_startup_directories_are_reported() {
        let home = PathBuf::from("/fixture/home");
        let at_defaults = Places {
            startup: StartupEnv {
                home: Some(home.clone()),
                other_homes: Vec::new(),
                zdotdir: Some(home.clone()),
                xdg_config: Some(home.join(".config")),
                etc_roots: Vec::new(),
            },
            claude_config: Some(home.join(".claude")),
            codex_home: Some(home.join(".codex")),
            appdata: Some(home.join("AppData/Roaming")),
            claude_env_file: Some(home.join(".claude/session-env/abc/hook-1.sh")),
        };
        assert!(relocated(&at_defaults).is_empty());
        let moved = Places {
            startup: StartupEnv {
                home: Some(home),
                other_homes: Vec::new(),
                zdotdir: Some(PathBuf::from("/fixture/work/zdir")),
                xdg_config: Some(PathBuf::from("/fixture/xdg")),
                etc_roots: Vec::new(),
            },
            claude_config: Some(PathBuf::from("/fixture/claude")),
            codex_home: Some(PathBuf::from("/fixture/codex")),
            appdata: None,
            claude_env_file: Some(PathBuf::from("/fixture/env/hook-1.sh")),
        };
        let found = relocated(&moved);
        assert_eq!(found.len(), 5, "{found:?}");
        assert!(found.iter().any(|f| f.starts_with("CLAUDE_CONFIG_DIR is ")));
        assert!(found.iter().any(|f| f.starts_with("CODEX_HOME is ")));
        assert!(found.iter().any(|f| f.starts_with("CLAUDE_ENV_FILE is ")));
        // A configuration folder inside the project is reported on its own;
        // one outside it, and the default home folders, are not.
        let project = PathBuf::from("/fixture/work/project");
        assert!(inside_project(&moved, &project).is_empty());
        let inside = Places {
            claude_config: Some(project.join(".harness/claude")),
            codex_home: Some(PathBuf::from("/fixture/work/project-other/codex")),
            ..moved
        };
        assert_eq!(
            inside_project(&inside, &project),
            ["CLAUDE_CONFIG_DIR is /fixture/work/project/.harness/claude"]
        );
    }
}
