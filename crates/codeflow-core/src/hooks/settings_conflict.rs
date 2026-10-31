//! The local read of a `CodeFlow` settings file during a merge conflict
//! (TSK-330).
//!
//! The hooks read `.codeflow/policy.json` and `.codeflow/project.toml` from
//! the working copy in a few places: the git hooks' levels, the bootstrap
//! flag, the guards' comparison of the local files with the authority, and
//! the tier. While the index holds unmerged entries for one of those files,
//! its working copy carries conflict markers and does not parse, which used
//! to make every hook refuse every command, including the ones that resolve
//! or abandon the conflict.
//!
//! [`read_local`] keeps the working-copy read and changes one case: when that
//! read fails and the index holds unmerged entries (stages 1 to 3) for the
//! file, the copy committed at `HEAD`, read as git data, stands in for that
//! one input, and one note naming the file is printed. It never chooses the
//! policy authority (`landed_policy` does, from git refs), never falls back
//! to defaults, and conflict marker text alone is not evidence of a
//! conflict: without an unmerged entry the original error stands.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use git2::Repository;

/// Files whose recovery note this process has printed, so a hook that
/// reads the same file several times prints its note once.
static NOTED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Read the settings file at `relative` under `root` and parse it with
/// `parse`; `Ok(None)` only when the working copy proves the file absent.
///
/// When reading or parsing the working copy fails and the index holds
/// unmerged entries for the file, the copy committed at `HEAD` is parsed
/// instead and a note naming the file is printed once per process.
///
/// # Errors
/// The working copy fails and the index holds no unmerged entry for the
/// file (the original error), or it is unmerged and `HEAD` has no commit,
/// holds no copy, or holds a copy that does not parse.
pub(crate) fn read_local<T>(
    root: &Path,
    relative: &str,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<Option<T>, String> {
    let error = match super::policy::optional_text(&root.join(relative))
        .and_then(|text| text.as_deref().map(&parse).transpose())
    {
        Ok(value) => return Ok(value),
        Err(error) => error,
    };
    let Some(conflict) = unmerged(root, relative) else {
        return Err(error);
    };
    let text = committed_copy(&conflict).map_err(|why| refusal(relative, &why))?;
    let value = parse(&text).map_err(|why| {
        refusal(
            relative,
            &format!("its copy at HEAD does not parse either ({why})"),
        )
    })?;
    note(relative);
    Ok(Some(value))
}

/// The repository and the file's path inside it, for a file the index holds
/// unmerged entries for; `None` when it holds none or cannot be read, in
/// which case the caller keeps the working copy's error.
struct Unmerged {
    repo: Repository,
    path: String,
}

fn unmerged(root: &Path, relative: &str) -> Option<Unmerged> {
    let repo = super::repo::open(root).ok()??;
    let workdir = canonical(repo.workdir()?)?;
    let inside = canonical(root)?.strip_prefix(&workdir).ok()?.join(relative);
    let mut parts = Vec::new();
    for component in inside.components() {
        let std::path::Component::Normal(part) = component else {
            return None;
        };
        parts.push(part.to_str()?.to_owned());
    }
    let path = parts.join("/");
    let index = repo.index().ok()?;
    let conflicted = (1..=3).any(|stage| index.get_path(Path::new(&path), stage).is_some());
    conflicted.then_some(Unmerged { repo, path })
}

fn canonical(path: &Path) -> Option<PathBuf> {
    path.canonicalize()
        .ok()
        .map(crate::portable_path::without_verbatim)
}

/// The text of the regular file committed at `HEAD`.
fn committed_copy(conflict: &Unmerged) -> Result<String, String> {
    let commit = match conflict.repo.head() {
        Ok(head) => head
            .peel_to_commit()
            .map_err(|e| format!("HEAD cannot be read ({e})"))?,
        Err(e)
            if matches!(
                e.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) =>
        {
            return Err("HEAD has no commit to read it from".into())
        }
        Err(e) => return Err(format!("HEAD cannot be read ({e})")),
    };
    let tree = commit
        .tree()
        .map_err(|e| format!("HEAD cannot be read ({e})"))?;
    let entry = match tree.get_path(Path::new(&conflict.path)) {
        Ok(entry) => entry,
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            return Err("HEAD holds no copy of it".into())
        }
        Err(e) => return Err(format!("its copy at HEAD cannot be read ({e})")),
    };
    if entry.kind() != Some(git2::ObjectType::Blob) || entry.filemode() == 0o120_000 {
        return Err("its copy at HEAD is not a regular file".into());
    }
    let blob = conflict
        .repo
        .find_blob(entry.id())
        .map_err(|e| format!("its copy at HEAD cannot be read ({e})"))?;
    String::from_utf8(blob.content().to_vec())
        .map_err(|_| "its copy at HEAD is not UTF-8 text".into())
}

