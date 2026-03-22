//! File-based locking utilities.
//!
//! Provides locked read-modify-write (RMW) for JSON and binary state files
//! using sidecar `.lock` files with `fs2::lock_exclusive`. Used by PathFlow
//! state files, coordination state (state.loro), and any other files requiring
//! exclusive access across concurrent hook invocations.
//!
//! Lock mechanism: sidecar `{path}.lock` file + `fs2::FileExt::lock_exclusive()`.
//! The data file itself is never locked directly.

use std::fs;
use std::path::Path;

use fs2::FileExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Sidecar lock helpers
// ---------------------------------------------------------------------------

/// Acquire an exclusive sidecar lock for the given data file path.
///
/// Creates `{path}.lock` (ensuring parent dirs exist) and acquires an
/// exclusive `fs2` lock on it. Returns the lock file handle — the lock is
/// held until the handle is dropped.
fn acquire_exclusive_lock(path: &Path) -> Result<fs::File, String> {
    let lock_path = sidecar_lock_path(path);

    if let Some(dir) = lock_path.parent() {
        let _ = fs::create_dir_all(dir);
    }

    let lock_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| format!("lock open: {e}"))?;

    FileExt::lock_exclusive(&lock_file).map_err(|e| format!("lock acquire: {e}"))?;

    Ok(lock_file)
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
/// For files with a recognized extension, appends `.lock` to the full
/// extension (e.g., `foo.json` → `foo.json.lock`, `state.loro` → `state.loro.lock`).
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
    let _lock = acquire_exclusive_lock(path)?;

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
/// Use when read failure would trigger an irreversible destructive action
/// (e.g., session sweep deleting artifacts). Retries only if the parent
/// directory exists (indicating transient unavailability, not genuine absence).
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
/// Used by checkpoint.rs for `CheckpointFile` and any other typed state.
///
/// # Errors
///
/// Returns `Err(String)` on lock, read, parse, callback, serialize, or write failure.
pub fn locked_rmw_typed<T, F>(path: &Path, default_fn: fn() -> T, f: F) -> Result<(), String>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce(&mut T) -> Result<(), String>,
{
    let _lock = acquire_exclusive_lock(path)?;

    // Read (default on NotFound)
    let mut value: T = match fs::read_to_string(path) {
        Ok(data) => serde_json::from_str(&data).map_err(|e| format!("parse: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => default_fn(),
        Err(e) => return Err(format!("read: {e}")),
    };

    // Modify (fallible)
    f(&mut value)?;

    // Write atomically (tmp + rename)
    let pretty = serde_json::to_string_pretty(&value).map_err(|e| format!("serialize: {e}"))?;
    atomic_write(path, format!("{pretty}\n").as_bytes()).map_err(|e| format!("write: {e}"))?;

    // Lock released on drop
    Ok(())
}

// ---------------------------------------------------------------------------
// Binary operations
// ---------------------------------------------------------------------------

/// Perform a locked read-modify-write on a binary file.
///
/// Like [`locked_rmw`] but works with raw bytes instead of JSON. The caller
/// provides `load` and `save` functions to convert between bytes and the
/// in-memory representation.
///
/// If the file does not exist, `default_fn` provides the initial value.
///
/// Used for binary state files such as `state.loro` (Loro CRDT snapshots).
///
/// # Errors
///
/// Returns `Err(String)` on lock, read, callback, or write failure.
pub fn locked_binary_rmw<T, L, S, F>(
    path: &Path,
    default_fn: fn() -> T,
    load: L,
    save: S,
    f: F,
) -> Result<(), String>
where
    L: FnOnce(&[u8]) -> Result<T, String>,
    S: FnOnce(&T) -> Result<Vec<u8>, String>,
    F: FnOnce(&mut T) -> Result<(), String>,
{
    let _lock = acquire_exclusive_lock(path)?;

    // Read (default on NotFound or empty file)
    let mut value: T = match fs::read(path) {
        Ok(data) if data.is_empty() => default_fn(),
        Ok(data) => load(&data)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => default_fn(),
        Err(e) => return Err(format!("read: {e}")),
    };

    // Modify (fallible)
    f(&mut value)?;

    // Serialize and write atomically
    let bytes = save(&value)?;
    atomic_write(path, &bytes).map_err(|e| format!("write: {e}"))?;

    // Lock released on drop
    Ok(())
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
            v["status"] = Value::String("pf-started".into());
            v["count"] = serde_json::json!(1);
        });
        assert!(result.is_ok(), "locked_rmw failed: {result:?}");

        let updated: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(updated["status"], "pf-started");
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
        fs::write(&path, r#"{"status":"pf-started"}"#).unwrap();

        let value = locked_read(&path).unwrap();
        assert_eq!(value["status"], "pf-started");
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

    // -- Binary RMW tests --

    #[test]
    fn test_locked_binary_rmw_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.loro");
        fs::write(&path, b"initial-data").unwrap();

        let result = locked_binary_rmw(
            &path,
            Vec::new,
            |bytes| Ok(bytes.to_vec()),
            |data| Ok(data.clone()),
            |data| {
                data.extend_from_slice(b"-modified");
                Ok(())
            },
        );
        assert!(result.is_ok(), "locked_binary_rmw failed: {result:?}");

        let updated = fs::read(&path).unwrap();
        assert_eq!(updated, b"initial-data-modified");
    }

    #[test]
    fn test_locked_binary_rmw_creates_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.loro");
        fs::write(&path, b"data").unwrap();

        let _ = locked_binary_rmw(
            &path,
            Vec::new,
            |bytes| Ok(bytes.to_vec()),
            |data| Ok(data.clone()),
            |_| Ok(()),
        );

        let lock_path = sidecar_lock_path(&path);
        assert!(lock_path.exists(), "lock file should be created");
        assert_eq!(
            lock_path.file_name().unwrap().to_str().unwrap(),
            "state.loro.lock"
        );
    }

    #[test]
    fn test_locked_binary_rmw_default_on_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.loro");
        // File does not exist — should use default_fn.

        let result = locked_binary_rmw(
            &path,
            || vec![0xDE, 0xAD],
            |bytes| Ok(bytes.to_vec()),
            |data| Ok(data.clone()),
            |data| {
                data.push(0xBE);
                Ok(())
            },
        );
        assert!(result.is_ok());

        let written = fs::read(&path).unwrap();
        assert_eq!(written, vec![0xDE, 0xAD, 0xBE]);
    }

    #[test]
    fn test_locked_binary_rmw_default_on_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.loro");
        fs::write(&path, b"").unwrap(); // Empty file

        let result = locked_binary_rmw(
            &path,
            || vec![0xFF],
            |bytes| Ok(bytes.to_vec()),
            |data| Ok(data.clone()),
            |data| {
                data.push(0x01);
                Ok(())
            },
        );
        assert!(result.is_ok());

        let written = fs::read(&path).unwrap();
        assert_eq!(written, vec![0xFF, 0x01]);
    }

    #[test]
    fn test_locked_binary_rmw_callback_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.loro");
        fs::write(&path, b"original").unwrap();

        let result = locked_binary_rmw(
            &path,
            Vec::new,
            |bytes| Ok(bytes.to_vec()),
            |data| Ok(data.clone()),
            |_| Err("abort".to_string()),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("abort"));

        // File should be unchanged.
        let unchanged = fs::read(&path).unwrap();
        assert_eq!(unchanged, b"original");
    }

    #[test]
    fn test_locked_binary_rmw_load_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.loro");
        fs::write(&path, b"corrupt-data").unwrap();

        let result = locked_binary_rmw(
            &path,
            Vec::new,
            |_bytes| Err("decode failed".to_string()),
            |data| Ok(data.clone()),
            |_| Ok(()),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("decode failed"));
    }

    #[test]
    fn test_locked_binary_rmw_save_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.loro");
        fs::write(&path, b"data").unwrap();

        let result = locked_binary_rmw(
            &path,
            Vec::new,
            |bytes| Ok(bytes.to_vec()),
            |_data| Err("encode failed".to_string()),
            |_| Ok(()),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("encode failed"));
    }

    #[test]
    fn test_sidecar_lock_path_json() {
        let path = Path::new("/tmp/foo.json");
        assert_eq!(sidecar_lock_path(path), Path::new("/tmp/foo.json.lock"));
    }

    #[test]
    fn test_sidecar_lock_path_loro() {
        let path = Path::new("/tmp/state.loro");
        assert_eq!(sidecar_lock_path(path), Path::new("/tmp/state.loro.lock"));
    }

    #[test]
    fn test_sidecar_lock_path_no_extension() {
        let path = Path::new("/tmp/lockfile");
        assert_eq!(sidecar_lock_path(path), Path::new("/tmp/lockfile.lock"));
    }

    #[test]
    fn test_locked_binary_rmw_sequential_consistency() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("counter.bin");
        fs::write(&path, &0u32.to_le_bytes()).unwrap();

        for _ in 0..10 {
            let _ = locked_binary_rmw(
                &path,
                || 0u32,
                |bytes| {
                    let arr: [u8; 4] = bytes.try_into().map_err(|_| "bad len".to_string())?;
                    Ok(u32::from_le_bytes(arr))
                },
                |val| Ok(val.to_le_bytes().to_vec()),
                |val| {
                    *val += 1;
                    Ok(())
                },
            );
        }

        let final_bytes = fs::read(&path).unwrap();
        let final_val = u32::from_le_bytes(final_bytes.try_into().unwrap());
        assert_eq!(final_val, 10);
    }
}
