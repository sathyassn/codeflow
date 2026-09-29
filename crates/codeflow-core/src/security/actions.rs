//! The action table: one list of the actions agent sessions are refused, for
//! all three harnesses (ADR-0075 decision 1; TSK-171).
//!
//! The table lives in `actions.json` beside this file, in the shape of the
//! design's reference table
//! (`docs/verification/evidence/permission-presets/actions.json`). From it
//! this module generates:
//!
//! - the Claude permission arrays of every shipped preset
//!   ([`ActionTable::apply_to_claude_preset`]): the read denies, then their
//!   `!` carve-outs (a carve-out narrows only the rules listed before it),
//!   then the `Edit` denies of the enforcement paths, then each family's
//!   command rules; no `ask` array while no family asks;
//! - the task-scoped delegate settings fragment
//!   ([`ActionTable::delegate_fragment`]);
//! - the Codex rules file `.codex/rules/codeflow.rules`
//!   ([`ActionTable::codex_rules`]).
//!
//! Grok reads the Claude command rules from `.claude/settings.json`, so it
//! needs no list of its own. [`ActionTable::parity_errors`] fails a family
//! that has no entry, and no stated reason, for a harness. The shipped
//! assets are checked against this generator by the `settings_presets`
//! tests; `CODEFLOW_BLESS=1` rewrites them.

use std::fmt::Write as _;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Map, Value};

/// The embedded table.
const TABLE_JSON: &str = include_str!("actions.json");

/// The harness versions each generated rule form was qualified on. A version
/// below its floor is unqualified, not unsupported: the form may work there,
/// but nothing has shown it. `codeflow doctor` warns below a floor (TSK-173).
pub const TESTED_FLOORS: &[TestedFloor] = &[TestedFloor {
    harness: "codex-cli",
    feature: "permission profiles (`cf-guard`, `cf-builder`) with the network proxy",
    version: "0.157.1",
}];

/// One recorded tested floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestedFloor {
    /// The catalog harness id.
    pub harness: &'static str,
    /// The generated form the floor applies to.
    pub feature: &'static str,
    /// The version the form was qualified on.
    pub version: &'static str,
}

/// Whether a family's rules deny or ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Refused without a prompt.
    Deny,
    /// Prompted. No shipped family asks (ADR-0075 decision 1).
    Ask,
}

/// One token of a Codex prefix pattern: a literal, or a list of
/// alternatives.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum PatternToken {
    /// One literal argument.
    Word(String),
    /// Any one of these arguments.
    AnyOf(Vec<String>),
}

/// One Codex `prefix_rule`, with the examples Codex checks when it loads the
/// rules file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexRule {
    /// The argv prefix.
    pub pattern: Vec<PatternToken>,
    /// Commands the rule must match.
    #[serde(default, rename = "match")]
    pub matches: Vec<String>,
    /// Commands the rule must not match.
    #[serde(default)]
    pub not_match: Vec<String>,
}

/// One action family.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    /// Stable family id (`privilege`, `publish`, ...).
    pub id: String,
    /// Deny or ask.
    pub decision: Decision,
    /// Why the family is refused; the Codex justification unless
    /// `codex_why` is set.
    pub why: String,
    /// Claude permission rules (also read by Grok).
    #[serde(default)]
    pub claude: Vec<String>,
    /// Codex prefix rules.
    #[serde(default)]
    pub codex: Vec<CodexRule>,
    /// What the Codex rules leave to exec-guard, or why there are none.
    #[serde(default)]
    pub codex_note: Option<String>,
    /// The Codex justification, when it differs from `why`.
    #[serde(default)]
    pub codex_why: Option<String>,
}

/// The delegate settings deny rules.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegateDenies {
    /// Why the fragment exists.
    pub why: String,
    /// Claude permission rules for a delegated seat.
    pub claude: Vec<String>,
}

/// The whole action table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionTable {
    /// What the table is.
    #[serde(rename = "_about")]
    pub about: String,
    /// The refused action families.
    pub families: Vec<Family>,
    /// Claude `Read(...)` denies of secret files and Claude's private state.
    pub claude_read_denies: Vec<String>,
    /// Claude `Edit(...)` denies of the enforcement paths.
    pub claude_edit_denies: Vec<String>,
    /// Credential variables the Claude sandbox removes from every command.
    pub sandbox_env_denies: Vec<String>,
    /// Secret stores the Claude sandbox denies to every subprocess.
    pub sandbox_read_denies: Vec<String>,
    /// `Read(!...)` carve-outs that narrow the read denies before them.
    pub claude_read_carveouts: Vec<String>,
    /// The delegate settings fragment.
    pub delegate_denies: DelegateDenies,
}

