//! Structural tests for the Codex configuration shipped to consuming repos.
//! The config is product behavior: it must select the guarded permission
//! profile without a legacy sandbox override, enable public research and local
//! verification, protect secret material, and route escalations through the
//! automatic reviewer.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn parse(path: &Path) -> toml::Value {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{} is not valid TOML: {e}", path.display()))
}

fn shipped_config() -> toml::Value {
    parse(&root().join("assets/base/codex/config.toml"))
}

fn cf_guard<'a>(cfg: &'a toml::Value, table: &str) -> &'a toml::value::Table {
    cfg.get("permissions")
        .and_then(|p| p.get("cf-guard"))
        .and_then(|g| g.get(table))
        .and_then(toml::Value::as_table)
        .unwrap_or_else(|| panic!("[permissions.cf-guard.{table}] table exists"))
}

#[test]
fn guarded_profile_is_the_only_sandbox_configuration() {
    let cfg = shipped_config();
    assert_eq!(cfg["default_permissions"].as_str(), Some("cf-guard"));
    assert!(
        cfg.get("sandbox_mode").is_none(),
        "legacy sandbox_mode shadows named permission profiles"
    );
    assert!(
        cfg.get("sandbox_workspace_write").is_none(),
        "legacy sandbox_workspace_write must not compete with cf-guard"
    );

    let profile = cfg["permissions"]["cf-guard"]
        .as_table()
        .expect("[permissions.cf-guard] profile exists");
    assert_eq!(profile["extends"].as_str(), Some(":workspace"));
}

#[test]
fn autonomy_and_automatic_review_are_enabled() {
    let cfg = shipped_config();
    assert_eq!(cfg["approval_policy"].as_str(), Some("never"));
    assert_eq!(cfg["approvals_reviewer"].as_str(), Some("auto_review"));
    assert_eq!(cfg["web_search"].as_str(), Some("live"));
    assert_eq!(
        cfg["model_reasoning_effort"].as_str(),
        Some("high"),
        "the primary Codex seat must not inherit an unrelated user-level effort"
    );
    assert_eq!(cfg["features"]["hooks"].as_bool(), Some(true));
    assert_eq!(
        cfg["shell_environment_policy"]["ignore_default_excludes"].as_bool(),
        Some(false),
        "the built-in KEY/SECRET/TOKEN environment scrub must stay enabled"
    );
}

#[test]
fn network_allows_public_research_and_exact_loopback_only() {
    let cfg = shipped_config();
    let network = cf_guard(&cfg, "network");
    assert_eq!(network["enabled"].as_bool(), Some(true));
    assert_eq!(
        network["mode"].as_str(),
        Some("full"),
        "full subprocess access for public research and tool traffic is intentional"
    );
    assert_eq!(network["allow_local_binding"].as_bool(), Some(false));
    assert_eq!(network["allow_upstream_proxy"].as_bool(), Some(false));
    for key in [
        "dangerously_allow_non_loopback_proxy",
        "dangerously_allow_all_unix_sockets",
    ] {
        assert_eq!(network[key].as_bool(), Some(false), "{key} must stay off");
    }

    let domains = network["domains"]
        .as_table()
        .expect("[permissions.cf-guard.network.domains] table exists");
    for host in ["*", "localhost", "127.0.0.1", "::1"] {
        assert_eq!(
            domains[host].as_str(),
            Some("allow"),
            "network rule missing {host}"
        );
    }
}

#[test]
fn cf_guard_denies_pure_secret_stores_and_workspace_env_files() {
    let cfg = shipped_config();
    let fs = cf_guard(&cfg, "filesystem");
    assert_eq!(fs["glob_scan_max_depth"].as_integer(), Some(6));
    for key in [
        "~/.ssh/**",
        "~/.aws/**",
        "~/.gnupg/**",
        "~/.netrc",
        "~/.codex/auth.json",
    ] {
        assert_eq!(fs[key].as_str(), Some("deny"), "missing deny for {key}");
    }

    let workspace = fs[":workspace_roots"]
        .as_table()
        .expect("workspace-relative filesystem table exists");
    for key in [
        "**/.env",
        "**/.env.*",
        "**/*.pem",
        "**/*.key",
        "**/*.p12",
        "**/*.pfx",
        "**/.netrc",
        "**/id_rsa*",
        "**/id_ed25519*",
    ] {
        assert_eq!(
            workspace[key].as_str(),
            Some("deny"),
            "missing workspace deny for {key}"
        );
    }
}

#[test]
fn cf_guard_preserves_authenticated_tool_carveouts() {
    let cfg = shipped_config();
    let fs = cf_guard(&cfg, "filesystem");
    for carveout in ["~/.config/gh", "~/.config/gh/**", "~/.docker/config.json"] {
        assert!(
            !fs.contains_key(carveout),
            "cf-guard must not deny {carveout:?}; the authenticated tool reads it"
        );
    }
}

#[test]
fn dogfood_codex_config_matches_the_shipped_posture() {
    let shipped = shipped_config();
    let dogfood = parse(&root().join(".codex/config.toml"));
    for key in [
        "approval_policy",
        "approvals_reviewer",
        "web_search",
        "default_permissions",
        "model_reasoning_effort",
        "features",
        "permissions",
    ] {
        assert_eq!(
            dogfood.get(key),
            shipped.get(key),
            "dogfood Codex config drifted at {key}"
        );
    }
    assert!(dogfood.get("sandbox_mode").is_none());
}

#[test]
fn customization_distinguishes_technical_permissions_from_task_authority() {
    let cfg = shipped_config();
    assert_eq!(cfg["approval_policy"].as_str(), Some("never"));

    let customize =
        std::fs::read_to_string(root().join("assets/base/agents/skills/cf-customize/SKILL.md"))
            .expect("read cf-customize");
    let normalized = customize.split_whitespace().collect::<Vec<_>>().join(" ");
    for required in [
        "no prompts/OS sandbox",
        "guards, hooks, and CI remain floors, not task authority",
        "settings alone neither authorize external communication nor protect arbitrary PII/public queries",
        "operator authority in the authenticated conversation without a global mode change",
        "approval never waives an agent-blocked floor",
        "missing required hard control withholds that risky lane, not safe work",
        "durability pushes (backup, not merge)",
    ] {
        assert!(
            normalized.contains(required),
            "customization lost permission/authority distinction: {required}"
        );
    }
}
