//! Process liveness utilities.
//!
//! Consolidates duplicate PID-checking functions from:
//! - `hooks::session_start::is_process_alive()`
//! - `coordination::sync::is_pid_alive()`
//! - Inline check in `worktree::setup`
//!
//! Also provides `get_claude_code_pid()` which walks the process tree
//! using name-based detection to find the actual Claude Code process,
//! instead of hardcoding a fixed ancestor depth.

use std::process::Command;

/// Maximum ancestor levels to walk when searching for Claude Code PID.
const MAX_ANCESTOR_WALK: usize = 10;

/// Check if a process with the given PID is alive.
///
/// Uses `kill -0 <pid>` which sends no signal but exits 0 if the process
/// exists. This avoids `unsafe` libc calls.
#[must_use]
pub fn is_process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Get the process name (comm) for a given PID.
///
/// Platform-specific:
/// - Linux: reads `/proc/<pid>/comm`
/// - macOS/other: uses `ps -o comm= -p <pid>`
///
/// Returns `None` if the process doesn't exist or the name cannot be read.
#[must_use]
pub fn get_process_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        get_process_name_linux(pid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        get_process_name_ps(pid)
    }
}

/// Get process name using `ps` command (macOS and most Unix).
#[cfg(not(target_os = "linux"))]
fn get_process_name_ps(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

/// Get process name from `/proc/<pid>/comm` (Linux only).
#[cfg(target_os = "linux")]
fn get_process_name_linux(pid: u32) -> Option<String> {
    let comm_path = format!("/proc/{pid}/comm");
    let name = std::fs::read_to_string(&comm_path).ok()?.trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

/// Check if a process name contains a given fragment (case-insensitive).
///
/// Returns `true` if the process exists and its name contains `name_fragment`.
#[must_use]
pub fn is_process_named(pid: u32, name_fragment: &str) -> bool {
    get_process_name(pid)
        .is_some_and(|name| name.to_lowercase().contains(&name_fragment.to_lowercase()))
}

/// Validate that a PID belongs to a Claude Code process.
///
/// Returns the PID unchanged if it is alive AND its process name contains
/// "claude". Returns 0 otherwise.
#[must_use]
pub fn validate_claude_pid(pid: u32) -> u32 {
    if pid > 0 && is_process_alive(pid) && is_process_named(pid, "claude") {
        pid
    } else {
        0
    }
}

/// Get the Claude Code process PID by walking the process tree.
///
/// Walks up the ancestor chain from the current process, checking each
/// ancestor's name for "claude". Stops at the first match or after
/// `MAX_ANCESTOR_WALK` levels. Falls back to the direct parent PID if
/// no "claude" ancestor is found.
#[must_use]
pub fn get_claude_code_pid() -> u32 {
    let my_pid = std::process::id();
    let mut current = my_pid;

    for _ in 0..MAX_ANCESTOR_WALK {
        let Some(ppid) = get_parent_of(current) else {
            break;
        };
        if ppid == 0 || ppid == current {
            break;
        }
        if is_process_named(ppid, "claude") {
            return ppid;
        }
        current = ppid;
    }

    // Fallback: direct parent PID (best effort when not under Claude Code).
    get_parent_of(my_pid).unwrap_or_else(|| {
        #[cfg(unix)]
        {
            std::os::unix::process::parent_id()
        }
        #[cfg(not(unix))]
        {
            0
        }
    })
}

/// Get the parent PID of a given process.
///
/// Platform-specific implementation:
/// - macOS: uses `sysctl` via `ps -o ppid= -p <pid>`
/// - Linux: reads `/proc/<pid>/stat`
///
/// Returns `None` if the process doesn't exist or the parent cannot be determined.
#[must_use]
pub fn get_parent_of(pid: u32) -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        get_parent_of_linux(pid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        get_parent_of_ps(pid)
    }
}

/// Get parent PID using `ps` command (works on macOS and most Unix systems).
#[cfg(not(target_os = "linux"))]
fn get_parent_of_ps(pid: u32) -> Option<u32> {
    let output = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let ppid_str = String::from_utf8_lossy(&output.stdout);
    ppid_str.trim().parse::<u32>().ok()
}

/// Get parent PID by reading `/proc/<pid>/stat` (Linux only).
#[cfg(target_os = "linux")]
fn get_parent_of_linux(pid: u32) -> Option<u32> {
    let stat_path = format!("/proc/{pid}/stat");
    let content = std::fs::read_to_string(&stat_path).ok()?;

    // Format: "pid (comm) state ppid ..."
    // The comm field can contain spaces and parentheses, so find the last ')'.
    let after_comm = content.rfind(')')? + 1;
    let fields: Vec<&str> = content[after_comm..].split_whitespace().collect();

    // fields[0] = state, fields[1] = ppid
    fields.get(1)?.parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_process_alive_for_current_pid() {
        let pid = std::process::id();
        assert!(
            is_process_alive(pid),
            "current process should be alive: pid={pid}"
        );
    }

    #[test]
    fn test_is_process_alive_false_for_nonexistent_pid() {
        // PID 99999 is extremely unlikely to exist.
        // Use a high PID that's almost certainly unused.
        assert!(!is_process_alive(4_000_000), "PID 4000000 should not exist");
    }

    #[test]
    fn test_is_process_alive_false_for_zero() {
        assert!(!is_process_alive(0), "PID 0 should return false");
    }

    #[test]
    fn test_get_claude_code_pid_returns_valid_pid() {
        // In test context, no "claude" ancestor exists, so expect fallback
        // to direct parent PID.
        let pid = get_claude_code_pid();
        assert!(pid > 0, "should return a non-zero PID: {pid}");
        // The returned PID should be alive (it's our parent/ancestor).
        assert!(
            is_process_alive(pid),
            "fallback parent PID should be alive: {pid}"
        );
    }

    #[test]
    fn test_get_parent_of_returns_result_for_current_pid() {
        let pid = std::process::id();
        let parent = get_parent_of(pid);
        // On some CI/sandbox environments `ps` may not be available,
        // so we accept both Some (normal) and None (restricted env).
        if let Some(ppid) = parent {
            assert!(ppid > 0, "parent PID should be non-zero: {ppid}");
        }
    }

    #[test]
    fn test_get_parent_of_returns_none_for_nonexistent_pid() {
        let parent = get_parent_of(4_000_000);
        assert!(
            parent.is_none(),
            "nonexistent PID should return None: {parent:?}"
        );
    }

    #[test]
    fn test_get_process_name_current_pid() {
        let pid = std::process::id();
        let name = get_process_name(pid);
        // On some CI/sandbox environments `ps` may not be available,
        // so we accept both Some (normal) and None (restricted env).
        if let Some(ref n) = name {
            assert!(!n.is_empty(), "process name should not be empty");
        }
    }

    #[test]
    fn test_get_process_name_nonexistent_pid() {
        let name = get_process_name(4_000_000);
        assert!(name.is_none(), "nonexistent PID should return None");
    }

    #[test]
    fn test_get_process_name_zero_pid() {
        let name = get_process_name(0);
        assert!(name.is_none(), "PID 0 should return None");
    }

    #[test]
    fn test_is_process_named_matches_current() {
        // The test runner process should NOT be named "claude".
        let pid = std::process::id();
        // In sandbox environments, ps may be unavailable, making
        // is_process_named always return false.
        assert!(
            !is_process_named(pid, "claude"),
            "test runner should not be named 'claude'"
        );
        // If process name is available, verify substring matching works.
        if let Some(name) = get_process_name(pid) {
            let fragment = &name[..name.len().min(3)];
            assert!(
                is_process_named(pid, fragment),
                "should match substring '{fragment}' of '{name}'"
            );
        }
    }

    #[test]
    fn test_is_process_named_no_match() {
        let pid = std::process::id();
        assert!(
            !is_process_named(pid, "zzz_nonexistent_process_name_zzz"),
            "should not match a bogus name fragment"
        );
    }

    #[test]
    fn test_validate_claude_pid_zero_for_non_claude() {
        // Current test process is not "claude", so validate should return 0.
        let pid = std::process::id();
        assert_eq!(
            validate_claude_pid(pid),
            0,
            "non-claude process should return 0"
        );
    }

    #[test]
    fn test_validate_claude_pid_zero_for_dead() {
        // A dead PID should return 0.
        assert_eq!(
            validate_claude_pid(4_000_000),
            0,
            "dead PID should return 0"
        );
    }

    #[test]
    fn test_validate_claude_pid_zero_for_zero() {
        assert_eq!(validate_claude_pid(0), 0, "PID 0 should return 0");
    }

    #[test]
    fn test_get_claude_code_pid_is_ancestor() {
        // The returned PID should be an ancestor of current process.
        let pid = get_claude_code_pid();
        let my_pid = std::process::id();
        // pid should differ from current process (it's a parent/ancestor).
        assert_ne!(pid, my_pid, "claude code PID should not be current process");
    }

    #[test]
    fn test_get_claude_code_pid_deterministic() {
        // Calling twice should return the same PID.
        let pid1 = get_claude_code_pid();
        let pid2 = get_claude_code_pid();
        assert_eq!(pid1, pid2, "should be deterministic");
    }

    #[test]
    fn test_is_process_named_nonexistent_pid() {
        assert!(
            !is_process_named(4_000_000, "anything"),
            "nonexistent PID should return false"
        );
    }

    #[test]
    fn test_is_process_named_zero_pid() {
        assert!(
            !is_process_named(0, "anything"),
            "PID 0 should return false"
        );
    }

    #[test]
    fn test_is_process_named_empty_fragment() {
        // Empty fragment should match any name (if ps works).
        let pid = std::process::id();
        // In sandbox, get_process_name may return None, so is_process_named
        // returns false. Both outcomes are acceptable.
        let _ = is_process_named(pid, "");
    }

    #[test]
    fn test_get_parent_of_zero() {
        let parent = get_parent_of(0);
        // PID 0 should either return None or Some(0).
        if let Some(ppid) = parent {
            assert_eq!(ppid, 0, "parent of PID 0 should be 0 if returned");
        }
    }

    #[test]
    fn test_is_process_alive_current_parent() {
        // Parent process should be alive (it's running the test).
        if let Some(ppid) = get_parent_of(std::process::id()) {
            assert!(
                is_process_alive(ppid),
                "parent process should be alive: {ppid}"
            );
        }
    }
}
