//! The destination's default branch from a source the pull request's author
//! cannot set (TSK-1267, SPC-013 R-120). The default branch decides which
//! range is judged under the release rules, so a value the head can change
//! would let a change pick the rules it is judged under.
//!
//! Only a GitHub Actions `pull_request_target` run is trusted: GitHub runs
//! that workflow from the default branch, never from the pull request, so
//! both its event payload (`repository.default_branch`) and a
//! `--default-branch` flag written in that workflow are out of the author's
//! reach. Every other run (a `pull_request` or `push` run, whose workflow
//! file comes from the pushed commit; another CI; a local run) ignores the
//! flag with a note and asks the destination, as before.

use std::ffi::OsString;

/// A trusted default branch name and where it came from.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Trusted {
    pub name: String,
    pub source: &'static str,
}

/// What this run may use as the default branch.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Reading {
    /// A name from a source the author cannot set.
    Trusted(Trusted),
    /// No trusted name; `Some` says why a given flag was not used.
    None(Option<String>),
}

const EVENT_SOURCE: &str = "the pull_request_target event (repository.default_branch)";
const FLAG_SOURCE: &str = "--default-branch in the pull_request_target workflow";

/// Read the trusted default branch for this run from `flag` and the
/// environment `env`.
///
/// # Errors
///
/// Returns why the run refuses: a trusted source names an invalid branch,
/// the event file cannot be read, or the flag and the event disagree.
pub(super) fn read(
    flag: Option<&str>,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Reading, String> {
    let text = |key: &str| {
        env(key)
            .map(OsString::into_string)
            .transpose()
            .map_err(|_| format!("cannot read {key} as UTF-8"))
    };
    let ignored = |why: &str| {
        Ok(Reading::None(flag.map(|value| {
            format!(
                "--default-branch '{}' is not trusted: {why}; the destination is asked instead",
                codeflow_core::git::display_text(value)
            )
        })))
    };
    if text("GITHUB_ACTIONS")?.as_deref() != Some("true") {
        return ignored("only a GitHub Actions pull_request_target run, whose workflow comes from the default branch, can pass it");
    }
    let event = text("GITHUB_EVENT_NAME")?.unwrap_or_default();
    if event != "pull_request_target" {
        return ignored(&format!(
            "the '{}' event runs a workflow the pushed commit can change; only pull_request_target runs the default branch's workflow",
            codeflow_core::git::display_text(&event)
        ));
    }
    let from_event = match env("GITHUB_EVENT_PATH") {
        None => None,
        Some(path) => {
            let bytes = std::fs::read(&path)
                .map_err(|error| format!("cannot read the GitHub event file: {error}"))?;
            let payload: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|error| format!("cannot read the GitHub event JSON: {error}"))?;
            match payload.pointer("/repository/default_branch") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(name)) => Some(name.clone()),
                Some(_) => {
                    return Err(
                        "the GitHub event's repository.default_branch is not a string".into(),
                    )
                }
            }
        }
    };
    let checked = |name: &str, source: &'static str| {
        if super::is_branch_name(name) {
            Ok(Trusted {
                name: name.to_string(),
                source,
            })
        } else {
            Err(format!(
                "{source} names '{}', which is not a branch name",
                codeflow_core::git::display_text(name)
            ))
        }
    };
    match (flag, from_event.as_deref()) {
        (Some(flag), Some(event)) if flag != event => Err(format!(
            "--default-branch '{}' disagrees with the event's repository.default_branch '{}'; pass the event's value (`${{{{ github.event.repository.default_branch }}}}`)",
            codeflow_core::git::display_text(flag),
            codeflow_core::git::display_text(event)
        )),
        (_, Some(event)) => checked(event, EVENT_SOURCE).map(Reading::Trusted),
        (Some(flag), None) => checked(flag, FLAG_SOURCE).map(Reading::Trusted),
        (None, None) => Ok(Reading::None(None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        move |key| map.get(key).map(OsString::from)
    }

    fn event_file(json: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut file, json.as_bytes()).unwrap();
        file
    }

    const EVENT: &str = r#"{"repository": {"default_branch": "main"}}"#;

    #[test]
    fn a_pull_request_target_event_supplies_the_default_branch() {
        let file = event_file(EVENT);
        let path = file.path().to_str().unwrap();
        let env = env_of(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request_target"),
            ("GITHUB_EVENT_PATH", path),
        ]);
        assert_eq!(
            read(None, &env).unwrap(),
            Reading::Trusted(Trusted {
                name: "main".into(),
                source: EVENT_SOURCE
            })
        );
        assert_eq!(
            read(Some("main"), &env).unwrap(),
            Reading::Trusted(Trusted {
                name: "main".into(),
                source: EVENT_SOURCE
            })
        );
    }

    #[test]
    fn the_flag_is_trusted_in_a_pull_request_target_run_without_the_field() {
        let file = event_file("{}");
        let env = env_of(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request_target"),
            ("GITHUB_EVENT_PATH", file.path().to_str().unwrap()),
        ]);
        assert_eq!(
            read(Some("trunk"), &env).unwrap(),
            Reading::Trusted(Trusted {
                name: "trunk".into(),
                source: FLAG_SOURCE
            })
        );
    }

    /// AC-1 negative: in a `pull_request` run the workflow file comes from
    /// the pull request's merge commit, so the head can set both the flag
    /// and the environment; neither is trusted.
    #[test]
    fn a_head_controlled_value_is_not_trusted() {
        let file = event_file(r#"{"repository": {"default_branch": "release/evil"}}"#);
        let path = file.path().to_str().unwrap();
        for event in ["pull_request", "push", "workflow_dispatch", ""] {
            let env = env_of(&[
                ("GITHUB_ACTIONS", "true"),
                ("GITHUB_EVENT_NAME", event),
                ("GITHUB_EVENT_PATH", path),
            ]);
            match read(Some("release/evil"), &env).unwrap() {
                Reading::None(Some(why)) => assert!(why.contains("is not trusted"), "{why}"),
                other => panic!("{event}: trusted a head-controlled value: {other:?}"),
            }
            assert_eq!(read(None, &env).unwrap(), Reading::None(None), "{event}");
        }
        // Outside GitHub Actions nothing names the workflow's origin.
        let env = env_of(&[("GITHUB_EVENT_NAME", "pull_request_target")]);
        assert!(matches!(
            read(Some("main"), &env).unwrap(),
            Reading::None(Some(_))
        ));
    }

    #[test]
    fn a_flag_that_disagrees_with_the_event_refuses() {
        let file = event_file(EVENT);
        let env = env_of(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request_target"),
            ("GITHUB_EVENT_PATH", file.path().to_str().unwrap()),
        ]);
        let why = read(Some("develop"), &env).unwrap_err();
        assert!(why.contains("disagrees"), "{why}");
    }

    #[test]
    fn an_invalid_or_unreadable_value_refuses() {
        let file = event_file(r#"{"repository": {"default_branch": "main~1"}}"#);
        let env = env_of(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request_target"),
            ("GITHUB_EVENT_PATH", file.path().to_str().unwrap()),
        ]);
        assert!(read(None, &env).unwrap_err().contains("not a branch name"));
        let env = env_of(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request_target"),
            ("GITHUB_EVENT_PATH", "/nonexistent/event.json"),
        ]);
        assert!(read(Some("main"), &env).is_err());
    }
}
