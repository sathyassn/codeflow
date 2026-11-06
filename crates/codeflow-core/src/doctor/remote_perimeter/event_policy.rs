//! Whether GitHub will start the workflows that run on `pull_request_target`
//! (TSK-1347).
//!
//! From 2026-12-02 GitHub blocks `pull_request_target` in public
//! repositories unless an applicable Actions event policy allows it. The
//! policy workflow's verdict jobs run only on that event, so without such a
//! policy no verdict is produced and every pull request waits on a required
//! check that never runs. Private and internal repositories are not
//! affected.
//!
//! For each workflow on the default branch that runs on the event, this
//! reads the repository's Actions event policies (and the organization's,
//! for an organization-owned repository) and reports:
//!
//! - a warning when no active policy that targets the workflow allows the
//!   event, with the policy that fixes it;
//! - a warning when a targeting policy leaves out the event, or another
//!   event the workflow runs on, since event rules are allowlists;
//! - a warning when an allowing policy also allows the event for other
//!   workflows (no path condition, `~ALL` or a wildcard), which reopens
//!   the risk GitHub's default closes;
//! - a note, never a pass, when the visibility, a policy list or a policy
//!   cannot be read (reading needs an administrator's token) or a condition
//!   cannot be judged.
//!
//! Doctor reads only; it never writes a policy.

use serde_json::Value;

use super::Perimeter;
use crate::remote::Declared;

/// The event GitHub blocks by default.
const EVENT: &str = crate::remote::GATED_EVENT;

/// Whether a policy applies to a workflow.
enum Applies {
    Yes,
    No,
    /// Its conditions cannot be judged, and why.
    Unknown(String),
}

/// Judge the event policies for every workflow `declared` lists that runs
/// on `pull_request_target`, adding warnings and unread notes to `out`.
pub(super) fn judge(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    nwo: &str,
    repo: &Value,
    declared: &Declared,
    out: &mut Perimeter,
) {
    let targets: Vec<(&str, &[String])> = declared
        .events
        .iter()
        .filter(|(_, events)| events.iter().any(|event| event == EVENT))
        .map(|(path, events)| (path.as_str(), events.as_slice()))
        .collect();
    if targets.is_empty() {
        return;
    }
    let files = targets
        .iter()
        .map(|(path, _)| *path)
        .collect::<Vec<_>>()
        .join(", ");
    match repo.get("visibility").and_then(Value::as_str) {
        Some("public") => {}
        // GitHub's default block applies to public repositories only.
        Some(_) => return,
        None => {
            out.event_unread.push(format!(
                "{nwo} reported no visibility, so doctor cannot tell whether GitHub blocks {EVENT} for {files}"
            ));
            return;
        }
    }
    let policies = match read_policies(gh, nwo, repo) {
        Ok(policies) => policies,
        Err(why) => {
            out.event_unread.push(format!(
                "{why}, so doctor cannot confirm that GitHub starts {files} on {EVENT}: on a public repository GitHub blocks that event from 2026-12-02 unless an Actions event policy allows it; rerun doctor with a repository administrator's token"
            ));
            return;
        }
    };
    for (path, events) in targets {
        judge_workflow(nwo, repo, path, events, &policies, out);
    }
}

