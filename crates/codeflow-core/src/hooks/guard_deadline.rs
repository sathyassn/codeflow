//! How long a session guard may take, in the harness and in itself
//! (TSK-1082).
//!
//! Claude Code, Codex and Grok all let a `PreToolUse` tool call through
//! when a command hook times out, and the shell wrapper around each guard
//! never runs on a kill. Two bounds close that where they can:
//!
//! - every shipped guard hook entry carries [`GUARD_HOOK_TIMEOUT_SECS`],
//!   and Claude's carries `"onFailure": "block"`, so a Claude timeout, crash
//!   or start failure blocks the call;
//! - the guard process refuses on its own, with exit 2 and a reason on
//!   stderr, once [`GUARD_DEADLINE_SECS`] pass without a decision. Exit 2
//!   with a reason blocks in all three harnesses, so a guard stalled inside
//!   its own process (a FIFO named by a git include, a hung filesystem)
//!   blocks in Grok and Codex too, which have no fail-closed setting.
//!
//! What stays open: a guard that cannot reach its own deadline, because the
//! process does not start or is starved past the harness timeout, fails open
//! in Grok and Codex. Claude blocks it through `onFailure`.

/// Seconds a harness waits for a guard before it gives up on it: the
/// `timeout` every shipped guard hook entry carries, the same in Claude,
/// Codex and Grok files (all three read seconds). The guard answers in
/// about 0.1 to 0.5 seconds on a loaded host, so this leaves room for a
/// cold start and a slow disk while bounding how long a stalled guard holds
/// a session.
pub const GUARD_HOOK_TIMEOUT_SECS: u64 = 30;

/// Seconds after which a guard process refuses on its own, well inside
/// [`GUARD_HOOK_TIMEOUT_SECS`] so the refusal reaches the harness before
/// the harness stops waiting.
pub const GUARD_DEADLINE_SECS: u64 = 20;

/// The session guards whose hook entries carry these bounds.
pub const GUARDS: [&str; 3] = ["git-guard", "exec-guard", "edit-guard"];

/// The guard a codeflow hook command runs (`codeflow hook <guard> …`), or
/// `None` for any other command.
#[must_use]
pub fn guard_of(command: &str) -> Option<&'static str> {
    let mut words = command.split([' ', '\t', '\n']).filter(|w| !w.is_empty());
    let program = words.next()?;
    if program.rsplit('/').next() != Some("codeflow") || words.next() != Some("hook") {
        return None;
    }
    let name = words.next()?;
    GUARDS.iter().copied().find(|guard| *guard == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deadline_falls_inside_the_harness_timeout() {
        const { assert!(GUARD_DEADLINE_SECS + 5 <= GUARD_HOOK_TIMEOUT_SECS) };
    }

    #[test]
    fn only_a_codeflow_guard_command_names_a_guard() {
        assert_eq!(
            guard_of("codeflow hook git-guard --contract 3 || { exit 2; }"),
            Some("git-guard")
        );
        assert_eq!(
            guard_of("/usr/local/bin/codeflow hook edit-guard"),
            Some("edit-guard")
        );
        assert_eq!(guard_of("codeflow hook exec-guard"), Some("exec-guard"));
        for other in [
            "codeflow hook session-orient --contract 3",
            "codeflow hook",
            "my-codeflow hook git-guard",
            "echo codeflow hook git-guard",
            "codeflow git-guard",
        ] {
            assert_eq!(guard_of(other), None, "{other}");
        }
    }
}
