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

use codeflow_core::workgraph::classify::{path_sets, ProjectPaths};

use super::is_docs_path;

/// One path the range changes, with its Git mode before and after
/// (`000000` on the side where the path does not exist).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RangeEntry {
    pub path: String,
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
/// detection so a rename is its deletion and its addition. Any failure is
/// `None`, which the caller reads as unknown, never as an empty range.
pub(super) fn range_inventory(root: &Path, base: &str, head: &str) -> Option<Vec<RangeEntry>> {
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
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_raw(&out.stdout)
}

/// Parse `git diff --raw -z --no-renames`: each change is a
/// `:<old mode> <new mode> <old id> <new id> <status>` field followed by one
/// path field. Anything else is malformed and yields `None`.
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
        // OS text rule (issue 79): the path is matched against policy globs
        // and prefixes, which need text, and a lossy spelling can match a
        // pattern the real bytes do not (`docs/caf[!x].md`), which would
        // lighten the checks. A path that is not valid UTF-8 makes the
        // inventory unknown, which reads as code, the conservative direction.
        let path = std::str::from_utf8(path).ok()?;
        entries.push(RangeEntry {
            path: path.to_string(),
            old_mode: old_mode.to_string(),
            new_mode: new_mode.to_string(),
        });
    }
    Some(entries)
}

/// The project paths the shared path sets read: the checkout's effective
/// policy (or its stack default), widened by the target side's own
/// `git.product_paths` and `git.breaking_watch_paths` at `base`, so neither
/// side can narrow the surfaces the other names.
pub(super) fn project_paths(root: &Path, base: &str) -> ProjectPaths {
    let mut project = ProjectPaths::load(root);
    let target = codeflow_core::git::command()
        .arg("-C")
        .arg(root)
        .args(["show", &format!("{base}:.codeflow/policy.json")])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| serde_json::from_slice::<serde_json::Value>(&out.stdout).ok());
    if let Some(policy) = target {
        let list = |key: &str| -> Vec<String> {
            policy["git"][key]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        project.product.extend(list("product_paths"));
        project.watched.extend(list("breaking_watch_paths"));
    }
    project
}

/// The class of a range from its inventory. `None` (unlisted) and an empty
/// inventory are code, the conservative direction.
pub(super) fn classify(entries: Option<&[RangeEntry]>, project: &ProjectPaths) -> ChangeClass {
    let Some(entries) = entries.filter(|entries| !entries.is_empty()) else {
        return ChangeClass::CODE;
    };
    let docs_only = entries
        .iter()
        .all(|entry| regular(entry) && is_docs_path(&entry.path));
    ChangeClass {
        docs_only,
        light: docs_only
            && entries
                .iter()
                .all(|entry| is_light_path(&entry.path, project)),
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
            path: path.to_string(),
            old_mode: old_mode.to_string(),
            new_mode: new_mode.to_string(),
        }
    }

    /// Issue 79: a path that is not valid UTF-8 makes the inventory unknown (so
    /// the change reads as code), and is never matched as its lossy spelling.
    #[test]
    fn a_raw_path_that_is_not_utf8_makes_the_inventory_unknown() {
        let valid = b":100644 100644 aaaa bbbb M\0docs/a.md\0";
        assert_eq!(parse_raw(valid).map(|entries| entries.len()), Some(1));
        let odd = b":100644 100644 aaaa bbbb M\0docs/caf\xe9.md\0";
        assert!(parse_raw(odd).is_none());
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