/// The embedded action table, parsed once.
///
/// # Panics
///
/// Never for a shipped build: the embedded table is checked by the unit
/// tests below.
#[must_use]
pub fn table() -> &'static ActionTable {
    static TABLE: OnceLock<ActionTable> = OnceLock::new();
    TABLE.get_or_init(|| ActionTable::parse(TABLE_JSON).expect("embedded actions.json parses"))
}

impl ActionTable {
    /// Parse a table from JSON text.
    ///
    /// # Errors
    ///
    /// Returns the parse error for text that is not a table.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// One line per family that lacks rules, and a stated reason, for a
    /// harness. Grok reads the Claude rules, so Claude entries are required;
    /// Codex needs rules or a `codex_note`.
    #[must_use]
    pub fn parity_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for family in &self.families {
            if family.claude.is_empty() {
                errors.push(format!("{}: no Claude entries", family.id));
            }
            if family.codex.is_empty() && family.codex_note.is_none() {
                errors.push(format!("{}: no Codex entries and no reason", family.id));
            }
        }
        errors
    }

    fn family_rules(&self, decision: Decision) -> Vec<String> {
        self.families
            .iter()
            .filter(|family| family.decision == decision)
            .flat_map(|family| family.claude.iter().cloned())
            .collect()
    }

    /// The Claude `permissions.deny` array, in its required order: read
    /// denies, their carve-outs, the `Edit` denies, then the families.
    #[must_use]
    pub fn claude_deny(&self) -> Vec<String> {
        self.claude_read_denies
            .iter()
            .chain(&self.claude_read_carveouts)
            .chain(&self.claude_edit_denies)
            .cloned()
            .chain(self.family_rules(Decision::Deny))
            .collect()
    }

    /// The Claude `permissions.ask` array; empty for the shipped table.
    #[must_use]
    pub fn claude_ask(&self) -> Vec<String> {
        self.family_rules(Decision::Ask)
    }

    /// Write the generated arrays into a Claude settings preset: replace
    /// `permissions.deny`, drop `permissions.ask` when no family asks, and
    /// add the table's sandbox credential and store denies to the preset's
    /// own lists. Every other key is left as it is.
    pub fn apply_to_claude_preset(&self, preset: &mut Value) {
        let Some(root) = preset.as_object_mut() else {
            return;
        };
        let permissions = object_at(root, "permissions");
        let ask = self.claude_ask();
        if ask.is_empty() {
            permissions.remove("ask");
        } else {
            permissions.insert("ask".to_string(), strings(ask));
        }
        permissions.insert("deny".to_string(), strings(self.claude_deny()));

        let sandbox = object_at(root, "sandbox");
        let credentials = object_at(sandbox, "credentials");
        let env_vars = array_at(credentials, "envVars");
        for name in &self.sandbox_env_denies {
            let listed = env_vars
                .iter()
                .any(|entry| entry.get("name").and_then(Value::as_str) == Some(name.as_str()));
            if !listed {
                env_vars.push(serde_json::json!({"name": name, "mode": "deny"}));
            }
        }
        let filesystem = object_at(sandbox, "filesystem");
        let deny_read = array_at(filesystem, "denyRead");
        for path in &self.sandbox_read_denies {
            let path = Value::String(path.clone());
            if !deny_read.contains(&path) {
                deny_read.push(path);
            }
        }
    }

    /// The settings fragment `codeflow delegate init` writes into a
    /// delegated Claude seat's task-scoped `--settings` file.
    #[must_use]
    pub fn delegate_fragment(&self) -> Value {
        serde_json::json!({"permissions": {"deny": self.delegate_denies.claude}})
    }

    /// The Codex rules file: one `forbidden` `prefix_rule` per Codex entry,
    /// with the examples Codex checks when it loads the file.
    #[must_use]
    pub fn codex_rules(&self) -> String {
        let mut out = String::from(
            "# CodeFlow command rules for Codex, generated from actions.json.\n\
             # Loaded only when the project `.codex/` layer is trusted. `forbidden`\n\
             # refuses without prompting under every approval policy and sandbox,\n\
             # including danger-full-access (codex-rs core/src/exec_policy.rs:393-404,\n\
             # unified_exec/process_manager.rs:1479-1504 at 4fd5745e).\n",
        );
        for family in &self.families {
            let why = family.codex_why.as_deref().unwrap_or(&family.why);
            for rule in &family.codex {
                let pattern: Vec<String> = rule
                    .pattern
                    .iter()
                    .map(|token| match token {
                        PatternToken::Word(word) => starlark_string(word),
                        PatternToken::AnyOf(words) => starlark_list(words),
                    })
                    .collect();
                let _ = write!(
                    out,
                    "\nprefix_rule(\n    pattern = [{}],\n    decision = \"forbidden\",\n    justification = {},\n",
                    pattern.join(", "),
                    starlark_string(why)
                );
                if !rule.matches.is_empty() {
                    let _ = writeln!(out, "    match = {},", starlark_list(&rule.matches));
                }
                if !rule.not_match.is_empty() {
                    let _ = writeln!(out, "    not_match = {},", starlark_list(&rule.not_match));
                }
                out.push_str(")\n");
            }
        }
        out
    }
}

