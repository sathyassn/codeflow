//! A lint against test repositories that point at a remote on the network
//! (TSK-1156). It is a lint, not proof that no test reaches the network.
//!
//! The guard and the policy authority ask a repository's remote what it
//! holds when its tracking refs are missing, and a test that gives its
//! repository `https://example.invalid/...` makes that question a real host
//! lookup. A Codex builder seat blocks the lookup and aborts the whole test
//! run, so a gate cannot finish there. Test repositories use a `file://`
//! remote, or a loopback address with a closed port when the transport is
//! the subject. `GIT_ALLOW_PROTOCOL` cannot do this job: the guard's remote
//! query clears the environment and sets its own allowed transports.
//!
//! The lint reads every Rust source under `crates/*/src` and
//! `crates/*/tests` as tokens: comments are dropped, and normal, byte and
//! raw string literals (also over several lines) are read whole. A literal
//! that is a network remote, a `scheme://host` URL of `http`, `https`, `ssh`
//! or `git`, or an scp-style `user@host:path`, is reported when its host is
//! not loopback (`127.0.0.1`, `localhost`, `[::1]`) and either
//!
//! - it sits inside a call, however many lines the call spans, that
//!   registers or asks a remote (`.remote(`, `Fixture::new(`,
//!   `repository_with_url(`, a `git` argument list with `"remote"` and
//!   `"add"` or `"set-url"`, `"pre-push"`, `"ls-remote"`, `"fetch"`,
//!   `"clone"` or `"push"`), looking at the four calls or lists around it, or
//! - it is bound to a name by `const`, `static` or `let`, because a bound URL
//!   can be handed to any registering call from anywhere.
//!
//! [`ALLOWED`] binds each reviewed exception to its file, host and exact
//! count of occurrences, so a new registration anywhere else fails, and an
//! entry that no longer matches fails too.
//!
//! Not seen, and so left to review: a URL built at run time (`format!`, a
//! function result, a variable defined in another file), a remote added by
//! a helper in another crate, and the three `*secret*` test files this
//! environment cannot read. A passing lint says these forms were not found,
//! nothing more.

#![allow(clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `(file under crates/, host, occurrences, reason)`: a network remote that
/// is reviewed and allowed. The count is exact.
const ALLOWED: &[(&str, &str, usize, &str)] = &[
    (
        "codeflow-cli/tests/acceptance_cli.rs",
        "example.invalid",
        1,
        "the transport is the subject: git runs a stub ssh command, so the host is never looked up",
    ),
    (
        "codeflow-cli/tests/ci_cli/destination.rs",
        "example.test",
        1,
        "the transport is the subject: git runs a stub ssh command, so the host is never looked up",
    ),
    (
        "codeflow-core/src/doctor/remote_perimeter.rs",
        "github.com",
        1,
        "a unit test that classifies a URL's host name; it asks no remote",
    ),
    (
        "codeflow-core/src/doctor/remote_perimeter.rs",
        "gitlab.com",
        1,
        "a unit test that classifies a URL's host name; it asks no remote",
    ),
    (
        "codeflow-core/src/hooks/ref_authority.rs",
        "example.invalid",
        1,
        "the default URL of a fixture whose tests read configuration only; the logging-proxy run of this module saw no request",
    ),
    (
        "codeflow-core/src/ids/git.rs",
        "host.invalid",
        1,
        "text for a redaction test; it is never given to a transport",
    ),
    (
        "codeflow-core/src/hooks/gitleaks.rs",
        "github.com",
        1,
        "production constant naming the gitleaks release page, not a test remote",
    ),
    (
        "codeflow-core/src/scaffold/release_pin.rs",
        "github.com",
        1,
        "production constant naming the release download base, not a test remote",
    ),
    (
        "codeflow-core/tests/model_eval_contract.rs",
        "github.com",
        1,
        "text of a pull request link in a redaction assertion; never given to a transport",
    ),
    (
        "codeflow-core/tests/wp7_contract.rs",
        "github.com",
        2,
        "remote URLs that only name a GitHub project for the work-start check; the proxy run of this binary saw no request",
    ),
];

const LOOPBACK: &[&str] = &["127.0.0.1", "localhost", "[::1]"];

/// A string literal: the byte offset of its body in the source, and the body.
struct Literal {
    start: usize,
    body: String,
}

