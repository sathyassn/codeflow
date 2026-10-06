//! The CI binary pin in `doctor`'s perimeter check (TSK-095; SPC-013 R-113).
//!
//! The scaffolded CI installs the `codeflow` version the target branch pins
//! in `.codeflow/project.toml` (`scaffold_version`), so the target's binary
//! judges every change. An upgrade takes two pull requests, in order: the
//! first raises only the pin, and after it lands the second carries
//! `codeflow update`. This compares the checkout's pin and policy with the
//! target branch's and says which of those states the checkout is in. The
//! target is the remote's default branch, then `main` or `master`; it is read
//! from the local refs only, never fetched.

use std::collections::BTreeSet;
use std::path::Path;

use crate::scaffold::version::is_older;

use super::Status;
use crate::remedy;

/// Where the checkout stands against the target's pin.
pub(super) struct PinReport {
    pub status: Status,
    pub message: String,
}

/// The local refs tried, in order, for the branch changes land on.
const TARGETS: [&str; 5] = [
    "refs/remotes/origin/HEAD",
    "refs/remotes/origin/main",
    "refs/remotes/origin/master",
    "refs/heads/main",
    "refs/heads/master",
];

const STATE: &str = ".codeflow/project.toml";
const POLICY: &str = ".codeflow/policy.json";

/// Each shipped install doctor recognizes: the template, and the first and
/// last lines of the span that chooses the target commit and installs the
/// release it pins, verified against the release's `sha256.sum`. The
/// GitLab, Bitbucket and generic spans end with the shared pinned run.
const INSTALLS: [(&str, &str, &str); 5] = [
    (
        include_str!("../../../../assets/base/ci/codeflow-ci.yml"),
        "- name: Install codeflow (target-pinned, checksum-verified)",
        "installed and verified against sha256.sum",
    ),
    (
        include_str!("../../../../assets/base/ci/codeflow-policy.yml"),
        "- uses: actions/checkout@v6",
        "installed and verified against sha256.sum",
    ),
    (
        include_str!("../../../../assets/base/ci/.gitlab-ci.yml"),
        "if [ \"${CI_PIPELINE_SOURCE:-}\" = merge_request_event ]; then",
        "# <<< codeflow pinned run",
    ),
    (
        include_str!("../../../../assets/base/ci/bitbucket-pipelines.yml"),
        "BASE=\"${BITBUCKET_PR_DESTINATION_COMMIT:-}\"",
        "# <<< codeflow pinned run",
    ),
    (
        include_str!("../../../../assets/base/ci/ci-generic.sh"),
        "BASE=\"${1:-${BASE:-}}\"",
        "# <<< codeflow pinned run",
    ),
];

/// Whether a CI file carries one of the shipped pinned installs unchanged,
/// at any indentation. Doctor claims the pin only for such a file: marker
/// text, a comment or an edited install says nothing about what CI runs.
pub(super) fn recognized(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().collect();
    INSTALLS.iter().any(|(template, first, last)| {
        let Some(span) = span(template, first, last) else {
            return false;
        };
        lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.trim_matches([' ', '\t']) == span[0])
            .any(|(at, line)| {
                let indent = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
                lines.len() >= at + span.len()
                    && lines[at..at + span.len()]
                        .iter()
                        .zip(&span)
                        .all(|(line, want)| dedent(line, indent) == Some(want.as_str()))
            })
    })
}

/// The lines of `template` from the one holding `first` to the next one
/// holding `last`, without the first line's indentation.
fn span(template: &str, first: &str, last: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = template.lines().collect();
    let start = lines.iter().position(|line| line.contains(first))?;
    let end = start + lines[start..].iter().position(|line| line.contains(last))?;
    let indent =
        &lines[start][..lines[start].len() - lines[start].trim_start_matches([' ', '\t']).len()];
    lines[start..=end]
        .iter()
        .map(|line| dedent(line, indent).map(str::to_string))
        .collect()
}

/// `line` without `indent`; a blank line is empty, and a line indented
/// less than `indent` does not belong to the span.
fn dedent<'a>(line: &'a str, indent: &str) -> Option<&'a str> {
    if line.trim_matches([' ', '\t']).is_empty() {
        return Some("");
    }
    line.strip_prefix(indent)
}

