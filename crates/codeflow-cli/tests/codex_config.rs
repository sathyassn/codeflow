//! Structural tests for the shipped Codex config (`assets/base/codex/config.toml`).
//! ADR-0014 closes the credential-read parity gap: Codex gains an OS-sandbox
//! deny-list via a named `cf-guard` permission profile selected with
//! `default_permissions`. These tests pin that wiring the way `codex_hooks.rs`
//! pins the hook JSON — the shipped TOML, not a live Codex run: the profile is
//! selected, it extends `:workspace`, its filesystem map denies the pure-secret
//! stores, and the deliberate gh/docker tool-token carve-out is preserved.

use std::path::PathBuf;

fn shipped_config() -> toml::Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/codex/config.toml");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("codex config.toml is not valid TOML: {e}"))
}

/// The `[permissions.cf-guard.filesystem]` map, or a panic naming what is missing.
fn cf_guard_filesystem(cfg: &toml::Value) -> toml::value::Table {
    cfg.get("permissions")
        .and_then(|p| p.get("cf-guard"))
        .and_then(|g| g.get("filesystem"))
        .and_then(toml::Value::as_table)
        .cloned()
        .expect("[permissions.cf-guard.filesystem] table exists")
}

#[test]
fn default_permissions_selects_cf_guard() {
    let cfg = shipped_config();
    let selected = cfg
        .get("default_permissions")
        .and_then(toml::Value::as_str)
        .expect("default_permissions is a string");
    assert_eq!(
        selected, "cf-guard",
        "default_permissions must select the cf-guard profile, got {selected:?}"
    );
}

#[test]
fn cf_guard_profile_exists_and_extends_workspace() {
    let cfg = shipped_config();
    let profile = cfg
        .get("permissions")
        .and_then(|p| p.get("cf-guard"))
        .expect("[permissions.cf-guard] profile exists");
    let extends = profile
        .get("extends")
        .and_then(toml::Value::as_str)
        .expect("cf-guard has an `extends` key");
    // Extending :workspace = workspace-write (read anywhere, write workspace);
    // the filesystem map only subtracts the credential stores from that base.
    assert_eq!(
        extends, ":workspace",
        "cf-guard must extend the built-in :workspace profile, got {extends:?}"
    );
}

#[test]
fn cf_guard_denies_the_pure_secret_stores() {
    let cfg = shipped_config();
    let fs = cf_guard_filesystem(&cfg);
    // Representative pure-secret keys (ssh, aws) plus a .env glob — each must be
    // present and mapped to "deny" (glob keys support only "deny").
    for key in ["~/.ssh/**", "~/.aws/**", "~/**/.env"] {
        let decision = fs
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_else(|| panic!("cf-guard filesystem must contain {key:?}"));
        assert_eq!(
            decision, "deny",
            "cf-guard filesystem {key:?} must be \"deny\", got {decision:?}"
        );
    }
}

#[test]
fn cf_guard_preserves_the_gh_tool_token_carveout() {
    // The tool-token carve-out (matches the Claude nuance): this OS-sandbox deny
    // blocks ALL subprocesses, so denying ~/.config/gh would break the `gh` tool
    // reading its own token. It must stay OUT of the deny map.
    let cfg = shipped_config();
    let fs = cf_guard_filesystem(&cfg);
    for carveout in ["~/.config/gh", "~/.config/gh/**", "~/.docker/config.json"] {
        assert!(
            !fs.contains_key(carveout),
            "cf-guard must NOT deny {carveout:?} — the gh/docker tools read their own tokens"
        );
    }
}

#[test]
fn dogfood_codex_config_matches_shipped_scaffold() {
    // This repo is its own first consumer: the dogfood `.codex/config.toml` cf-guard
    // wiring must stay consistent with the scaffold it ships. Compare the parsed
    // permissions blocks (comments/headers differ, the security posture must not).
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let shipped: toml::Value = toml::from_str(
        &std::fs::read_to_string(root.join("assets/base/codex/config.toml"))
            .expect("read shipped codex config.toml"),
    )
    .expect("shipped codex config.toml parses");
    let dogfood: toml::Value = toml::from_str(
        &std::fs::read_to_string(root.join(".codex/config.toml"))
            .expect("read dogfood .codex/config.toml"),
    )
    .expect("dogfood .codex/config.toml parses");

    assert_eq!(
        dogfood.get("default_permissions"),
        shipped.get("default_permissions"),
        "dogfood default_permissions drifted from the shipped scaffold"
    );
    assert_eq!(
        dogfood.get("permissions"),
        shipped.get("permissions"),
        "dogfood [permissions.cf-guard] drifted from the shipped scaffold"
    );
}