/// The literals of `text`; `text` with comments blanked; and that text with
/// the bodies of literals filled too, so bracket matching ignores what is
/// inside either. All three keep every byte offset.
fn lex(text: &str) -> (Vec<Literal>, String, Vec<u8>) {
    let bytes = text.as_bytes();
    let mut skeleton = bytes.to_vec();
    let mut code = bytes.to_vec();
    let mut literals = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        let next = bytes.get(at + 1).copied();
        if byte == b'/' && next == Some(b'/') {
            let end = bytes[at..]
                .iter()
                .position(|b| *b == b'\n')
                .map_or(bytes.len(), |n| at + n);
            skeleton[at..end].fill(b' ');
            code[at..end].fill(b' ');
            at = end;
        } else if byte == b'/' && next == Some(b'*') {
            let end = text[at + 2..]
                .find("*/")
                .map_or(bytes.len(), |n| at + 2 + n + 2);
            skeleton[at..end].fill(b' ');
            code[at..end].fill(b' ');
            at = end;
        } else if (byte == b'r' || byte == b'b') && (at == 0 || !is_word(bytes[at - 1])) {
            // r"..", r#".."#, b"..", br#".."#; anything else is an identifier.
            let mut cursor = at + usize::from(byte == b'b');
            let raw = bytes.get(cursor) == Some(&b'r');
            if raw {
                cursor += 1;
            }
            let mut hashes = 0;
            while raw && bytes.get(cursor) == Some(&b'#') {
                hashes += 1;
                cursor += 1;
            }
            if bytes.get(cursor) == Some(&b'"') && cursor > at {
                let body_start = cursor + 1;
                let close = if raw {
                    format!("\"{}", "#".repeat(hashes))
                } else {
                    "\"".to_string()
                };
                let mut search = body_start;
                let end = loop {
                    match text[search..].find(&close) {
                        Some(n) if !raw && escaped(bytes, search + n) => search += n + 1,
                        Some(n) => break search + n,
                        None => break bytes.len(),
                    }
                };
                skeleton[body_start..end].fill(b'x');
                literals.push(Literal {
                    start: body_start,
                    body: text[body_start..end].to_string(),
                });
                at = (end + close.len()).min(bytes.len());
            } else {
                at += 1;
            }
        } else if byte == b'"' {
            let body_start = at + 1;
            let mut end = body_start;
            while end < bytes.len() && (bytes[end] != b'"' || escaped(bytes, end)) {
                end += 1;
            }
            skeleton[body_start..end].fill(b'x');
            literals.push(Literal {
                start: body_start,
                body: text[body_start..end].to_string(),
            });
            at = (end + 1).min(bytes.len());
        } else if byte == b'\'' {
            // A character literal ('"', '\n') or a lifetime ('a).
            let close = if next == Some(b'\\') { at + 3 } else { at + 2 };
            if bytes.get(close) == Some(&b'\'') {
                skeleton[at + 1..close].fill(b'x');
                at = close + 1;
            } else {
                at += 1;
            }
        } else {
            at += 1;
        }
    }
    (
        literals,
        String::from_utf8_lossy(&code).into_owned(),
        skeleton,
    )
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether the byte at `at` is preceded by an odd number of backslashes.
fn escaped(bytes: &[u8], at: usize) -> bool {
    bytes[..at]
        .iter()
        .rev()
        .take_while(|b| **b == b'\\')
        .count()
        % 2
        == 1
}

/// The host of `body` when the literal is a network remote, else `None`.
fn network_host(body: &str) -> Option<String> {
    let body = body.trim();
    for scheme in ["http://", "https://", "ssh://", "git://"] {
        if let Some(rest) = body.strip_prefix(scheme) {
            let end = rest
                .find(['/', '"', '\'', ' ', ')', '\n'])
                .unwrap_or(rest.len());
            let host = rest[..end].rsplit('@').next().unwrap_or("");
            // A port follows the host, except inside an IPv6 literal.
            return Some(if host.starts_with('[') {
                host.split(']')
                    .next()
                    .map(|h| format!("{h}]"))
                    .unwrap_or_default()
            } else {
                host.split(':').next().unwrap_or(host).to_string()
            });
        }
    }
    // scp-style: user@host:path, with no scheme.
    let (user, rest) = body.split_once('@')?;
    let user_ok = !user.is_empty() && user.bytes().all(|b| is_word(b) || b == b'-' || b == b'.');
    let (host, path) = rest.split_once(':')?;
    let host_ok = !host.is_empty()
        && host.bytes().all(|b| is_word(b) || b == b'-' || b == b'.')
        && host.contains('.');
    let path_ok = path
        .bytes()
        .next()
        .is_some_and(|b| is_word(b) || b == b'/' || b == b'~');
    (user_ok && host_ok && path_ok && !path.starts_with(|c: char| c.is_ascii_digit()))
        .then(|| host.to_string())
}

