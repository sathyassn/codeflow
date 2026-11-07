//! `codeflow ci base-checks` (TSK-1103): run the project checks declared in
//! the trusted commit's `.codeflow/base-checks.json` from that commit's own
//! copies of each script and its inputs, before the pull request is judged.
//!
//! Trust is bound to the commit `--trusted` names, resolved once: the
//! declaration, the scripts and their inputs are read from it, never from
//! the checkout, the head or the base. Each check runs in a fresh folder
//! that holds only those exported files, with a cleared environment and a
//! git directory of its own that borrows the repository's objects and holds
//! only the named commits, so the checkout's configuration and any
//! credentials persisted there are not handed to the script. The head is
//! git data the script may read; nothing from it is run here.
//!
//! The step fails closed: a declaration it cannot read, a declared file
//! that is missing, a symbolic link, a gitlink or unreadable, an interpreter
//! not on PATH, a check past its timeout or a check that exits non-zero
//! fails the run, naming the declaration and the path.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::Args;
use codeflow_core::base_checks::{
    self, describe_mode, entry_in, tree_at, Check, DECLARATION, MODE_EXECUTABLE, MODE_FILE,
    MODE_TREE,
};
use codeflow_core::remedy::{Finding, CI_BASE_CHECK_FAILED};

/// Arguments of `codeflow ci base-checks`.
#[derive(Debug, Args)]
pub struct BaseChecksArgs {
    /// The trusted commit whose declaration, scripts and inputs decide: on
    /// GitHub the default branch's commit the verdict job runs, elsewhere
    /// the target commit.
    #[arg(long, value_name = "REV")]
    pub trusted: String,

    /// The base of the judged range, passed to each check.
    #[arg(long, value_name = "REV")]
    pub base: String,

    /// The pull request's head, passed to each check as git data.
    #[arg(long, value_name = "REV")]
    pub head: String,

    /// The merge of the head into the base, when one exists; an empty value
    /// or none passes an empty argument (a conflicting pull request has no
    /// merge commit).
    #[arg(long, value_name = "REV", default_value = "")]
    pub merge: String,

    /// The repository the head comes from (`owner/name` on GitHub), passed
    /// as text.
    #[arg(long, value_name = "NAME", default_value = "")]
    pub head_repo: String,
}

const PLANE: &str = "codeflow ci base-checks";

/// Run the declared checks; the exit status is 0 when every check passed or
/// none is declared, 1 when one failed or the declaration cannot be read,
/// and 2 when a named commit cannot be resolved.
pub fn run(args: &BaseChecksArgs) -> i32 {
    let repo = match git2::Repository::discover(".") {
        Ok(repo) => repo,
        Err(error) => {
            eprintln!("{PLANE}: error: not in a git repository: {}", error.message());
            return 2;
        }
    };
    let resolve = |label: &str, rev: &str| -> Result<String, String> {
        repo.revparse_single(rev)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id().to_string())
            .map_err(|error| format!("the {label} commit {rev} cannot be resolved: {}", error.message()))
    };
    let named = (|| -> Result<_, String> {
        let trusted = resolve("trusted", &args.trusted)?;
        let base = resolve("base", &args.base)?;
        let head = resolve("head", &args.head)?;
        let merge = if args.merge.is_empty() {
            String::new()
        } else {
            resolve("merge", &args.merge)?
        };
        Ok((trusted, base, head, merge))
    })();
    let (trusted, base, head, merge) = match named {
        Ok(named) => named,
        Err(error) => {
            eprintln!(
                "{}",
                Finding::new(
                    format!("{error}; nothing is judged"),
                    codeflow_core::remedy::CI_RANGE_UNREADABLE.remedy()
                )
                .line(PLANE, "error")
            );
            return 2;
        }
    };
    let tree = match tree_at(&repo, &trusted) {
        Ok(tree) => tree,
        Err(error) => return refuse(&format!("{error}; nothing is judged")),
    };
    let declaration = match base_checks::read_in(&repo, &tree) {
        Ok(Some(declaration)) => declaration,
        Ok(None) => {
            println!("{PLANE}: the trusted commit {trusted} declares no checks ({DECLARATION} is absent)");
            return 0;
        }
        Err(error) => {
            return refuse(&format!(
                "the trusted commit {trusted}'s {error}; every pull request fails until the default branch's copy can be read"
            ))
        }
    };
    if declaration.checks.is_empty() {
        println!("{PLANE}: the trusted commit {trusted}'s {DECLARATION} declares no checks");
        return 0;
    }
    println!(
        "{PLANE}: {} check(s) from {DECLARATION} at the trusted commit {trusted}; base {base}, head {head}, merge {}, head repository {}",
        declaration.checks.len(),
        if merge.is_empty() { "(none)" } else { &merge },
        if args.head_repo.is_empty() { "(not given)" } else { &args.head_repo },
    );
    let work = match tempfile::Builder::new().prefix("codeflow-base-checks-").tempdir() {
        Ok(work) => work,
        Err(error) => return refuse(&format!("cannot make a working folder: {error}")),
    };
    let git_dir = work.path().join("git");
    if let Err(error) = borrowed_git_dir(&repo, &git_dir, &[
        ("trusted", &trusted),
        ("base", &base),
        ("head", &head),
        ("merge", &merge),
    ]) {
        return refuse(&format!("cannot prepare the checks' git directory: {error}"));
    }
    let run = Run {
        trusted: &trusted,
        base: &base,
        head: &head,
        merge: &merge,
        head_repo: &args.head_repo,
        git_dir: &git_dir,
    };
    let mut failed = 0;
    for (index, check) in declaration.checks.iter().enumerate() {
        let folder = work.path().join(format!("check-{index}"));
        match run_check(&repo, &tree, check, &folder, &run) {
            Ok(()) => println!("{PLANE}: {} passed", check.name),
            Err(why) => {
                failed += 1;
                eprintln!(
                    "{}",
                    Finding::new(
                        format!("check {} ({} in {DECLARATION}) {why}", check.name, check.script),
                        CI_BASE_CHECK_FAILED.remedy()
                    )
                    .line(PLANE, "error")
                );
            }
        }
    }
    if failed > 0 {
        eprintln!(
            "{PLANE}: {failed} of {} check(s) failed; the pull request is refused",
            declaration.checks.len()
        );
        return 1;
    }
    0
}

