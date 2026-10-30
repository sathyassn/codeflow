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
//! network or a GitHub `origin` is missing, the policy file does not parse
//! or the schema refuses `git.required_checks` (an empty list would
//! otherwise name nothing to find missing), or the rules, the classic
//! protection or a bypass list that decides the answer cannot be read (the
//! host omits a bypass list without write access), it says so in a note
//! rather than passing or warning. A schema error on another key is named
//! beside the answer. Only a 404 means no classic protection.
//!
//! It also judges the perimeter `codeflow remote protect` writes (TSK-1080):
//! it **fails** while the bypass list of "codeflow authority checks" (B) or
//! "codeflow line checks" (C) is not empty, since that is a recovery window
//! left open; with `--profile` it compares the owned rulesets with the
//! profile; and it reports classic protection that does not bind
//! administrators, a non-strict or pull-request-free rule on the
//! integration lines, an `authority:` check pinned to GitHub Actions, an
//! authority environment that admits more than the default branch or that
//! a second job references, and a foreign ruleset that requires a name no
//! workflow on the default branch produces.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{CheckResult, Options, Status};
use crate::remedy;
use crate::remote::{
    self, Profile, Targeting, AUTHORITY_PREFIX, DEFAULT_LINES, GITHUB_ACTIONS_APP_ID,
    POLICY_WORKFLOW, RULESET_AUTHORITY_CHECKS, RULESET_AUTHORITY_UPDATES, RULESET_LINE_CHECKS,
};

/// The environment that alone holds the authority App's key.
const AUTHORITY_ENVIRONMENT: &str = "codeflow-authority";

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
    let root = Path::new(&opts.project_dir);
    let policy = policy_list(root);
    let result = |status: Status, message: String| CheckResult {
        name: NAME.into(),
        status,
        message: format!("{message}{}", policy.as_ref().map_or("", |(_, also)| also)),
        duration: start.elapsed(),
    };
    let note =
        |message: String| result(Status::Note(remedy::DOCTOR_REMOTE_UNREAD.remedy()), message);

    let nwo = match origin_or_why(root) {
        Ok(nwo) => nwo,
        Err(why) => return note(why),
    };
    if opts.do_look_path("gh").is_err() {
        return note(format!(
            "`gh` not found, so doctor cannot read the host rules of {nwo}"
        ));
    }
    let required = match &policy {
        Ok((required, _)) => required,
        Err(why) => return note(why.clone()),
    };
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
    let problems = problems(opts, &nwo, &branch, &sources, required, &mut bypass_unread);
    let profile = match opts.remote_profile.as_deref().map(Profile::parse) {
        None => None,
        Some(Ok(profile)) => Some(profile),
        Some(Err(why)) => return note(why),
    };
    let extra = perimeter(&gh, &nwo, &branch, classic.as_ref(), profile, &problems);
    let base = base_answer(
        &nwo,
        &branch,
        &sources,
        &problems,
        classic_unread.as_ref(),
        &bypass_unread,
    );

    let mut message = match &base {
        Answer::Pass(m) | Answer::Warn(m) | Answer::Note(m) => m.clone(),
    };
    for part in extra.warns.iter().chain(&extra.unread) {
        message = format!("{message}; {part}");
    }
    if !extra.fails.is_empty() {
        return result(
            Status::Fail,
            format!("{nwo}: {}; {message}", extra.fails.join("; ")),
        );
    }
    let warn = || {
        result(
            Status::Warn(remedy::DOCTOR_REMOTE_PERIMETER.with(&[("branch", &branch)])),
            message.clone(),
        )
    };
    match base {
        Answer::Warn(_) => warn(),
        _ if !extra.warns.is_empty() => warn(),
        Answer::Note(_) => note(message.clone()),
        Answer::Pass(_) if !extra.unread.is_empty() => note(message.clone()),
        Answer::Pass(_) => result(Status::Pass, message.clone()),
    }
}

/// The answer from the default branch's required checks alone.
enum Answer {
    Pass(String),
    Warn(String),
    Note(String),
}