/// A JSON string literal, which Starlark reads as the same string.
fn starlark_string(text: &str) -> String {
    Value::String(text.to_string()).to_string()
}

fn starlark_list(words: &[String]) -> String {
    let items: Vec<String> = words.iter().map(|word| starlark_string(word)).collect();
    format!("[{}]", items.join(", "))
}

fn strings(items: Vec<String>) -> Value {
    Value::Array(items.into_iter().map(Value::String).collect())
}

/// The object under `key`, created when absent and replaced when not an
/// object (a preset is generated, never an adopter's file).
fn object_at<'m>(map: &'m mut Map<String, Value>, key: &str) -> &'m mut Map<String, Value> {
    let slot = map
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !slot.is_object() {
        *slot = Value::Object(Map::new());
    }
    slot.as_object_mut().expect("just made an object")
}

fn array_at<'m>(map: &'m mut Map<String, Value>, key: &str) -> &'m mut Vec<Value> {
    let slot = map
        .entry(key.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !slot.is_array() {
        *slot = Value::Array(Vec::new());
    }
    slot.as_array_mut().expect("just made an array")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_table_parses_and_has_parity() {
        let table = table();
        assert!(!table.families.is_empty());
        assert_eq!(table.parity_errors(), Vec::<String>::new());
    }

    #[test]
    fn the_shipped_families_are_the_decided_ones_and_all_deny() {
        let ids: Vec<&str> = table().families.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "privilege",
                "publish",
                "release",
                "account",
                "keychain",
                "persistence"
            ]
        );
        assert!(table()
            .families
            .iter()
            .all(|family| family.decision == Decision::Deny));
        assert!(table().claude_ask().is_empty());
    }

    #[test]
    fn parity_fails_a_family_without_entries_or_reason() {
        let mut table = table().clone();
        table.families[0].codex.clear();
        table.families[0].codex_note = None;
        table.families[1].claude.clear();
        let errors = table.parity_errors();
        assert_eq!(
            errors,
            [
                "privilege: no Codex entries and no reason",
                "publish: no Claude entries"
            ]
        );
        let mut noted = table.clone();
        noted.families[0].codex_note = Some("left to exec-guard".to_string());
        assert_eq!(noted.parity_errors(), ["publish: no Claude entries"]);
    }

    #[test]
    fn deny_order_puts_carveouts_after_the_read_denies_they_narrow() {
        let deny = table().claude_deny();
        let last_read_deny = deny
            .iter()
            .rposition(|rule| rule.starts_with("Read(") && !rule.starts_with("Read(!"))
            .unwrap();
        let first_carveout = deny
            .iter()
            .position(|rule| rule.starts_with("Read(!"))
            .unwrap();
        assert!(last_read_deny < first_carveout);
        assert!(deny.iter().all(|rule| !rule.starts_with("Write(")));
    }

    #[test]
    fn an_ask_family_generates_an_ask_array() {
        let mut table = table().clone();
        table.families[4].decision = Decision::Ask;
        let mut preset = serde_json::json!({"permissions": {"allow": []}});
        table.apply_to_claude_preset(&mut preset);
        let ask = preset["permissions"]["ask"].as_array().unwrap();
        assert_eq!(ask.len(), table.families[4].claude.len());
        assert!(!preset["permissions"]["deny"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| ask.contains(rule)));
    }
}