fn refuse(text: &str) -> i32 {
    eprintln!(
        "{}",
        Finding::new(text.to_string(), CI_BASE_CHECK_FAILED.remedy()).line(PLANE, "error")
    );
    1
}

/// What every check of one run receives.
struct Run<'a> {
    trusted: &'a str,
    base: &'a str,
    head: &'a str,
    merge: &'a str,
    head_repo: &'a str,
    git_dir: &'a Path,
}

/// A bare git directory at `dir` that reads the repository's objects
/// through `objects/info/alternates`, holds `refs/codeflow/<name>` for each
/// named commit and points HEAD at the trusted one. It carries no
/// configuration from the repository, so a credential the checkout
/// persisted there stays out of the checks' reach.
fn borrowed_git_dir(
    repo: &git2::Repository,
    dir: &Path,
    refs: &[(&str, &str)],
) -> Result<(), String> {
    let bare = git2::Repository::init_bare(dir).map_err(|error| error.message().to_string())?;
    let objects = repo.commondir().join("objects");
    let objects = objects
        .canonicalize()
        .map_err(|error| format!("cannot find {}: {error}", objects.display()))?;
    let alternates = dir.join("objects").join("info").join("alternates");
    std::fs::create_dir_all(alternates.parent().unwrap_or(dir))
        .and_then(|()| std::fs::write(&alternates, format!("{}\n", objects.display())))
        .map_err(|error| format!("cannot write {}: {error}", alternates.display()))?;
    // Reopen so the object database reads the alternates just written.
    drop(bare);
    let bare = git2::Repository::open_bare(dir).map_err(|error| error.message().to_string())?;
    for (name, sha) in refs {
        if sha.is_empty() {
            continue;
        }
        let oid = git2::Oid::from_str(sha).map_err(|error| error.message().to_string())?;
        bare.reference(&format!("refs/codeflow/{name}"), oid, true, "codeflow base checks")
            .map_err(|error| format!("cannot name {name} {sha}: {}", error.message()))?;
    }
    bare.set_head("refs/codeflow/trusted")
        .map_err(|error| error.message().to_string())?;
    Ok(())
}

