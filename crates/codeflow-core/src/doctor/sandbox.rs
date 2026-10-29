//! The `sandbox` check (TSK-397 design D, H-6): which text rule the exec
//! guard applies to each harness's calls in this project, and why.
//!
//! It decides through the function the hook uses
//! ([`crate::hooks::sandbox_state::reasons`]), for a session started at the
//! project root, once per harness, so the two cannot disagree. A tightened
//! harness fails the check, naming every trigger. Separately, it fails when
//! the policy cannot be read (the hook then refuses every call) and when
//! the state root (`CODEFLOW_HOME`) is writable from a sandboxed session.
//! A project with neither gets a note naming what no file shows (a
//! `--settings` file, server-managed settings, MDM policy, a launch flag).
//! Whether a running session's sandbox is active is not observed here.

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{CheckResult, Options, Status};
use crate::hooks::policy::TextGuard;
use crate::hooks::sandbox_state::{self, Harnesses, Host};

pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = Path::new(&opts.project_dir);
    let host = Host::for_session(root);
    let (status, message) = report(&host, opts.codeflow_home.as_deref());
    CheckResult {
        name: "sandbox".into(),
        status,
        message,
        duration: start.elapsed(),
    }
}

/// The check's status and lines for `host`, with the state root
/// `codeflow_home`.
pub(crate) fn report(host: &Host, codeflow_home: Option<&Path>) -> (Status, String) {
    let mut lines = Vec::new();
    let mut failed = false;
    let strict = match crate::hooks::landed_policy::load(&host.root) {
        Ok(authority) => authority.policy.security.text_guard == TextGuard::Strict,
        Err(error) => {
            failed = true;
            lines.push(format!(
                "policy: cannot be read ({error}); the exec guard refuses every shell call until it can"
            ));
            false
        }
    };
    for (name, harnesses) in [
        ("Claude Code", Harnesses::CLAUDE),
        ("Codex", Harnesses::CODEX),
        ("Grok Build", Harnesses::GROK),
    ] {
        let reasons = sandbox_state::reasons(host, harnesses, false, strict);
        if reasons.is_empty() {
            lines.push(format!(
                "text rule for {name} calls: baseline: no tightening trigger in the configuration read"
            ));
        } else {
            failed = true;
            lines.push(format!("text rule for {name} calls: tightened"));
            lines.extend(reasons.iter().map(|why| format!("  because {why}")));
        }
    }
    lines.push(
        "  a call marked `dangerouslyDisableSandbox` (verified on Claude Code 2.1.312) is always tightened; an unsandboxed run the harness does not mark, such as a Codex launch with `--sandbox danger-full-access`, follows the rules above"
            .to_string(),
    );
    match codeflow_home.map(|home| state_root(host, home)) {
        None => {}
        Some(Ok(None)) => {
            lines.push("state root: not writable from a sandboxed session".to_string());
        }
        Some(Ok(Some(why)) | Err(why)) => {
            failed = true;
            lines.push(format!("state root: {why}"));
        }
    }
    lines.push("hooks configured: see `codeflow doctor --check hooks`".to_string());
    let status = if failed {
        Status::Fail
    } else {
        Status::Note(crate::remedy::DOCTOR_TEXT_RULE_UNSEEN.remedy())
    };
    (status, lines.join("\n"))
}

/// Why the state root (`CODEFLOW_HOME`) is writable from a sandboxed
/// session: inside the project, or at or under an `allowWrite` entry of any
/// Claude settings layer. `Err` when a layer cannot be read.
fn state_root(host: &Host, home: &Path) -> Result<Option<String>, String> {
    let home = lexical(home);
    if home.starts_with(lexical(&host.root)) {
        return Ok(Some(format!(
            "`{}` is inside the project, which a sandboxed session can write",
            home.display()
        )));
    }
    let (entries, problems) = sandbox_state::claude_allow_write(host);
    if let Some(problem) = problems.first() {
        return Err(format!(
            "whether `{}` is writable cannot be established: {problem}",
            home.display()
        ));
    }
    for entry in entries {
        let path = match entry.strip_prefix("~/") {
            Some(rest) => match &host.home {
                Some(user) => user.join(rest),
                None => continue,
            },
            None => PathBuf::from(&entry),
        };
        if home.starts_with(lexical(&path)) {
            return Ok(Some(format!(
                "`{}` is under the sandbox write entry `{entry}`",
                home.display()
            )));
        }
    }
    Ok(None)
}

