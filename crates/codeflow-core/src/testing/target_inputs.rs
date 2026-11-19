//! Informational comparison inputs. These never participate in target selection.
use std::collections::BTreeSet;
use std::path::Path;

use super::delivery::GateOptions;
use super::error::TestingError;
use crate::workgraph::work_start::{is_stable_work_target, logical_target, target_reference_names};

pub(super) const ENV_KEYS: [&str; 4] = [
    "CODEFLOW_BASE",
    "CODEFLOW_HEAD",
    "CODEFLOW_CHANGED_PATHS_FILE",
    "CODEFLOW_LANDING_LINE",
];
const PATH_BYTES_LIMIT: usize = 16 * 1024 * 1024;

fn safe_landing_line(line: &str) -> bool {
    is_stable_work_target(line)
        && !line.contains("..")
        && line
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/-_.".contains(&byte))
}

fn declared_base(
    repo: &git2::Repository,
    head: git2::Oid,
    line: &str,
    branch: Option<&str>,
) -> Result<git2::Oid, String> {
    if !safe_landing_line(line) {
        return Err("declared landing line is not a safe stable target".into());
    }
    if branch.is_some_and(|branch| logical_target(line) == logical_target(branch)) {
        return Err("declared landing line is the current branch".into());
    }
    let names = target_reference_names(line)
        .ok_or_else(|| "declared landing line has no stable reference names".to_string())?;
    let mut target = None;
    for name in names {
        match repo.find_reference(&name) {
            Ok(reference) => {
                target = Some(
                    reference
                        .peel_to_commit()
                        .map_err(|_| {
                            "declared landing line does not resolve to a commit".to_string()
                        })?
                        .id(),
                );
                break;
            }
            Err(error) if error.code() == git2::ErrorCode::NotFound => {}
            Err(_) => return Err("declared landing line reference could not be read".into()),
        }
    }
    let target =
        target.ok_or_else(|| "declared landing line has no resolvable local commit".to_string())?;
    if target == head {
        return Err("declared landing line resolves to HEAD".into());
    }
    let bases = repo
        .merge_bases(head, target)
        .map_err(|_| "no provable merge base with the landing line".to_string())?;
    if bases.len() != 1 {
        return Err("landing line has no unique merge base".into());
    }
    Ok(bases[0])
}

#[derive(Debug, serde::Serialize)]
pub(super) struct TargetInputs {
    pub base: String,
    pub head: String,
    pub landing_line: String,
    pub proven: bool,
    pub reason: String,
    pub paths_file: Option<String>,
    #[serde(skip)]
    paths: Option<Vec<u8>>,
}

impl TargetInputs {
    pub fn discover(root: &Path, options: &GateOptions, landing: Option<&str>) -> Self {
        let mut inputs = Self {
            base: "unknown".into(),
            head: "unknown".into(),
            landing_line: landing
                .filter(|line| safe_landing_line(line))
                .unwrap_or("unknown")
                .into(),
            proven: false,
            reason: "no repository or readable HEAD commit".into(),
            paths_file: None,
            paths: None,
        };
        let result = inputs.comparison(root, options);
        match result {
            Ok((base, head, paths)) => {
                inputs.base = base.to_string();
                inputs.head = head.to_string();
                inputs.proven = true;
                inputs.reason = if options.since.is_some() {
                    "--since resolved to an exact commit"
                } else {
                    "merge base with the carried task's declared landing line"
                }
                .into();
                inputs.paths = Some(paths);
            }
            Err(reason) => inputs.reason = reason,
        }
        inputs
    }

