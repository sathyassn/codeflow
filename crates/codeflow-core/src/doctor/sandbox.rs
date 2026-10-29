//! The `sandbox` check (TSK-397 design D, H-6): which text rule the exec
//! guard applies to each harness's calls in this project, and why.
//!
//! It decides through the function the hook uses
//! ([`crate::hooks::sandbox_state::reasons`]), for a session started at the
//! project root, once per harness, so the two cannot disagree. A harness
//! this project configures (its settings file is present) fails the check
//! when its configuration is not attested, naming each reason; a harness it
//! does not configure gets a note, since its calls are tightened by design.
//! Separately, the check fails when the policy cannot be read (the hook
//! then refuses every call) and when a Claude Code grant it can read makes
//! the state root (`CODEFLOW_HOME`) writable from a sandboxed session. It
//! does not claim the state root is unwritable: Codex and Grok grants and
//! what no file shows are not computed. Grok's profile selection is read
//! from this process's environment, which may differ from a Grok session's.

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{CheckResult, Options, Status};
use crate::hooks::policy::TextGuard;
use crate::hooks::sandbox_state::{self, Harnesses, Host};

pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = Path::new(&opts.project_dir);
    let mut host = Host::for_session(root);
    if let Some(home) = &opts.harness_home {
        host.home = Some(home.clone());
        host.claude_config_dir = None;
        host.codex_home = None;
        host.grok_home = None;
    }
    host.grok_sandbox = sandbox_state::grok_selection(opts.env("GROK_SANDBOX"));
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
    let mut policy = Vec::new();
    match crate::hooks::landed_policy::load(&host.root) {
        Ok(authority) => {
            if authority.policy.security.text_guard == TextGuard::Strict {
                policy
                    .push("the project policy sets `security.text_guard` to `strict`".to_string());
            }
        }
        Err(error) => {
            failed = true;
            lines.push(format!(
                "policy: cannot be read ({error}); the exec guard refuses every shell call until it can"
            ));
        }
    }
    for (name, harnesses, file) in [
        ("Claude Code", Harnesses::CLAUDE, ".claude/settings.json"),
        ("Codex", Harnesses::CODEX, ".codex/config.toml"),
        ("Grok Build", Harnesses::GROK, ".grok/sandbox.toml"),
    ] {
        let reasons = sandbox_state::reasons(host, harnesses, false, &policy);
        if reasons.is_empty() {
            lines.push(format!(
                "text rule for {name} calls: baseline: the configuration read is attested"
            ));
            continue;
        }
        let configured = !matches!(
            crate::absence::symlink_metadata_optional(&host.root.join(file)),
            Ok(None)
        );
        if configured {
            failed = true;
            lines.push(format!("text rule for {name} calls: tightened"));
        } else {
            lines.push(format!(
                "text rule for {name} calls: tightened; this project does not configure {name} (`{file}` is absent)"
            ));
        }
        lines.extend(reasons.iter().map(|why| format!("  because {why}")));
    }
    lines.push(
        "  a call marked `dangerouslyDisableSandbox` (verified on Claude Code 2.1.312) is always tightened; launch flags and an unsandboxed run the harness does not mark are not visible here"
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
        } else if entry.starts_with('/') {
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
            (
                "../home/.codex/config.toml",
                format!(
                    "[projects.{:?}]\ntrust_level = \"trusted\"\n",
                    root.display().to_string()
                ),
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
            grok_home: None,
            grok_sandbox: Some("cf-guard".to_string()),
            managed_file: Some(dir.join("managed/managed-settings.json")),
            managed_dir: None,
            claude_unattested: Vec::new(),
            codex_unattested: Vec::new(),
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
        let (status, message) = report(&host, Some(&dir.path().join("home/.codeflow")), &[]);
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
        let (status, message) = report(&host, None, &[]);
        assert_eq!(status, Status::Fail, "{message}");
        assert!(
            message.contains("text rule for Claude Code calls: tightened"),
            "{message}"
        );
        assert!(
            message.contains("because the Claude settings")
                && message.contains("`sandbox.excludedCommands` lists commands"),
            "{message}"
        );
        assert!(
            message.contains("text rule for Codex calls: baseline"),
            "{message}"
        );
        for harnesses in [Harnesses::CLAUDE, Harnesses::CODEX, Harnesses::GROK] {
            let hook = sandbox_state::outcome(&host, harnesses, false, &[]);
            let shown = !sandbox_state::reasons(&host, harnesses, false, &[]).is_empty();
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
        let (status, message) = report(&host, Some(&host.root.join(".codeflow-home")), &[]);
        assert_eq!(status, Status::Fail, "{message}");
        assert!(
            message.contains("state root: `") && message.contains("inside the project"),
            "{message}"
        );
        assert!(
            message.contains("text rule for Claude Code calls: baseline"),
            "{message}"
        );
        let (status, message) =
            report(&host, Some(&dir.path().join("home/.codeflow/locks/x")), &[]);
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

    /// A harness the project does not configure is noted, not failed.
    #[test]
    fn an_unconfigured_harness_is_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let host = host(dir.path());
        std::fs::remove_file(host.root.join(".grok/sandbox.toml")).unwrap();
        let (status, message) = report(&host, None, &[]);
        assert!(matches!(status, Status::Note(_)), "{message}");
        assert!(
            message.contains("text rule for Grok Build calls: tightened; this project does not configure Grok Build"),
            "{message}"
        );
    }
}
