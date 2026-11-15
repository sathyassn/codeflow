//! TSK-1489: both guards resolve the transport Git selects for each remote
//! a command contacts, refuse a helper program the landed policy does not
//! admit, and admit the helpers `git.admitted_helpers` names on the policy
//! landed at the remote's default branch.
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
const ADMITTING: &str = r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off","admitted_helpers":["git-remote-fixture","gh auth git-credential"]}}"#;

struct Scratch {
    _temp: tempfile::TempDir,
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
        Self { _temp: temp, root }
    }

    fn config(&self, key: &str, value: &str) -> &Self {
        git(&self.root, &["config", "--add", key, value]);
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

fn assert_rows(scratch: &Scratch, refused: &[&str], allowed: &[&str]) {
    let mut wrong = Vec::new();
    for hook in HOOKS {
        for command in refused {
            let out = scratch.judge(hook, command);
            if !helper_refusal(&out) {
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

/// git-guard's earlier rule, which refuses a fetch whose source has no
/// URL before it reads what Git runs.
const NO_FETCH_URL: &str = "fetch source is not a configured remote or its URL";

/// A fetch from a remote with no URL: exec-guard refuses it by this task's
/// rule, and git-guard by its earlier rule for a fetch source with no URL.
fn assert_fetch_without_url(scratch: &Scratch, command: &str) {
    let out = scratch.judge("exec-guard", command);
    assert!(helper_refusal(&out), "exec-guard: {}", stderr(&out));
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
        .config("remote.selected.url", "https://127.0.0.1:9/r")
        .config("remote.selected.vcs", "fixture")
        .config("remote.bare.vcs", "fixture");
    assert_rows(
        &scratch,
        &[
            "git fetch address",
            "git ls-remote scheme",
            "git fetch selected",
            "git push selected HEAD:refs/heads/task/x",
            "git ls-remote bare",
            "git ls-remote fixture::https://127.0.0.1:9/r",
            "git ls-remote fixture://127.0.0.1:9/r",
            "git fetch --all",
        ],
        &["git fetch origin", "git ls-remote origin"],
    );
    assert_fetch_without_url(&scratch, "git fetch bare");
}

#[test]
fn both_guards_refuse_an_implicit_contact_with_a_helper_program() {
    let tracked = Scratch::landed(None);
    tracked
        .config("remote.other.vcs", "fixture")
        .config("branch.main.remote", "other");
    assert_rows(&tracked, &["git fetch", "git pull"], &[]);
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
            "git fetch --all",
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
    let destinations = Scratch::landed(None);
    destinations
        .config("remote.web.url", "https://127.0.0.1:9/r.git")
        .config("remote.web.pushurl", "https://127.0.0.1:9/r.git")
        .config("remote.web.pushurl", "fixture::x");
    assert_rows(
        &destinations,
        &["git push web HEAD:refs/heads/task/x"],
        &["git fetch web"],
    );
}

#[test]
fn an_unresolved_selection_refuses_naming_the_remote() {
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
        assert!(
            err.contains("git.hook_integrity"),
            "{hook} `{command}`: {err}"
        );
        assert!(
            err.contains("remote `missing`"),
            "{hook} `{command}`: {err}"
        );
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
        .config("remote.selected.url", "https://127.0.0.1:9/r")
        .config("remote.selected.vcs", "fixture")
        .config("remote.other.url", "https://127.0.0.1:9/o")
        .config("remote.other.vcs", "otherhelper");
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
    committed
        .config("remote.selected.url", "https://127.0.0.1:9/r")
        .config("remote.selected.vcs", "fixture");
    assert_rows(&committed, &["git fetch selected"], &[]);
    let edited = Scratch::landed(None);
    write_policy(edited.root(), ADMITTING);
    edited
        .config("remote.selected.url", "https://127.0.0.1:9/r")
        .config("remote.selected.vcs", "fixture");
    assert_rows(&edited, &["git fetch selected"], &[]);
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
