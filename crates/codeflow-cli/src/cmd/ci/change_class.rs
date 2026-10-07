//! The change class a pull request body is checked against (TSK-135).
//!
//! One checked, NUL-delimited tree diff from the merge-base to the head
//! lists every path the range changes, merge resolutions and deletions
//! included, a rename as both its sides, with both Git modes. From it the
//! range is:
//!
//! - light: every path is a regular Markdown file under `docs/` or
//!   `project-management/`, outside every adopter-facing and direct-change
//!   floor member of the shared path sets (SPC-013 R-70, R-114), with the
//!   target policy's product and watched contract paths; its body needs only
//!   Summary and Changes and may omit Release impact;
//! - docs-only: every path is a regular documentation file; its body skips
//!   the code sections (Testing);
//! - code: anything else, and every range whose paths could not be listed.

use std::path::Path;

use codeflow_core::git::GitName;
use codeflow_core::workgraph::classify::{path_sets, ProjectPaths};

use super::is_docs_path;

/// One path the range changes, as git's exact bytes, with its Git mode
/// before and after (`000000` on the side where the path does not exist).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RangeEntry {
    pub path: GitName,
    pub old_mode: String,
    pub new_mode: String,
}

/// What the range's paths allow the PR body to leave out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChangeClass {
    /// Only documentation: the code sections are not required.
    pub docs_only: bool,
    /// Only documentation and planning Markdown outside every contract
    /// surface: only Summary and Changes are required, and an absent
    /// Release impact reads as none.
    pub light: bool,
}

impl ChangeClass {
    /// A code range, and every range whose paths are unknown.
    pub const CODE: Self = Self {
        docs_only: false,
        light: false,
    };
}

/// Every path the range changes, from the merge-base of `base` and `head` to
/// `head`: one raw tree diff, NUL-delimited and unquoted, without rename
/// detection so a rename is its deletion and its addition. Obtaining failures
/// return a cannot-read error that the CI entry point refuses.
pub(super) fn range_inventory(
    root: &Path,
    base: &str,
    head: &str,
) -> Result<Vec<RangeEntry>, String> {
    let out = codeflow_core::git::command()
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "--raw",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            &format!("{base}...{head}"),
        ])
        .output()
        .map_err(|error| format!("cannot read changed paths: {error}"))?;
    if !out.status.success() {
        return Err(format!("cannot read changed paths: {}", out.status));
    }
    parse_raw(&out.stdout).ok_or_else(|| "cannot read raw changed-path inventory".to_string())
}

/// Parse `git diff --raw -z --no-renames`: each change is a
/// `:<old mode> <new mode> <old id> <new id> <status>` field followed by one
/// path field, kept as git's exact bytes. Anything else is unproven:
/// `range_inventory` maps None to a cannot-read error, which `ci::run`
/// refuses.
fn parse_raw(stdout: &[u8]) -> Option<Vec<RangeEntry>> {
    let mut fields = stdout.split(|byte| *byte == 0);
    let mut entries = Vec::new();
    while let Some(meta) = fields.next() {
        if meta.is_empty() {
            // Only the trailing terminator may be empty.
            return fields.all(<[u8]>::is_empty).then_some(entries);
        }
        let meta = std::str::from_utf8(meta.strip_prefix(b":")?).ok()?;
        let mut parts = meta.split(' ');
        let (old_mode, new_mode) = (parts.next()?, parts.next()?);
        if parts.count() != 3 {
            return None;
        }
        let path = fields.next().filter(|path| !path.is_empty())?;
        // OS text rule (issue 79): a successfully listed path is part of the
        // inventory whatever its bytes. `classify` judges a path that is not
        // valid UTF-8 the strict way (code, never light), since policy globs
        // and prefixes need text and a lossy spelling could match a pattern
        // the real bytes do not (`docs/caf[!x].md`).
        entries.push(RangeEntry {
            path: GitName::from_bytes(path),
            old_mode: old_mode.to_string(),
            new_mode: new_mode.to_string(),
        });
    }
    Some(entries)
}

