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
    /// When set, every command that captures output runs within these
    /// limits (TSK-327, the pre-push id sync).
    limits: Option<crate::git::remote_query::QueryLimits>,
}

impl Git {
    /// Git in the repository at `root`.
    #[must_use]
    pub fn new(root: &Path) -> Git {
        Git {
            root: root.to_path_buf(),
            limits: None,
        }
    }

    /// Git in the repository at `root` whose captured commands each run
    /// within `limits`: `fetch`, `push` and `ls-remote` within the fetch
    /// wait, any other within the query wait, and every answer within
    /// [`MAX_ANSWER_BYTES`](crate::git::remote_query::MAX_ANSWER_BYTES).
    /// A `fetch` or `ls-remote` that runs out of time writes nothing, so it
    /// is [`IdsError::Offline`], as an unreachable authority is. A push that
    /// gives no answer has an unknown outcome and is never offline
    /// ([`Git::push_output`]). Any other read that cannot answer is an
    /// error. Local reads also run with lazy fetching off, so a missing
    /// promised object fails at once and starts no transport.
    #[must_use]
    pub fn bounded(root: &Path, limits: crate::git::remote_query::QueryLimits) -> Git {
        Git {
            root: root.to_path_buf(),
            limits: Some(limits),
        }
    }

    /// [`Git::bounded`] within the lookup limits the authority's
    /// configuration sets, as the pre-push hook's registry reads run (the id
    /// sync and the append-only check). No guard runs them: a guard builds
    /// only `QueryLimits::guard`.
    ///
    /// # Errors
    ///
    /// Refuses a repository with a promisor remote on a git too old to
    /// honour the no-lazy-fetch setting, as the commit-time scan does: there
    /// a missing object would start a fetch outside every deadline.
    pub fn for_pre_push(root: &Path) -> Result<Git, IdsError> {
        crate::hooks::gitleaks::require_offline_objects(root).map_err(IdsError::Git)?;
        Ok(Git::bounded(
            root,
            crate::git::remote_query::QueryLimits::from_authority_or_default(root),
        ))
    }

    /// The deadline for `args` when limits are set: the fetch wait for a
    /// network command, the query wait for any other.
    fn deadline(&self, args: &[&str]) -> Option<std::time::Duration> {
        let limits = self.limits?;
        Some(
            if matches!(args.first(), Some(&"fetch" | &"push" | &"ls-remote")) {
                limits.fetch_timeout()
            } else {
                limits.timeout()
            },
        )
    }

    /// Capture `command`'s output, within the limits when they are set,
    /// feeding `input` on stdin when given. `Err` holds whether the
    /// deadline ran out and why there is no output.
    fn capture_raw(
        &self,
        mut command: Command,
        args: &[&str],
        input: Option<&[u8]>,
    ) -> Result<Output, (bool, String)> {
        let shown = || crate::git::remote_query::display_urls(&args.join(" "));
        let Some(deadline) = self.deadline(args) else {
            let output = match input {
                Some(input) => crate::git::output_with_input(&mut command, input),
                None => command.output(),
            };
            return output.map_err(|error| (false, format!("git {}: {error}", shown())));
        };
        let output = match input {
            Some(input) => {
                crate::git::remote_query::bounded_output_with_input(command, input, deadline)
            }
            None => crate::git::remote_query::bounded_output(command, deadline),
        };
        output.map_err(|why| (why.timed_out, format!("git {}: {}", shown(), why.reason)))
    }

    /// [`Git::capture_raw`] as the registry's error: only a `fetch` or
    /// `ls-remote` that ran out of time is offline.
    fn capture(
        &self,
        command: Command,
        args: &[&str],
        input: Option<&[u8]>,
    ) -> Result<Output, IdsError> {
        self.capture_raw(command, args, input)
            .map_err(|(timed_out, reason)| {
                if timed_out && matches!(args.first(), Some(&"fetch" | &"ls-remote")) {
                    IdsError::Offline(reason)
                } else {
                    IdsError::Git(reason)
                }
            })
    }

