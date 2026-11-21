//! Every pasteable command that carries a remote name, a ref or a `--base`
//! or `--into` value is rendered through `codeflow_core::git::pasted`, which
//! takes the repository root and refuses a value holding a remote name
//! codeflow withholds.
//!
//! This test scans the production source of `codeflow-core` and
//! `codeflow-cli` for a backticked `git` or `codeflow` command written as a
//! format literal with an interpolation, such as `` `git fetch {remote}` ``.
//! Such a literal prints its value without passing the renderer, so each one
//! must be listed in [`ALLOWED`] with the reason its value is never a remote
//! name or ref from outside: a record id, a check name, a SHA, a guard
//! description of the refused command's own subcommand, or a value already
//! rendered by `pasted`. A new unlisted literal fails the test, and so does
//! an entry that no longer matches any line.
//!
//! Limits: the scan reads one line at a time, so a command split across
//! lines, a command built with `push_str`, or one without backticks is not
//! seen. Test modules (`tests.rs`, `*_tests.rs` and the code after an inline
//! `#[cfg(test)] mod tests`) and comments are skipped.

use std::path::{Path, PathBuf};

use regex::Regex;

/// (file under `crates/`, text the line contains, why the value is safe).
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "codeflow-cli/src/cmd/ids.rs",
        "`codeflow ids retarget {}`",
        "record id",
    ),
    (
        "codeflow-cli/src/cmd/new.rs",
        "`codeflow task new --resume {id}`",
        "record id",
    ),
    (
        "codeflow-cli/src/cmd/work.rs",
        "`codeflow work start {task_id}`",
        "record id",
    ),
    (
        "codeflow-core/src/doctor/ci_pin.rs",
        "`codeflow update {}`",
        "pin_option renders through pasted",
    ),
    (
        "codeflow-core/src/doctor/guard_timeouts.rs",
        "`codeflow doctor --check {NAME}`",
        "constant check name",
    ),
    (
        "codeflow-core/src/doctor/mod.rs",
        "`codeflow doctor --check {name}`",
        "check name",
    ),
    (
        "codeflow-core/src/doctor/remote_perimeter.rs",
        "`codeflow remote protect --profile {}`",
        "profile enum name",
    ),
    (
        "codeflow-core/src/git/remote_query.rs",
        "`git fetch {}` failed",
        "the fetch URL through display_url (credential stripped when it has a scheme), describing a failed run; a URL, never a remote name or ref",
    ),
    (
        "codeflow-core/src/hooks/git_guard.rs",
        "`git {verb}`",
        "guard description of the refused subcommand",
    ),
    (
        "codeflow-core/src/hooks/git_guard.rs",
        "`git {sub}`",
        "guard description of the refused subcommand",
    ),
    (
        "codeflow-core/src/hooks/git_guard.rs",
        "`git {sub} --no-verify`",
        "guard description of the refused subcommand",
    ),
    (
        "codeflow-core/src/hooks/git_guard.rs",
        "`git {}` removes the integrity path",
        "guard description of the refused subcommand",
    ),
    (
        "codeflow-core/src/hooks/git_guard/path_writers.rs",
        "`git {verb}`",
        "guard description of the refused subcommand",
    ),
    (
        "codeflow-core/src/hooks/git_guard/path_writers.rs",
        "`git sparse-checkout {sub}`",
        "guard description of the refused subcommand",
    ),
    (
        "codeflow-core/src/hooks/git_guard/runners.rs",
        "`git {verb}`",
        "guard description of the refused subcommand",
    ),
    (
        "codeflow-core/src/ids/check.rs",
        "`codeflow ids retarget {id}`",
        "record id",
    ),
    (
        "codeflow-core/src/ids/check.rs",
        "`codeflow ids admit {}`",
        "record uid",
    ),
    (
        "codeflow-core/src/ids/check.rs",
        "`codeflow ids retarget {}`",
        "record id",
    ),
    (
        "codeflow-core/src/ids/issue.rs",
        "`codeflow ids retarget {}`",
        "record id",
    ),
    (
        "codeflow-core/src/ids/ledger.rs",
        "`codeflow ids restore {id}`",
        "record id",
    ),
    (
        "codeflow-core/src/remedy.rs",
        "`codeflow task status {id}",
        "record id",
    ),
    (
        "codeflow-core/src/remedy.rs",
        "`codeflow epic status {id}",
        "record id",
    ),
    (
        "codeflow-core/src/remedy.rs",
        "`codeflow spec status {id}",
        "record id",
    ),
    (
        "codeflow-core/src/remedy.rs",
        "`codeflow work start {id}`",
        "record id",
    ),
    (
        "codeflow-core/src/remedy.rs",
        "`git switch {root}`",
        "filled with a pasted::word value",
    ),
    (
        "codeflow-core/src/remedy.rs",
        "`codeflow ci {args}`",
        "filled from push_set::check_remedy through pasted",
    ),
    (
        "codeflow-core/src/remedy.rs",
        "`git config core.hooksPath {hooks}`",
        "hooks directory path",
    ),
    (
        "codeflow-core/src/remedy.rs",
        "`codeflow doctor --check {check}`",
        "check name",
    ),
    (
        "codeflow-core/src/remedy.rs",
        "`codeflow update {pin_option}`",
        "filled from ci_pin::pin_option through pasted",
    ),
    (
        "codeflow-core/src/remedy.rs",
        "`codeflow hook {guard}`",
        "guard name",
    ),
    (
        "codeflow-core/src/root_checkout.rs",
        "`git {sub}` would commit",
        "guard description of the refused subcommand",
    ),
    (
        "codeflow-core/src/security/startup.rs",
        "`git -c {shown}`",
        "guard description of the refused config option",
    ),
    (
        "codeflow-core/src/security/startup.rs",
        "`git --config-env {shown}`",
        "guard description of the refused config option",
    ),
    (
        "codeflow-core/src/workgraph/amendment.rs",
        "`codeflow task status {task} cancelled",
        "record id",
    ),
    (
        "codeflow-core/src/workgraph/claim_base.rs",
        "`codeflow work claim {task}`",
        "record id",
    ),
    (
        "codeflow-core/src/workgraph/release_line.rs",
        "`git cat-file -p {id}`",
        "object id",
    ),
    (
        "codeflow-core/src/remedy.rs",
        "`git merge -- \\\"{tip}\\\"`",
        "the target tip SHA from ci.rs target_containment_warning",
    ),
    (
        "codeflow-cli/src/cmd/push_set.rs",
        "`codeflow ci --base {base} --head {}`",
        "the merge-base SHA and the pushed SHA",
    ),
];

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_name().unwrap().to_string_lossy();
            if name != "tests.rs" && !name.ends_with("_tests.rs") {
                out.push(path);
            }
        }
    }
}