/// The refusal for an unmerged file with no usable copy at `HEAD`: it names
/// the conflict, why `HEAD` cannot stand in, and the operator's recovery,
/// since every hook refuses until the file is readable again.
fn refusal(relative: &str, why: &str) -> String {
    format!(
        "cannot read {relative}: it has an unresolved merge conflict and {why}; the operator resolves the file outside the session, or abandons the merge there with git merge --abort (git rebase --abort during a rebase)"
    )
}

fn note(relative: &str) {
    let first = NOTED
        .lock()
        .map_or(true, |mut noted| noted.insert(relative.to_owned()));
    if first {
        let finding = crate::remedy::Finding::new(
            format!(
                "{relative} has an unresolved merge conflict; the hooks read its copy at HEAD until it is resolved"
            ),
            crate::remedy::SETTINGS_CONFLICT.with(&[("path", relative)]),
        );
        eprintln!("{}", finding.line("codeflow policy", "note"));
    }
}

#[cfg(test)]
mod tests {
    use super::super::policy::{local_project_toml, read_project_toml, Policy};
    use std::fmt::Write as _;
    use std::path::Path;

    const PROJECT: &str = ".codeflow/project.toml";
    const POLICY: &str = ".codeflow/policy.json";

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = crate::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(dir: &Path, path: &str, text: &str) {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A repository whose `HEAD` holds `ours` at `path` and whose index
    /// holds the three stages of a conflict for it, with marker text in the
    /// working copy.
    fn unmerged(dir: &Path, path: &str, ours: Option<&str>) {
        git(dir, &["init", "-q", "-b", "main"]);
        write(dir, "base.txt", "base\n");
        if let Some(ours) = ours {
            write(dir, path, ours);
        }
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "chore: base"]);
        stage(dir, path);
    }

    fn stage(dir: &Path, path: &str) {
        let mut info = String::new();
        for (stage, text) in ["a = 1\n", "a = 2\n", "a = 3\n"].iter().enumerate() {
            write(dir, "blob.tmp", text);
            let id = git(dir, &["hash-object", "-w", "blob.tmp"]);
            let _ = writeln!(info, "100644 {id} {}\t{path}", stage + 1);
        }
        std::fs::remove_file(dir.join("blob.tmp")).unwrap();
        git(dir, &["update-index", "--force-remove", "--", path]);
        let list = dir.join("index-info.tmp");
        std::fs::write(&list, info).unwrap();
        let out = crate::git::command()
            .args(["update-index", "--index-info"])
            .current_dir(dir)
            .stdin(std::fs::File::open(&list).unwrap())
            .output()
            .unwrap();
        assert!(out.status.success());
        std::fs::remove_file(list).unwrap();
        write(dir, path, "<<<<<<< ours\n{\n=======\n}\n>>>>>>> theirs\n");
    }

    /// A main checkout with a linked worktree at `.worktrees/t` whose
    /// `HEAD` holds `tier = "minimal"` and whose index holds a conflict.
    pub(super) fn unmerged_for_worktree(main: &Path) {
        git(main, &["init", "-q", "-b", "main"]);
        write(main, "base.txt", "base\n");
        git(main, &["add", "."]);
        git(main, &["commit", "-q", "-m", "chore: base"]);
        git(
            main,
            &["worktree", "add", "-q", ".worktrees/t", "-b", "task/x"],
        );
        let task = main.join(".worktrees/t");
        write(&task, PROJECT, "tier = \"minimal\"\n");
        git(&task, &["add", "."]);
        git(&task, &["commit", "-q", "-m", "chore: ours"]);
        stage(&task, PROJECT);
    }

    #[test]
    fn an_unmerged_file_reads_its_head_copy() {
        let dir = tempfile::tempdir().unwrap();
        unmerged(dir.path(), PROJECT, Some("tier = \"minimal\"\n"));
        assert!(read_project_toml(dir.path()).is_err());
        let project = local_project_toml(dir.path()).unwrap().unwrap();
        assert_eq!(project["tier"].as_str(), Some("minimal"));

        let dir = tempfile::tempdir().unwrap();
        unmerged(
            dir.path(),
            POLICY,
            Some(r#"{"git":{"push_to_protected":"warn"}}"#),
        );
        assert!(Policy::load(dir.path()).is_err());
        let policy = Policy::load_local(dir.path()).unwrap();
        assert_eq!(
            policy.git.push_to_protected,
            super::super::PolicyLevel::Warn
        );
        // The working copy's other reads are left alone: the effective load
        // reads the same copy.
        assert_eq!(
            Policy::load_effective(dir.path())
                .unwrap()
                .0
                .git
                .push_to_protected,
            super::super::PolicyLevel::Warn
        );
    }

    #[test]
    fn a_parse_error_without_an_unmerged_entry_keeps_its_error() {
        let dir = tempfile::tempdir().unwrap();
        unmerged(dir.path(), "other.txt", Some("x\n"));
        write(dir.path(), PROJECT, "<<<<<<< ours\na = 1\n=======\n");
        let error = local_project_toml(dir.path()).unwrap_err();
        assert!(
            error.starts_with("cannot parse project settings .codeflow/project.toml"),
            "{error}"
        );
        write(dir.path(), POLICY, "{");
        let error = Policy::load_local(dir.path()).unwrap_err();
        assert!(error.starts_with("cannot parse policy"), "{error}");
    }

    #[test]
    fn an_unmerged_file_without_a_usable_head_copy_refuses() {
        let absent = tempfile::tempdir().unwrap();
        unmerged(absent.path(), PROJECT, None);
        let broken = tempfile::tempdir().unwrap();
        unmerged(broken.path(), PROJECT, Some("tier = = 1\n"));
        let unborn = tempfile::tempdir().unwrap();
        git(unborn.path(), &["init", "-q", "-b", "main"]);
        stage(unborn.path(), PROJECT);
        for (dir, why) in [
            (&absent, "HEAD holds no copy of it"),
            (&broken, "its copy at HEAD does not parse"),
            (&unborn, "HEAD has no commit"),
        ] {
            let error = local_project_toml(dir.path()).unwrap_err();
            assert!(error.contains("unresolved merge conflict"), "{error}");
            assert!(error.contains(why), "{error}");
            assert!(error.contains("git merge --abort"), "{error}");
            let error = Policy::load_effective(dir.path()).unwrap_err();
            assert!(error.contains(why), "{error}");
        }
    }

    #[test]
    fn a_project_below_the_work_tree_reads_its_own_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = "sub/.codeflow/project.toml";
        unmerged(dir.path(), path, Some("tier = \"full\"\n"));
        let project = local_project_toml(&dir.path().join("sub"))
            .unwrap()
            .unwrap();
        assert_eq!(project["tier"].as_str(), Some("full"));
    }
}

#[cfg(test)]
mod worktree_tests {
    use super::super::policy::local_project_toml;

    #[test]
    fn a_linked_worktree_reads_its_own_index() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir(&main).unwrap();
        super::tests::unmerged_for_worktree(&main);
        let task = main.join(".worktrees/t");
        let result = local_project_toml(&task);
        let project = result.unwrap().unwrap();
        assert_eq!(project["tier"].as_str(), Some("minimal"));
    }
}
