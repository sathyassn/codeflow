use super::*;

fn settable() -> impl Iterator<Item = &'static Limit> {
    LIMITS.iter().filter(|limit| limit.route == Route::Policy)
}

#[test]
fn every_default_sits_inside_its_bounds() {
    for limit in LIMITS {
        assert!(limit.default >= limit.min, "{}", limit.name);
        assert!(
            limit.max.is_none_or(|max| limit.default <= max),
            "{}",
            limit.name
        );
    }
}

#[test]
fn every_entry_says_why_it_has_its_value() {
    for limit in LIMITS {
        assert!(!limit.name.is_empty());
        assert!(!limit.reason.is_empty(), "{}", limit.name);
    }
}

#[test]
fn every_key_is_declared_once() {
    let mut keys: Vec<_> = LIMITS.iter().filter_map(|limit| limit.key).collect();
    let all = keys.len();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), all);
}

#[test]
fn a_policy_key_is_found_by_name_and_only_on_the_policy_route() {
    for limit in settable() {
        let key = limit.key.expect("a policy entry has a key");
        assert_eq!(for_key(key), Some(limit));
    }
    assert_eq!(for_key("timeout_seconds"), None);
    assert_eq!(for_key("limits.no_such_limit"), None);
}

/// A project never meets a ceiling on an operational or guidance value. The
/// entries below are the disclosed exceptions, where arithmetic needs one.
#[test]
fn only_security_limits_have_a_ceiling() {
    const ARITHMETIC: [&str; 4] = [
        "timeout_seconds",
        "stall_timeout_seconds",
        "cleanup_grace_seconds",
        "--timeout-seconds",
    ];
    for limit in LIMITS {
        if limit.class == LimitClass::Security || limit.route == Route::Fixed {
            continue;
        }
        let key = limit.key.unwrap_or_default();
        if limit.max.is_some() && !ARITHMETIC.contains(&key) {
            assert!(
                limit.max.is_some_and(|max| max >= u64::from(u32::MAX)),
                "{key} has a ceiling"
            );
        }
    }
}

#[test]
fn a_security_key_has_a_floor_and_a_ceiling() {
    for limit in LIMITS {
        if limit.class == LimitClass::Security && limit.route == Route::Policy {
            let max = limit.max.expect("a security key has a ceiling");
            assert!(limit.min >= 1, "{}", limit.name);
            assert!(limit.min <= max, "{}", limit.name);
        }
    }
}

#[test]
fn accepts_holds_the_bounds_exactly() {
    for limit in settable() {
        assert!(limit.accepts(limit.default), "{}", limit.name);
        assert!(limit.accepts(limit.min), "{}", limit.name);
        if limit.min > 0 {
            assert!(!limit.accepts(limit.min - 1), "{}", limit.name);
        }
        match limit.max {
            Some(max) => {
                assert!(limit.accepts(max), "{}", limit.name);
                if max < u64::MAX {
                    assert!(!limit.accepts(max + 1), "{}", limit.name);
                }
            }
            None => assert!(limit.accepts(u64::MAX), "{}", limit.name),
        }
    }
}

#[test]
fn a_guidance_entry_takes_any_large_value_and_carries_its_warning() {
    let entries: Vec<_> = LIMITS
        .iter()
        .filter(|limit| limit.class == LimitClass::Guidance)
        .collect();
    for needed in [&HOST_JOBS, &MEMORY_PER_JOB, &HEAVY_WAIT, &WORKER_BUDGET] {
        assert!(entries.contains(&needed), "{}", needed.name);
    }
    for limit in entries {
        assert_eq!(limit.max, None, "{}", limit.name);
        assert!(limit.accepts(u64::MAX), "{}", limit.name);
        assert!(!limit.warning.is_empty(), "{}", limit.name);
    }
}

/// The ceilings hold the same nesting the defaults do, so a project at every
/// ceiling at once still gets a named refusal before the harness stops waiting.
#[test]
fn the_guard_budgets_nest_at_their_defaults_and_at_their_ceilings() {
    let deadline = GUARD_DEADLINE.max.expect("a security key has a ceiling");
    let remote = GUARD_REMOTE_QUERY_WAIT.default;
    let lookup = GUARD_PR_LOOKUP_WAIT
        .max
        .expect("a security key has a ceiling");
    const {
        assert!(
            GUARD_REMOTE_QUERY_WAIT.default + GUARD_PR_LOOKUP_WAIT.default + 5
                <= GUARD_DEADLINE.default
        );
        assert!(
            GUARD_DEADLINE.default + GUARD_REFUSAL_WRITE_WAIT.default + 5
                <= GUARD_HOOK_TIMEOUT.default
        );
    };
    assert!(remote + lookup + 5 <= deadline);
    assert!(deadline + GUARD_REFUSAL_WRITE_WAIT.default + 5 <= GUARD_HOOK_TIMEOUT.default);
    assert_eq!(GUARD_HOOK_TIMEOUT.route, Route::Fixed);
    assert_eq!(GUARD_REMOTE_QUERY_WAIT.route, Route::Fixed);
    assert_eq!(GUARD_REFUSAL_WRITE_WAIT.route, Route::Fixed);
}

#[test]
fn the_named_keys_keep_their_shipped_defaults() {
    assert_eq!(
        for_key("git.remote_query_attempts"),
        Some(&REMOTE_QUERY_ATTEMPTS)
    );
    assert_eq!(COORD_CHECK_INTERVAL.default, 600);
    assert_eq!(COORD_HEARTBEAT_TIMEOUT.default, 1800);
    assert_eq!(REMOTE_QUERY_TIMEOUT.default, 30);
    assert_eq!(REMOTE_QUERY_ATTEMPTS.default, 3);
    assert_eq!(REMOTE_FETCH_TIMEOUT.default, 120);
    assert_eq!(GLOB_PATH_BYTES.default, 32_000);
}

#[test]
fn these_security_bounds_are_not_project_settable() {
    for limit in [
        &DELETION_READER_DEPTH,
        &DELETION_READER_VALUES,
        &DELETION_READER_VARIANTS,
        &DELETION_READER_LOOP_PASSES,
        &DELETION_GLOB_LIMIT,
        &GUARD_SCRIPT_READINGS,
        &GUARD_NESTING_LIMIT,
        &STARTUP_LINK_LIMIT,
        &ARGUMENT_WALK_LIMIT,
        &GUARD_ALIAS_DEPTH,
        &GUARD_SCRIPT_REPARSE_DEPTH,
    ] {
        assert_eq!(limit.route, Route::Fixed, "{}", limit.name);
        assert_eq!(limit.key, None, "{}", limit.name);
    }
}

#[test]
fn values_read_the_default_until_a_project_installs_one() {
    assert_eq!(MEMORY_PER_JOB.value(), MEMORY_PER_JOB.default);
    assert_eq!(GUARD_DEADLINE.wait().as_secs(), GUARD_DEADLINE.default);
    assert_eq!(
        POLL_WAIT_MILLIS.wait().as_millis(),
        u128::from(POLL_WAIT_MILLIS.default)
    );
}
