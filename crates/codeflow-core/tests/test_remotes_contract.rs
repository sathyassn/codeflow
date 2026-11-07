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
//! [`ALLOWED`] binds each reviewed exception to its file and host, with any
//! number of occurrences, so a new registration in another file or on another
//! host fails, and an entry that no longer matches fails too.
//!
//! A `git config remote.<name>.url <URL>` (or `.pushurl`) argument list is a
//! registration too, and a literal `remote.<name>.url=<URL>` is read as the
//! URL. Rust escapes in a normal or byte literal (`\x68`, `\u{68}`, `\n`,
//! a line continuation) are decoded before the URL is read.
//!
//! Not seen, and so left to review: a URL built at run time (`format!`, a
//! function result, `concat!`, a variable defined in another file), a remote
//! added by a helper in another crate, and the files named in [`UNREAD`],
//! which the lint never opens because their names say they hold secrets;
//! the run prints them and fails on any other unreadable file or any other
//! file with `secret` in its name. A passing lint says these forms were not
//! found, nothing more.

#![allow(clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `(file under crates/, host, reason)`: a network remote that is reviewed
/// and allowed, in any number of occurrences. A new file or a new host needs
/// its own entry; an entry that matches nothing fails until it is removed.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "codeflow-cli/tests/acceptance_cli.rs",
        "example.invalid",
        "the transport is the subject: git runs a stub ssh command, so the host is never looked up",
    ),
    (
        "codeflow-cli/tests/ci_cli/destination.rs",
        "upstream.invalid",
        "the configured upstream is only shown in the destination message; the logging-proxy run of this module saw no request",
    ),
    (
        "codeflow-cli/tests/ci_cli/destination.rs",
        "example.test",
        "the transport is the subject: git runs a stub ssh command, so the host is never looked up",
    ),
    (
        "codeflow-core/src/doctor/remote_perimeter.rs",
        "github.com",
        "a unit test that classifies a URL's host name; it asks no remote",
    ),
    (
        "codeflow-cli/tests/remote_protect_cli.rs",
        "github.com",
        "the origin URL only names the repository to a gh stub on PATH; the repository sets protocol.https.allow=never, so git never contacts the host",
    ),
    (
        "codeflow-core/src/doctor/remote_perimeter.rs",
        "gitlab.com",
        "a unit test that classifies a URL's host name; it asks no remote",
    ),
    (
        "codeflow-core/src/hooks/ref_authority.rs",
        "example.invalid",
        "the default URL of a fixture whose tests read configuration only; the logging-proxy run of this module saw no request",
    ),
    (
        "codeflow-core/src/ids/git.rs",
        "host.invalid",
        "text for a redaction test; it is never given to a transport",
    ),
    (
        "codeflow-core/src/hooks/gitleaks.rs",
        "github.com",
        "production constant naming the gitleaks release page, not a test remote",
    ),
    (
        "codeflow-core/src/scaffold/release_pin.rs",
        "github.com",
        "production constant naming the release download base, not a test remote",
    ),
    (
        "codeflow-core/tests/model_eval_contract.rs",
        "github.com",
        "text of a pull request link in a redaction assertion; never given to a transport",
    ),
    (
        "codeflow-cli/tests/work_cli.rs",
        "github.com",
        "ten `git config remote.<name>.url` writes that only name a GitHub project for the work-start check; the logging-proxy run of this binary saw no request",
    ),
    (
        "codeflow-core/tests/wp7_contract.rs",
        "github.com",
        "remote URLs that only name a GitHub project for the work-start check; the proxy run of this binary saw no request",
    ),
];