    /// Run `git push` and return its raw output, whatever the exit status.
    /// A push that gives no full answer, whether it ran out of time, its
    /// answer was too large or git could not be started, is `Err` with the
    /// reason: the caller cannot tell whether the authority took the
    /// write, so it reads the registry back and never treats the authority
    /// as offline (R-13, TSK-327).
    ///
    /// # Errors
    ///
    /// Why the push gave no answer.
    pub fn push_output(&self, args: &[&str]) -> Result<Output, String> {
        self.capture_raw(self.command(args), args, None)
            .map_err(|(_, reason)| reason)
    }

    /// Fetch from `remote`, in one request, every object reachable from
    /// `refs` that this clone holds only as a promise (a partial clone
    /// fetches commits and trees but not blobs). The bounded registry's
    /// local reads never fetch a missing object themselves, so the sync
    /// fills its registry here, within the fetch wait (TSK-327). A clone
    /// with nothing missing starts no fetch.
    ///
    /// # Errors
    ///
    /// Returns an error when the objects cannot be listed or the fetch
    /// fails. A fetch that runs out of time is [`IdsError::Offline`].
    pub fn fetch_missing(&self, remote: &str, refs: &[&str]) -> Result<(), IdsError> {
        if refs.is_empty() {
            return Ok(());
        }
        // One object id per LF record, a missing one led by `?`; no path is
        // printed, so a registry file name cannot forge a record.
        let mut listing = vec![
            "rev-list",
            "--objects",
            "--no-object-names",
            "--missing=print",
        ];
        listing.extend_from_slice(refs);
        let missing = self
            .run(&listing)?
            .split_terminator('\n')
            .filter_map(|line| line.strip_prefix('?'))
            .fold(String::new(), |mut acc, id| {
                acc.push_str(id);
                acc.push('\n');
                acc
            });
        if missing.is_empty() {
            return Ok(());
        }
        let args = [
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            "--recurse-submodules=no",
            "--filter=blob:none",
            "--stdin",
            remote,
        ];
        let output = self.capture(self.command(&args), &args, Some(missing.as_bytes()))?;
        if output.status.success() {
            return Ok(());
        }
        Err(IdsError::Git(format!(
            "fetching the registry's missing objects from {remote} failed: {}",
            crate::git::remote_query::display_urls(String::from_utf8_lossy(&output.stderr).trim())
        )))
    }

