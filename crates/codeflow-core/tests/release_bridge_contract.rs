//! The release rule's transition tables have no other door (TSK-140 AC-14,
//! SPC-013 R-120, planning resolution 29): no CLI flag, environment
//! variable or policy key skips the rule or a table, and only the release
//! judge reads them. The scan reads every production source under
//! `crates/` and the shipped policy, and fails naming each place outside
//! the two owners that names a table, the marker, or an override.

use std::path::{Path, PathBuf};

/// The keys of the bridge: the two tables and the adoption marker.
const KEYS: [&str; 3] = [
    "release_rule_baseline",
    "release_records_baseline",
    "release_rules",
];

/// The only production sources that may name them, and why: the judge
/// that reads the tables and the marker, the project state `init` and
/// `update` write the marker through, and the remedy text of the legacy
/// notice.
const OWNERS: [(&str, &str); 5] = [
    ("codeflow-core/src/workgraph/release_line.rs", "the reader"),
    ("codeflow-core/src/scaffold/state.rs", "the marker's field"),
    ("codeflow-core/src/scaffold/init.rs", "writes the marker"),
    ("codeflow-core/src/scaffold/update.rs", "writes the marker"),
    (
        "codeflow-core/src/remedy.rs",
        "names the table in a notice's remedy",
    ),
];

/// Variables whose names say RELEASE without opening the bridge, each with
/// the one source allowed to read it and why: they choose where a release
/// is downloaded from and skip no rule, table or check.
const NOT_DOORS: [(&str, &str, &str); 1] = [(
    "CODEFLOW_RELEASE_URL",
    "codeflow-core/src/scaffold/release_pin.rs",
    "where `codeflow update --pin` downloads a release; the archives are still checked against its sha256.sum",
)];

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("source dir is readable") {
        let path = entry.expect("entry is readable").path();
        if path.is_dir() {
            let name = path.file_name().and_then(|name| name.to_str());
            if !matches!(name, Some("tests" | "target" | "node_modules")) {
                sources(&path, out);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_flag_variable_or_policy_key_opens_the_bridge() {
    let root = crates_dir();
    let mut files = Vec::new();
    for krate in [
        "codeflow-core/src",
        "codeflow-cli/src",
        "codeflow-present/src",
    ] {
        sources(&root.join(krate), &mut files);
    }
    let mut found = Vec::new();
    for file in &files {
        let relative = file
            .strip_prefix(&root)
            .expect("under crates/")
            .to_string_lossy()
            .replace('\\', "/");
        // Unit tests inside a source file are fixtures, not a door.
        let text = std::fs::read_to_string(file).expect("source is readable");
        let production = text.split("#[cfg(test)]").next().unwrap_or_default();
        for key in KEYS {
            if production.contains(key) && !OWNERS.iter().any(|(owner, _)| *owner == relative) {
                found.push(format!("{relative} names {key}"));
            }
        }
        // An override would be read from the environment by a name that
        // says so.
        for (at, _) in production.match_indices("var") {
            let call = &production[at..];
            let Some(name) = call
                .strip_prefix("var(\"")
                .or_else(|| call.strip_prefix("var_os(\""))
                .and_then(|rest| rest.split('"').next())
            else {
                continue;
            };
            let allowed = NOT_DOORS
                .iter()
                .any(|(variable, owner, _)| *variable == name && *owner == relative);
            if !allowed
                && ["RELEASE", "BASELINE", "BRIDGE"]
                    .iter()
                    .any(|word| name.contains(word))
            {
                found.push(format!("{relative} reads the variable {name}"));
            }
        }
    }
    let policy = std::fs::read_to_string(root.join("../assets/base/policy.json"))
        .expect("the shipped policy is readable");
    for key in KEYS {
        if policy.contains(key) {
            found.push(format!("assets/base/policy.json names {key}"));
        }
    }
    assert!(
        found.is_empty(),
        "the release rule's bridge has another door:\n  {}",
        found.join("\n  ")
    );
}
