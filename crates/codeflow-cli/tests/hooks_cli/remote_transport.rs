//! TSK-1489: both guards resolve the transport Git selects for each remote
//! a command contacts, refuse a helper program the landed policy does not
//! admit, refuse every form whose transport they cannot prove, and admit
//! the helpers `git.admitted_helpers` names on the policy landed at the
//! remote's default branch.
//!
//! Every row is a hook payload judged by the built binary in a scratch
//! repository; nothing contacts a remote or starts a helper.
use super::{
    codeflow, enforced_repo, git, guard_payload, run_with_stdin, shell_path, write_agent_policy,
    write_policy,
};
use std::path::{Path, PathBuf};
use std::process::Output;

const HOOKS: [&str; 2] = ["exec-guard", "git-guard"];
const HELPER: &str = "a remote's transport starts a helper program";
const UNPROVEN: &str = "the guard cannot prove which transport Git selects";
const ADMITTING: &str = r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off","admitted_helpers":["git-remote-fixture","gh auth git-credential"]}}"#;

struct Scratch {
    _temp: tempfile::TempDir,
    base: PathBuf,
    root: PathBuf,
}

impl Scratch {
    /// A repository on `main` whose `origin`, a local bare clone, holds
    /// `landed` as the policy on its default branch, fetched with
    /// `origin/HEAD` set, so the guards read it as the landed policy.
    fn landed(landed: Option<&str>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = codeflow_core::portable_path::canonicalize(temp.path()).unwrap();
        let root = base.join("repo");
        std::fs::create_dir(&root).unwrap();
        enforced_repo(&root);
        if let Some(policy) = landed {
            write_agent_policy(&root, policy);
        }
        let origin = base.join("origin.git");
        super::isolate_clone(codeflow_fixture::clone(&base, &root, &origin))
            .bare()
            .run();
        git(&root, &["remote", "add", "origin", &shell_path(&origin)]);
        git(&root, &["fetch", "-q", "origin"]);
        git(&root, &["remote", "set-head", "origin", "main"]);
        Self {
            _temp: temp,
            base,
            root,
        }
    }

    fn config(&self, key: &str, value: &str) -> &Self {
        git(&self.root, &["config", "--add", key, value]);
        self
    }

    /// Write `bytes` to `path` under the repository root.
    fn write(&self, path: &str, bytes: &[u8]) -> &Self {
        let file = self.root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
        self
    }

    /// Append `bytes` to the repository's own configuration file.
    fn append_config(&self, bytes: &[u8]) -> &Self {
        let file = self.root.join(".git/config");
        let mut config = std::fs::read(&file).unwrap();
        config.extend_from_slice(bytes);
        std::fs::write(file, config).unwrap();
        self
    }

