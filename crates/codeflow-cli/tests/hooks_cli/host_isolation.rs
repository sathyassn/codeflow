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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use super::line_adoption_fixture::git_isolation::{names_host_state, strip_host_git_state};

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
/// run: [`strip_host_git_state`] (every inherited host variable removed, the
/// system and global files at `/dev/null`), an empty home and no agent
/// session marker, so a hook a commit runs does not apply the root-checkout
/// rule because the test itself runs inside an agent session. A test that
/// needs a value sets it after this call.
pub(super) fn isolate_git(cmd: &mut Command) -> &mut Command {
    for name in codeflow_core::root_checkout::AGENT_MARKERS {
        cmd.env_remove(name);
    }
    strip_host_git_state(cmd)
        .env("HOME", empty_home())
        .env("USERPROFILE", empty_home())
}

/// An empty directory for `GIT_TEMPLATE_DIR`, so `git init` and `git clone`
/// copy no hooks or files from a host template.
fn empty_templates() -> &'static Path {
    static TEMPLATES: OnceLock<PathBuf> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        let templates = empty_home().join("no-templates");
        std::fs::create_dir_all(&templates).unwrap();
        templates
    })
}

/// The same isolation for a fixture clone. Its builder can set a variable
/// but not remove one (`crates/codeflow-fixture/src/lib.rs` removes only
/// the three Git location variables), so this sets what a clone reads: the
/// two configuration files, the home and the template directory, the host
/// variable `git clone` copies into the new repository.
pub(super) fn isolate_clone(clone: codeflow_fixture::Clone) -> codeflow_fixture::Clone {
    clone
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TEMPLATE_DIR", empty_templates())
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
        .env("GIT_TEMPLATE_DIR", "/host/templates")
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
    let planted = [
        "GIT_AUTHOR_NAME",
        "GIT_TEMPLATE_DIR",
        "PAGER",
        "EDITOR",
        "XDG_CONFIG_HOME",
    ];
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

/// A host template directory reaches a plain fixture clone as a hook, and
/// does not reach one made through `isolate_clone`.
#[test]
fn a_host_template_does_not_reach_a_fixture_clone() {
    let temp = tempfile::tempdir().unwrap();
    let seed = temp.path().join("seed");
    std::fs::create_dir(&seed).unwrap();
    init_repo(&seed, "feat/canary");
    let template = temp.path().join("template");
    std::fs::create_dir_all(template.join("hooks")).unwrap();
    std::fs::write(template.join("hooks/pre-commit"), "#!/bin/sh\nexit 1\n").unwrap();

    let plain = temp.path().join("plain");
    codeflow_fixture::clone(temp.path(), &seed, &plain)
        .env("GIT_TEMPLATE_DIR", &template)
        .run();
    assert!(
        plain.join(".git/hooks/pre-commit").exists(),
        "the control clone should copy the template hook"
    );

    let isolated = temp.path().join("isolated");
    isolate_clone(
        codeflow_fixture::clone(temp.path(), &seed, &isolated).env("GIT_TEMPLATE_DIR", &template),
    )
    .run();
    assert!(!isolated.join(".git/hooks/pre-commit").exists());
}