/// Production lines holding a backticked command with an interpolation.
fn command_literals() -> Vec<(String, usize, String)> {
    let pattern = Regex::new(r"`(git|codeflow) [^`]*\{[^{}`]*\}").unwrap();
    let root = crates_dir();
    let mut files = Vec::new();
    for krate in ["codeflow-core/src", "codeflow-cli/src"] {
        sources(&root.join(krate), &mut files);
    }
    let mut found = Vec::new();
    for path in files {
        let file = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&path).unwrap();
        let mut previous = "";
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("mod tests {") && previous == "#[cfg(test)]" {
                break;
            }
            previous = trimmed;
            if trimmed.starts_with("//") {
                continue;
            }
            if pattern.is_match(line) {
                found.push((file.clone(), index + 1, trimmed.to_string()));
            }
        }
    }
    found
}

#[test]
fn every_printed_command_literal_with_a_value_is_listed() {
    let found = command_literals();
    let unlisted: Vec<String> = found
        .iter()
        .filter(|(file, _, line)| {
            !ALLOWED
                .iter()
                .any(|(f, needle, _)| f == file && line.contains(needle))
        })
        .map(|(file, number, line)| format!("{file}:{number}: {line}"))
        .collect();
    assert!(
        unlisted.is_empty(),
        "a backticked command prints a value without codeflow_core::git::pasted; render it through pasted with the repository root, or list it in ALLOWED with the reason its value is never a remote name or ref:\n{}",
        unlisted.join("\n")
    );
    let stale: Vec<String> = ALLOWED
        .iter()
        .filter(|(f, needle, _)| {
            !found
                .iter()
                .any(|(file, _, line)| f == file && line.contains(needle))
        })
        .map(|(f, needle, _)| format!("{f}: {needle}"))
        .collect();
    assert!(
        stale.is_empty(),
        "ALLOWED entries match no line; remove them:\n{}",
        stale.join("\n")
    );
}

#[test]
fn the_scan_sees_a_command_literal_that_bypasses_pasted() {
    let pattern = Regex::new(r"`(git|codeflow) [^`]*\{[^{}`]*\}").unwrap();
    assert!(pattern.is_match("format!(\"run `git fetch {remote} {branch}`\")"));
    assert!(pattern.is_match("\"`codeflow ci --base {base}`\""));
    assert!(!pattern.is_match("format!(\"run {}\", step(&command))"));
}