    fn comparison(
        &mut self,
        root: &Path,
        options: &GateOptions,
    ) -> Result<(git2::Oid, git2::Oid, Vec<u8>), String> {
        let repo = git2::Repository::discover(root).map_err(|_| self.reason.clone())?;
        let head = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|_| self.reason.clone())?
            .id();
        let attached = !repo.head_detached().unwrap_or(true);
        let branch = attached
            .then(|| repo.head().ok()?.shorthand().ok().map(str::to_owned))
            .flatten();
        let declared = branch.as_deref().and_then(|branch| {
            let task = crate::workgraph::task_id_from_branch_at(root, branch, &head.to_string())
                .ok()??;
            crate::workgraph::work_start::task_target_at_revision(root, &task, &head.to_string())
                .ok()?
        });
        if let Some(line) = &declared {
            self.landing_line = if safe_landing_line(line) {
                line.clone()
            } else {
                "unknown".into()
            };
        }
        if repo.is_shallow() {
            return Err("shallow repository: comparison history is incomplete".into());
        }
        let base = if let Some(since) = &options.since {
            repo.revparse_single(since)
                .and_then(|o| o.peel_to_commit())
                .map_err(|_| "--since does not resolve to a commit".to_string())?
                .id()
        } else {
            let line = declared.ok_or_else(|| {
                "no attached carried task with a declared landing line".to_string()
            })?;
            declared_base(&repo, head, &line, branch.as_deref())?
        };
        let tree = repo
            .find_commit(base)
            .and_then(|c| c.tree())
            .map_err(|e| e.to_string())?;
        let mut options = git2::DiffOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_typechange(true)
            .include_unreadable(true)
            .ignore_submodules(false);
        let diff = repo
            .diff_tree_to_workdir_with_index(Some(&tree), Some(&mut options))
            .map_err(|_| "changed paths could not be read completely".to_string())?;
        let mut paths = BTreeSet::new();
        let mut size = 0_usize;
        for delta in diff.deltas() {
            if delta.status() == git2::Delta::Unreadable {
                return Err("changed paths contain an unreadable entry".into());
            }
            for path in [delta.old_file().path_bytes(), delta.new_file().path_bytes()]
                .into_iter()
                .flatten()
            {
                if paths.insert(path.to_vec()) {
                    size = size.saturating_add(path.len() + 1);
                    if size > PATH_BYTES_LIMIT {
                        return Err("changed paths exceed the 16 MiB hint limit".into());
                    }
                }
            }
        }
        let mut bytes = Vec::with_capacity(size);
        for path in paths {
            bytes.extend(path);
            bytes.push(0);
        }
        Ok((base, head, bytes))
    }

    pub fn write_paths(&mut self, anchor: &Path, run_relative: &str) -> Result<(), TestingError> {
        if let Some(bytes) = &self.paths {
            let relative = format!("{run_relative}/changed-paths.nul");
            crate::contained::Tree::open(anchor)
                .and_then(|tree| tree.create_new(&relative, bytes))
                .map_err(|error| super::delivery::evidence_refused(anchor, &relative, &error))?;
            self.paths_file = anchor.join(relative).to_str().map(str::to_owned);
        }
        Ok(())
    }

    pub fn configure(&self, target: &mut super::config::TargetConfig) {
        for key in ENV_KEYS {
            target.env.remove(key);
        }
        for (key, value) in [
            (ENV_KEYS[0], Some(self.base.as_str())),
            (ENV_KEYS[1], Some(self.head.as_str())),
            (
                ENV_KEYS[2],
                if self.proven {
                    self.paths_file.as_deref()
                } else {
                    Some("unknown")
                },
            ),
            (ENV_KEYS[3], Some(self.landing_line.as_str())),
        ] {
            if let Some(value) = value {
                target.env.insert(key.into(), value.into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(repo: &git2::Repository, files: &[(&str, &[u8])]) -> git2::Oid {
        let root = repo.workdir().unwrap();
        let mut index = repo.index().unwrap();
        for (name, bytes) in files {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            index.add_path(Path::new(name)).unwrap();
        }
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "test: inputs",
            &tree,
            &parent.iter().collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn review_n1_path_list_over_sixteen_mib_is_unknown_without_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let blob = repo.blob(b"x").unwrap();
        let mut builder = repo.treebuilder(None).unwrap();
        // Git-only deleted entries avoid creating 66,000 filesystem files.
        // Each name plus its NUL is 256 bytes: 16,896,000 bytes in total.
        for index in 0..66_000 {
            let name = format!("{index:08}{}", "a".repeat(247));
            builder.insert(name.as_str(), blob, 0o100_644).unwrap();
        }
        let tree = repo.find_tree(builder.write().unwrap()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let base = repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                "test: oversized path list",
                &tree,
                &[],
            )
            .unwrap();
        let mut inputs = TargetInputs::discover(
            dir.path(),
            &GateOptions {
                since: Some(base.to_string()),
                ..GateOptions::default()
            },
            None,
        );
        assert!(!inputs.proven, "{inputs:?}");
        assert!(inputs.reason.contains("16 MiB"), "{inputs:?}");
        assert_eq!(inputs.base, "unknown");
        assert_eq!(inputs.head, "unknown");
        std::fs::create_dir(dir.path().join("run")).unwrap();
        inputs.write_paths(dir.path(), "run").unwrap();
        assert!(inputs.paths_file.is_none());
        assert!(std::fs::read_dir(dir.path().join("run"))
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn comparison_is_exact_and_preserves_newlines_and_deleted_paths() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let base = commit(&repo, &[("old", b"old"), ("edit", b"base")]);
        std::fs::remove_file(dir.path().join("old")).unwrap();
        std::fs::write(dir.path().join("edit"), b"new").unwrap();
        std::fs::write(dir.path().join("new\nname"), b"new").unwrap();
        let inputs = TargetInputs::discover(
            dir.path(),
            &GateOptions {
                since: Some(base.to_string()),
                ..GateOptions::default()
            },
            None,
        );
        assert!(inputs.proven, "{inputs:?}");
        assert_eq!(inputs.paths.unwrap(), b"edit\0new\nname\0old\0");
    }

    #[cfg(unix)]
    #[test]
    fn unrepresentable_path_hint_is_unset_and_configured_hints_cannot_replace_it() {
        let mut inputs = TargetInputs::discover(
            Path::new("/no/such/repository"),
            &GateOptions::default(),
            None,
        );
        inputs.proven = true;
        inputs.paths_file = None;
        let mut target: super::super::config::TargetConfig = serde_json::from_str(
            r#"{"name":"inputs","runner":"custom","env":{"CODEFLOW_CHANGED_PATHS_FILE":"lossy","CODEFLOW_BASE":"wrong"},"modes":{"quick":{"command":"true"}}}"#,
        ).unwrap();
        inputs.configure(&mut target);
        assert!(!target.env.contains_key(ENV_KEYS[2]));
        assert_eq!(target.env[ENV_KEYS[0]], "unknown");
        let mut cmd = std::process::Command::new("true");
        super::super::runner::configure_environment(&mut cmd, &target.env, &target.name).unwrap();
        assert!(cmd
            .get_envs()
            .any(|(key, value)| key == ENV_KEYS[2] && value.is_none()));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_path_bytes_are_never_replaced() {
        use std::os::unix::ffi::OsStrExt as _;
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        commit(&repo, &[("initial", b"x")]);
        let raw = std::ffi::OsStr::from_bytes(b"caf\xff");
        // Index-only names exercise this rule even on a filesystem that
        // rejects an invalid UTF-8 filename.
        let mut index = repo.index().unwrap();
        let mut entry = index.get_path(Path::new("initial"), 0).unwrap();
        entry.path = raw.as_bytes().to_vec();
        index.add(&entry).unwrap();
        index.write().unwrap();
        drop(index);
        let base = commit(&repo, &[]);
        let inputs = TargetInputs::discover(
            dir.path(),
            &GateOptions {
                since: Some(base.to_string()),
                ..GateOptions::default()
            },
            None,
        );
        assert!(inputs.proven, "{inputs:?}");
        assert_eq!(inputs.paths.unwrap(), b"caf\xff\0");
    }

    #[test]
    fn task_line_is_read_at_head_and_detached_head_keeps_only_the_carried_line() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let base = commit(
            &repo,
            &[
                ("initial", b"x"),
                (".codeflow/policy.json", br#"{"schema_version":1}"#),
            ],
        );
        repo.branch(
            "integration/checks",
            &repo.find_commit(base).unwrap(),
            false,
        )
        .unwrap();
        repo.branch(
            "task/TSK-001-inputs",
            &repo.find_commit(base).unwrap(),
            false,
        )
        .unwrap();
        repo.set_head("refs/heads/task/TSK-001-inputs").unwrap();
        let head = commit(
            &repo,
            &[(
                "project-management/tasks/TSK-001.md",
                b"---\nid: TSK-001\nintegration_target: integration/checks\n---\n",
            )],
        );
        let inputs = TargetInputs::discover(dir.path(), &GateOptions::default(), None);
        assert!(inputs.proven, "{inputs:?}");
        assert_eq!(inputs.base, base.to_string());
        assert_eq!(inputs.head, head.to_string());
        assert_eq!(inputs.landing_line, "integration/checks");
        repo.set_head_detached(head).unwrap();
        let inputs = TargetInputs::discover(
            dir.path(),
            &GateOptions::default(),
            Some("integration/checks"),
        );
        assert!(!inputs.proven);
        assert_eq!(inputs.base, "unknown");
        assert_eq!(inputs.landing_line, "integration/checks");
    }
}
