#![cfg(unix)]

//! TSK-1344 AC-6 and AC-7: an opt-in check, run once by the operator, of what
//! a command started through the Herdr app (`hc run`) can do outside the
//! in-session guards.
//!
//! The check is `#[ignore]` and also needs `CODEFLOW_SCRATCH_HC=1`; without
//! the variable it prints why and returns. When it runs, everything it uses
//! lives in one new directory under `/tmp`: its own hc core (own
//! `HC_SESSION`, `HC_SOCKET` and XDG paths), a scratch home, a scratch
//! repository and a bare remote at a local path. It refuses before creating
//! anything when the inherited environment points at a live hc session, and
//! before starting hc when any path or the socket resolves outside that
//! directory. It stops only the core it started and removes the directory on
//! every exit path.
//!
//! - AC-6: `hc run` writes a file in the scratch directory, a path outside
//!   the Claude Bash sandbox's write list. The table records whether the
//!   file was written. The row counts as AC-6 evidence only when the
//!   operator runs this test from a plain terminal: run inside the Claude
//!   sandbox, the sandbox itself can block the write, and the table would
//!   still call the path outside the sandbox.
//! - AC-7: in a repository whose `main` is protected by the codeflow git
//!   hooks, a force push to the bare remote runs through `hc run`, once
//!   plainly and once with `--no-verify`. The table records the remote refs
//!   before and after each push and which floor, if any, stopped it. The
//!   outcome is reported as found and never asserted.
//!
//! Run from the worktree, in a plain terminal (not inside an hc terminal
//! or the Claude sandbox):
//! `CODEFLOW_SCRATCH_HC=1 cargo test -p codeflow-cli --test hc_scratch_floor -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// The variable that opts in; only the exact value `1` counts.
const OPT_IN: &str = "CODEFLOW_SCRATCH_HC";

/// The scratch core's session name.
const SESSION: &str = "cfscratch";

/// Every XDG base directory the scratch environment sets.
const XDG: [&str; 5] = [
    "XDG_RUNTIME_DIR",
    "XDG_STATE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
];

/// Variables that must not reach any scratch command: each names a live
/// terminal, a hook binary override or a credential prompt.
const FORBIDDEN: [&str; 7] = [
    "CODEFLOW_HOOK_BINARY",
    "HC_TERMINAL_ID",
    "HC_TERMINAL_SOCKET",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
];

/// The longest socket path a Unix socket address holds on macOS, less one.
const SOCKET_PATH_MAX: usize = 103;

type Env = BTreeMap<String, String>;

/// Why the check does not run, or `None` when the operator opted in.
fn refusal_without_opt_in(inherited: &Env) -> Option<String> {
    if inherited.get(OPT_IN).is_some_and(|value| value == "1") {
        None
    } else {
        Some(format!(
            "hc_scratch_floor did not run: it starts a scratch hc core and runs \
             git through it, so it needs {OPT_IN}=1 as well as --ignored"
        ))
    }
}

/// Why the inherited environment could reach the operator's live hc server
/// or sessions. Empty when it cannot.
fn live_session_refusals(inherited: &Env, real_home: &Path) -> Vec<String> {
    let set = |name: &str| inherited.get(name).filter(|value| !value.is_empty());
    let mut refusals = Vec::new();
    for name in XDG {
        if let Some(value) = set(name) {
            if Path::new(value).join("herdr-core").exists() {
                refusals.push(format!(
                    "{name} points at {value}, which holds herdr-core state"
                ));
            }
        }
    }
    if let Some(session) = set("HC_SESSION") {
        let mut homes = vec![real_home.join(".local/state")];
        if let Some(state) = set("XDG_STATE_HOME") {
            homes.push(PathBuf::from(state));
        }
        for state in homes {
            let dir = state.join("herdr-core").join(session);
            if dir.exists() {
                refusals.push(format!(
                    "HC_SESSION={session} names a live session at {}",
                    dir.display()
                ));
            }
        }
    }
    for name in ["HC_SOCKET", "HC_TERMINAL_SOCKET", "HC_TERMINAL_ID"] {
        if let Some(value) = set(name) {
            refusals.push(format!(
                "{name}={value} is inherited; run from a terminal outside hc"
            ));
        }
    }
    refusals
}