struct PinInputs {
    state: Option<String>,
    policy: Option<String>,
    pin: Option<String>,
}

fn current_inputs(root: &Path) -> Result<PinInputs, String> {
    let read = |name: &str| -> Result<Option<String>, String> {
        let path = root.join(name);
        if !path.try_exists().map_err(|error| error.to_string())? {
            return Ok(None);
        }
        std::fs::read_to_string(path)
            .map(Some)
            .map_err(|error| format!("{name}: {error}"))
    };
    read_inputs(read)
}

fn read_inputs(read: impl Fn(&str) -> TargetText) -> Result<PinInputs, String> {
    let state = read(STATE)?;
    let policy = read(POLICY)?;
    let pin = state.as_deref().map(pinned).transpose()?.flatten();
    Ok(PinInputs { state, policy, pin })
}

/// The pin state of the checkout at `root`.
pub(super) fn report(root: &Path) -> PinReport {
    let refused = |error: String| PinReport {
        status: Status::Fail,
        message: format!("cannot read CI pin inputs: {error}"),
    };
    let PinInputs {
        state: head_state,
        policy: head_policy,
        pin: head_pin,
    } = match current_inputs(root) {
        Ok(inputs) => inputs,
        Err(error) => return refused(error),
    };
    let Some(head_pin) = head_pin else {
        return PinReport {
            status: Status::Warn(remedy::DOCTOR_CI_PIN_MISSING.remedy()),
            message: format!(
                "no scaffold_version is pinned in {STATE}, so the pinned CI install fails closed"
            ),
        };
    };
    let target_files = match target_files(root) {
        Ok(target) => target,
        Err(error) => return refused(error),
    };
    let Some((target, show)) = target_files else {
        return PinReport {
            status: Status::Pass,
            message: format!(
                "CI installs the codeflow version the target branch pins ({head_pin} here), verified against its sha256.sum; no local target branch to compare with"
            ),
        };
    };
    let PinInputs {
        state: target_state,
        policy: target_policy,
        pin: target_pin,
    } = match read_inputs(show) {
        Ok(inputs) => inputs,
        Err(error) => return refused(error),
    };
    if let Err(error) = head_policy
        .as_deref()
        .map(policy_keys)
        .transpose()
        .and_then(|_| target_policy.as_deref().map(policy_keys).transpose())
    {
        return refused(error);
    }
    let Some(target_pin) = target_pin else {
        return PinReport {
            status: Status::Warn(remedy::DOCTOR_CI_PIN_TARGET.with(&[("target", &target)])),
            message: format!(
                "{target} pins no scaffold_version, so CI on it fails closed until a pin lands there"
            ),
        };
    };
    if target_pin == head_pin {
        return PinReport {
            status: Status::Pass,
            message: format!(
                "CI installs codeflow {target_pin}, the version {target} pins, verified against its sha256.sum"
            ),
        };
    }
    if is_older(&head_pin, &target_pin) {
        return PinReport {
            status: Status::Warn(
                remedy::DOCTOR_CI_PIN_LOWERED.with(&[("pin", &target_pin), ("target", &target)]),
            ),
            message: format!(
                "this checkout lowers scaffold_version from {target_pin} ({target}) to {head_pin}; CI judges it with codeflow {target_pin} and fails a lowered pin"
            ),
        };
    }
    let carried = carried_upgrade(
        head_state.as_deref(),
        target_state.as_deref(),
        head_policy.as_deref(),
        target_policy.as_deref(),
    );
    let carried = match carried {
        Ok(carried) => carried,
        Err(error) => return refused(error),
    };
    if carried.is_empty() {
        return PinReport {
            status: Status::Pass,
            message: format!(
                "this checkout raises scaffold_version from {target_pin} ({target}) to {head_pin} and nothing else, upgrade step one: codeflow {target_pin} judges it and CI tests {head_pin} alongside; after it lands, run `codeflow update` on a new branch"
            ),
        };
    }
    PinReport {
        status: Status::Warn(remedy::DOCTOR_CI_PIN_ORDER.remedy()),
        message: format!(
            "this checkout raises scaffold_version from {target_pin} ({target}) to {head_pin} and also carries {}, which codeflow {target_pin}, the binary CI installs until the raise lands, may not read, so CI fails",
            carried.join(", ")
        ),
    }
}

