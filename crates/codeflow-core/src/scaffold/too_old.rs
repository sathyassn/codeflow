//! What a reader says when the binary is older than the repository
//! (TSK-1285).
//!
//! A binary that meets a field from a newer release can only read it as an
//! error, and the person is then asked to fix findings that are not there.
//! The CI pin, `scaffold_version` in `.codeflow/project.toml`, is the one
//! marker that moves with every release, so a reader that has failed asks
//! here whether the repository declares a newer `CodeFlow`.
//!
//! A newer pin alone refuses nothing: the first pull request of the two-step
//! upgrade raises only the pin and is judged by the older binary
//! (`doctor/ci_pin.rs`). This changes what a refusal says, never whether
//! one happens, so every caller keeps its non-zero exit.

use std::fmt;
use std::path::Path;

use super::state::PROJECT_TOML;
use super::version::{is_older, pinned, shown};
use crate::hooks::landed_policy::{self, INSTALL};
use crate::remedy::{self, Remedy};

/// The rule a too-old refusal reports under. It keeps a rule id's form so the
/// pre-push hook's hand-off from `codeflow ci` carries it.
pub const RULE: &str = "git.binary_too_old";

/// What the repository's pin says about the binary running.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pin {
    /// The repository declares this version, newer than the binary.
    Newer(String),
    /// The repository declares the binary's own version or an older one.
    NotNewer,
    /// The repository declares no version that can be compared.
    Unknown,
}

/// The binary's age against one repository state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryAge {
    binary: String,
    pin: Pin,
}

/// The refusal that replaces a reader's own findings.
#[derive(Debug, Clone)]
pub struct TooOld {
    /// What is refused: both versions and what could not be read.
    pub message: String,
    /// How to upgrade.
    pub remedy: Remedy,
}

impl fmt::Display for TooOld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n  clear it: {}", self.message, &*self.remedy)
    }
}

impl BinaryAge {
    /// The age against the text of a project state, if there is one. A state
    /// that does not parse, or names no version, compares as unknown.
    #[must_use]
    pub fn from_state(state: Option<&str>, binary: &str) -> Self {
        let pin = state
            .and_then(|text| pinned(text).ok().flatten())
            .and_then(|pin| match is_older(binary, &pin) {
                Ok(true) => Some(Pin::Newer(pin)),
                Ok(false) => Some(Pin::NotNewer),
                Err(_) => None,
            })
            .unwrap_or(Pin::Unknown);
        Self {
            binary: binary.to_string(),
            pin,
        }
    }

    /// The age against the working tree. Only for a reader of that same
    /// tree: it is the requester's text, so `codeflow ci` asks
    /// [`Self::at_commit`] before it removes a finding.
    #[must_use]
    pub fn at_root(root: &Path, binary: &str) -> Self {
        let state = std::fs::read_to_string(root.join(PROJECT_TOML)).ok();
        Self::from_state(state.as_deref(), binary)
    }

    /// The age against the project state at `rev`. A state that cannot be
    /// read there compares as unknown.
    #[must_use]
    pub fn at_commit(root: &Path, rev: &str, binary: &str) -> Self {
        let state = landed_policy::project_state_text_at(root, rev)
            .ok()
            .flatten();
        Self::from_state(state.as_deref(), binary)
    }

    /// Whether the repository declares a `CodeFlow` newer than the binary.
    #[must_use]
    pub fn repository_is_newer(&self) -> bool {
        matches!(self.pin, Pin::Newer(_))
    }

    /// The refusal for a reader that could not read `unread`, when the
    /// repository declares a newer `CodeFlow`.
    #[must_use]
    pub fn too_old(&self, unread: &str) -> Option<TooOld> {
        let Pin::Newer(needed) = &self.pin else {
            return None;
        };
        let needed = shown(needed);
        Some(TooOld {
            message: format!(
                "this binary is too old: it is CodeFlow {}, this repository needs {needed} or newer (`scaffold_version` in {PROJECT_TOML}), and it could not read {unread}",
                shown(&self.binary)
            ),
            remedy: remedy::BINARY_TOO_OLD.with(&[("version", &needed), ("install", INSTALL)]),
        })
    }

    /// The line a refusal adds when the repository declares no version to
    /// compare.
    #[must_use]
    pub fn may_be_older(&self) -> Option<String> {
        (self.pin == Pin::Unknown).then(|| {
            remedy::BINARY_MAY_BE_OLDER
                .with(&[("binary", &shown(&self.binary))])
                .to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn age(state: Option<&str>) -> BinaryAge {
        BinaryAge::from_state(state, "4.0.0")
    }

    #[test]
    fn a_newer_pin_names_both_versions_and_the_upgrade() {
        let refusal = age(Some("scaffold_version = \"4.1.0\"\n"))
            .too_old("the test config")
            .unwrap()
            .to_string();
        for part in ["CodeFlow 4.0.0", "needs 4.1.0 or newer", "the test config"] {
            assert!(refusal.contains(part), "{part} in {refusal}");
        }
        assert!(refusal.contains(INSTALL), "{refusal}");
    }

    #[test]
    fn a_pin_at_or_behind_the_binary_changes_nothing() {
        for pin in ["4.0.0", "3.9.0", "4.0.0-rc.1"] {
            let age = age(Some(&format!("scaffold_version = \"{pin}\"\n")));
            assert_eq!(age.too_old("x").map(|t| t.message), None);
            assert_eq!(age.may_be_older(), None);
        }
    }

    #[test]
    fn no_comparable_pin_says_the_binary_may_be_older() {
        for state in [
            None,
            Some("tier = \"full\"\n"),
            Some("not toml ["),
            Some("scaffold_version = \"\"\n"),
            Some("scaffold_version = \"newest\"\n"),
        ] {
            let age = age(state);
            assert!(age.too_old("x").is_none(), "{state:?}");
            let line = age.may_be_older().unwrap();
            assert!(line.contains("4.0.0"), "{line}");
            assert!(line.contains("codeflow doctor"), "{line}");
        }
    }

    #[test]
    fn the_rule_survives_the_hand_off_to_the_pre_push_hook() {
        assert!(crate::hooks::is_rule_id(RULE));
    }

    /// A pin is project text: an opaque suffix never reaches the message.
    #[test]
    fn a_pin_suffix_is_not_printed() {
        let refusal = age(Some("scaffold_version = \"9.0.0+opaque-value\"\n"))
            .too_old("x")
            .unwrap()
            .to_string();
        assert!(!refusal.contains("opaque-value"), "{refusal}");
    }
}
