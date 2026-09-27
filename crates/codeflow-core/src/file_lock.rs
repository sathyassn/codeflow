//! File-based locking utilities.
//!
//! Provides locked read-modify-write (RMW) for JSON state files using
//! sidecar `.lock` files with `fs2::lock_exclusive`. Used by anything that
//! needs exclusive access across concurrent invocations: the `integrate`
//! primitive, the cross-repo registry, and hook-written state.
//!
//! Lock mechanism: sidecar `{path}.lock` file + `fs2::FileExt::lock_exclusive()`.
//! The data file itself is never locked directly.

use std::fs;
use std::path::Path;

use fs2::FileExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Sidecar lock helpers
// ---------------------------------------------------------------------------

/// Acquire an exclusive sidecar lock for the given data file path.
///
/// Creates `{path}.lock` (ensuring parent dirs exist) and acquires an
/// exclusive `fs2` lock on it. Returns the lock file handle — the lock is
/// held until the handle is dropped.
fn acquire_exclusive_lock(path: &Path) -> Result<fs::File, RmwError> {
    let lock_path = sidecar_lock_path(path);

    if let Some(dir) = lock_path.parent() {
        fs::create_dir_all(dir).map_err(|e| RmwError::Io("lock dir", e))?;
    }

    let lock_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| RmwError::Io("lock open", e))?;

    FileExt::lock_exclusive(&lock_file).map_err(|e| RmwError::Io("lock acquire", e))?;

    Ok(lock_file)
}

/// A failed locked read-modify-write. An I/O failure keeps its OS error, so
/// a caller can tell a permission denial (a sandbox, a read-only home) from
/// other failures; it displays as `<step>: <error>`.
#[derive(Debug)]
pub(crate) enum RmwError {
    /// The step that failed and its OS error.
    Io(&'static str, std::io::Error),
    /// A parse, callback or serialize failure.
    Other(String),
}

impl RmwError {
    /// The OS error of an I/O failure.
    pub(crate) fn io(&self) -> Option<&std::io::Error> {
        match self {
            Self::Io(_, error) => Some(error),
            Self::Other(_) => None,
        }
    }
}

impl std::fmt::Display for RmwError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(step, error) => write!(f, "{step}: {error}"),
            Self::Other(message) => f.write_str(message),
        }
    }
}

/// Acquire a shared sidecar lock for the given data file path.
///
/// Creates `{path}.lock` (ensuring parent dirs exist) and acquires a
/// shared `fs2` lock on it. Returns the lock file handle — the lock is
/// held until the handle is dropped.
fn acquire_shared_lock(path: &Path) -> Result<fs::File, String> {
    let lock_path = sidecar_lock_path(path);

    if let Some(dir) = lock_path.parent() {
        let _ = fs::create_dir_all(dir);
    }

    let lock_file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("lock open: {e}"))?;

    FileExt::lock_shared(&lock_file).map_err(|e| format!("lock acquire: {e}"))?;

    Ok(lock_file)
}

/// Compute the sidecar lock path for a data file.
///
/// Appends `.lock` to the full file name (e.g., `foo.json` → `foo.json.lock`,
/// `registry.json` → `registry.json.lock`).
fn sidecar_lock_path(path: &Path) -> std::path::PathBuf {
    let mut lock_name = path.file_name().unwrap_or_default().to_os_string();
    lock_name.push(".lock");
    path.with_file_name(lock_name)
}

// ---------------------------------------------------------------------------
// JSON operations
// ---------------------------------------------------------------------------

/// Perform a locked read-modify-write on a JSON file.
///
/// 1. Acquire exclusive lock on `{path}.lock`
/// 2. Read JSON from `path`
/// 3. Call `f` with mutable reference to parsed JSON
/// 4. Write updated JSON back atomically (tmp+rename)
/// 5. Release lock on drop
///
/// Returns `Ok(())` on success, or error string on failure.
/// Graceful: lock/read/write failures return `Err`, never panic.
///
/// # Errors
///
/// Returns `Err(String)` if the lock file cannot be opened, the lock cannot
/// be acquired, the JSON file cannot be read or parsed, or the updated
/// content cannot be written atomically.
pub fn locked_rmw<F>(path: &Path, f: F) -> Result<(), String>
where
    F: FnOnce(&mut Value),
{
    let _lock = acquire_exclusive_lock(path).map_err(|e| e.to_string())?;

    // Read
    let data = fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    let mut value: Value = serde_json::from_str(&data).map_err(|e| format!("parse: {e}"))?;

    // Modify
    f(&mut value);

    // Write atomically (tmp + rename)
    let pretty = serde_json::to_string_pretty(&value).map_err(|e| format!("serialize: {e}"))?;
    atomic_write(path, pretty.as_bytes()).map_err(|e| format!("write: {e}"))?;

    // Lock released on drop
    Ok(())
}

