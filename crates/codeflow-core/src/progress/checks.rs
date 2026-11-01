//! Check runs and the required-check view.
//!
//! The pull request list gives every run but not which checks are
//! required, nor which app produced a run. The required set is read per
//! base branch from the forge's rules and branch protection, once each; a
//! requirement bound to an app is judged only against that app's check
//! runs, read per head. Every read shares the forge observation's one time
//! bound. A read that fails is an error with its reason, never an empty
//! set, so a missing required check is never reported as none required
//! and a same-named run from another app is never taken as the required
//! one.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::inputs::{CheckRunIn, RequiredCheckIn};

/// States of a run that has not finished.
const PENDING: [&str; 6] = [
    "pending",
    "queued",
    "in_progress",
    "expected",
    "waiting",
    "requested",
];

/// States that are neither a failure nor unfinished.
const SETTLED: [&str; 3] = ["success", "neutral", "skipped"];

/// The summary of run `states`: `failed` when any failed, was cancelled
/// or timed out, else `pending` when any has not finished, `missing` when
/// none ran, `skipped` when any skipped (a skip is not a pass without
/// reuse evidence), else `passed`.
fn summary_of(states: &[&str]) -> &'static str {
    if states.is_empty() {
        "missing"
    } else if states
        .iter()
        .any(|state| !SETTLED.contains(state) && !PENDING.contains(state))
    {
        "failed"
    } else if states.iter().any(|state| PENDING.contains(state)) {
        "pending"
    } else if states.contains(&"skipped") {
        "skipped"
    } else {
        "passed"
    }
}

/// The summary of `runs` (`summary_of`).
pub(super) fn summary<'a>(runs: impl IntoIterator<Item = &'a CheckRunIn>) -> &'static str {
    let states: Vec<&str> = runs.into_iter().map(|run| run.state.as_str()).collect();
    summary_of(&states)
}

/// A check run with the app that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProducedRun {
    pub name: String,
    pub app: u64,
    pub state: String,
}

/// The required view of a pull request's runs: `unverified` when the
/// required set could not be read, `none required` when the base requires
/// none, else the worst of the required checks. A requirement any app
/// satisfies is judged by name over `runs`; one bound to an app only over
/// that app's `produced` runs, and `unverified` when those were not read.
/// A required check with no run is `missing`.
pub(super) fn required_summary(
    runs: &[CheckRunIn],
    required: &Result<Vec<RequiredCheckIn>, String>,
    produced: &Result<Vec<ProducedRun>, String>,
) -> String {
    let Ok(checks) = required else {
        return "unverified".into();
    };
    if checks.is_empty() {
        return "none required".into();
    }
    let per_check: Vec<&'static str> = checks
        .iter()
        .map(|check| match (check.app, produced) {
            (None, _) => summary(runs.iter().filter(|run| run.name == check.name)),
            (Some(_), Err(_)) => "unverified",
            (Some(app), Ok(produced)) => {
                let states: Vec<&str> = produced
                    .iter()
                    .filter(|run| run.name == check.name && run.app == app)
                    .map(|run| run.state.as_str())
                    .collect();
                summary_of(&states)
            }
        })
        .collect();
    ["failed", "unverified", "pending", "missing", "skipped"]
        .into_iter()
        .find(|worst| per_check.contains(worst))
        .unwrap_or("passed")
        .into()
}

/// Whether a listed run named `name` is a required one: `Some(true)` when
/// a requirement any app satisfies names it, or every run of that name
/// came from the app its requirement names; `Some(false)` when none is
/// required by that name or none came from the required app; `None` when
/// the producers could not be read or differ.
pub(super) fn run_required(
    name: &str,
    required: &Result<Vec<RequiredCheckIn>, String>,
    produced: &Result<Vec<ProducedRun>, String>,
) -> Option<bool> {
    let checks = required.as_ref().ok()?;
    if checks.iter().any(|c| c.app.is_none() && c.name == name) {
        return Some(true);
    }
    let apps: Vec<u64> = checks
        .iter()
        .filter(|c| c.name == name)
        .filter_map(|c| c.app)
        .collect();
    if apps.is_empty() {
        return Some(false);
    }
    let produced = produced.as_ref().ok()?;
    let named: Vec<&ProducedRun> = produced.iter().filter(|run| run.name == name).collect();
    if named.iter().all(|run| apps.contains(&run.app)) && !named.is_empty() {
        Some(true)
    } else if named.iter().all(|run| !apps.contains(&run.app)) {
        Some(false)
    } else {
        None
    }
}

