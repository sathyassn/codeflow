//! A test never points a repository at a remote on the network (TSK-1156).
//!
//! The guard and the policy authority ask a repository's remote what it
//! holds when its tracking refs are missing, and a test that gives its
//! repository `https://example.invalid/...` makes that question a real host
//! lookup. A Codex builder seat blocks the lookup and aborts the whole test
//! run, so a gate cannot finish there. Test repositories use a `file://`
//! remote, or a loopback address with a closed port when the transport is
//! the subject, and this scan fails when a remote is registered any other
//! way. `GIT_ALLOW_PROTOCOL` cannot do this job: the guard's remote query
//! clears the environment and sets its own allowed transports.
//!
//! The scan reads every Rust source under `crates/*/src` and
//! `crates/*/tests`. A line that registers a remote (`.remote(`,
//! `"remote", "add"`, `set-url`, `Fixture::new(`) and holds a quoted
//! `http`, `https`, `ssh` or `git` URL must name a loopback host
//! (`127.0.0.1`, `localhost`, `[::1]`). [`ALLOWED`] lists the rest with the
//! reason the host is never contacted.

use std::path::{Path, PathBuf};

/// `(file under crates/, host, reason)`: a registered remote that is not
/// loopback. Each entry holds for every line of that file naming the host.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "codeflow-cli/tests/acceptance_cli.rs",
        "example.invalid",
        "the transport is the subject: git runs a stub ssh command, so the host is never looked up",
    ),
    (
        "codeflow-cli/tests/ci_cli/destination.rs",
        "example.test",
        "the transport is the subject: git runs a stub ssh command, so the host is never looked up",
    ),
    (
        "*",
        "github.com",
        "a forge host the fixtures name so a URL parses as a GitHub remote; no test runs a transport to it",
    ),
    (
        "*",
        "gitlab.com",
        "a forge host the fixtures name so a URL parses as a GitLab remote; no test runs a transport to it",
    ),
];

const LOOPBACK: &[&str] = &["127.0.0.1", "localhost", "[::1]"];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn registers_remote(line: &str) -> bool {
    line.contains(".remote(")
        || line.contains("Fixture::new(")
        || line.contains("set-url")
        || (line.contains("\"remote\"") && line.contains("\"add\""))
}

/// Whether `context` holds a call that hands a remote to git or the hooks.
fn is_asked(context: &str) -> bool {
    [
        "\"remote\"",
        "\"pre-push\"",
        ".remote(",
        "set-url",
        "ls-remote",
    ]
    .iter()
    .any(|word| context.contains(word))
}

/// The hosts of the network URLs quoted on `line`.
fn network_hosts(line: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for scheme in ["http://", "https://", "ssh://", "git://"] {
        for (at, _) in line.match_indices(scheme) {
            let rest = &line[at + scheme.len()..];
            let end = rest.find(['/', '"', '\'', ' ', ')']).unwrap_or(rest.len());
            let authority = &rest[..end];
            let host = authority.rsplit('@').next().unwrap_or(authority);
            // A port follows the host, except inside an IPv6 literal.
            let host = if host.starts_with('[') {
                host.split(']')
                    .next()
                    .map(|h| format!("{h}]"))
                    .unwrap_or_default()
            } else {
                host.split(':').next().unwrap_or(host).to_string()
            };
            hosts.push(host);
        }
    }
    hosts
}

#[test]
fn no_test_registers_a_remote_on_the_network() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&crates).unwrap() {
        let krate = entry.unwrap().path();
        rust_files(&krate.join("src"), &mut files);
        rust_files(&krate.join("tests"), &mut files);
    }
    assert!(files.len() > 100, "the scan found {} sources", files.len());
    let mut violations = Vec::new();
    for file in &files {
        let Ok(bytes) = std::fs::read(file) else {
            continue; // an unreadable file cannot register a remote
        };
        let text = String::from_utf8_lossy(&bytes);
        let shown = file
            .strip_prefix(&crates)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        // This file quotes URLs to explain itself and to test the reader.
        if shown.ends_with("tests/test_remotes_contract.rs") {
            continue;
        }
        let lines: Vec<&str> = text.split('\n').collect();
        for (number, line) in lines.iter().enumerate() {
            // A URL alone on its line is a call argument: it counts when one
            // of the four lines before it names a call that takes a remote.
            let context = lines[number.saturating_sub(4)..number].join("\n");
            let argument = line.trim_start().starts_with('"') && is_asked(&context);
            if !registers_remote(line) && !argument {
                continue;
            }
            for host in network_hosts(line) {
                let allowed = LOOPBACK.contains(&host.as_str())
                    || ALLOWED.iter().any(|(path, allowed_host, _)| {
                        (*path == "*" || shown == *path) && host == *allowed_host
                    });
                if !allowed {
                    violations.push(format!("{shown}:{}: remote host {host}", number + 1));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "a test registers a remote on the network, which a Codex builder seat blocks and aborts on; \
         use a file:// remote (for example file:///nonexistent/repo.git) or a loopback address, or list \
         the host in ALLOWED with the reason it is never contacted:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_reader_sees_hosts_and_registrations() {
    assert_eq!(
        network_hosts(r#".remote("origin", "https://user:tok@example.invalid:8443/r.git")"#),
        ["example.invalid"]
    );
    assert_eq!(
        network_hosts(r#"Fixture::new("ssh://[::1]:22/r.git")"#),
        ["[::1]"]
    );
    assert!(network_hosts(r#".remote("origin", "file:///x/r.git")"#).is_empty());
    assert!(registers_remote(r#"git(&["remote", "add", "o", "u"])"#));
    assert!(!registers_remote(r#"let url = "https://example.invalid";"#));
}