fn lexical(path: &Path) -> PathBuf {
    crate::security::outward::lexical_join(Path::new("/"), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session started at `<dir>/project`, carrying the shipped Claude
    /// preset, Codex configuration and Grok profiles.
    fn host(dir: &Path) -> Host {
        let root = dir.join("project");
        for (file, text) in [
            (".claude/settings.json", preset()),
            (
                ".codex/config.toml",
                include_str!("../../../../assets/base/codex/config.toml").to_string(),
            ),
            (
                ".grok/sandbox.toml",
                include_str!("../../../../assets/base/grok/sandbox.toml").to_string(),
            ),
        ] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        Host {
            session: Some(root.clone()),
            cwd: root.clone(),
            root,
            home: Some(dir.join("home")),
            claude_config_dir: None,
            codex_home: None,
            managed_file: Some(dir.join("managed/managed-settings.json")),
            managed_dir: None,
            native_windows: false,
        }
    }

    fn preset() -> String {
        let mut settings: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../assets/base/settings/default.json"
        ))
        .unwrap();
        crate::security::actions::table().apply_to_claude_preset(&mut settings);
        settings.to_string()
    }

    /// AC-5: the shipped configuration passes with a note; a trigger fails,
    /// naming it, on the line of the harness it applies to, and the hook's
    /// outcome for that harness agrees.
    #[test]
    fn the_lines_follow_the_configuration_per_harness() {
        let dir = tempfile::tempdir().unwrap();
        let host = host(dir.path());
        let (status, message) = report(&host, Some(&dir.path().join("home/.codeflow")));
        assert!(matches!(status, Status::Note(_)), "{status:?}: {message}");
        for name in ["Claude Code", "Codex", "Grok Build"] {
            assert!(
                message.contains(&format!("text rule for {name} calls: baseline")),
                "{message}"
            );
        }
        std::fs::create_dir_all(dir.path().join("managed")).unwrap();
        std::fs::write(
            host.managed_file.as_ref().unwrap(),
            r#"{"sandbox":{"excludedCommands":["git"]}}"#,
        )
        .unwrap();
        let (status, message) = report(&host, None);
        assert_eq!(status, Status::Fail, "{message}");
        assert!(
            message.contains("text rule for Claude Code calls: tightened"),
            "{message}"
        );
        assert!(
            message.contains("because the Claude settings list `sandbox.excludedCommands`"),
            "{message}"
        );
        assert!(
            message.contains("text rule for Codex calls: baseline"),
            "{message}"
        );
        for harnesses in [Harnesses::CLAUDE, Harnesses::CODEX, Harnesses::GROK] {
            let hook = sandbox_state::outcome(&host, harnesses, false, false);
            let shown = !sandbox_state::reasons(&host, harnesses, false, false).is_empty();
            assert_eq!(
                hook != sandbox_state::Outcome::Baseline,
                shown,
                "{harnesses:?}"
            );
        }
    }

    /// H-5: doctor fails when the state root is writable from a sandboxed
    /// session, through any settings layer, and that failure does not
    /// change the text rule it reports.
    #[test]
    fn a_writable_state_root_fails_on_its_own_line() {
        let dir = tempfile::tempdir().unwrap();
        let host = host(dir.path());
        let (status, message) = report(&host, Some(&host.root.join(".codeflow-home")));
        assert_eq!(status, Status::Fail, "{message}");
        assert!(
            message.contains("state root: `") && message.contains("inside the project"),
            "{message}"
        );
        assert!(
            message.contains("text rule for Claude Code calls: baseline"),
            "{message}"
        );
        let (status, message) = report(&host, Some(&dir.path().join("home/.codeflow/locks/x")));
        assert_eq!(status, Status::Fail, "{message}");
        assert!(message.contains("~/.codeflow/locks"), "{message}");
        // A grant in the user layer counts too.
        let user = dir.path().join("home/.claude/settings.json");
        std::fs::create_dir_all(user.parent().unwrap()).unwrap();
        let state = dir.path().join("state");
        std::fs::write(
            &user,
            serde_json::json!({"sandbox":{"filesystem":{"allowWrite":[state.display().to_string()]}}})
                .to_string(),
        )
        .unwrap();
        let (status, message) = report(&host, Some(&state));
        assert_eq!(status, Status::Fail, "{message}");
        assert!(
            message.contains("under the sandbox write entry"),
            "{message}"
        );
    }
}
