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
/// # Errors
/// Refuses unreadable adoption settings or script metadata.
pub fn adopted(root: &Path) -> Result<bool, String> {
    if release_backend(root)? != ReleaseBackend::Codeflow {
        return Ok(false);
    }
    let path = root.join(SCRIPT);
    if crate::absence::proven_absent(&path).map_err(|error| {
        format!(
            "cannot inspect {}: {error}; repair the release script path",
            path.display()
        )
    })? {
        return Ok(false);
    }
    std::fs::metadata(&path)
        .map(|metadata| metadata.is_file())
        .map_err(|error| {
            format!(
                "cannot inspect {}: {error}; repair the release script path",
                path.display()
            )
        })
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
/// `remote` naming where its target's tracking refs live. `codeflow` is the
/// binary release.py reads a PR draft with: the caller passes itself, so
/// the reader is the same binary as the enforcer (TSK-147 F4).
///
/// # Errors
/// The calculator could not run or printed no result; the caller reports
/// that the check did not run.
pub fn preflight(
    root: &Path,
    head: &str,
    branch: &str,
    remote: &str,
    codeflow: &Path,
) -> Result<Preflight, String> {
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
        // An environment variable, not an argument: a project's older
        // release.py ignores it instead of refusing the call.
        .env("CODEFLOW_BIN", codeflow)
        .current_dir(root)
        .output()
        .map_err(|error| format!("{PYTHON} {SCRIPT}: {error}"))?;
    match output.status.code() {
        Some(0 | 1) => serde_json::from_slice(&output.stdout)
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

/// The diagnostic for an unsuccessful run: the script's own text (stderr,
/// else stdout) with the exit status, which is always named.
fn failure(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    let text = text.trim();
    let status = match output.status.code() {
        Some(code) => format!("{SCRIPT} exited with code {code}"),
        None => format!("{SCRIPT} exited with {}", output.status),
    };
    if text.is_empty() {
        status
    } else {
        format!("{text} ({status})")
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
        assert!(adopted(project("codeflow", Some("")).path()).unwrap());
        assert!(!adopted(project("codeflow", None).path()).unwrap());
        assert!(!adopted(project("none", Some("")).path()).unwrap());
        assert!(!adopted(project("external", Some("")).path()).unwrap());
    }

    fn python_available() -> bool {
        Command::new(PYTHON).arg("--version").output().is_ok()
    }

    #[test]
    fn preflight_reads_the_result_and_a_blocking_exit() {
        if !python_available() {
            return;
        }
        // The stand-in calculator echoes the reader it was given, so the
        // test sees the caller's binary reach it by name (TSK-147 F4).
        let script = "import json, sys\nblocked = 'break' in sys.argv\nimport os\nreader = os.environ['CODEFLOW_BIN']\nprint(json.dumps({'status': 'blocked' if blocked else 'warn', 'notes': [reader]}))\nsys.exit(1 if blocked else 0)\n";
        let dir = project("codeflow", Some(script));
        let reader = Path::new("/opt/the-running-codeflow");
        let warn = preflight(dir.path(), "HEAD", "task/x", "origin", reader).unwrap();
        assert!(!warn.blocked());
        assert_eq!(warn.notes, ["/opt/the-running-codeflow"]);
        let blocked = preflight(dir.path(), "HEAD", "break", "origin", reader).unwrap();
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
            preflight(dir.path(), "HEAD", "b", "origin", Path::new("codeflow")).unwrap_err(),
            "release error: boom"
        );
        let silent = project("codeflow", Some("print('not json')\n"));
        assert!(
            preflight(silent.path(), "HEAD", "b", "origin", Path::new("codeflow"))
                .unwrap_err()
                .contains("printed no result")
        );
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

#[cfg(all(test, unix))]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_release_adoption_refuses_unreadable_settings_and_script() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!adopted(dir.path()).unwrap());
        std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
        let project = dir.path().join(".codeflow/project.toml");
        std::fs::write(&project, "invalid = [").unwrap();
        assert!(adopted(dir.path()).is_err());
        std::fs::write(&project, "[release]\nbackend = 'codeflow'\n").unwrap();
        assert!(!adopted(dir.path()).unwrap());
        std::fs::create_dir(dir.path().join("scripts")).unwrap();
        std::os::unix::fs::symlink("missing", dir.path().join(SCRIPT)).unwrap();
        assert!(adopted(dir.path()).is_err());
    }
}
