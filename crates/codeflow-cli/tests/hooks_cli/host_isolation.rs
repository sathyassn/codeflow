//! TSK-1084: a guard verdict never depends on the host's Git state.
//!
//! The guard judges a Git call by the programs it would run: the
//! `core.pager` setting, the `PAGER` and `GIT_PAGER` variables, and the other
//! settings and variables in its catalog. A developer's shell exports some
//! of those, so a test could pass on one host and fail on another. The
//! canary below plants a pager in every place a run could otherwise reach
//! it and shows that the verdicts of a fixed set of `git log` lines do not
//! move.
use super::{guard_payload, init_repo, isolated_codeflow, isolated_home, run_with_stdin};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Variables that do not start with `GIT_` but still decide what a Git call
/// or the guard reads: the pager, editor and askpass programs the guard's
/// catalog judges, and the directory Git reads a per-user file from.
const HOST_VARIABLES: [&str; 5] = [
    "PAGER",
    "EDITOR",
    "VISUAL",
    "SSH_ASKPASS",
    "XDG_CONFIG_HOME",
];

/// The settings `.cargo/config.toml` forces into every test process, which
/// switch automatic Git maintenance off (TSK-245). They belong to this
/// repository and are the same on every host, so the isolation keeps them.
fn forced_by_cargo(name: &str) -> bool {
    name == "GIT_CONFIG_COUNT"
        || name == "GIT_CONFIG_PARAMETERS"
        || name.starts_with("GIT_CONFIG_KEY_")
        || name.starts_with("GIT_CONFIG_VALUE_")
}

/// Whether a variable carries host state: a Git variable that the repository
/// does not force, or one of [`HOST_VARIABLES`].
fn names_host_state(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_uppercase();
    (name.starts_with("GIT_") && !forced_by_cargo(&name)) || HOST_VARIABLES.contains(&name.as_str())
}

/// An empty home shared by the tests of this process, so a run never reads
/// the developer's `~/.gitconfig` or `~/.config/git/config`.
fn empty_home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let home = isolated_home().join("user");
        std::fs::create_dir_all(&home).unwrap();
        home
    })
}

/// Cut a command off from the host's Git state, once for every `hooks_cli`
/// run. It removes every inherited `GIT_*` variable except the maintenance
/// settings the repository forces (`GIT_PAGER`, the author and editor
/// settings and the rest) and the non-`GIT_` variables in [`HOST_VARIABLES`],
/// points the system and global files at `/dev/null`, and gives the run an
/// empty home. A test that needs a value sets it after this call.
pub(super) fn isolate_git(cmd: &mut Command) -> &mut Command {
    let host: Vec<OsString> = std::env::vars_os()
        .map(|(name, _)| name)
        .chain(cmd.get_envs().map(|(name, _)| name.to_os_string()))
        .filter(|name| names_host_state(name))
        .collect();
    for name in host {
        cmd.env_remove(name);
    }
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("HOME", empty_home())
        .env("USERPROFILE", empty_home())
}

/// The same isolation for a fixture clone. Its builder can set a variable
/// but not remove one, and a clone reads only the files and the home, so it
/// sets those.
pub(super) fn isolate_clone(clone: codeflow_fixture::Clone) -> codeflow_fixture::Clone {
    clone
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("HOME", empty_home())
        .env("USERPROFILE", empty_home())
}

/// A `git` command that reads nothing from the host.
pub(super) fn git_command() -> Command {
    let mut cmd = Command::new("git");
    isolate_git(&mut cmd);
    cmd
}

/// The pager every planted source names. No such program exists; the guard
/// only reads the name.
const HOST_PAGER: &str = "host-pager";

/// `git log` lines whose verdict depends on whether a pager is configured:
/// a leading `^` handed to a Git call that runs a configured program must be
/// quoted, and without a program there is nothing to judge.
const LOG_LINES: [&str; 4] = [
    "git log ^main HEAD",
    "git log '^main' HEAD",
    "git log HEAD",
    "git log -1 --oneline",
];

