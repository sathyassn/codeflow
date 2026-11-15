//! The declared limits for asking a remote and for the session guards'
//! time budget (TSK-1301). Each limit is declared once here with its
//! default, bounds, class and reason, and code reads it only from here: the
//! policy defaults, the schema check, the guard deadline and the hook
//! timeout written into hook files. The shape is one record per limit, so
//! the limits registry (TSK-1299, "One limits registry, declared once") can
//! take these entries as they are: the probe keys
//! `git.remote_query_timeout_seconds`, `git.remote_query_attempts` and
//! `git.remote_fetch_timeout_seconds`, and the guard hook timeout, deadline,
//! remote lookup wait, pull request lookup wait and refusal write wait.
//! The push set's time budget, `git.push_budget_seconds` (TSK-1306), is
//! declared here too.

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

/// The pre-push set's time budget (TSK-1306): over it, the hook reports
/// the slowest step and the slowest checks inside it. It only reports and
/// never refuses a push, so it holds no safety property.
pub const PUSH_BUDGET: Limit = Limit {
    key: Some("git.push_budget_seconds"),
    name: "push set budget",
    unit: "seconds",
    default: 60,
    min: 1,
    max: None,
    class: LimitClass::Operational,
    reason: "the earlier fixed budget (TSK-132): a push is a backup and should give its verdict inside a minute; a project with a larger history or a slower host sets its own",
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

/// Seconds a guard waits for one `ls-remote` while a repository's remote
/// tracking refs are missing. It is per lookup: a command naming several
/// repositories can make several, and [`GUARD_DEADLINE`] bounds them all.
pub const GUARD_REMOTE_QUERY_WAIT: Limit = Limit {
    key: None,
    name: "guard remote lookup wait",
    unit: "seconds",
    default: 10,
    min: 10,
    max: Some(10),
    class: LimitClass::Security,
    reason: "a 10 s buffer under the 20 s guard deadline for the guard's other work, so one slow remote gets a named fail-closed refusal, never the harness kill; several lookups in one command stop at the deadline, which also refuses; git.remote_query_timeout_seconds does not apply inside a guard, and `git fetch` removes the need to ask",
};

/// Seconds the git guard waits for `gh pr view` to name a pull request's
/// base branch, the guard's other network wait.
pub const GUARD_PR_LOOKUP_WAIT: Limit = Limit {
    key: None,
    name: "guard pull request lookup wait",
    unit: "seconds",
    default: 5,
    min: 5,
    max: Some(5),
    class: LimitClass::Security,
    reason: "fits after one guard remote lookup with a 5 s buffer under the 20 s guard deadline; no answer in time is doubt, and the guard blocks on doubt",
};

/// Seconds the deadline's refusal waits for its reason to reach stderr
/// before the guard exits 2 regardless.
pub const GUARD_REFUSAL_WRITE_WAIT: Limit = Limit {
    key: None,
    name: "guard refusal write wait",
    unit: "seconds",
    default: 1,
    min: 1,
    max: Some(1),
    class: LimitClass::Security,
    reason: "a stderr pipe nobody reads must not hold the exit; the exit still comes well inside the 30 s guard hook timeout",
};

/// Every limit this module declares.
pub const LIMITS: [Limit; 9] = [
    REMOTE_QUERY_TIMEOUT,
    REMOTE_QUERY_ATTEMPTS,
    REMOTE_FETCH_TIMEOUT,
    PUSH_BUDGET,
    GUARD_HOOK_TIMEOUT,
    GUARD_DEADLINE,
    GUARD_REMOTE_QUERY_WAIT,
    GUARD_PR_LOOKUP_WAIT,
    GUARD_REFUSAL_WRITE_WAIT,
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
        const {
            assert!(
                GUARD_REMOTE_QUERY_WAIT.default + GUARD_PR_LOOKUP_WAIT.default + 5
                    <= GUARD_DEADLINE.default
            );
        };
        const {
            assert!(
                GUARD_DEADLINE.default + GUARD_REFUSAL_WRITE_WAIT.default + 5
                    <= GUARD_HOOK_TIMEOUT.default
            );
        };
        assert_eq!(
            for_key("git.remote_query_attempts"),
            Some(&REMOTE_QUERY_ATTEMPTS)
        );
        assert_eq!(for_key("git.push_budget_seconds"), Some(&PUSH_BUDGET));
    }

    /// Everything that sets or spends a configured wait stays out of the
    /// code a guard runs: `QueryLimits::guard` is the only limits a guard
    /// can build. A text scan, so it pins these names and does not trace
    /// calls; `no_configured_wait_reaches_a_guard` in the CLI's
    /// `policy_authority_journey` proves the behaviour.
    #[test]
    fn guard_code_names_no_configured_wait() {
        const CONFIGURED: [&str; 7] = [
            "QueryLimits::from",
            "QueryLimits::default",
            "remote_query::ls_remote",
            "remote_query::fetch_objects",
            "release_line::advertisement",
            "release_line::ask_destination",
            "release_line::checkout_scope",
        ];
        fn scan(path: &std::path::Path, this_file: &std::path::Path, found: &mut Vec<String>) {
            if path.is_dir() {
                for entry in std::fs::read_dir(path).unwrap() {
                    scan(&entry.unwrap().path(), this_file, found);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") && path != this_file {
                let text = std::fs::read_to_string(path).unwrap();
                for name in CONFIGURED {
                    if text.contains(name) {
                        found.push(format!("{}: {name}", path.display()));
                    }
                }
            }
        }
        let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let this_file = core.join("src/hooks/limits.rs");
        let mut found = Vec::new();
        for root in [
            core.join("src/hooks"),
            core.join("src/security"),
            core.join("../codeflow-cli/src/cmd/hook.rs"),
        ] {
            assert!(root.exists(), "{}", root.display());
            scan(&root, &this_file, &mut found);
        }
        assert!(found.is_empty(), "{found:#?}");
    }
}