/// Export the check's script and inputs from the trusted tree into
/// `folder/tree`, then run it there; `Err` says why it failed.
fn run_check(
    repo: &git2::Repository,
    tree: &git2::Tree<'_>,
    check: &Check,
    folder: &Path,
    run: &Run<'_>,
) -> Result<(), String> {
    let root = folder.join("tree");
    let home = folder.join("home");
    let temp = folder.join("tmp");
    for dir in [&root, &home, &temp] {
        std::fs::create_dir_all(dir).map_err(|error| format!("cannot make {}: {error}", dir.display()))?;
    }
    export(repo, tree, &check.script, &root, false)?;
    for input in &check.inputs {
        export(repo, tree, input, &root, true)?;
    }
    let interpreter = on_path(&check.interpreter).ok_or_else(|| {
        format!(
            "cannot run: its interpreter {} is not on PATH",
            check.interpreter
        )
    })?;
    let empty_config = folder.join("gitconfig");
    std::fs::write(&empty_config, "").map_err(|error| format!("cannot write {}: {error}", empty_config.display()))?;
    let mut command = Command::new(&interpreter);
    command
        .arg(root.join(&check.script))
        .args([run.base, run.head, run.merge, run.head_repo])
        .current_dir(&root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("TMPDIR", &temp)
        .env("LANG", "C.UTF-8")
        .env("GIT_DIR", run.git_dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &empty_config)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("CODEFLOW_CHECK_NAME", &check.name)
        .env("CODEFLOW_TRUSTED_SHA", run.trusted)
        .env("CODEFLOW_TRUSTED_ROOT", &root)
        .env("CODEFLOW_BASE_SHA", run.base)
        .env("CODEFLOW_HEAD_SHA", run.head)
        .env("CODEFLOW_MERGE_SHA", run.merge)
        .env("CODEFLOW_HEAD_REPO", run.head_repo)
        .stdin(Stdio::null());
    #[cfg(windows)]
    for key in ["SYSTEMROOT", "COMSPEC", "PATHEXT"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", interpreter.display()))?;
    let limit = Duration::from_secs(check.timeout());
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= limit => {
                stop(&mut child);
                return Err(format!(
                    "ran past its timeout of {} seconds and was stopped",
                    check.timeout()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                stop(&mut child);
                return Err(format!("cannot be waited on: {error}"));
            }
        }
    };
    if status.success() {
        Ok(())
    } else {
        Err(match status.code() {
            Some(code) => format!("failed (exit {code})"),
            None => "was ended by a signal".to_string(),
        })
    }
}

/// Stop the check and everything it started.
fn stop(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: kill(2) with a negative pid signals the process group the
        // child leads (`process_group(0)` above); it touches no memory.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Write the trusted tree's `path` under `root`. A script must be a regular
/// file; an input may also be a folder, whose every entry must be a regular
/// file or a folder.
fn export(
    repo: &git2::Repository,
    tree: &git2::Tree<'_>,
    path: &str,
    root: &Path,
    folder_allowed: bool,
) -> Result<(), String> {
    let what = if folder_allowed { "input" } else { "script" };
    let Some((mode, id)) = entry_in(tree, path)? else {
        return Err(format!("cannot run: its {what} {path} is missing on the trusted commit"));
    };
    match mode {
        MODE_FILE | MODE_EXECUTABLE => write_blob(repo, id, path, &root.join(path)),
        MODE_TREE if folder_allowed => {
            let sub = repo
                .find_tree(id)
                .map_err(|error| format!("cannot run: its input {path} cannot be read: {}", error.message()))?;
            let mut failure = None;
            sub.walk(git2::TreeWalkMode::PreOrder, |prefix, entry| {
                let name = String::from_utf8_lossy(entry.name_bytes());
                let inner = format!("{path}/{prefix}{name}");
                let result = match entry.filemode() {
                    MODE_TREE => return git2::TreeWalkResult::Ok,
                    MODE_FILE | MODE_EXECUTABLE if entry.name().is_ok() => {
                        write_blob(repo, entry.id(), &inner, &root.join(&inner))
                    }
                    other => Err(format!(
                        "cannot run: its input {inner} is {} on the trusted commit",
                        describe_mode(other)
                    )),
                };
                match result {
                    Ok(()) => git2::TreeWalkResult::Ok,
                    Err(why) => {
                        failure = Some(why);
                        git2::TreeWalkResult::Abort
                    }
                }
            })
            .ok();
            failure.map_or(Ok(()), Err)
        }
        other => Err(format!(
            "cannot run: its {what} {path} is {} on the trusted commit",
            describe_mode(other)
        )),
    }
}

fn write_blob(repo: &git2::Repository, id: git2::Oid, shown: &str, to: &Path) -> Result<(), String> {
    let blob = repo
        .find_blob(id)
        .map_err(|error| format!("cannot run: {shown} cannot be read on the trusted commit: {}", error.message()))?;
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("cannot run: cannot make {}: {error}", parent.display()))?;
    }
    std::fs::write(to, blob.content()).map_err(|error| format!("cannot run: cannot write {shown}: {error}"))
}

/// The first executable file named `name` on PATH.
fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidates = if cfg!(windows) {
            vec![dir.join(format!("{name}.exe")), dir.join(name)]
        } else {
            vec![dir.join(name)]
        };
        candidates.into_iter().find(|candidate| executable(candidate))
    })
}

fn executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
