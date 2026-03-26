//! Process liveness utilities.
//!
//! Consolidates duplicate PID-checking functions from:
//! - `hooks::session_start::is_process_alive()`
//! - `coordination::sync::is_pid_alive()`
//! - Inline check in `worktree::setup`
//!
//! Also provides `get_claude_code_pid()` which walks the process tree
//! to find the grandparent PID (the actual Claude Code process), instead
//! of using `parent_id()` which returns the ephemeral `sh -c` shell PID.

use std::process::Command;

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

/// Get the Claude Code process PID by walking the process tree.
///
/// The hook process hierarchy is:
///   Claude Code (grandparent) -> sh -c (parent) -> codeflow (us)
///
/// `parent_id()` returns the `sh -c` PID which exits immediately after
/// hook completion, making it useless for liveness detection.
///
/// This function walks up two levels to get the grandparent (Claude Code) PID.
/// Falls back to `parent_id()` if the walk fails.
#[must_use]
pub fn get_claude_code_pid() -> u32 {
    let my_pid = std::process::id();
    let parent = get_parent_of(my_pid);

    match parent {
        Some(ppid) => {
            // Walk one more level: parent of `sh -c` is Claude Code.
            get_parent_of(ppid).unwrap_or(ppid)
        }
        None => {
            // Fallback: use direct parent (same as parent_id()).
            #[cfg(unix)]
            {
                std::os::unix::process::parent_id()
            }
            #[cfg(not(unix))]
            {
                0
            }
        }
    }
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
        let pid = get_claude_code_pid();
        assert!(pid > 0, "should return a non-zero PID: {pid}");
        // The returned PID should be alive (it's our ancestor).
        assert!(
            is_process_alive(pid),
            "grandparent PID should be alive: {pid}"
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
}
