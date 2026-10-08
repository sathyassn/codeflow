//! The remote perimeter: whether the default branch's live host rules make
//! a pull request merge only on checks that ran on its current tip
//! (TSK-261).
//!
//! Required checks are point in time: CI tests the merge of a branch with
//! the base as it was when the run started. Unless the host requires the
//! branch to be up to date, two pull requests each green on an older base
//! can both merge and leave the default branch red. This check reads the
//! rules GitHub applies to the default branch (`rules/branches/<branch>`,
//! readable with read access) and its classic protection, and compares them
//! with `git.required_checks`. Bypass lists belong to one rule each, so a
//! policy check, or the up-to-date requirement, counts only when a rule that
//! binds everyone requires it. It warns, never blocks; when `gh`, the
//! network or a GitHub `origin` is missing, or the rules, the classic
//! protection or a bypass list that decides the answer cannot be read (the
//! host omits a bypass list without write access), it says so in a note
//! rather than passing or warning. Only a 404 means no classic protection.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{CheckResult, Options, Status};
use crate::remedy;

const NAME: &str = "remote-perimeter";

/// Each `gh` call is bounded, so an unreachable host cannot hang doctor.
const GH_TIMEOUT: Duration = Duration::from_secs(15);

/// Rules read per page of `rules/branches/<branch>`.
const RULES_PAGE_SIZE: usize = 100;

/// One rule that requires status checks: a ruleset or classic protection.
struct Source {
    label: String,
    strict: bool,
    contexts: Vec<String>,
    /// The ruleset id, for reading its bypass list.
    ruleset_id: Option<u64>,
    /// Classic protection only: whether it binds administrators.
    enforce_admins: Option<bool>,
    classic: bool,
}

/// Whether everyone is held to a source.
enum Bypass {
    /// Nobody can bypass it.
    Bound,
    /// Someone can, and how.
    Bypassable(String),
    /// Its bypass list cannot be read, and why.
    Unknown(String),
}

pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let result = |status: Status, message: String| CheckResult {
        name: NAME.into(),
        status,
        message,
        duration: start.elapsed(),
    };
    let note =
        |message: String| result(Status::Note(remedy::DOCTOR_REMOTE_UNREAD.remedy()), message);

    let root = Path::new(&opts.project_dir);
    let Some(nwo) = github_origin(root) else {
        return note(
            "no GitHub `origin` remote, so doctor reads no host rules for the default branch"
                .into(),
        );
    };
    if opts.do_look_path("gh").is_err() {
        return note(format!(
            "`gh` not found, so doctor cannot read the host rules of {nwo}"
        ));
    }
    let gh = |args: &[&str]| opts.do_exec_bounded("gh", args, GH_TIMEOUT);
    let parse = |text: &str| serde_json::from_str::<Value>(text).unwrap_or(Value::Null);

    let repo = match gh(&["api", &format!("repos/{nwo}")]) {
        Ok(text) => parse(&text),
        Err(error) => {
            return note(format!(
                "could not read {nwo} through `gh` ({})",
                super::first_line(&error)
            ))
        }
    };
    let Some(branch) = repo
        .get("default_branch")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return note(format!("{nwo} reported no default branch"));
    };
    let rules = match branch_rules(&gh, &nwo, &branch) {
        Ok(rules) => rules,
        Err(error) => {
            return note(format!(
                "could not read the rules of {nwo} {branch} through `gh` ({})",
                super::first_line(&error)
            ))
        }
    };
    let mut bypass_unread = Vec::new();
    let (classic, classic_unread) = classic_protection(&gh, &nwo, &branch);

    let sources = sources(&rules, classic.as_ref());
    let required: Vec<String> = crate::hooks::policy::Policy::load(root).git.required_checks;
    let problems = problems(opts, &nwo, &branch, &sources, &required, &mut bypass_unread);
    let mut unread: Vec<String> = classic_unread.iter().cloned().collect();
    unread.extend(bypass_unread.iter().cloned());

    if let (Some(why), false) = (&classic_unread, problems.is_empty()) {
        // Classic protection can add checks, the up-to-date rule or an
        // administrator binding, so what the rulesets lack alone is not a
        // gap doctor can report.
        let rest = if bypass_unread.is_empty() {
            String::new()
        } else {
            format!("; {}", bypass_unread.join("; "))
        };
        return note(format!(
            "doctor cannot judge {nwo} {branch}: {why}, and the rulesets alone do not require every name in git.required_checks on an up-to-date branch for everyone{rest}"
        ));
    }
    if problems.is_empty() && !bypass_unread.is_empty() {
        // The rules look right, but a bypass list that cannot be read could
        // hold an actor who skips them, so this is not a pass.
        return note(format!(
            "{nwo} {branch} requires up-to-date checks, but doctor cannot confirm nobody can bypass them: {}",
            unread.join("; ")
        ));
    }
    let unread = if unread.is_empty() {
        String::new()
    } else {
        format!("; {}", unread.join("; "))
    };
    if problems.is_empty() {
        let checks: Vec<String> = sources
            .iter()
            .flat_map(|s| s.contexts.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        result(
            Status::Pass,
            format!(
                "{nwo} {branch} requires {} on an up-to-date branch, including every name in git.required_checks{unread}",
                checks.join(", ")
            ),
        )
    } else {
        result(
            Status::Warn(remedy::DOCTOR_REMOTE_PERIMETER.with(&[("branch", &branch)])),
            format!("{nwo}: {}{unread}", problems.join("; ")),
        )
    }
}

