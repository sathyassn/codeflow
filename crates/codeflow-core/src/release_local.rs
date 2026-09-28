//! `CodeFlow`'s release calculator as the local planes run it (SPC-013 R-93,
//! R-94).
//!
//! A project that adopted the calculator (`release.backend = "codeflow"` in
//! `.codeflow/project.toml`) and carries `scripts/release.py` gets two cheap
//! local checks: the pre-push push set runs `release.py preflight` for each
//! pushed branch, and `codeflow integrate` runs `release.py check-state
//! --structural` in its test stage. Both judge the tree against the recorded
//! baseline and local tags, never against the host; the pull request job's
//! full `check-pr` stays the gate. Every other project runs nothing here.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::hooks::adoption::{release_backend, ReleaseBackend};

/// The calculator a project that adopted it carries.
pub const SCRIPT: &str = "scripts/release.py";

/// The interpreter the calculator runs under.
const PYTHON: &str = "python3";

/// Whether the project at `root` adopted `CodeFlow`'s release calculator and
/// carries it.
#[must_use]
pub fn adopted(root: &Path) -> bool {
    matches!(release_backend(root), Ok(ReleaseBackend::Codeflow)) && root.join(SCRIPT).is_file()
}

/// How `release.py preflight` opens the note that reports a valid release
/// tree, a result with nothing to clear.
pub const TREE_VALID: &str = "release tree valid at ";

/// What `release.py preflight` found for one pushed branch.
#[derive(Debug, Clone, Deserialize)]
pub struct Preflight {
    /// `ok`, `warn` or `blocked`: blocked only when the push breaks a release
    /// tree its base kept valid.
    pub status: String,
    /// One line per finding, each naming what was not checked.
    #[serde(default)]
    pub notes: Vec<String>,
}

impl Preflight {
    /// Whether the push breaks the release tree.
    #[must_use]
    pub fn blocked(&self) -> bool {
        self.status == "blocked"
    }
}

/// Run `release.py preflight` for the branch `branch` pushed at `head`, with
/// `remote` naming where its target's tracking refs live.
///
/// # Errors
/// The calculator could not run or printed no result; the caller reports
/// that the check did not run.
pub fn preflight(root: &Path, head: &str, branch: &str, remote: &str) -> Result<Preflight, String> {
    let output = Command::new(PYTHON)
        .args([
            "-B",
            SCRIPT,
            "preflight",
            "--head",
            head,
            "--branch",
            branch,
            "--remote",
            remote,
        ])
        .current_dir(root)
        .output()
        .map_err(|error| format!("{PYTHON} {SCRIPT}: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    match output.status.code() {
        Some(0 | 1) => serde_json::from_str(stdout.trim())
            .map_err(|error| format!("{SCRIPT} preflight printed no result: {error}")),
        _ => Err(failure(&output)),
    }
}

/// Run `release.py check-state --structural` on `reference`.
///
/// # Errors
/// The release tree at `reference` is invalid against the recorded baseline,
/// or the calculator could not run; the message says which.
pub fn structural(root: &Path, reference: &str) -> Result<(), String> {
    let output = Command::new(PYTHON)
        .args([
            "-B",
            SCRIPT,
            "check-state",
            "--structural",
            "--ref",
            reference,
        ])
        .current_dir(root)
        .output()
        .map_err(|error| format!("{PYTHON} {SCRIPT} could not run: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(failure(&output))
    }
}

fn failure(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    let text = text.trim();
    if text.is_empty() {
        format!("{SCRIPT} exited with {}", output.status)
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_valid_tree_note_is_the_one_release_py_prints() {
        let script = include_str!("../../../scripts/release.py");
        assert!(
            script.contains(&format!("f\"{TREE_VALID}{{")),
            "release.py no longer opens its valid-tree note with {TREE_VALID:?}"
        );
    }

    fn project(backend: &str, script: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        std::fs::write(
            dir.path().join(".codeflow/project.toml"),
            format!("scaffold_version = \"3.0.0\"\n\n[release]\nbackend = \"{backend}\"\n"),
        )
        .unwrap();
        if let Some(script) = script {
            std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
            std::fs::write(dir.path().join(SCRIPT), script).unwrap();
        }
        dir
    }

    #[test]
    fn only_a_project_that_adopted_and_carries_the_calculator_runs_it() {
        assert!(adopted(project("codeflow", Some("")).path()));
        assert!(!adopted(project("codeflow", None).path()));
        assert!(!adopted(project("none", Some("")).path()));
        assert!(!adopted(project("external", Some("")).path()));
    }

    fn python_available() -> bool {
        Command::new(PYTHON).arg("--version").output().is_ok()
    }

    #[test]
    fn preflight_reads_the_result_and_a_blocking_exit() {
        if !python_available() {
            return;
        }
        let script = "import json, sys\nblocked = 'break' in sys.argv\nprint(json.dumps({'status': 'blocked' if blocked else 'warn', 'notes': ['n']}))\nsys.exit(1 if blocked else 0)\n";
        let dir = project("codeflow", Some(script));
        let warn = preflight(dir.path(), "HEAD", "task/x", "origin").unwrap();
        assert!(!warn.blocked());
        assert_eq!(warn.notes, ["n"]);
        let blocked = preflight(dir.path(), "HEAD", "break", "origin").unwrap();
        assert!(blocked.blocked());
    }

    #[test]
    fn preflight_errors_are_reported_not_passed() {
        if !python_available() {
            return;
        }
        let dir = project(
            "codeflow",
            Some("import sys\nsys.stderr.write('release error: boom')\nsys.exit(2)\n"),
        );
        assert_eq!(
            preflight(dir.path(), "HEAD", "b", "origin").unwrap_err(),
            "release error: boom"
        );
        let silent = project("codeflow", Some("print('not json')\n"));
        assert!(preflight(silent.path(), "HEAD", "b", "origin")
            .unwrap_err()
            .contains("printed no result"));
    }

    #[test]
    fn structural_passes_only_on_success() {
        if !python_available() {
            return;
        }
        let ok = project("codeflow", Some("print('{}')\n"));
        assert!(structural(ok.path(), "HEAD").is_ok());
        let bad = project(
            "codeflow",
            Some("import sys\nsys.stderr.write('release error: stamps disagree')\nsys.exit(2)\n"),
        );
        assert_eq!(
            structural(bad.path(), "HEAD").unwrap_err(),
            "release error: stamps disagree"
        );
    }
}
