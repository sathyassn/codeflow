//! The ceremony report's one host call (SPC-013 R-103): review counts from
//! GitHub through `gh`. Any failure is returned, never guessed around; the
//! report then prints `unknown`.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The review states that count as a submitted review round.
const SUBMITTED: &str = "APPROVED, CHANGES_REQUESTED, COMMENTED, DISMISSED";
/// Pull requests asked for in one query.
const BATCH: usize = 50;
/// How long one `gh` call may take before it counts as failed.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Where review data comes from. The CLI uses [`Gh`]; tests supply their
/// own.
pub trait ReviewHost {
    /// The number of submitted reviews of each pull request in `numbers`
    /// that has at least one; a pull request without one is left out.
    ///
    /// # Errors
    ///
    /// Returns why the host could not be asked or did not answer.
    fn rounds(&self, repo_root: &Path, numbers: &[u64]) -> Result<BTreeMap<u64, u64>, String>;
}

/// GitHub through the `gh` on `PATH`, for the repository `gh` resolves
/// from `repo_root`.
pub struct Gh;

impl ReviewHost for Gh {
    fn rounds(&self, repo_root: &Path, numbers: &[u64]) -> Result<BTreeMap<u64, u64>, String> {
        let mut rounds = BTreeMap::new();
        for batch in numbers.chunks(BATCH) {
            let fields: Vec<String> = batch
                .iter()
                .map(|n| {
                    format!(
                        "pr{n}: pullRequest(number: {n}) {{ reviews(states: [{SUBMITTED}]) {{ totalCount }} }}"
                    )
                })
                .collect();
            let query = format!(
                "query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ {} }} }}",
                fields.join(" ")
            );
            let answer = gh(
                repo_root,
                &[
                    "api",
                    "graphql",
                    "-F",
                    "owner={owner}",
                    "-F",
                    "name={repo}",
                    "-f",
                    &format!("query={query}"),
                ],
            )?;
            rounds.extend(parse_rounds(&answer, batch)?);
        }
        Ok(rounds)
    }
}

/// The counts in one GraphQL answer. A pull request the host could not
/// resolve has no data; an answer without a repository object is a failure.
fn parse_rounds(answer: &str, numbers: &[u64]) -> Result<BTreeMap<u64, u64>, String> {
    let value: serde_json::Value =
        serde_json::from_str(answer).map_err(|e| format!("unreadable host answer: {e}"))?;
    let repository = value
        .pointer("/data/repository")
        .filter(|repository| repository.is_object())
        .ok_or_else(|| first_error(&value))?;
    Ok(numbers
        .iter()
        .filter_map(|n| {
            let count = repository
                .get(format!("pr{n}"))?
                .pointer("/reviews/totalCount")?
                .as_u64()?;
            (count > 0).then_some((*n, count))
        })
        .collect())
}

fn first_error(value: &serde_json::Value) -> String {
    value
        .pointer("/errors/0/message")
        .or_else(|| value.pointer("/message"))
        .and_then(serde_json::Value::as_str)
        .map_or_else(
            || "the host answer has no repository".to_string(),
            str::to_string,
        )
}

/// Run `gh` with `args` in `dir`, bounded by [`TIMEOUT`]. Its standard
/// output is returned when it is a GraphQL answer, even on a nonzero exit,
/// since `gh` exits nonzero for a partial answer.
fn gh(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new("gh")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("`gh` did not start: {e}"))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let err = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < TIMEOUT => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("`gh` gave no answer in {}s", TIMEOUT.as_secs()));
            }
            Err(e) => return Err(format!("`gh` could not be waited on: {e}")),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success()
        || stdout
            .trim_start_matches([' ', '\t', '\n', '\r'])
            .starts_with('{')
    {
        return Ok(stdout);
    }
    let reason = stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("no message")
        .trim()
        .to_string();
    Err(format!("`gh` failed: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_read_per_pull_request_and_zero_is_no_data() {
        let answer = r#"{"data":{"repository":{"pr1":{"reviews":{"totalCount":2}},"pr2":{"reviews":{"totalCount":0}},"pr3":null}}}"#;
        let got = parse_rounds(answer, &[1, 2, 3]).unwrap();
        assert_eq!(got, BTreeMap::from([(1, 2)]));
    }

    #[test]
    fn an_answer_without_a_repository_is_a_failure() {
        let answer = r#"{"data":{"repository":null},"errors":[{"message":"Could not resolve to a Repository"}]}"#;
        assert_eq!(
            parse_rounds(answer, &[1]).unwrap_err(),
            "Could not resolve to a Repository"
        );
        assert!(parse_rounds("not json", &[1]).is_err());
    }
}