/// The most rules or check runs one read lists; a full page may hide more.
const PAGE_LIMIT: usize = 100;

/// One `gh api` read of `path` within what is left of `deadline`.
fn api(root: &Path, gh: &std::ffi::OsStr, deadline: Instant, path: &str) -> Result<Value, String> {
    let limit = deadline.saturating_duration_since(Instant::now());
    if limit == Duration::ZERO {
        return Err("not read: the forge observation's time bound was spent".into());
    }
    let mut command = crate::git::process(gh);
    command
        .current_dir(root)
        .env("GH_PROMPT_DISABLED", "1")
        .args(["api", path]);
    let output = crate::coord::observe::command(command, limit).ok_or_else(|| {
        "the read did not finish within the forge observation's time bound or could not start"
            .to_string()
    })?;
    if !output.status.success() {
        return Err("the read failed (offline or not signed in)".into());
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| "the answer was not the expected JSON".into())
}

/// A requirement's producer: `None` when any app satisfies it (absent,
/// null, or the `-1` branch protection uses for any).
fn producer(check: &Value, key: &str) -> Result<Option<u64>, ()> {
    match check.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) if value.as_i64() == Some(-1) => Ok(None),
        Some(value) => value.as_u64().map(Some).ok_or(()),
    }
}

/// The checks `base` requires on `repository`, from its rulesets and its
/// branch protection, each with the app it must come from when one is
/// named, or why they could not be read.
pub(super) fn required_checks(
    root: &Path,
    gh: &std::ffi::OsStr,
    deadline: Instant,
    repository: &str,
    base: &str,
) -> Result<Vec<RequiredCheckIn>, String> {
    if base.is_empty()
        || !base
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._/-".contains(&byte))
    {
        return Err(format!(
            "the base branch `{base}` is not a name status reads required checks for"
        ));
    }
    let read = |path: String| {
        api(root, gh, deadline, &path)
            .map_err(|reason| format!("the required-check read: {reason}"))
    };
    let malformed = || "the required-check answer was not the expected JSON".to_string();
    let mut out: Vec<RequiredCheckIn> = Vec::new();
    let mut push = |check: &Value, app_key: &str| -> Result<(), String> {
        let name = check
            .get("context")
            .and_then(Value::as_str)
            .ok_or_else(malformed)?;
        let app = producer(check, app_key).map_err(|()| malformed())?;
        out.push(RequiredCheckIn {
            name: name.to_string(),
            app,
        });
        Ok(())
    };
    let rules = read(format!(
        "repos/{repository}/rules/branches/{base}?per_page={PAGE_LIMIT}"
    ))?;
    let rules = rules.as_array().ok_or_else(malformed)?;
    if rules.len() >= PAGE_LIMIT {
        return Err(format!(
            "the forge listed {} rules for `{base}`, its read limit, so more may exist",
            rules.len()
        ));
    }
    for rule in rules {
        let kind = rule
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(malformed)?;
        if kind != "required_status_checks" {
            continue;
        }
        let checks = rule
            .pointer("/parameters/required_status_checks")
            .and_then(Value::as_array)
            .ok_or_else(malformed)?;
        for check in checks {
            push(check, "integration_id")?;
        }
    }
    let branch = read(format!("repos/{repository}/branches/{base}"))?;
    if branch.get("name").and_then(Value::as_str) != Some(base) {
        return Err(malformed());
    }
    if let Some(protection) = branch.pointer("/protection/required_status_checks") {
        let contexts = protection.get("contexts").and_then(Value::as_array);
        let checks = protection.get("checks").and_then(Value::as_array);
        if contexts.is_none() && checks.is_none() {
            return Err(malformed());
        }
        let mut seen: Vec<String> = Vec::new();
        for check in checks.into_iter().flatten() {
            push(check, "app_id")?;
            if let Some(name) = check.get("context").and_then(Value::as_str) {
                seen.push(name.to_string());
            }
        }
        // `contexts` repeats the names `checks` lists, without their app.
        for context in contexts.into_iter().flatten() {
            let name = context.as_str().ok_or_else(malformed)?;
            if !seen.iter().any(|seen| seen == name) {
                push(&serde_json::json!({ "context": name }), "app_id")?;
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// The check runs on `sha` with the app that produced each, or why they
/// could not be read.
pub(super) fn produced_runs(
    root: &Path,
    gh: &std::ffi::OsStr,
    deadline: Instant,
    repository: &str,
    sha: &str,
) -> Result<Vec<ProducedRun>, String> {
    if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("the head `{sha}` is not a commit id"));
    }
    let malformed = || "the check-run answer was not the expected JSON".to_string();
    let answer = api(
        root,
        gh,
        deadline,
        &format!("repos/{repository}/commits/{sha}/check-runs?per_page={PAGE_LIMIT}"),
    )
    .map_err(|reason| format!("the check-run read: {reason}"))?;
    let runs = answer
        .get("check_runs")
        .and_then(Value::as_array)
        .ok_or_else(malformed)?;
    let total = answer
        .get("total_count")
        .and_then(Value::as_u64)
        .ok_or_else(malformed)?;
    if usize::try_from(total).map_or(true, |total| total > runs.len()) {
        return Err(format!(
            "the forge holds {total} check runs on the head and listed {}, so more may exist",
            runs.len()
        ));
    }
    runs.iter()
        .map(|run| {
            let text = |key: &str| run.get(key).and_then(Value::as_str);
            Some(ProducedRun {
                name: text("name")?.to_string(),
                app: run.pointer("/app/id").and_then(Value::as_u64)?,
                state: text("conclusion")
                    .filter(|text| !text.is_empty())
                    .or_else(|| text("status"))?
                    .to_ascii_lowercase(),
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(malformed)
}

/// An RFC 3339 time with any offset and optional fraction, as seconds since
/// the Unix epoch; `None` when it is not such a time.
pub(super) fn parse_instant(text: &str) -> Option<i64> {
    let (date_time, rest) = (text.get(..19)?, text.get(19..)?);
    let rest = match rest.strip_prefix('.') {
        Some(fraction) => {
            let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                return None;
            }
            &fraction[digits..]
        }
        None => rest,
    };
    let base = i64::try_from(crate::coord::parse_time(&format!("{date_time}Z")).ok()?).ok()?;
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let bytes = rest.as_bytes();
            if bytes.len() != 6 || bytes[3] != b':' {
                return None;
            }
            let sign = match bytes[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hours: i64 = rest.get(1..3)?.parse().ok()?;
            let minutes: i64 = rest.get(4..6)?.parse().ok()?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            sign * (hours * 3600 + minutes * 60)
        }
    };
    Some(base - offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &str, state: &str) -> CheckRunIn {
        CheckRunIn {
            name: name.into(),
            state: state.into(),
            completed_at: None,
            required: None,
        }
    }

    fn needs(name: &str, app: Option<u64>) -> RequiredCheckIn {
        RequiredCheckIn {
            name: name.into(),
            app,
        }
    }

    #[test]
    fn a_required_check_with_no_run_is_missing_and_optional_success_does_not_hide_it() {
        let none: Result<Vec<ProducedRun>, String> = Ok(Vec::new());
        let runs = [run("optional-lint", "success")];
        let build = Ok(vec![needs("build", None)]);
        assert_eq!(required_summary(&runs, &build, &none), "missing");
        assert_eq!(
            required_summary(&runs, &Ok(Vec::new()), &none),
            "none required"
        );
        assert_eq!(
            required_summary(&runs, &Err("unread".into()), &none),
            "unverified"
        );
        let runs = [run("build", "success"), run("lint", "failure")];
        assert_eq!(required_summary(&runs, &build, &none), "passed");
        assert_eq!(summary(&runs), "failed");
    }

    #[test]
    fn a_requirement_bound_to_an_app_takes_only_that_apps_runs() {
        let runs = [run("build", "success")];
        let bound = Ok(vec![needs("build", Some(42))]);
        let produced = |app| {
            Ok(vec![ProducedRun {
                name: "build".into(),
                app,
                state: "success".into(),
            }])
        };
        assert_eq!(required_summary(&runs, &bound, &produced(99)), "missing");
        assert_eq!(run_required("build", &bound, &produced(99)), Some(false));
        assert_eq!(required_summary(&runs, &bound, &produced(42)), "passed");
        assert_eq!(run_required("build", &bound, &produced(42)), Some(true));
        let unread = Err("unread".into());
        assert_eq!(required_summary(&runs, &bound, &unread), "unverified");
        assert_eq!(run_required("build", &bound, &unread), None);
    }

    #[test]
    fn offsets_and_fractions_normalize_to_utc() {
        assert_eq!(
            parse_instant("2026-11-01T07:00:00-04:00"),
            parse_instant("2026-11-01T11:00:00Z")
        );
        assert_eq!(
            parse_instant("2026-11-01T11:00:00.250Z"),
            parse_instant("2026-11-01T11:00:00Z")
        );
        assert_eq!(parse_instant("yesterday"), None);
        assert_eq!(parse_instant("2026-11-01T11:00:00+2:00"), None);
    }
}