    /// The repository root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Git for `args`. A bounded registry's local commands read this
    /// repository's objects only ([`crate::git::local_objects_only`]): in a
    /// partial clone a missing promised object would otherwise start a
    /// fetch from the promisor remote, a transport outside every deadline
    /// here. The registry fetch fills what a partial clone leaves out
    /// ([`Git::fetch_missing`]), so a missing object after it fails at once.
    /// The pre-push sync first refuses a git too old to honour the setting
    /// in a repository with a promisor remote ([`super::issue::sync`]).
    fn command(&self, args: &[&str]) -> Command {
        let mut command = crate::workgraph::history::command();
        command
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null());
        if self.limits.is_some() && !matches!(args.first(), Some(&"fetch" | &"push" | &"ls-remote"))
        {
            crate::git::local_objects_only(&mut command);
        }
        command
    }

    fn check_history(&self, args: &[&str]) -> Result<(), IdsError> {
        if matches!(
            args.first(),
            Some(&"log" | &"rev-list" | &"merge-base" | &"cherry" | &"diff-tree")
        ) {
            crate::workgraph::history::check_command(&self.root)
                .map_err(|error| IdsError::Git(format!("git {}: {error}", args[0])))?;
        }
        Ok(())
    }

    /// Run git and return its raw output, whatever the exit status.
    ///
    /// # Errors
    ///
    /// Returns an error when git cannot be started.
    pub fn output(&self, args: &[&str]) -> Result<Output, IdsError> {
        self.check_history(args)?;
        self.capture(self.command(args), args, None)
    }

    /// Run git with extra environment and return its raw output.
    ///
    /// # Errors
    ///
    /// Returns an error when git cannot be started.
    pub fn output_env(&self, args: &[&str], env: &[(&str, &Path)]) -> Result<Output, IdsError> {
        self.check_history(args)?;
        let mut command = self.command(args);
        for (key, value) in env {
            command.env(key, value);
        }
        self.capture(command, args, None)
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

    /// Run git feeding `input` on stdin, failing on a nonzero exit, within
    /// the limits when they are set.
    ///
    /// # Errors
    ///
    /// Returns an error naming the command and git's stderr.
    pub fn run_input(&self, args: &[&str], input: &[u8]) -> Result<String, IdsError> {
        self.check_history(args)?;
        let output = self.capture(self.command(args), args, Some(input))?;
        checked(args, &output)
    }

    /// The full object ID of `rev`, or absence proven by quiet verification.
    ///
    /// # Errors
    /// Returns an error if Git cannot read the revision or returns an invalid ID.
    pub fn rev(&self, rev: &str) -> Result<Option<String>, IdsError> {
        let name = format!("{rev}^{{commit}}");
        let args = ["rev-parse", "--verify", "--quiet", &name];
        let output = self.output(&args)?;
        if output.status.code() == Some(1) {
            return Ok(None);
        }
        let text = checked(&args, &output)?;
        let sha = text.strip_suffix('\n').unwrap_or(&text);
        if !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(IdsError::Git(
                "git rev-parse returned an invalid object ID".into(),
            ));
        }
        Ok(Some(sha.to_string()))
    }

    /// Whether this clone's history is shallow (fetched with `--depth`).
    ///
    /// # Errors
    /// Returns an error when Git fails or does not return a boolean answer.
    pub fn is_shallow(&self) -> Result<bool, IdsError> {
        match self
            .run(&["rev-parse", "--is-shallow-repository"])?
            .as_str()
        {
            "true\n" => Ok(true),
            "false\n" => Ok(false),
            _ => Err(IdsError::Git(
                "git rev-parse returned an invalid shallow answer".into(),
            )),
        }
    }

    /// Whether `ancestor` is an ancestor of (or equal to) `descendant`.
    ///
    /// # Errors
    /// Returns an error when Git cannot establish either answer.
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, IdsError> {
        let args = ["merge-base", "--is-ancestor", ancestor, descendant];
        let output = self.output(&args)?;
        if output.status.code() == Some(1) {
            return Ok(false);
        }
        checked(&args, &output)?;
        Ok(true)
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
        if bytes.is_empty() || bytes.contains(&0) {
            return Err(IdsError::Git(
                "git rev-parse returned an invalid common directory".into(),
            ));
        }
        let path = crate::git::GitName::from_bytes(bytes)
            .os_path()
            .map_err(|error| IdsError::Git(error.to_string()))?;
        Ok(if path.is_absolute() {
            path
        } else {
            self.root.join(path)
        })
    }

    /// Whether `remote` has a configured URL.
    ///
    /// # Errors
    /// Returns an error when Git cannot read configuration.
    pub fn has_remote(&self, remote: &str) -> Result<bool, IdsError> {
        let key = format!("remote.{remote}.url");
        let args = ["config", "--get", &key];
        let output = self.output(&args)?;
        if output.status.code() == Some(1) {
            return Ok(false);
        }
        checked(&args, &output)?;
        Ok(true)
    }

    /// `user.email`, or `unknown` when it is not set.
    ///
    /// # Errors
    /// Returns an error when Git fails or the issuer identity is not valid UTF-8.
    pub fn user_email(&self) -> Result<String, IdsError> {
        let args = ["config", "--get", "user.email"];
        let output = self.output(&args)?;
        if output.status.code() == Some(1) {
            return Ok("unknown".to_string());
        }
        let value = checked(&args, &output)?;
        // Only Git's own newline is framing; retain the existing empty-value policy.
        let value = value.strip_suffix('\n').unwrap_or(&value);
        Ok(if value.is_empty() {
            "unknown".to_string()
        } else {
            value.to_string()
        })
    }

    /// Read many blobs in one `cat-file --batch` process, within the limits
    /// when they are set.
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
        let args = ["cat-file", "--batch"];
        let output = self.capture(self.command(&args), &args, Some(request.as_bytes()))?;
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
            let header = std::str::from_utf8(&stdout[cursor..cursor + end]).map_err(|error| {
                IdsError::Git(format!("git cat-file header is not valid UTF-8: {error}"))
            })?;
            cursor += end + 1;
            if header.ends_with(" missing") {
                return Err(IdsError::Git(format!(
                    "git cat-file: requested blob {id} is missing; repair the repository objects, then retry"
                )));
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
        listing
            .split(|byte| *byte == 0)
            .filter(|row| !row.is_empty())
            .map(|row| {
                let malformed = || IdsError::Git("git ls-tree returned a malformed entry".into());
                let tab = row
                    .iter()
                    .position(|byte| *byte == b'\t')
                    .ok_or_else(malformed)?;
                let (meta, path) = (&row[..tab], &row[tab + 1..]);
                let meta = std::str::from_utf8(meta).map_err(|_| malformed())?;
                let parts: Vec<_> = meta.split(' ').collect();
                let [mode, _kind, blob] = parts.as_slice() else {
                    return Err(malformed());
                };
                Ok((
                    (*mode).to_string(),
                    (*blob).to_string(),
                    crate::git::GitName::from_bytes(path).storage_key(),
                ))
            })
            .collect()
    }

    /// Write `content` as a blob and return its id.
    ///
    /// # Errors
    ///
    /// Returns an error when git fails.
    pub fn write_blob(&self, content: &[u8]) -> Result<String, IdsError> {
        Ok(self
            .run_input(&["hash-object", "-w", "--stdin"], content)?
            .strip_suffix('\n')
            .unwrap_or("")
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
        let tree = self
            .run_env(&["write-tree"], &env)?
            .strip_suffix('\n')
            .unwrap_or("")
            .to_string();
        let mut args = vec!["commit-tree", tree.as_str(), "-m", message];
        if let Some(parent) = parent {
            args.push("-p");
            args.push(parent);
        }
        Ok(self
            .run(&args)?
            .strip_suffix('\n')
            .unwrap_or("")
            .to_string())
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
        listing
            .split_terminator('\n')
            .map(|line| {
                let malformed =
                    || IdsError::Git("git for-each-ref returned a malformed branch record".into());
                let parts: Vec<_> = line.split(' ').collect();
                let [name, sha, symref] = parts.as_slice() else {
                    return Err(malformed());
                };
                if !git2::Reference::is_valid_name(name)
                    || !matches!(sha.len(), 40 | 64)
                    || !sha.bytes().all(|byte| byte.is_ascii_hexdigit())
                    || (!symref.is_empty() && !git2::Reference::is_valid_name(symref))
                {
                    return Err(malformed());
                }
                Ok(symref
                    .is_empty()
                    .then(|| ((*name).to_string(), (*sha).to_string())))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|rows| rows.into_iter().flatten().collect())
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
                crate::git::remote_query::display_urls(&args.join(" "))
            ))
        })
    } else {
        Err(IdsError::Git(format!(
            "git {}: {}",
            crate::git::remote_query::display_urls(&args.join(" ")),
            crate::git::remote_query::display_urls(String::from_utf8_lossy(&output.stderr).trim())
        )))
    }
}

