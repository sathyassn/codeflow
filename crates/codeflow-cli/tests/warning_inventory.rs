//! Every warning and note names the step that clears it (SPC-013 R-80,
//! TSK-147 AC-1).
//!
//! A policy violation, a hook note, a validator warning and a doctor
//! warning each carry a `Remedy`, and a `Remedy` is made only from the
//! remedy catalogue. This test checks the catalogue against the real
//! commands, and scans the printing surfaces for a warning or note printed
//! as bare text around that type.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use codeflow_core::doctor::Status;
use codeflow_core::hooks::policy::PolicyLevel;
use codeflow_core::hooks::Violation;
use codeflow_core::remedy::{Finding, Remedy, Step, CATALOG};

fn codeflow() -> Command {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The `git` subcommands this machine's git knows.
fn git_commands() -> BTreeSet<String> {
    let out = Command::new("git").args(["help", "-a"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| line.starts_with("   "))
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// Bare `git` prints its usage, which lists the global options.
fn git_usage() -> String {
    let out = Command::new("git").output().unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    text
}

#[test]
fn every_catalogued_remedy_names_a_step_that_exists() {
    let git = git_commands();
    assert!(git.contains("commit"), "git help -a lists commands");
    let usage = git_usage();
    assert!(usage.contains("<command>"), "bare `git` prints its usage");
    let mut names = BTreeSet::new();
    for clearing in CATALOG {
        assert!(
            names.insert(clearing.name),
            "{} is catalogued twice",
            clearing.name
        );
        assert!(
            clearing.text.contains(clearing.step.named()),
            "{}: the text must name its step `{}`: {}",
            clearing.name,
            clearing.step.named(),
            clearing.text
        );
        match clearing.step {
            Step::Codeflow(command) => {
                let words: Vec<&str> = command.split_whitespace().collect();
                assert_eq!(words.first(), Some(&"codeflow"), "{}", clearing.name);
                let out = codeflow().args(&words[1..]).arg("--help").output().unwrap();
                assert!(
                    out.status.success(),
                    "{}: `{command}` is not in `codeflow --help`: {}",
                    clearing.name,
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            Step::Git(command) => {
                let words: Vec<&str> = command.split_whitespace().collect();
                assert_eq!(words.first(), Some(&"git"), "{}", clearing.name);
                // A global option (`git -C`) is checked against git's usage
                // line; anything else must be a listed subcommand.
                let known = words.get(1).is_some_and(|sub| {
                    if sub.starts_with('-') {
                        usage.contains(&format!("[{sub} ")) || usage.contains(&format!("[{sub}]"))
                    } else {
                        git.contains(*sub)
                    }
                });
                assert!(known, "{}: `{command}` is not a git command", clearing.name);
            }
            Step::Manual(_) => {
                assert!(
                    clearing.name.starts_with("DOCTOR_") && clearing.text.contains("cannot verify"),
                    "{}: a manual step is only for a doctor note that says doctor cannot verify it",
                    clearing.name
                );
            }
            Step::Edit(path) => {
                assert!(
                    path == "{path}"
                        || (!path.starts_with('/')
                            && !path.contains(' ')
                            && (path.contains('/') || path.contains('.'))),
                    "{}: `{path}` is not a file path",
                    clearing.name
                );
            }
        }
    }
}

/// The production part of a source file: everything before its test module.
fn production(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap();
    match text.find("\n#[cfg(test)]\nmod ") {
        Some(end) => text[..end].to_string(),
        None => text,
    }
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files.sort();
    files
}

/// The printing surfaces of R-80: `codeflow ci`, `validate --docs`,
/// `doctor`, the git hooks and the session guards.
fn surfaces() -> Vec<PathBuf> {
    let root = workspace();
    let cli = root.join("crates/codeflow-cli/src/cmd");
    let core = root.join("crates/codeflow-core/src");
    let mut files = vec![
        cli.join("ci.rs"),
        cli.join("validate.rs"),
        cli.join("doctor.rs"),
        cli.join("git_hook.rs"),
        cli.join("push_set.rs"),
        cli.join("hook.rs"),
        cli.join("mod.rs"),
        core.join("workgraph/lifecycle.rs"),
        core.join("ids/check.rs"),
    ];
    for dir in [
        cli.join("ci"),
        core.join("hooks"),
        core.join("doctor"),
        core.join("validate"),
    ] {
        files.extend(rust_files(&dir));
    }
    files
}

#[test]
fn no_warning_or_note_is_printed_as_bare_text() {
    // A literal that prints a warning or note itself bypasses the remedy.
    let words = ["warning:", "note:", "notice:", "warning —", "note —"];
    let mut found = Vec::new();
    for path in surfaces() {
        for (number, line) in production(&path).lines().enumerate() {
            let quoted = line.split('"').skip(1).step_by(2).collect::<Vec<_>>();
            if quoted
                .iter()
                .any(|text| words.iter().any(|word| text.contains(word)))
            {
                found.push(format!(
                    "{}:{}: {}",
                    path.strip_prefix(workspace()).unwrap_or(&path).display(),
                    number + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "print these through a type that carries a Remedy:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_report_carries_its_warnings_and_notes_with_a_remedy() {
    // The report types are the inventory for everything that is not a
    // policy violation: each warning or note field holds a `Finding`, and a
    // doctor warning holds its `Remedy` in the status itself. A field that
    // went back to `String` stops this test compiling.
    fn findings(_: &[Finding]) {}
    let stage = codeflow_core::hooks::git_hook::StageReport::default();
    findings(&stage.notes);
    let guard = codeflow_core::hooks::git_guard::Evaluation::default();
    findings(&guard.notes);
    let judged = codeflow_core::workgraph::lifecycle::Verdict::default();
    findings(&judged.warnings);
    findings(&judged.notices);
    let graph = codeflow_core::validate::WorkgraphValidationReport::default();
    findings(&graph.warnings);
    findings(&graph.notes);
    let docs = codeflow_core::validate::docs::DocsLintReport::default();
    findings(&docs.notes);
    findings(&codeflow_core::hooks::policy_schema::deprecation_warnings(
        Path::new("/nonexistent"),
    ));
    let warn: fn(Remedy) -> Status = Status::Warn;
    assert!(warn(codeflow_core::remedy::DOCTOR_INIT.remedy()).is_warn());
}

#[test]
fn a_policy_violation_is_built_only_with_a_catalogued_remedy() {
    // The constructor's type is the inventory for violations: a violation
    // that can print at warn takes a `Remedy`, never a bare string.
    let build: fn(&str, PolicyLevel, String, Remedy) -> Violation = Violation::new;
    let remedy = codeflow_core::remedy::TASK_STATUS.with(&[("id", "TSK-001")]);
    let violation = build("work.test", PolicyLevel::Warn, "m".into(), remedy);
    assert!(violation
        .render("ci")
        .contains("codeflow task status TSK-001"));
}

#[test]
fn every_emitted_git_rule_reads_its_own_level_key() {
    // A rule printed under one name and configured under another must map to
    // the key the project sets (TSK-147 F1), or a configured block would be
    // lowered as if unset. Each `git.` rule a violation is built with either
    // maps to a level key of the policy, or is one whose level no project key
    // sets (always-blocking, guard-only or a fixed warning).
    let fixed = [
        "git.secret_scan",
        "git.commit_to_protected",
        "git.push_to_protected",
        "git.merge_to_protected",
        "git.force_push_protected",
        "git.delete_protected",
        "git.local_ref_protection",
        "git.hook_integrity",
        "git.no_verify_bypass",
        "git.override_token_laundering",
        "git.pr_merge_delete_branch",
        "git.breaking_watch_paths",
    ];
    let mut rules = BTreeSet::new();
    for path in rust_files(&workspace().join("crates")) {
        let text = production(&path);
        for (at, _) in text.match_indices("Violation::new(") {
            let rest = text[at + "Violation::new(".len()..].trim_start();
            if let Some(rule) = rest.strip_prefix('"').and_then(|r| r.split('"').next()) {
                rules.insert(rule.to_string());
            }
        }
    }
    rules.insert(codeflow_core::hooks::git_hook::WATCHED_PATH_RULE.to_string());
    assert!(rules.contains("git.commit_ticket"), "{rules:?}");
    for rule in rules.iter().filter(|rule| rule.starts_with("git.")) {
        match codeflow_core::hooks::adjustable_key(rule) {
            Some(key) => assert!(
                codeflow_core::hooks::LEVEL_KEYS.contains(&key),
                "{rule} maps to {key}, which is not a level key"
            ),
            None => assert!(
                fixed.contains(&rule.as_str()),
                "{rule} has no level key: map it in hooks::adjustable_key or list it as fixed"
            ),
        }
    }
}
