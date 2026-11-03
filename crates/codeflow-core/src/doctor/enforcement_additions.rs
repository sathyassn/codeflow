//! Inventory project additions without treating them as managed-region drift.
//! Locations identify entries; permission values and command text stay private.
//! This is an inventory, never evidence that a harness enforces an added rule.

use std::path::Path;

use serde_json::Value;

use crate::scaffold::state::{InstalledManifest, ProjectState};
use crate::scaffold::{ScaffoldError, Shown};

#[derive(Default)]
pub(super) struct Inventory {
    pub entries: Vec<String>,
    pub notes: Vec<String>,
}

pub(super) fn inventory(
    root: &Path,
    installed: &InstalledManifest,
) -> Result<Inventory, ScaffoldError> {
    let mut inventory = Inventory::default();
    if installed.files.is_empty() {
        return Ok(inventory);
    }
    for dest in [
        ".codeflow/policy.json",
        ".claude/settings.json",
        ".codex/rules/codeflow.rules",
    ] {
        let Some(file) = installed.files.get(dest) else {
            continue;
        };
        let Some(current) = read_optional(root, dest)? else {
            continue;
        };
        let Some(base) = read_optional(root, &format!(".codeflow/.baseline/{dest}"))? else {
            inventory.notes.push(format!(
                "{dest}: additions unclassified (no shipped baseline)"
            ));
            continue;
        };
        if crate::scaffold::sha256_hex(base.as_bytes()) != file.sha256 {
            return Err(ScaffoldError::invalid(
                Shown::fixed(dest),
                Shown::fixed("the shipped baseline does not match the installed record"),
            ));
        }
        if dest.ends_with(".json") {
            let base: Value = serde_json::from_str(&base)?;
            let current: Value = serde_json::from_str(&current)?;
            if !base.is_object() || !current.is_object() {
                return Err(ScaffoldError::invalid(
                    Shown::fixed(dest),
                    Shown::fixed("the enforcement file and its baseline must be JSON objects"),
                ));
            }
            let mut paths = Vec::new();
            json_additions(&base, &current, "", &mut paths);
            inventory.entries.extend(
                paths
                    .into_iter()
                    .map(|path| format!("{dest}: entry {}", crate::git::display_line(&path))),
            );
        } else if let Some(lines) = added_lines(&base, &current) {
            inventory
                .entries
                .extend(lines.into_iter().map(|line| format!("{dest}: line {line}")));
        } else if current != base {
            inventory.notes.push(format!(
                "{dest}: shipped content changed; additions not classified"
            ));
        }
    }
    inventory
        .entries
        .extend(project_rule_files(root, installed)?);

    if let Some(text) = read_optional(root, ".codeflow/project.toml")? {
        let state: ProjectState = toml::from_str(&text).map_err(|_| invalid_state())?;
        let owned = toml::Table::try_from(&state).map_err(|_| invalid_state())?;
        let fields: std::collections::BTreeMap<String, toml::Spanned<toml::Value>> =
            toml::from_str(&text).map_err(|_| invalid_state())?;
        for (key, field) in fields {
            if !owned.contains_key(&key) && !migration_key(&key) {
                let line = text[..field.span().start]
                    .bytes()
                    .filter(|b| *b == b'\n')
                    .count()
                    + 1;
                inventory.entries.push(format!(
                    ".codeflow/project.toml: entry {} (line {line})",
                    crate::git::display_line(&key)
                ));
            }
        }
    }
    Ok(inventory)
}