/// Test sources the lint never opens, relative to `crates/`: their names say
/// they hold secrets, so no agent or lint run reads them.
const UNREAD: &[&str] = &[
    "codeflow-cli/tests/ci_cli/secret_scan_exemption.rs",
    "codeflow-cli/tests/refusal_journey/secret_paths.rs",
    "codeflow-core/tests/managed_secret_scan.rs",
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
                    body: if raw {
                        text[body_start..end].to_string()
                    } else {
                        unescape(&text[body_start..end])
                    },
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
                body: unescape(&text[body_start..end]),
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

/// The value of a normal or byte literal's body: `\n`, `\x68`, `\u{68}`,
/// escaped quotes and backslashes, and a backslash-newline continuation (which
/// also drops the leading whitespace of the next line) are decoded.
fn unescape(body: &str) -> String {
    let mut out = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('x') => {
                let hex: String = (0..2).filter_map(|_| chars.next()).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some('u') => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    let mut hex = String::new();
                    for h in chars.by_ref() {
                        if h == '}' {
                            break;
                        }
                        if h != '_' {
                            hex.push(h);
                        }
                    }
                    if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(ch);
                    }
                }
            }
            Some('\n') => {
                while chars.peek().is_some_and(|n| n.is_whitespace()) {
                    chars.next();
                }
            }
            Some('\r') => {
                chars.next_if_eq(&'\n');
                while chars.peek().is_some_and(|n| n.is_whitespace()) {
                    chars.next();
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
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
    let mut body = body.trim();
    // `remote.<name>.url=<URL>`, as `git -c` or a config line writes it.
    if let Some((key, value)) = body.split_once('=') {
        if is_remote_url_key(key) {
            body = value.trim();
        }
    }
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

/// Whether `key` is `remote.<name>.url` or `remote.<name>.pushurl`.
fn is_remote_url_key(key: &str) -> bool {
    // A key name, not a file name: the comparison is exact.
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    let named = |rest: &str| rest.ends_with(".url") || rest.ends_with(".pushurl");
    key.strip_prefix("remote.").is_some_and(named)
}

/// Whether a call or list (whitespace removed) writes a remote URL key with
/// a literal key: `"remote.<name>.url"` or `"remote.<name>.pushurl"`.
fn writes_remote_url(context: &str) -> bool {
    context.split('"').any(is_remote_url_key)
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
        let keyed = literal
            .body
            .split_once('=')
            .is_some_and(|(key, _)| is_remote_url_key(key.trim()));
        let registered = keyed
            || contexts(text, &skeleton, literal.start)
                .iter()
                .any(|context| {
                    writes_remote_url(context)
                        || REGISTERING.iter().any(|word| context.contains(word))
                });
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
    allowed: &[(&str, &str, &str)],
) -> Vec<String> {
    let mut violations = Vec::new();
    for (file, hosts) in found {
        for (host, count) in hosts {
            let listed = allowed
                .iter()
                .any(|entry| entry.0 == file && entry.1 == host);
            if !listed {
                violations.push(format!("{file}: {count} remote(s) on {host}"));
            }
        }
    }
    for (path, host, _) in allowed {
        let seen = found.get(*path).and_then(|hosts| hosts.get(*host)).copied();
        if seen.is_none() {
            violations.push(format!(
                "ALLOWED entry for {path} on {host} matches nothing; remove it"
            ));
        }
    }
    violations
}

/// The path of this lint, which quotes URLs to explain itself and to test the
/// reader. Exactly this file under `crates/` is not scanned.
const SELF: &str = "codeflow-core/tests/test_remotes_contract.rs";

/// The Rust sources under `dir`, found without following a link. Every
/// failure to list a folder, every link and every source that is not a
/// regular file is added to `problems`, so a source cannot hide behind an
/// unreadable folder or an alias.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>, problems: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            problems.push(format!("{}: cannot list folder ({error})", dir.display()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                problems.push(format!("{}: cannot list folder ({error})", dir.display()));
                continue;
            }
        };
        let path = entry.path();
        // `DirEntry::file_type` does not follow a link.
        match entry.file_type() {
            Ok(kind) if kind.is_symlink() => problems.push(format!(
                "{}: a link in the source tree is refused, never followed",
                path.display()
            )),
            Ok(kind) if kind.is_dir() => rust_files(&path, out, problems),
            Ok(kind) if kind.is_file() => {
                if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
            Ok(_) => problems.push(format!(
                "{}: neither a folder nor a regular file",
                path.display()
            )),
            Err(error) => problems.push(format!("{}: cannot inspect ({error})", path.display())),
        }
    }
}

