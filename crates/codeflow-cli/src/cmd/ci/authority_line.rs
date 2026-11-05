//! The hosted report's authority line (TSK-1177 AC-2, ADR-0091 "Report
//! lines"). The policy workflow's verdict job reads the host's rules for the
//! default branch with its read-only token and names the file in
//! `CODEFLOW_HOST_RULES`; `codeflow ci` prints one line from it after the
//! content verdict: which commit judged, which identity the required checks
//! accept, and what the host rules show. The line is information only. It
//! never changes the exit code, and an unreadable file or answer prints a
//! line saying so.

use std::fmt::Write as _;
use std::path::Path;

use codeflow_core::remote::{
    AUTHORITY_PREFIX, GITHUB_ACTIONS_APP_ID, RULESET_AUTHORITY_CHECKS, RULESET_AUTHORITY_UPDATES,
    TRUSTED_PREFIX,
};
use serde_json::Value;

/// The variable naming the host-rules file the verdict job writes.
pub(super) const HOST_RULES_ENV: &str = "CODEFLOW_HOST_RULES";

/// The administrator's route to what this token cannot see.
const ADMINISTRATOR_ROUTE: &str =
    "run `codeflow doctor --check remote-perimeter` as an administrator";

/// Print the authority line when the verdict job named a host-rules file.
pub(super) fn print_from_env() {
    if let Some(path) = std::env::var_os(HOST_RULES_ENV).filter(|path| !path.is_empty()) {
        println!("{}", line_from_file(Path::new(&path)));
    }
}

/// The line for the file at `path`; an unreadable file is named, never an
/// error.
fn line_from_file(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Err(error) => format!(
            "codeflow ci: authority: the host rules file {} is not readable ({}); the verdict above stands; {ADMINISTRATOR_ROUTE}",
            printable(&path.display().to_string()),
            printable(&error.to_string())
        ),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Err(error) => format!(
                "codeflow ci: authority: the host rules file is not JSON ({}); the verdict above stands; {ADMINISTRATOR_ROUTE}",
                printable(&error.to_string())
            ),
            Ok(value) => line(&value),
        },
    }
}

/// The authority line for the verdict job's host-rules document:
/// `{branch, authority, rules, rulesets}`, or `{branch, authority,
/// unreadable}` when the job could not read them.
pub(super) fn line(document: &Value) -> String {
    let branch = printable(document["branch"].as_str().unwrap_or("(unknown branch)"));
    let sha = printable(document["authority"].as_str().unwrap_or("(unknown commit)"));
    let mut out = format!("codeflow ci: authority: judged by {branch}@{sha}");
    if let Some(why) = document["unreadable"].as_str() {
        let _ = write!(
            out,
            "; check identity and host rules not readable ({}); the verdict above stands; {ADMINISTRATOR_ROUTE}",
            printable(why)
        );
        return out;
    }
    let (Some(rules), Some(rulesets)) = (
        document["rules"].as_array(),
        document["rulesets"].as_array(),
    ) else {
        let _ = write!(
            out,
            "; check identity and host rules not readable (the host's answer is not a list of rules and rulesets); the verdict above stands; {ADMINISTRATOR_ROUTE}"
        );
        return out;
    };
    let pins = pins(rules);
    let _ = write!(
        out,
        "; check identity: {}; host rules read from rules/branches/{branch}: A {}, B {}, {}, {}, {}; bypass actors not visible to this token; {ADMINISTRATOR_ROUTE}",
        identity(&pins),
        presence(RULESET_AUTHORITY_UPDATES, rules, rulesets),
        presence(RULESET_AUTHORITY_CHECKS, rules, rulesets),
        approvals(rules),
        strict(rules),
        pin_list(&pins),
    );
    out
}

