//! Copies of files an earlier version of this tool shipped, for an update that
//! finds no recorded baseline (TSK-171).
//!
//! `codeflow update` merges `.claude/settings.json` and syncs
//! `.codeflow/policy.json` against the shipped copy recorded under
//! `.codeflow/.baseline/`. When that copy is missing, the update would
//! otherwise have nothing to compare with: the 2.x `ask` entries and the old
//! `warn` defaults would stay, and the baseline it then writes would keep
//! them from ever moving. These copies stand in for the missing baseline on
//! that first run, and the update reports that it used them.
//!
//! The copies are the files already kept in the tree as test evidence: the
//! 2.1.0 release presets and policy, and the policy of the 3.0.0 staging
//! line the permission design started from.

/// The release whose shipped files stand in for a missing baseline.
pub const RELEASE: &str = "2.1.0";

const SETTINGS_DEFAULT: &str = include_str!("../../tests/fixtures/presets-2.1.0/default.json");
const SETTINGS_ACCEPT_EDITS: &str =
    include_str!("../../tests/fixtures/presets-2.1.0/acceptEdits.json");
const SETTINGS_BYPASS_SANDBOXED: &str =
    include_str!("../../tests/fixtures/presets-2.1.0/bypass-sandboxed.json");
const POLICY: &str = include_str!("../../tests/fixtures/presets-2.1.0/policy.json");
const STAGING_POLICY: &str = include_str!(
    "../../../../docs/verification/evidence/permission-presets/current/policy-asset.json"
);

/// The Claude preset the release shipped from the asset `src`
/// (`settings/default.json` and the other two), or `None` for any other
/// asset.
#[must_use]
pub fn settings(src: &str) -> Option<&'static str> {
    match src {
        "settings/default.json" => Some(SETTINGS_DEFAULT),
        "settings/acceptEdits.json" => Some(SETTINGS_ACCEPT_EDITS),
        "settings/bypass-sandboxed.json" => Some(SETTINGS_BYPASS_SANDBOXED),
        _ => None,
    }
}

/// The policies shipped before this version for the asset `src`, oldest
/// first: the release copy, whose keys decide which keys are new, then the
/// staging copy, whose defaults a project may also still hold. `None` for
/// any other asset.
#[must_use]
pub fn policies(src: &str) -> Option<[&'static str; 2]> {
    (src == "policy.json").then_some([POLICY, STAGING_POLICY])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_copy_parses_as_a_json_object() {
        let copies = ["default", "acceptEdits", "bypass-sandboxed"]
            .map(|name| settings(&format!("settings/{name}.json")).unwrap());
        for text in copies.iter().chain(policies("policy.json").unwrap().iter()) {
            let value: serde_json::Value = serde_json::from_str(text).unwrap();
            assert!(value.is_object());
        }
        assert!(settings("codex/hooks.json").is_none());
        assert!(policies("model-selection.json").is_none());
    }

    #[test]
    fn the_release_presets_carry_the_asks_this_version_retires() {
        for name in ["default", "acceptEdits", "bypass-sandboxed"] {
            let value: serde_json::Value =
                serde_json::from_str(settings(&format!("settings/{name}.json")).unwrap()).unwrap();
            let asks = value["permissions"]["ask"].as_array().unwrap();
            assert!(asks.iter().any(|a| a == "Bash(sudo *)"), "{name}");
        }
    }
}
