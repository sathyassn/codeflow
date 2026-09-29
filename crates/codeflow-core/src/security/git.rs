//! Protected-branch matching shared by the hook policy and `integrate`.
//!
//! The v1 `GitModule` command scanner that lived here was never wired to a
//! production plane and was removed (TSK-137, ADR-0008 amendment); the live
//! git protections are `hooks/git_guard.rs`.

use regex::Regex;

/// Check if the given branch is in the protected list.
#[must_use]
pub fn is_on_protected_branch(branch: &str, policy: &crate::security::SecurityPolicy) -> bool {
    if branch.is_empty() {
        return false;
    }

    let branches = policy.protected_branch_list();

    for pb in &branches {
        if pb.contains('*') {
            // Wildcard pattern: convert to regex.
            let pattern = format!("^{}$", regex::escape(pb).replace(r"\*", ".*"));
            if let Ok(re) = Regex::new(&pattern) {
                if re.is_match(branch) {
                    return true;
                }
            }
        } else if branch == pb.as_str() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::SecurityPolicy;

    #[test]
    fn test_is_on_protected_branch_defaults() {
        let policy = SecurityPolicy::defaults();
        assert!(is_on_protected_branch("main", &policy));
        assert!(is_on_protected_branch("master", &policy));
        assert!(!is_on_protected_branch("feat/test", &policy));
        assert!(!is_on_protected_branch("", &policy));
    }

    #[test]
    fn test_is_on_protected_branch_glob_patterns() {
        let policy = SecurityPolicy {
            protected_branches: vec!["main".into(), "release/*".into()],
            ..SecurityPolicy::defaults()
        };
        assert!(is_on_protected_branch("main", &policy));
        assert!(is_on_protected_branch("release/v1.0", &policy));
        assert!(!is_on_protected_branch("release", &policy));
        assert!(!is_on_protected_branch("feat/release", &policy));
    }
}
