//! TSK-1489: a guard refusal about a remote's transport never prints a
//! configured remote's name, a setting's subsection or a word's attached
//! value; it names the kind of remote, setting or option instead.
//!
//! Every row is a hook payload judged by the built binary in a scratch
//! repository holding a synthetic token; nothing contacts a remote or
//! starts a helper.
use super::{codeflow, enforced_repo, git, guard_payload, run_with_stdin, shell_path};
use std::path::{Path, PathBuf};

const TOKEN: &str = "OPAQUECANARY";
const HOOKS: [&str; 2] = ["git-guard", "exec-guard"];

/// A repository on `main` whose `origin`, a local bare copy, is fetched
/// with `origin/HEAD` set, as the remote-transport rows build one, with
/// `config` appended to its own configuration file.
fn landed(config: &[u8]) -> (tempfile::TempDir, PathBuf) {
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
    let file = root.join(".git/config");
    let mut bytes = std::fs::read(&file).unwrap();
    bytes.extend_from_slice(config);
    std::fs::write(&file, bytes).unwrap();
    (temp, root)
}

/// Judge `command` with `hook` in `root`; the exit code and everything
/// printed.
fn judge(root: &Path, hook: &str, command: &str) -> (Option<i32>, String) {
    let out = run_with_stdin(
        codeflow()
            .args(["hook", hook, "--contract", "3"])
            .current_dir(root),
        &guard_payload(command, root),
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), text)
}

/// Assert that `command` refuses under each of `hooks` in `root`, printing
/// `class` and none of `withheld`.
fn refuses_withholding(root: &Path, hooks: &[&str], command: &str, class: &str, withheld: &[&str]) {
    for hook in hooks {
        let (code, text) = judge(root, hook, command);
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
}

#[test]
fn withholding_case_1() {
    let (_temp, root) =
        landed(b"[url \"https://user:OPAQUECANARY@host.invalid/\"]\n\tinsteadOf = \xff\n");
    refuses_withholding(
        &root,
        &HOOKS,
        "git status",
        "a `url.*.insteadof` setting is not UTF-8",
        &[TOKEN, "host.invalid"],
    );
}

#[test]
fn withholding_case_2() {
    let (_temp, root) =
        landed(b"[http \"https://user:OPAQUECANARY@host.invalid/\"]\n\textraHeader = \xff\n");
    for hook in HOOKS {
        let (_, text) = judge(&root, hook, "git status");
        assert!(!text.contains(TOKEN), "{hook} prints the token:\n{text}");
        assert!(
            !text.contains("host.invalid"),
            "{hook} prints the address:\n{text}"
        );
    }
}

#[test]
fn withholding_case_3() {
    let (_temp, root) = landed(b"[remote \"fork-OPAQUECANARY\"]\n\turl = https://user:OPAQUECANARY@host.invalid/r\n\tpromisor = true\n\tvcs = fixture\n");
    refuses_withholding(
        &root,
        &HOOKS,
        "git status",
        "a configured remote selects a helper through its configuration",
        &[TOKEN, "fork-", "host.invalid"],
    );
}

#[test]
fn withholding_case_4() {
    let (_temp, root) = landed(b"[remote \"fork-OPAQUECANARY\"]\n\turl = https://user:OPAQUECANARY@host.invalid/r\n\tskipDefaultUpdate = maybe\n");
    refuses_withholding(
        &root,
        &["exec-guard"],
        "git remote update",
        "skip a configured remote cannot be read",
        &[TOKEN, "fork-", "host.invalid"],
    );
}

/// Judge an operand-free `git fetch` on a current branch whose settings
/// end with `values`.
fn branch_row(values: &[u8]) {
    let mut config = b"[branch \"OPAQUECANARY\"]\n".to_vec();
    config.extend_from_slice(values);
    let (_temp, root) = landed(&config);
    git(&root, &["checkout", "-q", "-b", TOKEN]);
    refuses_withholding(
        &root,
        &HOOKS,
        "git fetch",
        "the remote of the current branch has more than one value or an empty one",
        &[TOKEN],
    );
}

#[test]
fn withholding_case_5() {
    branch_row(b"\tremote = origin\n\tremote = origin\n");
}

#[test]
fn withholding_case_6() {
    branch_row(b"\tremote =\n");
}

#[test]
fn withholding_case_7() {
    let (_temp, root) = landed(b"");
    refuses_withholding(
        &root,
        &HOOKS,
        "git archive --unlisted=OPAQUECANARY HEAD",
        "`archive` with an option the guard does not read",
        &[TOKEN],
    );
}

#[test]
fn withholding_case_8() {
    let (_temp, root) = landed(b"");
    let command = "git remote add -f --track=OPAQUECANARY fresh https://example.invalid/r";
    // The git guard refuses this command by its remote authority rule first.
    let (code, text) = judge(&root, "git-guard", command);
    assert_eq!(code, Some(2), "git-guard must refuse:\n{text}");
    assert!(!text.contains(TOKEN), "git-guard prints the token:\n{text}");
    refuses_withholding(
        &root,
        &["exec-guard"],
        command,
        "adding a remote with an option the guard does not read",
        &[TOKEN],
    );
}