fn base_answer(
    nwo: &str,
    branch: &str,
    sources: &[Source],
    problems: &[String],
    classic_unread: Option<&String>,
    bypass_unread: &[String],
) -> Answer {
    let mut unread: Vec<String> = classic_unread.into_iter().cloned().collect();
    unread.extend(bypass_unread.iter().cloned());

    if let (Some(why), false) = (classic_unread, problems.is_empty()) {
        // Classic protection can add checks, the up-to-date rule or an
        // administrator binding, so what the rulesets lack alone is not a
        // gap doctor can report.
        let rest = if bypass_unread.is_empty() {
            String::new()
        } else {
            format!("; {}", bypass_unread.join("; "))
        };
        return Answer::Note(format!(
            "doctor cannot judge {nwo} {branch}: {why}, and the rulesets alone do not require every name in git.required_checks on an up-to-date branch for everyone{rest}"
        ));
    }
    if problems.is_empty() && !bypass_unread.is_empty() {
        // The rules look right, but a bypass list that cannot be read could
        // hold an actor who skips them, so this is not a pass.
        return Answer::Note(format!(
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
        Answer::Pass(format!(
            "{nwo} {branch} requires {} on an up-to-date branch, including every name in git.required_checks{unread}",
            checks.join(", ")
        ))
    } else {
        Answer::Warn(format!("{nwo}: {}{unread}", problems.join("; ")))
    }
}

/// What the perimeter `remote protect` writes adds to the answer: failures
/// (an open recovery window), warnings and what could not be read.
#[derive(Default)]
struct Perimeter {
    fails: Vec<String>,
    warns: Vec<String>,
    unread: Vec<String>,
}

/// Judge the owned rulesets, the line rules, the authority environment and
/// the rulesets that require names nothing produces.
fn perimeter(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    nwo: &str,
    branch: &str,
    classic: Option<&Value>,
    profile: Option<Profile>,
    problems: &[String],
) -> Perimeter {
    let mut out = Perimeter::default();
    if classic
        .and_then(|c| c.pointer("/enforce_admins/enabled"))
        .and_then(Value::as_bool)
        == Some(false)
        && !problems.iter().any(|p| p.contains("enforce_admins is off"))
    {
        out.warns.push(format!(
            "classic branch protection on {branch} does not bind administrators (enforce_admins is off)"
        ));
    }
    let rulesets = match all_rulesets(gh, nwo) {
        Ok(rulesets) => rulesets,
        Err(why) => {
            out.unread.push(why);
            return out;
        }
    };
    let named = |name: &str| rulesets.iter().find(|r| r["name"] == name);

    open_windows(&rulesets, &mut out);

    if let Some(profile) = profile {
        profile_gaps(&rulesets, profile, &mut out);
    }

    // The line pattern is C's, else the default.
    let lines = named(RULESET_LINE_CHECKS)
        .and_then(|c| c.pointer("/conditions/ref_name/include/0"))
        .and_then(Value::as_str)
        .and_then(|include| include.strip_prefix("refs/heads/"))
        .unwrap_or(DEFAULT_LINES)
        .to_string();
    line_rules(&rulesets, &lines, profile.is_some(), &mut out);

    loose_authority_pins(&rulesets, &mut out);
    declared_findings(gh, nwo, branch, &rulesets, &mut out);
    out
}

/// An open recovery window: B or C names a bypass actor.
fn open_windows(rulesets: &[Value], out: &mut Perimeter) {
    let named = |name: &str| rulesets.iter().find(|r| r["name"] == name);
    for name in [RULESET_AUTHORITY_CHECKS, RULESET_LINE_CHECKS] {
        let Some(ruleset) = named(name) else { continue };
        let label = remote::ruleset_label(ruleset);
        match ruleset.get("bypass_actors").and_then(Value::as_array) {
            Some(actors) if !actors.is_empty() => out.fails.push(format!(
                "ruleset {label} lets {} bypass it, so a recovery window is open since {}; close it: codeflow remote protect",
                actors_text(actors),
                ruleset
                    .get("updated_at")
                    .and_then(Value::as_str)
                    .unwrap_or("an unknown time")
            )),
            Some(_) => {}
            None => out
                .unread
                .push(format!("ruleset {label} bypass list not readable")),
        }
    }
}

/// An `authority:` name accepted from GitHub Actions or from anyone.
fn loose_authority_pins(rulesets: &[Value], out: &mut Perimeter) {
    for ruleset in rulesets.iter().filter(|r| r["enforcement"] == "active") {
        let loose: Vec<&str> = status_checks(ruleset)
            .filter(|check| {
                check["context"]
                    .as_str()
                    .is_some_and(|c| c.starts_with(AUTHORITY_PREFIX))
                    && check["integration_id"]
                        .as_u64()
                        .is_none_or(|app| app == GITHUB_ACTIONS_APP_ID)
            })
            .filter_map(|check| check["context"].as_str())
            .collect();
        if !loose.is_empty() {
            out.warns.push(format!(
                "ruleset {} accepts {} from GitHub Actions or any source, which any workflow can produce; pin it to the authority App with `codeflow remote protect --check-app <id>`",
                remote::ruleset_label(ruleset),
                loose.join(", ")
            ));
        }
    }
}

/// What the authority environment and the default branch's workflows
/// show: a second reference to the environment, and a foreign ruleset that
/// requires a name nothing produces.
fn declared_findings(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    nwo: &str,
    branch: &str,
    rulesets: &[Value],
    out: &mut Perimeter,
) {
    let environment = authority_environment(gh, nwo, branch, out);
    match remote::read_declared(gh, nwo, branch) {
        Err(why) => out.unread.push(why),
        Ok(declared) => {
            if environment {
                let references: Vec<String> = declared
                    .environments
                    .iter()
                    .filter(|(_, _, env)| env == AUTHORITY_ENVIRONMENT || env.contains("${{"))
                    .map(|(file, job, _)| format!("{file} job {job}"))
                    .collect();
                let outside = declared.environments.iter().any(|(file, _, env)| {
                    (env == AUTHORITY_ENVIRONMENT || env.contains("${{")) && file != POLICY_WORKFLOW
                });
                if references.len() > 1 || outside {
                    out.warns.push(format!(
                        "the environment {AUTHORITY_ENVIRONMENT} must be referenced only by the authority job in {POLICY_WORKFLOW}, but on {branch} it is referenced by {}",
                        references.join(", ")
                    ));
                }
            }
            for ruleset in rulesets.iter().filter(|r| {
                r["enforcement"] == "active"
                    && !r["name"].as_str().is_some_and(|name| {
                        [
                            RULESET_AUTHORITY_UPDATES,
                            RULESET_AUTHORITY_CHECKS,
                            RULESET_LINE_CHECKS,
                        ]
                        .contains(&name)
                    })
            }) {
                let dead: Vec<&str> = status_checks(ruleset)
                    .filter_map(|check| check["context"].as_str())
                    .filter(|name| !declared.produces(name))
                    .collect();
                if !dead.is_empty() {
                    out.warns.push(format!(
                        "ruleset {} requires {}, which no workflow on {branch} produces, so every pull request it covers is blocked; run `codeflow remote protect`, then retire that ruleset on the host",
                        remote::ruleset_label(ruleset),
                        dead.join(", ")
                    ));
                }
            }
        }
    }
}

/// Every branch ruleset of the repository, read in full, or why not.
fn all_rulesets(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    nwo: &str,
) -> Result<Vec<Value>, String> {
    let unread = |error: &str| {
        format!(
            "the rulesets of {nwo} are not readable ({})",
            super::first_line(error)
        )
    };
    let mut rulesets = Vec::new();
    for page in 1.. {
        let text = gh(&[
            "api",
            &format!("repos/{nwo}/rulesets?per_page={RULES_PAGE_SIZE}&page={page}"),
        ])
        .map_err(|e| unread(&e))?;
        let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(&text) else {
            return Err(unread(&format!("page {page} is not a JSON list")));
        };
        for row in &rows {
            let ours = row["target"] == "branch"
                && row
                    .get("source_type")
                    .and_then(Value::as_str)
                    .is_none_or(|source| source == "Repository");
            let Some(id) = row.get("id").and_then(Value::as_u64).filter(|_| ours) else {
                continue;
            };
            let text =
                gh(&["api", &format!("repos/{nwo}/rulesets/{id}")]).map_err(|e| unread(&e))?;
            let full: Value = serde_json::from_str(&text)
                .map_err(|_| unread(&format!("ruleset {id} is not JSON")))?;
            rulesets.push(full);
        }
        if rows.len() < RULES_PAGE_SIZE {
            break;
        }
    }
    Ok(rulesets)
}

/// The check objects of a ruleset's `required_status_checks` rules.
fn status_checks(ruleset: &Value) -> impl Iterator<Item = &Value> {
    rules_of(ruleset, "required_status_checks")
        .filter_map(|rule| rule["parameters"]["required_status_checks"].as_array())
        .flatten()
}

fn rules_of<'a>(ruleset: &'a Value, kind: &'a str) -> impl Iterator<Item = &'a Value> {
    ruleset["rules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(move |rule| rule["type"] == kind)
}

/// `RepositoryRole 5 (always), Team 9 (pull_request)`.
fn actors_text(actors: &[Value]) -> String {
    actors
        .iter()
        .map(|actor| {
            format!(
                "{} {} ({})",
                actor["actor_type"].as_str().unwrap_or("actor"),
                actor["actor_id"]
                    .as_u64()
                    .map_or_else(|| "-".to_string(), |id| id.to_string()),
                actor["bypass_mode"].as_str().unwrap_or("always")
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// How the owned rulesets differ from `profile`.
fn profile_gaps(rulesets: &[Value], profile: Profile, out: &mut Perimeter) {
    let mut gaps = Vec::new();
    for name in [
        RULESET_AUTHORITY_UPDATES,
        RULESET_AUTHORITY_CHECKS,
        RULESET_LINE_CHECKS,
    ] {
        let Some(ruleset) = rulesets.iter().find(|r| r["name"] == name) else {
            gaps.push(format!("ruleset '{name}' is missing"));
            continue;
        };
        let label = remote::ruleset_label(ruleset);
        let mut lacks = Vec::new();
        if ruleset["enforcement"] != "active" {
            lacks.push("it is not active".to_string());
        }
        let include = ruleset
            .pointer("/conditions/ref_name/include")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if name != RULESET_LINE_CHECKS && !include.contains(&Value::from("~DEFAULT_BRANCH")) {
            lacks.push("it does not target ~DEFAULT_BRANCH".to_string());
        }
        if name == RULESET_AUTHORITY_UPDATES {
            lacks.extend(update_gaps(ruleset, profile, out));
        } else {
            lacks.extend(checks_gaps(ruleset, name, profile));
        }
        if !lacks.is_empty() {
            gaps.push(format!("ruleset {label}: {}", lacks.join(", ")));
        }
    }
    if !gaps.is_empty() {
        out.warns.push(format!(
            "the owned rulesets differ from the {} profile ({}); run `codeflow remote protect --profile {}`",
            profile.as_str(),
            gaps.join("; "),
            profile.as_str()
        ));
    }
}

/// How ruleset A differs from `profile`.
fn update_gaps(ruleset: &Value, profile: Profile, out: &mut Perimeter) -> Vec<String> {
    let mut lacks = Vec::new();
    match rules_of(ruleset, "update").next() {
        None => lacks.push("it has no update rule".to_string()),
        Some(rule) if rule["parameters"]["update_allows_fetch_and_merge"] != false => {
            lacks.push("its update rule allows fetch and merge".to_string());
        }
        Some(_) => {}
    }
    match ruleset.get("bypass_actors").and_then(Value::as_array) {
        None => out.unread.push(format!(
            "ruleset {} bypass list not readable",
            remote::ruleset_label(ruleset)
        )),
        Some(actors) => {
            let (id, role) = profile.update_role();
            let exact = actors.len() == 1
                && actors[0]["actor_type"] == "RepositoryRole"
                && actors[0]["actor_id"] == id
                && actors[0]["bypass_mode"] == "always";
            if !exact {
                lacks.push(format!(
                    "its bypass list is {} where {} expects the {role} role (RepositoryRole {id}) alone",
                    if actors.is_empty() {
                        "empty".to_string()
                    } else {
                        actors_text(actors)
                    },
                    profile.as_str()
                ));
            }
        }
    }
    lacks
}

/// How ruleset B or C differs from `profile`.
fn checks_gaps(ruleset: &Value, name: &str, profile: Profile) -> Vec<String> {
    let mut lacks = Vec::new();
    let approvals = if name == RULESET_AUTHORITY_CHECKS {
        profile.approvals()
    } else {
        0
    };
    match rules_of(ruleset, "pull_request").next() {
        None => lacks.push("it requires no pull request".to_string()),
        Some(rule) => {
            let p = &rule["parameters"];
            if p["required_approving_review_count"].as_u64().unwrap_or(0) < approvals {
                lacks.push(format!("it requires fewer than {approvals} approval(s)"));
            }
            if name == RULESET_AUTHORITY_CHECKS
                && profile == Profile::IndependentReview
                && (p["dismiss_stale_reviews_on_push"] != true
                    || p["require_last_push_approval"] != true)
            {
                lacks.push(
                    "it does not dismiss stale approvals and require the last push approved"
                        .to_string(),
                );
            }
        }
    }
    let checks: Vec<&Value> = status_checks(ruleset).collect();
    if checks.is_empty() {
        lacks.push("it requires no status checks".to_string());
    } else if rules_of(ruleset, "required_status_checks")
        .any(|rule| rule["parameters"]["strict_required_status_checks_policy"] != true)
    {
        lacks.push("its checks are not strict".to_string());
    }
    let unpinned: Vec<&str> = checks
        .iter()
        .filter(|check| check["integration_id"].as_u64().is_none())
        .filter_map(|check| check["context"].as_str())
        .collect();
    if !unpinned.is_empty() {
        lacks.push(format!(
            "it accepts {} from any source",
            unpinned.join(", ")
        ));
    }
    for kind in ["non_fast_forward", "deletion"] {
        if rules_of(ruleset, kind).next().is_none() {
            lacks.push(format!("it has no {kind} rule"));
        }
    }
    if rules_of(ruleset, "update").next().is_some() {
        lacks.push("it carries an update rule".to_string());
    }
    lacks
}

/// The rules on the integration lines: a non-strict check rule, and no
/// rule that requires a pull request.
fn line_rules(rulesets: &[Value], lines: &str, asked: bool, out: &mut Perimeter) {
    let mut targeting = Vec::new();
    for ruleset in rulesets.iter().filter(|r| r["enforcement"] == "active") {
        match remote::ruleset_targets(ruleset, lines, None) {
            Targeting::Targets => targeting.push(ruleset),
            Targeting::Misses => {}
            Targeting::Unexplained(why) => out
                .unread
                .push(format!("ruleset {}: {why}", remote::ruleset_label(ruleset))),
        }
    }
    for ruleset in &targeting {
        if rules_of(ruleset, "required_status_checks")
            .any(|rule| rule["parameters"]["strict_required_status_checks_policy"] != true)
        {
            out.warns.push(format!(
                "ruleset {} requires checks on {lines} but not that a branch be up to date, so a line pull request can merge on checks that ran against an older line",
                remote::ruleset_label(ruleset)
            ));
        }
    }
    if (asked || !targeting.is_empty())
        && !targeting
            .iter()
            .any(|ruleset| rules_of(ruleset, "pull_request").next().is_some())
    {
        out.warns.push(format!(
            "no active ruleset requires a pull request on {lines}, so a push can move a line without any check"
        ));
    }
}

/// Whether the authority environment exists, warning when its branch rule
/// admits more than the default branch.
fn authority_environment(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    nwo: &str,
    branch: &str,
    out: &mut Perimeter,
) -> bool {
    let unread = |error: &str| {
        format!(
            "the environment {AUTHORITY_ENVIRONMENT} is not readable ({})",
            super::first_line(error)
        )
    };
    let environment = match gh(&[
        "api",
        &format!("repos/{nwo}/environments/{AUTHORITY_ENVIRONMENT}"),
    ]) {
        Ok(text) => serde_json::from_str::<Value>(&text).unwrap_or(Value::Null),
        Err(error) if error.contains("404") || error.contains("Not Found") => return false,
        Err(error) => {
            out.unread.push(unread(&error));
            return true;
        }
    };
    let admits = |what: String| {
        format!(
            "the environment {AUTHORITY_ENVIRONMENT} admits {what}, so code from a branch other than {branch} can read the authority key; set its deployment branches to {branch} alone"
        )
    };
    let policy = &environment["deployment_branch_policy"];
    if policy.is_null() {
        out.warns.push(admits("every branch".to_string()));
    } else if policy["protected_branches"] == true {
        out.warns.push(admits("every protected branch".to_string()));
    } else if policy["custom_branch_policies"] == true {
        match gh(&[
            "api",
            &format!("repos/{nwo}/environments/{AUTHORITY_ENVIRONMENT}/deployment-branch-policies"),
        ]) {
            Err(error) => out.unread.push(unread(&error)),
            Ok(text) => {
                let policies: Vec<(String, String)> = serde_json::from_str::<Value>(&text)
                    .unwrap_or(Value::Null)["branch_policies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|p| {
                        (
                            p["name"].as_str().unwrap_or("?").to_string(),
                            p["type"].as_str().unwrap_or("branch").to_string(),
                        )
                    })
                    .collect();
                if policies != [(branch.to_string(), "branch".to_string())] {
                    out.warns.push(admits(format!(
                        "the patterns {}",
                        policies
                            .iter()
                            .map(|(name, kind)| format!("{name} ({kind})"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
            }
        }
    }
    true
}

/// The `git.required_checks` list to judge, with a suffix naming the
/// policy's other schema errors (empty when there are none), or the note
/// for a list that cannot be trusted. It cannot when the file does not
/// parse (enforcement refuses it, so no list is in force), when the schema
/// refuses the key itself (an empty list names no check to find missing and
/// would pass whenever some other strict rule binds everyone), or when an
/// unknown key looks like a misspelt `required_checks` (the intended list
/// never loads). A schema error elsewhere leaves the loaded list usable, so
/// it is judged and the error named rather than hiding the host's answer.
fn policy_list(root: &Path) -> Result<(Vec<String>, String), String> {
    use crate::hooks::policy::{Policy, PolicySource};
    use crate::hooks::policy_schema::{spec_for, validate_policy, PolicyError};

    let errors = validate_policy(root).err().unwrap_or_default();
    let text = |errors: &[&PolicyError]| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    };
    let untrusted = |why: String| {
        Err(format!(
            "doctor cannot compare the host rules with git.required_checks: {why} (see `codeflow policy explain`)"
        ))
    };
    let all: Vec<_> = errors.iter().collect();
    // `policy.json` is the key of a file-level error: unreadable, not JSON,
    // not an object, or refused by the loader's own deserialize.
    if Policy::source(root) == PolicySource::MalformedFile
        || errors.iter().any(|e| e.key == "policy.json")
    {
        return untrusted(format!(
            "the policy file does not parse, so enforcement refuses it and no list is in force: {}",
            text(&all)
        ));
    }
    let misspelt = |key: &str| {
        let leaf = key.rsplit('.').next().unwrap_or(key).to_ascii_lowercase();
        spec_for(key).is_none() && leaf.contains("check") && !leaf.contains("checkout")
    };
    let list_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.key == "git.required_checks" || misspelt(&e.key))
        .collect();
    if !list_errors.is_empty() {
        return untrusted(text(&list_errors));
    }
    // An unreadable policy is named, never read as the defaults (issue 79).
    let required = match Policy::load(root) {
        Ok(policy) => policy.git.required_checks,
        Err(why) => return untrusted(why),
    };
    if required.is_empty() || required.iter().any(|name| name.trim().is_empty()) {
        return untrusted("git.required_checks names no usable check".into());
    }
    let also = if all.is_empty() {
        String::new()
    } else {
        format!(
            "; the policy also fails the schema: {} (see `codeflow policy explain`)",
            text(&all)
        )
    };
    Ok((required, also))
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

/// `owner/name` of the GitHub `origin`, or the note that says why doctor
/// reads no host rules.
fn origin_or_why(root: &Path) -> Result<String, String> {
    match github_origin(root) {
        Ok(Some(nwo)) => Ok(nwo),
        Ok(None) => Err(
            "no GitHub `origin` remote, so doctor reads no host rules for the default branch"
                .into(),
        ),
        Err(why) => Err(format!(
            "{why}, so doctor reads no host rules for the default branch"
        )),
    }
}

/// `owner/name` of the `origin` remote when it is on github.com.
///
/// OS text rule (issue 79): only a proven missing repository or `origin` is
/// none. An unreadable repository or an `origin` URL that is not UTF-8 is
/// named, never read as no remote.
fn github_origin(root: &Path) -> Result<Option<String>, String> {
    let Some(repo) = crate::hooks::repo::open(root)? else {
        return Ok(None);
    };
    let remote = match repo.find_remote("origin") {
        Ok(remote) => remote,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot read the `origin` remote: {}",
                error.message()
            ))
        }
    };
    let url = remote
        .url()
        .map_err(|error| format!("cannot read the `origin` URL: {}", error.message()))?;
    Ok(parse_github_url(url))
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
    use std::sync::atomic::{AtomicUsize, Ordering};

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
            // No repository ruleset beyond those `rules` names, no
            // authority environment and no workflow folder.
            ["api", url] if url.starts_with("repos/o/r/rulesets?") => Ok("[]".into()),
            ["api", "repos/o/r/environments/codeflow-authority"] => {
                Err("gh: Not Found (HTTP 404)".into())
            }
            ["api", url] if url.starts_with("repos/o/r/contents/") => {
                Err("gh: Not Found (HTTP 404)".into())
            }
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

    /// Write `text` as the project's policy file.
    fn write_policy(dir: &Path, text: &str) {
        std::fs::create_dir_all(dir.join(".codeflow")).unwrap();
        std::fs::write(dir.join(".codeflow/policy.json"), text).unwrap();
    }

    #[test]
    fn an_untrusted_required_checks_list_notes_before_any_host_call() {
        // The host strictly requires one other check for everyone. An empty
        // list would leave no policy name to find missing, a file that does
        // not parse is refused by enforcement, and a misnamed key loads the
        // defaults in place of the intended list, so each must stop the
        // check before `gh` is asked anything instead of passing it.
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            let lint = r#"[{"context":"lint","integration_id":15368}]"#;
            host(args, &format!("[{}]", rule(7, true, lint)), None, "[]")
        }
        for (policy, why) in [
            (r#"{"git":{"required_checks":[]}}"#, "git.required_checks"),
            (
                r#"{"git":{"required_checks":[" "]}}"#,
                "git.required_checks",
            ),
            (
                r#"{"git":{"required_checks":"lint"}}"#,
                "git.required_checks",
            ),
            (
                r#"{"git":{"required_check":["lint"]}}"#,
                "git.required_check",
            ),
            ("not json", "does not parse"),
        ] {
            let dir = project();
            write_policy(dir.path(), policy);
            let result = check(&opts(dir.path(), exec));
            assert!(
                matches!(result.status, Status::Note(_)),
                "{policy}: {:?}: {}",
                result.status,
                result.message
            );
            assert!(result.message.contains(why), "{policy}: {}", result.message);
        }
        assert_eq!(CALLS.load(Ordering::SeqCst), 0, "gh was called");
    }

    #[test]
    fn another_schema_error_still_judges_the_loaded_list() {
        // The file deserializes and the list it loads is usable, so a schema
        // error elsewhere is named but does not hide the host's answer.
        fn strict(_: &str, args: &[&str]) -> Result<String, String> {
            host(args, &ruleset(true), None, "[]")
        }
        let dir = project();
        write_policy(dir.path(), r#"{"git":{"nope":1}}"#);
        let result = check(&opts(dir.path(), strict));
        assert_eq!(result.status, Status::Pass, "{}", result.message);
        assert!(result.message.contains("git.nope"), "{}", result.message);
    }

    #[test]
    fn another_schema_error_keeps_the_up_to_date_warning() {
        fn loose(_: &str, args: &[&str]) -> Result<String, String> {
            host(args, &ruleset(false), None, "[]")
        }
        let dir = project();
        write_policy(
            dir.path(),
            r#"{"git":{"required_checks":["codeflow gates"],"commit_ticket_pattern":"("}}"#,
        );
        let text = warn_text(&check(&opts(dir.path(), loose)));
        assert!(
            text.contains("not that a branch be up to date (ruleset 7)"),
            "{text}"
        );
        assert!(text.contains("git.commit_ticket_pattern"), "{text}");
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
        // Ruleset checks, and classic protection without checks that binds
        // administrators.
        fn exec(_: &str, args: &[&str]) -> Result<String, String> {
            host(
                args,
                &ruleset(true),
                Some(
                    r#"{"required_status_checks":null,"enforce_admins":{"enabled":true},"required_conversation_resolution":{"enabled":true}}"#,
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

    /// TSK-1080 AC-3: classic protection that does not bind administrators
    /// is reported even when it carries no checks (this repository's live
    /// `main` on 2026-10-29).
    #[test]
    fn classic_protection_that_does_not_bind_administrators_warns() {
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
        let text = warn_text(&check(&opts(dir.path(), exec)));
        assert!(
            text.contains(
                "classic branch protection on main does not bind administrators (enforce_admins is off)"
            ),
            "{text}"
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

    // -------------------------------------------------------------------
    // The perimeter `remote protect` writes (TSK-1080 AC-3), on a recorded
    // host held per test thread.
    // -------------------------------------------------------------------

    #[derive(Clone, Default)]
    struct World {
        rulesets: Vec<Value>,
        classic: Option<String>,
        environment: Option<Value>,
        branch_policies: Option<Value>,
        workflows: Vec<(&'static str, String)>,
    }

    thread_local! {
        static WORLD: std::cell::RefCell<World> = std::cell::RefCell::new(World::default());
    }

    fn world_exec(_: &str, args: &[&str]) -> Result<String, String> {
        WORLD.with(|world| {
            let world = world.borrow();
            let not_found = || Err("gh: Not Found (HTTP 404)".to_string());
            match args {
                ["api", "repos/o/r"] => Ok(r#"{"default_branch":"main"}"#.into()),
                ["api", url] if url.starts_with("repos/o/r/rules/branches/main") => {
                    if !url.ends_with("&page=1") {
                        return Ok("[]".into());
                    }
                    let rules: Vec<Value> = world
                        .rulesets
                        .iter()
                        .filter(|r| {
                            r["enforcement"] == "active"
                                && r["conditions"]["ref_name"]["include"]
                                    .as_array()
                                    .unwrap()
                                    .contains(&Value::from("~DEFAULT_BRANCH"))
                        })
                        .flat_map(|r| {
                            r["rules"].as_array().unwrap().iter().map(move |rule| {
                                let mut rule = rule.clone();
                                rule["ruleset_id"] = r["id"].clone();
                                rule
                            })
                        })
                        .collect();
                    Ok(Value::Array(rules).to_string())
                }
                ["api", "repos/o/r/branches/main/protection"] => {
                    world.classic.clone().map_or_else(not_found, Ok)
                }
                ["api", url] if url.starts_with("repos/o/r/rulesets?") => Ok(Value::Array(
                    world
                        .rulesets
                        .iter()
                        .map(|r| {
                            serde_json::json!({
                                "id": r["id"], "name": r["name"], "target": "branch",
                                "source_type": "Repository"
                            })
                        })
                        .collect(),
                )
                .to_string()),
                ["api", url] if url.starts_with("repos/o/r/rulesets/") => {
                    let id: u64 = url["repos/o/r/rulesets/".len()..].parse().unwrap();
                    world
                        .rulesets
                        .iter()
                        .find(|r| r["id"] == id)
                        .map_or_else(not_found, |r| Ok(r.to_string()))
                }
                ["api", "repos/o/r/environments/codeflow-authority"] => world
                    .environment
                    .as_ref()
                    .map_or_else(not_found, |e| Ok(e.to_string())),
                ["api", "repos/o/r/environments/codeflow-authority/deployment-branch-policies"] => {
                    world
                        .branch_policies
                        .as_ref()
                        .map_or_else(not_found, |p| Ok(p.to_string()))
                }
                ["api", "repos/o/r/contents/.github/workflows?ref=main"] => {
                    if world.workflows.is_empty() {
                        return not_found();
                    }
                    Ok(Value::Array(
                        world
                            .workflows
                            .iter()
                            .map(|(path, _)| serde_json::json!({ "path": path, "type": "file" }))
                            .collect(),
                    )
                    .to_string())
                }
                ["api", "-H", _, url] => world
                    .workflows
                    .iter()
                    .find(|(path, _)| *url == format!("repos/o/r/contents/{path}?ref=main"))
                    .map_or_else(not_found, |(_, text)| Ok(text.clone())),
                other => Err(format!("unexpected gh call {other:?}")),
            }
        })
    }

    const POLICY_WORKFLOW_TEXT: &str = r#"
on: pull_request_target
jobs:
  commit-standards:
    name: 'trusted: commit standards'
  secret-scan:
    name: 'trusted: secret scan'
  authority:
    environment: codeflow-authority
    steps:
      - run: "create 'authority: commit standards' 'authority: secret scan'"
"#;

    const CI_WORKFLOW_TEXT: &str = "
on: pull_request
jobs:
  gates:
    name: codeflow gates
";

    const CI_WORKFLOW_PATH: &str = ".github/workflows/codeflow-ci.yml";

    fn checks(names: &[(&str, u64)]) -> Value {
        Value::Array(
            names
                .iter()
                .map(|(name, app)| serde_json::json!({ "context": name, "integration_id": app }))
                .collect(),
        )
    }

    /// One owned ruleset with a pull request and strict checks.
    fn checked(id: u64, name: &str, include: &str, checks: &Value) -> Value {
        serde_json::json!({
            "id": id, "name": name, "target": "branch", "enforcement": "active",
            "updated_at": "2026-10-30T10:00:00Z",
            "conditions": { "ref_name": { "include": [include], "exclude": [] } },
            "bypass_actors": [],
            "rules": [
                { "type": "pull_request", "parameters": { "required_approving_review_count": 0,
                  "dismiss_stale_reviews_on_push": false, "require_code_owner_review": false,
                  "require_last_push_approval": false, "required_review_thread_resolution": false } },
                { "type": "required_status_checks", "parameters": {
                  "strict_required_status_checks_policy": true, "do_not_enforce_on_create": false,
                  "required_status_checks": checks } },
                { "type": "non_fast_forward" },
                { "type": "deletion" }
            ]
        })
    }

    /// A, B and C as `remote protect` writes them for the solo profile in
    /// Mode B, and the workflows that declare their names.
    fn protected_world() -> World {
        let trusted = checks(&[
            ("trusted: commit standards", 15368),
            ("trusted: secret scan", 15368),
            ("codeflow gates", 15368),
        ]);
        World {
            rulesets: vec![
                serde_json::json!({
                    "id": 1, "name": "codeflow authority updates", "target": "branch",
                    "enforcement": "active",
                    "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } },
                    "bypass_actors": [{ "actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always" }],
                    "rules": [{ "type": "update", "parameters": { "update_allows_fetch_and_merge": false } }]
                }),
                checked(2, "codeflow authority checks", "~DEFAULT_BRANCH", &trusted),
                checked(
                    3,
                    "codeflow line checks",
                    "refs/heads/integration/**",
                    &trusted,
                ),
            ],
            workflows: vec![
                (POLICY_WORKFLOW, POLICY_WORKFLOW_TEXT.to_string()),
                (CI_WORKFLOW_PATH, CI_WORKFLOW_TEXT.to_string()),
            ],
            ..World::default()
        }
    }

    fn judge(world: World, profile: Option<&str>) -> CheckResult {
        WORLD.with(|w| *w.borrow_mut() = world);
        let dir = project();
        write_policy(
            dir.path(),
            r#"{"git":{"required_checks":["codeflow gates"]}}"#,
        );
        let mut options = opts(dir.path(), world_exec);
        options.remote_profile = profile.map(str::to_string);
        check(&options)
    }

    fn ruleset_mut<'a>(world: &'a mut World, name: &str) -> &'a mut Value {
        world
            .rulesets
            .iter_mut()
            .find(|r| r["name"] == name)
            .unwrap()
    }

    #[test]
    fn the_rulesets_protect_writes_pass_for_their_profile() {
        let result = judge(protected_world(), Some("solo-administrator"));
        assert_eq!(result.status, Status::Pass, "{}", result.message);
    }

    /// AC-3: a non-empty bypass list on B or C fails, naming the actors and
    /// the close; the close (an empty list) turns it green.
    #[test]
    fn an_open_recovery_window_fails_until_it_is_closed() {
        for name in ["codeflow authority checks", "codeflow line checks"] {
            let mut world = protected_world();
            ruleset_mut(&mut world, name)["bypass_actors"] = serde_json::json!([
                { "actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always" }
            ]);
            for profile in [None, Some("solo-administrator")] {
                let result = judge(world.clone(), profile);
                assert_eq!(result.status, Status::Fail, "{name}: {}", result.message);
                for expected in [
                    name,
                    "lets RepositoryRole 5 (always) bypass it",
                    "recovery window is open since 2026-10-30T10:00:00Z",
                    "close it: codeflow remote protect",
                ] {
                    assert!(
                        result.message.contains(expected),
                        "{expected}: {}",
                        result.message
                    );
                }
            }
            ruleset_mut(&mut world, name)["bypass_actors"] = serde_json::json!([]);
            let result = judge(world, Some("solo-administrator"));
            assert_eq!(result.status, Status::Pass, "{}", result.message);
        }
        // A bypass list that cannot be read stays unknown: never a pass.
        let mut world = protected_world();
        ruleset_mut(&mut world, "codeflow line checks")
            .as_object_mut()
            .unwrap()
            .remove("bypass_actors");
        let result = judge(world, None);
        assert!(
            matches!(result.status, Status::Note(_)),
            "{:?}: {}",
            result.status,
            result.message
        );
        assert!(
            result.message.contains("bypass list not readable"),
            "{}",
            result.message
        );
    }

    /// AC-3: a non-strict check rule on the lines (this repository's
    /// 25435656 on 2026-10-29) warns.
    #[test]
    fn a_non_strict_line_rule_warns() {
        let mut world = protected_world();
        world.rulesets.push(serde_json::json!({
            "id": 25_435_656, "name": "integration lines", "target": "branch", "enforcement": "active",
            "conditions": { "ref_name": { "include": ["refs/heads/integration/**"], "exclude": [] } },
            "bypass_actors": [],
            "rules": [
                { "type": "required_status_checks", "parameters": {
                  "strict_required_status_checks_policy": false,
                  "required_status_checks": checks(&[("codeflow gates", 15368)]) } },
                { "type": "non_fast_forward" }, { "type": "deletion" }
            ]
        }));
        let text = warn_text(&judge(world, None));
        assert!(
            text.contains("ruleset 'integration lines' (25435656) requires checks on integration/** but not that a branch be up to date"),
            "{text}"
        );
    }

    /// AC-3: a line pattern no ruleset puts a pull request on warns.
    #[test]
    fn a_line_pattern_without_a_pull_request_rule_warns() {
        let mut world = protected_world();
        world
            .rulesets
            .retain(|r| r["name"] != "codeflow line checks");
        world.rulesets.push(serde_json::json!({
            "id": 9, "name": "line deletion", "target": "branch", "enforcement": "active",
            "conditions": { "ref_name": { "include": ["refs/heads/integration/**"], "exclude": [] } },
            "bypass_actors": [], "rules": [{ "type": "deletion" }]
        }));
        let text = warn_text(&judge(world, None));
        assert!(
            text.contains("no active ruleset requires a pull request on integration/**"),
            "{text}"
        );
    }

    /// AC-3: an `authority:` name accepted from GitHub Actions warns.
    #[test]
    fn an_authority_name_pinned_to_github_actions_warns() {
        let mut world = protected_world();
        ruleset_mut(&mut world, "codeflow authority checks")["rules"][1]["parameters"]
            ["required_status_checks"] = checks(&[
            ("authority: commit standards", 15368),
            ("authority: secret scan", 4242),
            ("codeflow gates", 15368),
        ]);
        let text = warn_text(&judge(world, None));
        assert!(
            text.contains("accepts authority: commit standards from GitHub Actions or any source"),
            "{text}"
        );
        assert!(!text.contains("authority: secret scan from"), "{text}");
    }

    /// AC-3: an authority environment that admits more than the default
    /// branch warns; one that admits `main` alone does not.
    #[test]
    fn an_environment_that_admits_more_than_the_default_branch_warns() {
        let custom =
            serde_json::json!({ "protected_branches": false, "custom_branch_policies": true });
        for (policy, branches, expected) in [
            (Value::Null, None, Some("admits every branch")),
            (
                serde_json::json!({ "protected_branches": true, "custom_branch_policies": false }),
                None,
                Some("admits every protected branch"),
            ),
            (
                custom.clone(),
                Some(serde_json::json!({ "branch_policies": [
                    { "name": "main", "type": "branch" },
                    { "name": "integration/*", "type": "branch" }
                ] })),
                Some("admits the patterns main (branch), integration/* (branch)"),
            ),
            (
                custom,
                Some(
                    serde_json::json!({ "branch_policies": [{ "name": "main", "type": "branch" }] }),
                ),
                None,
            ),
        ] {
            let mut world = protected_world();
            world.environment = Some(serde_json::json!({
                "name": "codeflow-authority", "deployment_branch_policy": policy
            }));
            world.branch_policies = branches;
            let result = judge(world, None);
            match expected {
                Some(expected) => {
                    let text = warn_text(&result);
                    assert!(text.contains(expected), "{expected}: {text}");
                }
                None => assert_eq!(result.status, Status::Pass, "{}", result.message),
            }
        }
    }

    /// AC-3: a second job, or a job in another workflow, that references
    /// the authority environment warns.
    #[test]
    fn a_second_job_referencing_the_environment_warns() {
        let mut world = protected_world();
        world.environment = Some(serde_json::json!({
            "name": "codeflow-authority",
            "deployment_branch_policy": { "protected_branches": false, "custom_branch_policies": true }
        }));
        world.branch_policies =
            Some(serde_json::json!({ "branch_policies": [{ "name": "main", "type": "branch" }] }));
        world.workflows.push((
            ".github/workflows/publish.yml",
            "jobs:\n  publish:\n    environment:\n      name: codeflow-authority\n".to_string(),
        ));
        let text = warn_text(&judge(world, None));
        assert!(
            text.contains("referenced by .github/workflows/codeflow-policy.yml job authority, .github/workflows/publish.yml job publish"),
            "{text}"
        );
    }

    /// AC-3: a foreign ruleset that requires names no workflow on the
    /// default branch produces (this repository's 24379074 once the old
    /// names are gone) warns with the route.
    #[test]
    fn a_foreign_ruleset_requiring_a_name_nothing_produces_warns() {
        let mut world = protected_world();
        world.rulesets.push(serde_json::json!({
            "id": 24_379_074, "name": "main required checks", "target": "branch", "enforcement": "active",
            "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } },
            "bypass_actors": [],
            "rules": [{ "type": "required_status_checks", "parameters": {
                "strict_required_status_checks_policy": true,
                "required_status_checks": checks(&[
                    ("codeflow gates", 15368), ("commit standards", 15368), ("secret scan", 15368)
                ]) } }]
        }));
        let text = warn_text(&judge(world, None));
        assert!(
            text.contains("ruleset 'main required checks' (24379074) requires commit standards, secret scan, which no workflow on main produces"),
            "{text}"
        );
        assert!(
            text.contains("then retire that ruleset on the host"),
            "{text}"
        );
    }

    /// AC-3: with `--profile`, the owned rulesets are compared with it.
    #[test]
    fn the_owned_rulesets_are_compared_with_the_profile() {
        let text = warn_text(&judge(protected_world(), Some("independent-review")));
        for expected in [
            "differ from the independent-review profile",
            "expects the maintain role (RepositoryRole 2) alone",
            "requires fewer than 1 approval(s)",
            "does not dismiss stale approvals",
            "codeflow remote protect --profile independent-review",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        let mut world = protected_world();
        world
            .rulesets
            .retain(|r| r["name"] != "codeflow authority updates");
        let text = warn_text(&judge(world.clone(), Some("solo-administrator")));
        assert!(
            text.contains("ruleset 'codeflow authority updates' is missing"),
            "{text}"
        );
        // Without a profile, a missing ruleset is not judged.
        let result = judge(world, None);
        assert_eq!(result.status, Status::Pass, "{}", result.message);
    }
}