fn judge_workflow(
    nwo: &str,
    repo: &Value,
    path: &str,
    events: &[String],
    policies: &[Value],
    out: &mut Perimeter,
) {
    let mut allowing = Vec::new();
    let mut gaps = Vec::new();
    let mut unknown = Vec::new();
    for policy in policies.iter().filter(|p| p["enforcement"] == "active") {
        match applies(policy, path, repo) {
            Applies::No => continue,
            Applies::Unknown(why) => {
                unknown.push(format!("{} ({why})", label(policy)));
                continue;
            }
            Applies::Yes => {}
        }
        for rule in policy["rules"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|rule| rule["type"] == "restrict_action_events")
        {
            let allowed: Vec<&str> = rule
                .pointer("/parameters/allowed_events")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let missing: Vec<&str> = events
                .iter()
                .map(String::as_str)
                .filter(|event| !allowed.contains(event))
                .collect();
            if missing.is_empty() {
                allowing.push(policy);
            } else {
                gaps.push((policy, missing));
            }
        }
    }
    for (policy, missing) in &gaps {
        let outage = if missing.contains(&EVENT) {
            ", and every pull request waits on a required check that never runs"
        } else {
            ""
        };
        out.event_warns.push(format!(
            "Actions event policy {} applies to {path} but does not allow {}, so GitHub never starts {path} on {}{outage}; add {} to that policy's allowed events",
            label(policy),
            missing.join(", "),
            if missing.len() == 1 { "that event" } else { "those events" },
            if missing.len() == 1 { "it" } else { "them" },
        ));
    }
    if !gaps.is_empty() {
        return;
    }
    if !unknown.is_empty() {
        out.event_unread.push(format!(
            "doctor cannot judge whether Actions event policy {} applies to {path}, so it cannot confirm that GitHub starts {path} on {EVENT}",
            unknown.join(", ")
        ));
        return;
    }
    if allowing.is_empty() {
        out.event_warns.push(format!(
            "no Actions event policy allows {EVENT} for {path}: on a public repository GitHub blocks that event from 2026-12-02 unless a policy allows it, so {path} never starts and every pull request waits on a required check that never runs. As a repository administrator, allow it for this workflow alone: echo '{}' | gh api --method POST repos/{nwo}/actions/policies --input -",
            fix_body(path, events)
        ));
        return;
    }
    for policy in allowing {
        if let Some(scope) = broad(policy) {
            out.event_warns.push(format!(
                "Actions event policy {} allows {EVENT} for {scope}, not only {path}, so any workflow added there runs on that event with the default branch's token and secrets; narrow its workflow targets to {path}",
                label(policy)
            ));
        }
    }
}

/// The policy that allows exactly the events `path` runs on, for `path`
/// alone: the one `remote protect` creates.
fn fix_body(path: &str, events: &[String]) -> String {
    crate::remote::event_policy_body(path, events).to_string()
}

/// `"name" (id)`, for messages: the JSON forms, so a missing name reads
/// `null`.
fn label(policy: &Value) -> String {
    format!("{} ({})", policy["name"], policy["id"])
}

/// Which workflows besides one exact path a policy targets, if any: every
/// workflow when it has no path condition or includes `~ALL`, or the
/// patterns with a wildcard.
fn broad(policy: &Value) -> Option<String> {
    let Some(include) = policy
        .pointer("/conditions/workflow_path/include")
        .and_then(Value::as_array)
    else {
        return Some("every workflow".to_string());
    };
    let patterns: Vec<&str> = include.iter().filter_map(Value::as_str).collect();
    if patterns.contains(&"~ALL") {
        return Some("every workflow".to_string());
    }
    let wild: Vec<&str> = patterns
        .into_iter()
        .filter(|pattern| pattern.contains(['*', '?', '[', '{']))
        .collect();
    (!wild.is_empty()).then(|| format!("the workflows matching {}", wild.join(", ")))
}

/// Whether `policy` applies to the workflow at `path` in `repo`.
fn applies(policy: &Value, path: &str, repo: &Value) -> Applies {
    let conditions = &policy["conditions"];
    if policy["source_type"] == "Organization" {
        if conditions.get("repository_property").is_some() {
            return Applies::Unknown("it selects repositories by property".to_string());
        }
        if let Some(names) = conditions.get("repository_name") {
            let Some(name) = repo["name"].as_str() else {
                return Applies::Unknown(
                    "it selects repositories by name, and the repository reported none".to_string(),
                );
            };
            match targeted(names, name, false) {
                Ok(true) => {}
                Ok(false) => return Applies::No,
                Err(why) => return Applies::Unknown(why),
            }
        }
        if let Some(ids) = conditions.pointer("/repository_id/repository_ids") {
            let Some(ids) = ids.as_array() else {
                return Applies::Unknown("its repository ids are not a list".to_string());
            };
            if !ids.contains(&repo["id"]) {
                return Applies::No;
            }
        }
    }
    match conditions.get("workflow_path") {
        None | Some(Value::Null) => Applies::Yes,
        Some(paths) => match targeted(paths, path, true) {
            Ok(true) => Applies::Yes,
            Ok(false) => Applies::No,
            Err(why) => Applies::Unknown(why),
        },
    }
}

