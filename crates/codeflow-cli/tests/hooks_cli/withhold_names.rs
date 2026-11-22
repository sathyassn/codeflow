//! TSK-1489: a guard refusal about a remote's transport never prints a
//! configured remote's name or a setting's subsection, since either may
//! carry a credential; it names the kind of remote or setting instead.
//!
//! Every row is a hook payload judged by the built binary in a scratch
//! repository whose configuration holds a synthetic credential; nothing
//! contacts a remote or starts a helper.
use super::{codeflow, enforced_repo, git, guard_payload, run_with_stdin, shell_path};
use std::path::PathBuf;

const TOKEN: &str = "OPAQUECANARY";

/// A repository on `main` whose `origin`, a local bare copy, is fetched
/// with `origin/HEAD` set, as the remote-transport rows build one.
fn landed() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let base = codeflow_core::portable_path::canonicalize(temp.path()).unwrap();
    let root = base.join("repo");
    std::fs::create_dir(&root).unwrap();
    enforced_repo(&root);
    let origin = base.join("origin.git");
    super::isolate_clone(codeflow_fixture::clone(&base, &root, &origin))
        .bare()
        .run();
    git(&root, &["remote", "add", "origin", &shell_path(&origin)]);
    git(&root, &["fetch", "-q", "origin"]);
    git(&root, &["remote", "set-head", "origin", "main"]);
    (temp, root)
}

/// Append `config` to a fresh repository's own configuration file, judge
/// `command` with `hook`, and return the exit code and everything printed.
fn judge(config: &[u8], hook: &str, command: &str) -> (Option<i32>, String) {
    let (_temp, root) = landed();
    let file = root.join(".git/config");
    let mut bytes = std::fs::read(&file).unwrap();
    bytes.extend_from_slice(config);
    std::fs::write(&file, bytes).unwrap();
    let out = run_with_stdin(
        codeflow()
            .args(["hook", hook, "--contract", "3"])
            .current_dir(&root),
        &guard_payload(command, &root),
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), text)
}

/// Assert that `command` refuses under `hook`, printing `class` and none of
/// `withheld`.
fn refuses_withholding(config: &[u8], hook: &str, command: &str, class: &str, withheld: &[&str]) {
    let (code, text) = judge(config, hook, command);
    assert_eq!(code, Some(2), "{hook} `{command}` must refuse:\n{text}");
    for word in withheld {
        assert!(
            !text.contains(word),
            "{hook} `{command}` prints `{word}`:\n{text}"
        );
    }
    assert!(
        text.contains(class),
        "{hook} `{command}` must print `{class}`:\n{text}"
    );
}

#[test]
fn an_unreadable_rewrite_value_withholds_the_rewrite_address() {
    let config: &[u8] = b"[url \"https://user:OPAQUECANARY@host.invalid/\"]\n\tinsteadOf = \xff\n";
    for hook in ["git-guard", "exec-guard"] {
        refuses_withholding(
            config,
            hook,
            "git status",
            "a `url.*.insteadof` setting is not UTF-8",
            &[TOKEN, "host.invalid"],
        );
    }
}

#[test]
fn an_unreadable_header_value_prints_no_address() {
    let config: &[u8] =
        b"[http \"https://user:OPAQUECANARY@host.invalid/\"]\n\textraHeader = \xff\n";
    for hook in ["git-guard", "exec-guard"] {
        let (_, text) = judge(config, hook, "git status");
        assert!(!text.contains(TOKEN), "{hook} prints the token:\n{text}");
        assert!(
            !text.contains("host.invalid"),
            "{hook} prints the address:\n{text}"
        );
    }
}

#[test]
fn a_promisor_remote_with_a_helper_setting_withholds_its_name() {
    let config: &[u8] = b"[remote \"fork-OPAQUECANARY\"]\n\turl = https://user:OPAQUECANARY@host.invalid/r\n\tpromisor = true\n\tvcs = fixture\n";
    for hook in ["git-guard", "exec-guard"] {
        refuses_withholding(
            config,
            hook,
            "git status",
            "a configured remote selects a helper through its configuration",
            &[TOKEN, "fork-", "host.invalid"],
        );
    }
}

#[test]
fn an_unreadable_skip_setting_withholds_the_remote_name() {
    let config: &[u8] = b"[remote \"fork-OPAQUECANARY\"]\n\turl = https://user:OPAQUECANARY@host.invalid/r\n\tskipDefaultUpdate = maybe\n";
    refuses_withholding(
        config,
        "exec-guard",
        "git remote update",
        "skip a configured remote cannot be read",
        &[TOKEN, "fork-", "host.invalid"],
    );
}