/// The project paths the shared path sets read: the checkout's effective
/// policy (or its stack default), widened by the target side's own
/// `git.product_paths` and `git.breaking_watch_paths` at `base`, so neither
/// side can narrow the surfaces the other names. The target side is read
/// strictly: a present key that is not an array of strings, or a glob that
/// does not parse, refuses; an absent key adds nothing.
pub(super) fn project_paths(root: &Path, base: &str) -> Result<ProjectPaths, String> {
    let mut project = ProjectPaths::load(root)?;
    let target = codeflow_core::hooks::landed_policy::policy_text_at(root, base)?
        .map(|text| {
            serde_json::from_str::<serde_json::Value>(&text)
                .map_err(|error| format!("cannot read target path policy: {error}"))
        })
        .transpose()?;
    if let Some(policy) = target {
        let target = ProjectPaths {
            product: target_globs(&policy, "product_paths")?,
            watched: target_globs(&policy, "breaking_watch_paths")?,
        };
        target
            .validate()
            .map_err(|error| format!("cannot read target path policy: {error}"))?;
        project.product.extend(target.product);
        project.watched.extend(target.watched);
    }
    Ok(project)
}

/// One of the target policy's `git` path lists. Absent adds nothing, as
/// does `git.product_paths: null`, which the policy reads as the stack
/// default; any other value that is not an array of strings refuses.
fn target_globs(policy: &serde_json::Value, key: &str) -> Result<Vec<String>, String> {
    use serde_json::Value;
    let unreadable = |what: &str| format!("cannot read target path policy: {what}");
    let Value::Object(policy) = policy else {
        return Err(unreadable("the policy is not a JSON object"));
    };
    let git = match policy.get("git") {
        None => return Ok(Vec::new()),
        Some(Value::Object(git)) => git,
        Some(_) => return Err(unreadable("git is not an object")),
    };
    match git.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Null) if key == "product_paths" => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str().map(str::to_string).ok_or_else(|| {
                    unreadable(&format!("git.{key} holds a value that is not a string"))
                })
            })
            .collect(),
        Some(_) => Err(unreadable(&format!("git.{key} is not an array of strings"))),
    }
}

/// The class of a range from its inventory. `None` (unlisted) and an empty
/// inventory are code, the conservative direction, and so is a range with a
/// path that is not valid UTF-8: no prose or light rule can read its name.
pub(super) fn classify(entries: Option<&[RangeEntry]>, project: &ProjectPaths) -> ChangeClass {
    let Some(entries) = entries.filter(|entries| !entries.is_empty()) else {
        return ChangeClass::CODE;
    };
    let docs_only = entries
        .iter()
        .all(|entry| regular(entry) && entry.path.rule_text().is_ok_and(|path| is_docs_path(path)));
    ChangeClass {
        docs_only,
        light: docs_only
            && entries.iter().all(|entry| {
                entry
                    .path
                    .rule_text()
                    .is_ok_and(|path| is_light_path(path, project))
            }),
    }
}

/// Both sides are a regular, non-executable file or absent: a gitlink, a
/// symlink or an executable is never prose, whatever its name.
fn regular(entry: &RangeEntry) -> bool {
    [&entry.old_mode, &entry.new_mode]
        .iter()
        .all(|mode| matches!(mode.as_str(), "000000" | "100644"))
}

