//! The `.codeflow/policy.json` key schema — one registry describing every
//! leaf key (path, kind, valid values, purpose, sharp edges), plus the strict
//! validator built on it.
//!
//! Consumers get only the binary, so the file must be fully discoverable and
//! strictly checkable from the binary alone: `codeflow policy explain` renders
//! this registry, `codeflow policy show` reads it to report effective values,
//! and [`validate_policy_file`] walks it to fail loudly on an invalid file —
//! naming the offending key, its value, and the valid set — instead of the
//! enforcement loader's silent whole-file fallback to defaults (which would
//! quietly discard a consumer's hardened configuration). Defaults are rendered
//! live from the real [`Policy::default`], never hand-maintained literals, and
//! a drift-guard test pins the registry to the serde fields both ways.

use std::fmt;
use std::path::Path;

use serde_json::Value;

use super::policy::Policy;

/// The four accepted [`PolicyLevel`](super::PolicyLevel) spellings, in the
/// serde `lowercase` form a policy file must use.
pub const LEVEL_VALUES: [&str; 4] = ["off", "warn", "allow", "block"];

/// The [`LEVEL_VALUES`] joined for error messages and rendering.
pub const LEVEL_VALUES_TEXT: &str = "off, warn, allow, block";

/// One-paragraph legend for what each level does — `explain` prints it once
/// instead of repeating it per key.
pub const LEVEL_LEGEND: &str =
    "block = violations stop the operation; warn = violations are reported \
     and the operation proceeds; allow = explicitly permitted; off = the check \
     is not run. `allow` and `off` are BOTH inactive; only warn/block enforce.";

/// The JSON value kind of a policy leaf key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// An enforcement level: one of [`LEVEL_VALUES`].
    Level,
    /// A non-negative integer (`u32`).
    UInt,
    /// An array of strings.
    StringList,
    /// A free-form string.
    String,
    /// A closed enum accepting exactly the listed values.
    Enum(&'static [&'static str]),
}

impl KeyKind {
    /// Short type name for rendering (`Level`, `uint`, `string list`, …).
    #[must_use]
    pub fn type_name(self) -> &'static str {
        match self {
            Self::Level => "Level",
            Self::UInt => "uint",
            Self::StringList => "string list",
            Self::String => "string",
            Self::Enum(_) => "enum",
        }
    }
}

/// Schema entry for one leaf key of `.codeflow/policy.json`.
#[derive(Debug, Clone, Copy)]
pub struct KeySpec {
    /// Dotted leaf path, e.g. `git.commit_ticket_pattern`.
    pub path: &'static str,
    /// The JSON value kind the key accepts.
    pub kind: KeyKind,
    /// Valid values / format, as consumer-facing text.
    pub valid: &'static str,
    /// One-line purpose.
    pub purpose: &'static str,
    /// Sharp edges and interdependencies; empty when there are none.
    pub notes: &'static str,
}

/// Valid-values text shared by every Level key.
const LEVEL_VALID: &str = "off | warn | allow | block";