    fn judge(&self, hook: &str, command: &str) -> Output {
        run_with_stdin(
            codeflow()
                .args(["hook", hook, "--contract", "3"])
                .current_dir(&self.root),
            &guard_payload(command, &self.root),
        )
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A refusal by this task's rule: hook integrity, with the helper reason.
fn helper_refusal(out: &Output) -> bool {
    let err = stderr(out);
    out.status.code() == Some(2) && err.contains("git.hook_integrity") && err.contains(HELPER)
}

/// A refusal by this task's rule: hook integrity, for a form whose
/// transport the guard cannot prove.
fn unproven_refusal(out: &Output) -> bool {
    let err = stderr(out);
    out.status.code() == Some(2) && err.contains("git.hook_integrity") && err.contains(UNPROVEN)
}

/// git-guard's earlier rule that refuses a fetch whose effective address
/// differs from the configured one, before it reads what Git runs.
const DIFFERENT_URL: &str = "differs from remote.";

/// git-guard's earlier rule for a configuration environment override.
const CONFIG_OVERRIDE: &str = "after changing environment variable GIT_CONFIG_COUNT";

/// Every command in `refused` is refused by `refusal` in both guards, except
/// that git-guard may refuse a command `earlier` pairs with a reason by that
/// earlier rule; every command in `allowed` passes both.
fn assert_judged_with(
    scratch: &Scratch,
    refusal: fn(&Output) -> bool,
    refused: &[&str],
    allowed: &[&str],
    earlier: &[(&str, &str)],
) {
    let mut wrong = Vec::new();
    for hook in HOOKS {
        for command in refused {
            let out = scratch.judge(hook, command);
            let first = earlier
                .iter()
                .find(|(line, _)| line == command)
                .filter(|_| hook == "git-guard");
            if let Some((_, reason)) = first {
                let err = stderr(&out);
                if out.status.code() != Some(2) || !err.contains(reason) {
                    wrong.push(format!("{hook} should refuse `{command}` first by `{reason}`: {err}"));
                }
                continue;
            }
            if !refusal(&out) {
                wrong.push(format!(
                    "{hook} should refuse `{command}`: {}",
                    stderr(&out)
                ));
            }
        }
        for command in allowed {
            let out = scratch.judge(hook, command);
            if out.status.code() != Some(0) {
                wrong.push(format!("{hook} should allow `{command}`: {}", stderr(&out)));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

fn assert_rows(scratch: &Scratch, refused: &[&str], allowed: &[&str]) {
    assert_judged_with(scratch, helper_refusal, refused, allowed, &[]);
}

fn assert_unproven(scratch: &Scratch, refused: &[&str], allowed: &[&str]) {
    assert_judged_with(scratch, unproven_refusal, refused, allowed, &[]);
}

/// git-guard's earlier rule, which refuses a fetch whose source has no
/// URL before it reads what Git runs.
const NO_FETCH_URL: &str = "fetch source is not a configured remote or its URL";

/// A fetch the resolution refuses that git-guard refuses first by its
/// earlier rule for a fetch source with no URL.
fn assert_fetch_without_url(scratch: &Scratch, command: &str) {
    let out = scratch.judge("exec-guard", command);
    assert!(unproven_refusal(&out), "exec-guard: {}", stderr(&out));
    let out = scratch.judge("git-guard", command);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(2), "git-guard: {err}");
    assert!(
        err.contains("git.policy_authority") && err.contains(NO_FETCH_URL),
        "git-guard: {err}"
    );
}

#[test]
fn both_guards_refuse_a_transport_that_starts_a_helper_program() {
    let scratch = Scratch::landed(None);
    scratch
        .config("remote.address.url", "fixture::https://127.0.0.1:9/r")
        .config("remote.scheme.url", "fixture://127.0.0.1:9/r")
        .config("remote.pushed.url", "https://127.0.0.1:9/r")
        .config("remote.pushed.pushurl", "fixture::x");
    assert_rows(
        &scratch,
        &[
            "git fetch address",
            "git ls-remote scheme",
            "git push pushed HEAD:refs/heads/task/x",
            "git ls-remote fixture::https://127.0.0.1:9/r",
            "git ls-remote fixture://127.0.0.1:9/r",
        ],
        &["git fetch origin", "git ls-remote origin", "git fetch pushed"],
    );
}

#[test]
fn both_guards_refuse_an_implicit_contact_with_a_helper_program() {
    let tracked = Scratch::landed(None);
    tracked
        .config("remote.other.url", "fixture::x")
        .config("branch.main.remote", "other");
    assert_rows(&tracked, &["git fetch", "git pull"], &["git fetch origin"]);
    let promisor = Scratch::landed(None);
    promisor
        .config("remote.lazy.url", "fixture::x")
        .config("remote.lazy.promisor", "true");
    assert_rows(&promisor, &["git log -1"], &[]);
}

#[test]
fn ordinary_transports_keep_their_verdict() {
    let scratch = Scratch::landed(None);
    scratch
        .config("remote.web.url", "https://127.0.0.1:9/r.git")
        .config("remote.shell.url", "ssh://127.0.0.1:9/r.git")
        .config("remote.short.url", "git@127.0.0.1:r.git")
        .config("remote.file.url", "file:///srv/r.git");
    assert_rows(
        &scratch,
        &[],
        &[
            "git fetch web",
            "git ls-remote shell",
            "git ls-remote short",
            "git ls-remote file",
            "git ls-remote https://127.0.0.1:9/r.git",
            "git fetch origin",
            "git fetch",
            "git pull --ff-only",
            "git fetch --no-recurse-submodules origin",
            "git push origin HEAD:refs/heads/task/x",
        ],
    );
    // The `ext` transport keeps its own refusal and message.
    for hook in HOOKS {
        let out = scratch.judge(hook, "git ls-remote 'ext::sh -c true'");
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{hook}: {err}");
        assert!(err.contains("ext:: transport command"), "{hook}: {err}");
    }
}

#[test]
fn rewrites_and_push_destinations_are_resolved_per_direction() {
    let rewrite = Scratch::landed(None);
    rewrite
        .config("remote.web.url", "https://127.0.0.1:9/r.git")
        .config(
            "url.fixture::https://127.0.0.1:9/.pushInsteadOf",
            "https://127.0.0.1:9/",
        );
    assert_rows(
        &rewrite,
        &["git push web HEAD:refs/heads/task/x"],
        &["git fetch web"],
    );
    let fetch_rewrite = Scratch::landed(None);
    fetch_rewrite
        .config("remote.web.url", "https://127.0.0.1:9/r.git")
        .config(
            "url.fixture::https://127.0.0.1:9/.insteadOf",
            "https://127.0.0.1:9/",
        );
    assert_judged_with(
        &fetch_rewrite,
        helper_refusal,
        &["git fetch web", "git push web HEAD:refs/heads/task/x"],
        &["git fetch origin"],
        &[("git fetch web", DIFFERENT_URL)],
    );
}

#[test]
fn both_guards_refuse_a_remote_with_a_helper_setting() {
    let scratch = Scratch::landed(None);
    scratch
        .config("remote.conflict.url", "fixture::https://127.0.0.1:9/r")
        .config("remote.conflict.vcs", "https")
        .config("remote.empty.url", "https://127.0.0.1:9/r")
        .config("remote.empty.vcs", "")
        .config("remote.selected.url", "https://127.0.0.1:9/r")
        .config("remote.selected.vcs", "fixture")
        .config("remote.web.url", "https://127.0.0.1:9/r");
    assert_unproven(
        &scratch,
        &[
            "git fetch conflict",
            "git push conflict HEAD:refs/heads/task/x",
            "git fetch empty",
            "git ls-remote empty",
            "git fetch selected",
            "git push selected HEAD:refs/heads/task/x",
        ],
        &["git fetch web", "git ls-remote web"],
    );
    let without_url = Scratch::landed(None);
    without_url.config("remote.bare.vcs", "fixture");
    for hook in HOOKS {
        let out = without_url.judge(hook, "git ls-remote bare");
        assert!(unproven_refusal(&out), "{hook}: {}", stderr(&out));
    }
    assert_fetch_without_url(&without_url, "git fetch bare");
}

#[test]
fn both_guards_refuse_an_operand_free_contact_they_cannot_prove() {
    // A sole remote other than `origin`, which Git falls back to.
    let sole = Scratch::landed(None);
    git(sole.root(), &["remote", "rename", "origin", "backup"]);
    git(sole.root(), &["config", "remote.backup.url", "fixture::x"]);
    assert_unproven(&sole, &["git fetch", "git pull", "git ls-remote"], &[]);

    // Fetching every remote, by configuration or by option.
    let all = Scratch::landed(None);
    all.config("remote.backup.url", "fixture::x")
        .config("fetch.all", "true");
    assert_unproven(
        &all,
        &[
            "git fetch",
            "git pull",
            "git fetch --all",
            "git fetch --al",
            "git remote update",
            "git maintenance run",
            "git maintenance run --task=prefetch",
        ],
        &["git fetch origin", "git maintenance run --task=gc"],
    );

    let ordinary = Scratch::landed(None);
    assert_unproven(
        &ordinary,
        &["git push", "git ls-remote", "git push --force-with-lease=main"],
        &["git fetch", "git push origin HEAD:refs/heads/task/x"],
    );
}

#[test]
fn both_guards_refuse_each_operand_free_fetch_state_they_cannot_prove() {
    let detached = Scratch::landed(None);
    git(detached.root(), &["checkout", "-q", "--detach"]);
    assert_unproven(&detached, &["git fetch", "git pull"], &["git fetch origin"]);

    let missing = Scratch::landed(None);
    missing.config("branch.main.remote", "gone");
    assert_unproven(&missing, &["git fetch", "git pull"], &["git fetch origin"]);

    let no_origin = Scratch::landed(None);
    git(no_origin.root(), &["remote", "rename", "origin", "first"]);
    assert_unproven(&no_origin, &["git fetch", "git pull"], &["git fetch first"]);

    let all = Scratch::landed(None);
    all.config("fetch.all", "true");
    assert_unproven(&all, &["git fetch", "git pull"], &["git fetch origin"]);

    let group = Scratch::landed(None);
    group.config("remotes.origin", "origin");
    assert_unproven(&group, &["git fetch", "git fetch origin"], &[]);

    let twice = Scratch::landed(None);
    twice.config("remote.origin.url", "https://127.0.0.1:9/r");
    assert_judged_with(
        &twice,
        unproven_refusal,
        &["git fetch", "git fetch origin", "git ls-remote origin"],
        &[],
        &[
            ("git fetch", DIFFERENT_URL),
            ("git fetch origin", DIFFERENT_URL),
        ],
    );

    let unclear = Scratch::landed(None);
    unclear.config("fetch.all", "maybe");
    assert_unproven(&unclear, &["git fetch", "git pull"], &["git fetch origin"]);
    let unreadable = Scratch::landed(None);
    unreadable.append_config(b"[branch \"main\"]\n\tremote = or\xffigin\n");
    for hook in HOOKS {
        let out = unreadable.judge(hook, "git fetch");
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{hook}: {err}");
        assert!(err.contains("git.hook_integrity"), "{hook}: {err}");
    }
}

#[test]
fn both_guards_refuse_a_remote_name_that_is_also_a_group() {
    let scratch = Scratch::landed(None);
    scratch
        .config("remote.mirrors.url", "https://127.0.0.1:9/m")
        .config("remote.backup.url", "fixture::x")
        .config("remotes.mirrors", "origin backup");
    assert_unproven(
        &scratch,
        &["git fetch mirrors", "git remote update mirrors"],
        &["git fetch origin"],
    );
}

#[test]
fn an_unresolved_group_refuses_in_both_guards() {
    let scratch = Scratch::landed(None);
    scratch.config("remotes.mirrors", "origin missing");
    for (hook, command) in HOOKS
        .iter()
        .flat_map(|hook| ["git fetch mirrors", "git remote update mirrors"].map(|c| (*hook, c)))
    {
        let out = scratch.judge(hook, command);
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{hook} `{command}`: {err}");
        // git-guard refuses a fetch of a group by its earlier rule for a
        // fetch source with no URL, before it reads what Git runs.
        if hook == "git-guard" {
            assert!(err.contains(NO_FETCH_URL), "{hook} `{command}`: {err}");
            continue;
        }
        assert!(unproven_refusal(&out), "{hook} `{command}`: {err}");
        assert!(err.contains("remote group"), "{hook} `{command}`: {err}");
    }
}

#[test]
fn both_guards_refuse_submodule_recursion_they_cannot_prove() {
    let listed = Scratch::landed(None);
    listed.write(".gitmodules", b"");
    let initialized = Scratch::landed(None);
    initialized.write(
        ".git/modules/lib/config",
        b"[remote \"origin\"]\n\turl = fixture::x\n",
    );
    let configured = Scratch::landed(None);
    configured.config("submodule.lib.url", "https://127.0.0.1:9/lib");
    for scratch in [&listed, &initialized, &configured] {
        assert_unproven(
            scratch,
            &[
                "git fetch origin",
                "git pull origin main",
                "git fetch",
                "git remote update origin",
                "git submodule update",
                "git submodule update --init",
            ],
            &[
                "git fetch --no-recurse-submodules origin",
                "git fetch origin --recurse-submodules=no",
                "git push origin HEAD:refs/heads/task/x",
            ],
        );
    }
    let pushing = Scratch::landed(None);
    pushing
        .write(".gitmodules", b"")
        .config("push.recurseSubmodules", "on-demand");
    assert_unproven(
        &pushing,
        &["git push origin HEAD:refs/heads/task/x"],
        &["git push --no-recurse-submodules origin HEAD:refs/heads/task/x"],
    );
}

#[test]
fn a_command_line_setting_does_not_move_the_repository_probe() {
    // The probe that looks for submodules reads only the call's location
    // inputs, so a work-tree setting on the command line or in the
    // environment leaves the verdict unchanged.
    let scratch = Scratch::landed(None);
    scratch.write(".gitmodules", b"");
    let empty = scratch.base.join("empty");
    std::fs::create_dir(&empty).unwrap();
    let empty = shell_path(&empty);
    let setting = format!("git -c core.worktree={empty} fetch origin");
    let environment = format!(
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.worktree GIT_CONFIG_VALUE_0={empty} git fetch origin"
    );
    assert_judged_with(
        &scratch,
        unproven_refusal,
        &["git fetch origin", &setting, &environment],
        &[],
        &[(&environment, CONFIG_OVERRIDE)],
    );
}

#[test]
fn both_guards_refuse_a_remote_with_more_than_one_address_value() {
    let scratch = Scratch::landed(None);
    scratch
        .config("remote.web.url", "https://127.0.0.1:9/r")
        .config("remote.web.url", "fixture::x")
        .config("remote.reset.url", "https://127.0.0.1:9/r")
        .config("remote.reset.pushurl", "fixture::x")
        .config("remote.reset.pushurl", "")
        .config("remote.reset.pushurl", "https://127.0.0.1:9/r")
        .config("remote.cleared.url", "fixture::x")
        .config("remote.cleared.url", "")
        .config("remote.cleared.url", "https://127.0.0.1:9/r");
    assert_judged_with(
        &scratch,
        unproven_refusal,
        &[
            "git fetch web",
            "git push web HEAD:refs/heads/task/x",
            "git push reset HEAD:refs/heads/task/x",
            "git fetch reset",
            "git fetch cleared",
            "git push cleared HEAD:refs/heads/task/x",
        ],
        &["git fetch origin"],
        &[("git fetch web", DIFFERENT_URL)],
    );
}

#[test]
fn a_recovery_fetch_is_withheld_for_a_helper_transport() {
    // Two remotes and no `origin` leave the policy authority unresolved, so
    // only the recovery fetch of a configured remote may pass, and only over
    // a transport Git runs itself.
    let scratch = Scratch::landed(None);
    git(scratch.root(), &["remote", "rename", "origin", "first"]);
    scratch.config("remote.second.url", "fixture::x");
    for hook in HOOKS {
        for command in ["git fetch second", "git fetch"] {
            let out = scratch.judge(hook, command);
            let err = stderr(&out);
            assert_eq!(out.status.code(), Some(2), "{hook} `{command}`: {err}");
            assert!(err.contains("policy source is ambiguous"), "{hook} `{command}`: {err}");
        }
        let out = scratch.judge(hook, "git fetch first");
        assert_eq!(out.status.code(), Some(0), "{hook}: {}", stderr(&out));
    }
}

#[test]
fn both_guards_refuse_a_name_a_legacy_remote_file_may_define() {
    for folder in ["remotes", "branches"] {
        let scratch = Scratch::landed(None);
        scratch.write(&format!(".git/{folder}/old"), b"URL: fixture::x\n");
        std::fs::write(scratch.root().join(".git/config"), b"").unwrap();
        for hook in HOOKS {
            let out = scratch.judge(hook, "git ls-remote old");
            assert!(unproven_refusal(&out), "{folder} {hook}: {}", stderr(&out));
        }
    }
}

#[cfg(unix)]
#[test]
fn an_unreadable_configuration_refuses_naming_the_file() {
    let scratch = Scratch::landed(None);
    let extra = scratch.root().join(".git/extra-config");
    std::fs::write(&extra, "[remote \"web\"]\n\turl = https://127.0.0.1:9/r\n").unwrap();
    scratch.config("include.path", &shell_path(&extra));
    let _denied = super::RestorePermissions::deny(&extra);
    for hook in HOOKS {
        let out = scratch.judge(hook, "git fetch web");
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{hook}: {err}");
        assert!(err.contains("extra-config"), "{hook}: {err}");
    }
}

#[test]
fn the_landed_policy_admits_a_named_remote_helper() {
    let scratch = Scratch::landed(Some(ADMITTING));
    scratch
        .config("remote.selected.url", "fixture::https://127.0.0.1:9/r")
        .config("remote.other.url", "otherhelper::https://127.0.0.1:9/o");
    assert_rows(
        &scratch,
        &["git fetch other"],
        &[
            "git fetch selected",
            "git ls-remote fixture::https://127.0.0.1:9/r",
        ],
    );
}

#[test]
fn only_the_landed_policy_admits_a_helper() {
    // The list committed on `main` after `origin` was fetched is not the
    // landed policy; nor is a working-copy edit.
    let committed = Scratch::landed(None);
    write_agent_policy(committed.root(), ADMITTING);
    committed.config("remote.selected.url", "fixture::https://127.0.0.1:9/r");
    assert_rows(&committed, &["git fetch selected"], &[]);
    let edited = Scratch::landed(None);
    write_policy(edited.root(), ADMITTING);
    edited.config("remote.selected.url", "fixture::https://127.0.0.1:9/r");
    assert_rows(&edited, &["git fetch selected"], &[]);
}

/// The admitting policy with the secret-read rule at warn, so the
/// cross-repository location check, a separate control, does not refuse
/// first.
const ADMITTING_WARN: &str = r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off","admitted_helpers":["git-remote-fixture"]},"security":{"secret_reads":"warn"}}"#;

#[test]
fn an_admission_does_not_reach_another_repository() {
    // Repository A admits the helper; repository B, which the call selects
    // by a location input, names it and admits nothing.
    let a = Scratch::landed(Some(ADMITTING_WARN));
    a.config("remote.selected.url", "fixture::https://127.0.0.1:9/r");
    let b = Scratch::landed(None);
    b.config("remote.selected.url", "fixture::https://127.0.0.1:9/r");
    let b_git = shell_path(&b.root().join(".git"));
    let b_root = shell_path(b.root());
    assert_rows(
        &a,
        &[
            &format!("git --git-dir={b_git} fetch selected"),
            &format!(
                "git --git-dir {b_git} --work-tree {b_root} fetch --no-recurse-submodules selected"
            ),
            &format!("GIT_DIR={b_git} git fetch selected"),
        ],
        &["git fetch selected"],
    );
}

/// Whether exec-guard's output carries the account finding.
fn account_finding(out: &Output) -> bool {
    stderr(out).contains("account action")
}

#[test]
fn the_landed_policy_admits_a_named_credential_helper_on_a_remote_contact() {
    let fetch = "git fetch origin";
    let push = "git push origin HEAD:refs/heads/task/x";
    let read = "git credential fill";
    let plain = Scratch::landed(None);
    plain.config("credential.helper", "!gh auth git-credential");
    for command in [fetch, push] {
        let out = plain.judge("exec-guard", command);
        assert!(
            out.status.code() == Some(2) && account_finding(&out),
            "{command}: {}",
            stderr(&out)
        );
    }
    let admitted = Scratch::landed(Some(ADMITTING));
    admitted.config("credential.helper", "!gh auth git-credential");
    for command in [fetch, push] {
        let out = admitted.judge("exec-guard", command);
        assert!(!account_finding(&out), "{command}: {}", stderr(&out));
        assert_eq!(out.status.code(), Some(0), "{command}: {}", stderr(&out));
    }
    // Reading the credential directly stays refused.
    let out = admitted.judge("exec-guard", read);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    // A helper the list does not name keeps its finding.
    let other = Scratch::landed(Some(ADMITTING));
    other.config(
        "credential.helper",
        "!gh auth git-credential --hostname example.test",
    );
    let out = other.judge("exec-guard", fetch);
    assert!(
        out.status.code() == Some(2) && account_finding(&out),
        "{}",
        stderr(&out)
    );
}