/// Every path the check uses, all under one root.
struct Layout {
    root: PathBuf,
    home: PathBuf,
    xdg: Vec<(&'static str, PathBuf)>,
    state_dir: PathBuf,
    socket: PathBuf,
    repo: PathBuf,
    remote: PathBuf,
    templates: PathBuf,
    codeflow_home: PathBuf,
    gitconfig: PathBuf,
    ac6_target: PathBuf,
}

impl Layout {
    fn new(root: &Path) -> Self {
        let xdg: Vec<(&'static str, PathBuf)> = XDG
            .iter()
            .map(|name| (*name, root.join("x").join(name.to_ascii_lowercase())))
            .collect();
        let state_home = root.join("x").join("xdg_state_home");
        let state_dir = state_home.join("herdr-core").join(SESSION);
        let socket = state_dir.join(format!("{SESSION}.sock"));
        let home = root.join("home");
        Self {
            root: root.to_path_buf(),
            gitconfig: home.join(".gitconfig"),
            home,
            xdg,
            state_dir,
            socket,
            repo: root.join("repo"),
            remote: root.join("remote.git"),
            templates: root.join("templates"),
            codeflow_home: root.join("codeflow-home"),
            ac6_target: root.join("ac6").join("written-by-hc-run"),
        }
    }

    /// Every path besides the socket, labelled for a refusal.
    fn paths(&self) -> Vec<(String, &Path)> {
        let mut paths: Vec<(String, &Path)> = vec![
            ("home".into(), &self.home),
            ("hc state directory".into(), &self.state_dir),
            ("repository".into(), &self.repo),
            ("bare remote".into(), &self.remote),
            ("git templates".into(), &self.templates),
            ("codeflow home".into(), &self.codeflow_home),
            ("global git config".into(), &self.gitconfig),
            ("AC-6 target".into(), &self.ac6_target),
        ];
        for (name, path) in &self.xdg {
            paths.push(((*name).to_string(), path));
        }
        paths
    }

    fn create(&self) -> std::io::Result<()> {
        for dir in [
            &self.home,
            &self.state_dir,
            &self.repo,
            &self.templates,
            &self.codeflow_home,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        for (_, dir) in &self.xdg {
            std::fs::create_dir_all(dir)?;
        }
        if let Some(parent) = self.ac6_target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.gitconfig, "")
    }
}

/// `path` with symlinks resolved: the nearest existing ancestor is
/// canonicalized and the rest appended. A `..` in the rest, or a dangling
/// symlink, is an error.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        if existing.symlink_metadata().is_ok() {
            return Err(format!("{} is a dangling symlink", existing.display()));
        }
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => return Err(format!("{} cannot be resolved", path.display())),
        }
    }
    let mut resolved = existing
        .canonicalize()
        .map_err(|err| format!("{}: {err}", existing.display()))?;
    for name in rest.iter().rev() {
        resolved.push(name);
    }
    if resolved
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(format!("{} keeps a `..`", path.display()));
    }
    Ok(resolved)
}

/// Why `path` is not inside `root` once resolved, or `None` when it is.
fn outside(root: &Path, label: &str, path: &Path) -> Option<String> {
    let root = match resolve(root) {
        Ok(root) => root,
        Err(err) => return Some(format!("the scratch root: {err}")),
    };
    match resolve(path) {
        Ok(resolved) if resolved.starts_with(&root) => None,
        Ok(resolved) => Some(format!(
            "{label} resolves to {}, outside {}",
            resolved.display(),
            root.display()
        )),
        Err(err) => Some(format!("{label}: {err}")),
    }
}