/// A Markdown file under `docs/` or `project-management/`, in no hidden
/// directory, and outside every member of the shared path sets: the
/// adopter-facing surfaces (product and watched contract paths, hook
/// sources, shipped templates, managed instructions, CI workflows) and the
/// record schema and templates.
fn is_light_path(path: &str, project: &ProjectPaths) -> bool {
    (path.starts_with("docs/") || path.starts_with("project-management/"))
        && Path::new(path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        && !path.split('/').any(|part| part.starts_with('.'))
        && path_sets().adopter_facing_member(path, project).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, old_mode: &str, new_mode: &str) -> RangeEntry {
        RangeEntry {
            path: GitName::from_text(path),
            old_mode: old_mode.to_string(),
            new_mode: new_mode.to_string(),
        }
    }

    /// Issue 79: a listed path that is not valid UTF-8 stays in the
    /// inventory as its exact bytes, and its range reads as code: it is never
    /// matched as its lossy spelling, and never makes the inventory unproven.
    #[test]
    fn a_raw_path_that_is_not_utf8_is_listed_and_judged_as_code() {
        let project = ProjectPaths::default();
        let valid = b":100644 100644 aaaa bbbb M\0docs/a.md\0";
        let entries = parse_raw(valid).unwrap();
        assert!(classify(Some(&entries), &project).light);
        let odd = b":100644 100644 aaaa bbbb M\0docs/caf\xe9.md\0";
        let entries = parse_raw(odd).unwrap();
        assert_eq!(entries[0].path.bytes(), b"docs/caf\xe9.md");
        assert_eq!(classify(Some(&entries), &project), ChangeClass::CODE);
        // One odd name beside a prose file makes the whole range code.
        let mixed = b":100644 100644 aaaa bbbb M\0docs/a.md\0\
:100644 100644 aaaa bbbb M\0docs/caf\xe9.md\0";
        let entries = parse_raw(mixed).unwrap();
        assert_eq!(classify(Some(&entries), &project), ChangeClass::CODE);
    }

    /// A repository whose one commit holds `policy` as its policy file, with
    /// the file then removed from the working tree so the checkout reads the
    /// shipped default and only the target side carries the policy under
    /// test.
    fn target_policy_repo(policy: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join(".codeflow").join("policy.json");
        std::fs::create_dir(root.join(".codeflow")).unwrap();
        std::fs::write(&file, policy).unwrap();
        for args in [
            &["init", "-q"][..],
            &["add", "."],
            &["commit", "-q", "-m", "policy"],
        ] {
            let out = codeflow_core::git::command()
                .arg("-C")
                .arg(root)
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .output()
                .unwrap();
            assert!(out.status.success(), "{out:?}");
        }
        std::fs::remove_file(&file).unwrap();
        dir
    }

    /// Round 23: the target side's path lists are read strictly. A present
    /// key that is not an array of strings, an item that is not a string or
    /// a glob that does not parse refuses instead of being dropped.
    #[test]
    fn r23_target_path_policy_refuses_unreadable_lists_and_globs() {
        for (policy, expected) in [
            (
                r#"{"git": {"product_paths": ["docs/**", 5]}}"#,
                "not a string",
            ),
            (r#"{"git": {"product_paths": "docs/**"}}"#, "not an array"),
            (
                r#"{"git": {"breaking_watch_paths": {"a": 1}}}"#,
                "not an array",
            ),
            (r#"{"git": {"breaking_watch_paths": null}}"#, "not an array"),
            (
                r#"{"git": {"product_paths": ["src/["]}}"#,
                "invalid git.product_paths glob",
            ),
            (
                r#"{"git": {"breaking_watch_paths": ["docs/["]}}"#,
                "invalid git.breaking_watch_paths glob",
            ),
            (r#"{"git": 5}"#, "git is not an object"),
            ("[]", "not a JSON object"),
        ] {
            let dir = target_policy_repo(policy);
            let error = project_paths(dir.path(), "HEAD").unwrap_err();
            assert!(
                error.contains("cannot read target path policy") && error.contains(expected),
                "{policy}: {error}"
            );
        }
    }

    /// The control: an absent key or section adds nothing, a null
    /// `product_paths` reads as absent, and valid lists widen the project.
    #[test]
    fn r23_target_path_policy_keeps_absent_keys_and_valid_globs() {
        let base = project_paths(target_policy_repo("{}").path(), "HEAD").unwrap();
        for policy in [r#"{"git": {}}"#, r#"{"git": {"product_paths": null}}"#] {
            let paths = project_paths(target_policy_repo(policy).path(), "HEAD").unwrap();
            assert_eq!(paths.product, base.product, "{policy}");
            assert_eq!(paths.watched, base.watched, "{policy}");
        }
        let policy = r#"{"git": {"product_paths": ["docs/runtime/**"], "breaking_watch_paths": ["docs/api.md"]}}"#;
        let paths = project_paths(target_policy_repo(policy).path(), "HEAD").unwrap();
        assert!(paths.product.iter().any(|path| path == "docs/runtime/**"));
        assert!(paths.watched.iter().any(|path| path == "docs/api.md"));
    }

    fn class(paths: &[&str], project: &ProjectPaths) -> ChangeClass {
        let entries: Vec<_> = paths
            .iter()
            .map(|path| entry(path, "100644", "100644"))
            .collect();
        classify(Some(&entries), project)
    }

    #[test]
    fn raw_output_parses_with_modes_and_unquoted_names() {
        let raw = b":000000 100644 0000000 1111111 A\0docs/caf\xc3\xa9.md\0\
:100644 000000 2222222 0000000 D\0src/app.rs\0\
:000000 160000 0000000 3333333 A\0docs/a\"b.md\0";
        let entries = parse_raw(raw).unwrap();
        assert_eq!(entries[0], entry("docs/café.md", "000000", "100644"));
        assert_eq!(entries[1], entry("src/app.rs", "100644", "000000"));
        assert_eq!(entries[2], entry("docs/a\"b.md", "000000", "160000"));
        assert_eq!(parse_raw(b"").unwrap(), Vec::new());
        assert_eq!(parse_raw(b"garbage\0path\0"), None);
        assert_eq!(parse_raw(b":100644 100644 a b M\0"), None);
    }

    #[test]
    fn light_is_markdown_under_docs_or_records_outside_every_path_set() {
        let project = ProjectPaths {
            product: vec!["src/**".into(), "docs/runtime/**".into()],
            watched: vec!["docs/api/contract.md".into()],
        };
        for path in [
            "docs/guide.md",
            "docs/a b.md",
            "docs/café.md",
            "docs/a\"b.md",
            "docs/plan/v2/plan.md",
            "project-management/tasks/TSK-001.md",
        ] {
            assert!(class(&[path], &project).light, "{path}");
        }
        for path in [
            "src/prompt.md",
            "docs/runtime/prompt.md",
            "docs/api/contract.md",
            "docs/decisions/template.md",
            "project-management/templates/spec.md",
            ".github/pull_request_template.md",
            "requirements.txt",
            "docs/plan/runner.py",
            "docs/plan/roadmap.json",
            "project-management/tools/run.py",
            "docs/plan/.agents/guide.md",
            "assets/guide.md",
            ".agents/skills/explain/SKILL.md",
            "README.md",
            "docs/AGENTS.md",
        ] {
            assert!(!class(&[path], &project).light, "{path}");
        }
        // One contract path makes the whole range full.
        assert!(!class(&["docs/guide.md", "src/lib.rs"], &project).light);
    }

    #[test]
    fn modes_and_unknown_inventories_are_never_light() {
        let project = ProjectPaths::default();
        for (old, new) in [
            ("000000", "160000"),
            ("000000", "120000"),
            ("100644", "100755"),
        ] {
            let entries = [entry("docs/module.md", old, new)];
            assert_eq!(classify(Some(&entries), &project), ChangeClass::CODE);
        }
        let deleted = [entry("docs/old.md", "100644", "000000")];
        assert!(classify(Some(&deleted), &project).light);
        assert_eq!(classify(None, &project), ChangeClass::CODE);
        assert_eq!(classify(Some(&[]), &project), ChangeClass::CODE);
    }

    #[test]
    fn docs_only_keeps_its_wider_prose_set_without_the_light_exemption() {
        let project = ProjectPaths::default();
        let readme = class(&["README.md"], &project);
        assert!(readme.docs_only && !readme.light);
        let template = class(&["docs/decisions/template.md"], &project);
        assert!(template.docs_only && !template.light);
        assert_eq!(class(&["src/lib.rs"], &project), ChangeClass::CODE);
    }
}
