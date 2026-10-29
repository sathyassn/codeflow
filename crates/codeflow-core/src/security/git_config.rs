//! Read the configuration a Git invocation will use, without running an alias,
//! pager, hook or transport. Git owns precedence and conditional-include syntax.
//! Its include-depth bound, our output limit and deadline bound the read.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

pub(crate) type Environment = BTreeMap<String, String>;
type ConfigValue<'a> = (&'a str, &'a str, &'a [u8]);

#[derive(Debug)]
struct Entry {
    scope: String,
    origin: String,
    key: Vec<u8>,
    value: Option<Vec<u8>>,
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct Include {
    /// The canonical file, for inventory and deduplication.
    pub(crate) path: PathBuf,
    /// The spelling Git reads the file by: the include value resolved
    /// against the including file's own spelling. Relative includes inside
    /// it resolve from this path's directory, not the canonical one.
    pub(crate) logical: PathBuf,
    pub(crate) missing: bool,
    /// The `includeIf` condition, or `None` for a plain `include.path`.
    pub(crate) condition: Option<String>,
}

#[derive(Debug)]
pub(crate) struct EffectiveConfig {
    entries: Vec<Entry>,
    /// Files contributing values, resolved from Git's reported origin directory.
    pub(crate) sources: Vec<PathBuf>,
    /// Includes named by the files Git read, including inactive conditions.
    pub(crate) includes: Vec<Include>,
}

impl EffectiveConfig {
    /// Effective key names, without exposing or executing their values.
    pub(crate) fn keys(&self) -> Result<Vec<&str>, String> {
        let mut keys = self
            .entries
            .iter()
            .map(|entry| {
                std::str::from_utf8(&entry.key)
                    .map_err(|_| "cannot verify Git configuration: a key is not UTF-8".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        keys.sort_unstable();
        keys.dedup();
        Ok(keys)
    }

    pub(crate) fn get(&self, key: &str) -> Result<Option<&str>, String> {
        self.get_all(key)
            .last()
            .map(|(_, _, value)| {
                std::str::from_utf8(value)
                    .map_err(|_| format!("cannot verify Git configuration: {key} is not UTF-8"))
            })
            .transpose()
    }

    /// All raw values in Git's precedence order, with their scope and origin.
    pub(crate) fn get_all(&self, key: &str) -> Vec<ConfigValue<'_>> {
        // Git folds section and variable names, but preserves subsection case.
        let mut key = key.as_bytes().to_vec();
        if let Some(first) = key.iter().position(|byte| *byte == b'.') {
            let last = key.iter().rposition(|byte| *byte == b'.').unwrap_or(first);
            key[..first].make_ascii_lowercase();
            key[last + 1..].make_ascii_lowercase();
        }
        self.entries
            .iter()
            .filter(|entry| entry.key == key)
            .map(|entry| {
                (
                    entry.scope.as_str(),
                    entry.origin.as_str(),
                    entry.value.as_deref().unwrap_or(b"true"),
                )
            })
            .collect()
    }

    pub(crate) fn boolean(&self, key: &str) -> Result<bool, String> {
        match self.get(key)? {
            None => Ok(false),
            Some(value) => super::git::config_boolean(value)
                .ok_or_else(|| format!("cannot verify Git configuration: {key} is not a boolean")),
        }
    }
}

fn validate_environment(environment: &Environment) -> Result<(), String> {
    for name in environment.keys() {
        if !(name.starts_with("GIT_CONFIG_")
            || matches!(
                name.as_str(),
                "GIT_DIR"
                    | "GIT_WORK_TREE"
                    | "GIT_COMMON_DIR"
                    | "HOME"
                    | "XDG_CONFIG_HOME"
                    | "GIT_CEILING_DIRECTORIES"
            ))
        {
            return Err(format!(
                "cannot verify Git configuration: unsupported environment variable {name}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// How many effective-configuration reads [`read`] made on this thread,
    /// for tests that prove a refusal comes before any read.
    pub(crate) static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `globals` contains only the options before the Git subcommand. Environment
/// entries are allowlisted literal command-local assignments; inherited native
/// bytes stay in the child environment. Unresolved shell expressions must be
/// refused by the caller before reaching this reader.
pub(crate) fn read(
    cwd: &Path,
    globals: &[String],
    environment: &Environment,
) -> Result<EffectiveConfig, String> {
    #[cfg(test)]
    READS.with(|reads| reads.set(reads.get() + 1));
    let deadline = Instant::now() + Duration::from_secs(3);
    validate_environment(environment)?;
    let mut command = crate::git::command();
    command
        .current_dir(cwd)
        .envs(environment)
        .stdin(Stdio::null());
    // GIT_CONFIG selects a file only for `git config`, not for the command
    // being judged. Keep the runtime configuration environment intact.
    command.env_remove("GIT_CONFIG");
    let mut directory = with_globals(&mut command, cwd, globals)?;
    // Git reports relative file origins from its discovered repository root,
    // not necessarily the caller's directory. Probe with the identical globals
    // and environment. Outside a repository config still works from cwd.
    let mut location = crate::git::command();
    location
        .current_dir(cwd)
        .envs(environment)
        .env_remove("GIT_CONFIG")
        .stdin(Stdio::null())
        .args(command.get_args())
        .args(["rev-parse", "--show-prefix"]);
    directory = directory.canonicalize().unwrap_or(directory);
    if let Ok(prefix) = bounded_output(&mut location, deadline) {
        for _ in prefix.split(|byte| *byte == b'/').skip(1) {
            directory.pop();
        }
    }
    command.args([
        "--no-pager",
        "config",
        "--null",
        "--list",
        "--show-origin",
        "--show-scope",
        "--includes",
    ]);
    let bytes = bounded_output(&mut command, deadline)?;
    parse(&bytes, &directory, environment)
}

fn parse(
    bytes: &[u8],
    directory: &Path,
    environment: &Environment,
) -> Result<EffectiveConfig, String> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err("cannot verify Git configuration: incomplete output".into());
    }
    let mut fields = bytes
        .strip_suffix(&[0])
        .unwrap_or(bytes)
        .split(|byte| *byte == 0);
    let mut entries = Vec::new();
    let mut includes = Vec::new();
    let mut sources = Vec::new();
    while let Some(scope) = fields.next() {
        if scope.is_empty() && bytes.is_empty() {
            break;
        }
        let origin = fields
            .next()
            .ok_or("cannot verify Git configuration: missing origin")?;
        let scope = std::str::from_utf8(scope)
            .map_err(|_| "cannot verify Git configuration: scope is not UTF-8")?;
        let origin = std::str::from_utf8(origin)
            .map_err(|_| "cannot verify Git configuration: origin is not UTF-8")?;
        if let Some(file) = origin.strip_prefix("file:") {
            sources.push(directory.join(file));
        }
        let entry = fields
            .next()
            .ok_or("cannot verify Git configuration: incomplete output")?;
        // Git forbids newlines and NUL in keys; values may contain newlines.
        let mut parts = entry.splitn(2, |byte| *byte == b'\n');
        let key = parts
            .next()
            .ok_or("cannot verify Git configuration: missing key")?;
        let value = parts.next();
        if super::git::is_include_key(key) {
            let value = value.ok_or("cannot verify Git configuration: include has no path")?;
            let value = std::str::from_utf8(value)
                .map_err(|_| "cannot verify Git configuration: include path is not UTF-8")?;
            let path = include_path(value, origin, directory, environment)?;
            // Git silently skips missing includes. Keep absence explicit, even
            // for an inactive condition: a later branch may activate it.
            let missing = match std::fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => false,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
                Ok(_) => {
                    return Err(format!(
                        "cannot verify Git configuration include {}: not a regular file",
                        path.display()
                    ))
                }
                Err(error) => {
                    return Err(format!(
                        "cannot verify Git configuration include {}: {error}",
                        path.display()
                    ))
                }
            };
            let logical = path.clone();
            let path = path.canonicalize().unwrap_or(path);
            let condition = match key
                .strip_prefix(b"includeif.")
                .and_then(|rest| rest.strip_suffix(b".path"))
            {
                Some(condition) => Some(
                    std::str::from_utf8(condition)
                        .map_err(|_| {
                            "cannot verify Git configuration: an include condition is not UTF-8"
                        })?
                        .to_string(),
                ),
                None => None,
            };
            includes.push(Include {
                path,
                logical,
                missing,
                condition,
            });
            if includes.len() > 128 {
                return Err("cannot verify Git configuration: more than 128 includes".into());
            }
        }
        entries.push(Entry {
            scope: scope.into(),
            origin: origin.into(),
            key: key.to_vec(),
            value: value.map(<[u8]>::to_vec),
        });
    }
    includes.sort();
    includes.dedup();
    sources.sort();
    sources.dedup();
    Ok(EffectiveConfig {
        entries,
        sources,
        includes,
    })
}

/// Read one configuration file and the files it includes, as Git reads
/// `git config --file`. Used for an include whose condition the caller
/// cannot evaluate yet, such as one keyed on a worktree that does not exist.
pub(crate) fn read_file(path: &Path, environment: &Environment) -> Result<EffectiveConfig, String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    validate_environment(environment)?;
    // Include paths are canonical; Git on Windows does not accept the
    // verbatim `\\?\` spelling that canonicalization produces there.
    let path = &crate::portable_path::without_verbatim(path.to_path_buf());
    let directory = path
        .parent()
        .ok_or("cannot verify Git configuration: include file has no parent")?;
    let mut command = crate::git::command();
    command
        .current_dir(directory)
        .envs(environment)
        .env_remove("GIT_CONFIG")
        .stdin(Stdio::null())
        .arg("--no-pager")
        .arg("config")
        .arg("--file")
        .arg(path)
        .args([
            "--null",
            "--list",
            "--show-origin",
            "--show-scope",
            "--includes",
        ]);
    let bytes = bounded_output(&mut command, deadline)?;
    parse(&bytes, directory, environment)
}

fn include_path(
    value: &str,
    origin: &str,
    cwd: &Path,
    environment: &Environment,
) -> Result<PathBuf, String> {
    if let Some(rest) = value.strip_prefix("~/") {
        let home = environment
            .get("HOME")
            .map(std::ffi::OsString::from)
            .or_else(|| std::env::var_os("HOME"))
            .ok_or("cannot verify Git configuration: include needs HOME")?;
        return Ok(PathBuf::from(home).join(rest));
    }
    if value.starts_with('~') || value.starts_with("%(") {
        return Err(format!(
            "cannot verify Git configuration: include path {value} uses unsupported interpolation"
        ));
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let file = origin
        .strip_prefix("file:")
        .ok_or("cannot verify Git configuration: relative include has no source file")?;
    let source = cwd.join(file);
    Ok(source
        .parent()
        .ok_or("cannot verify Git configuration: include source has no parent")?
        .join(path))
}

/// Add `globals`, the options before the Git subcommand, to `command` as Git
/// reads them, and return the directory Git starts in. An option the
/// reader does not support refuses.
fn with_globals(
    command: &mut std::process::Command,
    cwd: &Path,
    globals: &[String],
) -> Result<PathBuf, String> {
    let mut directory = cwd.to_path_buf();
    let mut at = 0;
    while at < globals.len() {
        let arg = globals[at].as_str();
        if super::git::GLOBAL_VALUE_OPTIONS.contains(&arg) {
            let value = globals
                .get(at + 1)
                .ok_or("cannot verify Git configuration: missing global-option value")?;
            if arg == "-C" {
                directory = directory.join(value);
            }
            command.args([arg, value]);
            at += 2;
        } else if arg.starts_with("--exec-path=") || matches!(arg, "-p" | "-P") {
            // Configuration and rev-parse are builtins. Do not select an
            // external program or start a pager while inspecting their data.
            at += 1;
        } else if arg.starts_with("-c")
            || arg.starts_with("-C")
            || [
                "--git-dir=",
                "--work-tree=",
                "--namespace=",
                "--config-env=",
                "--attr-source=",
                "--shallow-file=",
            ]
            .iter()
            .any(|prefix| arg.starts_with(prefix))
            || matches!(
                arg,
                "--bare"
                    | "--no-pager"
                    | "--paginate"
                    | "--no-optional-locks"
                    | "--no-replace-objects"
                    | "--literal-pathspecs"
                    | "--glob-pathspecs"
                    | "--noglob-pathspecs"
                    | "--icase-pathspecs"
            )
        {
            if let Some(path) = arg.strip_prefix("-C") {
                directory = directory.join(path);
            }
            // --paginate must not run a configured pager during inspection.
            if arg != "--paginate" {
                command.arg(arg);
            }
            at += 1;
        } else {
            return Err(format!(
                "cannot verify Git configuration: unsupported global option {arg}"
            ));
        }
    }
    Ok(directory)
}

/// What a Git object name resolves to in the repository Git would use with
/// `globals` from `cwd`: its type and whether it peels to a commit, or
/// `None` when it names no object. One bounded `cat-file --batch-check`
/// answers every name; lazy fetches are off, so a partial clone never goes
/// to the network (TSK-1002, object reads by id).
/// The Git directory and work tree Git finds from `cwd` with `globals`
/// (`-C` and `-c` only), as Git reports them: `None` where Git finds no
/// repository with a work tree (TSK-1002 Fable review 13).
pub(crate) fn repository_of(
    cwd: &Path,
    globals: &[String],
) -> Result<Option<(PathBuf, PathBuf)>, String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut probe = crate::git::command();
    probe.current_dir(cwd).stdin(Stdio::null());
    with_globals(&mut probe, cwd, globals)?;
    probe.args([
        "--no-pager",
        "rev-parse",
        "--absolute-git-dir",
        "--show-toplevel",
    ]);
    let bytes = match bounded_output(&mut probe, deadline) {
        Ok(bytes) => bytes,
        Err(why) if why.contains("git rejected") => return Ok(None),
        Err(why) => return Err(why),
    };
    let text = String::from_utf8(bytes).map_err(|_| "cannot read the Git repository paths")?;
    let mut lines = text.strip_suffix('\n').unwrap_or(&text).split('\n');
    match (lines.next(), lines.next(), lines.next()) {
        (Some(dir), Some(top), None) if !dir.is_empty() && !top.is_empty() => {
            Ok(Some((PathBuf::from(dir), PathBuf::from(top))))
        }
        _ => Err("cannot read the Git repository paths".into()),
    }
}

pub(crate) fn object_kinds(
    cwd: &Path,
    globals: &[String],
    names: &[String],
) -> Result<Vec<Option<(String, bool)>>, String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut command = crate::git::command();
    command.current_dir(cwd).env("GIT_NO_LAZY_FETCH", "1");
    with_globals(&mut command, cwd, globals)?;
    command.args(["--no-pager", "cat-file", "--batch-check=%(objecttype)"]);
    let mut input = String::new();
    for name in names {
        if name.contains(['\n', '\r', '\0']) {
            return Err("cannot establish a Git object name that spans lines".into());
        }
        input.push_str(name);
        input.push('\n');
        input.push_str(name);
        input.push_str("^{commit}\n");
    }
    // Where Git finds no repository it resolves no object name either. Any
    // other failure, a timeout included, stays unverified.
    let mut probe = crate::git::command();
    probe.current_dir(cwd).stdin(Stdio::null());
    with_globals(&mut probe, cwd, globals)?;
    probe.args(["--no-pager", "rev-parse", "--git-dir"]);
    if let Err(why) = bounded_output(&mut probe, deadline) {
        return if why.contains("git rejected") {
            Ok(vec![None; names.len()])
        } else {
            Err(why)
        };
    }
    let bytes = bounded(&mut command, deadline, Some(input.into_bytes()))?;
    let text = String::from_utf8(bytes).map_err(|_| "cannot read Git object types")?;
    let lines: Vec<&str> = text
        .strip_suffix('\n')
        .unwrap_or(&text)
        .split('\n')
        .collect();
    if lines.len() != names.len() * 2 {
        return Err("cannot read Git object types".into());
    }
    let kind = |line: &str, query: &str| -> Result<Option<String>, String> {
        if ["blob", "commit", "tag", "tree"].contains(&line) {
            Ok(Some(line.to_string()))
        } else if line.strip_suffix(" missing") == Some(query) {
            Ok(None)
        } else {
            Err(format!("cannot establish the Git object `{query}`"))
        }
    };
    names
        .iter()
        .enumerate()
        .map(|(at, name)| {
            let peeled = format!("{name}^{{commit}}");
            Ok(kind(lines[2 * at], name)?.map(|found| {
                (
                    found,
                    kind(lines[2 * at + 1], &peeled).ok().flatten().is_some(),
                )
            }))
        })
        .collect()
}

pub(crate) fn bounded_output(
    command: &mut std::process::Command,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    bounded(command, deadline, None)
}

/// [`bounded_output`] with `input` written to the child's stdin when given.
fn bounded(
    command: &mut std::process::Command,
    deadline: Instant,
    input: Option<Vec<u8>>,
) -> Result<Vec<u8>, String> {
    const LIMIT: u64 = 4 * 1024 * 1024;
    if Instant::now() >= deadline {
        return Err("cannot verify Git configuration: child exceeded 3 seconds".into());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    // The shared helper writes stdin from its own thread; the deadline
    // below bounds the child whatever the write does.
    let mut child = match input {
        Some(input) => crate::git::spawn_with_input(command, input).map(|(child, _)| child),
        None => command.spawn(),
    }
    .map_err(|error| format!("cannot verify Git configuration: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or("cannot verify Git configuration: no output pipe")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    let result = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    let bytes = match result {
        Ok(Ok(bytes)) if bytes.len() as u64 <= LIMIT => bytes,
        _ => {
            let _ = child.kill();
            std::thread::spawn(move || child.wait());
            return Err(
                "cannot verify Git configuration: read failed or exceeded 3 seconds / 4 MiB".into(),
            );
        }
    };
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("cannot verify Git configuration: {error}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            std::thread::spawn(move || child.wait());
            return Err("cannot verify Git configuration: child exceeded 3 seconds".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if !status.success() {
        return Err("cannot verify Git configuration: git rejected the configuration (including malformed values or an include loop)".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Environment) {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let environment = Environment::from([
            ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
            ("GIT_CONFIG_SYSTEM".into(), "/dev/null".into()),
            ("GIT_CONFIG_COUNT".into(), "0".into()),
            ("GIT_CONFIG_PARAMETERS".into(), String::new()),
            (
                "GIT_DIR".into(),
                dir.path().join(".git").to_str().unwrap().into(),
            ),
            ("GIT_WORK_TREE".into(), dir.path().to_str().unwrap().into()),
        ]);
        (dir, environment)
    }

    #[test]
    fn lookup_matches_git_key_case_rules() {
        let (dir, env) = fixture();
        let config = read(
            dir.path(),
            &[
                "-c".into(),
                "Alias.Pm=push origin HEAD:main".into(),
                "-c".into(),
                "remote.Mixed.Url=first".into(),
                "-c".into(),
                "remote.mixed.url=second".into(),
            ],
            &env,
        )
        .unwrap();
        assert_eq!(
            config.get("ALIAS.Pm").unwrap(),
            Some("push origin HEAD:main")
        );
        assert_eq!(config.get_all("REMOTE.Mixed.URL")[0].2, b"first");
        assert_eq!(config.get("REMOTE.mixed.URL").unwrap(), Some("second"));
    }

    #[test]
    fn missing_inactive_include_does_not_hide_aliases() {
        let (dir, mut env) = fixture();
        let global = dir.path().join("global");
        std::fs::write(
            &global,
            "[includeIf \"gitdir:/nowhere/work/\"]\n path = missing\n[alias]\n st = status\n",
        )
        .unwrap();
        env.insert("GIT_CONFIG_GLOBAL".into(), global.to_str().unwrap().into());
        let config = read(dir.path(), &[], &env).unwrap();
        assert_eq!(config.get("alias.st").unwrap(), Some("status"));
        assert_eq!(config.includes.len(), 1);
        assert!(config.includes[0].missing);
        assert_eq!(config.includes[0].path, dir.path().join("missing"));
    }

    #[test]
    fn relative_origins_from_subdirectories_match_repository_root() {
        let (dir, mut env) = fixture();
        env.remove("GIT_DIR");
        env.remove("GIT_WORK_TREE");
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("extra"), "[alias]\n st = status\n").unwrap();
        git2::Repository::open(dir.path())
            .unwrap()
            .config()
            .unwrap()
            .set_str("include.path", "../extra")
            .unwrap();
        for (cwd, args) in [
            (dir.path().join("sub"), vec![]),
            (dir.path().to_path_buf(), vec!["-C".into(), "sub".into()]),
        ] {
            let config = read(&cwd, &args, &env).unwrap();
            assert_eq!(config.get("alias.st").unwrap(), Some("status"));
            assert_eq!(
                config.includes[0].path,
                dir.path().join("extra").canonicalize().unwrap()
            );
            assert!(!config.includes[0].missing);
        }
    }

    #[test]
    fn files_environment_and_options_follow_git_precedence() {
        let (dir, mut env) = fixture();
        let global = dir.path().join("global");
        let system = dir.path().join("system");
        std::fs::write(&global, "[alias]\n ship = global\n").unwrap();
        std::fs::write(&system, "[alias]\n system = present\n").unwrap();
        env.insert("GIT_CONFIG_GLOBAL".into(), global.to_str().unwrap().into());
        env.insert("GIT_CONFIG_SYSTEM".into(), system.to_str().unwrap().into());
        let config = read(dir.path(), &[], &env).unwrap();
        assert_eq!(config.get("alias.ship").unwrap(), Some("global"));
        assert_eq!(config.get("alias.system").unwrap(), Some("present"));
        env.insert("GIT_CONFIG_NOSYSTEM".into(), "true".into());
        assert_eq!(
            read(dir.path(), &[], &env)
                .unwrap()
                .get("alias.system")
                .unwrap(),
            None
        );
        env.insert(
            "GIT_CONFIG_PARAMETERS".into(),
            "'alias.ship=parameters'".into(),
        );
        assert_eq!(
            read(dir.path(), &[], &env)
                .unwrap()
                .get("alias.ship")
                .unwrap(),
            Some("parameters")
        );
        env.insert("GIT_CONFIG_COUNT".into(), "1".into());
        env.insert("GIT_CONFIG_KEY_0".into(), "alias.ship".into());
        env.insert("GIT_CONFIG_VALUE_0".into(), "count".into());
        assert_eq!(
            read(dir.path(), &[], &env)
                .unwrap()
                .get("alias.ship")
                .unwrap(),
            Some("parameters")
        );
        env.insert("GIT_CONFIG_PARAMETERS".into(), String::new());
        assert_eq!(
            read(dir.path(), &[], &env)
                .unwrap()
                .get("alias.ship")
                .unwrap(),
            Some("count")
        );
        let args = vec!["-c".into(), "alias.ship=option".into()];
        assert_eq!(
            read(dir.path(), &args, &env)
                .unwrap()
                .get("alias.ship")
                .unwrap(),
            Some("option")
        );
        env.insert("GIT_CONFIG_VALUE_0".into(), "config-env".into());
        let args = vec![
            "-c".into(),
            "alias.ship=earlier".into(),
            "--config-env=alias.ship=GIT_CONFIG_VALUE_0".into(),
        ];
        assert_eq!(
            read(dir.path(), &args, &env)
                .unwrap()
                .get("alias.ship")
                .unwrap(),
            Some("config-env")
        );
    }

    #[test]
    fn includes_and_conditional_includes_are_read_in_the_command_repository() {
        let (dir, mut env) = fixture();
        let global = dir.path().join("global");
        let included = dir.path().join("included");
        let conditional = dir.path().join("conditional");
        std::fs::write(&included, "[alias]\n ship = push origin v9.9.9\n").unwrap();
        std::fs::write(&conditional, "[push]\n followTags = true\n").unwrap();
        let output = crate::git::judging_command()
            .current_dir(dir.path())
            .args(["rev-parse", "--absolute-git-dir"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let git_dir = String::from_utf8(output.stdout).unwrap();
        let git_dir = git_dir
            .trim_end()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        std::fs::write(&global, format!("[include]\n path = included\n[includeIf \"gitdir:{git_dir}\"]\n path = conditional\n")).unwrap();
        env.insert("GIT_CONFIG_GLOBAL".into(), global.to_str().unwrap().into());
        let config = read(dir.path(), &[], &env).unwrap();
        assert_eq!(
            config.get("alias.ship").unwrap(),
            Some("push origin v9.9.9")
        );
        assert!(config.boolean("push.followtags").unwrap());
        assert_eq!(config.includes.len(), 2);

        let (other, mut other_env) = fixture();
        other_env.insert("GIT_CONFIG_GLOBAL".into(), global.to_str().unwrap().into());
        let config = read(other.path(), &[], &other_env).unwrap();
        assert_eq!(
            config.get("alias.ship").unwrap(),
            Some("push origin v9.9.9")
        );
        assert!(!config.boolean("push.followtags").unwrap());
    }

    #[test]
    fn include_loops_and_invalid_environment_cannot_be_verified() {
        let (dir, mut env) = fixture();
        let global = dir.path().join("global");
        env.insert("GIT_CONFIG_GLOBAL".into(), global.to_str().unwrap().into());
        std::fs::write(&global, "[include]\n path = global\n").unwrap();
        assert!(read(dir.path(), &[], &env)
            .unwrap_err()
            .contains("cannot verify"));
        std::fs::write(&global, "").unwrap();
        env.insert("GIT_CONFIG_COUNT".into(), "1".into());
        env.insert("GIT_CONFIG_KEY_0".into(), String::new());
        assert!(read(dir.path(), &[], &env).is_err());
        env.insert("GIT_CONFIG_COUNT".into(), "0".into());
        assert!(read(
            dir.path(),
            &["--config-env=alias.ship=MISSING_TSK283".into()],
            &env
        )
        .is_err());
    }

    #[test]
    fn values_retain_scope_origin_and_order() {
        let (dir, mut env) = fixture();
        let global = dir.path().join("global");
        std::fs::write(&global, "[credential]\n helper = fixture-global\n").unwrap();
        env.insert("GIT_CONFIG_GLOBAL".into(), global.to_str().unwrap().into());
        let repo = git2::Repository::open(dir.path()).unwrap();
        repo.config()
            .unwrap()
            .set_str("credential.helper", "fixture-local")
            .unwrap();
        let config = read(dir.path(), &[], &env).unwrap();
        let values = config.get_all("credential.helper");
        assert_eq!(values.len(), 2);
        for ((scope, origin, value), (expected_scope, path, expected_value)) in values.iter().zip([
            ("global", global, b"fixture-global".as_slice()),
            (
                "local",
                dir.path().join(".git/config"),
                b"fixture-local".as_slice(),
            ),
        ]) {
            assert_eq!(*scope, expected_scope);
            assert_eq!(*value, expected_value);
            assert_eq!(
                Path::new(origin.strip_prefix("file:").unwrap())
                    .canonicalize()
                    .unwrap(),
                path.canonicalize().unwrap()
            );
        }
        assert_eq!(
            config.get("credential.helper").unwrap(),
            Some("fixture-local")
        );
    }

    #[test]
    fn single_value_reads_decode_only_the_effective_value() {
        let config = parse(
            b"global\0file:/unused\0alias.st\n\xff\0local\0file:.git/config\0alias.st\nstatus\0",
            Path::new("/unused"),
            &Environment::new(),
        )
        .unwrap();
        assert_eq!(config.get("alias.st").unwrap(), Some("status"));
        assert_eq!(config.get_all("alias.st")[0].2, &[0xff]);
    }

    #[test]
    fn child_environment_rejects_unapproved_names_before_spawn() {
        let (dir, base) = fixture();
        for name in [
            "PATH",
            "GIT_EXEC_PATH",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "UNRELATED",
        ] {
            let mut env = base.clone();
            env.insert(name.into(), "unused".into());
            let error = read(dir.path(), &[], &env).unwrap_err();
            assert!(
                error.contains(name) && error.contains("cannot verify"),
                "{error}"
            );
        }
    }

    #[test]
    fn oversized_configuration_is_bounded() {
        let (dir, mut env) = fixture();
        let global = dir.path().join("oversized");
        std::fs::write(
            &global,
            format!("[alias]\n large = {}\n", "x".repeat(4 * 1024 * 1024)),
        )
        .unwrap();
        env.insert("GIT_CONFIG_GLOBAL".into(), global.to_str().unwrap().into());
        let start = Instant::now();
        assert!(read(dir.path(), &[], &env).unwrap_err().contains("4 MiB"));
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn missing_unconditional_includes_are_empty_and_inventory_is_bounded() {
        let (dir, mut env) = fixture();
        let global = dir.path().join("global");
        env.insert("GIT_CONFIG_GLOBAL".into(), global.to_str().unwrap().into());
        std::fs::write(&global, "[include]\n path = missing\n").unwrap();
        assert!(read(dir.path(), &[], &env).unwrap().includes[0].missing);
        std::fs::write(&global, "[include]\n path = missing\n".repeat(129)).unwrap();
        assert!(read(dir.path(), &[], &env)
            .unwrap_err()
            .contains("128 includes"));
    }

    #[cfg(unix)]
    #[test]
    fn fifo_configuration_and_child_exit_share_the_deadline() {
        let (dir, mut env) = fixture();
        let fifo = dir.path().join("blocked-config");
        assert!(std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        env.insert("GIT_CONFIG_GLOBAL".into(), fifo.to_str().unwrap().into());
        let start = Instant::now();
        assert!(read(dir.path(), &[], &env)
            .unwrap_err()
            .contains("3 seconds"));
        assert!(start.elapsed() < Duration::from_secs(5));
        let mut child = std::process::Command::new("sh");
        child.args(["-c", "exec 1>&-; exec sleep 10"]);
        let start = Instant::now();
        assert!(bounded_output(&mut child, start + Duration::from_secs(3))
            .unwrap_err()
            .contains("3 seconds"));
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn newline_in_a_subsection_cannot_impersonate_an_alias_key() {
        let (dir, env) = fixture();
        let globals = vec![
            "-c".into(),
            "alias.ship=push origin v9.9.9".into(),
            "-c".into(),
            "alias.ship\npoison.x=log".into(),
        ];
        assert!(read(dir.path(), &globals, &env)
            .unwrap_err()
            .contains("cannot verify"));
    }
}
