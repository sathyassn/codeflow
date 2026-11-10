//! What a change's declared inputs select, and the tests that guard the
//! paths it touches (TSK-353).
//!
//! Nothing here narrows a run. The gate's `delivery::select` still runs every
//! eligible target, because no local run record earns selection credit
//! (ADR-0089). This module reports what each target's `narrow` and
//! `input_class` declarations would select for a change, and which tests the
//! config's `guards` rules name for each touched path, so a builder can run
//! them as targeted tests.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::config::{InputClass, TestConfig};
use super::delivery::{git_output, matches, with_prerequisites};

/// One path change between a base and the working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Added, modified or untracked, or a type change.
    Changed(String),
    /// Deleted.
    Deleted(String),
    /// Renamed or copied.
    Renamed {
        /// The path before.
        from: String,
        /// The path after.
        to: String,
    },
}

impl Change {
    /// Every path the change names.
    #[must_use]
    pub fn paths(&self) -> Vec<&str> {
        match self {
            Self::Changed(path) | Self::Deleted(path) => vec![path],
            Self::Renamed { from, to } => vec![from, to],
        }
    }
}

/// The targets a change's declarations select in one mode.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclaredSelection {
    /// Set when one change selects every eligible target, with its reason.
    pub every_target: Option<String>,
    /// Each target the declarations select, with its reason.
    pub selected: BTreeMap<String, String>,
    /// Prerequisites the selected targets need that were not selected
    /// themselves.
    pub prerequisites: BTreeSet<String>,
}

impl DeclaredSelection {
    /// The selected targets and their prerequisites.
    #[must_use]
    pub fn closure(&self) -> BTreeSet<String> {
        self.selected
            .keys()
            .chain(&self.prerequisites)
            .cloned()
            .collect()
    }
}

/// What the targets' declarations select for `changes` in `mode`.
///
/// A deletion, a rename, a path in `execution.run_everything`, or a path no
/// target declares selects every eligible target. Otherwise a target with
/// no declared inputs, or one that is not `content` class, always runs, and
/// a `content` target runs when one of its declared inputs changed. Each
/// selected target's prerequisites follow.
#[must_use]
pub fn declared_selection(
    config: &TestConfig,
    mode: &str,
    changes: &[Change],
) -> DeclaredSelection {
    let eligible: Vec<_> = config
        .targets
        .iter()
        .filter(|t| t.enabled && t.modes.contains_key(mode))
        .collect();
    let every_target = changes.iter().find_map(|change| match change {
        Change::Deleted(path) => Some(format!("deleted path {path}")),
        Change::Renamed { from, to } => Some(format!("renamed path {from} to {to}")),
        Change::Changed(path) if matches(&config.execution.run_everything, path) => {
            Some(format!("shared input {path} changed"))
        }
        Change::Changed(path) if !config.targets.iter().any(|t| matches(&t.narrow, path)) => {
            Some(format!("no target declares {path}"))
        }
        Change::Changed(_) => None,
    });
    let mut selected = BTreeMap::new();
    for target in &eligible {
        let reason = if let Some(reason) = &every_target {
            Some(format!("every target: {reason}"))
        } else if target.narrow.is_empty() {
            Some("no declared inputs: always runs".to_string())
        } else if target.input_class != Some(InputClass::Content) {
            Some(match target.input_class {
                Some(class) => format!("{} input class: always runs", class_name(class)),
                None => "no input class: always runs".to_string(),
            })
        } else {
            changes
                .iter()
                .flat_map(Change::paths)
                .find(|path| matches(&target.narrow, path))
                .map(|path| format!("declared input {path} changed"))
        };
        if let Some(reason) = reason {
            selected.insert(target.name.clone(), reason);
        }
    }
    let closure = with_prerequisites(&config.targets, selected.keys().cloned().collect());
    let prerequisites = closure
        .into_iter()
        .filter(|name| !selected.contains_key(name))
        .collect();
    DeclaredSelection {
        every_target,
        selected,
        prerequisites,
    }
}