/// `(context, integration id)` for each required check of a trusted or
/// authority name, in the order the host lists them.
fn pins(rules: &[Value]) -> Vec<(String, Option<u64>)> {
    rules
        .iter()
        .filter(|rule| rule["type"] == "required_status_checks")
        .flat_map(|rule| {
            rule["parameters"]["required_status_checks"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|check| {
            let context = check["context"].as_str()?;
            (context.starts_with(TRUSTED_PREFIX) || context.starts_with(AUTHORITY_PREFIX))
                .then(|| (printable(context), check["integration_id"].as_u64()))
        })
        .collect()
}

/// The identity a required verdict check accepts: the App pinned on the
/// `authority:` names (Mode A), else GitHub Actions (Mode B).
fn identity(pins: &[(String, Option<u64>)]) -> String {
    let mut apps: Vec<u64> = pins
        .iter()
        .filter(|(context, _)| context.starts_with(AUTHORITY_PREFIX))
        .filter_map(|(_, app)| *app)
        .filter(|app| *app != GITHUB_ACTIONS_APP_ID)
        .collect();
    apps.sort_unstable();
    apps.dedup();
    if apps.is_empty() {
        "Actions".to_string()
    } else {
        apps.iter()
            .map(|app| format!("App {app}"))
            .collect::<Vec<_>>()
            .join(" and ")
    }
}

/// Whether the ruleset named `name` applies to the branch: listed among the
/// repository's rulesets and the source of a rule on the branch.
fn presence(name: &str, rules: &[Value], rulesets: &[Value]) -> &'static str {
    let ids: Vec<u64> = rulesets
        .iter()
        .filter(|ruleset| ruleset["name"] == name)
        .filter_map(|ruleset| ruleset["id"].as_u64())
        .collect();
    if rules.iter().any(|rule| {
        rule["ruleset_id"]
            .as_u64()
            .is_some_and(|id| ids.contains(&id))
    }) {
        "present"
    } else {
        "absent"
    }
}

/// The highest approval count any pull request rule on the branch sets.
fn approvals(rules: &[Value]) -> String {
    rules
        .iter()
        .filter(|rule| rule["type"] == "pull_request")
        .map(|rule| {
            rule["parameters"]["required_approving_review_count"]
                .as_u64()
                .unwrap_or(0)
        })
        .max()
        .map_or_else(
            || "no pull request rule".to_string(),
            |count| format!("approvals {count}"),
        )
}

/// Whether a required-checks rule on the branch makes the branch be up to
/// date before merging.
fn strict(rules: &[Value]) -> &'static str {
    let checks: Vec<&Value> = rules
        .iter()
        .filter(|rule| rule["type"] == "required_status_checks")
        .collect();
    if checks.is_empty() {
        "no required checks"
    } else if checks
        .iter()
        .any(|rule| rule["parameters"]["strict_required_status_checks_policy"] == true)
    {
        "strict yes"
    } else {
        "strict no"
    }
}

/// Each trusted or authority name the branch requires, with its pin.
fn pin_list(pins: &[(String, Option<u64>)]) -> String {
    if pins.is_empty() {
        return "no trusted or authority name required".to_string();
    }
    let named: Vec<String> = pins
        .iter()
        .map(|(context, app)| {
            let source = match app {
                None => "any source".to_string(),
                Some(app) if *app == GITHUB_ACTIONS_APP_ID => "Actions".to_string(),
                Some(app) => format!("App {app}"),
            };
            format!("{context} pinned to {source}")
        })
        .collect();
    format!("pins: {}", named.join(", "))
}