/// The complete key schema: every leaf key the [`Policy`] structs deserialize,
/// in file order (top-level, then `git`, then `security`). A drift-guard test
/// pins this table to the serde fields in both directions.
pub const SCHEMA: [KeySpec; 42] = [
    // ---- top-level -------------------------------------------------------
    KeySpec {
        path: "schema_version",
        kind: KeyKind::UInt,
        valid: "a non-negative integer",
        purpose: "Version of the policy.json schema.",
        notes: "The scaffold writes 1; the loader does not read or migrate on \
                it today (inert).",
    },
    KeySpec {
        path: "human_authorization",
        kind: KeyKind::Enum(&["none"]),
        valid: "none",
        purpose: "Out-of-band human-authorization mode for irreversible actions (ADR-0009).",
        notes: "Only `none` exists today: an inert seam for future \
                totp/push/webauthn adapters.",
    },
    // ---- git: protected branches -----------------------------------------
    KeySpec {
        path: "git.protected_branches",
        kind: KeyKind::StringList,
        valid: "an array of branch names or glob patterns (e.g. release/*)",
        purpose: "Branches every enforcement plane treats as protected.",
        notes: "Globs use the `glob` crate's syntax; read by all four planes.",
    },
    KeySpec {
        path: "git.commit_to_protected",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "A commit made directly on a protected branch.",
        notes: "",
    },
    KeySpec {
        path: "git.push_to_protected",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "A push to a protected branch.",
        notes: "",
    },
    KeySpec {
        path: "git.force_push_protected",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "A force-push to a protected branch.",
        notes: "",
    },
    KeySpec {
        path: "git.force_push_unprotected",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "A force-push to a NON-protected branch.",
        notes: "The only non-strict default (allow); it sanctions the \
                durability push with --force-with-lease; set block to forbid.",
    },
    KeySpec {
        path: "git.delete_protected",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Deleting a protected branch.",
        notes: "",
    },
    KeySpec {
        path: "git.hard_reset_protected",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "A hard reset on a protected branch.",
        notes: "",
    },
    KeySpec {
        path: "git.merge_to_protected",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "A merge commit landing on a protected branch.",
        notes: "PR merge and `codeflow integrate` are the sanctioned paths; a \
                human may override the git layer with CODEFLOW_HUMAN_OVERRIDE=1.",
    },
    KeySpec {
        path: "git.pr_merge_to_protected",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "A `gh pr merge` whose base branch is protected (git-guard only).",
        notes: "",
    },
    KeySpec {
        path: "git.local_ref_protection",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Any local update of a protected ref that did not come from the remote.",
        notes: "The reference-transaction backstop (ADR-0007): catches \
                fast-forward merges, reset --hard, and branch -D that classic \
                client hooks miss.",
    },
    KeySpec {
        path: "git.hook_integrity",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Tampering with the enforcement plane itself (hooksPath flips, hook-skip envs, hook/policy writes).",
        notes: "git-guard only (ADR-0009); suspended only in the \
                pre-first-commit bootstrap window.",
    },
    // ---- git: commit format ----------------------------------------------
    KeySpec {
        path: "git.commit_format",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Conventional-commit subject shape: `type(scope): description`.",
        notes: "Governs three checks: commit_types, commit_desc_max_len, and \
                commit_subject_max_len; off/allow disables all of them.",
    },
    KeySpec {
        path: "git.commit_types",
        kind: KeyKind::StringList,
        valid: "an array of lower-case conventional commit types",
        purpose: "The whitelist of commit types.",
        notes: "Enforced under commit_format.",
    },
    KeySpec {
        path: "git.commit_desc_max_len",
        kind: KeyKind::UInt,
        valid: "a non-negative integer (character count)",
        purpose: "Max length of the description, the text after `type(scope): `.",
        notes: "Enforced under commit_format.",
    },
    KeySpec {
        path: "git.commit_subject_max_len",
        kind: KeyKind::UInt,
        valid: "a non-negative integer (character count)",
        purpose: "Max length of the whole subject line (git's 72-column wrap).",
        notes: "Enforced under commit_format.",
    },
    // ---- git: commit body / footers / tickets -----------------------------
    KeySpec {
        path: "git.commit_body",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Shape of the commit body: only `- ` bullets, blank lines, and sanctioned footers.",
        notes: "Governs the whole body family: the bullet caps, footer \
                tokens, required footers, and ticket trailers; off/allow \
                disables them all. Merge/revert/fixup/squash commits are exempt.",
    },
    KeySpec {
        path: "git.commit_body_max_bullets",
        kind: KeyKind::UInt,
        valid: "a non-negative integer",
        purpose: "Max number of `- ` bullets allowed in a commit body.",
        notes: "Enforced under commit_body.",
    },
    KeySpec {
        path: "git.commit_body_bullet_max_len",
        kind: KeyKind::UInt,
        valid: "a non-negative integer (character count, including the `- ` marker)",
        purpose: "Max length of a single commit-body bullet line.",
        notes: "Enforced under commit_body.",
    },
    KeySpec {
        path: "git.commit_footer_tokens",
        kind: KeyKind::StringList,
        valid: "an array of exact, case-sensitive trailer tokens (e.g. Signed-off-by)",
        purpose: "Footer trailers ALLOWED (optional) beyond the always-allowed BREAKING CHANGE:.",
        notes: "Unioned with commit_required_footers and commit_ticket_keys \
                into the sanctioned-footer set; enforced under commit_body.",
    },
    KeySpec {
        path: "git.commit_required_footers",
        kind: KeyKind::StringList,
        valid: "an array of exact, case-sensitive trailer tokens (e.g. Signed-off-by)",
        purpose: "Footer trailers that MUST appear on every non-exempt commit (e.g. DCO).",
        notes: "A required token is implicitly allowed; a commit missing one \
                blocks under commit_body.",
    },
    KeySpec {
        path: "git.commit_ticket_keys",
        kind: KeyKind::StringList,
        valid: "an array of exact, case-sensitive trailer tokens (e.g. Refs, Closes)",
        purpose: "Ticket-reference trailer tokens the project recognizes.",
        notes: "EMPTY = the ticket feature is off and a `Refs:` line BLOCKS \
                unless listed in commit_footer_tokens; non-empty allows these \
                tokens and arms commit_ticket_required.",
    },
    KeySpec {
        path: "git.commit_ticket_required",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Whether a matching ticket-reference trailer is REQUIRED on every commit.",
        notes: "Only meaningful when commit_ticket_keys is non-empty; `allow` \
                behaves like `off` here (inactive); use warn or block to \
                require. Merge/revert/fixup/squash commits are exempt.",
    },
    KeySpec {
        path: "git.commit_ticket_pattern",
        kind: KeyKind::String,
        valid: "a regular expression, e.g. ^PROJ-\\d+$ (empty = no format check)",
        purpose: "Regex a present ticket trailer's value must match.",
        notes: "Strictly validated: an unparseable pattern is a loud error at \
                the commit-msg hook, `codeflow ci`, and `codeflow validate` \
                (at enforcement time it would silently fail open).",
    },
    // ---- git: breaking-change tripwire ------------------------------------
    KeySpec {
        path: "git.breaking_watch_paths",
        kind: KeyKind::StringList,
        valid: "an array of path globs (the `glob` crate's syntax)",
        purpose: "Declared contract surfaces: an unmarked commit touching one draws a WARN.",
        notes: "Warn-only, never blocks, whatever the levels say; empty = no \
                tripwire. The nudge asks for a `type!:` marker or a BREAKING \
                CHANGE: footer.",
    },
    // ---- git: hygiene ------------------------------------------------------
    KeySpec {
        path: "git.ai_attribution",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "AI attribution (Co-Authored-By AI trailers, \"Generated with\", robot emoji) in commits and PR bodies.",
        notes: "",
    },
    KeySpec {
        path: "git.commit_emoji",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Emoji in commit subjects and PR bodies.",
        notes: "",
    },
    KeySpec {
        path: "git.policy_characters",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "En and em dashes (U+2013, U+2014) in commit messages, PR bodies, and lines a ci range adds under the written-content trees (ADR-0067).",
        notes: "Judges new text only: `codeflow ci` checks lines the range \
                adds under docs/, project-management/ and the skill trees, so \
                existing bytes are grandfathered and nothing asks for a sweep.",
    },
    // ---- git: PR-body structure --------------------------------------------
    KeySpec {
        path: "git.pr_sections",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Required sections in the PR/MR body: the structure check `codeflow ci` runs on a provided PR body.",
        notes: "Governs pr_required_sections and pr_code_sections; off/allow \
                disables both. PR events require a non-empty body; local/push runs without one skip. \
                Presentation and template-remnant checks always warn, never block.",
    },
    KeySpec {
        path: "git.pr_release_impact",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Generic Release impact fields and breaking commit floor in PR bodies.",
        notes: "Defaults to warn independently of pr_sections. Extra project fields are allowed; this check does not calculate versions or require release automation.",
    },
    KeySpec {
        path: "git.pr_breaking_level",
        kind: KeyKind::Enum(&["patch", "minor", "major"]),
        valid: "patch | minor | major",
        purpose: "Minimum impact level permitted for Breaking: yes in the project's release policy.",
        notes: "Defaults to major. Pre-1.0 projects explicitly choose their level, commonly minor. Breaking commit markers floor Impact at this level.",
    },
    KeySpec {
        path: "git.pr_required_sections",
        kind: KeyKind::StringList,
        valid: "an array of heading names without the leading ## (e.g. Summary)",
        purpose: "Headings every PR body must carry, matched case-insensitively at ##/### depth.",
        notes: "A present-but-empty section (only HTML comments and bare `-` \
                bullets) counts as missing; enforced under pr_sections. Fresh \
                installs also list Reviews and Release impact; without this key \
                the built-in default stays Summary and Changes.",
    },
    KeySpec {
        path: "git.pr_code_sections",
        kind: KeyKind::StringList,
        valid: "an array of heading names without the leading ## (e.g. Testing)",
        purpose: "Headings required only when the commit range touches non-docs files.",
        notes: "Docs-only = every changed path is *.md, *.txt, LICENSE*, \
                docs/**, or a .github template; anything else, or a range \
                whose files could not be resolved, counts as code. Enforced \
                under pr_sections.",
    },
    KeySpec {
        path: "git.branch_naming",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Branch names must match `{prefix}/{kebab-name}` with a sanctioned prefix.",
        notes: "Protected branches are exempt (governed by the protection \
                rules, not naming); prefixes come from branch_prefixes.",
    },
    KeySpec {
        path: "git.branch_prefixes",
        kind: KeyKind::StringList,
        valid: "an array of prefixes, each ending with `/` (e.g. feat/)",
        purpose: "The sanctioned branch-name prefixes.",
        notes: "A prefix missing its trailing `/` never matches a \
                `{prefix}/{name}` branch.",
    },
    // ---- git: gates --------------------------------------------------------
    KeySpec {
        path: "git.secret_scan",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Pre-commit scan for staged secrets and .env files.",
        notes: "Never suspended by bootstrap grace, but off/allow HERE does \
                disable the scan (it is policy, not hardcoded); leave at block.",
    },
    KeySpec {
        path: "git.test_gate_on_push",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Run the test gate before a push (pre-push hook).",
        notes: "",
    },
    KeySpec {
        path: "git.security_review",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "The umbrella gate for the CI security-review job.",
        notes: "Ships at warn (ADR-0016); harden to block when ready.",
    },
    KeySpec {
        path: "git.dep_audit",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "The dependency-audit gate in CI.",
        notes: "Ships at warn (ADR-0016); harden to block when ready.",
    },
    // ---- security ----------------------------------------------------------
    KeySpec {
        path: "security.dangerous_commands",
        kind: KeyKind::Enum(&["block"]),
        valid: "block",
        purpose: "Destructive commands the exec-guard catches (rm -rf on system paths, dd to devices, mkfs, fork bombs).",
        notes: "Non-relaxable safety floor: stale or hand-edited weaker values \
                are ignored by enforcement and rejected by validation.",
    },
    KeySpec {
        path: "security.privilege_escalation",
        kind: KeyKind::Level,
        valid: LEVEL_VALID,
        purpose: "Privilege escalation the exec-guard catches (Unix sudo/su/doas/pkexec, Windows gsudo/runas/elevated PowerShell, LD_PRELOAD/PATH injection).",
        notes: "Default warn, not block: the harness's ask tier owns sudo \
                prompting; the guard only surfaces in-session feedback.",
    },
];