fn class_name(class: InputClass) -> &'static str {
    match class {
        InputClass::Content => "content",
        InputClass::History => "history",
        InputClass::Remote => "remote",
        InputClass::Time => "time",
        InputClass::External => "external",
    }
}

/// The tests that guard each path a change touches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuardReport {
    /// Each touched path that a `guards` rule matches, with its tests in
    /// rule order.
    pub guarded: BTreeMap<String, Vec<String>>,
    /// Touched paths no rule matches.
    pub unguarded: Vec<String>,
    /// Every named test once, in the order first named.
    pub tests: Vec<String>,
}

/// The tests the config's `guards` rules name for each path in `changes`.
#[must_use]
pub fn guards(config: &TestConfig, changes: &[Change]) -> GuardReport {
    let paths: BTreeSet<&str> = changes.iter().flat_map(Change::paths).collect();
    let mut report = GuardReport::default();
    for path in paths {
        let mut tests: Vec<String> = Vec::new();
        for rule in config.guards.iter().filter(|r| matches(&r.paths, path)) {
            for test in rule.tests.iter().filter(|t| !t.trim().is_empty()) {
                if !tests.contains(test) {
                    tests.push(test.clone());
                }
            }
        }
        if tests.is_empty() {
            report.unguarded.push(path.to_string());
            continue;
        }
        for test in &tests {
            if !report.tests.contains(test) {
                report.tests.push(test.clone());
            }
        }
        report.guarded.insert(path.to_string(), tests);
    }
    report
}

/// Parse `git diff --name-status -z --find-renames` output.
///
/// # Errors
///
/// Names a status this reader does not know, or a record cut short.
pub fn parse_name_status(output: &str) -> Result<Vec<Change>, String> {
    let mut changes = Vec::new();
    let mut fields = output.split('\0').filter(|field| !field.is_empty());
    while let Some(status) = fields.next() {
        let mut path = || {
            fields
                .next()
                .map(str::to_string)
                .ok_or_else(|| format!("change status {status} has no path"))
        };
        let change = match status.chars().next() {
            Some('A' | 'M' | 'T') => Change::Changed(path()?),
            Some('D') => Change::Deleted(path()?),
            Some('R' | 'C') => {
                let from = path()?;
                Change::Renamed { from, to: path()? }
            }
            _ => return Err(format!("unrecognized change status {status}")),
        };
        changes.push(change);
    }
    Ok(changes)
}