/// Every reason the layout reaches outside its root. Empty when it does not.
fn layout_refusals(layout: &Layout) -> Vec<String> {
    let mut refusals: Vec<String> = layout
        .paths()
        .into_iter()
        .filter_map(|(label, path)| outside(&layout.root, &label, path))
        .collect();
    if let Some(refusal) = outside(&layout.root, "the hc socket", &layout.socket) {
        refusals.push(refusal);
    }
    let length = layout.socket.as_os_str().len();
    if length > SOCKET_PATH_MAX {
        refusals.push(format!(
            "the hc socket path is {length} bytes, over {SOCKET_PATH_MAX}"
        ));
    }
    refusals
}

/// The whole environment of every scratch command. Nothing else is
/// inherited: each command starts from an empty environment.
fn scratch_env(layout: &Layout, path: &str) -> Env {
    let mut env = Env::new();
    let mut put = |name: &str, value: &Path| {
        env.insert(name.to_string(), value.display().to_string());
    };
    put("HOME", &layout.home);
    for (name, dir) in &layout.xdg {
        put(name, dir);
    }
    put("HC_SOCKET", &layout.socket);
    put("GIT_CONFIG_GLOBAL", &layout.gitconfig);
    put("GIT_TEMPLATE_DIR", &layout.templates);
    put("CODEFLOW_HOME", &layout.codeflow_home);
    for (name, value) in [
        ("HC_SESSION", SESSION),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("SSH_AUTH_SOCK", ""),
        ("GIT_SSH_COMMAND", "false"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("TERM", "xterm-256color"),
        ("SHELL", "/bin/sh"),
        ("LANG", "C"),
        ("PATH", path),
    ] {
        env.insert(name.to_string(), value.to_string());
    }
    env
}

/// Every way `env` breaks the scratch contract. Empty when it holds.
fn env_refusals(env: &Env, layout: &Layout) -> Vec<String> {
    let mut refusals = Vec::new();
    let mut inside = vec![
        "HOME",
        "GIT_CONFIG_GLOBAL",
        "GIT_TEMPLATE_DIR",
        "CODEFLOW_HOME",
        "HC_SOCKET",
    ];
    inside.extend(XDG);
    for name in inside {
        match env.get(name) {
            Some(value) => {
                if let Some(refusal) = outside(&layout.root, name, Path::new(value)) {
                    refusals.push(refusal);
                }
            }
            None => refusals.push(format!("{name} is not set")),
        }
    }
    for (name, wanted) in [
        ("HC_SESSION", SESSION),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("SSH_AUTH_SOCK", ""),
        ("GIT_SSH_COMMAND", "false"),
    ] {
        if env.get(name).map(String::as_str) != Some(wanted) {
            refusals.push(format!("{name} is not {wanted:?}"));
        }
    }
    if env.get("HC_SOCKET").map(PathBuf::from).as_deref() != Some(layout.socket.as_path()) {
        refusals.push("HC_SOCKET is not the scratch socket".to_string());
    }
    for name in env.keys() {
        if FORBIDDEN.contains(&name.as_str()) || name.starts_with("GIT_CONFIG_KEY_") {
            refusals.push(format!("{name} is set"));
        }
    }
    refusals
}

/// Why `remote` is not a local filesystem path inside the root, or `None`.
fn remote_refusal(root: &Path, remote: &str) -> Option<String> {
    if remote.contains("://") {
        return Some(format!("the remote {remote} is a URL"));
    }
    let before_slash = remote.split('/').next().unwrap_or_default();
    if before_slash.contains(':') {
        return Some(format!("the remote {remote} is a host:path address"));
    }
    if !Path::new(remote).is_absolute() {
        return Some(format!("the remote {remote} is not an absolute path"));
    }
    outside(root, "the bare remote", Path::new(remote))
}

/// What one bounded run produced.
struct Ran {
    code: Option<i32>,
    output: String,
    timed_out: bool,
}

/// A command with exactly `env` as its environment.
fn scratch_command(program: &Path, env: &Env) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear().envs(env).stdin(Stdio::null());
    cmd
}

