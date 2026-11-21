//! Remote ref protection is a separate finding from required merge checks.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use serde_json::Value;

use super::super::{CheckResult, Options, Status};
use crate::hooks::policy::Policy;
use crate::remedy;

pub(in crate::doctor) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = Path::new(&opts.project_dir);
    let gaps = match Policy::load(root) {
        Ok(policy) => match findings(opts, &policy) {
            Ok(gaps) => gaps,
            Err(why) => policy
                .git
                .protected_branches
                .iter()
                .map(|name| gap(name, &why))
                .collect(),
        },
        Err(why) => vec![gap("configured protected branches", &why)],
    };
    CheckResult {
        name: "remote-ref-protection".into(),
        status: if gaps.is_empty() {
            Status::Pass
        } else {
            Status::Warn(remedy::DOCTOR_REMOTE_REF_PROTECTION.remedy())
        },
        message: if gaps.is_empty() {
            "remote force-update and deletion protection verified for every existing protected branch (if any)".into()
        } else {
            gaps.join("; ")
        },
        duration: start.elapsed(),
    }
}

fn gap(branch: &str, why: &str) -> String {
    format!("cannot verify remote force-update and deletion protection for {branch}: {why}; when the push hook is skipped, a forced update or deletion on the remote has no verified backstop; clear it with `codeflow remote protect` or the host's settings")
}

fn findings(opts: &Options, policy: &Policy) -> Result<Vec<String>, String> {
    if policy.git.protected_branches.is_empty() {
        return Ok(Vec::new());
    }
    let root = Path::new(&opts.project_dir);
    let nwo = super::origin_or_why(root)?;
    opts.do_look_path("gh")
        .map_err(|_| "`gh` is unavailable".to_string())?;
    let gh = |args: &[&str]| {
        opts.do_exec_bounded(
            "gh",
            args,
            codeflow_limits::DOCTOR_GH_TIMEOUT_SECONDS.wait(),
        )
    };
    let branches = branches(&gh, &nwo)?;
    let mut gaps = Vec::new();
    // A ruleset may cover many branches; read each bypass list only once.
    let mut bound = BTreeMap::new();
    for branch in branches
        .iter()
        .filter(|name| policy.git.branch_is_protected(name))
    {
        if let Err(why) = verify_branch(&gh, &nwo, branch, &mut bound) {
            gaps.push(gap(branch, &why));
        }
    }
    Ok(gaps)
}

type Host<'a> = dyn Fn(&[&str]) -> Result<String, String> + 'a;

