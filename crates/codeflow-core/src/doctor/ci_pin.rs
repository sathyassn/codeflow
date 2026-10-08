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

use crate::scaffold::release_pin::{
    is_table_header, pinned_digests, unread_line, PinnedDigests, TABLE,
};
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
/// release it pins, verified against the digest pinned beside it and the
/// release's `sha256.sum`. The GitLab, Bitbucket and generic spans end with
/// the shared pinned run.
const INSTALLS: [(&str, &str, &str); 5] = [
    (
        include_str!("../../../../assets/base/ci/codeflow-ci.yml"),
        "- name: Install codeflow (target-pinned, checksum-verified)",
        "installed and verified against ${verified}",
    ),
    (
        include_str!("../../../../assets/base/ci/codeflow-policy.yml"),
        "- uses: actions/checkout@v6",
        "installed and verified against ${verified}",
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

/// The project state and policy texts; a proven missing file is `None`.
struct PinInputs {
    state: Option<String>,
    policy: Option<String>,
}

fn current_inputs(root: &Path) -> Result<PinInputs, String> {
    let read = |name: &str| -> Result<Option<String>, String> {
        let path = root.join(name);
        // Only a proven missing input is absent; a dangling link is read
        // and refused.
        if crate::absence::proven_absent(&path).map_err(|error| format!("{name}: {error}"))? {
            return Ok(None);
        }
        std::fs::read_to_string(path)
            .map(Some)
            .map_err(|error| format!("{name}: {error}"))
    };
    read_inputs(read)
}

fn read_inputs(read: impl Fn(&str) -> TargetText) -> Result<PinInputs, String> {
    Ok(PinInputs {
        state: read(STATE)?,
        policy: read(POLICY)?,
    })
}

/// A report that stops at an input that cannot be read.
fn refused(error: &str) -> PinReport {
    PinReport {
        status: Status::Fail,
        message: format!("cannot read CI pin inputs: {error}"),
    }
}

/// The checkout's own pin: its state text, policy, pin and what CI checks
/// the pinned release against, or the report that stops there.
fn checkout_pin(root: &Path) -> Result<(String, Option<String>, String, String), PinReport> {
    let PinInputs { state, policy } = current_inputs(root).map_err(|error| refused(&error))?;
    // A table the installers refuse may not parse as TOML at all (one
    // declared twice), so name it before reading the pin.
    if let Some(state) = state.as_deref() {
        if let PinnedDigests::Unreadable(reason) = pinned_digests(state) {
            return Err(unreadable("", state, &reason));
        }
    }
    let pin = state
        .as_deref()
        .map(pinned)
        .transpose()
        .map_err(|error| refused(&error))?
        .flatten();
    let (Some(state), Some(pin)) = (state, pin) else {
        return Err(PinReport {
            status: Status::Warn(remedy::DOCTOR_CI_PIN_MISSING.remedy()),
            message: format!(
                "no scaffold_version is pinned in {STATE}, so the pinned CI install fails closed"
            ),
        });
    };
    let verified = digest_mode(&state, &pin).map_err(|problem| PinReport {
        status: Status::Warn(remedy::DOCTOR_CI_DIGEST.with(&[("version", pin.as_str())])),
        message: problem,
    })?;
    Ok((state, policy, pin, verified))
}

/// The pin state of the checkout at `root`.
pub(super) fn report(root: &Path) -> PinReport {
    let (head_state, head_policy, head_pin, verified) = match checkout_pin(root) {
        Ok(checked) => checked,
        Err(report) => return report,
    };
    let target_files = match target_files(root) {
        Ok(target) => target,
        Err(error) => return refused(&error),
    };
    let Some((target, show)) = target_files else {
        return PinReport {
            status: Status::Pass,
            message: format!(
                "CI installs the codeflow version the target branch pins ({head_pin} here), verified against {verified}; no local target branch to compare with"
            ),
        };
    };
    let PinInputs {
        state: target_state,
        policy: target_policy,
    } = match read_inputs(show) {
        Ok(inputs) => inputs,
        Err(error) => return refused(&error),
    };
    if let Err(error) = head_policy
        .as_deref()
        .map(policy_keys)
        .transpose()
        .and_then(|_| target_policy.as_deref().map(policy_keys).transpose())
    {
        return refused(&error);
    }
    let (target_text, target_pin, target_verified) =
        match target_check(&target, target_state.as_deref()) {
            Ok(checked) => checked,
            Err(report) => return report,
        };
    if target_pin == head_pin {
        return same_pin(
            &target,
            &target_pin,
            (target_text, &target_verified),
            (&head_state, &verified),
        );
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
        Some(head_state.as_str()),
        target_state.as_deref(),
        head_policy.as_deref(),
        target_policy.as_deref(),
    );
    let carried = match carried {
        Ok(carried) => carried,
        Err(error) => return refused(&error),
    };
    if carried.is_empty() {
        return PinReport {
            status: Status::Pass,
            message: format!(
                "this checkout raises scaffold_version from {target_pin} ({target}) to {head_pin} and nothing else, upgrade step one: codeflow {target_pin}, verified against {target_verified}, judges it and CI tests {head_pin} alongside; after it lands, run `codeflow update` on a new branch"
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

/// What the installers check a download of `version` against, read from the
/// project state's `[scaffold_sha256]` table (sathyassn/codeflow#47); `Err`
/// names a table the installers refuse, so CI fails closed.
fn digest_mode(state: &str, version: &str) -> Result<String, String> {
    match pinned_digests(state) {
        PinnedDigests::Unreadable(reason) => Err(format!(
            "the [{TABLE}] table in {STATE} {reason}, so the CI install fails closed"
        )),
        PinnedDigests::Absent => Ok(format!(
            "its sha256.sum only: no release digest is pinned beside it (`codeflow update --pin {version}` pins one)"
        )),
        PinnedDigests::Table { version: None, .. } => Err(format!(
            "the [{TABLE}] table in {STATE} names no version the CI installers can read (they read `version = \"<version>\"`), so the CI install fails closed"
        )),
        PinnedDigests::Table {
            version: Some(table),
            ..
        } if table != version => Err(format!(
            "the [{TABLE}] table in {STATE} pins the digests of codeflow {table}, not {version}, so the CI install fails closed"
        )),
        PinnedDigests::Table { missing, .. } if !missing.is_empty() => Err(format!(
            "the [{TABLE}] table in {STATE} lists no digest for {}, so the CI install on that platform fails closed",
            missing.join(", ")
        )),
        PinnedDigests::Table { .. } => Ok(format!(
            "the release digests pinned in {STATE} and its sha256.sum"
        )),
    }
}

/// The warning for a state whose digest table the installers refuse: the
/// line they stop at, and the edit that clears it (`codeflow update --pin`
/// keeps the project's own lines, so it cannot).
fn unreadable(prefix: &str, state: &str, reason: &str) -> PinReport {
    let (number, line) = unread_line(state).unwrap_or((0, "an unnamed line".to_string()));
    PinReport {
        status: Status::Warn(
            remedy::DOCTOR_CI_DIGEST_LINE.with(&[("line", number.to_string().as_str())]),
        ),
        message: format!(
            "{prefix}the [{TABLE}] table in {STATE} {reason} at {line}, so the CI install fails closed"
        ),
    }
}

/// The target's pin and what CI checks its release against, or the warning
/// that stops the report. CI installs the target's pin first, checked against
/// the target's table, whatever the checkout raises or lowers it to.
fn target_check<'s>(
    target: &str,
    state: Option<&'s str>,
) -> Result<(&'s str, String, String), PinReport> {
    let digest_warning = |version: &str, problem: String| PinReport {
        status: Status::Warn(remedy::DOCTOR_CI_DIGEST.with(&[("version", version)])),
        message: format!("on {target}, {problem}"),
    };
    let no_pin = || PinReport {
        status: Status::Warn(remedy::DOCTOR_CI_PIN_TARGET.with(&[("target", target)])),
        message: format!(
            "{target} pins no scaffold_version, so CI on it fails closed until a pin lands there"
        ),
    };
    // A target without the state file pins nothing.
    let Some(state) = state else {
        return Err(no_pin());
    };
    // A table the installers refuse may not parse as TOML at all (one
    // declared twice), so name it before reading the pin.
    if let PinnedDigests::Unreadable(reason) = pinned_digests(state) {
        return Err(unreadable(&format!("on {target}, "), state, &reason));
    }
    let pin = match pinned(state) {
        Ok(Some(pin)) => pin,
        Ok(None) => return Err(no_pin()),
        Err(error) => {
            return Err(PinReport {
                status: Status::Fail,
                message: format!("cannot read CI pin inputs: on {target}, {error}"),
            })
        }
    };
    match digest_mode(state, &pin) {
        Ok(verified) => Ok((state, pin, verified)),
        Err(problem) => Err(digest_warning(&pin, problem)),
    }
}

/// The report when the checkout keeps the target's pin. CI reads the digests
/// from the target, so the target's table is what protects the install; the
/// checkout's (`head_verified`) applies once it lands.
fn same_pin(
    target: &str,
    pin: &str,
    (target_state, target_verified): (&str, &str),
    (head_state, head_verified): (&str, &str),
) -> PinReport {
    let changed = if table_lines(target_state) == table_lines(head_state) {
        String::new()
    } else {
        format!(
            "; this checkout changes the [{TABLE}] table, which CI checks once it lands on {target} (then: {head_verified})"
        )
    };
    PinReport {
        status: Status::Pass,
        message: format!(
            "CI installs codeflow {pin}, the version {target} pins, verified against {target_verified}{changed}"
        ),
    }
}

/// The lines of a state's `[scaffold_sha256]` section, trimmed, without
/// blanks and comments: equal lines pin the same digests. The header is
/// recognized as the installers recognize it.
fn table_lines(state: &str) -> Vec<&str> {
    let mut inside = false;
    state
        .lines()
        .map(|line| line.trim_matches([' ', '\t', '\x0B', '\x0C']))
        .filter(|line| {
            if line.starts_with('[') {
                inside = is_table_header(line);
                return false;
            }
            inside && !line.is_empty() && !line.starts_with('#')
        })
        .collect()
}

/// The project-owned setup hook the managed CI sources before the test gate
/// (sathyassn/codeflow#46).
pub(super) const SETUP_HOOK: &str = ".codeflow/ci-setup.sh";

/// The line of a shipped CI file that sources [`SETUP_HOOK`].
const SETUP_LINE: &str = ". ./.codeflow/ci-setup.sh";

/// Whether the CI file `content` sources [`SETUP_HOOK`]: a line that is
/// [`SETUP_LINE`] with only spaces or tabs around it, as the shell reads it.
fn sources_setup_hook(content: &str) -> bool {
    content
        .lines()
        .any(|line| line.trim_matches([' ', '\t']) == SETUP_LINE)
}

/// Whether the project has a setup hook, whether the CI file `dest` (with
/// `content`) sources it, and its first command.
pub(super) fn setup_note(root: &Path, dest: &str, content: &str) -> String {
    // Only a proven missing hook is "no hook"; one that cannot be read is
    // named as such, since CI still sources whatever the file holds.
    let path = root.join(SETUP_HOOK);
    let script = match crate::absence::proven_absent(&path) {
        Ok(true) => return format!("no project setup hook ({SETUP_HOOK})"),
        Ok(false) => std::fs::read_to_string(&path),
        Err(error) => Err(error),
    };
    let script = match script {
        Ok(script) => script,
        Err(error) => {
            return format!(
                "the project setup hook {SETUP_HOOK} cannot be read ({error}), so what CI runs before `codeflow test` is unknown"
            )
        }
    };
    let first = script
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'));
    let what = first.map_or_else(
        || "it holds no command".to_string(),
        |command| {
            // The hook comes from the checkout, which may be untrusted: show
            // control characters escaped so they never reach the terminal.
            let shown: String = command
                .chars()
                .take(60)
                .map(|c| {
                    if c.is_control() {
                        c.escape_default().to_string()
                    } else {
                        c.to_string()
                    }
                })
                .collect();
            let more = if command.chars().count() > 60 {
                "..."
            } else {
                ""
            };
            format!("first command `{shown}{more}`")
        },
    );
    if sources_setup_hook(content) {
        format!("project setup hook {SETUP_HOOK} runs before `codeflow test` ({what})")
    } else {
        format!(
            "project setup hook {SETUP_HOOK} is present ({what}), but {dest} does not source it before `codeflow test`"
        )
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
    let Some(repo) = crate::hooks::repo::open(root)? else {
        return Ok(None);
    };
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

    #[test]
    fn r17_target_files_without_repository_are_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(target_files(dir.path()).unwrap().is_none());
        git2::Repository::init(dir.path()).unwrap();
        std::fs::remove_file(dir.path().join(".git/HEAD")).unwrap();
        assert!(target_files(dir.path()).is_err());
    }

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

    const DIGEST: &str = "117f6fa832677ab63b87f3b8c01f996e5e6c427848cde2003a6885656ce73384";

    /// `state(version)` with a `[scaffold_sha256]` table for `table` and a
    /// digest for each of `triples`.
    fn with_digests(version: &str, table: &str, triples: &[&str]) -> String {
        let mut text = format!(
            "{}\n[scaffold_sha256]\nversion = \"{table}\"\n",
            state(version)
        );
        for triple in triples {
            text.push_str(triple);
            text.push_str(" = \"");
            text.push_str(DIGEST);
            text.push_str("\"\n");
        }
        text
    }

    /// sathyassn/codeflow#47: doctor names the verification mode the CI
    /// install uses, and warns when the pinned table would fail it closed.
    #[test]
    fn the_report_names_the_digest_mode() {
        let dir = project("1.2.3");
        let unpinned = report(dir.path());
        assert_eq!(unpinned.status, Status::Pass, "{}", unpinned.message);
        assert!(
            unpinned
                .message
                .contains("verified against its sha256.sum only: no release digest is pinned"),
            "{}",
            unpinned.message
        );
        assert!(unpinned.message.contains("codeflow update --pin 1.2.3"));

        write(
            dir.path(),
            STATE,
            &with_digests("1.2.3", "1.2.3", &crate::scaffold::release_pin::TRIPLES),
        );
        // Main pins none, so the checkout's table applies once it lands.
        let pinned = report(dir.path());
        assert_eq!(pinned.status, Status::Pass, "{}", pinned.message);
        assert!(
            pinned.message.contains(
                "(then: the release digests pinned in .codeflow/project.toml and its sha256.sum)"
            ),
            "{}",
            pinned.message
        );

        write(
            dir.path(),
            STATE,
            &with_digests("1.2.3", "1.2.2", &crate::scaffold::release_pin::TRIPLES),
        );
        let stale = report(dir.path());
        assert!(stale.status.is_warn(), "{}", stale.message);
        assert!(
            stale.message.contains(
                "pins the digests of codeflow 1.2.2, not 1.2.3, so the CI install fails closed"
            ),
            "{}",
            stale.message
        );
        let Status::Warn(remedy) = &stale.status else {
            panic!("{}", stale.message)
        };
        assert!(remedy.contains("codeflow update --pin 1.2.3"), "{remedy}");

        write(
            dir.path(),
            STATE,
            &with_digests("1.2.3", "1.2.3", &["x86_64-unknown-linux-gnu"]),
        );
        let partial = report(dir.path());
        assert!(partial.status.is_warn(), "{}", partial.message);
        assert!(
            partial
                .message
                .contains("lists no digest for aarch64-apple-darwin, x86_64-apple-darwin"),
            "{}",
            partial.message
        );
    }

    /// sathyassn/codeflow#46: doctor says whether the setup hook exists,
    /// whether the CI file sources it, and what it runs first.
    #[test]
    fn the_setup_note_names_the_hook_and_whether_ci_runs_it() {
        let dir = tempfile::tempdir().unwrap();
        let ci = include_str!("../../../../assets/base/ci/codeflow-ci.yml");
        assert_eq!(
            setup_note(dir.path(), "ci.yml", ci),
            "no project setup hook (.codeflow/ci-setup.sh)"
        );
        write(
            dir.path(),
            SETUP_HOOK,
            "#!/bin/sh\n# Node for the gate\n\ncorepack enable\npnpm install --frozen-lockfile\n",
        );
        for shipped in [
            ci,
            include_str!("../../../../assets/base/ci/ci-generic.sh"),
            include_str!("../../../../assets/base/ci/.gitlab-ci.yml"),
            include_str!("../../../../assets/base/ci/bitbucket-pipelines.yml"),
        ] {
            assert_eq!(
                setup_note(dir.path(), "ci.yml", shipped),
                "project setup hook .codeflow/ci-setup.sh runs before `codeflow test` (first command `corepack enable`)"
            );
        }
        assert_eq!(
            setup_note(dir.path(), "ci.yml", "jobs: {}\n"),
            "project setup hook .codeflow/ci-setup.sh is present (first command `corepack enable`), but ci.yml does not source it before `codeflow test`"
        );
        write(
            dir.path(),
            SETUP_HOOK,
            &format!("echo {}\n", "x".repeat(80)),
        );
        assert!(
            setup_note(dir.path(), "ci.yml", ci).contains(&format!("`echo {}...`", "x".repeat(55)))
        );
        write(dir.path(), SETUP_HOOK, "# nothing yet\n");
        assert!(setup_note(dir.path(), "ci.yml", ci).contains("(it holds no command)"));
        // A hook from an untrusted checkout never sends control characters
        // to the terminal.
        write(dir.path(), SETUP_HOOK, "echo \u{1b}[2Jcleared\u{7}\n");
        let note = setup_note(dir.path(), "ci.yml", ci);
        assert!(note.contains("`echo \\u{1b}[2Jcleared\\u{7}`"), "{note}");
        assert!(!note.chars().any(char::is_control), "{note}");
    }

    /// Issue 79: a setup hook that cannot be read is named as such, never
    /// reported as no hook, since CI still sources whatever it holds.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_setup_hook_is_not_reported_missing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let ci = include_str!("../../../../assets/base/ci/codeflow-ci.yml");
        write(dir.path(), SETUP_HOOK, "corepack enable\n");
        let hook = dir.path().join(SETUP_HOOK);
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o000)).unwrap();
        let note = setup_note(dir.path(), "ci.yml", ci);
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(note.contains("cannot be read"), "{note}");
        assert!(!note.starts_with("no project setup hook"), "{note}");
    }

    /// Commits `state` on `main` of `dir` and returns to `feat/x` at it.
    fn commit_on_main(dir: &Path, state: &str) {
        git(dir, &["checkout", "-q", "main"]);
        write(dir, STATE, state);
        git(dir, &["add", "-A"]);
        git(
            dir,
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-q",
                "-m",
                "chore: pin",
            ],
        );
        git(dir, &["checkout", "-q", "feat/x"]);
        git(dir, &["merge", "-q", "--ff-only", "main"]);
    }

    /// A digest table for `version` listing every triple.
    fn table(version: &str) -> String {
        let digest = "a".repeat(64);
        format!(
            "\n[scaffold_sha256]\nversion = \"{version}\"\naarch64-apple-darwin = \"{digest}\"\nx86_64-apple-darwin = \"{digest}\"\nx86_64-unknown-linux-gnu = \"{digest}\"\n"
        )
    }

    #[test]
    fn the_report_names_the_targets_digest_mode_not_the_checkouts() {
        // The target pins no digests: a table added in the checkout does
        // not protect CI until it lands.
        let dir = project("1.2.3");
        write(
            dir.path(),
            STATE,
            &format!("{}{}", state("1.2.3"), table("1.2.3")),
        );
        let found = report(dir.path());
        assert_eq!(found.status, Status::Pass, "{}", found.message);
        assert!(
            found.message.contains("verified against its sha256.sum only")
                && found.message.contains(
                    "this checkout changes the [scaffold_sha256] table, which CI checks once it lands on main (then: the release digests pinned"
                ),
            "{}",
            found.message
        );

        // The target pins them and the checkout keeps them.
        commit_on_main(dir.path(), &format!("{}{}", state("1.2.3"), table("1.2.3")));
        let found = report(dir.path());
        assert_eq!(found.status, Status::Pass, "{}", found.message);
        assert!(
            found.message.ends_with(&format!(
                "verified against the release digests pinned in {STATE} and its sha256.sum"
            )),
            "{}",
            found.message
        );

        // A broken table on the target warns even when the checkout's is fine.
        let dir = project("1.2.3");
        commit_on_main(dir.path(), &format!("{}{}", state("1.2.3"), table("1.2.2")));
        write(
            dir.path(),
            STATE,
            &format!("{}{}", state("1.2.3"), table("1.2.3")),
        );
        let found = report(dir.path());
        assert!(matches!(found.status, Status::Warn(_)), "{}", found.message);
        assert!(
            found.message.starts_with(&format!(
                "on main, the [scaffold_sha256] table in {STATE} pins the digests of codeflow 1.2.2, not 1.2.3"
            )),
            "{}",
            found.message
        );
    }

    /// A header the installers read with tabs or a trailing comment holding
    /// `]` is the table's header for the change note too.
    #[test]
    fn a_changed_table_is_noted_whatever_header_form_the_installers_read() {
        for header in [
            "[scaffold_sha256] # reviewed [digests]",
            "\t[\tscaffold_sha256\t]\t",
        ] {
            let dir = project("1.2.3");
            let pinned = table("1.2.3").replace("[scaffold_sha256]", header);
            commit_on_main(dir.path(), &format!("{}{pinned}", state("1.2.3")));
            let changed = pinned.replacen(&"a".repeat(64), &"b".repeat(64), 1);
            write(dir.path(), STATE, &format!("{}{changed}", state("1.2.3")));
            let found = report(dir.path());
            assert_eq!(found.status, Status::Pass, "{header}: {}", found.message);
            assert!(
                found
                    .message
                    .contains("this checkout changes the [scaffold_sha256] table"),
                "{header}: {}",
                found.message
            );
        }
    }

    #[test]
    fn a_raise_is_judged_against_the_targets_digest_table() {
        // CI installs the target's pin first, so a stale, partial or
        // unreadable table on the target warns even when the checkout
        // raises the pin with a good one.
        for broken in [
            table("1.2.2"),
            "\n[scaffold_sha256]\nversion = \"1.2.3\"\n".to_string(),
            format!("{}{}", table("1.2.3"), table("1.2.3")),
        ] {
            let dir = project("1.2.3");
            commit_on_main(dir.path(), &format!("{}{broken}", state("1.2.3")));
            write(
                dir.path(),
                STATE,
                &format!("{}{}", state("1.2.4"), table("1.2.4")),
            );
            let found = report(dir.path());
            assert!(matches!(found.status, Status::Warn(_)), "{}", found.message);
            assert!(
                found
                    .message
                    .starts_with("on main, the [scaffold_sha256] table in")
                    && found.message.ends_with("fails closed"),
                "{}",
                found.message
            );
        }
        // A good target table is named in the raise.
        let dir = project("1.2.3");
        commit_on_main(dir.path(), &format!("{}{}", state("1.2.3"), table("1.2.3")));
        write(
            dir.path(),
            STATE,
            &format!("{}{}", state("1.2.4"), table("1.2.4")),
        );
        let found = report(dir.path());
        assert_eq!(found.status, Status::Pass, "{}", found.message);
        assert!(
            found.message.contains(
                "upgrade step one: codeflow 1.2.3, verified against the release digests pinned"
            ),
            "{}",
            found.message
        );
    }

    #[test]
    fn an_unreadable_table_warns_that_ci_fails_closed() {
        let dir = project("1.2.3");
        for (text, reason) in [
            (
                format!(
                    "{}scaffold_sha256 = {{ version = \"1.2.3\" }}\n",
                    state("1.2.3")
                ),
                "is written in a form the CI installers do not read",
            ),
            (
                format!("{}{}{}", state("1.2.3"), table("1.2.3"), table("1.2.3")),
                "is declared twice",
            ),
            // A byte-order mark makes the file invalid TOML; doctor still
            // names the line the installers refuse.
            (
                format!("\u{feff}{}{}", table("1.2.3").trim_start(), state("1.2.3")),
                "may be hidden by a line that starts with a character the CI installers do not read",
            ),
        ] {
            write(dir.path(), STATE, &text);
            let found = report(dir.path());
            assert!(matches!(found.status, Status::Warn(_)), "{}", found.message);
            assert!(
                found
                    .message
                    .starts_with(&format!("the [scaffold_sha256] table in {STATE} {reason}"))
                    && found.message.ends_with("so the CI install fails closed"),
                "{}",
                found.message
            );
        }
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
