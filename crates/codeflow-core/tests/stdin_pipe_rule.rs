//! A child's stdin is piped in one place: `crates/codeflow-core/src/git/stdin.rs`
//! (issue 71, TSK-237). A caller that wrote stdin itself, or piped it and
//! drained one output stream at a time, could deadlock against a child that
//! answers while it reads. This scan fails when a production `.stdin(...)`
//! call that pipes appears anywhere else in the workspace crates, so the rule
//! holds as code is added.

use std::path::{Path, PathBuf};

/// The only source file allowed to pipe a child's stdin.
const HELPER: &str = "crates/codeflow-core/src/git/stdin.rs";

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

/// The text before the file's trailing test module.
fn production(text: &str) -> &str {
    let cut = text
        .match_indices("\n#[cfg(test)]\n")
        .find(|(at, marker)| text[at + marker.len()..].trim_start().starts_with("mod "))
        .map_or(text.len(), |(at, _)| at);
    &text[..cut]
}

/// Every `.stdin(...)` call in `text` whose arguments pipe, and every direct
/// take of a child's stdin handle, as (line, text).
fn piped_stdin_calls(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices(".stdin(") {
        let open = at + ".stdin".len();
        let mut depth = 0_usize;
        let mut end = text.len();
        for (offset, byte) in text[open..].bytes().enumerate() {
            match byte {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + offset + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let call = &text[at..end];
        if call.contains("piped") {
            let line = text[..at].matches('\n').count() + 1;
            found.push((line, call.split_whitespace().collect::<Vec<_>>().join(" ")));
        }
    }
    for (at, _) in text.match_indices("stdin.take()") {
        let line = text[..at].matches('\n').count() + 1;
        found.push((line, "stdin.take()".to_string()));
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
        let text = std::fs::read_to_string(&file).unwrap();
        for (line, call) in piped_stdin_calls(production(&text)) {
            if relative != HELPER {
                offenders.push(format!("{relative}:{line}: {call}"));
            }
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
fn the_scan_reads_piped_stdin_calls() {
    let sample = "a.stdin(Stdio::null());\nb\n    .stdin(\n        if x { Stdio::piped() } else { Stdio::null() },\n    )\n    .spawn();\nc.stdin(std::process::Stdio::piped());";
    let found = piped_stdin_calls(sample);
    assert_eq!(
        found.iter().map(|(line, _)| *line).collect::<Vec<_>>(),
        vec![3, 7]
    );
    let split = "fn a() { x.stdin(Stdio::piped()); }\n#[cfg(test)]\nmod tests { fn t() { y.stdin(Stdio::piped()); } }\n";
    assert_eq!(piped_stdin_calls(production(split)).len(), 1);
}