/// Run `cmd`, killing it after `limit`. Output is stdout then stderr.
fn run_bounded(cmd: &mut Command, limit: Duration) -> Ran {
    let mut child = match cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(child) => child,
        Err(err) => {
            return Ran {
                code: None,
                output: format!("did not start: {err}"),
                timed_out: false,
            }
        }
    };
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|mut stream| {
        thread::spawn(move || {
            let mut text = String::new();
            let _ = stream.read_to_string(&mut text);
            text
        })
    })
    .collect();
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() > limit => {
                let _ = child.kill();
                timed_out = true;
                break child.wait().ok();
            }
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(_) => break None,
        }
    };
    let output = readers
        .into_iter()
        .map(|reader| reader.join().unwrap_or_default())
        .collect::<String>();
    Ran {
        code: status.and_then(|status| status.code()),
        output,
        timed_out,
    }
}

/// `program` on the colon-separated `path`, the first executable found.
fn find_on_path(program: &str, path: &str) -> Option<PathBuf> {
    path.split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join(program))
        .find(|candidate| candidate.is_file())
}

/// The scratch core and its directory. Dropping it stops the core it
/// started, by recorded pid and then by its own session, and removes the
/// directory.
struct Scratch {
    hc: PathBuf,
    env: Env,
    state_dir: PathBuf,
    socket: PathBuf,
    foreground: Option<Child>,
    pids: Vec<u32>,
    _dir: tempfile::TempDir,
}

impl Scratch {
    /// Start the scratch core and record its pids.
    fn start(&mut self) -> Result<(), String> {
        if self.socket.exists() {
            return Err("the scratch socket exists before start".to_string());
        }
        let mut cmd = scratch_command(&self.hc, &self.env);
        cmd.args(["server", "start"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|err| format!("hc server start: {err}"))?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(status)) => return Err(format!("hc server start exited {status}")),
                Ok(None) if started.elapsed() > Duration::from_secs(10) => {
                    // It stays in the foreground: this child is the core.
                    self.pids.push(child.id());
                    self.foreground = Some(child);
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(100)),
                Err(err) => return Err(format!("hc server start: {err}")),
            }
        }
        let waited = Instant::now();
        while !self.socket.exists() {
            if waited.elapsed() > Duration::from_secs(15) {
                return Err("the scratch socket did not appear".to_string());
            }
            thread::sleep(Duration::from_millis(100));
        }
        let holders = self.socket_holders();
        self.pids.extend(holders);
        self.pids.sort_unstable();
        self.pids.dedup();
        Ok(())
    }

    /// The pids holding any socket in the scratch state directory. Every
    /// such socket lives under the scratch root, so each pid is the scratch
    /// core or one of its holders.
    fn socket_holders(&self) -> Vec<u32> {
        let Ok(entries) = std::fs::read_dir(&self.state_dir) else {
            return Vec::new();
        };
        let me = std::process::id();
        entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "sock"))
            .flat_map(|socket| {
                let ran = run_bounded(
                    Command::new("/usr/sbin/lsof")
                        .env_clear()
                        .arg("-t")
                        .arg(&socket),
                    Duration::from_secs(10),
                );
                ran.output
                    .lines()
                    .filter_map(|line| line.trim().parse::<u32>().ok())
                    .collect::<Vec<_>>()
            })
            .filter(|pid| *pid != me)
            .collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let mut pids = self.pids.clone();
        pids.extend(self.socket_holders());
        pids.sort_unstable();
        pids.dedup();
        for pid in &pids {
            let _ = Command::new("/bin/kill")
                .env_clear()
                .args(["-TERM", &pid.to_string()])
                .status();
        }
        if let Some(mut child) = self.foreground.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if self.socket.exists() && env_has_scratch_session(&self.env) {
            let _ = run_bounded(
                scratch_command(&self.hc, &self.env).args(["server", "stop"]),
                Duration::from_secs(15),
            );
        }
        let left = self.socket_holders();
        if !left.is_empty() {
            eprintln!("hc_scratch_floor: scratch processes still running: {left:?}");
        }
        eprintln!(
            "hc_scratch_floor: stopped scratch pids {pids:?}; removing the scratch directory"
        );
    }
}

