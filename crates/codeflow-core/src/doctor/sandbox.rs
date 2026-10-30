//! The `sandbox` check (TSK-397, H-6): which text rule the exec guard
//! applies to shell calls in this project, and whether the state root is
//! writable from a sandboxed Claude Code session.
//!
//! The text rule is decided by the function the hook uses
//! ([`crate::hooks::sandbox_state::reasons`]), for a session started at the
//! project root, so the two cannot disagree. Strict is the default and
//! needs nothing. Relaxed is printed as the operator's choice, with the
//! ref it was read from; nothing here verifies a sandbox. The check fails
//! when the policy cannot be read (the hook then refuses every call) and
//! when a Claude Code grant it can read makes the state root
//! (`CODEFLOW_HOME`) writable from a sandboxed session. It does not claim
//! the state root is unwritable: Codex and Grok grants and what no file
//! shows are not computed.

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{CheckResult, Options, Status};
use crate::hooks::sandbox_state::{self, Host};

pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = Path::new(&opts.project_dir);
    let mut host = Host::for_session(root);
    if let Some(home) = &opts.harness_home {
        host.home = Some(home.clone());
        host.claude_config_dir = None;
    }
    let (status, message) = report(&host, opts.codeflow_home.as_deref(), &temporary_dirs());
    CheckResult {
        name: "sandbox".into(),
        status,
        message,
        duration: start.elapsed(),
    }
}

/// The temporary directories a sandboxed Claude Code session can write:
/// this process's, `/tmp` and, on macOS, the per-user folders. A session's
/// own may differ, so the list is broad.
fn temporary_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![std::env::temp_dir(), PathBuf::from("/tmp")];
    if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/private/tmp"));
        dirs.push(PathBuf::from("/var/folders"));
        dirs.push(PathBuf::from("/private/var/folders"));
    }
    dirs
}

