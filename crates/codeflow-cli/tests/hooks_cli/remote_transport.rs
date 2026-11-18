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
        Self::landed_with(|_| landed.map(str::to_owned))
    }

    /// [`Scratch::landed`] with the policy `make` builds from `origin`'s
    /// URL, for a policy that names it.
    fn landed_with(make: impl FnOnce(&str) -> Option<String>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = codeflow_core::portable_path::canonicalize(temp.path()).unwrap();
        let root = base.join("repo");
        std::fs::create_dir(&root).unwrap();
        enforced_repo(&root);
        let origin = base.join("origin.git");
        if let Some(policy) = make(&shell_path(&origin)) {
            write_agent_policy(&root, &policy);
        }
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

    /// Judge `command` as run from the directory `cwd`.
    fn judge_in(hook: &str, command: &str, cwd: &Path) -> Output {
        run_with_stdin(
            codeflow()
                .args(["hook", hook, "--contract", "3"])
                .current_dir(cwd),
            &guard_payload(command, cwd),
        )
    }

    /// Judge `command` with `inherited` set in the guard's own environment,
    /// as a session that exports it would run the hook.
    fn judge_inheriting(&self, hook: &str, command: &str, inherited: (&str, &str)) -> Output {
        run_with_stdin(
            codeflow()
                .args(["hook", hook, "--contract", "3"])
                .env(inherited.0, inherited.1)
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

/// git-guard's earlier `git.policy_authority` refusal of a remote section
/// with an empty name, which it cannot read.
const UNREADABLE_EMPTY_REMOTE: &str = "policy rule git.policy_authority (block)\n  cannot read configured remote : '' is not a valid remote name.";

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
                    wrong.push(format!(
                        "{hook} should refuse `{command}` first by `{reason}`: {err}"
                    ));
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

fn assert_nested(scratch: &Scratch, refused: &[&str], allowed: &[&str]) {
    assert_judged_with(scratch, nested_refusal, refused, allowed, &[]);
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
        &[
            "git fetch origin",
            "git ls-remote origin",
            "git fetch pushed",
        ],
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
fn both_guards_judge_every_implicit_remote_registration() {
    // Each way of registering an implicit remote counts.
    let filtered = Scratch::landed(None);
    filtered
        .config("remote.shadow.url", "fixture::x")
        .config("remote.shadow.partialCloneFilter", "blob:none");
    assert_rows(&filtered, &["git log -1"], &[]);
    let unset = Scratch::landed(None);
    unset
        .config("remote.shadow.url", "fixture::x")
        .config("remote.shadow.promisor", "false")
        .config("remote.shadow.partialCloneFilter", "blob:none");
    assert_rows(&unset, &["git log -1"], &[]);
    let retracted = Scratch::landed(None);
    retracted
        .config("remote.shadow.url", "fixture::x")
        .config("remote.shadow.promisor", "true")
        .config("remote.shadow.promisor", "false");
    assert_rows(&retracted, &["git log -1"], &[]);
    // A helper remote that is no promisor is not contacted.
    let plain = Scratch::landed(None);
    plain
        .config("remote.shadow.url", "fixture::x")
        .config("remote.shadow.promisor", "false");
    assert_rows(&plain, &[], &["git log -1"]);
    // A name Git reads as empty, in either representation of it, refuses.
    for config in [
        &b"[remote \"\"]\n\tpartialCloneFilter = blob:none\n"[..],
        &b"[remote]\n\tpartialCloneFilter = blob:none\n"[..],
        &b"[remote]\n\tpromisor = true\n"[..],
    ] {
        let empty = Scratch::landed(None);
        empty.append_config(config);
        assert_unproven(&empty, &["git log -1"], &[]);
    }
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
    // The transport that carries a command keeps its own refusal and message.
    for hook in HOOKS {
        let out = scratch.judge(hook, "git ls-remote 'ext::sh -c true'");
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{hook}: {err}");
        assert!(err.contains("ext:: transport command"), "{hook}: {err}");
    }
}

#[test]
fn destinations_are_resolved_per_direction() {
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
fn both_guards_refuse_a_program_chosen_by_remote_settings() {
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
fn both_guards_refuse_an_implicit_contact_they_cannot_prove() {
    // A fallback the guards cannot prove refuses.
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
            "git maintenance run",
            "git maintenance run --task=prefetch",
        ],
        &["git fetch origin", "git maintenance run --task=gc"],
    );
    // An update with no operand reads the remotes Git selects: here every
    // remote, so the helper remote refuses by name.
    assert_rows(&all, &["git remote update"], &[]);
    let skipped = Scratch::landed(None);
    skipped
        .config("remote.backup.url", "fixture::x")
        .config("remote.backup.skipDefaultUpdate", "true");
    assert_rows(
        &skipped,
        &["git remote update backup"],
        &["git remote update"],
    );

    let ordinary = Scratch::landed(None);
    assert_unproven(
        &ordinary,
        &[
            "git push",
            "git ls-remote",
            "git push --force-with-lease=main",
        ],
        &[
            "git fetch",
            "git push origin HEAD:refs/heads/task/x",
            "git remote update",
        ],
    );
}

#[test]
fn both_guards_refuse_a_selection_that_hides_a_remote() {
    // A group the guards cannot resolve refuses.
    for group in [&b"\"\""[..], &b"\" \\t \""[..]] {
        let configured = Scratch::landed(None);
        configured
            .append_config(b"[remote \"default\"]\n\turl = fixture::x\n[remotes]\n\tdefault = ")
            .append_config(group)
            .append_config(b"\n");
        assert_unproven(&configured, &["git remote update"], &[]);
        let legacy = Scratch::landed(None);
        legacy
            .append_config(b"[remotes]\n\tdefault = ")
            .append_config(group)
            .append_config(b"\n")
            .write(".git/remotes/default", b"URL: fixture::x\n");
        // git-guard refuses first: the selected name is not a configured
        // remote.
        assert_fetch_without_url(&legacy, "git remote update");
    }

    // git-guard refuses an unnamed section first, by its earlier rule for a
    // configured remote it cannot read.
    let empty = Scratch::landed(None);
    empty.append_config(b"[remote \"\"]\n\turl = fixture::x\n");
    assert_judged_with(
        &empty,
        unproven_refusal,
        &["git remote update"],
        &[],
        &[("git remote update", UNREADABLE_EMPTY_REMOTE)],
    );
    // A dotted name and names that differ only in case are each read.
    let dotted = Scratch::landed(None);
    dotted.config("remote.a.b.url", "fixture::x");
    assert_rows(&dotted, &["git remote update"], &[]);
    let cased = Scratch::landed(None);
    cased.append_config(
        b"[remote \"Up\"]\n\turl = fixture::x\n[remote \"up\"]\n\turl = https://127.0.0.1:9/r\n\tskipDefaultUpdate = true\n",
    );
    assert_rows(&cased, &["git remote update"], &[]);
}

#[test]
fn both_guards_refuse_every_contact_beside_an_unnamed_remote_definition() {
    // A definition the guards cannot resolve refuses every contact that
    // reads the configuration's remotes. git-guard refuses first, by its
    // earlier rule for a configured remote it cannot read.
    let empty = Scratch::landed(None);
    empty.append_config(
        b"[remote \"\"]\n\turl = fixture::x\n\tpushurl = fixture::x\n\tvcs = fixture\n",
    );
    assert_judged_with(
        &empty,
        section_refusal,
        &[
            "git ls-remote .url",
            "git fetch .url",
            "git ls-remote .vcs",
            "git push .pushurl HEAD:refs/heads/task/x",
            "git ls-remote origin",
            "git fetch origin",
            "git ls-remote https://127.0.0.1:9/r",
        ],
        &["git log -1"],
        &[("git fetch .url", UNREADABLE_EMPTY_REMOTE)],
    );
    // A section whose name starts with a dot is an ordinary remote.
    let dotted = Scratch::landed(None);
    dotted.append_config(b"[remote \".a\"]\n\turl = fixture::x\n");
    assert_rows(&dotted, &["git ls-remote .a"], &["git fetch origin"]);
}

/// A refusal by this task's rule for a remote section with an empty name,
/// naming the key it reads.
fn section_refusal(out: &Output) -> bool {
    unproven_refusal(out) && stderr(out).contains("`remote..")
}

/// Replace the legacy remote folder `folder` of `scratch` with `form`.
#[cfg(unix)]
fn legacy_folder(scratch: &Scratch, folder: &str, form: &str) {
    let path = scratch.root().join(".git").join(folder);
    // A link or file goes with `remove_file`; a folder needs the other call.
    if std::fs::remove_file(&path).is_err() {
        let _ = std::fs::remove_dir_all(&path);
    }
    let target = scratch.base.join("empty-target");
    std::fs::create_dir_all(&target).unwrap();
    match form {
        "empty" => std::fs::create_dir(&path).unwrap(),
        "link to empty" => std::os::unix::fs::symlink(&target, &path).unwrap(),
        "dangling link" => std::os::unix::fs::symlink(target.join("gone"), &path).unwrap(),
        "file" => std::fs::write(&path, "").unwrap(),
        "populated" => {
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("old"), "URL: fixture::x\n").unwrap();
        }
        _ => unreachable!("{form}"),
    }
}

#[cfg(unix)]
#[test]
fn both_guards_pass_an_older_definition_store_only_when_proven_empty() {
    let scratch = Scratch::landed(None);
    for folder in ["remotes", "branches"] {
        legacy_folder(&scratch, folder, "empty");
        assert_unproven(&scratch, &[], &["git remote update"]);
        for form in ["link to empty", "dangling link", "file", "populated"] {
            legacy_folder(&scratch, folder, form);
            assert_unproven(&scratch, &["git remote update"], &[]);
        }
        legacy_folder(&scratch, folder, "empty");
    }
}

#[test]
fn both_guards_refuse_each_implicit_fetch_state_they_cannot_prove() {
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
fn both_guards_refuse_an_ambiguous_remote_name() {
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
fn an_unresolved_selection_refuses_in_both_guards() {
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
fn both_guards_refuse_nested_contacts_they_cannot_prove() {
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
fn a_location_setting_the_probe_does_not_read_keeps_nested_contacts_refused() {
    // A location setting cannot hide a record the probe found.
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
    // Recursion refuses with the setting where it passes without it.
    let plain = Scratch::landed(None);
    let elsewhere = plain.base.join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let elsewhere = shell_path(&elsewhere);
    let setting = format!("git -c core.worktree={elsewhere} fetch origin");
    let off = format!("git -c core.worktree={elsewhere} fetch --no-recurse-submodules origin");
    assert_unproven(&plain, &[&setting], &["git fetch origin", &off]);
}

#[test]
fn both_guards_refuse_a_remote_with_ambiguous_addresses() {
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
    // Two remotes and none that Git falls back to leave the policy authority
    // unresolved, so only the recovery fetch of a configured remote may pass,
    // and only over a transport Git runs itself.
    let scratch = Scratch::landed(None);
    git(scratch.root(), &["remote", "rename", "origin", "first"]);
    scratch.config("remote.second.url", "fixture::x");
    for hook in HOOKS {
        for command in ["git fetch second", "git fetch"] {
            let out = scratch.judge(hook, command);
            let err = stderr(&out);
            assert_eq!(out.status.code(), Some(2), "{hook} `{command}`: {err}");
            assert!(
                err.contains("policy source is ambiguous"),
                "{hook} `{command}`: {err}"
            );
        }
        let out = scratch.judge(hook, "git fetch first");
        assert_eq!(out.status.code(), Some(0), "{hook}: {}", stderr(&out));
    }
}

#[test]
fn both_guards_refuse_a_name_an_older_definition_may_define() {
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
fn an_unreadable_configuration_refuses_by_the_shared_reader() {
    // The file sits in a wider scope, which the project-root reader does not
    // open, so the refusal comes from the shared configuration reader and
    // not from an earlier check.
    let scratch = Scratch::landed(None);
    let extra = scratch.base.join("extra-config");
    std::fs::write(&extra, "[remote \"web\"]\n\turl = https://127.0.0.1:9/r\n").unwrap();
    let global = scratch.base.join("global.gitconfig");
    std::fs::write(
        &global,
        format!("[include]\n\tpath = {}\n", shell_path(&extra)),
    )
    .unwrap();
    let global = shell_path(&global);
    {
        let _denied = super::RestorePermissions::deny(&extra);
        for hook in HOOKS {
            let out = scratch.judge_inheriting(
                hook,
                "git ls-remote https://127.0.0.1:9/r",
                ("GIT_CONFIG_GLOBAL", &global),
            );
            let err = stderr(&out);
            assert_eq!(out.status.code(), Some(2), "{hook}: {err}");
            assert!(
                err.contains("git.hook_integrity")
                    && err.contains("cannot read Git runner: cannot verify Git configuration"),
                "{hook}: {err}"
            );
        }
    }
    // Control: once the same file is readable, the same line passes, so the
    // refusal above comes from the unreadable file and not from the line.
    for hook in HOOKS {
        let out = scratch.judge_inheriting(
            hook,
            "git ls-remote https://127.0.0.1:9/r",
            ("GIT_CONFIG_GLOBAL", &global),
        );
        assert_eq!(out.status.code(), Some(0), "{hook}: {}", stderr(&out));
    }
}

#[test]
fn the_landed_policy_admits_a_named_program() {
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
fn only_the_landed_policy_admits_a_program() {
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
    // Nor is a setting on the command line, whatever key it uses.
    let line = Scratch::landed(None);
    line.config("remote.selected.url", "fixture::https://127.0.0.1:9/r");
    assert_rows(
        &line,
        &[
            "git -c git.admittedHelpers=git-remote-fixture fetch selected",
            "git -c codeflow.admittedHelpers=git-remote-fixture fetch selected",
        ],
        &[],
    );
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
fn the_landed_policy_skips_the_account_finding_for_a_named_entry() {
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
    // Reading the credential directly stays refused, by the account rule.
    let out = admitted.judge("exec-guard", read);
    let err = stderr(&out);
    assert!(
        out.status.code() == Some(2)
            && err.contains("security.outward_actions")
            && account_finding(&out),
        "{err}"
    );
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

/// git-guard's earlier rule that refuses adding a remote, before it reads
/// what Git runs.
const REMOTE_IDENTITY: &str = "the operator manages remote identity";

#[test]
fn both_guards_read_every_spelling_that_turns_nested_contacts_on() {
    let plain = Scratch::landed(None);
    assert_unproven(
        &plain,
        &[
            "git clone --recurse-submodules=no https://127.0.0.1:9/r copy",
            "git clone --recursive=lib https://127.0.0.1:9/r copy",
            "git clone -c submodule.recurse=true https://127.0.0.1:9/r copy",
        ],
        &["git clone --no-recurse-submodules https://127.0.0.1:9/r copy"],
    );
    // The option reader both guards share refuses an option it cannot prove
    // before the transport resolution reads it.
    for hook in HOOKS {
        let out = plain.judge(hook, "git clone --recurs https://127.0.0.1:9/r copy");
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{hook}: {err}");
        assert!(err.contains("ambiguous option `--recurs`"), "{hook}: {err}");
    }
    let listed = Scratch::landed(None);
    listed.write(".gitmodules", b"");
    assert_unproven(
        &listed,
        &["git push --no-recurse-submodules --recu=on-demand origin HEAD:refs/heads/task/x"],
        &["git push --no-recurse-submodules origin HEAD:refs/heads/task/x"],
    );
    let ordered = Scratch::landed(None);
    ordered
        .write(".gitmodules", b"")
        .config("push.recurseSubmodules", "no")
        .config("submodule.recurse", "true");
    assert_unproven(
        &ordered,
        &["git push origin HEAD:refs/heads/task/x"],
        &["git push --no-recurse-submodules origin HEAD:refs/heads/task/x"],
    );
}

#[test]
fn both_guards_refuse_indirect_contacts_they_cannot_prove() {
    let scratch = Scratch::landed(None);
    scratch
        .config("remote.fresh.vcs", "fixture")
        .config("remote.backup.url", "fixture::x");
    let add = "git remote add -f fresh https://127.0.0.1:9/r";
    assert_judged_with(
        &scratch,
        unproven_refusal,
        &[
            "git submodule--helper update --init",
            add,
            "git maintenance run --task=PREFETCH",
            "git maintenance run --task=gc --ta=prefetch",
        ],
        &["git maintenance run --task=gc"],
        &[(add, REMOTE_IDENTITY)],
    );
}

/// A word an option reading consumes as a value proves nothing in either
/// guard.
#[test]
fn both_guards_read_a_consumed_word_as_a_value() {
    let listed = Scratch::landed(None);
    listed.write(".gitmodules", b"");
    assert_unproven(
        &listed,
        &[
            "git fetch --recurse-submodules=on-demand --server-option --no-recurse-submodules origin",
            "git fetch --server-option --recurse-submodules=no origin",
        ],
        &["git fetch -q --no-recurse-submodules origin"],
    );
    let plain = Scratch::landed(None);
    assert_unproven(
        &plain,
        &[
            "git clone --recurse-submodules --server-option --no-recurse-submodules https://127.0.0.1:9/r copy",
            "git clone --server-option -- --recurse-submodules https://127.0.0.1:9/r copy",
        ],
        &["git clone -q --no-recurse-submodules https://127.0.0.1:9/r copy"],
    );
}

/// A remote definition the guards cannot prove refuses in both. git-guard
/// refuses every such definition first by its remote identity rule.
#[test]
fn both_guards_read_a_new_remote_definition_as_git_does() {
    let scratch = Scratch::landed(None);
    let global = scratch.base.join("global.gitconfig");
    std::fs::write(&global, "[remote \"fresh\"]\n\tvcs = fixture\n").unwrap();
    let global = shell_path(&global);
    let add = "git remote add -t -- -f fresh https://127.0.0.1:9/r";
    let out = scratch.judge_inheriting("exec-guard", add, ("GIT_CONFIG_GLOBAL", &global));
    assert!(unproven_refusal(&out), "exec-guard: {}", stderr(&out));
    let out = scratch.judge_inheriting("git-guard", add, ("GIT_CONFIG_GLOBAL", &global));
    let err = stderr(&out);
    assert!(
        out.status.code() == Some(2) && err.contains(REMOTE_IDENTITY),
        "git-guard: {err}"
    );
}

/// A verb the contacting-verb inventory lists without a resolution of its
/// own refuses by name in both guards; an ordinary listing still passes.
#[test]
fn both_guards_refuse_a_listed_contacting_verb_they_do_not_resolve() {
    let scratch = Scratch::landed(None);
    assert_unproven(
        &scratch,
        &["git request-pull HEAD fixture::x HEAD"],
        &["git ls-remote origin"],
    );
}

#[cfg(unix)]
#[test]
fn both_guards_check_an_older_definition_name_as_git_reads_it() {
    let scratch = Scratch::landed(None);
    scratch.write(".git/remotes/old\\name", b"URL: fixture::x\n");
    std::fs::write(scratch.root().join(".git/config"), b"").unwrap();
    for hook in HOOKS {
        let out = scratch.judge(hook, "git ls-remote 'old\\name'");
        assert!(unproven_refusal(&out), "{hook}: {}", stderr(&out));
    }
}

#[test]
fn an_inherited_location_input_admits_nothing() {
    let a = Scratch::landed(Some(ADMITTING_WARN));
    a.config("remote.selected.url", "fixture::https://127.0.0.1:9/r");
    let b = Scratch::landed(None);
    b.config("remote.selected.url", "fixture::https://127.0.0.1:9/r");
    let b_git = shell_path(&b.root().join(".git"));
    for hook in HOOKS {
        let plain = a.judge(hook, "git fetch selected");
        assert_eq!(plain.status.code(), Some(0), "{hook}: {}", stderr(&plain));
        for inherited in [
            ("GIT_DISCOVERY_ACROSS_FILESYSTEM", "1"),
            ("GIT_DIR", b_git.as_str()),
        ] {
            let out = a.judge_inheriting(hook, "git fetch selected", inherited);
            assert!(
                helper_refusal(&out),
                "{hook} {inherited:?}: {}",
                stderr(&out)
            );
        }
    }
}

/// TSK-1479's rule refusing an agent push of a listed branch.
const AGENT_PUSH: &str = "git.agent_push_refused_branches";

/// `git.agent_remote_urls` and `git.admitted_helpers` each keep to their
/// own rule in these cases: a listed agent remote URL admits no helper
/// program for a fetch and keeps the account finding on a listing, and an
/// admitted helper relaxes no agent push refusal of a protected or
/// integration branch.
#[test]
fn the_two_policy_lists_keep_to_their_own_rules() {
    const HELPER_URL: &str = "fixture::https://127.0.0.1:9/r";
    let urls_only = Scratch::landed_with(|origin| {
        let policy = serde_json::json!({"git": {
            "protected_branches": ["main", "master"],
            "root_checkout_commits": "off",
            "agent_remote_urls": [origin, HELPER_URL],
        }});
        Some(policy.to_string())
    });
    urls_only
        .config("remote.selected.url", HELPER_URL)
        .config("credential.helper", "!gh auth git-credential");
    for hook in HOOKS {
        let out = urls_only.judge(hook, "git fetch selected");
        assert!(helper_refusal(&out), "{hook}: {}", stderr(&out));
    }
    let out = urls_only.judge("exec-guard", "git ls-remote origin");
    assert!(
        out.status.code() == Some(2) && account_finding(&out),
        "{}",
        stderr(&out)
    );

    let helpers_only = Scratch::landed(Some(ADMITTING));
    helpers_only.config("credential.helper", "!gh auth git-credential");
    for line in [
        "git push origin HEAD:refs/heads/main",
        "git push origin HEAD:refs/heads/integration/EPC-001-line",
    ] {
        for hook in HOOKS {
            let out = helpers_only.judge(hook, line);
            assert!(
                out.status.code() == Some(2) && stderr(&out).contains(AGENT_PUSH),
                "{hook} {line}: {}",
                stderr(&out)
            );
        }
    }
}

/// Through both guards: a selection word is read as Git reads it.
#[test]
fn both_guards_read_a_selection_word_as_git_reads_it() {
    let scratch = Scratch::landed(None);
    scratch.config("remote.backup.url", "fixture::x");
    assert_rows(
        &scratch,
        &["git remote update default", "git remote update -p default"],
        &["git remote update origin"],
    );
}

/// Through both guards: a word Git keeps as an operand names the remote.
#[test]
fn both_guards_read_every_word_git_keeps_as_an_operand() {
    let scratch = Scratch::landed(None);
    scratch.config("remote.-.url", "fixture::x");
    assert_rows(
        &scratch,
        &[
            "git fetch -",
            "git ls-remote -",
            "git remote show -",
            "git push - HEAD:refs/heads/task/x",
        ],
        &["git fetch origin"],
    );
}

/// Through both guards: a new copy judges every address it may use.
#[test]
fn both_guards_judge_every_address_a_copy_may_use() {
    let scratch = Scratch::landed(None);
    let global = scratch.base.join("global.gitconfig");
    std::fs::write(&global, "[remote \"upstream\"]\n\turl = fixture::x\n").unwrap();
    let global = shell_path(&global);
    for command in [
        "git clone --no-recurse-submodules https://127.0.0.1:9/r copy",
        "git clone -o upstream --no-recurse-submodules https://127.0.0.1:9/r copy",
    ] {
        for hook in HOOKS {
            let out = scratch.judge_inheriting(hook, command, ("GIT_CONFIG_GLOBAL", &global));
            assert!(helper_refusal(&out), "{hook} `{command}`: {}", stderr(&out));
            let out = scratch.judge(hook, command);
            assert_eq!(
                out.status.code(),
                Some(0),
                "{hook} `{command}`: {}",
                stderr(&out)
            );
        }
    }
}

/// Build a nested-repository fixture `lib` under `scratch`; the returned
/// value is its commit.
fn record_nested(scratch: &Scratch) -> String {
    git(scratch.root(), &["init", "-q", "lib"]);
    git(
        &scratch.root().join("lib"),
        &["config", "remote.origin.url", "fixture::x"],
    );
    let head = git2::Repository::open(scratch.root())
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap()
        .to_string();
    git(
        scratch.root(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{head},lib"),
        ],
    );
    head
}

/// Through both guards: a nested repository the guards cannot rule out
/// refuses unless the line turns recursion off.
#[test]
fn both_guards_refuse_nested_contacts_with_one_marker() {
    let scratch = Scratch::landed(None);
    let head = record_nested(&scratch);
    assert_nested(
        &scratch,
        &[
            "git fetch origin",
            "git fetch --recurse-submodules origin",
            "git pull origin main",
            "git fetch",
        ],
        &["git fetch --no-recurse-submodules origin"],
    );

    let selected = Scratch::landed(None);
    let index = selected.base.join("other-index");
    let mut other = git2::Index::open(&index).unwrap();
    other
        .add(&git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: 0o160_000,
            uid: 0,
            gid: 0,
            file_size: 0,
            id: git2::Oid::from_str(&head).unwrap(),
            flags: 3,
            flags_extended: 0,
            path: b"lib".to_vec(),
        })
        .unwrap();
    other.write().unwrap();
    let index = shell_path(&index);
    // exec-guard refuses an index file set on the line first, by its rule
    // for what Git reads.
    let line = format!("GIT_INDEX_FILE={index} git fetch origin");
    let out = selected.judge("git-guard", &line);
    assert!(nested_refusal(&out), "git-guard: {}", stderr(&out));
    let out = selected.judge("exec-guard", &line);
    let err = stderr(&out);
    assert!(
        out.status.code() == Some(2) && err.contains("security.secret_reads"),
        "exec-guard: {err}"
    );
    assert_unproven(&selected, &[], &["git fetch origin"]);
    for hook in HOOKS {
        let out = selected.judge_inheriting(hook, "git fetch origin", ("GIT_INDEX_FILE", &index));
        assert!(nested_refusal(&out), "{hook}: {}", stderr(&out));
    }
}

/// Hold a definition file in the index only, or (with `in_commit`) in the
/// last commit only, and leave the work tree without it.
fn definition_outside_work_tree(scratch: &Scratch, in_commit: bool) {
    let repo = git2::Repository::open(scratch.root()).unwrap();
    std::fs::write(scratch.root().join(".gitmodules"), "").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(".gitmodules")).unwrap();
    index.write().unwrap();
    if in_commit {
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let who = git2::Signature::now("fixture", "fixture@example.test").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &who, &who, "fixture", &tree, &[&parent])
            .unwrap();
        index.remove_path(Path::new(".gitmodules")).unwrap();
        index.write().unwrap();
    }
    std::fs::remove_file(scratch.root().join(".gitmodules")).unwrap();
}

/// Through both guards: a definition the work tree lacks but the index or
/// the last commit holds refuses unless the line turns recursion off. The
/// control holds no definition anywhere.
#[test]
fn both_guards_refuse_a_definition_held_outside_the_work_tree() {
    for in_commit in [false, true] {
        let scratch = Scratch::landed(None);
        definition_outside_work_tree(&scratch, in_commit);
        assert_nested(
            &scratch,
            &["git fetch origin", "git fetch", "git pull origin main"],
            &["git fetch --no-recurse-submodules origin"],
        );
    }
    let control = Scratch::landed(None);
    assert_rows(&control, &[], &["git fetch origin", "git fetch"]);
}

/// A refusal by this task's rule for nested repositories the guards cannot
/// rule out.
fn nested_refusal(out: &Output) -> bool {
    unproven_refusal(out) && stderr(out).contains("may recurse into nested repositories")
}

fn unresolved_probe_refusal(out: &Output) -> bool {
    unproven_refusal(out)
        && stderr(out).contains(
            "cannot establish an index or object input for this child; run the commands separately",
        )
}

/// Through both guards: the definition check reads the top of the
/// repository from any directory and through the directory option, with the
/// nested-repository reason and a recursion-off control.
#[test]
fn both_guards_read_the_definition_from_any_directory() {
    for in_commit in [false, true] {
        let scratch = Scratch::landed(None);
        definition_outside_work_tree(&scratch, in_commit);
        let child = scratch.root().join("sub");
        std::fs::create_dir(&child).unwrap();
        for hook in HOOKS {
            for (cwd, refused, allowed) in [
                (
                    child.as_path(),
                    "git fetch origin",
                    "git fetch --no-recurse-submodules origin",
                ),
                (
                    scratch.root(),
                    "git -C sub fetch origin",
                    "git -C sub fetch --no-recurse-submodules origin",
                ),
                (
                    child.as_path(),
                    "git -C . pull origin main",
                    "git -C . pull --no-recurse-submodules origin main",
                ),
            ] {
                let out = Scratch::judge_in(hook, refused, cwd);
                assert!(nested_refusal(&out), "{hook} `{refused}`: {}", stderr(&out));
                let out = Scratch::judge_in(hook, allowed, cwd);
                assert_eq!(
                    out.status.code(),
                    Some(0),
                    "{hook} `{allowed}`: {}",
                    stderr(&out)
                );
            }
        }
    }
    // Control: a definition below the top only.
    let below = Scratch::landed(None);
    below.write("sub/.gitmodules", b"");
    git(below.root(), &["add", "sub/.gitmodules"]);
    for hook in HOOKS {
        for cwd in [below.root().to_path_buf(), below.root().join("sub")] {
            let out = Scratch::judge_in(hook, "git fetch origin", &cwd);
            assert_eq!(out.status.code(), Some(0), "{hook}: {}", stderr(&out));
        }
    }
}

/// Commit a tree holding a nested-repository record named `lib`, on the new
/// branch `branch`, or as the last commit with no record in the index.
fn commit_nested_record(scratch: &Scratch, branch: Option<&str>) {
    let repo = git2::Repository::open(scratch.root()).unwrap();
    let mut tree = repo.treebuilder(None).unwrap();
    tree.insert(
        "lib",
        git2::Oid::from_str("0123456789012345678901234567890123456789").unwrap(),
        0o160_000,
    )
    .unwrap();
    let tree = repo.find_tree(tree.write().unwrap()).unwrap();
    let who = git2::Signature::now("fixture", "fixture@example.test").unwrap();
    if let Some(name) = branch {
        let update = format!("refs/heads/{name}");
        repo.commit(Some(&update), &who, &who, "fixture", &tree, &[])
            .unwrap();
    } else {
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &who, &who, "fixture", &tree, &[&parent])
            .unwrap();
    }
}

/// Through both guards: a push whose recursion may be on refuses, whether
/// the nested record is in a pushed revision other than the last commit, in
/// the last commit only, or nowhere the guards read. A push that leaves
/// recursion off, or turns it off on the line, keeps its verdict.
#[test]
fn both_guards_refuse_a_push_whose_recursion_may_be_on() {
    let elsewhere = Scratch::landed(None);
    commit_nested_record(&elsewhere, Some("other"));
    let last = Scratch::landed(None);
    commit_nested_record(&last, None);
    let plain = Scratch::landed(None);
    for scratch in [&elsewhere, &last, &plain] {
        assert_judged_with(
            scratch,
            nested_refusal,
            &[
                "git push --recurse-submodules=on-demand origin other:refs/heads/task/x",
                "git push --recurse-submodules=check origin HEAD:refs/heads/task/x",
            ],
            &[
                "git push origin other:refs/heads/task/x",
                "git push --no-recurse-submodules origin HEAD:refs/heads/task/x",
                "git push --recurse-submodules=no origin HEAD:refs/heads/task/x",
            ],
            &[],
        );
    }
    let set = Scratch::landed(None);
    set.config("push.recurseSubmodules", "on-demand");
    assert_judged_with(
        &set,
        nested_refusal,
        &["git push origin HEAD:refs/heads/task/x"],
        &["git push --no-recurse-submodules origin HEAD:refs/heads/task/x"],
        &[],
    );
}

/// Through git-guard: a replacement variable the line sets reaches the
/// probe. exec-guard refuses such a line first, by its rule for what Git
/// reads, so the row covers git-guard only, and exec-guard is held to
/// refusing it.
#[test]
fn git_guard_reads_a_replacement_variable_set_on_the_line() {
    let scratch = Scratch::landed(None);
    definition_outside_work_tree(&scratch, true);
    let repo = git2::Repository::open(scratch.root()).unwrap();
    let original = repo.head().unwrap().target().unwrap();
    let tree = repo
        .find_tree(repo.treebuilder(None).unwrap().write().unwrap())
        .unwrap();
    let who = git2::Signature::now("fixture", "fixture@example.test").unwrap();
    let swap = repo
        .commit(None, &who, &who, "fixture", &tree, &[])
        .unwrap();
    repo.reference(&format!("refs/replace/{original}"), swap, true, "fixture")
        .unwrap();
    // Controls: the replacement is followed as the call follows it.
    for hook in HOOKS {
        let out = scratch.judge(hook, "git fetch origin");
        assert_eq!(out.status.code(), Some(0), "{hook}: {}", stderr(&out));
    }
    for line in [
        "GIT_NO_REPLACE_OBJECTS=1 git fetch origin",
        "env GIT_NO_REPLACE_OBJECTS=1 git fetch origin",
    ] {
        let out = scratch.judge("git-guard", line);
        assert!(nested_refusal(&out), "{line}: {}", stderr(&out));
        let out = scratch.judge("exec-guard", line);
        assert_eq!(out.status.code(), Some(2), "{line}: {}", stderr(&out));
    }
    let out = scratch.judge(
        "git-guard",
        "GIT_NO_REPLACE_OBJECTS=1 git fetch --no-recurse-submodules origin",
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

/// Make a replacement for the last commit: a commit with an empty tree, or
/// with `with_definition`, one that holds the definition file.
fn replace_last_commit(scratch: &Scratch, with_definition: bool) {
    let repo = git2::Repository::open(scratch.root()).unwrap();
    let original = repo.head().unwrap().target().unwrap();
    let mut builder = repo.treebuilder(None).unwrap();
    if with_definition {
        let blob = repo.blob(b"").unwrap();
        builder.insert(".gitmodules", blob, 0o100_644).unwrap();
    }
    let tree = repo.find_tree(builder.write().unwrap()).unwrap();
    let who = git2::Signature::now("fixture", "fixture@example.test").unwrap();
    let swap = repo
        .commit(None, &who, &who, "fixture", &tree, &[])
        .unwrap();
    repo.reference(&format!("refs/replace/{original}"), swap, true, "fixture")
        .unwrap();
}

/// A refusal by git-guard for the nested repositories the probe found.
fn git_guard_refuses(scratch: &Scratch, line: &str) {
    let out = scratch.judge("git-guard", line);
    assert!(nested_refusal(&out), "{line}: {}", stderr(&out));
}

/// git-guard allows the line.
fn git_guard_allows(scratch: &Scratch, line: &str) {
    let out = scratch.judge("git-guard", line);
    assert_eq!(out.status.code(), Some(0), "{line}: {}", stderr(&out));
}

#[test]
fn git_guard_reads_removal_from_an_inherited_view() {
    let scratch = Scratch::landed(None);
    replace_last_commit(&scratch, true);
    let mut wrong = Vec::new();
    for line in [
        "unset GIT_NO_REPLACE_OBJECTS; git fetch origin",
        "GIT_NO_REPLACE_OBJECTS=1; unset GIT_NO_REPLACE_OBJECTS; git fetch origin",
        "export -n GIT_NO_REPLACE_OBJECTS; git fetch origin",
        "declare +x GIT_NO_REPLACE_OBJECTS; git fetch origin",
    ] {
        let out = scratch.judge_inheriting("git-guard", line, ("GIT_NO_REPLACE_OBJECTS", "1"));
        if line.starts_with("declare +x") {
            assert!(out.status.code() == Some(2) && stderr(&out).contains("git.policy_authority"));
            continue;
        }
        if !(nested_refusal(&out) || unresolved_probe_refusal(&out)) {
            wrong.push(format!("{line}: {}", stderr(&out)));
        }
    }
    let out = scratch.judge_inheriting(
        "git-guard",
        "unset GIT_NO_REPLACE_OBJECTS; git fetch --no-recurse-submodules origin",
        ("GIT_NO_REPLACE_OBJECTS", "1"),
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn git_guard_reads_assignments_across_repeated_bodies() {
    let scratch = Scratch::landed(None);
    definition_outside_work_tree(&scratch, true);
    replace_last_commit(&scratch, false);
    let mut wrong = Vec::new();
    for line in [
        "for i in 1 2; do git fetch origin; export GIT_NO_REPLACE_OBJECTS=1; done",
        "f() { git fetch origin; }; export GIT_NO_REPLACE_OBJECTS=1; f",
        "while false; do git fetch origin; export GIT_NO_REPLACE_OBJECTS=1; done",
    ] {
        let out = scratch.judge("git-guard", line);
        if !nested_refusal(&out) {
            wrong.push(format!("{line}: {}", stderr(&out)));
        }
    }
    git_guard_allows(&scratch, "git fetch origin");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn git_guard_refuses_unresolved_setter_views() {
    let scratch = Scratch::landed(None);
    definition_outside_work_tree(&scratch, true);
    replace_last_commit(&scratch, false);
    let mut wrong = Vec::new();
    for line in [
        "GIT_NO_REPLACE_OBJECTS+=1; export GIT_NO_REPLACE_OBJECTS; git fetch origin",
        "read GIT_NO_REPLACE_OBJECTS; git fetch origin",
        "printf -v GIT_NO_REPLACE_OBJECTS 1; git fetch origin",
        "printf -vGIT_NO_REPLACE_OBJECTS 1; git fetch origin",
        "mapfile GIT_NO_REPLACE_OBJECTS; git fetch origin",
        "((GIT_NO_REPLACE_OBJECTS=1)); git fetch origin",
        "set -a; ((++GIT_NO_REPLACE_OBJECTS)); git fetch origin",
        "set -a; : ${GIT_NO_REPLACE_OBJECTS:=1}; git fetch origin",
        "set -a; : $[GIT_NO_REPLACE_OBJECTS=1]; git fetch origin",
        "trap 'export GIT_NO_REPLACE_OBJECTS=1' DEBUG; git fetch origin",
        "set -a; ((--GIT_REPLACE_REF_BASE)); git fetch origin",
        "set -a; : ${GIT_REPLACE_REF_BASE:=1}; git fetch origin",
        "set -a; : $[GIT_REPLACE_REF_BASE=1]; git fetch origin",
        "trap 'export GIT_REPLACE_REF_BASE=refs/none/' DEBUG; git fetch origin",
        "GIT_NO_REPLACE_OBJECTS[0]=1; git fetch origin",
    ] {
        let out = scratch.judge("git-guard", line);
        let expected = if line.contains("[0]") {
            out.status.code() == Some(2) && stderr(&out).contains("git.policy_authority")
        } else if line.starts_with("set -a") || line.starts_with("trap ") {
            unresolved_probe_refusal(&out)
        } else {
            nested_refusal(&out) || unresolved_probe_refusal(&out)
        };
        if !expected {
            wrong.push(format!("{line}: {}", stderr(&out)));
        }
    }
    git_guard_allows(&scratch, "git fetch origin");
    git_guard_allows(
        &scratch,
        "set -a; ((++GIT_NO_REPLACE_OBJECTS)); git fetch --no-recurse-submodules origin",
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn git_guard_keeps_removals_in_carried_bodies() {
    let scratch = Scratch::landed(None);
    replace_last_commit(&scratch, true);
    let root = shell_path(scratch.root());
    let mut wrong = Vec::new();
    for removal in ["-uGIT_NO_REPLACE_OBJECTS", "-u GIT_NO_REPLACE_OBJECTS"] {
        let line = format!("env {removal} sh -c 'git -C {root} fetch origin'");
        let out = scratch.judge_inheriting("git-guard", &line, ("GIT_NO_REPLACE_OBJECTS", "1"));
        if !unresolved_probe_refusal(&out) {
            wrong.push(format!("{line}: {}", stderr(&out)));
        }
        let off =
            format!("env {removal} sh -c 'git -C {root} fetch --no-recurse-submodules origin'");
        let out = scratch.judge_inheriting("git-guard", &off, ("GIT_NO_REPLACE_OBJECTS", "1"));
        assert_eq!(out.status.code(), Some(0), "{off}: {}", stderr(&out));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn git_guard_judges_runtime_probe_inputs() {
    let scratch = Scratch::landed(None);
    definition_outside_work_tree(&scratch, true);
    replace_last_commit(&scratch, false);
    for setter in [
        "y=_OBJECTS; x=GIT_NO_REPLACE$y=1; set -a; ((x))",
        "y=_OBJECTS; x=GIT_NO_REPLACE$y=1; set -o allexport; declare -i n=x",
        "y=_OBJECTS; x=GIT_NO_REPLACE$y=1; set -ea; [[ x -eq 1 ]]",
    ] {
        let line = format!("{setter}; git fetch origin");
        let out = scratch.judge("git-guard", &line);
        assert!(unresolved_probe_refusal(&out), "{line}: {}", stderr(&out));
        git_guard_allows(
            &scratch,
            &format!("{setter}; git fetch --no-recurse-submodules origin"),
        );
    }
}

#[test]
fn git_guard_judges_inherited_arithmetic_inputs() {
    let scratch = Scratch::landed(None);
    definition_outside_work_tree(&scratch, true);
    replace_last_commit(&scratch, false);
    for setter in [
        "y=_BASE; x=GIT_REPLACE_REF$y=1; ((x))",
        "y=_BASE; x=GIT_REPLACE_REF$y=1; : $((x))",
        "y=_BASE; x=GIT_REPLACE_REF$y=1; declare -i n=x",
        "y=_BASE; x=GIT_REPLACE_REF$y=1; [[ x -eq 1 ]]",
    ] {
        let line = format!("{setter}; git fetch origin");
        let out = scratch.judge_inheriting(
            "git-guard",
            &line,
            ("GIT_REPLACE_REF_BASE", "refs/replace/"),
        );
        assert!(unresolved_probe_refusal(&out), "{line}: {}", stderr(&out));
        let off = format!("{setter}; git fetch --no-recurse-submodules origin");
        let out =
            scratch.judge_inheriting("git-guard", &off, ("GIT_REPLACE_REF_BASE", "refs/replace/"));
        assert_eq!(out.status.code(), Some(0), "{off}: {}", stderr(&out));
    }
}

#[test]
fn git_guard_keeps_noncontacting_mentions_allowed() {
    let scratch = Scratch::landed(None);
    for line in [
        "git log --grep='GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_NO_REPLACE_OBJECTS GIT_REPLACE_REF_BASE'",
        "echo 'GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_NO_REPLACE_OBJECTS GIT_REPLACE_REF_BASE'; git status",
    ] {
        git_guard_allows(&scratch, line);
    }
    let out = scratch.judge("git-guard", "echo 'GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_NO_REPLACE_OBJECTS GIT_REPLACE_REF_BASE'; git fetch --no-recurse-submodules origin");
    assert!(
        out.status.code() == Some(2) && stderr(&out).contains("git.policy_authority"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn git_guard_judges_opaque_runtime_inputs() {
    let scratch = Scratch::landed(None);
    definition_outside_work_tree(&scratch, true);
    replace_last_commit(&scratch, false);
    let mut wrong = Vec::new();
    for setter in [
        "printf -v y %s _OBJECTS; x=GIT_NO_REPLACE$y=1; set -a; ((x))",
        "printf -v y %s _OBJECTS; x=GIT_NO_REPLACE$y=1; shopt -o -s allexport; ((x))",
        "printf -v y %s _OBJECTS; x=GIT_NO_REPLACE$y=1; setopt ALL_EXPORT; ((x))",
        "printf -v y %s _OBJECTS; x=GIT_NO_REPLACE$y=1; ((x))",
        "printf -v y %s _BASE; wait -p \"GIT_REPLACE_REF$y\"",
    ] {
        let line = format!("{setter}; git fetch origin");
        let out = scratch.judge("git-guard", &line);
        if !unresolved_probe_refusal(&out) {
            wrong.push(format!("{line}: {}", stderr(&out)));
        }
        git_guard_allows(
            &scratch,
            &format!("{setter}; git fetch --no-recurse-submodules origin"),
        );
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn git_guard_judges_opaque_inherited_inputs() {
    let scratch = Scratch::landed(None);
    definition_outside_work_tree(&scratch, true);
    replace_last_commit(&scratch, false);
    let mut wrong = Vec::new();
    for setter in [
        "printf -v y %s _BASE; x=GIT_REPLACE_REF$y=1; ((x))",
        "printf -v y %s _BASE; x=GIT_REPLACE_REF$y=1; : $((x))",
        "printf -v y %s _BASE; x=GIT_REPLACE_REF$y=1; declare -i n=x",
        "printf -v y %s _BASE; x=GIT_REPLACE_REF$y=1; [[ x -eq 1 ]]",
    ] {
        let line = format!("{setter}; git fetch origin");
        let out = scratch.judge_inheriting(
            "git-guard",
            &line,
            ("GIT_REPLACE_REF_BASE", "refs/replace/"),
        );
        if !unresolved_probe_refusal(&out) {
            wrong.push(format!("{line}: {}", stderr(&out)));
        }
        let off = format!("{setter}; git fetch --no-recurse-submodules origin");
        let out =
            scratch.judge_inheriting("git-guard", &off, ("GIT_REPLACE_REF_BASE", "refs/replace/"));
        assert_eq!(out.status.code(), Some(0), "{off}: {}", stderr(&out));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn git_guard_judges_nested_arithmetic_inputs() {
    let scratch = Scratch::landed(None);
    definition_outside_work_tree(&scratch, true);
    replace_last_commit(&scratch, false);
    let mut wrong = Vec::new();
    for context in [
        r#": "${s:x}""#,
        r#": "${s:0:x}""#,
        r#": "${BASH_VERSINFO[x]}""#,
        r#"[[ -v a[x] ]]"#,
        r#"eval '((x))'"#,
        r#"eval ': $((x))'"#,
        r#"eval \(\(x\)\)"#,
        ": \"$( ((x)); git fetch origin )\"",
        "# don't\n((x))",
        r#"echo $'\''; ((x))"#,
        "repeat x do :; done",
        "float n=x",
        "typeset -F n=x",
        "typeset -E n=x",
        r#": "$s[x]""#,
        "declare 'a[x]=1'",
        "printf -v 'a[x]' %s 1",
        "read 'a[x]'",
        "unset 'a[x]'",
        "getopts o 'a[x]'",
        "wait -p 'a[x]'",
    ] {
        let prefix = format!("printf -v y %s _BASE; x=GIT_REPLACE_REF$y=1; s=abc; {context}");
        let line = format!("{prefix}; git fetch origin");
        let out = scratch.judge_inheriting(
            "git-guard", &line, ("GIT_REPLACE_REF_BASE", "refs/replace/"),
        );
        // Some destination forms already have an earlier authority refusal.
        if !unresolved_probe_refusal(&out) && !stderr(&out).contains("git.policy_authority") {
            wrong.push(format!("contact {context}: {}", stderr(&out)));
        }
        let off = format!("{}; git fetch --no-recurse-submodules origin", prefix.replace("git fetch origin", "git fetch --no-recurse-submodules origin"));
        let out = scratch.judge_inheriting(
            "git-guard", &off, ("GIT_REPLACE_REF_BASE", "refs/replace/"),
        );
        if out.status.code() != Some(0) && !stderr(&out).contains("git.policy_authority") {
            wrong.push(format!("control {context}: {}", stderr(&out)));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn git_guard_keeps_arithmetic_data_allowed() {
    let scratch = Scratch::landed(None);
    let mut wrong = Vec::new();
    for line in [
        "git fetch origin # ((example))",
        "cat <<'EOF'\n((example))\nEOF\ngit fetch origin",
        "echo '((example))'; git fetch origin",
        "echo '${s:x}'; git fetch origin",
        "echo '${a[x]}'; git fetch origin",
        "echo \"${s:-literal}\"; git fetch origin",
    ] {
        let out = scratch.judge("git-guard", line);
        if out.status.code() != Some(0) {
            wrong.push(format!("{line}: {}", stderr(&out)));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn git_guard_reads_the_environment_of_carried_bodies() {
    let scratch = Scratch::landed(None);
    definition_outside_work_tree(&scratch, true);
    replace_last_commit(&scratch, false);
    let root = shell_path(scratch.root());
    let fetch = format!("git -C {root} fetch origin");
    let mut wrong = Vec::new();
    for line in [
        format!("GIT_NO_REPLACE_OBJECTS=1 eval '{fetch}'"),
        format!("GIT_NO_REPLACE_OBJECTS=1 sh -c '{fetch}'"),
        format!("env GIT_NO_REPLACE_OBJECTS=1 sh -c '{fetch}'"),
        format!("GIT_NO_REPLACE_OBJECTS=1 command {fetch}"),
        format!("GIT_NO_REPLACE_OBJECTS=1 exec {fetch}"),
        format!("GIT_NO_REPLACE_OBJECTS=1 nice {fetch}"),
        format!("GIT_NO_REPLACE_OBJECTS=1 time {fetch}"),
    ] {
        let out = scratch.judge("git-guard", &line);
        if !nested_refusal(&out) {
            wrong.push(format!("{line}: {}", stderr(&out)));
        }
    }
    git_guard_allows(&scratch, "sh -c 'git fetch origin'");
    git_guard_allows(
        &scratch,
        &format!(
            "GIT_NO_REPLACE_OBJECTS=1 eval 'git -C {root} fetch --no-recurse-submodules origin'"
        ),
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn git_guard_judges_uncertain_child_inheritance() {
    let reverse = Scratch::landed(None);
    replace_last_commit(&reverse, true);
    reverse.config("fixture.repos", &shell_path(reverse.root()));
    git_guard_refuses(
        &reverse,
        "GIT_NO_REPLACE_OBJECTS=1 git for-each-repo --config=fixture.repos fetch origin",
    );
    git_guard_allows(
        &reverse,
        "GIT_NO_REPLACE_OBJECTS=1 git for-each-repo --config=fixture.repos fetch --no-recurse-submodules origin",
    );
}

#[test]
fn git_guard_judges_the_environment_of_parent_callbacks() {
    let forward = Scratch::landed(None);
    definition_outside_work_tree(&forward, true);
    replace_last_commit(&forward, false);
    let root = shell_path(forward.root());
    let out = forward.judge(
        "git-guard",
        &format!("GIT_NO_REPLACE_OBJECTS=1 git -c core.pager='git -C {root} fetch origin' --paginate submodule status"),
    );
    assert!(unresolved_probe_refusal(&out), "{}", stderr(&out));
    let out = forward.judge("git-guard", &format!("GIT_NO_REPLACE_OBJECTS=1 git -c core.pager='git -C {root} fetch --no-recurse-submodules origin' --paginate submodule status"));
    assert!(
        out.status.code() == Some(2)
            && stderr(&out).contains("git.policy_authority")
            && stderr(&out)
                .contains("cannot determine the transport repository: the `env` option `-u`"),
        "{}",
        stderr(&out)
    );
    git_guard_allows(&forward, &format!("git -c core.pager='git -C {root} fetch --no-recurse-submodules origin' --paginate submodule status"));
}

#[test]
fn git_guard_keeps_the_earlier_environment_authority_refusal() {
    let scratch = Scratch::landed(None);
    let out = scratch.judge("git-guard", "GIT_NO_REPLACE_OBJECTS=1; GIT_REPLACE_REF_BASE=refs/none/; GIT_OBJECT_DIRECTORY=objects; GIT_ALTERNATE_OBJECT_DIRECTORIES=alternate; git fetch origin");
    assert!(
        out.status.code() == Some(2) && stderr(&out).contains("git.policy_authority"),
        "{}",
        stderr(&out)
    );
}

/// Through git-guard: line candidates are judged in recorded, absent and
/// inherited views. Any view that finds the definition refuses. A current
/// command's own assignment is certain; a carried prefix is not. The
/// earlier authority rule in exec-guard remains separate.
#[test]
fn git_guard_judges_an_earlier_assignment_in_every_environment_it_may_have() {
    // The last commit holds no definition; its replacement does. Git
    // follows the replacement unless the call's own environment turns it off.
    let reverse = Scratch::landed(None);
    replace_last_commit(&reverse, true);
    git_guard_refuses(&reverse, "git fetch origin");
    for line in [
        "GIT_NO_REPLACE_OBJECTS=1; git fetch origin",
        "readonly GIT_NO_REPLACE_OBJECTS=1; git fetch origin",
        "false && export GIT_NO_REPLACE_OBJECTS=1; git fetch origin",
        "export GIT_NO_REPLACE_OBJECTS=1; unset GIT_NO_REPLACE_OBJECTS; git fetch origin",
        "export GIT_NO_REPLACE_OBJECTS=1; git fetch origin",
        "GIT_REPLACE_REF_BASE=refs/none/; git fetch origin",
    ] {
        git_guard_refuses(&reverse, line);
    }
    // Controls: a prefix or a launcher is certain, so the call does not
    // follow the replacement and the last commit holds no definition.
    for line in [
        "GIT_NO_REPLACE_OBJECTS=1 git fetch origin",
        "env GIT_NO_REPLACE_OBJECTS=1 git fetch origin",
        "git fetch --no-recurse-submodules origin",
        "GIT_NO_REPLACE_OBJECTS=1; git fetch --no-recurse-submodules origin",
    ] {
        git_guard_allows(&reverse, line);
    }
    // The last commit holds a definition; its replacement does not. An
    // earlier assignment or export reaches the probe as one view.
    let forward = Scratch::landed(None);
    definition_outside_work_tree(&forward, true);
    replace_last_commit(&forward, false);
    git_guard_allows(&forward, "git fetch origin");
    for line in [
        "GIT_NO_REPLACE_OBJECTS=1; git fetch origin",
        "export GIT_NO_REPLACE_OBJECTS=1; git fetch origin",
        "GIT_NO_REPLACE_OBJECTS=1 git fetch origin",
    ] {
        git_guard_refuses(&forward, line);
    }
    // A runner child is judged with each known input kept and cleared.
    let repos = shell_path(forward.root());
    forward.config("fixture.repos", &repos);
    git_guard_refuses(
        &forward,
        "GIT_NO_REPLACE_OBJECTS=1 git for-each-repo --config=fixture.repos fetch origin",
    );
    git_guard_allows(
        &forward,
        "git for-each-repo --config=fixture.repos fetch origin",
    );
}

/// Through both guards: a command not proven local refuses.
#[test]
fn both_guards_refuse_a_command_that_sets_up_later_contacts() {
    let scratch = Scratch::landed(None);
    assert_unproven(
        &scratch,
        &["git maintenance start", "git maintenance register"],
        &["git maintenance unregister", "git maintenance stop"],
    );
}

/// Both guards judge a remote option in every spelling Git's option parser
/// accepts, attached or separate, as they judge its full spelling.
#[test]
fn both_guards_read_a_remote_option_in_every_spelling_git_accepts() {
    let scratch = Scratch::landed(None);
    assert_rows(
        &scratch,
        &[
            "git archive --remote=fixture::x HEAD",
            "git archive --rem=fixture::x HEAD",
            "git archive --r=fixture::x HEAD",
            "git archive --remote fixture::x HEAD",
            "git archive --rem fixture::x HEAD",
            "git archive --r fixture::x HEAD",
        ],
        &[
            "git archive HEAD",
            "git archive --remote=https://127.0.0.1:9/r HEAD",
            "git archive --rem=https://127.0.0.1:9/r HEAD",
            "git archive --rem https://127.0.0.1:9/r HEAD",
        ],
    );
    // A form the option table does not prove refuses.
    assert_unproven(
        &scratch,
        &["git archive --unlisted=1 HEAD", "git archive -z HEAD"],
        &[],
    );
}

/// The reason a refusal gives for an option reading the guards cannot prove.
const HIDDEN: &str = "an option word may be read as another option's value";

/// A refusal by this task's rule for an option reading the guards cannot
/// prove.
fn hidden_refusal(out: &Output) -> bool {
    let err = stderr(out);
    out.status.code() == Some(2) && err.contains("git.hook_integrity") && err.contains(HIDDEN)
}

/// Both guards refuse an option reading they cannot prove.
#[test]
fn both_guards_read_a_remote_option_word_a_value_may_consume() {
    let scratch = Scratch::landed(None);
    for hook in HOOKS {
        for command in [
            "git archive --format --remote=fixture::x HEAD",
            "git archive --prefix --remote fixture::x HEAD",
            "git archive -9o --remote=fixture::x HEAD",
            "git archive --out --remote=fixture::x HEAD",
        ] {
            let out = scratch.judge(hook, command);
            assert!(hidden_refusal(&out), "{hook} `{command}`: {}", stderr(&out));
        }
        // Controls: a provable reading is judged as usual.
        for command in [
            "git archive --format zip --remote=https://127.0.0.1:9/r HEAD",
            "git archive --prefix p/ --rem https://127.0.0.1:9/r HEAD",
            "git archive -9 -o out.tar --remote=https://127.0.0.1:9/r HEAD",
        ] {
            let out = scratch.judge(hook, command);
            assert_eq!(
                out.status.code(),
                Some(0),
                "{hook} `{command}`: {}",
                stderr(&out)
            );
        }
        let out = scratch.judge(hook, "git archive --format zip --remote=fixture::x HEAD");
        assert!(helper_refusal(&out), "{hook}: {}", stderr(&out));
    }
}

/// Both guards read the whole index, whatever directory the call runs in
/// and whichever way it selects a directory or an index file.
#[test]
fn both_guards_read_the_whole_index_from_every_directory() {
    let scratch = Scratch::landed(None);
    scratch.write("src/deep/a.txt", b"a\n");
    let head = record_nested(&scratch);
    let src = scratch.root().join("src");
    let deep = src.join("deep");
    for hook in HOOKS {
        for (cwd, refused, allowed) in [
            (
                scratch.root(),
                "git fetch origin",
                "git fetch --no-recurse-submodules origin",
            ),
            (
                src.as_path(),
                "git fetch origin",
                "git fetch --no-recurse-submodules origin",
            ),
            (
                deep.as_path(),
                "git fetch origin",
                "git fetch --no-recurse-submodules origin",
            ),
            (
                scratch.root(),
                "git -C src fetch origin",
                "git -C src fetch --no-recurse-submodules origin",
            ),
            (
                scratch.root(),
                "git -C src/deep fetch --recurse-submodules origin",
                "git -C src/deep fetch --no-recurse-submodules origin",
            ),
            (
                src.as_path(),
                "git -C deep pull origin main",
                "git -C deep pull --no-recurse-submodules origin main",
            ),
        ] {
            let out = Scratch::judge_in(hook, refused, cwd);
            assert!(
                unproven_refusal(&out),
                "{hook} `{refused}` in {}: {}",
                cwd.display(),
                stderr(&out)
            );
            let out = Scratch::judge_in(hook, allowed, cwd);
            assert_eq!(
                out.status.code(),
                Some(0),
                "{hook} `{allowed}` in {}: {}",
                cwd.display(),
                stderr(&out)
            );
        }
    }
    // Controls: with no record in the index, each directory passes.
    let plain = Scratch::landed(None);
    plain.write("src/deep/a.txt", b"a\n");
    for hook in HOOKS {
        for cwd in [plain.root().to_path_buf(), plain.root().join("src")] {
            let out = Scratch::judge_in(hook, "git fetch origin", &cwd);
            assert_eq!(out.status.code(), Some(0), "{hook}: {}", stderr(&out));
        }
    }
    // An index file the environment selects is the one read, from a
    // directory below the root too.
    let selected = Scratch::landed(None);
    selected.write("src/a.txt", b"a\n");
    let index = selected.base.join("sibling-index");
    let mut other = git2::Index::open(&index).unwrap();
    other
        .add(&git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: 0o160_000,
            uid: 0,
            gid: 0,
            file_size: 0,
            id: git2::Oid::from_str(&head).unwrap(),
            flags: 3,
            flags_extended: 0,
            path: b"lib".to_vec(),
        })
        .unwrap();
    other.write().unwrap();
    let index = shell_path(&index);
    let selected_src = selected.root().join("src");
    for hook in HOOKS {
        let out = run_with_stdin(
            codeflow()
                .args(["hook", hook, "--contract", "3"])
                .env("GIT_INDEX_FILE", &index)
                .current_dir(&selected_src),
            &guard_payload("git fetch origin", &selected_src),
        );
        assert!(nested_refusal(&out), "{hook}: {}", stderr(&out));
    }
}

/// The guards read the whole index under any pathspec setting.
#[test]
fn both_guards_read_the_whole_index_under_pathspec_settings() {
    let scratch = Scratch::landed(None);
    scratch.write("src/a.txt", b"a\n");
    record_nested(&scratch);
    for hook in HOOKS {
        for setting in [
            "GIT_LITERAL_PATHSPECS",
            "GIT_GLOB_PATHSPECS",
            "GIT_NOGLOB_PATHSPECS",
            "GIT_ICASE_PATHSPECS",
        ] {
            let out = scratch.judge_inheriting(hook, "git fetch origin", (setting, "1"));
            assert!(unproven_refusal(&out), "{hook} {setting}: {}", stderr(&out));
            // The same setting given on the judged line reads the same.
            let line = format!("{setting}=1 git fetch origin");
            let out = scratch.judge(hook, &line);
            assert!(unproven_refusal(&out), "{hook} `{line}`: {}", stderr(&out));
        }
        // Each setting as a top-level option reads the same.
        for option in [
            "--literal-pathspecs",
            "--glob-pathspecs",
            "--noglob-pathspecs",
            "--icase-pathspecs",
        ] {
            let line = format!("git {option} fetch origin");
            let out = scratch.judge(hook, &line);
            if hook == "git-guard" && option == "--icase-pathspecs" {
                // git-guard's earlier rule refuses this option on its own
                // terms, before it reads what Git runs.
                let err = stderr(&out);
                assert!(
                    out.status.code() == Some(2) && err.contains("case-insensitive"),
                    "{hook} `{line}`: {err}"
                );
                continue;
            }
            assert!(unproven_refusal(&out), "{hook} `{line}`: {}", stderr(&out));
        }
        // Controls: the same line with recursion turned off passes.
        for line in [
            "GIT_LITERAL_PATHSPECS=1 git fetch --no-recurse-submodules origin",
            "git --glob-pathspecs fetch --no-recurse-submodules origin",
        ] {
            let out = scratch.judge(hook, line);
            assert_eq!(
                out.status.code(),
                Some(0),
                "{hook} `{line}`: {}",
                stderr(&out)
            );
        }
    }
}