/// The `scaffold_version` a project state pins, if any.
fn pinned(state: &str) -> Result<Option<String>, String> {
    let value: toml::Value =
        toml::from_str(state).map_err(|error| format!("cannot parse state: {error}"))?;
    Ok(value
        .get("scaffold_version")
        .and_then(toml::Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_string))
}

/// The first target ref that exists, and a reader of files at its commit.
type TargetText = Result<Option<String>, String>;

type TargetFiles<R> = Result<Option<(String, R)>, String>;

fn target_files(root: &Path) -> TargetFiles<impl Fn(&str) -> TargetText> {
    let repo = git2::Repository::discover(root).map_err(|error| error.to_string())?;
    let mut target = None;
    for name in TARGETS {
        let reference = match repo.find_reference(name) {
            Ok(reference) => reference,
            Err(error) if error.code() == git2::ErrorCode::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        let resolved = reference.resolve().map_err(|error| error.to_string())?;
        let tree = resolved
            .peel_to_commit()
            .and_then(|commit| commit.tree())
            .map_err(|error| error.to_string())?
            .id();
        target = Some((
            crate::git::name::reference_shorthand(&resolved)
                .display()
                .to_string(),
            tree,
        ));
        break;
    }
    let Some((name, tree)) = target else {
        return Ok(None);
    };
    let prefix = if let Some(workdir) = repo.workdir() {
        root.canonicalize()
            .map_err(|error| error.to_string())?
            .strip_prefix(workdir.canonicalize().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?
            .to_path_buf()
    } else {
        return Err("target repository has no worktree".into());
    };
    let show = move |path: &str| -> TargetText {
        let tree = repo.find_tree(tree).map_err(|error| error.to_string())?;
        let entry = match tree.get_path(&prefix.join(path)) {
            Ok(entry) => entry,
            Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let blob = repo
            .find_blob(entry.id())
            .map_err(|error| error.to_string())?;
        String::from_utf8(blob.content().to_vec())
            .map(Some)
            .map_err(|error| error.to_string())
    };
    Ok(Some((name, show)))
}

/// What an upgrade carries beyond the pin: policy keys the target's policy
/// lacks, and a changed `schema_version` in the state or the policy.
fn carried_upgrade(
    head_state: Option<&str>,
    target_state: Option<&str>,
    head_policy: Option<&str>,
    target_policy: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut carried = Vec::new();
    let head_keys = head_policy
        .map(policy_keys)
        .transpose()?
        .unwrap_or_default();
    let target_keys = target_policy
        .map(policy_keys)
        .transpose()?
        .unwrap_or_default();
    let added: Vec<String> = head_keys.difference(&target_keys).cloned().collect();
    if !added.is_empty() {
        carried.push(format!("new policy keys ({})", added.join(", ")));
    }
    let state_schema = |text: Option<&str>| {
        text.map(toml::from_str::<toml::Value>)
            .transpose()
            .map(|value| value.and_then(|v| v.get("schema_version").map(ToString::to_string)))
            .map_err(|error| error.to_string())
    };
    if state_schema(head_state)? != state_schema(target_state)? {
        carried.push(format!("a new {STATE} schema_version"));
    }
    let policy_schema = |text: Option<&str>| {
        text.map(serde_json::from_str::<serde_json::Value>)
            .transpose()
            .map(|value| value.and_then(|v| v.get("schema_version").map(ToString::to_string)))
            .map_err(|error| error.to_string())
    };
    if policy_schema(head_policy)? != policy_schema(target_policy)? {
        carried.push(format!("a new {POLICY} schema_version"));
    }
    Ok(carried)
}

/// Every object key path in a policy, dotted (`git.commit_format`).
fn policy_keys(text: &str) -> Result<BTreeSet<String>, String> {
    fn walk(value: &serde_json::Value, prefix: &str, out: &mut BTreeSet<String>) {
        if let Some(map) = value.as_object() {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                walk(child, &path, out);
                out.insert(path);
            }
        }
    }
    let mut out = BTreeSet::new();
    let value = serde_json::from_str::<serde_json::Value>(text)
        .map_err(|error| format!("cannot parse policy: {error}"))?;
    walk(&value, "", &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = crate::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    fn write(dir: &Path, path: &str, text: &str) {
        std::fs::create_dir_all(dir.join(path).parent().unwrap()).unwrap();
        std::fs::write(dir.join(path), text).unwrap();
    }

    fn state(version: &str) -> String {
        format!("schema_version = 1\ntier = \"minimal\"\nscaffold_version = \"{version}\"\n")
    }

    const POLICY_TEXT: &str = r#"{"schema_version": 1, "git": {"commit_format": "block"}}"#;

    /// A repository whose `main` pins `version`, with `feat/x` checked out.
    fn project(version: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        write(dir.path(), STATE, &state(version));
        write(dir.path(), POLICY, POLICY_TEXT);
        git(dir.path(), &["add", "-A"]);
        git(
            dir.path(),
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-q",
                "-m",
                "chore: init",
            ],
        );
        git(dir.path(), &["checkout", "-q", "-b", "feat/x"]);
        dir
    }

    #[test]
    fn a_pinned_project_names_the_version_ci_installs() {
        let dir = project("1.2.3");
        let report = report(dir.path());
        assert_eq!(report.status, Status::Pass, "{}", report.message);
        assert!(
            report
                .message
                .contains("CI installs codeflow 1.2.3, the version main pins"),
            "{}",
            report.message
        );
        assert!(report.message.contains("sha256.sum"), "{}", report.message);
    }

    #[test]
    fn an_unpinned_project_is_told_ci_fails_closed() {
        let dir = project("1.2.3");
        write(
            dir.path(),
            STATE,
            "schema_version = 1\ntier = \"minimal\"\n",
        );
        let report = report(dir.path());
        assert!(report.status.is_warn(), "{}", report.message);
        assert!(
            report.message.contains("no scaffold_version is pinned"),
            "{}",
            report.message
        );
        assert!(
            report.message.contains("fails closed"),
            "{}",
            report.message
        );
    }

    #[test]
    fn an_update_before_the_raised_pin_lands_names_the_two_step_order() {
        let dir = project("1.2.3");
        write(dir.path(), STATE, &state("1.3.0"));
        write(
            dir.path(),
            POLICY,
            r#"{"schema_version": 1, "git": {"commit_format": "block", "future_key": "warn"}}"#,
        );
        let report = report(dir.path());
        assert!(report.status.is_warn(), "{}", report.message);
        for part in [
            "raises scaffold_version from 1.2.3 (main) to 1.3.0",
            "new policy keys (git.future_key)",
        ] {
            assert!(report.message.contains(part), "{part}: {}", report.message);
        }
        let Status::Warn(remedy) = &report.status else {
            panic!("{}", report.message)
        };
        for part in [
            "two pull requests, in order",
            "run `codeflow update` on a new branch",
        ] {
            assert!(remedy.contains(part), "{part}: {remedy}");
        }
    }

    #[test]
    fn a_raise_alone_is_upgrade_step_one() {
        let dir = project("1.2.3");
        write(dir.path(), STATE, &state("1.3.0"));
        let report = report(dir.path());
        assert_eq!(report.status, Status::Pass, "{}", report.message);
        assert!(
            report.message.contains("upgrade step one"),
            "{}",
            report.message
        );
    }

    #[test]
    fn a_lowered_pin_is_reported() {
        let dir = project("1.2.3");
        write(dir.path(), STATE, &state("1.0.0"));
        let report = report(dir.path());
        assert!(report.status.is_warn(), "{}", report.message);
        assert!(
            report
                .message
                .contains("lowers scaffold_version from 1.2.3 (main) to 1.0.0"),
            "{}",
            report.message
        );
    }

    #[test]
    fn a_changed_schema_counts_as_carried() {
        let carried = carried_upgrade(
            Some(&state("1.3.0").replace("schema_version = 1", "schema_version = 2")),
            Some(&state("1.2.3")),
            Some(POLICY_TEXT),
            Some(POLICY_TEXT),
        );
        assert_eq!(
            carried.unwrap(),
            vec![format!("a new {STATE} schema_version")]
        );
    }
}