fn checked_bytes(args: &[&str], output: Output) -> Result<Vec<u8>, IdsError> {
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(IdsError::Git(format!(
            "git {}: {}",
            crate::git::remote_query::display_urls(&args.join(" ")),
            crate::git::remote_query::display_urls(String::from_utf8_lossy(&output.stderr).trim())
        )))
    }
}

/// The fields of NUL-delimited (`-z`) git output. Paths arrive verbatim, so
/// a name git would quote (non-ASCII, a quote, a tab) is never altered. A
/// name that is not valid UTF-8 reads as its storage key (OS text rule,
/// issue 79): it keeps its exact bytes, differs from every text name, and no
/// record or registry path matches it. Only NUL field framing is removed;
/// every byte in a nonempty field, including leading LF, is preserved.
#[must_use]
pub fn z_fields(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| crate::git::GitName::from_bytes(field).storage_key())
        .collect()
}
/// Raw-log record reader. RS belongs only to header fields; LF belongs only
/// to raw metadata framing, and neither delimiter is removed from path fields.
#[must_use]
pub fn z_records(bytes: &[u8]) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut path_next = false;
    for field in bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
    {
        if path_next {
            record.push(crate::git::GitName::from_bytes(field).storage_key());
            path_next = false;
        } else if let Some(header) = field.strip_prefix(&[0x1e]) {
            if !record.is_empty() {
                records.push(std::mem::take(&mut record));
            }
            record.push(crate::git::GitName::from_bytes(header).storage_key());
        } else {
            let meta = field
                .strip_prefix(b"\n")
                .filter(|meta| meta.starts_with(b":"))
                .unwrap_or(field);
            path_next = meta.starts_with(b":");
            record.push(crate::git::GitName::from_bytes(meta).storage_key());
        }
    }
    if !record.is_empty() {
        records.push(record);
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn failed_git_commands_redact_url_arguments_and_stderr() {
        use std::os::unix::process::ExitStatusExt as _;
        let url = "https://u:FAKE_REGISTRY_TOKEN@host.invalid/repo";
        let output = Output {
            status: std::process::ExitStatus::from_raw(128 << 8),
            stdout: Vec::new(),
            stderr: format!("fatal: cannot fetch '{url}'").into_bytes(),
        };
        for error in [
            checked(&["fetch", url], &output).unwrap_err(),
            checked_bytes(&["fetch", url], output).unwrap_err(),
        ] {
            let text = error.to_string();
            assert!(!text.contains("FAKE_REGISTRY_TOKEN"));
            assert_eq!(text.matches("https://***@host.invalid/repo").count(), 2);
        }
    }

    #[cfg(unix)]
    fn r20_malformed_answer_child(test: &str, body: &str) -> bool {
        use std::os::unix::fs::PermissionsExt as _;
        const CHILD: &str = "CODEFLOW_R20_IDS_ANSWER_CHILD";
        if std::env::var(CHILD).as_deref() == Ok(test) {
            return true;
        }
        let programs = tempfile::tempdir().unwrap();
        let stub = programs.path().join("git");
        std::fs::write(&stub, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("ids::git::tests::{test}"),
                "--nocapture",
            ])
            .env(CHILD, test)
            .env("PATH", programs.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        false
    }

    #[cfg(unix)]
    #[test]
    fn r20_malformed_registry_branch_listing_refuses() {
        if r20_malformed_answer_child(
            "r20_malformed_registry_branch_listing_refuses",
            "printf malformed",
        ) {
            let root = tempfile::tempdir().unwrap();
            assert!(Git::new(root.path()).branch_refs().is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn r20_malformed_registry_common_dir_refuses() {
        if r20_malformed_answer_child("r20_malformed_registry_common_dir_refuses", "printf '\\n'") {
            let root = tempfile::tempdir().unwrap();
            assert!(Git::new(root.path()).common_dir().is_err());
        }
    }

    /// TSK-327: a bounded registry's stdin-fed read has the query wait. A
    /// command that takes its input and answers nothing for 30 s (an alias
    /// that sleeps) refuses at the 1 s wait, naming it. Before, the read had
    /// no deadline and waited out the stall.
    #[cfg(unix)]
    #[test]
    fn a_stalled_stdin_fed_registry_read_refuses_within_the_wait() {
        let dir = tempfile::tempdir().unwrap();
        Git::new(dir.path()).run(&["init", "-q"]).unwrap();
        let git = Git::bounded(
            dir.path(),
            crate::git::remote_query::QueryLimits::scaled(std::time::Duration::from_secs(1), 1),
        );
        let started = std::time::Instant::now();
        let error = git
            .run_input(
                &["-c", "alias.stall=!sleep 30", "stall"],
                &vec![b'x'; 1 << 20],
            )
            .unwrap_err()
            .to_string();
        let took = started.elapsed();
        assert!(error.contains("no answer within 1s"), "{error}");
        assert!(took < std::time::Duration::from_secs(10), "took {took:?}");
    }

    /// TSK-327: a bounded registry's local reads run with lazy fetching off.
    /// In a partial clone whose registry blob is promised and absent,
    /// `blobs` fails at once as missing, and the promisor's upload-pack (a
    /// wrapper that leaves a marker) never starts. The unbounded runner,
    /// which keeps git's default, fetches the same blob through the wrapper:
    /// that shows the fixture would start a transport and that this git
    /// honours `GIT_NO_LAZY_FETCH`.
    #[cfg(unix)]
    #[test]
    fn a_missing_promised_registry_blob_starts_no_transport() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let upstream = dir.path().join("upstream");
        let work = dir.path().join("work");
        let run = |at: &Path, args: &[&str]| Git::new(at).run(args).unwrap();
        std::fs::create_dir_all(&upstream).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        run(&upstream, &["init", "-q", "-b", "main"]);
        std::fs::write(upstream.join("001.toml"), "id = \"TSK-001\"\n").unwrap();
        run(&upstream, &["add", "."]);
        run(
            &upstream,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "commit",
                "-q",
                "-m",
                "chore: one entry",
            ],
        );
        run(&upstream, &["config", "uploadpack.allowFilter", "true"]);
        run(
            &upstream,
            &["config", "uploadpack.allowAnySHA1InWant", "true"],
        );
        let blob = run(&upstream, &["rev-parse", "HEAD:001.toml"])
            .trim()
            .to_string();
        run(&work, &["init", "-q", "-b", "main"]);
        let url = format!("file://{}", upstream.display());
        run(&work, &["remote", "add", "origin", &url]);
        run(&work, &["config", "remote.origin.promisor", "true"]);
        run(&work, &["config", "extensions.partialClone", "origin"]);
        run(
            &work,
            &["fetch", "-q", "--filter=blob:none", "origin", "main"],
        );
        let marker = dir.path().join("fetched");
        let wrapper = dir.path().join("upload-pack");
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\ntouch '{}'\nexec git upload-pack \"$@\"\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        run(
            &work,
            &[
                "config",
                "remote.origin.uploadpack",
                wrapper.to_str().unwrap(),
            ],
        );
        let bounded = Git::bounded(&work, crate::git::remote_query::QueryLimits::default());
        let error = bounded
            .blobs(std::slice::from_ref(&blob))
            .unwrap_err()
            .to_string();
        assert!(error.contains("missing"), "{error}");
        assert!(!marker.exists(), "a lazy fetch started a transport");
        let fetched = Git::new(&work).blobs(std::slice::from_ref(&blob)).unwrap();
        assert_eq!(fetched[&blob], b"id = \"TSK-001\"\n");
        assert!(marker.exists(), "the fixture's lazy fetch did not run");
    }

    /// In a partial clone, the registry fetch brings the blobs the clone
    /// filter left out, so the bounded sync's ledger reads, which never
    /// fetch lazily, still read every entry, an earlier entry included.
    #[test]
    fn the_registry_fetch_fills_a_partial_clone() {
        let dir = tempfile::tempdir().unwrap();
        let upstream = dir.path().join("upstream");
        let work = dir.path().join("work");
        let run = |at: &Path, args: &[&str]| Git::new(at).run(args).unwrap();
        let commit = |at: &Path, name: &str| {
            std::fs::create_dir_all(at.join("ids/TSK")).unwrap();
            std::fs::write(at.join(format!("ids/TSK/{name}.toml")), name).unwrap();
            run(at, &["add", "."]);
            run(
                at,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@example.invalid",
                    "commit",
                    "-q",
                    "-m",
                    "chore: one entry",
                ],
            );
            run(at, &["rev-parse", &format!("HEAD:ids/TSK/{name}.toml")])
                .trim()
                .to_string()
        };
        std::fs::create_dir_all(&upstream).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        run(
            &upstream,
            &["init", "-q", "-b", super::super::REGISTRY_BRANCH],
        );
        run(&upstream, &["config", "uploadpack.allowFilter", "true"]);
        run(
            &upstream,
            &["config", "uploadpack.allowAnySHA1InWant", "true"],
        );
        let first = commit(&upstream, "001");
        run(&work, &["init", "-q", "-b", "main"]);
        let url = format!("file://{}", upstream.display());
        run(&work, &["remote", "add", "origin", &url]);
        run(&work, &["config", "remote.origin.promisor", "true"]);
        run(
            &work,
            &["config", "remote.origin.partialCloneFilter", "blob:none"],
        );
        run(&work, &["config", "extensions.partialClone", "origin"]);
        let tracking = super::super::tracking_ref("origin");
        let refspec = format!("refs/heads/{}:{tracking}", super::super::REGISTRY_BRANCH);
        run(
            &work,
            &["fetch", "-q", "--filter=blob:none", "origin", &refspec],
        );
        let second = commit(&upstream, "002");
        let bounded = Git::bounded(&work, crate::git::remote_query::QueryLimits::default());
        assert!(
            bounded.blobs(std::slice::from_ref(&first)).is_err(),
            "the fixture's clone filter left nothing out"
        );
        assert!(matches!(
            super::super::issue::fetch(&bounded, "origin").unwrap(),
            super::super::issue::Fetched::Ok
        ));
        let read = bounded.blobs(&[first.clone(), second.clone()]).unwrap();
        assert_eq!(read[&first], b"001");
        assert_eq!(read[&second], b"002");
    }

    #[test]
    fn r20_registry_rev_failure_is_not_an_empty_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(&dir.path().join("missing"));
        assert!(crate::ids::Ledger::read(&git, "HEAD").is_err());
    }

    #[test]
    fn r20_registry_email_failure_is_not_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(&dir.path().join("missing"));
        assert!(git.user_email().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn r20_registry_malformed_shallow_answer_refuses() {
        use std::os::unix::fs::PermissionsExt as _;
        const CHILD: &str = "CODEFLOW_R20_SHALLOW_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let root = tempfile::tempdir().unwrap();
            assert!(Git::new(root.path()).is_shallow().is_err());
            return;
        }
        let programs = tempfile::tempdir().unwrap();
        let stub = programs.path().join("git");
        std::fs::write(&stub, "#!/bin/sh\nprintf malformed\n").unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ids::git::tests::r20_registry_malformed_shallow_answer_refuses",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("PATH", programs.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Issue 79: a name that is not valid UTF-8 keeps its exact bytes and
    /// differs from the lookalike with a replacement character.
    #[test]
    fn a_name_that_is_not_utf8_is_kept_apart_from_its_lookalike() {
        let fields = z_fields(b"docs/caf\xe9.md\0docs/caf\xef\xbf\xbd.md\0\n:meta\0");
        assert_eq!(fields.len(), 3);
        assert_ne!(fields[0], fields[1]);
        assert_eq!(fields[1], "docs/caf\u{fffd}.md");
        assert_eq!(crate::git::display_key(&fields[0]), "docs/caf\\xe9.md");
        assert_eq!(fields[2], "\n:meta");
        assert_eq!(z_records(b"\x1eabc\0x\0\x1edef\0").len(), 2);
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
        let clone = codeflow_fixture::clone(dir.path(), &main, &target)
            .bare()
            .output();
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

    /// Round thirteen on issue 79: an email that ends in a no-break space is
    /// another identity, not the one without it.
    #[test]
    fn a_user_email_keeps_whitespace_that_is_part_of_it() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let git = Git::new(dir.path());
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(".git").join("config"))
            .unwrap();
        config
            .write_all("[user]\n\temail = \"owner@example.test\u{a0}\"\n".as_bytes())
            .unwrap();
        assert_eq!(git.user_email().unwrap(), "owner@example.test\u{a0}");
    }
}

#[cfg(test)]
mod r16_core_regressions {
    #[test]
    fn r16_git_raw_paths_keep_lf_and_record_separator() {
        assert_eq!(super::z_fields(b"\nfile\0")[0], "\nfile");
        let records = super::z_records(b"\x1esha\0\n:meta\0\x1efile\0:meta\0\nfile\0");
        assert_eq!(
            records,
            vec![vec!["sha", ":meta", "\x1efile", ":meta", "\nfile"]]
        );
    }
}

#[cfg(test)]
mod r22_tests {
    #[test]
    fn r22_requested_missing_blob_refuses_and_empty_request_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let git = super::Git::new(dir.path());
        assert!(git.blobs(&[]).unwrap().is_empty());
        assert!(matches!(
            git.blobs(&["1111111111111111111111111111111111111111".into()]),
            Err(super::IdsError::Git(_))
        ));
    }
}
