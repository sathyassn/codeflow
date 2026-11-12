//! Tracked paths keep names that the shipped Claude read denies do not hide
//! (TSK-1481 AC-5).
//!
//! A Claude session's sandbox cannot read a file whose name matches a
//! `Read(...)` deny of the shipped presets (`src/security/actions.json`).
//! `git status`, and every verb that reads a worktree's changes, then reports
//! such a tracked file as unreadable. This test lists the tracked paths with
//! `git ls-files`, reads no file contents, and fails naming each path that a
//! deny matches, with the pattern. A path is hidden when its own name or the
//! name of a folder above it matches. The `!` exceptions are ignored, because
//! TSK-1106 records that they do not take effect as permission rules.
//!
//! The patterns come from `codeflow_core::security::actions::table()`, which
//! embeds `actions.json`, and are matched with the shipped glob matcher,
//! `codeflow_core::security::pattern::match_extended_glob`. A `**/` deny is
//! matched against the path relative to the repository root. A `~/` deny
//! names a path under the home folder, which no tracked path is, so it is
//! skipped. Any other pattern form fails the test until this file handles it.

use std::path::{Path, PathBuf};

use codeflow_core::security::actions::table;
use codeflow_core::security::pattern::{match_extended_glob, GlobMatch};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// Every tracked path, relative to the repository root, with `/` separators.
fn tracked_paths(root: &Path) -> Vec<String> {
    let output = codeflow_core::git::command()
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .expect("git is available to list tracked paths");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).replace('\\', "/"))
        .collect()
}

/// The deny patterns that can name a repository path, from `Read(...)`
/// rules: `**/` patterns are kept and `~/` patterns are skipped. A rule in
/// any other form is returned as an error, so a new form is looked at.
fn repository_patterns(rules: &[String]) -> Result<Vec<String>, Vec<String>> {
    let mut patterns = Vec::new();
    let mut unhandled = Vec::new();
    for rule in rules {
        let inner = rule.strip_prefix("Read(").and_then(|r| r.strip_suffix(')'));
        match inner {
            Some(pattern) if pattern.starts_with("**/") => patterns.push(pattern.to_string()),
            Some(pattern) if pattern.starts_with("~/") => {}
            _ => unhandled.push(format!("{rule}: a read deny this check cannot match")),
        }
    }
    if unhandled.is_empty() {
        Ok(patterns)
    } else {
        Err(unhandled)
    }
}

/// One line for each path that a pattern hides: the path, the pattern and,
/// when the match is on a folder above it, that folder. A match the glob
/// matcher cannot finish is reported too, never read as no match.
fn hidden_paths(paths: &[String], patterns: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for path in paths {
        let mut prefixes: Vec<&str> = path.match_indices('/').map(|(at, _)| &path[..at]).collect();
        prefixes.push(path);
        'patterns: for pattern in patterns {
            for prefix in &prefixes {
                match match_extended_glob(prefix, pattern) {
                    GlobMatch::NoMatch => {}
                    GlobMatch::Match if *prefix == path.as_str() => {
                        found.push(format!("{path}: its name matches Read({pattern})"));
                        continue 'patterns;
                    }
                    GlobMatch::Match => {
                        found.push(format!(
                            "{path}: its folder {prefix} matches Read({pattern})"
                        ));
                        continue 'patterns;
                    }
                    GlobMatch::Unbounded(cause) => {
                        found.push(format!(
                            "{path}: cannot be checked against Read({pattern}): {cause}"
                        ));
                        continue 'patterns;
                    }
                }
            }
        }
    }
    found
}

fn shipped_patterns() -> Vec<String> {
    let patterns = repository_patterns(&table().claude_read_denies())
        .unwrap_or_else(|unhandled| panic!("{}", unhandled.join("\n")));
    assert!(
        patterns.iter().any(|p| p == "**/*secret*"),
        "the shipped read denies no longer hold **/*secret*: {patterns:?}"
    );
    patterns
}

#[test]
fn tracked_paths_have_names_the_shipped_read_denies_do_not_hide() {
    let root = repo_root();
    let paths = tracked_paths(&root);
    assert!(!paths.is_empty(), "git ls-files listed no tracked paths");
    let hidden = hidden_paths(&paths, &shipped_patterns());
    assert!(
        hidden.is_empty(),
        "tracked paths that a Claude session's sandbox cannot read; \
         rename each so no shipped Read deny matches it:\n  {}",
        hidden.join("\n  ")
    );
}

#[test]
fn a_planted_secret_named_path_is_refused_with_its_pattern() {
    let paths = vec!["src/lib.rs".to_string(), "tests/x_secret_y.rs".to_string()];
    let hidden = hidden_paths(&paths, &shipped_patterns());
    assert_eq!(
        hidden,
        vec!["tests/x_secret_y.rs: its name matches Read(**/*secret*)".to_string()]
    );
}

#[test]
fn the_exceptions_reopen_nothing() {
    // `!**/*secret*.rs` and `!**/.env.example` are ignored.
    let paths = vec![
        "a/b_secret.rs".to_string(),
        ".env.example".to_string(),
        "deploy/credentials.md".to_string(),
    ];
    let hidden = hidden_paths(&paths, &shipped_patterns());
    assert_eq!(
        hidden,
        vec![
            "a/b_secret.rs: its name matches Read(**/*secret*)".to_string(),
            ".env.example: its name matches Read(**/.env.*)".to_string(),
            "deploy/credentials.md: its name matches Read(**/*credentials*)".to_string(),
        ]
    );
}

#[test]
fn a_folder_name_hides_the_paths_below_it() {
    let paths = vec!["fixtures/secret/data.txt".to_string()];
    let hidden = hidden_paths(&paths, &["**/*secret*".to_string()]);
    assert_eq!(
        hidden,
        vec![
            "fixtures/secret/data.txt: its folder fixtures/secret matches Read(**/*secret*)"
                .to_string()
        ]
    );
}

#[test]
fn ordinary_names_pass() {
    let paths = vec![
        "crates/codeflow-cli/tests/ci_cli/leak_scan_exemption.rs".to_string(),
        "crates/codeflow-cli/tests/refusal_journey/guarded_read_paths.rs".to_string(),
        "crates/codeflow-core/tests/managed_leak_scan.rs".to_string(),
        "docs/keys.md".to_string(),
        ".envrc.md".to_string(),
    ];
    assert_eq!(
        hidden_paths(&paths, &shipped_patterns()),
        Vec::<String>::new()
    );
}

#[test]
fn home_denies_are_skipped_and_other_forms_are_refused() {
    let rules = vec![
        "Read(**/*.pem)".to_string(),
        "Read(~/.ssh/**)".to_string(),
        "Read(/build/**)".to_string(),
        "Read(!**/*secret*.rs)".to_string(),
        "Edit(**/x)".to_string(),
    ];
    assert_eq!(
        repository_patterns(&rules),
        Err(vec![
            "Read(/build/**): a read deny this check cannot match".to_string(),
            "Read(!**/*secret*.rs): a read deny this check cannot match".to_string(),
            "Edit(**/x): a read deny this check cannot match".to_string(),
        ])
    );
    assert_eq!(
        repository_patterns(&rules[..2]),
        Ok(vec!["**/*.pem".to_string()])
    );
}