/// The check's status and lines for `host`, with the state root
/// `codeflow_home` and the temporary directories `temporary`.
pub(crate) fn report(
    host: &Host,
    codeflow_home: Option<&Path>,
    temporary: &[PathBuf],
) -> (Status, String) {
    let mut lines = Vec::new();
    let mut failed = false;
    match crate::hooks::landed_policy::load(&host.root) {
        Ok(authority) => {
            let policy: Vec<String> =
                sandbox_state::policy_reason(&authority, "the project policy")
                    .into_iter()
                    .collect();
            let reasons = sandbox_state::reasons(host, false, &policy);
            let integrity = authority.policy.git.hook_integrity;
            if reasons.is_empty() {
                lines.push(format!(
                    "text rule: relaxed by operator choice, not verified (`security.text_guard: relaxed` read from {}, the remote-tracking refs as last fetched; a process that can write the repository's `.git` can set those refs, and the OS sandbox is not checked)",
                    authority.source
                ));
                lines.push(format!(
                    "  the line's interpreter-argument findings take `git.hook_integrity: {integrity}`"
                ));
            } else {
                lines.push("text rule: strict".to_string());
                lines.extend(reasons.iter().map(|why| format!("  because {why}")));
                lines.push(format!(
                    "  the strict rule's own refusals always block; the line's interpreter-argument findings take `git.hook_integrity: {integrity}`"
                ));
            }
        }
        Err(error) => {
            failed = true;
            lines.push(format!(
                "policy: cannot be read ({error}); the exec guard refuses every shell call until it can, except one plain `git fetch` of a configured remote, and the literal worktree recovery commands when the repository itself cannot be read"
            ));
        }
    }
    lines.push(
        "  a recognized marked retry (`dangerouslyDisableSandbox`, verified on Claude Code 2.1.312) always takes the strict rule; a harness that sends no such field is judged by the rule above; a Claude Code call also needs `relaxed` in its session project's policy"
            .to_string(),
    );
    if let Some(home) = codeflow_home {
        match state_root(host, home, temporary) {
            Ok(None) => lines.push(format!(
                "state root: no Claude Code write grant read here reaches `{}` (the project, the temporary directory, and `allowWrite` and `additionalDirectories` in every settings layer); Codex and Grok grants are not checked",
                home.display()
            )),
            Ok(Some(why)) | Err(why) => {
                failed = true;
                lines.push(format!("state root: {why}"));
            }
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
/// Claude Code session: inside the project, under a temporary directory
/// the sandbox lets a session write, or at or under an `allowWrite` or
/// `additionalDirectories` entry of any layer. `Err` when a layer cannot be
/// read or an entry is relative, so its origin cannot be established.
fn state_root(host: &Host, home: &Path, temporary: &[PathBuf]) -> Result<Option<String>, String> {
    let home = lexical(home);
    if home.starts_with(lexical(&host.root)) {
        return Ok(Some(format!(
            "`{}` is inside the project, which a sandboxed session can write",
            home.display()
        )));
    }
    for dir in temporary {
        if home.starts_with(lexical(dir)) {
            return Ok(Some(format!(
                "`{}` is under the temporary directory `{}`, which a sandboxed session can write",
                home.display(),
                dir.display()
            )));
        }
    }
    let (grants, problems) = sandbox_state::claude_write_grants(host);
    if let Some(problem) = problems.first() {
        return Err(format!(
            "whether `{}` is writable cannot be established: {problem}",
            home.display()
        ));
    }
    for (entry, file) in grants {
        let path = if let Some(rest) = entry.strip_prefix("~/") {
            match &host.home {
                Some(user) => user.join(rest),
                None => {
                    return Err(format!(
                        "whether `{}` is writable cannot be established: the home directory for `{entry}` in {file} is unknown",
                        home.display()
                    ))
                }
            }
        } else if let Some(rest) = entry.strip_prefix("//") {
            Path::new("/").join(rest)
        } else if entry.starts_with('/') || Path::new(&entry).is_absolute() {
            // A Windows drive path is absolute there too.
            PathBuf::from(&entry)
        } else {
            return Err(format!(
                "whether `{}` is writable cannot be established: the write grant `{entry}` in {file} is relative",
                home.display()
            ));
        };
        if home.starts_with(lexical(&path)) {
            return Ok(Some(format!(
                "`{}` is under the sandbox write grant `{entry}` in {file}",
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

    /// A session started at `<dir>/project`, a repository with the shipped
    /// Claude preset committed and no remote.
    fn host(dir: &Path) -> Host {
        let root = dir.join("project");
        let path = root.join(".claude/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, preset()).unwrap();
        git2::Repository::init(&root).unwrap();
        Host {
            session: Some(root.clone()),
            cwd: root.clone(),
            root,
            home: Some(dir.join("home")),
            claude_config_dir: None,
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

    /// AC-5: with no relaxed choice the line reads strict and names why,
    /// and the hook's function agrees; a relaxed working copy is not
    /// honoured.
    #[test]
    fn the_text_rule_is_strict_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let host = host(dir.path());
        let (status, message) = report(&host, None, &[]);
        assert!(matches!(status, Status::Note(_)), "{status:?}: {message}");
        assert!(
            message.contains("text rule: strict")
                && message.contains("keeps the default `security.text_guard: strict`"),
            "{message}"
        );
        let policy = host.root.join(".codeflow/policy.json");
        std::fs::create_dir_all(policy.parent().unwrap()).unwrap();
        std::fs::write(&policy, r#"{"security":{"text_guard":"relaxed"}}"#).unwrap();
        let (_, message) = report(&host, None, &[]);
        assert!(
            message.contains("text rule: strict") && message.contains("not honoured"),
            "{message}"
        );
        assert!(!message.contains("relaxed by operator choice"), "{message}");
    }

    /// H-5: doctor fails when the state root is writable from a sandboxed
    /// session, through any settings layer.
    #[test]
    fn a_writable_state_root_fails_on_its_own_line() {
        let dir = tempfile::tempdir().unwrap();
        let host = host(dir.path());
        let (status, message) = report(&host, Some(&host.root.join(".codeflow-home")), &[]);
        assert_eq!(status, Status::Fail, "{message}");
        assert!(
            message.contains("state root: `") && message.contains("inside the project"),
            "{message}"
        );
        assert!(message.contains("text rule: strict"), "{message}");
        let (status, message) =
            report(&host, Some(&dir.path().join("home/.codeflow/locks/x")), &[]);
        assert_eq!(status, Status::Fail, "{message}");
        assert!(message.contains("~/.codeflow/locks"), "{message}");
        // A grant in the user layer counts.
        let user = dir.path().join("home/.claude/settings.json");
        std::fs::create_dir_all(user.parent().unwrap()).unwrap();
        let state = dir.path().join("state");
        std::fs::write(
            &user,
            serde_json::json!({"sandbox":{"filesystem":{"allowWrite":[state.display().to_string()]}}})
                .to_string(),
        )
        .unwrap();
        let (status, message) = report(&host, Some(&state), &[]);
        assert_eq!(status, Status::Fail, "{message}");
        assert!(
            message.contains("under the sandbox write grant"),
            "{message}"
        );
        // An additional directory, a relative grant and the temporary
        // directory each count; a state root reached by none is not
        // claimed unwritable.
        let extra = dir.path().join("extra");
        std::fs::write(
            &user,
            serde_json::json!({"permissions":{"additionalDirectories":[extra.display().to_string()]}})
                .to_string(),
        )
        .unwrap();
        let (status, message) = report(&host, Some(&extra.join("state")), &[]);
        assert_eq!(status, Status::Fail, "{message}");
        std::fs::write(
            &user,
            serde_json::json!({"sandbox":{"filesystem":{"allowWrite":["state"]}}}).to_string(),
        )
        .unwrap();
        let (status, message) = report(&host, Some(&state), &[]);
        assert_eq!(status, Status::Fail, "{message}");
        assert!(message.contains("is relative"), "{message}");
        std::fs::remove_file(&user).unwrap();
        let (status, message) = report(&host, Some(&state), &[dir.path().to_path_buf()]);
        assert_eq!(status, Status::Fail, "{message}");
        assert!(message.contains("temporary directory"), "{message}");
        let (status, message) = report(&host, Some(&state), &[]);
        assert!(matches!(status, Status::Note(_)), "{message}");
        assert!(
            message.contains("no Claude Code write grant read here reaches")
                && message.contains("Codex and Grok grants are not checked")
                && !message.contains("not writable"),
            "{message}"
        );
    }
}
