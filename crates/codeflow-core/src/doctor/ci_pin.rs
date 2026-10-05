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
    let lines: Vec<&str> = content.lines().map(|l| l.trim_end_matches('\r')).collect();
    INSTALLS.iter().any(|(template, first, last)| {
        let Some(span) = span(template, first, last) else {
            return false;
        };
        lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.trim() == span[0])
            .any(|(at, line)| {
                let indent = &line[..line.len() - line.trim_start().len()];
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
    let indent = &lines[start][..lines[start].len() - lines[start].trim_start().len()];
    lines[start..=end]
        .iter()
        .map(|line| dedent(line, indent).map(str::to_string))
        .collect()
}

/// `line` without `indent`; a blank line is empty, and a line indented
/// less than `indent` does not belong to the span.
fn dedent<'a>(line: &'a str, indent: &str) -> Option<&'a str> {
    if line.trim().is_empty() {
        return Some("");
    }
    line.strip_prefix(indent)
}

/// The pin state of the checkout at `root`.
pub(super) fn report(root: &Path) -> PinReport {
    let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
    let head_state = read(STATE);
    let Some(head_pin) = head_state.as_deref().and_then(pinned) else {
        return PinReport {
            status: Status::Warn(remedy::DOCTOR_CI_PIN_MISSING.remedy()),
            message: format!(
                "no scaffold_version is pinned in {STATE}, so the pinned CI install fails closed"
            ),
        };
    };
    let Some((target, show)) = target_files(root) else {
        return PinReport {
            status: Status::Pass,
            message: format!(
                "CI installs the codeflow version the target branch pins ({head_pin} here), verified against its sha256.sum; no local target branch to compare with"
            ),
        };
    };
    let Some(target_pin) = show(STATE).as_deref().and_then(pinned) else {
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
        show(STATE).as_deref(),
        read(POLICY).as_deref(),
        show(POLICY).as_deref(),
    );
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
fn pinned(state: &str) -> Option<String> {
    let value: toml::Value = toml::from_str(state).ok()?;
    value
        .get("scaffold_version")
        .and_then(toml::Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// The first target ref that exists, and a reader of files at its commit.
fn target_files(root: &Path) -> Option<(String, impl Fn(&str) -> Option<String>)> {
    let repo = git2::Repository::discover(root).ok()?;
    let (name, tree) = TARGETS.iter().find_map(|name| {
        let reference = repo.find_reference(name).ok()?;
        let resolved = reference.resolve().ok()?;
        let tree = resolved.peel_to_commit().ok()?.tree().ok()?.id();
        // OS text rule (issue 79): the name is only shown; the tree is already
        // resolved, so a name that is not valid UTF-8 falls back to the ref.
        let shown = crate::git::name::reference_shorthand(&resolved)
            .display()
            .to_string();
        Some((shown, tree))
    })?;
    let prefix = repo
        .workdir()
        .and_then(|workdir| {
            let root = root.canonicalize().ok()?;
            let workdir = workdir.canonicalize().ok()?;
            root.strip_prefix(workdir).ok().map(Path::to_path_buf)
        })
        .unwrap_or_default();
    let show = move |path: &str| {
        let tree = repo.find_tree(tree).ok()?;
        let entry = tree.get_path(&prefix.join(path)).ok()?;
        let blob = repo.find_blob(entry.id()).ok()?;
        String::from_utf8(blob.content().to_vec()).ok()
    };
    Some((name, show))
}

/// What an upgrade carries beyond the pin: policy keys the target's policy
/// lacks, and a changed `schema_version` in the state or the policy.
fn carried_upgrade(
    head_state: Option<&str>,
    target_state: Option<&str>,
    head_policy: Option<&str>,
    target_policy: Option<&str>,
) -> Vec<String> {
    let mut carried = Vec::new();
    let head_keys = head_policy.map(policy_keys).unwrap_or_default();
    let target_keys = target_policy.map(policy_keys).unwrap_or_default();
    let added: Vec<String> = head_keys.difference(&target_keys).cloned().collect();
    if !added.is_empty() {
        carried.push(format!("new policy keys ({})", added.join(", ")));
    }
    let state_schema = |text: Option<&str>| {
        text.and_then(|t| toml::from_str::<toml::Value>(t).ok())
            .and_then(|v| v.get("schema_version").map(ToString::to_string))
    };
    if state_schema(head_state) != state_schema(target_state) {
        carried.push(format!("a new {STATE} schema_version"));
    }
    let policy_schema = |text: Option<&str>| {
        text.and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
            .and_then(|v| v.get("schema_version").map(ToString::to_string))
    };
    if policy_schema(head_policy) != policy_schema(target_policy) {
        carried.push(format!("a new {POLICY} schema_version"));
    }
    carried
}

/// Every object key path in a policy, dotted (`git.commit_format`).
fn policy_keys(text: &str) -> BTreeSet<String> {
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
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        walk(&value, "", &mut out);
    }
    out
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
        assert_eq!(carried, vec![format!("a new {STATE} schema_version")]);
    }
}