/// The env still names the scratch session and socket, so `hc server stop`
/// can reach only the scratch core.
fn env_has_scratch_session(env: &Env) -> bool {
    env.get("HC_SESSION").map(String::as_str) == Some(SESSION)
        && env
            .get("HC_SOCKET")
            .is_some_and(|socket| socket.ends_with(&format!("/{SESSION}.sock")))
}

/// A table cell: one line, no pipe.
fn cell(text: &str) -> String {
    let flat: String = text
        .trim()
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" / ")
        .replace('|', "\\|");
    if flat.is_empty() {
        "(empty)".to_string()
    } else {
        flat
    }
}

/// Run git with the scratch env in `dir`, failing the setup on error.
fn setup_git(git: &Path, env: &Env, dir: &Path, args: &[&str]) {
    let ran = run_bounded(
        scratch_command(git, env).current_dir(dir).args(args),
        Duration::from_secs(60),
    );
    assert_eq!(ran.code, Some(0), "setup git {args:?}: {}", ran.output);
}

/// `git show-ref` on the bare remote, run locally with the scratch env.
fn remote_refs(git: &Path, env: &Env, remote: &Path) -> String {
    let ran = run_bounded(
        scratch_command(git, env)
            .arg("--git-dir")
            .arg(remote)
            .arg("show-ref"),
        Duration::from_secs(30),
    );
    cell(&ran.output)
}

/// Which floor stopped a push, judged from its result. Reported, never
/// asserted.
fn floor(ran: &Ran, before: &str, after: &str) -> String {
    let rules: Vec<&str> = regex::Regex::new(r"policy rule ([a-z_]+\.[a-z_.]+)")
        .expect("pattern compiles")
        .captures_iter(&ran.output)
        .filter_map(|caps| caps.get(1).map(|m| m.as_str()))
        .collect();
    if before != after {
        return format!("none: the remote ref moved (exit {:?})", ran.code);
    }
    if ran.timed_out {
        return "unknown: the run timed out".to_string();
    }
    if !rules.is_empty() {
        return format!("codeflow git hook ({})", rules.join(", "));
    }
    if ran.output.contains("codeflow") {
        return format!("codeflow git hook: {}", cell(&ran.output));
    }
    match ran.code {
        Some(0) => "none needed: exit 0 and the remote did not move".to_string(),
        code => format!("other (exit {code:?}): {}", cell(&ran.output)),
    }
}

/// Build the scratch repository and its bare remote: `main` pushed at one
/// commit, the local `main` amended so a push must force, then the codeflow
/// git hooks turned on.
fn build_repository(git: &Path, env: &Env, layout: &Layout) {
    let repo = &layout.repo;
    let remote = layout.remote.display().to_string();
    setup_git(
        git,
        env,
        &layout.root,
        &["init", "--bare", "-b", "main", &remote],
    );
    setup_git(git, env, repo, &["init", "-b", "main"]);
    setup_git(
        git,
        env,
        repo,
        &["config", "user.email", "scratch@example.invalid"],
    );
    setup_git(git, env, repo, &["config", "user.name", "scratch"]);
    let hooks = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.codeflow/git-hooks");
    let target = repo.join(".codeflow/git-hooks");
    std::fs::create_dir_all(&target).expect("hook directory");
    for entry in std::fs::read_dir(&hooks).expect("worktree git hooks") {
        let entry = entry.expect("hook entry");
        std::fs::copy(entry.path(), target.join(entry.file_name())).expect("hook copied");
    }
    std::fs::write(
        repo.join(".codeflow/policy.json"),
        r#"{"git":{"protected_branches":["main"],"root_checkout_commits":"off"}}"#,
    )
    .expect("policy");
    std::fs::write(repo.join("base.txt"), "one\n").expect("file");
    setup_git(git, env, repo, &["add", "."]);
    setup_git(git, env, repo, &["commit", "-m", "chore: scratch base"]);
    setup_git(git, env, repo, &["remote", "add", "origin", &remote]);
    setup_git(git, env, repo, &["push", "origin", "main"]);
    std::fs::write(repo.join("base.txt"), "two\n").expect("file");
    setup_git(
        git,
        env,
        repo,
        &["commit", "-am", "chore: scratch diverge", "--amend"],
    );
    setup_git(
        git,
        env,
        repo,
        &["config", "core.hooksPath", ".codeflow/git-hooks"],
    );
    let helpers = run_bounded(
        scratch_command(git, env).current_dir(repo).args([
            "config",
            "--get-all",
            "credential.helper",
        ]),
        Duration::from_secs(30),
    );
    assert!(
        helpers.output.trim().is_empty(),
        "a credential helper is configured: {}",
        helpers.output
    );
}