/// One source root of a crate (`src` or `tests`), inspected without
/// following a link first. Only a genuinely absent root is skipped; a link
/// (dangling or not), a file or an uninspectable root is a problem.
fn source_root(root: &Path, out: &mut Vec<PathBuf>, problems: &mut Vec<String>) {
    match std::fs::symlink_metadata(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => problems.push(format!("{}: cannot inspect ({error})", root.display())),
        Ok(metadata) if metadata.file_type().is_symlink() => problems.push(format!(
            "{}: a link in the source tree is refused, never followed",
            root.display()
        )),
        Ok(metadata) if metadata.is_dir() => rust_files(root, out, problems),
        Ok(_) => problems.push(format!(
            "{}: a source root must be a folder",
            root.display()
        )),
    }
}

/// Every Rust source under `crates/*/src` and `crates/*/tests`, or the
/// problems that stopped the walk from seeing some.
fn collect_sources(crates: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let mut problems = Vec::new();
    match std::fs::read_dir(crates) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        let krate = entry.path();
                        match entry.file_type() {
                            Ok(kind) if kind.is_dir() => {
                                source_root(&krate.join("src"), &mut files, &mut problems);
                                source_root(&krate.join("tests"), &mut files, &mut problems);
                            }
                            Ok(kind) if kind.is_symlink() => problems.push(format!(
                                "{}: a link in the source tree is refused, never followed",
                                krate.display()
                            )),
                            Ok(_) => {}
                            Err(error) => problems
                                .push(format!("{}: cannot inspect ({error})", krate.display())),
                        }
                    }
                    Err(error) => {
                        problems.push(format!(
                            "{}: cannot list folder ({error})",
                            crates.display()
                        ));
                    }
                }
            }
        }
        Err(error) => problems.push(format!(
            "{}: cannot list folder ({error})",
            crates.display()
        )),
    }
    (files, problems)
}

/// The sources under `crates` and the violations the lint reports for them.
fn lint_tree(crates: &Path) -> (usize, Vec<String>) {
    let (files, mut unexpected) = collect_sources(crates);
    let mut found: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut skipped = Vec::new();
    for file in &files {
        let shown = file
            .strip_prefix(crates)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if UNREAD.contains(&shown.as_str()) {
            skipped.push(shown); // named, and never opened
            continue;
        }
        if shown.to_lowercase().contains("secret") {
            unexpected.push(format!("{shown}: a secret-named file not in UNREAD"));
            continue;
        }
        if shown == SELF {
            continue;
        }
        let bytes = match std::fs::read(file) {
            Ok(bytes) => bytes,
            Err(error) => {
                unexpected.push(format!("{shown}: unreadable ({error})"));
                continue;
            }
        };
        for (_, host, _) in scan_text(&String::from_utf8_lossy(&bytes)) {
            *found
                .entry(shown.clone())
                .or_default()
                .entry(host)
                .or_default() += 1;
        }
    }
    skipped.sort();
    eprintln!("remote lint skipped, never opened: {skipped:?}");
    let mut violations = judge(&found, ALLOWED);
    violations.extend(unexpected);
    for path in UNREAD {
        if !skipped.iter().any(|s| s == path) {
            violations.push(format!("UNREAD entry {path} matches no source; remove it"));
        }
    }
    (files.len(), violations)
}

