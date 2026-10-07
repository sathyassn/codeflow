//! User-level cross-repo state under `~/.codeflow/` (charter §7 layer 3, §8).
//!
//! User-owned state lives here:
//!
//! - `registry.json` — the flock-guarded registry of initialized repos
//!   (path, name, tier, scaffold version, last activity). Upserted by a
//!   cheap [`touch_registry`] call the CLI makes on every command run
//!   inside an initialized repo. A view, not a system: no daemon, lazy
//!   sync at query time.
//! - `config.toml` — user defaults, read if present, never required.
//! - `qualified-bindings/` — compact, non-secret indexes of human-approved
//!   native model+harness evaluations.
//!
//! The home directory honors the `CODEFLOW_HOME` environment override so
//! tests (and unusual setups) can redirect all user-level state.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::file_lock::{locked_read, locked_rmw_typed_io};

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
    resolve_home(
        std::env::var_os("CODEFLOW_HOME"),
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
    )
}

/// Pure resolution behind [`codeflow_home`], separated so the precedence is
/// testable without mutating process-global environment (`setenv` concurrent
/// with `getenv` in the parallel test harness is a data race on POSIX).
fn resolve_home(
    override_dir: Option<std::ffi::OsString>,
    home_dir: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    if let Some(v) = override_dir {
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    home_dir
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

/// Directory of promoted model+harness binding records.
#[must_use]
pub fn qualified_bindings_path(home: &Path) -> PathBuf {
    home.join("qualified-bindings")
}

// ---------------------------------------------------------------------------
// Repo shape helpers
// ---------------------------------------------------------------------------

/// A candidate project root whose `CodeFlow` state could not be inspected:
/// the file read and the error, so a caller can name what to repair.
#[derive(Debug)]
pub struct RootUnreadable {
    /// The `.codeflow` file whose presence could not be established.
    pub path: PathBuf,
    /// The metadata error.
    pub error: std::io::Error,
}

impl std::fmt::Display for RootUnreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot inspect {}: {}", self.path.display(), self.error)
    }
}

impl std::error::Error for RootUnreadable {}

impl From<RootUnreadable> for std::io::Error {
    fn from(unreadable: RootUnreadable) -> Self {
        Self::new(unreadable.error.kind(), unreadable.to_string())
    }
}

/// Whether `repo_root` looks like an initialized codeflow repo: a
/// `.codeflow/` directory containing `project.toml` or `policy.json`.
/// # Errors
/// Returns metadata errors, naming the file, instead of skipping a possible
/// repository root.
pub fn is_initialized(repo_root: &Path) -> std::io::Result<bool> {
    initialized_at(repo_root).map_err(Into::into)
}

fn initialized_at(repo_root: &Path) -> Result<bool, RootUnreadable> {
    let dir = repo_root.join(".codeflow");
    for name in ["project.toml", "policy.json"] {
        let path = dir.join(name);
        let file = crate::absence::proven_absent(&path)
            .and_then(|absent| Ok(!absent && std::fs::metadata(&path)?.is_file()));
        match file {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(error) => return Err(RootUnreadable { path, error }),
        }
    }
    Ok(false)
}

/// Walk upward from `start` to find the nearest directory containing a
/// `.codeflow/` directory (an initialized repo root).
/// # Errors
/// Returns an error when a candidate root cannot be inspected.
pub fn find_repo_root(start: &Path) -> std::io::Result<Option<PathBuf>> {
    find_repo_root_checked(start).map_err(Into::into)
}

