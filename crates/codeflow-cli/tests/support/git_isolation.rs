//! TSK-1084: the host's Git state a test process must not inherit.
//!
//! A developer's shell exports `PAGER`, `GIT_PAGER`, author and template
//! settings and more. The guard and Git read them, so a fixture that keeps
//! them can pass on one host and fail on another. Test files that start Git
//! or `codeflow` include this module and call [`strip_host_git_state`].
#![allow(dead_code)]
use std::ffi::{OsStr, OsString};
use std::process::Command;

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
pub fn names_host_state(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_uppercase();
    (name.starts_with("GIT_") && !forced_by_cargo(&name)) || HOST_VARIABLES.contains(&name.as_str())
}

/// Remove every inherited variable [`names_host_state`] names (every `GIT_*`
/// variable except the maintenance settings the repository forces, such as
/// `GIT_PAGER`, `GIT_TEMPLATE_DIR` and the author settings, and the
/// variables in [`HOST_VARIABLES`]) and point the system and global
/// configuration at `/dev/null`. A test that needs a value sets it after this
/// call.
pub fn strip_host_git_state(cmd: &mut Command) -> &mut Command {
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
}
