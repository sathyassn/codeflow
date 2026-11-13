//! Retired controls: load old files and report their lack of effect.
//! These entries are not part of the effective policy schema or defaults.
use super::policy_schema::{KeyKind, KeySpec};
const LEVEL_VALID: &str = "off | warn | allow | block";

pub const UNSUPPORTED_KEYS: &[&str] = &[
    "git.direct_changes",
    "git.clean_regenerable",
    "security.script_bypass",
    "security.interpreter_scan",
    "security.enforcement_baseline",
    "security.workflow_pushes",
    "security.sandbox_retry",
    "security.sandbox_retry_allow",
];

const LEGACY: [KeySpec; 7] = [
    KeySpec {
        path: "git.clean_regenerable",
        kind: KeyKind::StringList,
        valid: "an array of directory paths, each ending with `/`",
        purpose: "Not supported, has no effect. Accepted only for compatibility with older policy files.",
        notes: "No enforcement code reads this key. Existing guard rules apply independently of its value.",
    },
    KeySpec {
        path: "security.script_bypass",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Not supported, has no effect. Accepted only for compatibility with older policy files.",
        notes: "There is no enforcement reader. Hooks warn when this key is present. Update removes a prior shipped value and keeps an edited value.",
    },
    KeySpec {
        path: "security.interpreter_scan",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Not supported, has no effect. Accepted only for compatibility with older policy files.",
        notes: "No enforcement code reads this key. Existing guard rules apply independently of its value.",
    },
    KeySpec {
        path: "security.enforcement_baseline",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Not supported, has no effect. Accepted only for compatibility with older policy files.",
        notes: "No enforcement code reads this key. Existing guard rules apply independently of its value.",
    },
    KeySpec {
        path: "security.workflow_pushes",
        kind: KeyKind::Enum(&["queue", "allow"]),
        valid: "queue | allow",
        purpose: "Not supported, has no effect. Accepted only for compatibility with older policy files.",
        notes: "No enforcement code reads this key. Existing guard rules apply independently of its value.",
    },
    KeySpec {
        path: "security.sandbox_retry",
        kind: KeyKind::Enum(&["allowlist", "block"]),
        valid: "allowlist | block",
        purpose: "Not supported, has no effect. Accepted only for compatibility with older policy files.",
        notes: "There is no enforcement reader. Hooks warn when this key is present. Update removes a prior shipped value and keeps an edited value.",
    },
    KeySpec {
        path: "security.sandbox_retry_allow",
        kind: KeyKind::RetryEntries,
        valid: "an array of entry objects, each naming its `program`",
        purpose: "Not supported, has no effect. Accepted only for compatibility with older policy files.",
        notes: "There is no enforcement reader. Hooks warn when this key is present. Update removes a prior shipped value and keeps an edited value.",
    },
];

pub fn spec_for(path: &str) -> Option<&'static KeySpec> {
    LEGACY.iter().find(|spec| spec.path == path)
}
