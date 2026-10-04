//! A child's stdin is piped in one place: `crates/codeflow-core/src/git/stdin.rs`
//! (issue 71, TSK-237). A caller that wrote stdin itself, or piped it and
//! drained one output stream at a time, could deadlock against a child that
//! answers while it reads. This scan covers the Rust sources under `crates/`
//! (not scripts or assets). It reads code tokens, not text: comments, string
//! and character literals are ignored, and `#[cfg(test)]` items are skipped
//! one by one, so production code after a test module is still read. Outside
//! the helper it accepts only a stdin set to null or inherited, or the
//! process's own `io::stdin()`; any other `.stdin(...)` argument (a variable
//! or a conditional hides what it pipes), a `Command::stdin` path call and any
//! use of a child's `stdin` handle, `ChildStdin`, or a `Child { .. }` pattern is
//! a failure. Raw identifiers (`r#stdin`) are read as the plain name. Limits,
//! stated rather than hidden: code a macro generates, and raw file
//! descriptors or handles taken from the operating system, are not seen.

use std::path::{Path, PathBuf};

/// The only source file allowed to pipe a child's stdin.
const HELPER: &str = "crates/codeflow-core/src/git/stdin.rs";

/// The arguments of `.stdin(...)` that do not pipe, whitespace removed.
const NOT_PIPED: [&str; 6] = [
    "Stdio::null()",
    "std::process::Stdio::null()",
    "process::Stdio::null()",
    "Stdio::inherit()",
    "std::process::Stdio::inherit()",
    "process::Stdio::inherit()",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name != "tests" && name != "target" {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && name != "tests.rs"
            && !name.ends_with("_tests.rs")
        {
            out.push(path);
        }
    }
}

