//! The `sandbox` check (TSK-397 design D, H-6): which text rule the exec
//! guard applies on this host and why.
//!
//! It reads the same harness configuration the guard reads
//! ([`crate::hooks::sandbox_state`]) and reports three lines: the
//! configuration it inspected, the text rule that configuration selects,
//! and where the hook wiring is checked. A trigger fails the check, naming
//! it; a host with none gets a note naming what no file shows (a
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
    let host = Host::from_process(root);
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
    let mut failures = sandbox_state::triggers(host, Harnesses::ALL);
    if let Some(home) = codeflow_home {
        if let Some(why) = home_in_write_set(host, home) {
            failures.push(why);
        }
    }
    let strict = match crate::hooks::landed_policy::load(&host.root) {
        Ok(authority) => Ok(authority.policy.security.text_guard == TextGuard::Strict),
        Err(error) => Err(error),
    };
    let mut lines = Vec::new();
    if failures.is_empty() {
        lines.push(
            "configuration inspected: the readable Claude, Codex and Grok settings show the sandbox enabled, failing closed and carrying the shipped floor"
                .to_string(),
        );
    } else {
        lines.extend(
            failures
                .iter()
                .map(|trigger| format!("configuration inspected: {trigger}")),
        );
    }
    let rule = match (&strict, failures.first()) {
        (Ok(true), _) => {
            "tightened: the project policy sets `security.text_guard` to `strict`".to_string()
        }
        (_, Some(first)) => format!("tightened: the strict rule is added because {first}"),
        (Err(error), None) => format!(
            "baseline, unless the policy that cannot be read ({error}) sets `security.text_guard` to `strict`"
        ),
        (Ok(false), None) => "baseline: the line's rule".to_string(),
    };
    lines.push(format!("text rule on this host: {rule}"));
    lines.push(
        "  a call marked `dangerouslyDisableSandbox` (verified on Claude Code 2.1.312) is always tightened; an unsandboxed run the harness does not mark, such as a Codex launch with `--sandbox danger-full-access`, follows the rule above"
            .to_string(),
    );
    lines.push("hooks configured: see `codeflow doctor --check hooks`".to_string());
    let status = if failures.is_empty() {
        Status::Note(crate::remedy::DOCTOR_TEXT_RULE_UNSEEN.remedy())
    } else {
        Status::Fail
    };
    (status, lines.join("\n"))
}

/// Why the state root (`CODEFLOW_HOME`) is writable from a sandboxed session: inside
/// the project, or at or under a `sandbox.filesystem.allowWrite` entry.
fn home_in_write_set(host: &Host, home: &Path) -> Option<String> {
    let home = lexical(home);
    if home.starts_with(lexical(&host.root)) {
        return Some(format!(
            "the CodeFlow state root `{}` is inside the project, which a sandboxed session can write",
            home.display()
        ));
    }
    let settings = host.root.join(".claude/settings.json");
    let value: serde_json::Value = std::fs::read_to_string(settings)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())?;
    let entries = value
        .pointer("/sandbox/filesystem/allowWrite")?
        .as_array()?;
    for entry in entries.iter().filter_map(serde_json::Value::as_str) {
        let path = match entry.strip_prefix("~/") {
            Some(rest) => host.home.as_ref()?.join(rest),
            None => PathBuf::from(entry),
        };
        if home.starts_with(lexical(&path)) {
            return Some(format!(
                "the CodeFlow state root `{}` is under the sandbox write entry `{entry}`",
                home.display()
            ));
        }
    }
    None
}

fn lexical(path: &Path) -> PathBuf {
    crate::security::outward::lexical_join(Path::new("/"), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(dir: &Path) -> Host {
        Host {
            root: dir.join("project"),
            main_root: Ok(None),
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

    /// AC-5: the shipped preset passes with a note; each trigger fails,
    /// naming it; the text rule line names the outcome.
    #[test]
    fn the_three_lines_follow_the_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let host = host(dir.path());
        std::fs::create_dir_all(host.root.join(".claude")).unwrap();
        std::fs::write(host.root.join(".claude/settings.json"), preset()).unwrap();
        let (status, message) = report(&host, Some(&dir.path().join("home/.codeflow")));
        assert!(matches!(status, Status::Note(_)), "{status:?}: {message}");
        assert!(
            message.contains("text rule on this host: baseline"),
            "{message}"
        );
        assert!(message.contains("hooks configured"), "{message}");
        std::fs::create_dir_all(dir.path().join("managed")).unwrap();
        std::fs::write(
            host.managed_file.as_ref().unwrap(),
            r#"{"sandbox":{"excludedCommands":["git"]}}"#,
        )
        .unwrap();
        let (status, message) = report(&host, None);
        assert_eq!(status, Status::Fail, "{message}");
        assert!(
            message.contains(
                "configuration inspected: the Claude settings list `sandbox.excludedCommands`"
            ),
            "{message}"
        );
        assert!(
            message.contains("text rule on this host: tightened"),
            "{message}"
        );
    }

    /// H-5: doctor fails when the state root is writable from a sandboxed
    /// session.
    #[test]
    fn a_writable_state_root_fails() {
        let dir = tempfile::tempdir().unwrap();
        let host = host(dir.path());
        std::fs::create_dir_all(host.root.join(".claude")).unwrap();
        std::fs::write(host.root.join(".claude/settings.json"), preset()).unwrap();
        let (status, message) = report(&host, Some(&host.root.join(".codeflow-home")));
        assert_eq!(status, Status::Fail, "{message}");
        assert!(message.contains("inside the project"), "{message}");
        let (status, message) = report(&host, Some(&dir.path().join("home/.codeflow/locks/x")));
        assert_eq!(status, Status::Fail, "{message}");
        assert!(message.contains("~/.codeflow/locks"), "{message}");
    }
}