/// The full schema, for callers that render or validate it.
#[must_use]
pub fn schema() -> &'static [KeySpec] {
    &SCHEMA
}

/// Find one key's spec by its dotted leaf path.
#[must_use]
pub fn spec_for(path: &str) -> Option<&'static KeySpec> {
    SCHEMA.iter().find(|s| s.path == path)
}

/// [`Policy::default`] serialized to JSON — the live source `explain`/`show`
/// render defaults from (never a hand-maintained literal, so it cannot drift).
///
/// # Panics
/// Never in practice: `Policy` serializes infallibly to a JSON object.
#[must_use]
pub fn default_policy_value() -> Value {
    serde_json::to_value(Policy::default()).expect("Policy serializes to JSON")
}

/// Look up a dotted leaf path (e.g. `git.commit_format`) in a policy JSON value.
#[must_use]
pub fn lookup<'v>(root: &'v Value, path: &str) -> Option<&'v Value> {
    path.split('.').try_fold(root, Value::get)
}

/// Render a JSON value for display: bare strings (`block`), `""` for the empty
/// string, compact JSON for everything else (`["main","master"]`, `50`).
#[must_use]
pub fn render_value(v: &Value) -> String {
    match v {
        Value::String(s) if s.is_empty() => "\"\"".to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Leniently parse policy-file text to a JSON value — for display callers
/// (`codeflow policy show`) that render what the file says even when it is
/// invalid. `None` when the text is not JSON at all.
#[must_use]
pub fn parse_lenient(data: &str) -> Option<Value> {
    serde_json::from_str(data).ok()
}

/// One invalid finding in a policy file: the offending key (dotted leaf path,
/// or `policy.json` for file-level problems) and the full human-readable
/// message naming the offending value and the valid set/format.
#[derive(Debug, Clone)]
pub struct PolicyError {
    /// The offending key's dotted path, or `policy.json` for file-level errors.
    pub key: String,
    /// The complete message, e.g. `invalid value 'worn' for
    /// git.commit_ticket_required; expected one of: off, warn, allow, block`.
    pub message: String,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl PolicyError {
    fn file(message: impl Into<String>) -> Self {
        Self {
            key: "policy.json".to_string(),
            message: message.into(),
        }
    }

    fn invalid_value(path: &str, value: &Value, expected: &str) -> Self {
        Self {
            key: path.to_string(),
            message: format!(
                "invalid value {} for {path}; expected {expected}",
                show_value(value)
            ),
        }
    }
}

/// Render a JSON value for an error message: a string as `'value'`, else its JSON.
fn show_value(v: &Value) -> String {
    match v {
        Value::String(s) => format!("'{s}'"),
        other => other.to_string(),
    }
}

/// Strictly validate `<root>/.codeflow/policy.json`. See [`validate_policy_file`].
///
/// # Errors
/// Every invalid finding, each naming the key, the offending value, and the
/// valid set/format.
pub fn validate_policy(root: &Path) -> Result<(), Vec<PolicyError>> {
    validate_policy_file(&root.join(".codeflow").join("policy.json"))
}

/// Strictly validate a policy file against the schema registry, collecting
/// EVERY finding: malformed JSON, unknown keys, wrong-typed values, invalid
/// enum values, and an unparseable `commit_ticket_pattern` regex. An ABSENT
/// file is not an error — the built-in strict defaults apply; a PRESENT but
/// invalid file is a loud error, never a silent fallback to defaults (which
/// would quietly discard the consumer's intended configuration). Surfaced by
/// the commit-msg git hook, `codeflow ci`, and `codeflow validate`.
///
/// # Errors
/// One [`PolicyError`] per finding, each naming the key, the offending value,
/// and the valid set/format.
pub fn validate_policy_file(path: &Path) -> Result<(), Vec<PolicyError>> {
    match std::fs::read_to_string(path) {
        Ok(data) => validate_policy_str(&data),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(vec![PolicyError::file(format!(
            "policy.json exists but cannot be read: {e}"
        ))]),
    }
}

/// The schema-driven strict validator behind [`validate_policy_file`]. Walks
/// every key the file carries against [`SCHEMA`]; a final strict deserialize
/// is the backstop for anything the walk does not model (so a file the
/// enforcement loader would silently default away can never validate clean).
///
/// # Errors
/// One [`PolicyError`] per finding.
pub fn validate_policy_str(data: &str) -> Result<(), Vec<PolicyError>> {
    let root: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            return Err(vec![PolicyError::file(format!(
                "policy.json is not valid JSON: {e}"
            ))]);
        }
    };
    let Some(obj) = root.as_object() else {
        return Err(vec![PolicyError::file("policy.json must be a JSON object")]);
    };

    let mut errors = Vec::new();
    for (key, value) in obj {
        match key.as_str() {
            // The two object sections: walk their leaves with the prefix.
            section @ ("git" | "security") => match value.as_object() {
                Some(section_obj) => {
                    for (leaf, leaf_value) in section_obj {
                        validate_leaf(&format!("{section}.{leaf}"), leaf_value, &mut errors);
                    }
                }
                None => errors.push(PolicyError::invalid_value(section, value, "a JSON object")),
            },
            _ => validate_leaf(key, value, &mut errors),
        }
    }

    // Backstop: catch anything the schema walk did not model (a structural
    // error, or a registry gap) rather than let the enforcement loader
    // silently default the file away.
    if errors.is_empty() {
        if let Err(e) = serde_json::from_str::<Policy>(data) {
            errors.push(PolicyError::file(format!("policy.json is invalid: {e}")));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validate one leaf `path`/`value` pair against its [`KeySpec`]; a path the
/// schema does not know is an unknown-key error.
fn validate_leaf(path: &str, value: &Value, errors: &mut Vec<PolicyError>) {
    let Some(spec) = spec_for(path) else {
        errors.push(PolicyError {
            key: path.to_string(),
            message: format!(
                "unknown key {path}; not a policy key (see `codeflow policy explain` for the full schema)"
            ),
        });
        return;
    };
    match spec.kind {
        KeyKind::Level => {
            if !value.as_str().is_some_and(|s| LEVEL_VALUES.contains(&s)) {
                errors.push(PolicyError::invalid_value(
                    path,
                    value,
                    &format!("one of: {LEVEL_VALUES_TEXT}"),
                ));
            }
        }
        KeyKind::UInt => {
            if value.as_u64().is_none_or(|n| u32::try_from(n).is_err()) {
                errors.push(PolicyError::invalid_value(
                    path,
                    value,
                    "a non-negative integer",
                ));
            }
        }
        KeyKind::StringList => {
            if !value
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_string))
            {
                errors.push(PolicyError::invalid_value(
                    path,
                    value,
                    "an array of strings",
                ));
            }
        }
        KeyKind::String => match value.as_str() {
            Some(s) => {
                // The one string key is a regex; an unparseable pattern would
                // silently disable the format check at enforcement time.
                if path == "git.commit_ticket_pattern" && !s.is_empty() {
                    if let Err(e) = regex::Regex::new(s) {
                        // regex's Display is a multi-line caret diagram; the
                        // last line ("error: unclosed group") is the reason.
                        let reason = e
                            .to_string()
                            .lines()
                            .last()
                            .unwrap_or_default()
                            .trim_start_matches("error: ")
                            .to_string();
                        errors.push(PolicyError {
                            key: path.to_string(),
                            message: format!(
                                "invalid value '{s}' for {path}; not a valid regular expression ({reason})"
                            ),
                        });
                    }
                }
            }
            None => errors.push(PolicyError::invalid_value(path, value, "a string")),
        },
        KeyKind::Enum(values) => {
            if !value.as_str().is_some_and(|s| values.contains(&s)) {
                errors.push(PolicyError::invalid_value(
                    path,
                    value,
                    &format!("one of: {}", values.join(", ")),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collect every dotted leaf path in a serialized policy JSON object.
    fn leaf_paths(value: &Value, prefix: &str, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    let path = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    leaf_paths(v, &path, out);
                }
            }
            _ => out.push(prefix.to_string()),
        }
    }

    #[test]
    fn test_schema_covers_every_policy_leaf_both_ways() {
        // The drift guard: every leaf the default Policy serializes must be in
        // the schema, and every schema path must be a real serde leaf — a new
        // field (or a renamed one) fails this test until the registry follows.
        let mut struct_paths = Vec::new();
        leaf_paths(&default_policy_value(), "", &mut struct_paths);
        let schema_paths: Vec<&str> = SCHEMA.iter().map(|s| s.path).collect();
        for p in &struct_paths {
            assert!(
                schema_paths.contains(&p.as_str()),
                "policy field {p} is missing from the schema registry"
            );
        }
        for p in &schema_paths {
            assert!(
                struct_paths.iter().any(|s| s == p),
                "schema registry lists {p}, which is not a policy field"
            );
        }
        assert_eq!(struct_paths.len(), SCHEMA.len(), "one spec per leaf");
    }

    #[test]
    fn test_defaults_render_from_the_real_default_impl() {
        let defaults = default_policy_value();
        let get = |p: &str| render_value(lookup(&defaults, p).expect(p));
        assert_eq!(get("git.commit_format"), "block");
        assert_eq!(get("git.force_push_unprotected"), "allow");
        assert_eq!(get("git.commit_desc_max_len"), "50");
        assert_eq!(get("git.commit_ticket_pattern"), "\"\"");
        assert_eq!(get("git.protected_branches"), r#"["main","master"]"#);
        assert_eq!(get("human_authorization"), "none");
    }

    #[test]
    fn test_validate_absent_file_ok_and_shipped_asset_clean() {
        // An absent file is not an error (built-in defaults apply)...
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_policy(dir.path()).is_ok());
        // ...and the shipped scaffold asset validates cleanly.
        let asset = include_str!("../../../../assets/base/policy.json");
        assert!(validate_policy_str(asset).is_ok());
    }

    #[test]
    fn test_validate_bad_level_names_key_value_and_options() {
        // The headline case: a typo'd level must ERROR naming the key, the
        // offending value, and the four valid values — never silently revert
        // the whole file to defaults.
        let errs = validate_policy_str(r#"{"git":{"commit_ticket_required":"worn"}}"#).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].key, "git.commit_ticket_required");
        assert!(errs[0].message.contains("'worn'"), "{}", errs[0]);
        assert!(
            errs[0].message.contains("off, warn, allow, block"),
            "{}",
            errs[0]
        );
    }

    #[test]
    fn test_validate_unknown_keys_error() {
        let errs =
            validate_policy_str(r#"{"git":{"commit_tikcet_required":"block"}}"#).unwrap_err();
        assert!(errs[0]
            .message
            .contains("unknown key git.commit_tikcet_required"));
        let errs = validate_policy_str(r#"{"gti":{}}"#).unwrap_err();
        assert!(errs[0].message.contains("unknown key gti"));
    }

    #[test]
    fn test_validate_bad_ticket_regex_errors() {
        // At enforcement time an unparseable pattern silently fails open; the
        // strict validator makes it loud instead.
        let errs = validate_policy_str(r#"{"git":{"commit_ticket_pattern":"("}}"#).unwrap_err();
        assert_eq!(errs[0].key, "git.commit_ticket_pattern");
        assert!(
            errs[0].message.contains("regular expression"),
            "{}",
            errs[0]
        );
    }

    #[test]
    fn test_validate_wrong_types_name_key() {
        let errs = validate_policy_str(r#"{"git":{"commit_desc_max_len":"fifty"}}"#).unwrap_err();
        assert!(errs[0].message.contains("git.commit_desc_max_len"));
        assert!(errs[0].message.contains("non-negative integer"));
        let errs =
            validate_policy_str(r#"{"git":{"commit_footer_tokens":"Signed-off-by"}}"#).unwrap_err();
        assert!(errs[0].message.contains("array of strings"));
        let errs = validate_policy_str(r#"{"git":[]}"#).unwrap_err();
        assert!(errs[0].message.contains("expected a JSON object"));
        let errs = validate_policy_str(r#"{"human_authorization":"totp"}"#).unwrap_err();
        assert!(
            errs[0].message.contains("expected one of: none"),
            "{}",
            errs[0]
        );
        let errs =
            validate_policy_str(r#"{"security":{"dangerous_commands":"nope"}}"#).unwrap_err();
        assert_eq!(errs[0].key, "security.dangerous_commands");
    }

    #[test]
    fn test_validate_rejects_weakening_catastrophic_floor() {
        for level in ["off", "warn", "allow"] {
            let json = format!(r#"{{"security":{{"dangerous_commands":"{level}"}}}}"#);
            let errs = validate_policy_str(&json).unwrap_err();
            assert_eq!(errs[0].key, "security.dangerous_commands");
            assert!(errs[0].message.contains("block"), "{}", errs[0]);
        }
    }

    #[test]
    fn test_validate_collects_every_error_not_just_the_first() {
        let errs = validate_policy_str(
            r#"{"git":{"commit_format":"blok","commit_desc_max_len":-1,"nope":true}}"#,
        )
        .unwrap_err();
        assert_eq!(errs.len(), 3, "{errs:?}");
    }

    #[test]
    fn test_validate_malformed_json_and_non_object() {
        let errs = validate_policy_str("{ not json").unwrap_err();
        assert!(errs[0].message.contains("not valid JSON"));
        let errs = validate_policy_str("[1,2]").unwrap_err();
        assert!(errs[0].message.contains("must be a JSON object"));
    }

    #[test]
    fn test_validate_valid_opt_in_config_ok() {
        let json = r#"{"schema_version":1,"git":{"commit_footer_tokens":["Signed-off-by"],
            "commit_ticket_keys":["Refs"],"commit_ticket_required":"block",
            "commit_ticket_pattern":"^PROJ-\\d+$"},"human_authorization":"none"}"#;
        assert!(validate_policy_str(json).is_ok());
    }
}
