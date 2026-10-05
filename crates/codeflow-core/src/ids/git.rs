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
        let dir = self.run(&["rev-parse", "--git-common-dir"])?;
        let path = PathBuf::from(dir.trim());
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

    /// `user.email`, or `unknown`.
    #[must_use]
    pub fn user_email(&self) -> String {
        self.run(&["config", "user.email"])
            .map(|value| value.trim().to_string())
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
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
        let listing = self.run(&args)?;
        Ok(z_fields(&listing)
            .filter_map(|line| {
                let (meta, path) = line.split_once('\t')?;
                let mut parts = meta.split_whitespace();
                let mode = parts.next()?.to_string();
                let _kind = parts.next()?;
                let blob = parts.next()?.to_string();
                Some((mode, blob, path.to_string()))
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

fn checked(args: &[&str], output: &Output) -> Result<String, IdsError> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(IdsError::Git(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// The fields of NUL-delimited (`-z`) git output. Paths arrive verbatim, so
/// a name git would quote (non-ASCII, a quote, a tab) is never altered.
/// Leading newlines that `log -z` puts between a header and its first path
/// are dropped, and so is the empty tail.
pub fn z_fields(text: &str) -> impl Iterator<Item = &str> {
    text.split('\0')
        .map(|field| field.trim_start_matches('\n'))
        .filter(|field| !field.is_empty())
}
