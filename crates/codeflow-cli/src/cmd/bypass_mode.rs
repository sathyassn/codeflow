//! Where Claude Code's bypass permission mode can be turned off: the
//! managed settings files and the person's user file (TSK-1070, ADR-0093).
//!
//! Claude Code ignores `permissions.defaultMode: "bypassPermissions"` in a
//! project's `.claude/settings.json` and `.claude/settings.local.json`.
//! `codeflow claude-session --install` writes it, on the person's yes, into
//! Codeflow's own session file, which `claude` starts with inside a
//! Codeflow project. This module checks first that no managed or user
//! settings file sets `permissions.disableBypassPermissionsMode` to
//! `"disable"`, and refuses when one cannot be read or parsed. Server-managed
//! and MDM policy are not visible here; Claude Code still enforces them.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// The files that can turn bypass mode off.
#[derive(Debug, Clone)]
pub struct Paths {
    /// The person's Claude Code user settings file (read only).
    pub user: PathBuf,
    /// Managed settings files that can disable bypass mode, in the order
    /// they are checked.
    pub managed: Vec<PathBuf>,
}

impl Paths {
    /// The user file from `CLAUDE_CONFIG_DIR`, else `~/.claude`, and this
    /// platform's managed settings file and drop-in directory.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let config_dir = match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => home()?.join(".claude"),
        };
        Some(Self {
            user: config_dir.join("settings.json"),
            managed: managed_files(&managed_dir()),
        })
    }
}

fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The system directory Claude Code reads `managed-settings.json` from.
fn managed_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/ClaudeCode")
    } else if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\ClaudeCode")
    } else {
        PathBuf::from("/etc/claude-code")
    }
}

/// `managed-settings.json` in `dir`, then each `.json` file of its
/// `managed-settings.d` drop-in directory in name order.
#[must_use]
pub fn managed_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = vec![dir.join("managed-settings.json")];
    if let Ok(entries) = std::fs::read_dir(dir.join("managed-settings.d")) {
        let mut drop_ins: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        drop_ins.sort();
        files.extend(drop_ins);
    }
    files
}

/// Why bypass mode is not offered.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A settings file sets `disableBypassPermissionsMode: "disable"`.
    Disabled(PathBuf),
    /// A file exists but cannot be read, so the check cannot be sure.
    Unreadable(PathBuf, String),
    /// A file is not a JSON object, so it cannot be checked.
    Invalid(PathBuf, String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled(path) => write!(
                f,
                "{} sets permissions.disableBypassPermissionsMode to \"disable\"; bypass mode is turned off on this machine",
                path.display()
            ),
            Self::Unreadable(path, why) => write!(
                f,
                "cannot read {} ({why}); run this in your own terminal, outside an agent sandbox",
                path.display()
            ),
            Self::Invalid(path, why) => write!(
                f,
                "{} is not a JSON settings object ({why}); fix it first",
                path.display()
            ),
        }
    }
}

/// Read a settings file: `None` when it does not exist.
fn read_object(path: &Path) -> Result<Option<Map<String, Value>>, Refusal> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Refusal::Unreadable(path.to_path_buf(), err.to_string())),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(Some(map)),
        Ok(_) => Err(Refusal::Invalid(
            path.to_path_buf(),
            "top level is not an object".to_string(),
        )),
        Err(err) => Err(Refusal::Invalid(path.to_path_buf(), err.to_string())),
    }
}

fn disables_bypass(settings: &Map<String, Value>) -> bool {
    settings
        .get("permissions")
        .and_then(|p| p.get("disableBypassPermissionsMode"))
        .and_then(Value::as_str)
        == Some("disable")
}

/// The file that turns bypass mode off, if any. A file that exists but
/// cannot be read or parsed refuses too, since the check cannot be sure.
///
/// # Errors
///
/// Returns the refusal when a file disables bypass mode or cannot be read.
pub fn check_allowed(paths: &Paths) -> Result<(), Refusal> {
    for path in paths.managed.iter().chain(std::iter::once(&paths.user)) {
        if let Some(settings) = read_object(path)? {
            if disables_bypass(&settings) {
                return Err(Refusal::Disabled(path.clone()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(user: Option<&str>, managed: &[(&str, &str)]) -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("home/.claude");
        std::fs::create_dir_all(&claude).unwrap();
        let user_path = claude.join("settings.json");
        if let Some(text) = user {
            std::fs::write(&user_path, text).unwrap();
        }
        let managed_dir = dir.path().join("managed");
        std::fs::create_dir_all(managed_dir.join("managed-settings.d")).unwrap();
        for (name, text) in managed {
            std::fs::write(managed_dir.join(name), text).unwrap();
        }
        let paths = Paths {
            user: user_path,
            managed: managed_files(&managed_dir),
        };
        (dir, paths)
    }

    #[test]
    fn refuses_when_managed_or_user_settings_disable_bypass() {
        let disable = r#"{"permissions": {"disableBypassPermissionsMode": "disable"}}"#;
        for managed in [
            vec![("managed-settings.json", disable)],
            vec![("managed-settings.d/50-policy.json", disable)],
        ] {
            let (_dir, paths) = fixture(Some("{}"), &managed);
            let refusal = check_allowed(&paths).unwrap_err();
            assert!(matches!(refusal, Refusal::Disabled(_)), "{refusal:?}");
        }
        let (_dir, paths) = fixture(Some(disable), &[]);
        assert_eq!(
            check_allowed(&paths).unwrap_err(),
            Refusal::Disabled(paths.user.clone())
        );
    }

    #[test]
    fn refuses_an_unparsable_managed_file_or_user_file() {
        let (_dir, paths) = fixture(Some("{}"), &[("managed-settings.json", "{not json")]);
        assert!(matches!(
            check_allowed(&paths).unwrap_err(),
            Refusal::Invalid(..)
        ));
        let (_dir, paths) = fixture(Some("[]"), &[]);
        assert!(matches!(
            check_allowed(&paths).unwrap_err(),
            Refusal::Invalid(..)
        ));
        let (_dir, paths) = fixture(Some(r#"{"theme": "dark"}"#), &[]);
        assert_eq!(check_allowed(&paths), Ok(()));
        let (_dir, paths) = fixture(None, &[]);
        assert_eq!(check_allowed(&paths), Ok(()));
    }
}