/// `text` with comments, string literals and character literals blanked
/// (newlines kept, so line numbers stay true).
fn code_only(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for byte in &mut out[from..to] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        if rest.starts_with(b"//") {
            let end = rest
                .iter()
                .position(|b| *b == b'\n')
                .map_or(bytes.len(), |n| i + n);
            blank(&mut out, i, end);
            i = end;
        } else if rest.starts_with(b"/*") {
            let mut depth = 0_usize;
            let mut j = i;
            while j < bytes.len() {
                if bytes[j..].starts_with(b"/*") {
                    depth += 1;
                    j += 2;
                } else if bytes[j..].starts_with(b"*/") {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            blank(&mut out, i, j);
            i = j;
        } else if (i == 0 || !is_ident(bytes[i - 1]))
            && (rest.starts_with(b"r\"")
                || rest.starts_with(b"r#")
                || rest.starts_with(b"br\"")
                || rest.starts_with(b"br#"))
        {
            let start = i + usize::from(rest[0] == b'b') + 1;
            let hashes = bytes[start..].iter().take_while(|b| **b == b'#').count();
            if rest.starts_with(b"r#") && bytes.get(start + hashes).is_some_and(|b| is_ident(*b)) {
                // A raw identifier (`r#stdin`): keep the name, drop `r#`.
                blank(&mut out, i, i + 2);
                i += 2;
            } else if bytes.get(start + hashes) == Some(&b'"') {
                let closing = format!("\"{}", "#".repeat(hashes));
                let body = start + hashes + 1;
                let end = text[body..]
                    .find(&closing)
                    .map_or(bytes.len(), |n| body + n + closing.len());
                blank(&mut out, i, end);
                i = end;
            } else {
                i += 1;
            }
        } else if rest[0] == b'"' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'"' {
                j += if bytes[j] == b'\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(bytes.len());
            blank(&mut out, i, end);
            i = end;
        } else if rest[0] == b'\'' {
            // A character literal, not a lifetime: `'x'` or an escape.
            let literal_end = if rest.get(1) == Some(&b'\\') {
                rest.iter()
                    .skip(2)
                    .position(|b| *b == b'\'')
                    .map(|n| i + n + 3)
            } else if rest.get(2) == Some(&b'\'') {
                Some(i + 3)
            } else {
                None
            };
            if let Some(end) = literal_end {
                blank(&mut out, i, end.min(bytes.len()));
                i = end;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

/// `code` with each `#[cfg(test)]` item blanked.
fn without_test_items(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut out = bytes.to_vec();
    let mut from = 0;
    while let Some(found) = code[from..].find("#[cfg(") {
        let at = from + found;
        let close = code[at..].find(']').map_or(code.len(), |n| at + n + 1);
        let attribute: String = code[at..close].split_whitespace().collect();
        from = close;
        if attribute != "#[cfg(test)]" && !attribute.starts_with("#[cfg(all(test,") {
            continue;
        }
        let mut depth = 0_usize;
        let mut end = code.len();
        for (offset, byte) in code[close..].bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = close + offset + 1;
                        break;
                    }
                }
                b';' if depth == 0 => {
                    end = close + offset + 1;
                    break;
                }
                _ => {}
            }
        }
        for byte in &mut out[at..end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        from = end;
    }
    String::from_utf8(out).unwrap()
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Every use of a child's stdin in `source`, as (line, what).
fn stdin_uses(source: &str) -> Vec<(usize, String)> {
    let code = without_test_items(&code_only(source));
    let bytes = code.as_bytes();
    let mut found = Vec::new();
    for (at, _) in code.match_indices("stdin") {
        let end = at + "stdin".len();
        if (at > 0 && is_ident(bytes[at - 1])) || bytes.get(end).is_some_and(|b| is_ident(*b)) {
            continue;
        }
        let before = code[..at].trim_end();
        let path_call = before.ends_with("::");
        if !before.ends_with('.') && !path_call {
            continue;
        }
        let line = code[..at].matches('\n').count() + 1;
        let after = code[end..].trim_start();
        if !after.starts_with('(') {
            // `use std::io::stdin;` names the process's own stdin.
            let statement = code[..at].rfind([';', '{', '}']).map_or(0, |n| n + 1);
            let names_import = path_call
                && code[statement..at]
                    .trim_start()
                    .trim_start_matches("pub ")
                    .starts_with("use ");
            if !names_import {
                found.push((line, "a use of a child's stdin handle".to_string()));
            }
            continue;
        }
        let open = end + (code[end..].len() - after.len());
        let mut depth = 0_usize;
        let mut close = code.len();
        for (offset, byte) in code[open..].bytes().enumerate() {
            match byte {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = open + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        let argument: String = code[open + 1..close].split_whitespace().collect();
        let argument = argument.trim_end_matches(',').to_string();
        let own_stdin = path_call && argument.is_empty();
        if !own_stdin && !NOT_PIPED.contains(&argument.as_str()) {
            found.push((line, format!("stdin({argument})")));
        }
    }
    // Destructuring a `Child { stdin, .. }` reaches the handle without the
    // word after a dot, and `ChildStdin` names its type.
    for name in ["ChildStdin", "Child"] {
        for (at, _) in code.match_indices(name) {
            let end = at + name.len();
            if (at > 0 && is_ident(bytes[at - 1])) || bytes.get(end).is_some_and(|b| is_ident(*b)) {
                continue;
            }
            if name == "Child" {
                // Only a `Child { stdin, .. }` pattern: braces that name
                // `stdin` (a return type followed by a body does not).
                let rest = code[end..].trim_start();
                let names_stdin = rest.starts_with('{')
                    && rest.find('}').is_some_and(|close| {
                        rest[..close]
                            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                            .any(|word| word == "stdin")
                    });
                if !names_stdin {
                    continue;
                }
            }
            let line = code[..at].matches('\n').count() + 1;
            found.push((
                line,
                format!("the child process type `{name}` spelled as a pattern"),
            ));
        }
    }
    found.sort();
    found
}

#[test]
fn only_the_stdin_helper_pipes_a_childs_stdin() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(files.len() > 100, "the scan found the workspace sources");
    let mut offenders = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
            .replace("/../", "/");
        let relative = relative.trim_start_matches("./").to_string();
        if relative == HELPER {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (line, what) in stdin_uses(&text) {
            offenders.push(format!("{relative}:{line}: {what}"));
        }
    }
    assert!(
        offenders.is_empty(),
        "pipe a child's stdin through codeflow_core::git::output_with_input or \
         spawn_with_input (crates/codeflow-core/src/git/stdin.rs), which writes it from its \
         own thread; found:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_scan_flags_each_way_to_reach_a_childs_stdin() {
    for (source, what) in [
        ("c.stdin(Stdio::piped());", "piped"),
        ("c.stdin(std::process::Stdio::piped());", "qualified"),
        ("c . stdin (\n  Stdio::piped(),\n);", "spaced and split"),
        ("c.stdin(mode);", "a variable"),
        (
            "c.stdin(if x { Stdio::piped() } else { Stdio::null() });",
            "a conditional",
        ),
        ("Command::stdin(&mut c, mode);", "a path call"),
        ("let w = child.stdin.take();", "take"),
        (
            "let w = child\n    .stdin\n    .take();",
            "take across lines",
        ),
        ("child.stdin.as_mut().unwrap();", "as_mut"),
        ("command.r#stdin(Stdio::piped());", "raw identifier call"),
        ("let w = child.r#stdin.take();", "raw identifier field"),
        (
            "Command::r#stdin(&mut command, mode);",
            "raw identifier path",
        ),
        ("let Child { stdin, .. } = child;", "a destructured child"),
        ("fn f(w: ChildStdin) {}", "the handle type"),
    ] {
        assert_eq!(stdin_uses(source).len(), 1, "{what}: {source}");
    }
}

#[test]
fn the_scan_passes_what_does_not_pipe() {
    for (source, what) in [
        ("c.stdin(Stdio::null());", "null"),
        ("c . stdin ( std::process::Stdio::inherit() );", "inherit"),
        (
            "let mut s = String::new(); std::io::stdin().read_line(&mut s);",
            "own stdin",
        ),
        (
            "// c.stdin(Stdio::piped()) in a comment\nfn f() {}",
            "line comment",
        ),
        ("/* child.stdin.take() */ fn f() {}", "block comment"),
        ("let s = \"child.stdin.take()\";", "string"),
        ("let s = r#\"c.stdin(Stdio::piped())\"#;", "raw string"),
        (
            "let p = \"C:\\\\piped\\\\stdin.rs\"; let c = '\\'';",
            "windows path",
        ),
        (
            "fn f<'a>(x: &'a str) { c.stdin(Stdio::null()); }",
            "lifetime",
        ),
        (
            "fn stdin_text() {} let stdin_bytes = 1;",
            "a name that contains stdin",
        ),
    ] {
        assert!(stdin_uses(source).is_empty(), "{what}: {source}");
    }
}

#[test]
fn the_scan_skips_test_items_but_reads_the_code_after_them() {
    let source = "fn a() { c.stdin(Stdio::null()); }\n\
        #[cfg(test)]\nmod tests { fn t() { y.stdin(Stdio::piped()); let w = z.stdin.take(); } }\n\
        #[cfg(all(test, unix))]\nfn only_in_tests() { y.stdin(Stdio::piped()); }\n\
        #[cfg(not(test))]\nfn later() { y.stdin(Stdio::piped()); }\n\
        fn after() { w.stdin.take(); }\n";
    let lines: Vec<usize> = stdin_uses(source)
        .into_iter()
        .map(|(line, _)| line)
        .collect();
    assert_eq!(lines, vec![7, 8]);
}