/// Perform a locked read of a JSON file.
///
/// Acquires a shared (read) lock to ensure the file is not being written
/// concurrently. Returns parsed JSON value.
///
/// # Errors
///
/// Returns `Err(String)` if the lock file cannot be opened, the lock cannot
/// be acquired, or the JSON file cannot be read or parsed.
pub fn locked_read(path: &Path) -> Result<Value, String> {
    let _lock = acquire_shared_lock(path)?;

    let data = fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    let value: Value = serde_json::from_str(&data).map_err(|e| format!("parse: {e}"))?;

    // Lock released on drop
    Ok(value)
}

/// Locked read with exponential backoff for critical (destructive) paths.
///
/// Use when read failure would trigger an irreversible destructive action.
/// Retries only if the parent directory exists (indicating transient
/// unavailability, not genuine absence).
///
/// Backoff: 50ms, 100ms, 200ms, ... (doubles each attempt).
///
/// # Errors
///
/// Returns `Err(String)` if the initial read fails and either the parent
/// directory does not exist or all retries are exhausted.
pub fn locked_read_critical(path: &Path, max_retries: u32) -> Result<Value, String> {
    // Fast-fail: if parent dir doesn't exist, the file cannot exist.
    // Check before locked_read to avoid its create_dir_all side effect on the lock path.
    if !path.parent().is_some_and(std::path::Path::exists) {
        return Err("read: parent directory does not exist".to_string());
    }

    match locked_read(path) {
        Ok(v) => Ok(v),
        Err(first_err) => {
            // Parent dir may have been removed during the read attempt.
            if !path.parent().is_some_and(std::path::Path::exists) {
                return Err(first_err);
            }
            let mut delay_ms = 50u64;
            for attempt in 1..=max_retries {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                match locked_read(path) {
                    Ok(v) => return Ok(v),
                    Err(e) if attempt == max_retries => {
                        return Err(format!(
                            "critical read failed after {max_retries} retries: first={first_err}, last={e}"
                        ));
                    }
                    Err(_) => {
                        delay_ms = delay_ms.saturating_mul(2);
                    }
                }
            }
            Err(first_err) // unreachable but satisfies compiler
        }
    }
}

/// Perform a locked read-modify-write on a JSON file with typed deserialization.
///
/// Like [`locked_rmw`] but works with any `Serialize + DeserializeOwned` type
/// instead of raw `serde_json::Value`. If the file does not exist, `default_fn`
/// provides the initial value.
///
/// # Errors
///
/// Returns `Err(String)` on lock, read, parse, callback, serialize, or write failure.
pub fn locked_rmw_typed<T, F>(path: &Path, default_fn: fn() -> T, f: F) -> Result<(), String>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce(&mut T) -> Result<(), String>,
{
    locked_rmw_typed_io(path, default_fn, f).map_err(|e| e.to_string())
}

/// [`locked_rmw_typed`], keeping the OS error of an I/O failure (lock, read
/// or atomic write) typed in [`RmwError::Io`].
pub(crate) fn locked_rmw_typed_io<T, F>(
    path: &Path,
    default_fn: fn() -> T,
    f: F,
) -> Result<(), RmwError>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce(&mut T) -> Result<(), String>,
{
    let _lock = acquire_exclusive_lock(path)?;

    // Read (default on NotFound)
    let mut value: T = match fs::read_to_string(path) {
        Ok(data) => {
            serde_json::from_str(&data).map_err(|e| RmwError::Other(format!("parse: {e}")))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => default_fn(),
        Err(e) => return Err(RmwError::Io("read", e)),
    };

    // Modify (fallible)
    f(&mut value).map_err(RmwError::Other)?;

    // Write atomically (tmp + rename)
    let pretty = serde_json::to_string_pretty(&value)
        .map_err(|e| RmwError::Other(format!("serialize: {e}")))?;
    atomic_write(path, format!("{pretty}\n").as_bytes()).map_err(|e| RmwError::Io("write", e))?;

    // Lock released on drop
    Ok(())
}

// ---------------------------------------------------------------------------
// Direct path locks
// ---------------------------------------------------------------------------

/// An exclusive advisory lock on an arbitrary lock-file path.
///
/// Unlike the JSON helpers above (which lock a `.lock` sidecar of a data
/// file), here the file at the given path IS the lock. Used by the
/// `integrate` primitive on `.git/codeflow/integrate.lock`. The lock is
/// held until the guard is dropped.
#[derive(Debug)]
pub struct PathLock {
    _file: fs::File,
}

/// Acquire an exclusive lock on `path` itself, creating parent directories
/// and the file as needed. Blocks until the lock is available.
///
/// # Errors
///
/// Returns `Err(String)` if the file cannot be opened or the lock cannot
/// be acquired.
pub fn lock_path_exclusive(path: &Path) -> Result<PathLock, String> {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }

    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("lock open: {e}"))?;

    FileExt::lock_exclusive(&file).map_err(|e| format!("lock acquire: {e}"))?;

    Ok(PathLock { _file: file })
}

