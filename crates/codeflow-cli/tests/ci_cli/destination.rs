//! Destination lookup failures have one rule, independent of lifecycle checks.
use super::{codeflow, combined, git, repo_with_range};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Output;

struct Stub {
    dir: tempfile::TempDir,
    path: std::ffi::OsString,
    log: PathBuf,
    remote: PathBuf,
}

impl Stub {
    fn new(root: &Path, response: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let real = std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        let real = String::from_utf8(real.stdout).unwrap();
        let remote = dir.path().join("remote.git");
        git(
            root,
            &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
        );
        git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(root, &["push", "-q", "origin", "main"]);
        let log = dir.path().join("attempts");
        let script = format!(
            "#!/bin/sh\nfor arg do\n if [ \"$arg\" = ls-remote ]; then\n  echo attempt >> '{}'\n  {response}\n fi\ndone\nexec '{}' \"$@\"\n",
            log.display(), real.trim()
        );
        let shim = dir.path().join("git");
        std::fs::write(&shim, script).unwrap();
        std::fs::set_permissions(shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths(
            std::iter::once(dir.path().to_path_buf())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        // An existing local default must never replace the stub's answer.
        git(root, &["update-ref", "refs/remotes/origin/main", "main"]);
        git(
            root,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        Self {
            dir,
            path,
            log,
            remote,
        }
    }

    fn attempts(&self) -> usize {
        std::fs::read_to_string(&self.log).unwrap().lines().count()
    }

    fn ci(&self, root: &Path) -> Output {
        self.ci_to(root, self.remote.to_str().unwrap())
    }

    fn ci_to(&self, root: &Path, url: &str) -> Output {
        codeflow()
            .current_dir(root)
            .env("PATH", &self.path)
            .args([
                "ci",
                "--base",
                "main",
                "--head",
                "HEAD",
                "--branch",
                "feat/x",
                "--destination",
                url,
            ])
            .output()
            .unwrap()
    }
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "docs");
    std::fs::create_dir_all(dir.path().join("project-management/tasks")).unwrap();
    std::fs::write(
        dir.path().join("project-management/tasks/TSK-001.md"),
        super::planned_task("TSK-001", ""),
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    std::fs::write(
        dir.path().join(".codeflow/policy.json"),
        "{\"schema_version\":1}\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "docs: track work"]);
    git(dir.path(), &["branch", "-f", "main", "HEAD"]);
    std::fs::write(dir.path().join("guide.md"), "changed\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "docs: update guide"]);
    dir
}

fn destination_refusal(out: &Output, cause: &str) {
    let text = combined(out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(
        text.matches("rule ci.destination_unavailable").count(),
        1,
        "{text}"
    );
    assert!(text.contains(cause), "{text}");
    assert!(!text.contains("invariant work.criteria_frozen"), "{text}");
    assert!(!text.contains("invariant work.records"), "{text}");
    assert!(!text.contains("planning PR"), "{text}");
    assert!(
        text.contains("network") && text.contains("remote"),
        "{text}"
    );
}

#[test]
fn authentication_failure_is_one_destination_refusal_without_retry() {
    let dir = fixture();
    let stub = Stub::new(
        dir.path(),
        "echo 'fatal: Authentication failed: private-diagnostic-marker' >&2; exit 128",
    );
    let out = stub.ci(dir.path());
    destination_refusal(&out, "authentication failed");
    assert!(!combined(&out).contains("private-diagnostic-marker"));
    assert_eq!(stub.attempts(), 1);
    destination_refusal(&stub.pre_push(dir.path()), "authentication failed");
    assert_eq!(stub.attempts(), 2);
}

#[test]
fn malformed_advertisement_is_one_destination_refusal_without_retry() {
    let dir = fixture();
    let stub = Stub::new(dir.path(), "echo malformed; exit 0");
    destination_refusal(&stub.ci(dir.path()), "malformed");
    assert_eq!(stub.attempts(), 1);
    destination_refusal(&stub.pre_push(dir.path()), "malformed");
    assert_eq!(stub.attempts(), 2);
}

#[test]
fn missing_default_is_one_destination_refusal_without_retry() {
    let dir = fixture();
    let stub = Stub::new(
        dir.path(),
        "printf '1111111111111111111111111111111111111111\\trefs/heads/main\\n'; exit 0",
    );
    destination_refusal(&stub.ci(dir.path()), "does not name a default branch");
    assert_eq!(stub.attempts(), 1);
    destination_refusal(&stub.pre_push(dir.path()), "does not name a default branch");
    assert_eq!(stub.attempts(), 2);
}

#[test]
fn timeout_is_one_destination_refusal_and_ignores_stale_local_default() {
    let dir = fixture();
    let stub = Stub::new(dir.path(), "sleep 60");
    git(
        dir.path(),
        &["update-ref", "refs/remotes/origin/old-default", "main"],
    );
    git(
        dir.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/old-default",
        ],
    );
    destination_refusal(&stub.ci(dir.path()), "timeout");
    assert_eq!(stub.attempts(), 3);
}

impl Stub {
    fn pre_push(&self, root: &Path) -> Output {
        self.pre_push_to(root, self.remote.to_str().unwrap())
    }

    fn pre_push_to(&self, root: &Path, url: &str) -> Output {
        use std::io::Write as _;
        use std::process::Stdio;
        let out = std::process::Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let head = String::from_utf8(out.stdout).unwrap();
        let head = head.trim();
        let refs = format!("refs/heads/feat/x {head} refs/heads/feat/one {zeros}\nrefs/heads/feat/x {head} refs/heads/feat/two {zeros}\n", zeros = "0".repeat(40));
        let mut child = codeflow()
            .current_dir(root)
            .env("PATH", &self.path)
            .args(["git-hook", "pre-push", "origin", url])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(refs.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

#[test]
fn two_pushed_refs_share_a_first_attempt_success() {
    let dir = fixture();
    let stub = Stub::new(dir.path(), ":");
    let out = stub.pre_push(dir.path());
    assert!(out.status.success(), "{}", combined(&out));
    assert_eq!(stub.attempts(), 1);
    assert_eq!(combined(&out).matches("`codeflow ci` judges ").count(), 2);
}

#[test]
fn two_pushed_refs_share_timeout_then_success() {
    let dir = fixture();
    let stub = Stub::new(
        dir.path(),
        "if [ ! -f \"$0.once\" ]; then touch \"$0.once\"; sleep 60; fi",
    );
    let out = stub.pre_push(dir.path());
    assert!(out.status.success(), "{}", combined(&out));
    assert_eq!(stub.attempts(), 2);
    assert_eq!(combined(&out).matches("`codeflow ci` judges ").count(), 2);
}

#[test]
fn two_pushed_refs_share_exhausted_timeout() {
    let dir = fixture();
    let stub = Stub::new(dir.path(), "sleep 60");
    let out = stub.pre_push(dir.path());
    let text = combined(&out);
    assert!(!out.status.success(), "{text}");
    assert_eq!(
        text.matches("ci.destination_unavailable").count(),
        1,
        "{text}"
    );
    assert!(!text.contains("work.criteria_frozen"), "{text}");
    assert!(!text.contains("work.records"), "{text}");
    assert_eq!(stub.attempts(), 3);
}

#[test]
fn installed_pre_push_refuses_timeout_then_allows_the_same_task_push() {
    let dir = fixture();
    git(dir.path(), &["switch", "-c", "task/TSK-001-work"]);
    let stub = Stub::new(dir.path(), ":");
    // Real git prepends its own executable directory to PATH when it runs
    // hooks. Stub SSH instead, keeping the real Git advertisement protocol.
    let ssh = stub.dir.path().join("ssh");
    std::fs::write(&ssh, format!(
        "#!/bin/sh\ncase \"$*\" in\n *git-upload-pack*) echo attempt >> '{}'; if [ ! -f \"$0.answer\" ]; then sleep 60; fi; exec git upload-pack '{}' ;;\n *git-receive-pack*) exec git receive-pack '{}' ;;\n *) exit 1 ;;\nesac\n",
        stub.log.display(), stub.remote.display(), stub.remote.display()
    )).unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(
        dir.path(),
        &[
            "remote",
            "set-url",
            "origin",
            "ssh://example.test/destination",
        ],
    );
    let hook = dir.path().join(".git/hooks/pre-push");
    std::fs::write(
        &hook,
        include_bytes!("../../../../assets/base/git-hooks/pre-push"),
    )
    .unwrap();
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let push = || {
        let mut git = std::process::Command::new("git");
        git.env("CODEFLOW_HOME", super::isolated_home())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("CODEFLOW_PR_BODY")
            .env_remove("GITHUB_EVENT_NAME")
            .env_remove("GITHUB_HEAD_REF")
            .env_remove("GITHUB_BASE_REF")
            .env_remove("CI_PIPELINE_SOURCE")
            .env_remove("CI_MERGE_REQUEST_IID")
            .env_remove("CI_MERGE_REQUEST_TARGET_BRANCH_NAME")
            .env_remove("BITBUCKET_PR_ID")
            .env_remove("BITBUCKET_PR_DESTINATION_BRANCH")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .current_dir(dir.path())
            .env("GIT_SSH_COMMAND", &ssh)
            .env("GIT_SSH_VARIANT", "ssh")
            .env("CODEFLOW_HOOK_BINARY", env!("CARGO_BIN_EXE_codeflow"))
            .args(["push", "origin", "task/TSK-001-work"])
            .output()
            .unwrap()
    };
    let refused = push();
    let text = combined(&refused);
    println!("timeout journey:\n{text}");
    assert!(!refused.status.success(), "{text}");
    assert_eq!(
        text.matches("ci.destination_unavailable").count(),
        1,
        "{text}"
    );
    assert_eq!(stub.attempts(), 3);
    std::fs::write(stub.dir.path().join("ssh.answer"), "answer").unwrap();
    let accepted = push();
    println!("answering journey:\n{}", combined(&accepted));
    assert!(accepted.status.success(), "{}", combined(&accepted));
    assert_eq!(stub.attempts(), 4);
}

/// A synthetic credential: it must never reach any output.
const TOKEN: &str = "FAKE_TOKEN_5d2e81";

fn credential_urls(root: &Path) -> String {
    git(
        root,
        &[
            "config",
            "remote.upstream.url",
            &format!("https://{TOKEN}@upstream.invalid/y.git"),
        ],
    );
    format!("https://u:{TOKEN}@h.invalid/x.git")
}

#[test]
fn a_credential_in_the_destination_url_is_never_printed_in_a_refusal() {
    let dir = fixture();
    let stub = Stub::new(dir.path(), "echo malformed; exit 0");
    let url = credential_urls(dir.path());
    for out in [
        stub.ci_to(dir.path(), &url),
        stub.pre_push_to(dir.path(), &url),
    ] {
        let text = combined(&out);
        destination_refusal(&out, "https://***@h.invalid/x.git");
        assert_eq!(text.matches(TOKEN).count(), 0, "{text}");
    }
}

#[test]
fn a_credential_in_the_destination_url_is_never_printed_in_a_judged_push() {
    let dir = fixture();
    // Answer every lookup with the real advertisement of the stub's remote.
    let stub = Stub::new(
        dir.path(),
        "PATH=${PATH#*:} git ls-remote --symref \"$(dirname \"$0\")/remote.git\" HEAD 'refs/heads/*' 'refs/tags/*'; exit $?",
    );
    let url = credential_urls(dir.path());
    let out = stub.pre_push_to(dir.path(), &url);
    let text = combined(&out);
    println!("judged push:\n{text}");
    assert!(out.status.success(), "{text}");
    assert_eq!(text.matches("`codeflow ci` judges ").count(), 2, "{text}");
    assert!(
        text.contains("the push goes to https://***@h.invalid/x.git, not the configured upstream https://***@upstream.invalid/y.git"),
        "{text}"
    );
    assert_eq!(text.matches(TOKEN).count(), 0, "{text}");

    // A failing child check prints the command to rerun, destination included.
    std::fs::write(dir.path().join("guide.md"), "again\n").unwrap();
    git(
        dir.path(),
        &["commit", "-qam", "not a conventional subject"],
    );
    let out = stub.pre_push_to(dir.path(), &url);
    let text = combined(&out);
    println!("refused push:\n{text}");
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("--destination https://***@h.invalid/x.git"),
        "{text}"
    );
    assert_eq!(text.matches(TOKEN).count(), 0, "{text}");
}