fn project_rule_files(
    root: &Path,
    installed: &InstalledManifest,
) -> Result<Vec<String>, ScaffoldError> {
    let directory = crate::scaffold::state::guard_beneath_root(root, Path::new(".codex/rules"))?;
    if crate::absence::proven_absent(&directory)
        .map_err(|error| ScaffoldError::io(&directory, error))?
    {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in
        std::fs::read_dir(&directory).map_err(|error| ScaffoldError::io(&directory, error))?
    {
        let entry = entry.map_err(|error| ScaffoldError::io(&directory, error))?;
        let path = entry.path();
        if path
            .extension()
            .is_none_or(|extension| extension != "rules")
        {
            continue;
        }
        let relative = path.strip_prefix(root).map_err(|_| {
            ScaffoldError::invalid(
                Shown::path(&path),
                Shown::fixed("the rules file is outside the project"),
            )
        })?;
        let relative = relative
            .components()
            .map(|component| {
                component.as_os_str().to_str().ok_or_else(|| {
                    ScaffoldError::invalid(
                        Shown::path(&path),
                        Shown::fixed("the rules path is not UTF-8"),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("/");
        if installed.files.contains_key(&relative) {
            continue;
        }
        if let Some(text) = read_optional(root, &relative)? {
            for (index, line) in text.lines().enumerate() {
                if !line.trim().is_empty() {
                    entries.push(format!(
                        "{}: line {}",
                        crate::git::display_line(&relative),
                        index + 1
                    ));
                }
            }
        }
    }
    entries.sort();
    Ok(entries)
}

fn read_optional(root: &Path, dest: &str) -> Result<Option<String>, ScaffoldError> {
    let path = crate::scaffold::state::guard_beneath_root(root, Path::new(dest))?;
    if crate::absence::proven_absent(&path).map_err(|error| ScaffoldError::io(&path, error))? {
        return Ok(None);
    }
    let bytes = crate::bounded_file::read_bounded_following(
        &path,
        crate::hooks::policy::SETTINGS_READ_LIMIT,
    )
    .map_err(|error| ScaffoldError::io(&path, error))?;
    String::from_utf8(bytes).map(Some).map_err(|_| {
        ScaffoldError::invalid(
            Shown::path(dest),
            Shown::fixed("the enforcement file is not UTF-8"),
        )
    })
}

fn invalid_state() -> ScaffoldError {
    ScaffoldError::invalid(
        Shown::fixed(".codeflow/project.toml"),
        Shown::fixed("project additions could not be read as TOML"),
    )
}

fn migration_key(key: &str) -> bool {
    matches!(
        key,
        "work_records_baseline" | "outcome_present_uids" | "outcome_required_since"
    )
}

/// JSON pointers name the exact current entry, including its array index.
/// A changed or removed shipped scalar is not an addition. Arrays report
/// only values absent from the recorded shipped array, never their contents.
fn json_additions(base: &Value, current: &Value, path: &str, out: &mut Vec<String>) {
    match (base, current) {
        (Value::Object(old), Value::Object(now)) => {
            for (key, value) in now {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                let next = format!("{path}/{escaped}");
                match old.get(key) {
                    Some(before) => json_additions(before, value, &next, out),
                    None => out.push(next),
                }
            }
        }
        (Value::Array(old), Value::Array(now)) => {
            for (index, value) in now.iter().enumerate() {
                if !old.contains(value) {
                    out.push(format!("{path}/{index}"));
                }
            }
        }
        _ => {}
    }
}

/// Recognize pure line additions without interpreting or executing Starlark.
/// Every shipped line must remain in order. A removed or changed shipped
/// line prevents the file from being described as additions only.
fn added_lines(base: &str, current: &str) -> Option<Vec<usize>> {
    let mut shipped = base.split_inclusive('\n').peekable();
    let mut additions = Vec::new();
    for (index, line) in current.split_inclusive('\n').enumerate() {
        if shipped.peek() == Some(&line) {
            shipped.next();
        } else if !line.trim().is_empty() {
            additions.push(index + 1);
        }
    }
    shipped.next().is_none().then_some(additions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_entries_are_named_without_values_or_removed_defaults() {
        let base = serde_json::json!({"permissions": {"deny": ["old"]}, "guard": true});
        let now =
            serde_json::json!({"permissions": {"deny": ["project-private-value"]}, "guard": false});
        let mut paths = Vec::new();
        json_additions(&base, &now, "", &mut paths);
        assert_eq!(paths, ["/permissions/deny/0"]);
        let mut paths = Vec::new();
        json_additions(
            &serde_json::json!({}),
            &serde_json::json!({"a/b~c": 1}),
            "",
            &mut paths,
        );
        assert_eq!(paths, ["/a~1b~0c"]);
    }

    #[test]
    fn a_changed_shipped_rule_is_never_classified_as_a_pure_addition() {
        let base = "# Rules\nprefix_rule(shipped)\n";
        assert_eq!(added_lines(base, base), Some(vec![]));
        assert_eq!(
            added_lines(base, &format!("# Project\n{base}prefix_rule(project)\n")),
            Some(vec![1, 4])
        );
        assert_eq!(added_lines(base, "# Rules\nprefix_rule(changed)\n"), None);
        assert_eq!(added_lines(base, "# Rules\n"), None);
    }
}