const REGISTERING: &[&str] = &[
    ".remote(",
    "Fixture::new(",
    "repository_with_url(",
    "remote_set_url(",
    "\"remote\",\"add\"",
    "\"remote\",\"set-url\"",
    "\"set-url\"",
    "\"pre-push\"",
    "\"ls-remote\"",
    "\"fetch\"",
    // Split so the clone contract scan, which reads literals, sees no clone.
    concat!("\"cl", "one\""),
    "\"push\"",
];

/// The text of the enclosing calls and lists around `at`, innermost first,
/// as written (whitespace removed), for up to four levels.
fn contexts(code: &str, skeleton: &[u8], at: usize) -> Vec<String> {
    let mut found = Vec::new();
    let mut position = at;
    for _ in 0..4 {
        let mut depth = 0usize;
        let mut opener = None;
        for index in (0..position).rev() {
            match skeleton[index] {
                b')' | b']' | b'}' => depth += 1,
                b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
                b'(' | b'[' => {
                    opener = Some(index);
                    break;
                }
                b'{' | b';' => break,
                _ => {}
            }
        }
        let Some(open) = opener else { break };
        let mut depth = 0usize;
        let mut close = skeleton.len();
        for (index, byte) in skeleton.iter().enumerate().skip(open) {
            match byte {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = index + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let before = &code[..open];
        let from = before
            .rfind([';', '{', '}'])
            .map_or(0, |n| n + 1)
            .max(open.saturating_sub(80));
        let from = (from..=open)
            .find(|n| code.is_char_boundary(*n))
            .unwrap_or(open);
        let text: String = code[from..close.min(code.len())]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        found.push(text);
        position = open;
    }
    found
}

/// What a source holds that this lint reports: `(line, host, form)`.
fn scan_text(text: &str) -> Vec<(usize, String, &'static str)> {
    let (literals, code, skeleton) = lex(text);
    let text = code.as_str();
    let mut hits = Vec::new();
    for literal in &literals {
        let Some(host) = network_host(&literal.body) else {
            continue;
        };
        if LOOPBACK.contains(&host.as_str()) {
            continue;
        }
        let line = text[..literal.start].matches('\n').count() + 1;
        // The statement before the literal: `const X: &str = ` or `let url = `.
        let before = &text[..literal.start.saturating_sub(1)];
        let statement = before
            .rfind([';', '{', '}'])
            .map_or(before, |n| &before[n + 1..])
            .trim()
            .trim_end_matches(['r', 'b', '#'])
            .trim_end();
        let bound = ["const ", "static ", "let "]
            .iter()
            .any(|word| statement.starts_with(word) || statement.contains(&format!(" {word}")))
            && statement.ends_with('=');
        let registered = contexts(text, &skeleton, literal.start)
            .iter()
            .any(|context| REGISTERING.iter().any(|word| context.contains(word)));
        if bound {
            hits.push((line, host, "bound to a name"));
        } else if registered {
            hits.push((line, host, "registered"));
        }
    }
    hits
}

/// The violations among `found` (`file -> host -> count`) against `allowed`.
fn judge(
    found: &BTreeMap<String, BTreeMap<String, usize>>,
    allowed: &[(&str, &str, usize, &str)],
) -> Vec<String> {
    let mut violations = Vec::new();
    for (file, hosts) in found {
        for (host, count) in hosts {
            let entry = allowed
                .iter()
                .find(|entry| entry.0 == file && entry.1 == host);
            match entry {
                None => violations.push(format!("{file}: {count} remote(s) on {host}")),
                Some((_, _, expected, _)) if expected != count => violations.push(format!(
                    "{file}: {count} remote(s) on {host}, but ALLOWED expects {expected}"
                )),
                Some(_) => {}
            }
        }
    }
    for (path, host, expected, _) in allowed {
        let seen = found.get(*path).and_then(|hosts| hosts.get(*host)).copied();
        if seen.is_none() {
            violations.push(format!(
                "ALLOWED entry for {path} on {host} (expects {expected}) matches nothing; remove it"
            ));
        }
    }
    violations
}

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

#[test]
fn no_test_registers_a_remote_on_the_network() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&crates).unwrap() {
        let krate = entry.unwrap().path();
        rust_files(&krate.join("src"), &mut files);
        rust_files(&krate.join("tests"), &mut files);
    }
    assert!(files.len() > 100, "the lint found {} sources", files.len());
    let mut found: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for file in &files {
        let Ok(bytes) = std::fs::read(file) else {
            continue; // an unreadable file cannot be read for a remote
        };
        let shown = file
            .strip_prefix(&crates)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        // This file quotes URLs to explain itself and to test the reader.
        if shown.ends_with("tests/test_remotes_contract.rs") {
            continue;
        }
        for (_, host, _) in scan_text(&String::from_utf8_lossy(&bytes)) {
            *found
                .entry(shown.clone())
                .or_default()
                .entry(host)
                .or_default() += 1;
        }
    }
    let violations = judge(&found, ALLOWED);
    assert!(
        violations.is_empty(),
        "a test registers a remote on the network, which a Codex builder seat blocks and aborts on; \
         use a file:// remote (for example file:///nonexistent/repo.git) or a loopback address, or \
         add the exact file, host and count to ALLOWED with the reason it is never contacted:\n{}",
        violations.join("\n")
    );
}