/// Plant a pager in every source a host could supply: the system and global
/// files, the `PAGER` and `GIT_PAGER` variables, and a home with its own Git
/// configuration. The files are built under `host`.
fn plant_host_pager(cmd: &mut Command, host: &Path) {
    let config = format!("[core]\n\tpager = {HOST_PAGER}\n");
    let system = host.join("system-gitconfig");
    let global = host.join("global-gitconfig");
    let home = host.join("home");
    let xdg = home.join("xdg");
    std::fs::create_dir_all(xdg.join("git")).unwrap();
    for file in [
        &system,
        &global,
        &home.join(".gitconfig"),
        &xdg.join("git/config"),
    ] {
        std::fs::write(file, &config).unwrap();
    }
    cmd.env("GIT_CONFIG_SYSTEM", &system)
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GIT_PAGER", HOST_PAGER)
        .env("PAGER", HOST_PAGER)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &xdg);
}

/// The exit code the guard gives each of [`LOG_LINES`] in `repo`, run the
/// way every `hooks_cli` test runs it, with the host's state planted first
/// when `host` is given.
fn verdicts(repo: &Path, host: Option<&Path>) -> Vec<Option<i32>> {
    LOG_LINES
        .iter()
        .map(|line| {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
            if let Some(host) = host {
                plant_host_pager(&mut cmd, host);
            }
            let mut cmd = isolated_codeflow(cmd);
            cmd.args(["hook", "git-guard", "--contract", "3"])
                .current_dir(repo);
            run_with_stdin(&mut cmd, &guard_payload(line, repo))
                .status
                .code()
        })
        .collect()
}

#[test]
fn host_git_state_does_not_change_guard_verdicts() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo, "feat/canary");
    let host = temp.path().join("host");
    std::fs::create_dir(&host).unwrap();

    // No pager anywhere in the fixture: nothing runs, every line passes.
    let clean = verdicts(&repo, None);
    assert_eq!(
        clean,
        vec![Some(0); LOG_LINES.len()],
        "unconfigured fixture"
    );
    assert_eq!(
        verdicts(&repo, Some(&host)),
        clean,
        "a pager planted in the host's files, variables and home changed a verdict"
    );

    // The set is sensitive: a pager the fixture itself configures does move
    // the verdict of the unquoted caret line.
    super::git(&repo, &["config", "core.pager", HOST_PAGER]);
    let pinned = verdicts(&repo, Some(&host));
    assert_eq!(pinned, verdicts(&repo, None), "pinned fixture");
    assert_eq!(pinned[0], Some(2), "unquoted caret line with a pager");
    assert_eq!(&pinned[1..], &[Some(0); 3], "other lines with a pager");
}

/// Every host variable the child could inherit, and every one planted on
/// the command beforehand, ends up removed or pointed at `/dev/null`, while
/// the maintenance settings the repository forces stay.
#[test]
fn isolation_leaves_no_host_variable_for_the_child() {
    let mut cmd = Command::new("git");
    cmd.env("GIT_CONFIG_COUNT", "1")
        .env("GIT_AUTHOR_NAME", "host")
        .env("PAGER", "x")
        .env("EDITOR", "x")
        .env("XDG_CONFIG_HOME", "/host/xdg");
    isolate_git(&mut cmd);
    let envs: std::collections::BTreeMap<_, _> = cmd
        .get_envs()
        .map(|(name, value)| (name.to_string_lossy().to_string(), value.map(Path::new)))
        .collect();
    let inherited = std::env::vars_os()
        .map(|(name, _)| name)
        .filter(|name| names_host_state(name));
    let planted = ["GIT_AUTHOR_NAME", "PAGER", "EDITOR", "XDG_CONFIG_HOME"];
    for name in inherited.chain(planted.map(Into::into)) {
        let name = name.to_string_lossy().to_string();
        match envs.get(&name) {
            Some(None) => {}
            Some(Some(value)) if name == "GIT_CONFIG_GLOBAL" || name == "GIT_CONFIG_SYSTEM" => {
                assert_eq!(value, &Path::new("/dev/null"), "{name}");
            }
            other => panic!("{name} reaches the child: {other:?}"),
        }
    }
    for name in ["HOME", "USERPROFILE"] {
        assert_eq!(envs[name], Some(empty_home()), "{name}");
    }
    assert_eq!(
        envs["GIT_CONFIG_COUNT"],
        Some(Path::new("1")),
        "the forced maintenance settings are kept"
    );
}
