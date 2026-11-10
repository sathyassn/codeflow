//! `codeflow scan` (TSK-1176 AC-6 to AC-8, moved from TSK-430): the hosted
//! secret scan and dependency audit verdicts over `base..head`, read from
//! the base.
//!
//! Tests that run the official pinned binaries read their folders from
//! `CODEFLOW_TEST_GITLEAKS_DIR` and `CODEFLOW_TEST_OSV_DIR`; without them
//! those tests check nothing. The level and binary-state tests use fake
//! binaries, which never match a pin and so are never run.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/git_isolation.rs"]
mod git_isolation;

fn git(dir: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    git_isolation::strip_host_git_state(&mut cmd);
    let out = cmd
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit_all(dir: &Path, message: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "--no-verify", "-m", message]);
    git(dir, &["rev-parse", "HEAD"])
}

/// A repository on `main` with a policy at the given levels, and a task
/// branch checked out from it.
fn repo(policy: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    std::fs::write(dir.path().join(".codeflow/policy.json"), policy).unwrap();
    std::fs::write(dir.path().join("README.md"), "base\n").unwrap();
    commit_all(dir.path(), "chore: init");
    git(dir.path(), &["checkout", "-q", "-b", "feat/x"]);
    dir
}

const POLICY: &str = r#"{"schema_version":1,"git":{"secret_scan":"block","security_review":"warn","dep_audit":"off"}}"#;

/// A line gitleaks' `generic-api-key` rule matches: a quoted path and 64
/// hex digits, built at run time so this file holds no such line.
fn leak_line() -> String {
    let key = format!(
        "{}{}",
        "9f2c4e1ab7d03586c2e9f41a7b8d5e3c", "06a1f9b2d7c4e8a35f1b6d0c9e2a7f48"
    );
    format!("{{\"src/auth/token.rs\": \"{key}\"}}\n")
}

/// `PATH` with `bin` first, then the system folders that hold git.
fn path_with(bin: &Path) -> String {
    format!("{}:/usr/bin:/bin", bin.display())
}