/// Whether `value` is matched by a condition's `include` and not by its
/// `exclude`; `~ALL` includes everything. With `path`, a `/` in the value
/// must be matched literally.
fn targeted(condition: &Value, value: &str, path: bool) -> Result<bool, String> {
    let matches = |key: &str| -> Result<bool, String> {
        let mut any = false;
        for pattern in condition[key].as_array().into_iter().flatten() {
            let Some(pattern) = pattern.as_str() else {
                return Err(format!("an {key} pattern is not a string"));
            };
            if pattern == "~ALL" {
                any = true;
                continue;
            }
            let glob = globset::GlobBuilder::new(pattern)
                .literal_separator(path)
                .build()
                .map_err(|error| format!("the pattern {pattern} does not parse: {error}"))?;
            any |= glob.compile_matcher().is_match(value);
        }
        Ok(any)
    };
    Ok(matches("include")? && !matches("exclude")?)
}

/// Every Actions event policy that can apply to the repository, in full:
/// the repository's, and the organization's for an organization-owned one.
fn read_policies(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    nwo: &str,
    repo: &Value,
) -> Result<Vec<Value>, String> {
    let mut summaries = list(gh, &format!("repos/{nwo}/actions/policies"), nwo)?;
    if repo.pointer("/owner/type").and_then(Value::as_str) == Some("Organization") {
        let Some(org) = repo.pointer("/owner/login").and_then(Value::as_str) else {
            return Err(format!("{nwo} reported no owner login"));
        };
        summaries.extend(list(
            gh,
            &format!("orgs/{org}/actions/policies"),
            &format!("the organization {org}"),
        )?);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut policies = Vec::new();
    for summary in summaries {
        let id = summary["id"].clone();
        if !id.is_u64() {
            return Err(format!(
                "an Actions event policy of {nwo} has no id: {summary}"
            ));
        }
        if !seen.insert(id.to_string()) {
            continue;
        }
        // The list names each policy's source; an organization policy
        // without one cannot be read, so it is unknown, never skipped.
        let url = match (summary["source_type"].as_str(), summary["source"].as_str()) {
            (Some("Repository") | None, _) => format!("repos/{nwo}/actions/policies/{id}"),
            (Some("Organization"), Some(org)) => format!("orgs/{org}/actions/policies/{id}"),
            (Some(kind), source) => {
                return Err(format!(
                "Actions event policy {} comes from the {} {}, whose conditions doctor cannot read",
                label(&summary),
                kind.to_ascii_lowercase(),
                source.map_or_else(|| "that the list does not name".to_string(), str::to_string)
            ))
            }
        };
        let text = gh(&["api", &url]).map_err(|error| {
            format!(
                "Actions event policy {} is not readable ({})",
                label(&summary),
                super::super::first_line(&error)
            )
        })?;
        let mut policy: Value = serde_json::from_str(&text)
            .map_err(|_| format!("Actions event policy {} is not JSON", label(&summary)))?;
        if policy.get("source_type").is_none() {
            policy["source_type"] = summary["source_type"].clone();
        }
        policies.push(policy);
    }
    Ok(policies)
}

/// Every policy summary of one list, all pages, or why not.
fn list(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    url: &str,
    owner: &str,
) -> Result<Vec<Value>, String> {
    crate::remote::paged_list(gh, url, "policies").map_err(|why| {
        // Reading needs Administration write, and the host refuses a
        // token without it.
        if why.contains("HTTP 403") || why.contains("HTTP 404") {
            format!("the Actions event policies of {owner} are unreadable: needs an administrator's token ({why})")
        } else {
            format!("the Actions event policies of {owner} are not readable ({why})")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    const PATH: &str = ".github/workflows/codeflow-policy.yml";

    /// The live list of `sathyassn/codeflow`, read 2026-11-05 21:06Z.
    const LIVE_LIST: &str = r#"{"total_count":1,"policies":[{"id":11322,"name":"codeflow policy check","target":"actions","source_type":"Repository","source":"sathyassn/codeflow","enforcement":"active","node_id":"MDE0OkFjdGlvbnNSdWxlc2V0MTEzMjI=","_links":{"self":{"href":"https://api.github.com/repos/sathyassn/codeflow/actions/policies/11322"},"html":{"href":"https://github.com/sathyassn/codeflow/settings/actions/rules/11322"}},"created_at":"2026-11-05T14:21:13.202-05:00","updated_at":"2026-11-05T14:21:13.241-05:00"}]}"#;

    /// The live policy 11322, read 2026-11-05 21:06Z.
    const LIVE_POLICY: &str = r#"{"id":11322,"name":"codeflow policy check","target":"actions","source_type":"Repository","source":"sathyassn/codeflow","enforcement":"active","conditions":{"workflow_path":{"exclude":[],"include":[".github/workflows/codeflow-policy.yml"]}},"rules":[{"type":"restrict_action_events","parameters":{"allowed_events":["pull_request_target","push","schedule","workflow_dispatch"]}}],"node_id":"MDE0OkFjdGlvbnNSdWxlc2V0MTEzMjI=","created_at":"2026-11-05T14:21:13.202-05:00","updated_at":"2026-11-05T14:21:13.241-05:00","_links":{"self":{"href":"https://api.github.com/repos/sathyassn/codeflow/actions/policies/11322"},"html":{"href":"https://github.com/sathyassn/codeflow/settings/actions/rules/11322"}}}"#;

    /// GitHub's answer to a token without administration rights.
    const FORBIDDEN: &str = "gh: Resource not accessible by integration (HTTP 403)";

    fn public() -> Value {
        serde_json::json!({"id": 1, "name": "codeflow", "visibility": "public",
            "owner": {"login": "sathyassn", "type": "User"}})
    }

    fn declared(events: &[(&str, &[&str])]) -> Declared {
        Declared {
            events: events
                .iter()
                .map(|(path, events)| {
                    (
                        (*path).to_string(),
                        events.iter().map(|e| (*e).to_string()).collect(),
                    )
                })
                .collect(),
            ..Declared::default()
        }
    }

    fn policy_workflow() -> Declared {
        declared(&[
            (PATH, &["pull_request_target"]),
            (
                ".github/workflows/codeflow-ci.yml",
                &["pull_request", "push"],
            ),
        ])
    }

    /// A host serving `answers` by URL (the query of a list page dropped
    /// for page 1, and an empty page after it); every call is recorded.
    struct Host {
        answers: BTreeMap<String, Result<String, String>>,
        calls: RefCell<Vec<String>>,
    }

    impl Host {
        fn new(answers: &[(&str, Result<&str, &str>)]) -> Self {
            Self {
                answers: answers
                    .iter()
                    .map(|(url, answer)| {
                        (
                            (*url).to_string(),
                            answer.map(str::to_string).map_err(str::to_string),
                        )
                    })
                    .collect(),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn gh(&self, args: &[&str]) -> Result<String, String> {
            let url = args.last().copied().unwrap_or_default();
            self.calls.borrow_mut().push(url.to_string());
            let key = url.strip_suffix("?per_page=100&page=1").unwrap_or(url);
            if url.ends_with("&page=2") {
                return Ok(r#"{"total_count":0,"policies":[]}"#.into());
            }
            self.answers
                .get(key)
                .cloned()
                .unwrap_or_else(|| Err(format!("unexpected gh call {url}")))
        }
    }

    fn run(host: &Host, repo: &Value, declared: &Declared) -> Perimeter {
        let mut out = Perimeter::default();
        judge(
            &|args| host.gh(args),
            "sathyassn/codeflow",
            repo,
            declared,
            &mut out,
        );
        out
    }

    fn list_of(policies: &[&str]) -> String {
        let ids: Vec<String> = policies
            .iter()
            .map(|p| {
                let v: Value = serde_json::from_str(p).unwrap();
                format!(
                    r#"{{"id":{},"name":{},"source_type":"Repository","source":"sathyassn/codeflow","enforcement":{}}}"#,
                    v["id"], v["name"], v["enforcement"]
                )
            })
            .collect();
        format!(
            r#"{{"total_count":{},"policies":[{}]}}"#,
            ids.len(),
            ids.join(",")
        )
    }

    fn policy(id: u64, enforcement: &str, conditions: &str, events: &[&str]) -> String {
        format!(
            r#"{{"id":{id},"name":"p{id}","source_type":"Repository","enforcement":"{enforcement}","conditions":{conditions},"rules":[{{"type":"restrict_action_events","parameters":{{"allowed_events":{}}}}}]}}"#,
            serde_json::to_string(events).unwrap()
        )
    }

    const ONLY_POLICY: &str =
        r#"{"workflow_path":{"include":[".github/workflows/codeflow-policy.yml"],"exclude":[]}}"#;

    #[test]
    fn the_live_policy_allows_the_policy_workflow() {
        let host = Host::new(&[
            ("repos/sathyassn/codeflow/actions/policies", Ok(LIVE_LIST)),
            (
                "repos/sathyassn/codeflow/actions/policies/11322",
                Ok(LIVE_POLICY),
            ),
        ]);
        let out = run(&host, &public(), &policy_workflow());
        assert!(out.event_warns.is_empty(), "{:?}", out.event_warns);
        assert!(out.event_unread.is_empty(), "{:?}", out.event_unread);
        assert!(out.fails.is_empty());
    }

    #[test]
    fn no_policy_on_a_public_repository_warns_with_the_fix() {
        let host = Host::new(&[(
            "repos/sathyassn/codeflow/actions/policies",
            Ok(r#"{"total_count":0,"policies":[]}"#),
        )]);
        let out = run(&host, &public(), &policy_workflow());
        assert_eq!(out.event_warns.len(), 1, "{:?}", out.event_warns);
        let warn = &out.event_warns[0];
        assert!(
            warn.contains("no Actions event policy allows pull_request_target for .github/workflows/codeflow-policy.yml"),
            "{warn}"
        );
        assert!(warn.contains("from 2026-12-02"), "{warn}");
        // The fix is the exact policy, for this workflow alone and its events.
        let body = warn
            .split("echo '")
            .nth(1)
            .and_then(|rest| {
                rest.split(
                    "' | gh api --method POST repos/sathyassn/codeflow/actions/policies --input -",
                )
                .next()
            })
            .expect(warn);
        let body: Value = serde_json::from_str(body).expect(body);
        assert_eq!(
            body["conditions"]["workflow_path"]["include"],
            serde_json::json!([PATH])
        );
        assert_eq!(
            body["rules"][0]["parameters"]["allowed_events"],
            serde_json::json!(["pull_request_target"])
        );
        assert_eq!(body["enforcement"], "active");
        assert!(out.event_unread.is_empty());
    }

    #[test]
    fn a_policy_that_leaves_the_event_out_blocks_it() {
        let blocking = policy(5, "active", ONLY_POLICY, &["push", "workflow_dispatch"]);
        let host = Host::new(&[
            (
                "repos/sathyassn/codeflow/actions/policies",
                Ok(&list_of(&[&blocking])),
            ),
            ("repos/sathyassn/codeflow/actions/policies/5", Ok(&blocking)),
        ]);
        let out = run(&host, &public(), &policy_workflow());
        assert_eq!(out.event_warns.len(), 1, "{:?}", out.event_warns);
        assert!(
            out.event_warns[0].contains(r#"policy "p5" (5) applies to .github/workflows/codeflow-policy.yml but does not allow pull_request_target"#)
                && out.event_warns[0].contains("required check that never runs"),
            "{}",
            out.event_warns[0]
        );
    }

    #[test]
    fn a_policy_allowing_only_some_events_blocks_the_rest_even_beside_an_allowing_one() {
        // Policies layer: the live policy allows everything, a second one
        // targeting all workflows leaves out pull_request_target.
        let all = policy(6, "active", "{}", &["push", "pull_request"]);
        let host = Host::new(&[
            (
                "repos/sathyassn/codeflow/actions/policies",
                Ok(&list_of(&[LIVE_POLICY, &all])),
            ),
            (
                "repos/sathyassn/codeflow/actions/policies/11322",
                Ok(LIVE_POLICY),
            ),
            ("repos/sathyassn/codeflow/actions/policies/6", Ok(&all)),
        ]);
        let out = run(&host, &public(), &policy_workflow());
        assert_eq!(out.event_warns.len(), 1, "{:?}", out.event_warns);
        assert!(
            out.event_warns[0].contains(r#""p6" (6)"#),
            "{}",
            out.event_warns[0]
        );
    }

    #[test]
    fn an_unreadable_policy_list_is_a_note_never_a_pass() {
        let host = Host::new(&[("repos/sathyassn/codeflow/actions/policies", Err(FORBIDDEN))]);
        let out = run(&host, &public(), &policy_workflow());
        assert!(out.event_warns.is_empty(), "{:?}", out.event_warns);
        assert_eq!(out.event_unread.len(), 1, "{:?}", out.event_unread);
        let note = &out.event_unread[0];
        assert!(
            note.contains("the Actions event policies of sathyassn/codeflow are unreadable: needs an administrator's token (gh: Resource not accessible by integration (HTTP 403))"),
            "{note}"
        );
        assert!(note.contains("administrator's token"), "{note}");
    }

    #[test]
    fn an_unreadable_policy_is_a_note_never_a_pass() {
        let host = Host::new(&[
            ("repos/sathyassn/codeflow/actions/policies", Ok(LIVE_LIST)),
            (
                "repos/sathyassn/codeflow/actions/policies/11322",
                Err(FORBIDDEN),
            ),
        ]);
        let out = run(&host, &public(), &policy_workflow());
        assert!(out.event_warns.is_empty(), "{:?}", out.event_warns);
        assert!(
            out.event_unread[0]
                .contains(r#"policy "codeflow policy check" (11322) is not readable"#),
            "{:?}",
            out.event_unread
        );
    }

    #[test]
    fn a_short_list_is_a_note() {
        let host = Host::new(&[(
            "repos/sathyassn/codeflow/actions/policies",
            Ok(r#"{"total_count":3,"policies":[]}"#),
        )]);
        let out = run(&host, &public(), &policy_workflow());
        assert!(out.event_warns.is_empty());
        assert!(
            out.event_unread[0].contains("ended before its total_count"),
            "{:?}",
            out.event_unread
        );
    }

    #[test]
    fn private_internal_and_unaffected_repositories_read_no_policy() {
        let host = Host::new(&[]);
        for visibility in ["private", "internal"] {
            let mut repo = public();
            repo["visibility"] = visibility.into();
            let out = run(&host, &repo, &policy_workflow());
            assert!(out.event_warns.is_empty() && out.event_unread.is_empty());
        }
        // No workflow runs on the event.
        let out = run(
            &host,
            &public(),
            &declared(&[(".github/workflows/ci.yml", &["pull_request"])]),
        );
        assert!(out.event_warns.is_empty() && out.event_unread.is_empty());
        assert!(host.calls.borrow().is_empty(), "{:?}", host.calls.borrow());
    }

    #[test]
    fn an_unknown_visibility_is_a_note() {
        let host = Host::new(&[]);
        let mut repo = public();
        repo.as_object_mut().unwrap().remove("visibility");
        let out = run(&host, &repo, &policy_workflow());
        assert!(out.event_warns.is_empty());
        assert!(
            out.event_unread[0].contains("reported no visibility"),
            "{:?}",
            out.event_unread
        );
    }

    #[test]
    fn policies_that_do_not_apply_leave_the_default_block() {
        // Evaluate and disabled policies block nothing and allow nothing; a
        // policy for another workflow, or one that excludes this one, does
        // not apply.
        let evaluate = policy(1, "evaluate", ONLY_POLICY, &["pull_request_target"]);
        let disabled = policy(2, "disabled", ONLY_POLICY, &["pull_request_target"]);
        let other = policy(
            3,
            "active",
            r#"{"workflow_path":{"include":[".github/workflows/other.yml"],"exclude":[]}}"#,
            &["pull_request_target"],
        );
        let excluded = policy(
            4,
            "active",
            r#"{"workflow_path":{"include":["~ALL"],"exclude":[".github/workflows/codeflow-policy.yml"]}}"#,
            &["pull_request_target"],
        );
        let all = [&evaluate, &disabled, &other, &excluded];
        let list = list_of(&all.map(String::as_str));
        let mut answers = vec![(
            "repos/sathyassn/codeflow/actions/policies".to_string(),
            list,
        )];
        for (id, body) in (1..=4).zip(all) {
            answers.push((
                format!("repos/sathyassn/codeflow/actions/policies/{id}"),
                body.clone(),
            ));
        }
        let answers: Vec<(&str, Result<&str, &str>)> = answers
            .iter()
            .map(|(url, body)| (url.as_str(), Ok(body.as_str())))
            .collect();
        let host = Host::new(&answers);
        let out = run(&host, &public(), &policy_workflow());
        assert_eq!(out.event_warns.len(), 1, "{:?}", out.event_warns);
        assert!(
            out.event_warns[0].starts_with("no Actions event policy allows pull_request_target"),
            "{}",
            out.event_warns[0]
        );
    }

    #[test]
    fn an_allowing_policy_for_every_workflow_warns() {
        for (conditions, scope) in [
            ("{}", "every workflow"),
            (
                r#"{"workflow_path":{"include":["~ALL"],"exclude":[]}}"#,
                "every workflow",
            ),
            (
                r#"{"workflow_path":{"include":[".github/workflows/*.yml"],"exclude":[]}}"#,
                "the workflows matching .github/workflows/*.yml",
            ),
        ] {
            let wide = policy(
                9,
                "active",
                conditions,
                &["pull_request_target", "pull_request", "push"],
            );
            let host = Host::new(&[
                (
                    "repos/sathyassn/codeflow/actions/policies",
                    Ok(&list_of(&[&wide])),
                ),
                ("repos/sathyassn/codeflow/actions/policies/9", Ok(&wide)),
            ]);
            let out = run(&host, &public(), &policy_workflow());
            assert_eq!(
                out.event_warns.len(),
                1,
                "{conditions}: {:?}",
                out.event_warns
            );
            assert!(
                out.event_warns[0]
                    .contains(&format!("allows pull_request_target for {scope}, not only")),
                "{}",
                out.event_warns[0]
            );
        }
    }

    #[test]
    fn an_organization_repository_reads_the_organization_policies_too() {
        let mut repo = public();
        repo["owner"] = serde_json::json!({"login": "acme", "type": "Organization"});
        let empty = r#"{"total_count":0,"policies":[]}"#;

        // The organization's list cannot be read: a note.
        let host = Host::new(&[
            ("repos/sathyassn/codeflow/actions/policies", Ok(empty)),
            ("orgs/acme/actions/policies", Err(FORBIDDEN)),
        ]);
        let out = run(&host, &repo, &policy_workflow());
        assert!(out.event_warns.is_empty());
        assert!(
            out.event_unread[0]
                .contains("the Actions event policies of the organization acme are unreadable: needs an administrator's token"),
            "{:?}",
            out.event_unread
        );

        // An organization policy that targets this repository by name and
        // allows the event: no warning.
        let org = r#"{"id":40,"name":"org allow","source_type":"Organization","source":"acme","enforcement":"active","conditions":{"repository_name":{"include":["code*"],"exclude":[]},"workflow_path":{"include":[".github/workflows/codeflow-policy.yml"],"exclude":[]}},"rules":[{"type":"restrict_action_events","parameters":{"allowed_events":["pull_request_target"]}}]}"#;
        let org_list = r#"{"total_count":1,"policies":[{"id":40,"name":"org allow","source_type":"Organization","source":"acme","enforcement":"active"}]}"#;
        let host = Host::new(&[
            ("repos/sathyassn/codeflow/actions/policies", Ok(empty)),
            ("orgs/acme/actions/policies", Ok(org_list)),
            ("orgs/acme/actions/policies/40", Ok(org)),
        ]);
        let out = run(&host, &repo, &policy_workflow());
        assert!(
            out.event_warns.is_empty() && out.event_unread.is_empty(),
            "{:?} {:?}",
            out.event_warns,
            out.event_unread
        );

        // One that selects repositories by property cannot be judged.
        let by_property = org.replace(
            r#""repository_name":{"include":["code*"],"exclude":[]}"#,
            r#""repository_property":{"include":[{"name":"tier","property_values":["a"]}]}"#,
        );
        let host = Host::new(&[
            ("repos/sathyassn/codeflow/actions/policies", Ok(empty)),
            ("orgs/acme/actions/policies", Ok(org_list)),
            ("orgs/acme/actions/policies/40", Ok(&by_property)),
        ]);
        let out = run(&host, &repo, &policy_workflow());
        assert!(out.event_warns.is_empty());
        assert!(
            out.event_unread[0].contains("selects repositories by property"),
            "{:?}",
            out.event_unread
        );

        // An organization policy whose list entry names no source cannot be
        // read, so it is a note.
        let unsourced = org_list.replace(r#""source":"acme","#, "");
        let host = Host::new(&[
            ("repos/sathyassn/codeflow/actions/policies", Ok(empty)),
            ("orgs/acme/actions/policies", Ok(&unsourced)),
        ]);
        let out = run(&host, &repo, &policy_workflow());
        assert!(out.event_warns.is_empty(), "{:?}", out.event_warns);
        assert!(
            out.event_unread[0].contains("the organization that the list does not name"),
            "{:?}",
            out.event_unread
        );

        // A repository that reports no name cannot be matched by name, so
        // the name-selected policy is unknown, never "does not apply".
        let host = Host::new(&[
            ("repos/sathyassn/codeflow/actions/policies", Ok(empty)),
            ("orgs/acme/actions/policies", Ok(org_list)),
            ("orgs/acme/actions/policies/40", Ok(org)),
        ]);
        let mut nameless = repo.clone();
        nameless.as_object_mut().unwrap().remove("name");
        let out = run(&host, &nameless, &policy_workflow());
        assert!(out.event_warns.is_empty(), "{:?}", out.event_warns);
        assert!(
            out.event_unread[0].contains("the repository reported none"),
            "{:?}",
            out.event_unread
        );
    }

    #[test]
    fn workflow_events_read_every_on_form() {
        use crate::remote::workflow_events;
        assert_eq!(
            workflow_events("on: pull_request_target\njobs: {}\n").unwrap(),
            ["pull_request_target"]
        );
        assert_eq!(
            workflow_events("on: [push, pull_request_target]\n").unwrap(),
            ["push", "pull_request_target"]
        );
        assert_eq!(
            workflow_events(
                "on:\n  pull_request_target:\n    types: [opened]\n  push:\n    branches: [main]\n"
            )
            .unwrap(),
            ["pull_request_target", "push"]
        );
        assert!(workflow_events("jobs: {}\n").unwrap().is_empty());
        assert!(workflow_events("on: 3\n").is_err());
    }
}