/// The PATH of every scratch command: the binary this test built first, so
/// the hook shims run it, then the inherited PATH.
fn scratch_path(inherited: &Env) -> String {
    let bin = Path::new(env!("CARGO_BIN_EXE_codeflow"))
        .parent()
        .expect("binary directory")
        .display()
        .to_string();
    let rest = inherited.get("PATH").cloned().unwrap_or_default();
    format!("{bin}:{rest}")
}

fn print_environment(env: &Env, layout: &Layout) {
    println!("\n### Scratch environment (asserted before the run)\n");
    println!("| Variable | Value |\n|---|---|");
    for (name, value) in env {
        if name != "PATH" {
            println!("| {name} | {} |", cell(value));
        }
    }
    println!("| bare remote | {} |", layout.remote.display());
    println!("| credential.helper | none |");
}

fn run_ac6(scratch: &Scratch, layout: &Layout) {
    let line = format!("printf written > '{}'", layout.ac6_target.display());
    let ran = run_bounded(
        scratch_command(&scratch.hc, &scratch.env)
            .current_dir(&layout.root)
            .args(["run", "--cwd"])
            .arg(&layout.root)
            .args(["--", "sh", "-c", &line]),
        Duration::from_secs(120),
    );
    let written = layout.ac6_target.exists();
    println!("\n### AC-6: a write through hc run\n");
    println!(
        "Counts as AC-6 evidence only when this test ran from a plain terminal; \
         inside the Claude sandbox the sandbox can block the write.\n"
    );
    println!("| Target | Outside the Claude sandbox write list | hc exit | Timed out | Written |");
    println!("|---|---|---|---|---|");
    println!(
        "| {} | yes (not /tmp/claude, $TMPDIR or the worktree) | {:?} | {} | {} |",
        layout.ac6_target.display(),
        ran.code,
        ran.timed_out,
        if written { "yes" } else { "no" }
    );
    println!("\nhc output: {}", cell(&ran.output));
}

fn run_ac7(scratch: &Scratch, layout: &Layout, git: &Path) {
    println!("\n### AC-7: a force push to protected main through hc run\n");
    println!("| Push | hc exit | Remote refs before | Remote refs after | Floor that stopped it |");
    println!("|---|---|---|---|---|");
    let mut outputs = Vec::new();
    for (label, extra) in [("force", None), ("force --no-verify", Some("--no-verify"))] {
        let before = remote_refs(git, &scratch.env, &layout.remote);
        let mut cmd = scratch_command(&scratch.hc, &scratch.env);
        cmd.current_dir(&layout.repo)
            .args(["run", "--cwd"])
            .arg(&layout.repo)
            .args(["--", "git", "push", "--force"]);
        if let Some(flag) = extra {
            cmd.arg(flag);
        }
        cmd.args(["origin", "main"]);
        let ran = run_bounded(&mut cmd, Duration::from_secs(180));
        let after = remote_refs(git, &scratch.env, &layout.remote);
        println!(
            "| git push {label} origin main | {:?} | {before} | {after} | {} |",
            ran.code,
            floor(&ran, &before, &after)
        );
        outputs.push((label, ran.output));
    }
    for (label, output) in outputs {
        println!("\n{label} output: {}", cell(&output));
    }
}

