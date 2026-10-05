//! A thin runner for the git plumbing the registry uses. The registry is
//! written with plumbing only (`hash-object`, a temporary index,
//! `commit-tree`, `update-ref`), so issuing never touches the working tree.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use super::IdsError;

/// Git in one repository.
#[derive(Debug, Clone)]
pub struct Git {
    root: PathBuf,
}

impl Git {
    /// Git in the repository at `root`.
    #[must_use]
    pub fn new(root: &Path) -> Git {
        Git {
            root: root.to_path_buf(),
        }
    }

    /// The repository root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = crate::git::command();
        command
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null());
        command
    }

    /// Run git and return its raw output, whatever the exit status.
    ///
    /// # Errors
    ///
    /// Returns an error when git cannot be started.
    pub fn output(&self, args: &[&str]) -> Result<Output, IdsError> {
        self.command(args)
            .output()
            .map_err(|error| IdsError::Git(format!("git {}: {error}", args.join(" "))))
    }

    /// Run git with extra environment and return its raw output.
    ///
    /// # Errors
    ///
    /// Returns an error when git cannot be started.
    pub fn output_env(&self, args: &[&str], env: &[(&str, &Path)]) -> Result<Output, IdsError> {
        let mut command = self.command(args);
        for (key, value) in env {
            command.env(key, value);
        }
        command
            .output()
            .map_err(|error| IdsError::Git(format!("git {}: {error}", args.join(" "))))
    }

    /// Run git and return stdout, failing on a nonzero exit.
    ///
    /// # Errors
    ///
    /// Returns an error naming the command and git's stderr.
    pub fn run(&self, args: &[&str]) -> Result<String, IdsError> {
        let output = self.output(args)?;
        checked(args, &output)
    }

    /// Run git and return stdout as exact bytes, failing on a nonzero exit.
    /// Use it where the answer holds names (OS text rule, issue 79), then
    /// read it with [`z_fields`] or [`z_records`].
    ///
    /// # Errors
    ///
    /// Returns an error naming the command and git's stderr.
    pub fn run_bytes(&self, args: &[&str]) -> Result<Vec<u8>, IdsError> {
        let output = self.output(args)?;
        checked_bytes(args, output)
    }

    /// Run git with extra environment, failing on a nonzero exit.
    ///
    /// # Errors
    ///
    /// Returns an error naming the command and git's stderr.
    pub fn run_env(&self, args: &[&str], env: &[(&str, &Path)]) -> Result<String, IdsError> {
        let output = self.output_env(args, env)?;
        checked(args, &output)
    }

    /// Run git feeding `input` on stdin, failing on a nonzero exit.
    ///
    /// # Errors
    ///
    /// Returns an error naming the command and git's stderr.
    pub fn run_input(&self, args: &[&str], input: &[u8]) -> Result<String, IdsError> {
        let output = crate::git::output_with_input(&mut self.command(args), input)
            .map_err(|error| IdsError::Git(format!("git {}: {error}", args.join(" "))))?;
        checked(args, &output)
    }

    /// The full sha of `rev`, `None` when it does not resolve.
    #[must_use]
    pub fn rev(&self, rev: &str) -> Option<String> {
        let output = self
            .output(&[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{rev}^{{commit}}"),
            ])
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|sha| !sha.is_empty())
    }

    /// Whether this clone's history is shallow (fetched with `--depth`).
    ///
    /// # Errors
    ///
    /// Returns an error when git fails.
    pub fn is_shallow(&self) -> Result<bool, IdsError> {
        Ok(self.run(&["rev-parse", "--is-shallow-repository"])?.trim() == "true")
    }

    /// Whether `ancestor` is an ancestor of (or equal to) `descendant`.
    #[must_use]
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
        self.output(&["merge-base", "--is-ancestor", ancestor, descendant])
            .is_ok_and(|output| output.status.success())
    }

    /// The git directory shared by every worktree of this repository.
    ///
    /// # Errors
    ///
    /// Returns an error outside a repository.
    pub fn common_dir(&self) -> Result<PathBuf, IdsError> {
        // OS text rule (issue 79): the folder keys the registry's state and
        // locks, so its exact bytes are used. Only git's own newline is
        // framing: a name may end in a space or a carriage return.
        let out = self.run_bytes(&["rev-parse", "--git-common-dir"])?;
        let bytes = out.strip_suffix(b"\n").unwrap_or(&out);
        let path = crate::git::GitName::from_bytes(bytes)
            .os_path()
            .map_err(|error| IdsError::Git(error.to_string()))?;
        Ok(if path.is_absolute() {
            path
        } else {
            self.root.join(path)
        })
    }

    /// Whether `remote` is configured.
    #[must_use]
    pub fn has_remote(&self, remote: &str) -> bool {
        self.output(&["remote", "get-url", remote])
            .is_ok_and(|output| output.status.success())
    }

    /// `user.email`, or `unknown` when it is not set.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is not valid UTF-8: it is an issuer
    /// identity, so two different values never both read as `unknown` (OS
    /// text rule, issue 79).
    pub fn user_email(&self) -> Result<String, IdsError> {
        let Ok(output) = self.output(&["config", "user.email"]) else {
            return Ok("unknown".to_string());
        };
        if !output.status.success() {
            return Ok("unknown".to_string());
        }
        let value = String::from_utf8(output.stdout).map_err(|_| {
            IdsError::Git("git config user.email: the value is not valid UTF-8".to_string())
        })?;
        let value = value.trim();
        Ok(if value.is_empty() {
            "unknown".to_string()
        } else {
            value.to_string()
        })
    }

    /// Read many blobs in one `cat-file --batch` process.
    ///
    /// # Errors
    ///
    /// Returns an error when git fails or its output is malformed.
    pub fn blobs(&self, ids: &[String]) -> Result<HashMap<String, Vec<u8>>, IdsError> {
        let mut out = HashMap::new();
        if ids.is_empty() {
            return Ok(out);
        }
        let request = ids.iter().fold(String::new(), |mut acc, id| {
            acc.push_str(id);
            acc.push('\n');
            acc
        });
        let output = crate::git::output_with_input(
            &mut self.command(&["cat-file", "--batch"]),
            request.as_bytes(),
        )
        .map_err(|error| IdsError::Git(format!("git cat-file: {error}")))?;
        if !output.status.success() {
            return Err(IdsError::Git("git cat-file --batch failed".to_string()));
        }
        let stdout = output.stdout;
        let mut cursor = 0;
        for id in ids {
            let end = stdout[cursor..]
                .iter()
                .position(|b| *b == b'\n')
                .ok_or_else(|| IdsError::Git("git cat-file: truncated output".to_string()))?;
            let header = String::from_utf8_lossy(&stdout[cursor..cursor + end]).to_string();
            cursor += end + 1;
            if header.ends_with(" missing") {
                continue;
            }
            let size: usize = header
                .rsplit(' ')
                .next()
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| IdsError::Git(format!("git cat-file: bad header {header}")))?;
            let body = stdout
                .get(cursor..cursor + size)
                .ok_or_else(|| IdsError::Git("git cat-file: truncated body".to_string()))?;
            out.insert(id.clone(), body.to_vec());
            cursor += size + 1;
        }
        Ok(out)
    }

    /// `(mode, blob, path)` for every file in `rev`'s tree under `paths`.
    ///
    /// # Errors
    ///
    /// Returns an error when git fails.
    pub fn tree(
        &self,
        rev: &str,
        paths: &[&str],
    ) -> Result<Vec<(String, String, String)>, IdsError> {
        let mut args = vec!["ls-tree", "-r", "-z", "--full-tree", rev, "--"];
        args.extend_from_slice(paths);
        let listing = self.run_bytes(&args)?;
        // Each row is `mode SP type SP blob TAB path`. The meta is ASCII, so
        // the row is split on its first tab as bytes, and only the path is
        // read as a name (OS text rule, issue 79).
        Ok(listing
            .split(|byte| *byte == 0)
            .filter_map(|row| {
                let tab = row.iter().position(|byte| *byte == b'\t')?;
                let (meta, path) = (&row[..tab], &row[tab + 1..]);
                let meta = std::str::from_utf8(meta).ok()?;
                let mut parts = meta.split_whitespace();
                let mode = parts.next()?.to_string();
                let _kind = parts.next()?;
                let blob = parts.next()?.to_string();
                Some((
                    mode,
                    blob,
                    crate::git::GitName::from_bytes(path).storage_key(),
                ))
            })
            .collect())
    }

    /// Write `content` as a blob and return its id.
    ///
    /// # Errors
    ///
    /// Returns an error when git fails.
    pub fn write_blob(&self, content: &[u8]) -> Result<String, IdsError> {
        Ok(self
            .run_input(&["hash-object", "-w", "--stdin"], content)?
            .trim()
            .to_string())
    }

    /// Commit `files` (path to blob id) on top of `parent`, or as an orphan
    /// root when `parent` is `None`, without touching the working tree.
    ///
    /// # Errors
    ///
    /// Returns an error when any plumbing step fails.
    pub fn commit_files(
        &self,
        parent: Option<&str>,
        files: &[(String, String)],
        message: &str,
    ) -> Result<String, IdsError> {
        let dir = self.common_dir()?.join("codeflow");
        std::fs::create_dir_all(&dir)
            .map_err(|error| IdsError::Git(format!("temporary index: {error}")))?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let index = TempIndex(dir.join(format!("ids-index-{}-{nanos}", std::process::id())));
        let env = [("GIT_INDEX_FILE", index.0.as_path())];
        match parent {
            Some(parent) => self.run_env(&["read-tree", parent], &env)?,
            None => self.run_env(&["read-tree", "--empty"], &env)?,
        };
        for (path, blob) in files {
            self.run_env(
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("100644,{blob},{path}"),
                ],
                &env,
            )?;
        }
        let tree = self.run_env(&["write-tree"], &env)?.trim().to_string();
        let mut args = vec!["commit-tree", tree.as_str(), "-m", message];
        if let Some(parent) = parent {
            args.push("-p");
            args.push(parent);
        }
        Ok(self.run(&args)?.trim().to_string())
    }

    /// Every local and remote-tracking branch with its tip, excluding
    /// symbolic `HEAD` refs.
    ///
    /// # Errors
    ///
    /// Returns an error when git fails.
    pub fn branch_refs(&self) -> Result<Vec<(String, String)>, IdsError> {
        let listing = self.run(&[
            "for-each-ref",
            "--format=%(refname) %(objectname) %(symref)",
            "refs/heads",
            "refs/remotes",
        ])?;
        Ok(listing
            .lines()
            .filter_map(|line| {
                let mut parts = line.split(' ');
                let name = parts.next()?.to_string();
                let sha = parts.next()?.to_string();
                let symref = parts.next().unwrap_or_default();
                symref.is_empty().then_some((name, sha))
            })
            .collect())
    }
}