/// Text from the host or the file, kept on one printable line so it cannot
/// start a line the runner reads as a workflow command.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rulesets() -> Value {
        json!([
            { "id": 11, "name": RULESET_AUTHORITY_UPDATES },
            { "id": 12, "name": RULESET_AUTHORITY_CHECKS },
            { "id": 99, "name": "foreign" }
        ])
    }

    fn mode_a() -> Value {
        json!({
            "branch": "main",
            "authority": "a".repeat(40),
            "rules": [
                { "type": "update", "ruleset_id": 11 },
                { "type": "pull_request", "ruleset_id": 12,
                  "parameters": { "required_approving_review_count": 1 } },
                { "type": "required_status_checks", "ruleset_id": 12, "parameters": {
                    "strict_required_status_checks_policy": true,
                    "required_status_checks": [
                        { "context": "authority: commit standards", "integration_id": 4242 },
                        { "context": "codeflow gates", "integration_id": 15368 },
                        { "context": "trusted: release impact" }
                    ] } }
            ],
            "rulesets": rulesets()
        })
    }

    #[test]
    fn mode_a_names_the_app_and_every_host_rule() {
        let text = line(&mode_a());
        assert_eq!(
            text,
            format!(
                "codeflow ci: authority: judged by main@{}; check identity: App 4242; host rules read from rules/branches/main: A present, B present, approvals 1, strict yes, pins: authority: commit standards pinned to App 4242, trusted: release impact pinned to any source; bypass actors not visible to this token; run `codeflow doctor --check remote-perimeter` as an administrator",
                "a".repeat(40)
            )
        );
    }

    #[test]
    fn mode_b_names_actions_and_absent_rulesets() {
        let document = json!({
            "branch": "main",
            "authority": "b".repeat(40),
            "rules": [
                { "type": "required_status_checks", "ruleset_id": 99, "parameters": {
                    "required_status_checks": [
                        { "context": "trusted: commit standards", "integration_id": 15368 }
                    ] } }
            ],
            "rulesets": rulesets()
        });
        let text = line(&document);
        assert!(text.contains("check identity: Actions;"), "{text}");
        assert!(
            text.contains("A absent, B absent, no pull request rule, strict no"),
            "{text}"
        );
        assert!(
            text.contains("pins: trusted: commit standards pinned to Actions;"),
            "{text}"
        );
    }

    #[test]
    fn an_authority_name_pinned_to_actions_is_not_an_app_identity() {
        let document = json!({
            "branch": "main", "authority": "c".repeat(40),
            "rules": [{ "type": "required_status_checks", "ruleset_id": 12, "parameters": {
                "required_status_checks": [
                    { "context": "authority: commit standards", "integration_id": 15368 }
                ] } }],
            "rulesets": rulesets()
        });
        assert!(line(&document).contains("check identity: Actions;"));
    }

    #[test]
    fn no_rules_reads_as_nothing_required() {
        let document = json!({
            "branch": "main", "authority": "d".repeat(40), "rules": [], "rulesets": []
        });
        let text = line(&document);
        assert!(
            text.contains("A absent, B absent, no pull request rule, no required checks, no trusted or authority name required;"),
            "{text}"
        );
    }

    #[test]
    fn an_unreadable_answer_is_named_and_the_verdict_stands() {
        let document = json!({
            "branch": "main", "authority": "e".repeat(40),
            "unreadable": "the host rules could not be read with this token"
        });
        let text = line(&document);
        assert!(
            text.ends_with("check identity and host rules not readable (the host rules could not be read with this token); the verdict above stands; run `codeflow doctor --check remote-perimeter` as an administrator"),
            "{text}"
        );
        let shapeless = json!({ "branch": "main", "authority": "e", "rules": {} });
        assert!(line(&shapeless).contains("is not a list of rules and rulesets"));
    }

    #[test]
    fn host_text_cannot_start_a_workflow_command() {
        let document = json!({
            "branch": "main\n::error::forged", "authority": "f", "unreadable": "x\r\n::warning::y"
        });
        let text = line(&document);
        assert!(!text.contains('\n') && !text.contains('\r'), "{text}");
        assert!(text.contains("main?::error::forged"), "{text}");
    }

    #[test]
    fn a_missing_or_malformed_file_is_a_line_never_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = line_from_file(&dir.path().join("absent.json"));
        assert!(missing.contains("is not readable"), "{missing}");
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{").unwrap();
        let bad = line_from_file(&path);
        assert!(bad.contains("is not JSON"), "{bad}");
        std::fs::write(&path, mode_a().to_string()).unwrap();
        assert!(line_from_file(&path).contains("check identity: App 4242"));
    }
}
