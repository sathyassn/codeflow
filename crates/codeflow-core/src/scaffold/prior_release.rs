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
const STAGING_POLICY: &str = include_str!("../../tests/fixtures/presets-3.0.0-staging/policy.json");

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

/// The SHA-256 of every Grok profile (`grok/sandbox.toml`) a CodeFlow build
/// has shipped, oldest first, this version's last.
///
/// - `7b0a0b6c…`: 3.0.0 through 3.1.1.
/// - `19dae22f…` and `975bbb83…`: builds of the 3.2.0 line before TSK-1057,
///   which never shipped as a release but did install the profile in the
///   repositories that ran them.
/// - `d23274b5…`: this version.
///
/// A change to the asset fails `every_follower_lists_its_shipped_bytes`
/// until its new hash is appended here; the earlier ones stay.
const GROK_PROFILE_SHA256: &[&str] = &[
    "7b0a0b6c46f9a16468d588f4db4f871302109150166bff01e644bd2c502cd686",
    "19dae22f569b22fd8f74a742e5be6d517419f32a02fdf9a3ee644bfec73a5ea1",
    "975bbb83ff7feaf639ef24bfb3f83ffb42613e50ef538f7912c136b0944bff12",
    "d23274b5b6e0635f047136d5a31621b9f9ac86e21d5f5b313b1c3470a965741c",
];

/// The SHA-256 of every version of the asset `src` that a CodeFlow build
/// has shipped, for a user-owned file that update replaces while it is
/// unmodified (`refresh_unmodified`, TSK-1057). A file whose bytes hash to
/// one of these is CodeFlow's own and carries no adopter edit. The proof is
/// this list, compiled into the binary: no file in the repository (the
/// `.codeflow/.baseline/` copy or the installed manifest's hash), which a
/// session can write, decides it. Empty for any other asset.
#[must_use]
pub fn shipped_sha256(src: &str) -> &'static [&'static str] {
    match src {
        "grok/sandbox.toml" => GROK_PROFILE_SHA256,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `refresh_unmodified` entry has its shipped versions listed, the
    /// current asset's bytes among them, so changing the asset without
    /// listing its hash fails here (TSK-1057).
    #[test]
    fn every_follower_lists_its_shipped_bytes() {
        let manifest: toml::Value = include_str!("../../../../assets/base/scaffold-manifest.toml")
            .parse()
            .unwrap();
        let followers: Vec<&str> = manifest["entry"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e.get("refresh_unmodified") == Some(&toml::Value::Boolean(true)))
            .map(|e| e["src"].as_str().unwrap())
            .collect();
        assert_eq!(followers, ["grok/sandbox.toml"]);
        let current = include_str!("../../../../assets/base/grok/sandbox.toml");
        let listed = shipped_sha256("grok/sandbox.toml");
        assert!(
            listed.contains(&super::super::hash::sha256_hex(current.as_bytes()).as_str()),
            "append the new grok/sandbox.toml hash to GROK_PROFILE_SHA256"
        );
        let release = include_str!("../../tests/fixtures/grok-3.1.1/sandbox.toml");
        assert_eq!(
            listed[0],
            super::super::hash::sha256_hex(release.as_bytes()),
            "the 3.1.1 profile"
        );
        assert!(shipped_sha256("policy.json").is_empty());
    }

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