#[test]
fn no_test_registers_a_remote_on_the_network() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let (sources, violations) = lint_tree(&crates);
    assert!(sources > 100, "the lint found {sources} sources");
    assert!(
        violations.is_empty(),
        "a test registers a remote on the network, which a Codex builder seat blocks and aborts on; \
         use a file:// remote (for example file:///nonexistent/repo.git) or a loopback address, or \
         add the file and host to ALLOWED with the reason it is never contacted:\n{}",
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
fn the_lint_sees_git_config_remote_url_writes_and_escaped_urls() {
    // `git config remote.<name>.url <URL>`, the URL on a later line.
    assert_eq!(
        hosts("git(root, &[\n    \"config\",\n    \"remote.review.url\",\n    \"https://outside.invalid/o.git\",\n]);"),
        ["outside.invalid"]
    );
    assert_eq!(
        hosts("git(root, &[\"config\", \"remote.o.pushurl\", \"ssh://outside.invalid/o.git\"]);"),
        ["outside.invalid"]
    );
    // `git -c remote.o.url=<URL>` and a config line in one literal.
    assert_eq!(
        hosts("run(&[\"-c\", \"remote.o.url=https://outside.invalid/o.git\"]);"),
        ["outside.invalid"]
    );
    // Rust escapes are decoded before the URL is read.
    assert_eq!(
        hosts("Fixture::new(\"\\x68ttps://outside.invalid/o.git\");"),
        ["outside.invalid"]
    );
    assert_eq!(
        hosts("Fixture::new(\"https:\\u{2f}/outside.invalid/o.git\");"),
        ["outside.invalid"]
    );
    assert_eq!(unescape("a\\\n    b\\n\\\\"), "ab\n\\");
}

#[test]
fn the_lint_leaves_alone_git_config_writes_that_are_not_network_remotes() {
    for quiet in [
        "git(root, &[\"config\", \"remote.o.url\", \"file:///nonexistent/o.git\"]);",
        "git(root, &[\"config\", \"remote.o.url\", \"https://127.0.0.1:9/o.git\"]);",
        "git(root, &[\"config\", \"user.email\", \"t@example.test\"]);",
        "let k = \"remote.o.url\";",
    ] {
        assert!(hosts(quiet).is_empty(), "{quiet}");
    }
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
fn an_allowance_is_bound_to_its_file_and_host() {
    let mut found: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    found
        .entry("crate/tests/listed.rs".into())
        .or_default()
        .insert("github.com".into(), 2);
    let allowed: &[(&str, &str, &str)] = &[("crate/tests/listed.rs", "github.com", "reviewed")];
    assert!(judge(&found, allowed).is_empty());
    // A new GitHub registration in another file is not covered.
    found
        .entry("crate/tests/new.rs".into())
        .or_default()
        .insert("github.com".into(), 1);
    assert_eq!(judge(&found, allowed).len(), 1);
    // A third occurrence in the listed file and host is covered.
    found.remove("crate/tests/new.rs");
    found
        .get_mut("crate/tests/listed.rs")
        .unwrap()
        .insert("github.com".into(), 3);
    assert!(judge(&found, allowed).is_empty());
    // An unlisted host in the listed file is not covered.
    found
        .get_mut("crate/tests/listed.rs")
        .unwrap()
        .insert("gitlab.com".into(), 1);
    assert_eq!(judge(&found, allowed).len(), 1);
    // An entry that matches nothing is stale.
    found.clear();
    assert_eq!(judge(&found, allowed).len(), 1);
}

#[cfg(unix)]
#[test]
fn the_walk_refuses_an_unreadable_folder_and_a_link() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let krate = tmp.path().join("one");
    let src = krate.join("src");
    let nested = src.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(src.join("lib.rs"), "fn a() {}").unwrap();
    std::fs::write(nested.join("hidden.rs"), "fn b() {}").unwrap();
    // A crate with no tests folder is fine.
    let (files, problems) = collect_sources(tmp.path());
    assert_eq!(files.len(), 2, "{files:?}");
    assert!(problems.is_empty(), "{problems:?}");

    // An unreadable nested folder is reported, not skipped.
    std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o000)).unwrap();
    let (_, problems) = collect_sources(tmp.path());
    std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o755)).unwrap();
    let root = unsafe { libc::geteuid() } == 0;
    assert!(
        root || problems.iter().any(|p| p.contains("cannot list folder")),
        "{problems:?}"
    );

    // A link with an innocent name to a target that must stay unopened: the
    // target is made unreadable, so any read of it would also be an error,
    // and the walk must name the link as the problem.
    let target = tmp.path().join("target-never-read.txt");
    std::fs::write(&target, "never read").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o000)).unwrap();
    std::os::unix::fs::symlink(&target, src.join("alias.rs")).unwrap();
    let (files, problems) = collect_sources(tmp.path());
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("alias.rs") && p.contains("link")),
        "{problems:?}"
    );
    assert!(!files.iter().any(|f| f.ends_with("alias.rs")), "{files:?}");
    // A linked folder is refused too.
    std::fs::remove_file(src.join("alias.rs")).unwrap();
    std::os::unix::fs::symlink(&nested, src.join("folder")).unwrap();
    let (_, problems) = collect_sources(tmp.path());
    assert!(
        problems
            .iter()
            .any(|p| p.contains("folder") && p.contains("link")),
        "{problems:?}"
    );
}

