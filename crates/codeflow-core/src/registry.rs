//! User-level cross-repo state under `~/.codeflow/` (charter §7 layer 3, §8).
//!
//! Two files live here:
//!
//! - `registry.json` — the flock-guarded registry of initialized repos
//!   (path, name, tier, scaffold version, last activity). Upserted by a
//!   cheap [`touch_registry`] call the CLI makes on every command run
//!   inside an initialized repo. A view, not a system: no daemon, lazy
//!   sync at query time.
//! - `config.toml` — user defaults, read if present, never required.
//!
//! The home directory honors the `CODEFLOW_HOME` environment override so
//! tests (and unusual setups) can redirect all user-level state.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::file_lock::{locked_read, locked_rmw_typed};

/// Registry schema version written by this binary.
pub const REGISTRY_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Home resolution
// ---------------------------------------------------------------------------

/// Resolve the user-level codeflow home directory.
///
/// Order: `CODEFLOW_HOME` (if set and non-empty) → `$HOME/.codeflow`
/// (or `%USERPROFILE%\.codeflow` on Windows). Returns `None` when no home
/// directory can be determined.
#[must_use]
pub fn codeflow_home() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("CODEFLOW_HOME") {
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(|h| PathBuf::from(h).join(".codeflow"))
}

/// Path of `registry.json` under a codeflow home.
#[must_use]
pub fn registry_path(home: &Path) -> PathBuf {
    home.join("registry.json")
}

/// Path of `recall.db` under a codeflow home.
#[must_use]
pub fn recall_db_path(home: &Path) -> PathBuf {
    home.join("recall.db")
}

/// Path of `config.toml` under a codeflow home.
#[must_use]
pub fn user_config_path(home: &Path) -> PathBuf {
    home.join("config.toml")
}

// ---------------------------------------------------------------------------
// Repo shape helpers
// ---------------------------------------------------------------------------

/// Whether `repo_root` looks like an initialized codeflow repo: a
/// `.codeflow/` directory containing `project.toml` or `policy.json`.
#[must_use]
pub fn is_initialized(repo_root: &Path) -> bool {
    let dir = repo_root.join(".codeflow");
    dir.join("project.toml").is_file() || dir.join("policy.json").is_file()
}

/// Walk upward from `start` to find the nearest directory containing a
/// `.codeflow/` directory (an initialized repo root).
#[must_use]
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if dir.join(".codeflow").is_dir() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

// ---------------------------------------------------------------------------
// Registry records
// ---------------------------------------------------------------------------

/// One registered repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// Canonical absolute repo root path.
    pub path: String,
    /// Display name (project.toml `name`, else the directory name).
    pub name: String,
    /// Tier from project.toml (`minimal` / `standard` / `full`).
    pub tier: String,
    /// Scaffold version recorded by init in project.toml.
    pub scaffold_version: String,
    /// RFC 3339 timestamp of the most recent codeflow command in this repo.
    pub last_activity: String,
}

/// The registry file shape (`~/.codeflow/registry.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Registry {
    pub schema_version: u32,
    pub repos: Vec<RegistryEntry>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
            repos: Vec::new(),
        }
    }
}

/// Project identity read leniently from `.codeflow/project.toml`.
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub name: String,
    pub tier: String,
    pub scaffold_version: String,
}