// ---------------------------------------------------------------------------
// Atomic write helper
// ---------------------------------------------------------------------------

/// Atomically write file contents: write to `.tmp` sibling then rename.
///
/// # Errors
///
/// Returns an I/O error if writing the temporary file or renaming it fails.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), std::io::Error> {
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, data)?;
    fs::rename(&tmp_path, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp_path);
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locked_rmw_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        fs::write(&path, r#"{"status":"created","count":0}"#).unwrap();

        let result = locked_rmw(&path, |v| {
            v["status"] = Value::String("updated".into());
            v["count"] = serde_json::json!(1);
        });
        assert!(result.is_ok(), "locked_rmw failed: {result:?}");

        let updated: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(updated["status"], "updated");
        assert_eq!(updated["count"], 1);
    }

    #[test]
    fn test_locked_rmw_creates_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        fs::write(&path, r#"{"a":1}"#).unwrap();

        let _ = locked_rmw(&path, |_| {});

        let lock_path = sidecar_lock_path(&path);
        assert!(lock_path.exists(), "lock file should be created");
    }

    #[test]
    fn test_locked_rmw_preserves_unmodified_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        fs::write(&path, r#"{"keep":"me","change":"old"}"#).unwrap();

        let _ = locked_rmw(&path, |v| {
            v["change"] = Value::String("new".into());
        });

        let updated: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(updated["keep"], "me");
        assert_eq!(updated["change"], "new");
    }

    #[test]
    fn test_locked_rmw_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let result = locked_rmw(&path, |_| {});
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("read:"));
    }

    #[test]
    fn test_locked_rmw_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "not json at all").unwrap();

        let result = locked_rmw(&path, |_| {});
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("parse:"));
    }

    #[test]
    fn test_locked_read_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        fs::write(&path, r#"{"status":"active"}"#).unwrap();

        let value = locked_read(&path).unwrap();
        assert_eq!(value["status"], "active");
    }

    #[test]
    fn test_locked_read_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");

        let result = locked_read(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("read:"));
    }

    #[test]
    fn test_locked_read_critical_success_first_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        fs::write(&path, r#"{"ok":true}"#).unwrap();

        let value = locked_read_critical(&path, 3).unwrap();
        assert_eq!(value["ok"], true);
    }

    #[test]
    fn test_locked_read_critical_no_retry_when_dir_missing() {
        // Parent directory does not exist — should fail immediately without retrying.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent_subdir").join("status.json");
        assert!(!path.parent().unwrap().exists());

        let start = std::time::Instant::now();
        let result = locked_read_critical(&path, 5);
        let elapsed = start.elapsed();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("parent directory does not exist"),
            "unexpected error: {err}"
        );
        // Should return quickly (no retries), well under the 50ms first backoff
        assert!(
            elapsed.as_millis() < 50,
            "took {elapsed:?}, expected immediate failure when parent dir missing"
        );
    }

    #[test]
    fn test_locked_read_critical_all_retries_exhausted() {
        let dir = tempfile::tempdir().unwrap();
        // Parent dir exists, but file does not — retries will all fail.
        let path = dir.path().join("missing.json");

        let result = locked_read_critical(&path, 2);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("critical read failed after 2 retries"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_locked_rmw_sequential_consistency() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("counter.json");
        fs::write(&path, r#"{"count":0}"#).unwrap();

        for _ in 0..10 {
            let _ = locked_rmw(&path, |v| {
                let count = v["count"].as_i64().unwrap_or(0);
                v["count"] = serde_json::json!(count + 1);
            });
        }

        let final_value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(final_value["count"], 10);
    }

    #[test]
    fn test_locked_rmw_typed_basic() {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
        struct Config {
            name: String,
            count: u32,
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, r#"{"name":"test","count":0}"#).unwrap();

        let result = locked_rmw_typed(&path, Config::default, |cfg| {
            cfg.count = 42;
            Ok(())
        });
        assert!(result.is_ok(), "locked_rmw_typed failed: {result:?}");

        let updated: Config = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(updated.count, 42);
        assert_eq!(updated.name, "test");
    }

    #[test]
    fn test_locked_rmw_typed_default_on_missing() {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Config {
            name: String,
        }

        impl Default for Config {
            fn default() -> Self {
                Self {
                    name: "default".to_string(),
                }
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");

        let result = locked_rmw_typed(&path, Config::default, |cfg| {
            cfg.name = "created".to_string();
            Ok(())
        });
        assert!(result.is_ok());

        let created: Config = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(created.name, "created");
    }

    #[test]
    fn test_locked_rmw_typed_callback_error() {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Default, Serialize, Deserialize)]
        struct Config {
            value: u32,
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, r#"{"value":10}"#).unwrap();

        let result = locked_rmw_typed(&path, Config::default, |_cfg| {
            Err("validation failed".to_string())
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("validation failed"));

        // File should be unchanged since callback failed before write.
        let unchanged: Config = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(unchanged.value, 10);
    }

    #[test]
    fn test_locked_rmw_typed_read_error_not_not_found() {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Default, Serialize, Deserialize)]
        struct Config {
            value: u32,
        }

        let dir = tempfile::tempdir().unwrap();
        // Create a directory at the path where a file is expected.
        // Reading a directory returns an error that is NOT NotFound.
        let path = dir.path().join("is_a_dir.json");
        fs::create_dir_all(&path).unwrap();

        let result = locked_rmw_typed(&path, Config::default, |_| Ok(()));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("read:"), "expected read error, got: {err}");
    }

    #[test]
    fn test_locked_rmw_typed_invalid_json() {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Default, Serialize, Deserialize)]
        struct Config {
            value: u32,
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "not valid json").unwrap();

        let result = locked_rmw_typed(&path, Config::default, |_| Ok(()));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("parse:"));
    }

    #[test]
    fn test_locked_read_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "{{invalid").unwrap();

        let result = locked_read(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("parse:"));
    }

    #[test]
    fn test_atomic_write_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        atomic_write(&path, b"hello").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        fs::write(&path, "old content").unwrap();
        atomic_write(&path, b"new content").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new content");
    }

    #[test]
    fn test_locked_rmw_typed_preserves_on_callback_error() {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
        struct Config {
            name: String,
            count: u32,
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, r#"{"name":"original","count":5}"#).unwrap();

        // Callback modifies then errors — file should be unchanged
        let result = locked_rmw_typed(&path, Config::default, |cfg| {
            cfg.count = 999;
            Err("abort".into())
        });
        assert!(result.is_err());

        let unchanged: Config = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(unchanged.name, "original");
        assert_eq!(unchanged.count, 5);
    }

    #[test]
    fn test_lock_path_exclusive_creates_file_and_parents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("codeflow").join("integrate.lock");

        let guard = lock_path_exclusive(&path).unwrap();
        assert!(path.exists(), "lock file should be created");
        drop(guard);

        // Reacquirable after drop.
        let again = lock_path_exclusive(&path);
        assert!(again.is_ok(), "lock should be reacquirable: {again:?}");
    }

    #[test]
    fn test_sidecar_lock_path_json() {
        let path = Path::new("/tmp/foo.json");
        assert_eq!(sidecar_lock_path(path), Path::new("/tmp/foo.json.lock"));
    }

    #[test]
    fn test_sidecar_lock_path_multi_extension() {
        let path = Path::new("/tmp/registry.backup.json");
        assert_eq!(
            sidecar_lock_path(path),
            Path::new("/tmp/registry.backup.json.lock")
        );
    }

    #[test]
    fn test_sidecar_lock_path_no_extension() {
        let path = Path::new("/tmp/lockfile");
        assert_eq!(sidecar_lock_path(path), Path::new("/tmp/lockfile.lock"));
    }
}