/// Every change from `base` to the working tree, untracked files included.
///
/// # Errors
///
/// Names a Git failure or output this reader cannot read.
pub fn changes_since(root: &Path, base: &str) -> Result<Vec<Change>, String> {
    let delta = git_output(
        root,
        &["diff", "--name-status", "-z", "--find-renames", base, "--"],
    )?;
    let mut changes = parse_name_status(&delta)?;
    let untracked = git_output(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    changes.extend(
        untracked
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(|path| Change::Changed(path.to_string())),
    );
    Ok(changes)
}

/// The commit to compare with, and where it came from: `since` when given,
/// else the merge base of `HEAD` with the declared integration target of
/// the task the branch names.
///
/// # Errors
///
/// Names why no base could be found.
pub fn comparison_base(root: &Path, since: Option<&str>) -> Result<(String, String), String> {
    let trimmed = |text: String| text.trim_end_matches('\n').to_string();
    if let Some(base) = since {
        let sha = git_output(
            root,
            &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
        )?;
        return Ok((trimmed(sha), format!("--since {base}")));
    }
    let repo = git2::Repository::discover(root).map_err(|_| "repository unavailable")?;
    if repo.head_detached().unwrap_or(true) {
        return Err("detached HEAD names no task".into());
    }
    let head = repo.head().map_err(|_| "HEAD unavailable")?;
    let branch_name = crate::git::name::reference_shorthand(&head);
    let branch = branch_name
        .rule_text()
        .map_err(|_| "branch name is not UTF-8")?;
    let task = crate::workgraph::work_start::task_id_from_branch(root, branch)
        .map_err(|error| format!("cannot resolve the task: {error}"))?
        .ok_or("the branch names no task")?;
    let target = crate::workgraph::declared_work_target(root, &task)
        .map_err(|error| format!("cannot read {task}'s line: {error}"))?
        .ok_or_else(|| format!("{task} declares no integration target"))?;
    let resolved = crate::workgraph::work_start::resolve_work_target_checked(root, Some(&target))
        .map_err(|error| format!("cannot resolve {task}'s line: {error}"))?
        .ok_or_else(|| format!("{task}'s line {target} is unavailable"))?;
    let base = git_output(root, &["merge-base", "HEAD", &resolved.target])?;
    Ok((
        trimmed(base),
        format!("merge base of HEAD with {task}'s line {}", resolved.target),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(json: serde_json::Value) -> TestConfig {
        serde_json::from_value(json).unwrap()
    }

    fn fixture() -> TestConfig {
        config(serde_json::json!({
            "schema_version": "1.0",
            "execution": {"run_everything": ["crates/**"]},
            "targets": [
                {"name": "build", "runner": "custom", "modes": {"quick": {"command": "b"}},
                 "narrow": ["crates/**"], "input_class": "content"},
                {"name": "docs", "runner": "custom", "modes": {"quick": {"command": "d"}},
                 "narrow": ["docs/**"], "input_class": "content", "requires": ["build"]},
                {"name": "lint", "runner": "custom", "modes": {"quick": {"command": "l"}},
                 "narrow": ["src/**"], "input_class": "content"},
                {"name": "remote", "runner": "custom", "modes": {"quick": {"command": "r"}},
                 "narrow": ["pkg/**"], "input_class": "remote"},
                {"name": "whole", "runner": "custom", "modes": {"quick": {"command": "w"}}},
                {"name": "full-only", "runner": "custom", "modes": {"full": {"command": "f"}},
                 "narrow": ["docs/**"], "input_class": "content"}
            ],
            "guards": [
                {"paths": ["docs/**"], "tests": ["cargo test --test links", "cargo test --test docs"]},
                {"paths": ["docs/api/**", "src/**"], "tests": ["cargo test --test links", "cargo test --test api"]}
            ]
        }))
    }

    fn changed(path: &str) -> Change {
        Change::Changed(path.into())
    }

    #[test]
    fn a_declared_input_selects_its_reader_and_prerequisites() {
        let selection = declared_selection(&fixture(), "quick", &[changed("docs/a.md")]);
        assert_eq!(selection.every_target, None);
        assert_eq!(
            selection.selected.keys().collect::<Vec<_>>(),
            ["docs", "remote", "whole"]
        );
        assert_eq!(
            selection.selected["docs"],
            "declared input docs/a.md changed"
        );
        assert_eq!(
            selection.selected["remote"],
            "remote input class: always runs"
        );
        assert_eq!(
            selection.selected["whole"],
            "no declared inputs: always runs"
        );
        assert_eq!(
            selection.prerequisites.iter().collect::<Vec<_>>(),
            ["build"]
        );
        assert!(!selection.closure().contains("lint"));
        assert!(!selection.closure().contains("full-only"));
    }

    #[test]
    fn unknown_deleted_renamed_and_shared_paths_select_every_target() {
        let every = ["build", "docs", "lint", "remote", "whole"];
        for (change, reason) in [
            (changed("other/x"), "no target declares other/x"),
            (
                Change::Deleted("docs/a.md".into()),
                "deleted path docs/a.md",
            ),
            (
                Change::Renamed {
                    from: "docs/a.md".into(),
                    to: "docs/b.md".into(),
                },
                "renamed path docs/a.md to docs/b.md",
            ),
            (changed("crates/x.rs"), "shared input crates/x.rs changed"),
        ] {
            let selection =
                declared_selection(&fixture(), "quick", &[changed("docs/a.md"), change]);
            assert_eq!(selection.every_target.as_deref(), Some(reason));
            assert_eq!(selection.selected.keys().collect::<Vec<_>>(), every);
            assert!(selection.prerequisites.is_empty());
        }
    }

    #[test]
    fn guards_name_each_paths_tests_once_and_report_unguarded_paths() {
        let report = guards(
            &fixture(),
            &[
                changed("docs/api/x.md"),
                changed("README.md"),
                Change::Renamed {
                    from: "src/a.rs".into(),
                    to: "notes/a.rs".into(),
                },
            ],
        );
        assert_eq!(
            report.guarded["docs/api/x.md"],
            [
                "cargo test --test links",
                "cargo test --test docs",
                "cargo test --test api"
            ]
        );
        assert_eq!(
            report.guarded["src/a.rs"],
            ["cargo test --test links", "cargo test --test api"]
        );
        assert_eq!(report.unguarded, ["README.md", "notes/a.rs"]);
        assert_eq!(
            report.tests,
            [
                "cargo test --test links",
                "cargo test --test docs",
                "cargo test --test api"
            ]
        );
    }

    #[test]
    fn name_status_reads_each_status_and_refuses_unknown_ones() {
        let parsed =
            parse_name_status("M\0a\0A\0b c\0D\0d\0R100\0old\0new\0C075\0src\0copy\0T\0t\0")
                .unwrap();
        assert_eq!(
            parsed,
            [
                changed("a"),
                changed("b c"),
                Change::Deleted("d".into()),
                Change::Renamed {
                    from: "old".into(),
                    to: "new".into()
                },
                Change::Renamed {
                    from: "src".into(),
                    to: "copy".into()
                },
                changed("t"),
            ]
        );
        assert!(parse_name_status("U\0a\0")
            .unwrap_err()
            .contains("status U"));
        assert!(parse_name_status("R100\0old\0")
            .unwrap_err()
            .contains("has no path"));
    }

    #[test]
    fn changes_since_reads_the_working_tree_and_untracked_files() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let write = |name: &str, text: &str| std::fs::write(dir.path().join(name), text).unwrap();
        write("kept.md", "a");
        write("gone.md", "b");
        write("moved.md", "the same text on both sides of the rename\n");
        let mut index = repo.index().unwrap();
        for name in ["kept.md", "gone.md", "moved.md"] {
            index.add_path(Path::new(name)).unwrap();
        }
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("test", "test@example.com").unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "test: base",
            &tree,
            &[],
        )
        .unwrap();
        write("kept.md", "changed");
        std::fs::remove_file(dir.path().join("gone.md")).unwrap();
        std::fs::rename(dir.path().join("moved.md"), dir.path().join("renamed.md")).unwrap();
        index.remove_path(Path::new("moved.md")).unwrap();
        index.add_path(Path::new("renamed.md")).unwrap();
        index.write().unwrap();
        write("new.md", "c");
        let changes = changes_since(dir.path(), "HEAD").unwrap();
        for change in [
            Change::Deleted("gone.md".into()),
            changed("kept.md"),
            Change::Renamed {
                from: "moved.md".into(),
                to: "renamed.md".into(),
            },
            changed("new.md"),
        ] {
            assert!(changes.contains(&change), "{change:?} in {changes:?}");
        }
        assert_eq!(changes.len(), 4, "{changes:?}");
        let (base, source) = comparison_base(dir.path(), Some("HEAD")).unwrap();
        assert_eq!(base.len(), 40, "{base}");
        assert_eq!(source, "--since HEAD");
        assert!(comparison_base(dir.path(), None).is_err());
    }
}