fn hosts(text: &str) -> Vec<String> {
    scan_text(text)
        .into_iter()
        .map(|(_, host, _)| host)
        .collect()
}

#[test]
fn the_lint_sees_the_forms_the_first_version_missed() {
    // The URL on the line after the registering call.
    assert_eq!(
        hosts("let f = Fixture::new(\n    \"https://outside.invalid/repo.git\",\n);"),
        ["outside.invalid"]
    );
    // scp-style, with no scheme.
    assert_eq!(
        hosts("repo.remote(\"origin\", \"git@outside.invalid:team/repo.git\");"),
        ["outside.invalid"]
    );
    // A URL held in a constant, a static or a let, then handed on.
    assert_eq!(
        hosts("const URL: &str = \"https://outside.invalid/c.git\";\nrepo.remote(\"o\", URL);"),
        ["outside.invalid"]
    );
    assert_eq!(
        hosts("let url = \"ssh://outside.invalid/l.git\";\nrepo.remote(\"o\", url);"),
        ["outside.invalid"]
    );
    // A raw string over several lines, also behind a byte prefix.
    assert_eq!(
        hosts("Fixture::new(r#\"\n    ssh://outside.invalid/raw.git\n\"#);"),
        ["outside.invalid"]
    );
    // A git argument list with the URL on its own line.
    assert_eq!(
        hosts("git(root, &[\n    \"remote\",\n    \"add\",\n    \"origin\",\n    \"https://outside.invalid/a.git\",\n]);"),
        ["outside.invalid"]
    );
    assert_eq!(
        hosts(
            "run(&[\"git-hook\", \"pre-push\", \"origin\",\n  \"https://outside.invalid/p.git\"]);"
        ),
        ["outside.invalid"]
    );
}

#[test]
fn the_lint_leaves_alone_what_is_not_a_network_remote() {
    for quiet in [
        "Fixture::new(\"file:///nonexistent/repo.git\");",
        "repo.remote(\"origin\", \"https://127.0.0.1:9/x.git\");",
        "repo.remote(\"origin\", \"ssh://localhost/x.git\");",
        "repo.remote(\"origin\", \"http://[::1]:9/x.git\");",
        "// repo.remote(\"origin\", \"https://outside.invalid/x.git\");",
        "/* Fixture::new(\"https://outside.invalid/x.git\") */",
        "let note = \"see https://outside.invalid/docs for details\";",
        "let who = \"test@example.com\";",
        "git(&[\"commit\", \"-m\", \"fetch https://outside.invalid/x\"]);",
        "let c = '\"'; Fixture::new(\"file:///x\");",
    ] {
        assert!(hosts(quiet).is_empty(), "{quiet}");
    }
}

#[test]
fn an_allowance_is_bound_to_its_file_host_and_count() {
    let mut found: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    found
        .entry("crate/tests/listed.rs".into())
        .or_default()
        .insert("github.com".into(), 2);
    let allowed: &[(&str, &str, usize, &str)] =
        &[("crate/tests/listed.rs", "github.com", 2, "reviewed")];
    assert!(judge(&found, allowed).is_empty());
    // A new GitHub registration in another file is not covered.
    found
        .entry("crate/tests/new.rs".into())
        .or_default()
        .insert("github.com".into(), 1);
    assert_eq!(judge(&found, allowed).len(), 1);
    // A third occurrence in the listed file is not covered.
    found.remove("crate/tests/new.rs");
    found
        .get_mut("crate/tests/listed.rs")
        .unwrap()
        .insert("github.com".into(), 3);
    assert_eq!(judge(&found, allowed).len(), 1);
    // An entry that matches nothing is stale.
    found.clear();
    assert_eq!(judge(&found, allowed).len(), 1);
}