fn branches(gh: &Host<'_>, nwo: &str) -> Result<BTreeSet<String>, String> {
    let mut branches = BTreeSet::new();
    for page in 1.. {
        let text = gh(&[
            "api",
            &format!("repos/{nwo}/branches?per_page=100&page={page}"),
        ])?;
        let rows = serde_json::from_str::<Vec<Value>>(&text)
            .map_err(|_| "host branch list is not a JSON list".to_string())?;
        for row in &rows {
            let name = row["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or_else(|| "host branch list has an unreadable name".to_string())?;
            branches.insert(name.to_string());
        }
        if rows.len() < 100 {
            return Ok(branches);
        }
    }
    unreachable!()
}

fn verify_branch(
    gh: &Host<'_>,
    nwo: &str,
    branch: &str,
    bound: &mut BTreeMap<u64, bool>,
) -> Result<(), String> {
    let encoded = path_segment(branch);
    let (classic, classic_unread) = super::classic_protection(gh, nwo, &encoded);
    // Classic protection counts only when both flags and administrator
    // enforcement are explicit. Missing fields do not prove a boundary.
    let classic_blocks = |key: &str| {
        classic.as_ref().is_some_and(|c| {
            c.pointer("/enforce_admins/enabled") == Some(&Value::Bool(true))
                && c.pointer(&format!("/{key}/enabled")) == Some(&Value::Bool(false))
        })
    };
    let mut force = classic_blocks("allow_force_pushes");
    let mut deletion = classic_blocks("allow_deletions");
    if force && deletion {
        return Ok(());
    }
    let rules = super::branch_rules(gh, nwo, &encoded)?;
    for rule in rules.as_array().into_iter().flatten() {
        let Some(kind) = rule["type"].as_str() else {
            continue;
        };
        if kind != "non_fast_forward" && kind != "deletion" {
            continue;
        }
        let Some(id) = rule["ruleset_id"].as_u64() else {
            continue;
        };
        let binds_everyone = *bound.entry(id).or_insert_with(|| {
            gh(&["api", &format!("repos/{nwo}/rulesets/{id}")]).is_ok_and(|text| {
                serde_json::from_str::<Value>(&text)
                    .is_ok_and(|body| body["bypass_actors"].as_array().is_some_and(Vec::is_empty))
            })
        });
        if binds_everyone {
            force |= kind == "non_fast_forward";
            deletion |= kind == "deletion";
        }
    }
    if force && deletion {
        Ok(())
    } else {
        Err(format!(
            "force-update protection {}, deletion protection {}{}",
            if force {
                "verified"
            } else {
                "missing or unverifiable"
            },
            if deletion {
                "verified"
            } else {
                "missing or unverifiable"
            },
            classic_unread.map_or(String::new(), |why| format!("; {why}"))
        ))
    }
}

fn path_segment(branch: &str) -> String {
    let mut out = String::new();
    for byte in branch.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn judge(exec: fn(&str, &[&str]) -> Result<String, String>) -> CheckResult {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        repo.remote("origin", "https://github.com/o/r.git").unwrap();
        std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
        std::fs::write(
            dir.path().join(".codeflow/policy.json"),
            r#"{"git":{"protected_branches":["main","release/*"]}}"#,
        )
        .unwrap();
        check(&Options {
            project_dir: dir.path().to_str().unwrap().into(),
            look_path: Some(|_| Ok("gh".into())),
            exec_command: Some(exec),
            ..Options::default()
        })
    }

    fn host(args: &[&str], rules: &Value, classic: Option<Value>) -> Result<String, String> {
        match args {
            ["api", url] if url.starts_with("repos/o/r/branches?") => {
                Ok(json!([{"name":"main"},{"name":"release/1"},{"name":"feat/x"}]).to_string())
            }
            ["api", url] if url.contains("/rules/branches/") => Ok(rules.to_string()),
            ["api", url] if url.ends_with("/protection") => classic
                .map(|v| v.to_string())
                .ok_or_else(|| "HTTP 404".into()),
            ["api", "repos/o/r/rulesets/7"] => Ok(json!({"bypass_actors":[]}).to_string()),
            other => Err(format!("unexpected call: {other:?}")),
        }
    }

    #[test]
    fn missing_protection_names_each_protected_branch_and_the_gap() {
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            host(args, &json!([]), None)
        }
        let result = judge(exec);
        assert!(result.status.is_warn(), "{result:?}");
        for text in [
            "main",
            "release/1",
            "forced update or deletion",
            "push hook is skipped",
            "codeflow remote protect",
            "host's settings",
        ] {
            assert!(result.message.contains(text), "{text}: {}", result.message);
        }
        assert!(!result.message.contains("feat/x"));
    }

    #[test]
    fn verified_rules_are_quiet() {
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            host(
                args,
                &json!([
                    {"type":"non_fast_forward","ruleset_id":7},
                    {"type":"deletion","ruleset_id":7}
                ]),
                None,
            )
        }
        let result = judge(exec);
        assert_eq!(result.status, Status::Pass, "{result:?}");
        assert!(!result.message.contains("skipped"));
    }

    #[test]
    fn unreadable_protection_warns_without_claiming_it_is_absent() {
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            if args
                .iter()
                .any(|arg| arg.contains("/rules/branches/") || arg.ends_with("/protection"))
            {
                return Err("HTTP 403".into());
            }
            host(args, &json!([]), None)
        }
        let result = judge(exec);
        assert!(result.status.is_warn(), "{result:?}");
        assert!(result.message.contains("cannot verify"), "{result:?}");
        assert!(result.message.contains("HTTP 403"), "{result:?}");
    }

    #[test]
    fn classic_protection_can_verify_both_rules_without_readable_rulesets() {
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            assert!(
                !args.iter().any(|arg| arg.contains("/rules/branches/")),
                "classic protection already proves both controls"
            );
            host(
                args,
                &json!([]),
                Some(json!({
                    "enforce_admins":{"enabled":true},
                    "allow_force_pushes":{"enabled":false},
                    "allow_deletions":{"enabled":false}
                })),
            )
        }
        assert_eq!(judge(exec).status, Status::Pass);
    }

    #[test]
    fn bypassable_or_unreadable_rules_never_verify_protection() {
        for bypass in [r#"{"bypass_actors":[{"actor_id":1}]}"#, "{}", "not JSON"] {
            let gh = |args: &[&str]| {
                if args == ["api", "repos/o/r/rulesets/7"] {
                    Ok(bypass.to_string())
                } else {
                    host(
                        args,
                        &json!([
                            {"type":"non_fast_forward","ruleset_id":7},
                            {"type":"deletion","ruleset_id":7}
                        ]),
                        None,
                    )
                }
            };
            assert!(verify_branch(&gh, "o/r", "main", &mut BTreeMap::new()).is_err());
        }
    }

    #[test]
    fn one_control_or_checks_alone_do_not_verify_both_controls() {
        for kind in ["non_fast_forward", "deletion", "required_status_checks"] {
            let gh = |args: &[&str]| host(args, &json!([{"type":kind,"ruleset_id":7}]), None);
            assert!(verify_branch(&gh, "o/r", "main", &mut BTreeMap::new()).is_err());
        }
        for classic in [
            json!({"enforce_admins":{"enabled":false}, "allow_force_pushes":{"enabled":false}, "allow_deletions":{"enabled":false}}),
            json!({"enforce_admins":{"enabled":true}}),
        ] {
            let gh = |args: &[&str]| host(args, &json!([]), Some(classic.clone()));
            assert!(verify_branch(&gh, "o/r", "main", &mut BTreeMap::new()).is_err());
        }
    }

    #[test]
    fn branches_are_paginated_and_ref_names_are_encoded() {
        let gh = |args: &[&str]| match args {
            ["api", url] if url.ends_with("page=1") => Ok(Value::Array(
                (0..100)
                    .map(|n| json!({"name":format!("feat/{n}")}))
                    .collect(),
            )
            .to_string()),
            ["api", url] if url.ends_with("page=2") => {
                Ok(json!([{"name":"release/1"}]).to_string())
            }
            other => panic!("unexpected call {other:?}"),
        };
        let names = branches(&gh, "o/r").unwrap();
        assert_eq!(names.len(), 101);
        assert!(names.contains("release/1"));
        assert_eq!(path_segment("release/1#?%"), "release%2F1%23%3F%25");
    }

    #[test]
    fn unreadable_branch_list_reports_the_configured_patterns() {
        fn exec(_: &str, _: &[&str]) -> Result<String, String> {
            Err("HTTP 403".into())
        }
        let result = judge(exec);
        assert!(result.status.is_warn());
        assert!(result.message.contains("main"), "{result:?}");
        assert!(result.message.contains("release/*"), "{result:?}");
    }
}
