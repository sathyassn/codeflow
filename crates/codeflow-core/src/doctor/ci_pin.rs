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

use crate::scaffold::release_pin::{pinned_digests, PinnedDigests, TABLE};
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
    if let Some(PinnedDigests::Unreadable(reason)) = head_state.as_deref().map(pinned_digests) {
        let version = head_state.as_deref().and_then(pinned);
        return PinReport {
            status: Status::Warn(
                remedy::DOCTOR_CI_DIGEST
                    .with(&[("version", version.as_deref().unwrap_or("<version>"))]),
            ),
            message: format!(
                "the [{TABLE}] table in {STATE} {reason}, so the CI install fails closed"
            ),
        };
    }
    let Some(head_pin) = head_state.as_deref().and_then(pinned) else {
        return PinReport {
            status: Status::Warn(remedy::DOCTOR_CI_PIN_MISSING.remedy()),
            message: format!(
                "no scaffold_version is pinned in {STATE}, so the pinned CI install fails closed"
            ),
        };
    };
    let verified = match digest_mode(head_state.as_deref().unwrap_or_default(), &head_pin) {
        Ok(verified) => verified,
        Err(problem) => {
            return PinReport {
                status: Status::Warn(
                    remedy::DOCTOR_CI_DIGEST.with(&[("version", head_pin.as_str())]),
                ),
                message: problem,
            }
        }
    };
    let Some((target, show)) = target_files(root) else {
        return PinReport {
            status: Status::Pass,
            message: format!(
                "CI installs the codeflow version the target branch pins ({head_pin} here), verified against {verified}; no local target branch to compare with"
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
        return same_pin(
            &target,
            &target_pin,
            &show(STATE).unwrap_or_default(),
            head_state.as_deref().unwrap_or_default(),
            &verified,
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

/// The report when the checkout keeps the target's pin. CI reads the digests
/// from the target, so the target's table is what protects the install; the
/// checkout's (`head_verified`) applies once it lands.
fn same_pin(
    target: &str,
    pin: &str,
    target_state: &str,
    head_state: &str,
    head_verified: &str,
) -> PinReport {
    let target_verified = match digest_mode(target_state, pin) {
        Ok(verified) => verified,
        Err(problem) => {
            return PinReport {
                status: Status::Warn(remedy::DOCTOR_CI_DIGEST.with(&[("version", pin)])),
                message: format!("on {target}, {problem}"),
            }
        }
    };
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
/// blanks and comments: equal lines pin the same digests.
fn table_lines(state: &str) -> Vec<&str> {
    let mut inside = false;
    state
        .lines()
        .map(str::trim)
        .filter(|line| {
            if line.starts_with('[') {
                inside = line.trim_end_matches(|c: char| c != ']').replace(' ', "")
                    == format!("[{TABLE}]");
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

/// Whether the project has a setup hook, whether the CI file `dest` (with
/// `content`) sources it, and its first command.
pub(super) fn setup_note(root: &Path, dest: &str, content: &str) -> String {
    let Ok(script) = std::fs::read_to_string(root.join(SETUP_HOOK)) else {
        return format!("no project setup hook ({SETUP_HOOK})");
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
    if content.lines().any(|line| line.trim() == SETUP_LINE) {
        format!("project setup hook {SETUP_HOOK} runs before `codeflow test` ({what})")
    } else {
        format!(
            "project setup hook {SETUP_HOOK} is present ({what}), but {dest} does not source it before `codeflow test`"
        )
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
        let shown = resolved
            .shorthand()
            .map_or_else(|_| (*name).to_string(), str::to_string);
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
        assert_eq!(carried, vec![format!("a new {STATE} schema_version")]);
    }
}