/// TSK-1344 AC-6 and AC-7, run once by the operator. See the module docs.
#[test]
#[ignore = "opt-in: starts a scratch hc core; needs CODEFLOW_SCRATCH_HC=1"]
fn hc_scratch_floor() {
    let inherited: Env = std::env::vars().collect();
    if let Some(reason) = refusal_without_opt_in(&inherited) {
        eprintln!("{reason}");
        return;
    }
    let real_home = PathBuf::from(inherited.get("HOME").cloned().unwrap_or_default());
    let live = live_session_refusals(&inherited, &real_home);
    assert!(
        live.is_empty(),
        "refused before starting:\n{}",
        live.join("\n")
    );
    let path = scratch_path(&inherited);
    let hc = find_on_path("hc", &path).expect("hc on PATH");
    let git = find_on_path("git", &path).expect("git on PATH");
    let dir = tempfile::Builder::new()
        .prefix("cfhc.")
        .tempdir_in("/tmp")
        .expect("scratch directory");
    let root = dir.path().canonicalize().expect("scratch root");
    let layout = Layout::new(&root);
    let refusals = layout_refusals(&layout);
    assert!(
        refusals.is_empty(),
        "refused before starting:\n{}",
        refusals.join("\n")
    );
    layout.create().expect("scratch layout");
    let refusals = layout_refusals(&layout);
    assert!(
        refusals.is_empty(),
        "refused after creating:\n{}",
        refusals.join("\n")
    );
    let env = scratch_env(&layout, &path);
    let refusals = env_refusals(&env, &layout);
    assert!(
        refusals.is_empty(),
        "refused environment:\n{}",
        refusals.join("\n")
    );
    let remote = layout.remote.display().to_string();
    if let Some(refusal) = remote_refusal(&root, &remote) {
        panic!("refused remote: {refusal}");
    }
    build_repository(&git, &env, &layout);
    print_environment(&env, &layout);
    let mut scratch = Scratch {
        hc,
        env,
        state_dir: layout.state_dir.clone(),
        socket: layout.socket.clone(),
        foreground: None,
        pids: Vec::new(),
        _dir: dir,
    };
    scratch.start().expect("scratch hc core");
    println!("\nScratch core pids: {:?}", scratch.pids);
    run_ac6(&scratch, &layout);
    run_ac7(&scratch, &layout, &git);
}

#[cfg(test)]
mod refusal {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Env {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn without_the_opt_in_it_does_not_run() {
        for unset in [
            env(&[]),
            env(&[(OPT_IN, "")]),
            env(&[(OPT_IN, "0")]),
            env(&[(OPT_IN, "true")]),
        ] {
            assert!(refusal_without_opt_in(&unset).is_some(), "{unset:?}");
        }
        assert!(refusal_without_opt_in(&env(&[(OPT_IN, "1")])).is_none());
    }