/// A temporary index file, removed when dropped.
struct TempIndex(PathBuf);

impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Text answers (shas, refs, config, counts) are read strictly: an answer
/// that is not valid UTF-8 is an error, never a substituted spelling (OS
/// text rule, issue 79). Answers that hold names come through
/// [`Git::run_bytes`].
fn checked(args: &[&str], output: &Output) -> Result<String, IdsError> {
    if output.status.success() {
        String::from_utf8(output.stdout.clone()).map_err(|_| {
            IdsError::Git(format!(
                "git {}: the answer is not valid UTF-8",
                args.join(" ")
            ))
        })
    } else {
        Err(IdsError::Git(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn checked_bytes(args: &[&str], output: Output) -> Result<Vec<u8>, IdsError> {
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(IdsError::Git(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// The fields of NUL-delimited (`-z`) git output. Paths arrive verbatim, so
/// a name git would quote (non-ASCII, a quote, a tab) is never altered. A
/// name that is not valid UTF-8 reads as its storage key (OS text rule,
/// issue 79): it keeps its exact bytes, differs from every text name, and no
/// record or registry path matches it. Leading newlines that `log -z` puts
/// between a header and its first path are dropped, and so is the empty
/// tail.
#[must_use]
pub fn z_fields(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .map(|field| {
            let start = field
                .iter()
                .position(|byte| *byte != b'\n')
                .unwrap_or(field.len());
            &field[start..]
        })
        .filter(|field| !field.is_empty())
        .map(|field| crate::git::GitName::from_bytes(field).storage_key())
        .collect()
}

/// The records of `log --format=%x1e...` output, each as its [`z_fields`].
#[must_use]
pub fn z_records(bytes: &[u8]) -> Vec<Vec<String>> {
    bytes
        .split(|byte| *byte == 0x1e)
        .filter(|record| !record.iter().all(u8::is_ascii_whitespace))
        .map(z_fields)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue 79: a name that is not valid UTF-8 keeps its exact bytes and
    /// differs from the lookalike with a replacement character.
    #[test]
    fn a_name_that_is_not_utf8_is_kept_apart_from_its_lookalike() {
        let fields = z_fields(b"docs/caf\xe9.md\0docs/caf\xef\xbf\xbd.md\0\n:meta\0");
        assert_eq!(fields.len(), 3);
        assert_ne!(fields[0], fields[1]);
        assert_eq!(fields[1], "docs/caf\u{fffd}.md");
        assert_eq!(crate::git::display_key(&fields[0]), "docs/caf\\xe9.md");
        assert_eq!(fields[2], ":meta");
        assert_eq!(z_records(b"\x1eabc\0x\0\x1e  \x1edef\0").len(), 2);
    }

    #[test]
    fn a_tree_listing_names_a_path_that_is_not_utf8_by_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        crate::git::repo_with_tree(
            dir.path(),
            &[
                (b"docs/caf\xe9.md", b"one"),
                (b"docs/caf\xef\xbf\xbd.md", b"two"),
            ],
        );
        let tree = Git::new(dir.path()).tree("main", &[]).unwrap();
        assert_eq!(tree.len(), 2);
        let mut paths: Vec<&str> = tree.iter().map(|(_, _, path)| path.as_str()).collect();
        paths.sort_unstable();
        assert_eq!(paths[0], "docs/caf\u{fffd}.md");
        assert_ne!(paths[0], paths[1]);
        assert!(
            paths[1].contains('\0'),
            "the key of a name that is not text"
        );
        // Only the path is encoded: it decodes to the exact bytes, with no
        // mode or blob in front of it.
        assert_eq!(
            crate::git::GitName::from_storage_key(paths[1]).bytes(),
            b"docs/caf\xe9.md"
        );
    }

    #[test]
    fn a_text_answer_that_is_not_utf8_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        crate::git::repo_with_tree(dir.path(), &[(b"a.txt", b"x")]);
        let git = Git::new(dir.path());
        // A blob read as text is not a name answer: the strict run refuses it.
        let blob = git.tree("main", &[]).unwrap()[0].1.clone();
        let written = git.write_blob(b"caf\xe9").unwrap();
        assert!(git.run(&["cat-file", "blob", &written]).is_err());
        assert!(git.run(&["cat-file", "blob", &blob]).is_ok());
    }

    /// Round seven on issue 79: an issuer identity that is not valid UTF-8 is
    /// a refusal, never `unknown`, which another value or none also reads as.
    #[test]
    fn a_user_email_that_is_not_utf8_is_refused_not_unknown() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let git = Git::new(dir.path());
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(".git").join("config"))
            .unwrap();
        config
            .write_all(b"[user]\n\temail = caf\xe9@example.test\n")
            .unwrap();
        let error = git.user_email().unwrap_err();
        assert!(error.to_string().contains("not valid UTF-8"), "{error}");
    }

    /// Round nine on issue 79: a git directory whose own name ends in a
    /// carriage return is that directory, not a sibling without it.
    #[test]
    fn the_common_dir_keeps_a_trailing_carriage_return() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir(&main).unwrap();
        crate::git::repo_with_tree(&main, &[(b"a.txt", b"x")]);
        let bare = dir.path().join("meta\r");
        let run = |cwd: &std::path::Path, args: &[&str]| {
            let out = crate::git::command()
                .args(args)
                .current_dir(cwd)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {out:?}");
        };
        let target = bare.clone();
        let clone = crate::git::command()
            .args(["clone", "-q", "--bare"])
            .arg(&main)
            .arg(&target)
            .output()
            .unwrap();
        if !clone.status.success() {
            return; // this volume refuses the name
        }
        run(
            &bare,
            &["worktree", "add", "-q", "../wt", "-b", "other", "main"],
        );
        let common = Git::new(&dir.path().join("wt")).common_dir().unwrap();
        assert_eq!(common.file_name().unwrap(), "meta\r");
    }
}