/// Read `.codeflow/project.toml` leniently: looks for `name`, `tier`, and
/// `scaffold_version` at the top level, then under a `[project]` table.
/// Missing file or keys fall back to the directory name, the default tier
/// (`standard`), and `unknown`.
#[must_use]
pub fn read_project_info(repo_root: &Path) -> ProjectInfo {
    let dir_name = repo_root
        .file_name()
        .map_or_else(|| "unnamed".to_string(), |n| n.to_string_lossy().into_owned());

    let table: Option<toml::Table> = std::fs::read_to_string(repo_root.join(".codeflow/project.toml"))
        .ok()
        .and_then(|s| s.parse::<toml::Table>().ok());

    let lookup = |key: &str| -> Option<String> {
        let table = table.as_ref()?;
        table
            .get(key)
            .or_else(|| table.get("project").and_then(|p| p.get(key)))
            .and_then(toml::Value::as_str)
            .map(ToString::to_string)
    };

    ProjectInfo {
        name: lookup("name").unwrap_or(dir_name),
        tier: lookup("tier").unwrap_or_else(|| "standard".to_string()),
        scaffold_version: lookup("scaffold_version").unwrap_or_else(|| "unknown".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Registry operations
// ---------------------------------------------------------------------------

/// Upsert the registry entry for `repo_root`, refreshing `last_activity`.
///
/// This is the cheap per-command touch the CLI performs: it does nothing
/// (returning `Ok(false)`) when `repo_root` is not an initialized codeflow
/// repo, and otherwise upserts under the registry's flock sidecar so
/// concurrent invocations never lose entries. Entries whose recorded path
/// no longer exists on disk (deleted or moved repos) are pruned in the same
/// locked read-modify-write — the registry is a view, and stale rows make
/// it lie. Returns `Ok(true)` when an entry was written.
///
/// # Errors
///
/// Returns `Err(String)` when the lock cannot be acquired or the registry
/// file cannot be read, parsed, or written.
pub fn touch_registry(home: &Path, repo_root: &Path) -> Result<bool, String> {
    if !is_initialized(repo_root) {
        return Ok(false);
    }

    let canonical = std::fs::canonicalize(repo_root)
        .map_err(|e| format!("canonicalize {}: {e}", repo_root.display()))?;
    let info = read_project_info(&canonical);
    let entry = RegistryEntry {
        path: canonical.to_string_lossy().into_owned(),
        name: info.name,
        tier: info.tier,
        scaffold_version: info.scaffold_version,
        last_activity: rfc3339_now(),
    };

    locked_rmw_typed(&registry_path(home), Registry::default, |reg| {
        reg.schema_version = REGISTRY_SCHEMA_VERSION;
        // Prune stale rows: a cheap existence check per entry, inside the
        // same lock so concurrent touches never resurrect a pruned path.
        reg.repos
            .retain(|r| r.path == entry.path || Path::new(&r.path).exists());
        match reg.repos.iter_mut().find(|r| r.path == entry.path) {
            Some(existing) => *existing = entry.clone(),
            None => reg.repos.push(entry.clone()),
        }
        Ok(())
    })?;

    Ok(true)
}

/// List registered repos. A missing registry file is an empty registry.
///
/// # Errors
///
/// Returns `Err(String)` when the registry file exists but cannot be read
/// or parsed.
pub fn list_repos(home: &Path) -> Result<Vec<RegistryEntry>, String> {
    let path = registry_path(home);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let value = locked_read(&path)?;
    let registry: Registry =
        serde_json::from_value(value).map_err(|e| format!("registry parse: {e}"))?;
    Ok(registry.repos)
}

// ---------------------------------------------------------------------------
// User config (~/.codeflow/config.toml)
// ---------------------------------------------------------------------------

/// User defaults from `~/.codeflow/config.toml`. Read if present — every
/// field is optional and absence of the file is the common case.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UserConfig {
    pub recall: RecallDefaults,
}

/// `[recall]` section of the user config.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RecallDefaults {
    /// Default result limit for `codeflow recall`.
    pub limit: Option<usize>,
}

impl UserConfig {
    /// Load the user config. A missing file yields defaults.
    ///
    /// # Errors
    ///
    /// Returns `Err(String)` when the file exists but is not valid TOML —
    /// malformed user config is reported, never silently ignored (charter
    /// principle 8: legible degradation).
    pub fn load(home: &Path) -> Result<Self, String> {
        let path = user_config_path(home);
        match std::fs::read_to_string(&path) {
            Ok(data) => {
                toml::from_str(&data).map_err(|e| format!("config.toml parse: {e}"))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("config.toml read: {e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Timestamps (dependency-free RFC 3339 UTC)
// ---------------------------------------------------------------------------

/// Current time as an RFC 3339 UTC string (second precision).
#[must_use]
pub fn rfc3339_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339_from_epoch_secs(secs)
}

/// Format seconds since the Unix epoch as RFC 3339 UTC.
#[must_use]
pub fn rfc3339_from_epoch_secs(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Convert days since 1970-01-01 to a (year, month, day) civil date.
/// Howard Hinnant's `civil_from_days`, restricted to the post-epoch range.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let (month, year) = if mp < 10 { (mp + 3, y) } else { (mp - 9, y + 1) };
    (year, month, day)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn init_repo(root: &Path, name: Option<&str>) {
        fs::create_dir_all(root.join(".codeflow")).unwrap();
        fs::write(root.join(".codeflow/policy.json"), "{}").unwrap();
        let toml = name.map_or_else(String::new, |n| {
            format!("name = \"{n}\"\ntier = \"full\"\nscaffold_version = \"2.0.0-dev\"\n")
        });
        fs::write(root.join(".codeflow/project.toml"), toml).unwrap();
    }

    #[test]
    fn test_rfc3339_known_dates() {
        assert_eq!(rfc3339_from_epoch_secs(0), "1970-01-01T00:00:00Z");
        // 2026-06-11T12:30:45Z
        assert_eq!(rfc3339_from_epoch_secs(1_781_181_045), "2026-06-11T12:30:45Z");
        // leap day: 2024-02-29T00:00:00Z
        assert_eq!(rfc3339_from_epoch_secs(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn test_codeflow_home_env_override() {
        // Serialized via distinct env var usage: set, read, restore.
        let prev = std::env::var_os("CODEFLOW_HOME");
        std::env::set_var("CODEFLOW_HOME", "/tmp/cf-test-home");
        assert_eq!(codeflow_home(), Some(PathBuf::from("/tmp/cf-test-home")));
        match prev {
            Some(v) => std::env::set_var("CODEFLOW_HOME", v),
            None => std::env::remove_var("CODEFLOW_HOME"),
        }
    }

    #[test]
    fn test_is_initialized_requires_codeflow_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_initialized(dir.path()));
        init_repo(dir.path(), None);
        assert!(is_initialized(dir.path()));
    }

    #[test]
    fn test_find_repo_root_walks_up() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), None);
        let nested = dir.path().join("src/deep/module");
        fs::create_dir_all(&nested).unwrap();
        let found = find_repo_root(&nested).unwrap();
        assert_eq!(found, dir.path());
    }

    #[test]
    fn test_find_repo_root_none_outside() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_repo_root(dir.path()), None);
    }

    #[test]
    fn test_read_project_info_lenient_defaults() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), None); // empty project.toml
        let info = read_project_info(dir.path());
        assert_eq!(info.tier, "standard");
        assert_eq!(info.scaffold_version, "unknown");
        // name falls back to directory name
        assert_eq!(
            info.name,
            dir.path().file_name().unwrap().to_string_lossy()
        );
    }

    #[test]
    fn test_read_project_info_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), Some("myproj"));
        let info = read_project_info(dir.path());
        assert_eq!(info.name, "myproj");
        assert_eq!(info.tier, "full");
        assert_eq!(info.scaffold_version, "2.0.0-dev");
    }

    #[test]
    fn test_read_project_info_project_table() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        fs::write(
            dir.path().join(".codeflow/project.toml"),
            "[project]\nname = \"tabled\"\ntier = \"minimal\"\n",
        )
        .unwrap();
        let info = read_project_info(dir.path());
        assert_eq!(info.name, "tabled");
        assert_eq!(info.tier, "minimal");
    }

    #[test]
    fn test_touch_registry_skips_uninitialized() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let touched = touch_registry(home.path(), repo.path()).unwrap();
        assert!(!touched);
        assert!(!registry_path(home.path()).exists());
    }

    #[test]
    fn test_touch_registry_upserts() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path(), Some("alpha"));

        assert!(touch_registry(home.path(), repo.path()).unwrap());
        let repos = list_repos(home.path()).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "alpha");
        assert_eq!(repos[0].tier, "full");

        // Second touch updates in place, no duplicate.
        assert!(touch_registry(home.path(), repo.path()).unwrap());
        let repos = list_repos(home.path()).unwrap();
        assert_eq!(repos.len(), 1, "touch must upsert, not append");
    }

    #[test]
    fn test_touch_registry_multiple_repos() {
        let home = tempfile::tempdir().unwrap();
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        init_repo(a.path(), Some("a"));
        init_repo(b.path(), Some("b"));

        touch_registry(home.path(), a.path()).unwrap();
        touch_registry(home.path(), b.path()).unwrap();

        let mut names: Vec<String> = list_repos(home.path())
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn test_touch_registry_prunes_stale_entries() {
        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        init_repo(live.path(), Some("live"));

        let stale = tempfile::tempdir().unwrap();
        init_repo(stale.path(), Some("stale"));
        touch_registry(home.path(), stale.path()).unwrap();
        touch_registry(home.path(), live.path()).unwrap();
        assert_eq!(list_repos(home.path()).unwrap().len(), 2);

        // The stale repo disappears from disk; the next touch prunes it.
        let stale_path = stale.path().to_path_buf();
        drop(stale);
        assert!(!stale_path.exists());

        touch_registry(home.path(), live.path()).unwrap();
        let repos = list_repos(home.path()).unwrap();
        assert_eq!(repos.len(), 1, "stale entry must be pruned");
        assert_eq!(repos[0].name, "live");
    }

    #[test]
    fn test_touch_registry_keeps_live_entries_when_pruning() {
        let home = tempfile::tempdir().unwrap();
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let gone = tempfile::tempdir().unwrap();
        init_repo(a.path(), Some("a"));
        init_repo(b.path(), Some("b"));
        init_repo(gone.path(), Some("gone"));

        touch_registry(home.path(), a.path()).unwrap();
        touch_registry(home.path(), b.path()).unwrap();
        touch_registry(home.path(), gone.path()).unwrap();
        drop(gone);

        touch_registry(home.path(), a.path()).unwrap();
        let mut names: Vec<String> = list_repos(home.path())
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect();
        names.sort();
        assert_eq!(names, vec!["a", "b"], "live entries survive the prune");
    }

    #[test]
    fn test_touch_registry_concurrent_with_pruning() {
        let home = tempfile::tempdir().unwrap();
        let stale = tempfile::tempdir().unwrap();
        init_repo(stale.path(), Some("stale"));
        touch_registry(home.path(), stale.path()).unwrap();
        drop(stale);

        let repos: Vec<tempfile::TempDir> = (0..8)
            .map(|i| {
                let d = tempfile::tempdir().unwrap();
                init_repo(d.path(), Some(&format!("repo-{i}")));
                d
            })
            .collect();

        std::thread::scope(|scope| {
            for repo in &repos {
                let home = home.path().to_path_buf();
                scope.spawn(move || {
                    touch_registry(&home, repo.path()).unwrap();
                });
            }
        });

        let listed = list_repos(home.path()).unwrap();
        assert_eq!(
            listed.len(),
            8,
            "all live touches survive; the stale entry is pruned exactly once"
        );
        assert!(listed.iter().all(|r| r.name != "stale"));
    }

    #[test]
    fn test_touch_registry_concurrent_flock() {
        let home = tempfile::tempdir().unwrap();
        let repos: Vec<tempfile::TempDir> = (0..8)
            .map(|i| {
                let d = tempfile::tempdir().unwrap();
                init_repo(d.path(), Some(&format!("repo-{i}")));
                d
            })
            .collect();

        std::thread::scope(|scope| {
            for repo in &repos {
                let home = home.path().to_path_buf();
                scope.spawn(move || {
                    touch_registry(&home, repo.path()).unwrap();
                });
            }
        });

        let listed = list_repos(home.path()).unwrap();
        assert_eq!(
            listed.len(),
            8,
            "all concurrent touches must survive the flock'd upsert"
        );
    }

    #[test]
    fn test_touch_registry_concurrent_same_repo() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path(), Some("solo"));

        std::thread::scope(|scope| {
            for _ in 0..8 {
                let home = home.path().to_path_buf();
                let repo = repo.path().to_path_buf();
                scope.spawn(move || {
                    touch_registry(&home, &repo).unwrap();
                });
            }
        });

        let listed = list_repos(home.path()).unwrap();
        assert_eq!(listed.len(), 1, "same repo must never duplicate");
    }

    #[test]
    fn test_list_repos_missing_registry_is_empty() {
        let home = tempfile::tempdir().unwrap();
        assert!(list_repos(home.path()).unwrap().is_empty());
    }

    #[test]
    fn test_user_config_missing_is_default() {
        let home = tempfile::tempdir().unwrap();
        let cfg = UserConfig::load(home.path()).unwrap();
        assert_eq!(cfg.recall.limit, None);
    }

    #[test]
    fn test_user_config_reads_recall_limit() {
        let home = tempfile::tempdir().unwrap();
        fs::write(user_config_path(home.path()), "[recall]\nlimit = 5\n").unwrap();
        let cfg = UserConfig::load(home.path()).unwrap();
        assert_eq!(cfg.recall.limit, Some(5));
    }

    #[test]
    fn test_user_config_malformed_is_reported() {
        let home = tempfile::tempdir().unwrap();
        fs::write(user_config_path(home.path()), "not = [valid").unwrap();
        let err = UserConfig::load(home.path()).unwrap_err();
        assert!(err.contains("config.toml parse"), "got: {err}");
    }
}
