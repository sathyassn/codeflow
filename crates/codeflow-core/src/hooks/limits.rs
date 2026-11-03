//! The declared limits for asking a remote and for the session guards'
//! time budget (TSK-1301). Each limit is declared once here with its
//! default, bounds, class and reason, and code reads it only from here: the
//! policy defaults, the schema check, the guard deadline and the hook
//! timeout written into hook files. The shape is one record per limit, so
//! the limits registry (TSK-1299, "One limits registry, declared once") can
//! take these entries as they are: the probe keys
//! `git.remote_query_timeout_seconds`, `git.remote_query_attempts` and
//! `git.remote_fetch_timeout_seconds`, and the guard hook timeout, deadline
//! and network budget.

/// Whether a limit is tuned for speed or holds a safety property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitClass {
    /// Tunes how long work may take; a project may change it in policy.
    Operational,
    /// Keeps a control fail-closed; no policy key changes it.
    Security,
}

/// One declared limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    /// The policy key that sets it, or `None` when no key does.
    pub key: Option<&'static str>,
    /// What it limits, for messages and docs.
    pub name: &'static str,
    /// The unit `default`, `min` and `max` are in.
    pub unit: &'static str,
    /// The shipped value.
    pub default: u64,
    /// The smallest accepted value.
    pub min: u64,
    /// The largest accepted value; `None` when only the integer type
    /// bounds it.
    pub max: Option<u64>,
    pub class: LimitClass,
    /// Why the default is what it is.
    pub reason: &'static str,
}

/// How long one `git ls-remote` attempt may take on the pre-push and
/// `codeflow ci` paths.
pub const REMOTE_QUERY_TIMEOUT: Limit = Limit {
    key: Some("git.remote_query_timeout_seconds"),
    name: "remote query wait per attempt",
    unit: "seconds",
    default: 30,
    min: 1,
    max: None,
    class: LimitClass::Operational,
    reason: "GitHub answered the pre-push default-branch lookup in 11.3 s and 11.6 s on 2026-11-03, over the earlier fixed 10 s; 30 s leaves room above that while a silent destination still refuses",
};

/// How many attempts a timed-out `git ls-remote` gets, the first included.
pub const REMOTE_QUERY_ATTEMPTS: Limit = Limit {
    key: Some("git.remote_query_attempts"),
    name: "remote query attempts",
    unit: "attempts",
    default: 3,
    min: 1,
    max: None,
    class: LimitClass::Operational,
    reason: "the earlier lookup made one attempt and two retries; only a timeout is retried, so a refusal stays final",
};

/// How long a fetch of the objects a judge needs may take.
pub const REMOTE_FETCH_TIMEOUT: Limit = Limit {
    key: Some("git.remote_fetch_timeout_seconds"),
    name: "remote fetch wait",
    unit: "seconds",
    default: 120,
    min: 1,
    max: None,
    class: LimitClass::Operational,
    reason:
        "the earlier fixed fetch wait; a fetch carries objects, so it gets longer than a lookup",
};

/// Seconds a harness waits for a guard hook: the `timeout` every shipped
/// guard hook entry carries in Claude, Codex and Grok files.
pub const GUARD_HOOK_TIMEOUT: Limit = Limit {
    key: None,
    name: "guard hook timeout",
    unit: "seconds",
    default: 30,
    min: 30,
    max: Some(30),
    class: LimitClass::Security,
    reason: "a guard answers in about 0.1 to 0.5 s on a loaded host; 30 s leaves room for a cold start and a slow disk while bounding how long a stalled guard holds a session (TSK-1082)",
};

/// Seconds after which a guard refuses on its own.
pub const GUARD_DEADLINE: Limit = Limit {
    key: None,
    name: "guard deadline",
    unit: "seconds",
    default: 20,
    min: 20,
    max: Some(20),
    class: LimitClass::Security,
    reason: "a 10 s buffer under the 30 s guard hook timeout, so the guard's own refusal reaches the harness before the harness stops waiting and lets the call through in Codex and Grok (TSK-1082)",
};

/// Seconds a guard may wait for the network: one `ls-remote` while the
/// remote's tracking refs are missing.
pub const GUARD_REMOTE_QUERY_BUDGET: Limit = Limit {
    key: None,
    name: "guard network budget",
    unit: "seconds",
    default: 10,
    min: 10,
    max: Some(10),
    class: LimitClass::Security,
    reason: "a 10 s buffer under the 20 s guard deadline for the guard's other work, so a slow remote gets a named fail-closed refusal, never the harness kill; git.remote_query_timeout_seconds does not apply inside a guard, and `git fetch` removes the need to ask",
};

/// Every limit this module declares.
pub const LIMITS: [Limit; 6] = [
    REMOTE_QUERY_TIMEOUT,
    REMOTE_QUERY_ATTEMPTS,
    REMOTE_FETCH_TIMEOUT,
    GUARD_HOOK_TIMEOUT,
    GUARD_DEADLINE,
    GUARD_REMOTE_QUERY_BUDGET,
];

/// The accepted values of `limit`, for messages.
#[must_use]
pub fn bounds_text(limit: &Limit) -> String {
    let max = limit.max.unwrap_or(u64::MAX);
    format!(
        "a whole number of {} from {} to {max}",
        limit.unit, limit.min
    )
}

/// The declared limit a policy key sets, if any.
#[must_use]
pub fn for_key(key: &str) -> Option<&'static Limit> {
    LIMITS.iter().find(|limit| limit.key == Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_sits_inside_its_bounds_and_the_guard_budgets_nest() {
        for limit in LIMITS {
            assert!(limit.default >= limit.min, "{}", limit.name);
            assert!(
                limit.max.is_none_or(|max| limit.default <= max),
                "{}",
                limit.name
            );
            assert_eq!(
                limit.key.is_some(),
                limit.class == LimitClass::Operational,
                "{}: only operational limits have a policy key",
                limit.name
            );
        }
        const { assert!(GUARD_REMOTE_QUERY_BUDGET.default + 5 <= GUARD_DEADLINE.default) };
        const { assert!(GUARD_DEADLINE.default + 5 <= GUARD_HOOK_TIMEOUT.default) };
        assert_eq!(
            for_key("git.remote_query_attempts"),
            Some(&REMOTE_QUERY_ATTEMPTS)
        );
    }
}