/// [`find_repo_root`], with the file that could not be inspected.
/// # Errors
/// Returns the candidate file whose presence could not be established.
pub fn find_repo_root_checked(start: &Path) -> Result<Option<PathBuf>, RootUnreadable> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if initialized_at(dir)? {
            return Ok(Some(dir.to_path_buf()));
        }
        current = dir.parent();
    }
    Ok(None)
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
///
/// # Errors
///
/// Returns an error when existing project metadata cannot be read or parsed.
pub fn read_project_info(repo_root: &Path) -> Result<ProjectInfo, String> {
    // A label for a person (OS text rule, issue 79): the exact name, with
    // an invalid byte shown as an escape.
    let dir_name = repo_root.file_name().map_or_else(
        || "unnamed".to_string(),
        |n| crate::git::GitName::from_os_str(n).display().to_string(),
    );

    let path = repo_root.join(".codeflow/project.toml");
    let table: Option<toml::Table> = if path
        .try_exists()
        .map_err(|error| format!("cannot inspect project metadata: {error}"))?
    {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read project metadata: {error}"))?;
        Some(
            toml::from_str(&text)
                .map_err(|error| format!("cannot parse project metadata: {error}"))?,
        )
    } else {
        None
    };

    let lookup = |key: &str| -> Result<Option<String>, String> {
        let Some(table) = &table else {
            return Ok(None);
        };
        let value = table
            .get(key)
            .or_else(|| table.get("project").and_then(|project| project.get(key)));
        value
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("project {key} must be a string"))
            })
            .transpose()
    };
    Ok(ProjectInfo {
        name: lookup("name")?.unwrap_or(dir_name),
        tier: lookup("tier")?.unwrap_or_else(|| "standard".to_string()),
        scaffold_version: lookup("scaffold_version")?.unwrap_or_else(|| "unknown".to_string()),
    })
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
/// it lie. Returns `Ok(true)` when an entry was written, and `Ok(false)`
/// when nothing was: the repo is not initialized, or this process may not
/// write the registry (a sandbox or a read-only home), which is expected
/// and not worth a warning.
///
/// # Errors
///
/// Returns `Err(String)` when the lock cannot be acquired or the registry
/// file cannot be read, parsed, or written for another reason.
pub fn touch_registry(home: &Path, repo_root: &Path) -> Result<bool, String> {
    if !is_initialized(repo_root).map_err(|error| error.to_string())? {
        return Ok(false);
    }
    let canonical = std::fs::canonicalize(repo_root)
        .map_err(|e| format!("canonicalize {}: {e}", repo_root.display()))?;
    let info = read_project_info(&canonical)?;
    // OS text rule (issue 79): the path is the row's identity, and a lossy
    // spelling would let two repositories share one row. A path that is not
    // valid UTF-8 is not recorded (a registry write is best effort).
    let Some(path) = canonical.to_str() else {
        return Ok(false);
    };
    let entry = RegistryEntry {
        path: path.to_string(),
        name: info.name,
        tier: info.tier,
        scaffold_version: info.scaffold_version,
        last_activity: rfc3339_now(),
    };

    let written = locked_rmw_typed_io(&registry_path(home), Registry::default, |reg| {
        reg.schema_version = REGISTRY_SCHEMA_VERSION;
        // Prune stale rows: a cheap existence check per entry, inside the
        // same lock so concurrent touches never resurrect a pruned path.
        // try_exists distinguishes "definitively missing" (Ok(false), prune)
        // from permission/transient stat errors (Err, KEEP) — exists() would
        // collapse both and could permanently drop a live repo row.
        reg.repos.retain(|r| {
            r.path == entry.path || !matches!(Path::new(&r.path).try_exists(), Ok(false))
        });
        match reg.repos.iter_mut().find(|r| r.path == entry.path) {
            Some(existing) => *existing = entry.clone(),
            None => reg.repos.push(entry.clone()),
        }
        Ok(())
    });
    match written {
        Ok(()) => Ok(true),
        // Any step, from the lock to the atomic write, may be denied.
        Err(error) if error.io().is_some_and(is_denied) => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

/// A permission denial or a read-only file system: the registry is not this
/// process's to write, as in a sandbox.
fn is_denied(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
    )
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
            Ok(data) => toml::from_str(&data).map_err(|e| format!("config.toml parse: {e}")),
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
    let (month, year) = if mp < 10 {
        (mp + 3, y)
    } else {
        (mp - 9, y + 1)
    };
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
        assert_eq!(
            rfc3339_from_epoch_secs(1_781_181_045),
            "2026-06-11T12:30:45Z"
        );
        // leap day: 2024-02-29T00:00:00Z
        assert_eq!(
            rfc3339_from_epoch_secs(1_709_164_800),
            "2024-02-29T00:00:00Z"
        );
    }

    #[test]
    fn test_codeflow_home_resolution_precedence() {
        // Exercises the pure resolver — no process-global set_var/remove_var,
        // which would race concurrently running tests' getenv/spawn calls.
        let home = |s: &str| Some(std::ffi::OsString::from(s));
        assert_eq!(
            resolve_home(home("/tmp/cf-test-home"), home("/home/u")),
            Some(PathBuf::from("/tmp/cf-test-home")),
            "CODEFLOW_HOME override wins"
        );
        assert_eq!(
            resolve_home(None, home("/home/u")),
            Some(PathBuf::from("/home/u/.codeflow")),
            "falls back to $HOME/.codeflow"
        );
        assert_eq!(
            resolve_home(home(""), home("/home/u")),
            Some(PathBuf::from("/home/u/.codeflow")),
            "empty override is ignored"
        );
        assert_eq!(resolve_home(None, home("")), None, "empty home yields none");
        assert_eq!(resolve_home(None, None), None);
    }

    #[test]
    fn test_is_initialized_requires_codeflow_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_initialized(dir.path()).unwrap());
        init_repo(dir.path(), None);
        assert!(is_initialized(dir.path()).unwrap());
    }

    #[test]
    fn test_find_repo_root_walks_up() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), None);
        let nested = dir.path().join("src/deep/module");
        fs::create_dir_all(&nested).unwrap();
        let found = find_repo_root(&nested).unwrap().unwrap();
        assert_eq!(found, dir.path());
    }

    #[test]
    fn test_find_repo_root_none_outside() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_repo_root(dir.path()).unwrap(), None);
    }

    #[test]
    fn test_find_repo_root_ignores_user_state_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".codeflow")).unwrap();
        fs::write(
            dir.path().join(".codeflow/config.toml"),
            "[recall]\nlimit = 20\n",
        )
        .unwrap();
        let nested = dir.path().join("AppData/Local/Temp/project");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(find_repo_root(&nested).unwrap(), None);
    }

    #[test]
    fn test_read_project_info_lenient_defaults() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), None); // empty project.toml
        let info = read_project_info(dir.path()).unwrap();
        assert_eq!(info.tier, "standard");
        assert_eq!(info.scaffold_version, "unknown");
        // name falls back to directory name
        assert_eq!(info.name, dir.path().file_name().unwrap().to_string_lossy());
    }

    #[test]
    fn test_read_project_info_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), Some("myproj"));
        let info = read_project_info(dir.path()).unwrap();
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
        let info = read_project_info(dir.path()).unwrap();
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

    /// A stat ERROR (here: an unreadable parent directory) must KEEP the
    /// entry — only a definitive `Ok(false)` prunes. `exists()` would have
    /// collapsed the error into "missing" and dropped a live repo row.
    #[test]
    #[cfg(unix)]
    fn test_touch_registry_keeps_entries_behind_stat_errors() {
        use std::os::unix::fs::PermissionsExt;

        let home = tempfile::tempdir().unwrap();
        let live = tempfile::tempdir().unwrap();
        init_repo(live.path(), Some("live"));

        let guarded_parent = tempfile::tempdir().unwrap();
        let guarded = guarded_parent.path().join("repo");
        std::fs::create_dir(&guarded).unwrap();
        init_repo(&guarded, Some("guarded"));
        touch_registry(home.path(), &guarded).unwrap();

        // Remove traversal permission on the parent: stat on the child now
        // errors with EACCES instead of reporting "missing".
        let mut perms = std::fs::metadata(guarded_parent.path())
            .unwrap()
            .permissions();
        perms.set_mode(0o000);
        std::fs::set_permissions(guarded_parent.path(), perms).unwrap();
        assert!(
            Path::new(&guarded).try_exists().is_err(),
            "precondition: stat must error, not report missing"
        );

        touch_registry(home.path(), live.path()).unwrap();

        // Restore permissions before asserting so tempdir cleanup works.
        let mut restore = std::fs::metadata(guarded_parent.path())
            .unwrap()
            .permissions();
        restore.set_mode(0o755);
        std::fs::set_permissions(guarded_parent.path(), restore).unwrap();

        let mut names: Vec<String> = list_repos(home.path())
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["guarded", "live"],
            "entry behind a stat error must be kept"
        );
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

    /// Issue 79: a repository whose path is not valid UTF-8 is not recorded
    /// under a lossy spelling that another repository could share.
    #[cfg(unix)]
    #[test]
    fn a_repository_path_that_is_not_utf8_is_not_recorded() {
        use std::os::unix::ffi::OsStrExt as _;
        let home = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let repo = parent.path().join(std::ffi::OsStr::from_bytes(b"caf\xe9"));
        if std::fs::create_dir(&repo).is_err() {
            return; // this file system refuses the name
        }
        init_repo(&repo, Some("odd"));
        assert_eq!(touch_registry(home.path(), &repo), Ok(false));
        assert!(list_repos(home.path()).unwrap().is_empty());
    }
}

#[cfg(test)]
mod r16_obtaining_regressions {

    #[test]
    fn r16_registry_metadata_is_not_defaulted_on_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
        let path = dir.path().join(".codeflow/project.toml");
        for text in [b"name = 4".as_slice(), b"name = \"oops".as_slice(), &[0xff]] {
            std::fs::write(&path, text).unwrap();
            assert!(super::read_project_info(dir.path()).is_err());
        }
    }
}

#[cfg(all(test, unix))]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_registry_stops_at_unreadable_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_initialized(dir.path()).unwrap());
        std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
        std::fs::write(dir.path().join(".codeflow/policy.json"), "{}").unwrap();
        let nested = dir.path().join("child");
        std::fs::create_dir(&nested).unwrap();
        assert_eq!(
            find_repo_root(&nested).unwrap(),
            Some(dir.path().to_path_buf())
        );
        std::os::unix::fs::symlink("missing", nested.join(".codeflow")).unwrap();
        assert!(find_repo_root(&nested).is_err());
        // The checked form names the file whose presence it could not
        // establish, for the remedy to point at.
        let unreadable = find_repo_root_checked(&nested).unwrap_err();
        assert_eq!(
            unreadable.path,
            nested.join(".codeflow").join("project.toml")
        );
    }
}