/// The rules the host applies to `branch`, all pages: `rules/branches`
/// pages (30 by default), and a check-requiring rule past the first page
/// must still be seen.
fn branch_rules(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    nwo: &str,
    branch: &str,
) -> Result<Value, String> {
    let mut rules = Vec::new();
    for page in 1.. {
        let url =
            format!("repos/{nwo}/rules/branches/{branch}?per_page={RULES_PAGE_SIZE}&page={page}");
        let text = gh(&["api", &url])?;
        // A page that is not a list was not read; it is never an empty one.
        let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(&text) else {
            return Err(format!("page {page} of the rules is not a JSON list"));
        };
        rules.extend_from_slice(&rows);
        if rows.len() < RULES_PAGE_SIZE {
            break;
        }
    }
    Ok(Value::Array(rules))
}

/// The classic protection of `branch`, or why it could not be read. Only
/// a 404 means the branch has none; any other failure, or a body that is
/// not an object, leaves it unknown.
fn classic_protection(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    nwo: &str,
    branch: &str,
) -> (Option<Value>, Option<String>) {
    match gh(&["api", &format!("repos/{nwo}/branches/{branch}/protection")]) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(body @ Value::Object(_)) => (Some(body), None),
            _ => (
                None,
                Some("classic branch protection not readable (not a JSON object)".to_string()),
            ),
        },
        Err(error) if error.contains("404") || error.contains("not protected") => (None, None),
        Err(error) => (
            None,
            Some(format!(
                "classic branch protection not readable ({})",
                super::first_line(&error)
            )),
        ),
    }
}