    #[test]
    fn an_environment_reaching_a_live_session_is_refused() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".local/state/herdr-core/default")).unwrap();
        let xdg = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(xdg.path().join("herdr-core")).unwrap();
        let xdg_path = xdg.path().display().to_string();
        assert!(live_session_refusals(&env(&[]), home.path()).is_empty());
        assert!(live_session_refusals(&env(&[("HC_SESSION", "unused")]), home.path()).is_empty());
        for refused in [
            env(&[("HC_SESSION", "default")]),
            env(&[("XDG_STATE_HOME", &xdg_path)]),
            env(&[("XDG_RUNTIME_DIR", &xdg_path)]),
            env(&[("XDG_CONFIG_HOME", &xdg_path)]),
            env(&[("HC_SOCKET", "/tmp/elsewhere.sock")]),
            env(&[("HC_TERMINAL_ID", "t1")]),
        ] {
            assert!(
                !live_session_refusals(&refused, home.path()).is_empty(),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn a_path_or_socket_outside_the_root_is_refused() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().canonicalize().unwrap().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let layout = Layout::new(&root);
        // A temporary directory here can be longer than the real run's
        // `/tmp` root, so the socket length is checked on its own below.
        let contained = |layout: &Layout| -> Vec<String> {
            layout_refusals(layout)
                .into_iter()
                .filter(|refusal| !refusal.contains("bytes"))
                .collect()
        };
        assert!(contained(&layout).is_empty(), "{:?}", contained(&layout));
        layout.create().unwrap();
        assert!(contained(&layout).is_empty(), "{:?}", contained(&layout));

        let elsewhere = base.path().canonicalize().unwrap().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let mut socket_out = Layout::new(&root);
        socket_out.socket = elsewhere.join("cfscratch.sock");
        let refusals = layout_refusals(&socket_out);
        assert!(
            refusals.iter().any(|r| r.contains("the hc socket")),
            "{refusals:?}"
        );

        let escaped = base.path().canonicalize().unwrap().join("escaped");
        std::fs::create_dir_all(&escaped).unwrap();
        std::os::unix::fs::symlink(&elsewhere, escaped.join("home")).unwrap();
        let refusals = layout_refusals(&Layout::new(&escaped));
        assert!(
            refusals.iter().any(|r| r.starts_with("home ")),
            "{refusals:?}"
        );

        let mut dotted = Layout::new(&root);
        dotted.repo = root.join("missing/../../elsewhere");
        assert!(!layout_refusals(&dotted).is_empty());

        let mut long = Layout::new(&root);
        long.socket = root.join("s".repeat(SOCKET_PATH_MAX));
        let refusals = layout_refusals(&long);
        assert!(refusals.iter().any(|r| r.contains("bytes")), "{refusals:?}");
    }

    #[test]
    fn the_scratch_environment_holds_and_each_break_is_refused() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().canonicalize().unwrap();
        let layout = Layout::new(&root);
        let good = scratch_env(&layout, "/usr/bin:/bin");
        assert!(
            env_refusals(&good, &layout).is_empty(),
            "{:?}",
            env_refusals(&good, &layout)
        );
        let outside_root = std::env::temp_dir().display().to_string();
        let breaks: Vec<(&str, Option<&str>)> = vec![
            ("HOME", Some(outside_root.as_str())),
            ("XDG_STATE_HOME", Some(outside_root.as_str())),
            ("XDG_CACHE_HOME", None),
            ("GIT_CONFIG_GLOBAL", Some(outside_root.as_str())),
            ("GIT_CONFIG_NOSYSTEM", None),
            ("SSH_AUTH_SOCK", Some("/tmp/agent.sock")),
            ("GIT_SSH_COMMAND", Some("ssh")),
            ("HC_SESSION", Some("default")),
            ("HC_SOCKET", Some(outside_root.as_str())),
            ("CODEFLOW_HOOK_BINARY", Some("/usr/bin/true")),
            ("GIT_CONFIG_COUNT", Some("1")),
            ("GIT_CONFIG_KEY_0", Some("credential.helper")),
            ("GIT_ASKPASS", Some("/usr/bin/true")),
        ];
        for (name, value) in breaks {
            let mut broken = good.clone();
            match value {
                Some(value) => {
                    broken.insert(name.to_string(), value.to_string());
                }
                None => {
                    broken.remove(name);
                }
            }
            assert!(
                !env_refusals(&broken, &layout).is_empty(),
                "{name}={value:?}"
            );
        }
    }

    #[test]
    fn the_remote_must_be_a_local_path_inside_the_root() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().canonicalize().unwrap();
        let local = root.join("remote.git").display().to_string();
        assert!(remote_refusal(&root, &local).is_none());
        for refused in [
            "https://example.invalid/repo.git",
            "ssh://example.invalid/repo.git",
            "file:///tmp/repo.git",
            "git@example.invalid:repo.git",
            "example.invalid:repo.git",
            "remote.git",
            "/usr/remote.git",
        ] {
            assert!(remote_refusal(&root, refused).is_some(), "{refused}");
        }
    }

    #[test]
    fn the_floor_is_reported_as_found() {
        let ran = |code, output: &str| Ran {
            code,
            output: output.to_string(),
            timed_out: false,
        };
        let refused = ran(
            Some(1),
            "codeflow: policy rule git.push_to_protected (block)",
        );
        assert_eq!(
            floor(&refused, "a", "a"),
            "codeflow git hook (git.push_to_protected)"
        );
        assert!(floor(&ran(Some(0), ""), "a", "b").starts_with("none: the remote ref moved"));
        assert!(floor(&ran(Some(1), "rejected"), "a", "a").starts_with("other"));
    }
}