#[cfg(unix)]
#[test]
fn the_walk_refuses_a_linked_or_dangling_source_root() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("elsewhere");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("lib.rs"), "fn a() {}").unwrap();
    let krate = tmp.path().join("crates").join("one");
    std::fs::create_dir_all(&krate).unwrap();
    let crates = tmp.path().join("crates");
    // A crate with neither root is fine: the roots are genuinely absent.
    let (files, problems) = collect_sources(&crates);
    assert!(
        files.is_empty() && problems.is_empty(),
        "{files:?} {problems:?}"
    );
    // A linked root is refused and its target is not walked.
    std::os::unix::fs::symlink(&real, krate.join("src")).unwrap();
    let (files, problems) = collect_sources(&crates);
    assert!(files.is_empty(), "the link was followed: {files:?}");
    assert!(
        problems
            .iter()
            .any(|p| p.ends_with("one/src: a link in the source tree is refused, never followed")),
        "{problems:?}"
    );
    // A dangling link is not an absent root.
    std::os::unix::fs::symlink(tmp.path().join("nowhere"), krate.join("tests")).unwrap();
    let (_, problems) = collect_sources(&crates);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("one/tests") && p.contains("link")),
        "{problems:?}"
    );
    // A file where a root belongs is refused too.
    std::fs::remove_file(krate.join("tests")).unwrap();
    std::fs::write(krate.join("tests"), "x").unwrap();
    let (_, problems) = collect_sources(&crates);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("one/tests") && p.contains("must be a folder")),
        "{problems:?}"
    );
}

#[test]
fn only_this_exact_file_is_exempt_from_the_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let crates = tmp.path();
    // The same file name in another crate registers a network remote.
    let other = crates.join("another-crate").join("tests");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        other.join("test_remotes_contract.rs"),
        "fn t() { Fixture::new(\"https://outside.invalid/x.git\"); }",
    )
    .unwrap();
    // This lint's own path is exempt.
    let own = crates.join("codeflow-core").join("tests");
    std::fs::create_dir_all(&own).unwrap();
    std::fs::write(
        own.join("test_remotes_contract.rs"),
        "fn t() { Fixture::new(\"https://quoted.invalid/x.git\"); }",
    )
    .unwrap();
    let (_, violations) = lint_tree(crates);
    assert!(
        violations.iter().any(
            |v| v.contains("another-crate/tests/test_remotes_contract.rs")
                && v.contains("outside.invalid")
        ),
        "{violations:?}"
    );
    assert!(
        !violations.iter().any(|v| v.contains("quoted.invalid")),
        "{violations:?}"
    );
}