/// What lets a pull request merge on checks that did not run on the
/// branch's tip, one line each. A policy check, or the up-to-date
/// requirement, is unbound when every rule that requires it can be
/// bypassed; a bypass list that decides this and cannot be read is added
/// to `bypass_unread`.
fn problems(
    opts: &Options,
    nwo: &str,
    branch: &str,
    sources: &[Source],
    required: &[String],
    bypass_unread: &mut Vec<String>,
) -> Vec<String> {
    if sources.is_empty() {
        return vec![format!(
            "{branch} requires no status checks, so a pull request can merge without CI"
        )];
    }
    let mut problems = Vec::new();
    let strict: Vec<usize> = (0..sources.len()).filter(|&i| sources[i].strict).collect();
    if strict.is_empty() {
        problems.push(format!(
            "{branch} requires status checks but not that a branch be up to date ({}), so a pull request can merge on checks that ran against an older {branch}",
            sources
                .iter()
                .map(|s| s.label.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let missing: Vec<&str> = required
        .iter()
        .filter(|name| !sources.iter().any(|s| s.contexts.contains(name)))
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        problems.push(format!(
            "{branch} does not require {} named in git.required_checks",
            missing.join(", ")
        ));
    }

    // What each requirement rests on: the sources that list a policy name,
    // and the strict sources for the up-to-date rule.
    let mut needs: Vec<(Vec<usize>, String)> = required
        .iter()
        .map(|name| {
            let carriers = (0..sources.len())
                .filter(|&i| sources[i].contexts.contains(name))
                .collect();
            (carriers, name.clone())
        })
        .collect();
    needs.push((strict, "a branch to be up to date".to_string()));
    let mut bypass: Vec<Option<Bypass>> = (0..sources.len()).map(|_| None).collect();
    for (carriers, _) in &needs {
        for &i in carriers {
            if bypass[i].is_none() {
                bypass[i] = Some(bypass_of(opts, nwo, &sources[i]));
            }
        }
    }
    let state = |i: usize| bypass[i].as_ref().unwrap_or(&Bypass::Bound);

    // Unbound requirements, grouped by the bypassable rules they rest on.
    let mut unbound: Vec<(Vec<usize>, Vec<String>)> = Vec::new();
    let mut unknown: Vec<usize> = Vec::new();
    for (carriers, need) in needs {
        if carriers.is_empty() || carriers.iter().any(|&i| matches!(state(i), Bypass::Bound)) {
            continue;
        }
        let open: Vec<usize> = carriers
            .iter()
            .copied()
            .filter(|&i| matches!(state(i), Bypass::Unknown(_)))
            .collect();
        if !open.is_empty() {
            for i in open {
                if !unknown.contains(&i) {
                    unknown.push(i);
                }
            }
            continue;
        }
        match unbound.iter_mut().find(|(rules, _)| *rules == carriers) {
            Some((_, items)) => items.push(need),
            None => unbound.push((carriers, vec![need])),
        }
    }
    for (rules, items) in unbound {
        let how: Vec<&str> = rules
            .iter()
            .filter_map(|&i| match state(i) {
                Bypass::Bypassable(how) => Some(how.as_str()),
                _ => None,
            })
            .collect();
        problems.push(format!(
            "{}, and no rule that binds everyone requires {}",
            how.join(" and "),
            items.join(", ")
        ));
    }
    unknown.sort_unstable();
    for i in unknown {
        if let Bypass::Unknown(why) = state(i) {
            bypass_unread.push(why.clone());
        }
    }
    problems
}

/// Whether everyone is held to `source`: classic protection binds
/// administrators, and a ruleset has an empty bypass list.
fn bypass_of(opts: &Options, nwo: &str, source: &Source) -> Bypass {
    if source.classic {
        return match source.enforce_admins {
            Some(true) => Bypass::Bound,
            Some(false) => Bypass::Bypassable(format!(
                "{} does not bind administrators (enforce_admins is off)",
                source.label
            )),
            None => Bypass::Unknown(format!("{} enforce_admins not readable", source.label)),
        };
    }
    let Some(id) = source.ruleset_id else {
        return Bypass::Unknown(format!("{} bypass list not readable", source.label));
    };
    match opts.do_exec_bounded(
        "gh",
        &["api", &format!("repos/{nwo}/rulesets/{id}")],
        GH_TIMEOUT,
    ) {
        Ok(text) => {
            let ruleset = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
            match ruleset.get("bypass_actors").and_then(Value::as_array) {
                Some(actors) if !actors.is_empty() => Bypass::Bypassable(format!(
                    "{} lets {} actor(s) bypass it",
                    source.label,
                    actors.len()
                )),
                Some(_) => Bypass::Bound,
                None => Bypass::Unknown(format!("{} bypass list not readable", source.label)),
            }
        }
        Err(error) => Bypass::Unknown(format!(
            "{} not readable ({})",
            source.label,
            super::first_line(&error)
        )),
    }
}

/// Every rule that requires checks: each `required_status_checks` rule the
/// rulesets apply to the branch, and the classic protection's checks.
fn sources(rules: &Value, classic: Option<&Value>) -> Vec<Source> {
    let mut sources = Vec::new();
    for rule in rules.as_array().into_iter().flatten() {
        if rule.get("type").and_then(Value::as_str) != Some("required_status_checks") {
            continue;
        }
        let parameters = &rule["parameters"];
        let contexts = names(&parameters["required_status_checks"]);
        if contexts.is_empty() {
            continue;
        }
        let id = rule.get("ruleset_id").and_then(Value::as_u64);
        sources.push(Source {
            label: id.map_or_else(|| "a ruleset".to_string(), |id| format!("ruleset {id}")),
            strict: parameters
                .get("strict_required_status_checks_policy")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            contexts,
            ruleset_id: id,
            enforce_admins: None,
            classic: false,
        });
    }
    if let Some(checks) = classic
        .and_then(|c| c.get("required_status_checks"))
        .filter(|v| v.is_object())
    {
        let mut contexts: Vec<String> = checks
            .get("contexts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        for name in names(&checks["checks"]) {
            if !contexts.contains(&name) {
                contexts.push(name);
            }
        }
        if !contexts.is_empty() {
            sources.push(Source {
                label: "classic branch protection".into(),
                strict: checks
                    .get("strict")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                contexts,
                ruleset_id: None,
                enforce_admins: classic
                    .and_then(|c| c.pointer("/enforce_admins/enabled"))
                    .and_then(Value::as_bool),
                classic: true,
            });
        }
    }
    sources
}

/// The `context` of each check object in a list.
fn names(list: &Value) -> Vec<String> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|check| check.get("context").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// `owner/name` of the `origin` remote when it is on github.com.
fn github_origin(root: &Path) -> Option<String> {
    let repo = git2::Repository::discover(root).ok()?;
    let remote = repo.find_remote("origin").ok()?;
    parse_github_url(remote.url().ok()?)
}

/// `owner/name` from an HTTPS, SSH or scp-style github.com URL.
fn parse_github_url(url: &str) -> Option<String> {
    let rest = [
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
        "git@github.com:",
    ]
    .iter()
    .find_map(|prefix| url.strip_prefix(prefix))?;
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut parts = rest.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    (parts.next().is_none() && !owner.is_empty() && !name.is_empty())
        .then(|| format!("{owner}/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A project whose `origin` is `https://github.com/o/r.git`.
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        repo.remote("origin", "https://github.com/o/r.git").unwrap();
        dir
    }

    fn opts(dir: &Path, exec: fn(&str, &[&str]) -> Result<String, String>) -> Options {
        Options {
            project_dir: dir.to_string_lossy().into_owned(),
            look_path: Some(|_| Ok("gh".into())),
            exec_command: Some(exec),
            harness_home: Some(PathBuf::from("/nonexistent/codeflow-test-home")),
            env_var: Some(|_| None),
            ..Options::default()
        }
    }

    const SIX: &str = r#"[{"context":"codeflow gates","integration_id":15368},{"context":"commit standards","integration_id":15368},{"context":"release impact","integration_id":15368},{"context":"secret scan","integration_id":15368},{"context":"security review","integration_id":15368},{"context":"windows","integration_id":15368}]"#;

    /// The answers of a host whose default branch `main` carries `rules`
    /// (the body of `rules/branches/main`), `classic` protection (or 404)
    /// and ruleset 7 with `bypass` actors.
    fn host(
        args: &[&str],
        rules: &str,
        classic: Option<&str>,
        bypass: &str,
    ) -> Result<String, String> {
        match args {
            ["api", "repos/o/r"] => Ok(r#"{"default_branch":"main"}"#.into()),
            // The first page; a later page is empty unless a test serves one.
            ["api", url] if url.starts_with("repos/o/r/rules/branches/main") => {
                Ok(if url.ends_with("&page=1") || !url.contains("page=") {
                    rules.into()
                } else {
                    "[]".into()
                })
            }
            ["api", "repos/o/r/branches/main/protection"] => classic
                .map(str::to_string)
                .ok_or_else(|| "gh: Branch not protected (HTTP 404)".into()),
            // `bypass` is the actors array, `absent` for a body without one (a
            // token without write access), or `error` for a failed read.
            ["api", "repos/o/r/rulesets/7"] => match bypass {
                "absent" => Ok(r#"{"id":7}"#.into()),
                "error" => Err("gh: Resource not accessible (HTTP 403)".into()),
                actors => Ok(format!(r#"{{"id":7,"bypass_actors":{actors}}}"#)),
            },
            other => Err(format!("unexpected gh call {other:?}")),
        }
    }

    fn ruleset(strict: bool) -> String {
        format!(
            r#"[{{"type":"required_status_checks","parameters":{{"strict_required_status_checks_policy":{strict},"do_not_enforce_on_create":false,"required_status_checks":{SIX}}},"ruleset_source_type":"Repository","ruleset_source":"o/r","ruleset_id":7}}]"#
        )
    }

    fn warn_text(result: &CheckResult) -> String {
        match &result.status {
            Status::Warn(remedy) => format!("{} | {remedy}", result.message),
            other => panic!("expected a warning, got {other:?}: {}", result.message),
        }
    }

    #[test]
    fn strict_off_warns_with_the_setting_to_turn_on() {
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            host(args, &ruleset(false), None, "[]")
        }
        let dir = project();
        let text = warn_text(&check(&opts(dir.path(), exec)));
        assert!(
            text.contains("not that a branch be up to date (ruleset 7)"),
            "{text}"
        );
        assert!(
            text.contains("Require branches to be up to date before merging"),
            "{text}"
        );
        assert!(text.contains("codeflow remote protect"), "{text}");
    }

    #[test]
    fn no_required_checks_warns() {
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            host(args, "[]", Some(r#"{"required_status_checks":null}"#), "[]")
        }
        let dir = project();
        let text = warn_text(&check(&opts(dir.path(), exec)));
        assert!(text.contains("main requires no status checks"), "{text}");
    }

    #[test]
    fn a_policy_check_the_host_does_not_require_warns() {
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            host(args, &ruleset(true), None, "[]")
        }
        let dir = project();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        std::fs::write(
            dir.path().join(".codeflow/policy.json"),
            r#"{"git":{"required_checks":["codeflow gates","linux"]}}"#,
        )
        .unwrap();
        let text = warn_text(&check(&opts(dir.path(), exec)));
        assert!(
            text.contains("does not require linux named in git.required_checks"),
            "{text}"
        );
    }

    #[test]
    fn a_protection_admins_can_bypass_warns() {
        fn classic_exec(_: &str, args: &[&str]) -> Result<String, String> {
            host(
                args,
                "[]",
                Some(
                    r#"{"required_status_checks":{"strict":true,"contexts":["codeflow gates","secret scan","security review","commit standards"]},"enforce_admins":{"enabled":false}}"#,
                ),
                "[]",
            )
        }
        fn ruleset_exec(_: &str, args: &[&str]) -> Result<String, String> {
            host(
                args,
                &ruleset(true),
                None,
                r#"[{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"always"}]"#,
            )
        }
        let dir = project();
        let text = warn_text(&check(&opts(dir.path(), classic_exec)));
        assert!(
            text.contains("classic branch protection does not bind administrators"),
            "{text}"
        );
        let text = warn_text(&check(&opts(dir.path(), ruleset_exec)));
        assert!(
            text.contains("ruleset 7 lets 1 actor(s) bypass it"),
            "{text}"
        );
    }

    #[test]
    fn strict_checks_that_cover_the_policy_pass() {
        // The live shape of this repository's main once strict is on:
        // ruleset checks, and classic protection without checks or admin
        // enforcement, which carries no checks and so is not judged.
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            host(
                args,
                &ruleset(true),
                Some(
                    r#"{"required_status_checks":null,"enforce_admins":{"enabled":false},"required_conversation_resolution":{"enabled":true}}"#,
                ),
                "[]",
            )
        }
        let dir = project();
        let result = check(&opts(dir.path(), exec));
        assert_eq!(result.status, Status::Pass, "{}", result.message);
        assert!(
            result.message.contains("o/r main requires"),
            "{}",
            result.message
        );
    }

    #[test]
    fn a_bypass_list_that_cannot_be_read_is_a_note_not_a_pass() {
        fn absent(_: &str, args: &[&str]) -> Result<String, String> {
            host(args, &ruleset(true), None, "absent")
        }
        fn failed(_: &str, args: &[&str]) -> Result<String, String> {
            host(args, &ruleset(true), None, "error")
        }
        let dir = project();
        let result = check(&opts(dir.path(), absent));
        assert!(
            matches!(result.status, Status::Note(_)),
            "{:?}: {}",
            result.status,
            result.message
        );
        assert!(
            result
                .message
                .contains("ruleset 7 bypass list not readable"),
            "{}",
            result.message
        );
        let result = check(&opts(dir.path(), failed));
        assert!(
            matches!(result.status, Status::Note(_)),
            "{:?}: {}",
            result.status,
            result.message
        );
        assert!(
            result.message.contains("ruleset 7 not readable"),
            "{}",
            result.message
        );
    }

    #[test]
    fn an_unreadable_bypass_list_does_not_hide_a_warning() {
        // `linux` rests on classic protection alone, which administrators
        // can bypass; the other names rest on ruleset 7, whose bypass list
        // cannot be read.
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            host(
                args,
                &ruleset(true),
                Some(
                    r#"{"required_status_checks":{"strict":true,"contexts":["linux"]},"enforce_admins":{"enabled":false}}"#,
                ),
                "absent",
            )
        }
        let dir = project();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        std::fs::write(
            dir.path().join(".codeflow/policy.json"),
            r#"{"git":{"required_checks":["codeflow gates","secret scan","security review","commit standards","linux"]}}"#,
        )
        .unwrap();
        let text = warn_text(&check(&opts(dir.path(), exec)));
        assert!(
            text.contains("does not bind administrators (enforce_admins is off), and no rule that binds everyone requires linux"),
            "{text}"
        );
        assert!(
            text.contains("ruleset 7 bypass list not readable"),
            "{text}"
        );
    }

    /// The four policy names as ruleset check objects.
    const FOUR: &str = r#"[{"context":"codeflow gates","integration_id":15368},{"context":"secret scan","integration_id":15368},{"context":"security review","integration_id":15368},{"context":"commit standards","integration_id":15368}]"#;

    const ONE_ACTOR: &str =
        r#"[{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"always"}]"#;

    /// One `required_status_checks` rule of ruleset `id`.
    fn rule(id: u64, strict: bool, checks: &str) -> String {
        format!(
            r#"{{"type":"required_status_checks","parameters":{{"strict_required_status_checks_policy":{strict},"do_not_enforce_on_create":false,"required_status_checks":{checks}}},"ruleset_source_type":"Repository","ruleset_source":"o/r","ruleset_id":{id}}}"#
        )
    }

    /// A host with ruleset 1 strict on `lint` alone and no bypass, beside
    /// `other`: ruleset 2 non-strict on the four policy names with the
    /// bypass actors `bypass`, or classic protection when `classic` is set.
    fn split(args: &[&str], bypass: &str, classic: Option<&str>) -> Result<String, String> {
        let lint = r#"[{"context":"lint","integration_id":15368}]"#;
        let rules = if classic.is_some() {
            format!("[{}]", rule(1, true, lint))
        } else {
            format!("[{},{}]", rule(1, true, lint), rule(2, false, FOUR))
        };
        match args {
            ["api", "repos/o/r/rulesets/1"] => Ok(r#"{"id":1,"bypass_actors":[]}"#.into()),
            ["api", "repos/o/r/rulesets/2"] => {
                Ok(format!(r#"{{"id":2,"bypass_actors":{bypass}}}"#))
            }
            _ => host(args, &rules, classic, "[]"),
        }
    }

    /// Round 3 of the PR 127 review: the strict ruleset binds everyone but
    /// lists only `lint`; the policy names sit on a rule that one actor
    /// can bypass, so that actor merges past them.
    #[test]
    fn policy_checks_only_a_bypassable_rule_requires_warn() {
        fn ruleset_exec(_: &str, args: &[&str]) -> Result<String, String> {
            split(args, ONE_ACTOR, None)
        }
        fn classic_exec(_: &str, args: &[&str]) -> Result<String, String> {
            split(
                args,
                "[]",
                Some(
                    r#"{"required_status_checks":{"strict":false,"contexts":["codeflow gates","secret scan","security review","commit standards"]},"enforce_admins":{"enabled":false}}"#,
                ),
            )
        }
        let dir = project();
        let text = warn_text(&check(&opts(dir.path(), ruleset_exec)));
        assert!(
            text.contains("ruleset 2 lets 1 actor(s) bypass it, and no rule that binds everyone requires codeflow gates, secret scan, security review, commit standards"),
            "{text}"
        );
        let text = warn_text(&check(&opts(dir.path(), classic_exec)));
        assert!(
            text.contains("classic branch protection does not bind administrators (enforce_admins is off), and no rule that binds everyone requires codeflow gates"),
            "{text}"
        );
    }

    /// The same split with no bypass passes, and a bypassable non-strict
    /// rule does not warn while a rule that binds everyone lists the same
    /// names.
    #[test]
    fn a_split_that_binds_everyone_passes() {
        fn unbypassable(_: &str, args: &[&str]) -> Result<String, String> {
            split(args, "[]", None)
        }
        fn covered(_: &str, args: &[&str]) -> Result<String, String> {
            let rules = format!("[{},{}]", rule(1, true, FOUR), rule(2, false, FOUR));
            match args {
                ["api", "repos/o/r/rulesets/1"] => Ok(r#"{"id":1,"bypass_actors":[]}"#.into()),
                ["api", "repos/o/r/rulesets/2"] => {
                    Ok(format!(r#"{{"id":2,"bypass_actors":{ONE_ACTOR}}}"#))
                }
                _ => host(args, &rules, None, "[]"),
            }
        }
        let dir = project();
        for exec in [unbypassable as fn(&str, &[&str]) -> _, covered] {
            let result = check(&opts(dir.path(), exec));
            assert_eq!(result.status, Status::Pass, "{}", result.message);
        }
    }

    #[test]
    fn a_strict_rule_on_a_later_page_of_rules_still_passes() {
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            match args {
                ["api", url] if url.starts_with("repos/o/r/rules/branches/main") => {
                    if url.contains("page=2") {
                        Ok(ruleset(true))
                    } else {
                        let filler: Vec<Value> = (0..100)
                            .map(|_| serde_json::json!({"type": "deletion", "ruleset_id": 9}))
                            .collect();
                        Ok(serde_json::to_string(&filler).unwrap())
                    }
                }
                _ => host(args, "[]", None, "[]"),
            }
        }
        let dir = project();
        let result = check(&opts(dir.path(), exec));
        assert_eq!(result.status, Status::Pass, "{}", result.message);
    }

    /// Round 4 of the PR 127 review: a classic protection read that fails
    /// for a reason other than "not protected" is unknown, so it never
    /// becomes "requires no status checks".
    #[test]
    fn an_unreadable_classic_protection_is_a_note_not_a_warning() {
        fn denied(_: &str, args: &[&str]) -> Result<String, String> {
            match args {
                ["api", "repos/o/r/branches/main/protection"] => {
                    Err("gh: Resource not accessible (HTTP 403)".into())
                }
                _ => host(args, "[]", None, "[]"),
            }
        }
        fn garbled(_: &str, args: &[&str]) -> Result<String, String> {
            match args {
                ["api", "repos/o/r/branches/main/protection"] => Ok("<html>".into()),
                _ => host(args, "[]", None, "[]"),
            }
        }
        fn unprotected(_: &str, args: &[&str]) -> Result<String, String> {
            host(args, "[]", None, "[]")
        }
        let dir = project();
        for exec in [denied as fn(&str, &[&str]) -> _, garbled] {
            let result = check(&opts(dir.path(), exec));
            assert!(
                matches!(result.status, Status::Note(_)),
                "{:?}: {}",
                result.status,
                result.message
            );
            assert!(
                result
                    .message
                    .contains("classic branch protection not readable"),
                "{}",
                result.message
            );
            assert!(
                !result.message.contains("requires no status checks"),
                "{}",
                result.message
            );
        }
        // A real 404 is a branch without classic protection, and warns.
        let text = warn_text(&check(&opts(dir.path(), unprotected)));
        assert!(text.contains("main requires no status checks"), "{text}");
    }

    /// Classic protection cannot remove a ruleset requirement, so rulesets
    /// that meet the policy on their own still pass, naming the gap.
    #[test]
    fn rulesets_that_meet_the_policy_pass_with_classic_unread() {
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            match args {
                ["api", "repos/o/r/branches/main/protection"] => {
                    Err("gh: Resource not accessible (HTTP 403)".into())
                }
                _ => host(args, &ruleset(true), None, "[]"),
            }
        }
        let dir = project();
        let result = check(&opts(dir.path(), exec));
        assert_eq!(result.status, Status::Pass, "{}", result.message);
        assert!(
            result
                .message
                .contains("classic branch protection not readable"),
            "{}",
            result.message
        );
    }

    /// A rules page that is not a JSON array was not read, so it is a note,
    /// never an empty rule list.
    #[test]
    fn a_rules_page_that_is_not_a_list_is_a_note() {
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            match args {
                ["api", url] if url.starts_with("repos/o/r/rules/branches/main") => {
                    Ok(r#"{"message":"Bad credentials"}"#.into())
                }
                _ => host(args, "[]", None, "[]"),
            }
        }
        let dir = project();
        let result = check(&opts(dir.path(), exec));
        assert!(
            matches!(result.status, Status::Note(_)),
            "{:?}: {}",
            result.status,
            result.message
        );
        assert!(
            result.message.contains("could not read the rules"),
            "{}",
            result.message
        );
    }

    #[test]
    fn what_doctor_cannot_read_is_a_note() {
        fn offline(_: &str, _: &[&str]) -> Result<String, String> {
            Err("error connecting to api.github.com".into())
        }
        let is_note = |result: &CheckResult| matches!(result.status, Status::Note(_));
        // No GitHub origin.
        let plain = tempfile::tempdir().unwrap();
        git2::Repository::init(plain.path())
            .unwrap()
            .remote("origin", "https://gitlab.com/o/r.git")
            .unwrap();
        let result = check(&opts(plain.path(), offline));
        assert!(is_note(&result), "{result:?}");
        assert!(
            result.message.contains("no GitHub `origin`"),
            "{}",
            result.message
        );
        // No gh.
        let dir = project();
        let mut no_gh = opts(dir.path(), offline);
        no_gh.look_path = Some(|_| Err("not found".into()));
        let result = check(&no_gh);
        assert!(is_note(&result), "{result:?}");
        assert!(
            result.message.contains("`gh` not found"),
            "{}",
            result.message
        );
        // No network.
        let result = check(&opts(dir.path(), offline));
        assert!(is_note(&result), "{result:?}");
        assert!(
            result.message.contains("error connecting"),
            "{}",
            result.message
        );
    }

    #[test]
    fn github_urls_name_their_repository() {
        for url in [
            "https://github.com/o/r.git",
            "https://github.com/o/r",
            "git@github.com:o/r.git",
            "ssh://git@github.com/o/r.git",
        ] {
            assert_eq!(parse_github_url(url).as_deref(), Some("o/r"), "{url}");
        }
        for url in [
            "https://gitlab.com/o/r.git",
            "git@github.com:o",
            "/srv/r.git",
        ] {
            assert_eq!(parse_github_url(url), None, "{url}");
        }
    }
}