fn scan(dir: &Path, path: &str, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    git_isolation::strip_host_git_state(&mut cmd);
    cmd.arg("scan")
        .args(args)
        .current_dir(dir)
        .env("PATH", path)
        .env("CODEFLOW_HOME", dir.join(".codeflow-home"))
        .env_remove("GITLEAKS_CONFIG")
        .env_remove("GITLEAKS_CONFIG_TOML")
        .output()
        .unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A folder holding a fake `gitleaks` and `osv-scanner`, which match no
/// pin, and write `ran` beside themselves if anything runs them.
#[cfg(unix)]
fn fakes() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    for name in ["gitleaks", "osv-scanner"] {
        let script = bin.path().join(name);
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\ntouch '{}'\necho 8.30.1\n",
                bin.path().join("ran").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

fn pinned(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

/// AC-6, AC-1 (local): a secret a commit adds and a later commit deletes
/// is found, and a `.gitleaksignore` the range itself commits does not
/// exempt it: the exemptions come from the base. Once the base holds the
/// fingerprint, the same range is clean. A clean range passes.
#[cfg(unix)]
#[test]
fn the_scan_reads_every_commit_and_only_the_bases_exemptions() {
    let Some(gitleaks) = pinned("CODEFLOW_TEST_GITLEAKS_DIR") else {
        return;
    };
    let path = path_with(&gitleaks);
    let dir = repo(POLICY);
    let root = dir.path();

    std::fs::write(root.join("notes.txt"), "plain text\n").unwrap();
    commit_all(root, "docs: notes");
    let out = scan(root, &path, &["--base", "main", "--only", "secrets"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(text(&out).contains("no leaks found"), "{}", text(&out));

    std::fs::write(root.join("leak.json"), leak_line()).unwrap();
    let leaked = commit_all(root, "feat: add settings");
    std::fs::remove_file(root.join("leak.json")).unwrap();
    commit_all(root, "fix: drop settings");
    // The range's own exemption, in the working tree and committed.
    let fingerprint = format!("{leaked}:leak.json:generic-api-key:1\n");
    std::fs::write(root.join(".gitleaksignore"), &fingerprint).unwrap();
    commit_all(root, "chore: exempt");
    let out = scan(root, &path, &["--base", "main", "--only", "secrets"]);
    let shown = text(&out);
    assert_eq!(out.status.code(), Some(1), "{shown}");
    assert!(
        shown.contains("gitleaks generic-api-key at leak.json:1"),
        "{shown}"
    );
    assert!(shown.contains(&leaked[..12]), "{shown}");
    assert!(
        !shown.contains("9f2c4e1ab7d0"),
        "the secret is never printed: {shown}"
    );

    // The base takes the reviewed fingerprint first; the range is clean.
    let head = git(root, &["rev-parse", "HEAD"]);
    git(root, &["checkout", "-q", "main"]);
    std::fs::write(root.join(".gitleaksignore"), &fingerprint).unwrap();
    commit_all(root, "chore: land the exemption");
    git(root, &["checkout", "-q", &head]);
    let out = scan(root, &path, &["--base", "main", "--only", "secrets"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
}

/// AC-2 (local): a replace ref that swaps the leaking commit for a clean
/// one does not hide the secret, because every read ignores replace refs.
#[cfg(unix)]
#[test]
fn a_replace_ref_cannot_hide_a_planted_secret() {
    let Some(gitleaks) = pinned("CODEFLOW_TEST_GITLEAKS_DIR") else {
        return;
    };
    let path = path_with(&gitleaks);
    let dir = repo(POLICY);
    let root = dir.path();
    std::fs::write(root.join("leak.json"), leak_line()).unwrap();
    let leaked = commit_all(root, "feat: add settings");
    git(root, &["checkout", "-q", "-b", "decoy", "main"]);
    std::fs::write(root.join("leak.json"), "{}\n").unwrap();
    let decoy = commit_all(root, "feat: add settings");
    git(root, &["checkout", "-q", "feat/x"]);
    git(root, &["replace", &leaked, &decoy]);
    assert_eq!(git(root, &["show", "HEAD:leak.json"]), "{}");
    let out = scan(root, &path, &["--base", "main", "--only", "secrets"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("generic-api-key"), "{}", text(&out));
}

/// The preflight refuses an octopus merge in the range, whose additions
/// git cannot show, as the hosted job refuses one in a pull request.
#[cfg(unix)]
#[test]
fn an_octopus_merge_in_the_range_is_refused() {
    let Some(gitleaks) = pinned("CODEFLOW_TEST_GITLEAKS_DIR") else {
        return;
    };
    let path = path_with(&gitleaks);
    let dir = repo(POLICY);
    let root = dir.path();
    for branch in ["a", "b"] {
        git(root, &["checkout", "-q", "-b", branch, "main"]);
        std::fs::write(root.join(format!("{branch}.txt")), "x\n").unwrap();
        commit_all(root, "feat: side");
    }
    git(root, &["checkout", "-q", "feat/x"]);
    std::fs::write(root.join("c.txt"), "x\n").unwrap();
    commit_all(root, "feat: own");
    git(root, &["merge", "-q", "--no-edit", "--no-verify", "a", "b"]);
    let out = scan(root, &path, &["--base", "main", "--only", "secrets"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("octopus merge"), "{}", text(&out));
}

/// AC-7: the secret scan follows `git.secret_scan` at the base. At block a
/// missing or unpinned gitleaks fails closed and is never run; at warn it is
/// reported and the command passes; at off nothing is looked for.
#[cfg(unix)]
#[test]
fn the_secret_scan_follows_its_level_and_fails_closed_at_block() {
    let fake = fakes();
    let empty = tempfile::tempdir().unwrap();
    for (level, code) in [("block", 1), ("warn", 0), ("off", 0)] {
        let dir = repo(&format!(
            r#"{{"schema_version":1,"git":{{"secret_scan":"{level}","security_review":"off","dep_audit":"off"}}}}"#
        ));
        std::fs::write(dir.path().join("n.txt"), "x\n").unwrap();
        commit_all(dir.path(), "docs: n");
        for (bin, state) in [
            (empty.path(), "is not on PATH"),
            (fake.path(), "not the pinned binary"),
        ] {
            let out = scan(dir.path(), &path_with(bin), &["--base", "main"]);
            let shown = text(&out);
            assert_eq!(out.status.code(), Some(code), "{level} {state}: {shown}");
            if level == "off" {
                assert!(shown.contains("skipped"), "{shown}");
            } else {
                assert!(shown.contains(state), "{level}: {shown}");
                assert!(shown.contains("gitleaks 8.30.1"), "{shown}");
            }
        }
    }
    assert!(
        !fake.path().join("ran").exists(),
        "an unpinned binary never runs"
    );
}

/// AC-7: the dependency audit follows `git.security_review` and
/// `git.dep_audit` at the base: either at block blocks, both off skip, and a
/// missing key fails closed as the hosted step does. A missing or unpinned
/// osv-scanner fails closed at block and only warns at warn.
#[cfg(unix)]
#[test]
fn the_dependency_audit_follows_its_levels_and_fails_closed_at_block() {
    let fake = fakes();
    let empty = tempfile::tempdir().unwrap();
    let cases = [
        (
            r#""security_review":"warn","dep_audit":"block""#,
            1,
            "is not on PATH",
        ),
        (
            r#""security_review":"warn","dep_audit":"warn""#,
            0,
            "is not on PATH",
        ),
        (r#""security_review":"off","dep_audit":"off""#, 0, "skipped"),
        (
            r#""security_review":"warn""#,
            1,
            "missing or not block, warn or off",
        ),
    ];
    for (keys, code, says) in cases {
        let dir = repo(&format!(r#"{{"schema_version":1,"git":{{{keys}}}}}"#));
        std::fs::write(dir.path().join("n.txt"), "x\n").unwrap();
        commit_all(dir.path(), "docs: n");
        let out = scan(
            dir.path(),
            &path_with(empty.path()),
            &["--base", "main", "--only", "deps"],
        );
        assert_eq!(out.status.code(), Some(code), "{keys}: {}", text(&out));
        assert!(text(&out).contains(says), "{keys}: {}", text(&out));
    }
    let dir = repo(r#"{"schema_version":1,"git":{"security_review":"block","dep_audit":"warn"}}"#);
    let out = scan(
        dir.path(),
        &path_with(fake.path()),
        &["--base", "main", "--only", "deps"],
    );
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(
        text(&out).contains("not the pinned binary"),
        "{}",
        text(&out)
    );
    assert!(text(&out).contains("osv-scanner 2.4.0"), "{}", text(&out));
    assert!(
        !fake.path().join("ran").exists(),
        "an unpinned binary never runs"
    );
}

/// AC-6: the audit reads the head's lockfile with the base's suppressions.
/// A suppression the range commits does not clear its own advisory; once
/// the base holds it, the advisory is suppressed. Needs the network
/// (osv-scanner asks OSV.dev).
#[cfg(unix)]
#[test]
fn the_audit_takes_suppressions_from_the_base_only() {
    let Some(osv) = pinned("CODEFLOW_TEST_OSV_DIR") else {
        return;
    };
    let path = path_with(&osv);
    let policy = r#"{"schema_version":1,"git":{"secret_scan":"off","security_review":"warn","dep_audit":"block"}}"#;
    let dir = repo(policy);
    let root = dir.path();
    // smallvec 0.6.9 carries RUSTSEC-2019-0009.
    let lock = "version = 3\n\n[[package]]\nname = \"smallvec\"\nversion = \"0.6.9\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
    std::fs::write(root.join("Cargo.lock"), lock).unwrap();
    commit_all(root, "build: lock");
    let out = scan(root, &path, &["--base", "main", "--only", "deps"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(
        text(&out).contains("dependency advisories found"),
        "{}",
        text(&out)
    );

    let ignore = |ids: &[&str]| {
        ids.iter().fold(String::new(), |mut toml, id| {
            toml.push_str("[[IgnoredVulns]]\nid = \"");
            toml.push_str(id);
            toml.push_str("\"\nreason = \"fixture\"\n\n");
            toml
        })
    };
    let report =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let ids: Vec<&str> = report
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|word| word.starts_with("RUSTSEC-") || word.starts_with("GHSA-"))
        .collect();
    assert!(!ids.is_empty(), "{report}");
    std::fs::write(root.join("osv-scanner.toml"), ignore(&ids)).unwrap();
    commit_all(root, "chore: suppress");
    let out = scan(root, &path, &["--base", "main", "--only", "deps"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "the range's own suppression: {}",
        text(&out)
    );

    let head = git(root, &["rev-parse", "HEAD"]);
    git(root, &["checkout", "-q", "main"]);
    std::fs::write(root.join("osv-scanner.toml"), ignore(&ids)).unwrap();
    commit_all(root, "chore: land the suppression");
    git(root, &["checkout", "-q", &head]);
    let out = scan(root, &path, &["--base", "main", "--only", "deps"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
}

/// An unknown base names no commit and exits 2 before any scan.
#[test]
fn an_unknown_base_is_a_usage_error() {
    let dir = repo(POLICY);
    let out = scan(dir.path(), "/usr/bin:/bin", &["--base", "no-such-ref"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("names no commit"), "{}", text(&out));
}
