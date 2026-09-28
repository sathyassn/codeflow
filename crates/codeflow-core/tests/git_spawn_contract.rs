//! Every `git` process codeflow starts is built by `codeflow_core::git`
//! (TSK-141 AC-4, SPC-013 R-85 as amended), so a hook git fires during a
//! codeflow command runs that same binary. A spawn written as
//! `Command::new("git")`, or as `Command::new(program)` where `program`
//! may hold git, would dispatch its hooks by PATH again.
//!
//! The scan reads each production source with comments and string contents
//! set aside, finds every `Command::new(...)` however it is spaced, and
//! judges its argument. A literal that names git (`git`, `git.exe`,
//! `/usr/bin/git` and the like) is refused. Any other argument that is not
//! a literal must be listed in [`DYNAMIC`] with the reason it never holds
//! git, so a new spawn built from a variable has to be looked at. Renaming
//! `Command` on import would hide a spawn from the scan, so it is refused.
//!
//! Not covered: a shell that runs git from its own script text, such as a
//! test target the runner starts through `sh -c`. That git is the user's
//! command, not one codeflow builds.

use std::path::{Path, PathBuf};

/// Spawns whose program is not a literal: (file under `crates/`, the
/// argument with whitespace removed, how many, why it never holds git).
const DYNAMIC: &[(&str, &str, usize, &str)] = &[
    (
        "codeflow-core/src/git/mod.rs",
        "program",
        1,
        "the git constructor itself",
    ),
    ("codeflow-core/src/remote.rs", "&self.gh", 1, "the gh client"),
    (
        "codeflow-core/src/remote.rs",
        "path",
        1,
        "a test's fake gh shim",
    ),
    (
        "codeflow-core/src/release_local.rs",
        "PYTHON",
        3,
        "the python3 interpreter",
    ),
    (
        "codeflow-core/src/testing/runner/mod.rs",
        "executable",
        1,
        "powershell.exe or pwsh",
    ),
    (
        "codeflow-core/src/testing/doctor/mod.rs",
        "cmd",
        1,
        "a fixed list of test-runner probes, none of them git",
    ),
    (
        "codeflow-core/src/scaffold/state.rs",
        "std::env::current_exe().unwrap()",
        1,
        "a test re-running its own binary",
    ),
    (
        "codeflow-cli/src/cmd/present.rs",
        "executable",
        3,
        "this codeflow binary, as the service and its test canaries",
    ),
    (
        "codeflow-cli/src/cmd/push_set.rs",
        "exe",
        1,
        "this codeflow binary",
    ),
    (
        "codeflow-present/src/platform.rs",
        "executable",
        2,
        "the restricted constructor (browser and /bin/ps) and a test re-run",
    ),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// One character of source and whether it is code (not a comment and not
/// inside a string or character literal).
struct Source {
    chars: Vec<char>,
    code: Vec<bool>,
}

impl Source {
    fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let mut code = vec![true; chars.len()];
        let mut i = 0;
        let mark = |code: &mut Vec<bool>, from: usize, to: usize| {
            for flag in &mut code[from..to.min(chars.len())] {
                *flag = false;
            }
        };
        while i < chars.len() {
            let rest = |n: usize| chars.get(i + n).copied();
            match chars[i] {
                '/' if rest(1) == Some('/') => {
                    let end = (i..chars.len())
                        .find(|&j| chars[j] == '\n')
                        .unwrap_or(chars.len());
                    mark(&mut code, i, end);
                    i = end;
                }
                '/' if rest(1) == Some('*') => {
                    let (mut depth, mut j) = (0_usize, i);
                    while j < chars.len() {
                        if chars[j] == '/' && chars.get(j + 1) == Some(&'*') {
                            depth += 1;
                            j += 2;
                        } else if chars[j] == '*' && chars.get(j + 1) == Some(&'/') {
                            depth -= 1;
                            j += 2;
                            if depth == 0 {
                                break;
                            }
                        } else {
                            j += 1;
                        }
                    }
                    mark(&mut code, i, j);
                    i = j;
                }
                'r' if !ident(i.checked_sub(1).map(|p| chars[p]))
                    && matches!(rest(1), Some('"' | '#')) =>
                {
                    let hashes = (i + 1..chars.len())
                        .take_while(|&j| chars[j] == '#')
                        .count();
                    let open = i + 1 + hashes;
                    if chars.get(open) != Some(&'"') {
                        i += 1;
                        continue;
                    }
                    let mut j = open + 1;
                    while j < chars.len()
                        && !(chars[j] == '"'
                            && (1..=hashes).all(|h| chars.get(j + h) == Some(&'#')))
                    {
                        j += 1;
                    }
                    mark(&mut code, open + 1, j);
                    i = j + 1 + hashes;
                }
                '"' => {
                    let mut j = i + 1;
                    while j < chars.len() && chars[j] != '"' {
                        j += if chars[j] == '\\' { 2 } else { 1 };
                    }
                    mark(&mut code, i + 1, j);
                    i = j + 1;
                }
                '\'' if rest(1) == Some('\\') => {
                    let end = (i + 2..chars.len())
                        .find(|&j| chars[j] == '\'')
                        .unwrap_or(chars.len());
                    mark(&mut code, i + 1, end);
                    i = end + 1;
                }
                '\'' if rest(2) == Some('\'') => {
                    mark(&mut code, i + 1, i + 2);
                    i += 3;
                }
                _ => i += 1,
            }
        }
        Self { chars, code }
    }

    /// The code text with whitespace removed, and for each character its
    /// index in `chars`.
    fn compact(&self) -> (String, Vec<usize>) {
        let mut text = String::new();
        let mut at = Vec::new();
        for (i, &c) in self.chars.iter().enumerate() {
            if !self.code[i] || !c.is_whitespace() {
                text.push(if self.code[i] { c } else { '\u{0}' });
                at.push(i);
            }
        }
        (text, at)
    }
}

fn ident(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// The argument of each `Command::new(...)` in `text`, whitespace removed,
/// with the literal's value when the argument is one string literal.
fn spawns(text: &str) -> Vec<(String, Option<String>)> {
    let source = Source::new(text);
    let (compact, at) = source.compact();
    let marker = "Command::new(";
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = compact[from..].find(marker) {
        let start = from + offset;
        from = start + marker.len();
        let before = at[start].checked_sub(1).map(|i| source.chars[i]);
        if ident(before) {
            continue;
        }
        let open = at[start + marker.len() - 1];
        let mut depth = 0_usize;
        let mut close = open;
        for (j, &c) in source.chars.iter().enumerate().skip(open) {
            if !source.code[j] {
                continue;
            }
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = j;
                        break;
                    }
                }
                _ => {}
            }
        }
        let argument: String = source.chars[open + 1..close]
            .iter()
            .filter(|c| !c.is_whitespace())
            .collect();
        found.push((argument.clone(), literal(&argument)));
    }
    found
}

/// The value of `argument` when it is exactly one string literal.
fn literal(argument: &str) -> Option<String> {
    let body = argument.strip_prefix('r').unwrap_or(argument);
    let hashes = body.chars().take_while(|&c| c == '#').count();
    let inner = body[hashes..].strip_prefix('"')?;
    let inner = inner.strip_suffix(&"#".repeat(hashes))?.strip_suffix('"')?;
    Some(inner.replace("\\\\", "\\"))
}

/// Whether a program name runs git: its last path part, without an
/// extension, is `git`.
fn names_git(program: &str) -> bool {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let stem = name.split('.').next().unwrap_or(name);
    stem.eq_ignore_ascii_case("git")
}

/// Why `text`, read as the file `file`, breaks the contract.
fn violations(file: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (compact, _) = Source::new(text).compact();
    if compact.contains("Commandas") {
        out.push(format!(
            "{file}: renames Command on import, which hides its spawns"
        ));
    }
    let mut dynamic: Vec<(String, usize)> = Vec::new();
    for (argument, value) in spawns(text) {
        match value {
            Some(program) if names_git(&program) => out.push(format!(
                "{file}: Command::new({argument}) starts git; use codeflow_core::git"
            )),
            Some(_) => {}
            None => match dynamic.iter_mut().find(|(a, _)| *a == argument) {
                Some((_, n)) => *n += 1,
                None => dynamic.push((argument, 1)),
            },
        }
    }
    for (argument, count) in dynamic {
        let listed = DYNAMIC
            .iter()
            .find(|(f, a, _, _)| *f == file && *a == argument);
        match listed {
            Some((_, _, expected, _)) if *expected == count => {}
            Some((_, _, expected, _)) => out.push(format!(
                "{file}: Command::new({argument}) appears {count} times, listed {expected}"
            )),
            None => out.push(format!(
                "{file}: Command::new({argument}) builds a program from a value; \
                 spawn git through codeflow_core::git, or list it in DYNAMIC with \
                 the reason it never holds git"
            )),
        }
    }
    out
}

#[test]
fn every_git_process_is_built_by_the_one_constructor() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for krate in ["codeflow-core", "codeflow-cli", "codeflow-present"] {
        rust_files(&crates.join(krate).join("src"), &mut files);
    }
    assert!(
        files.contains(&crates.join("codeflow-core/src/git/mod.rs")),
        "the constructor moved"
    );
    let mut offenders = Vec::new();
    let mut seen = Vec::new();
    for file in &files {
        let name = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(file).unwrap();
        offenders.extend(violations(&name, &text));
        seen.extend(
            spawns(&text)
                .into_iter()
                .filter(|(_, value)| value.is_none())
                .map(|(argument, _)| (name.clone(), argument)),
        );
    }
    for (file, argument, _, _) in DYNAMIC {
        if !seen.iter().any(|(f, a)| f == file && a == argument) {
            offenders.push(format!(
                "{file}: DYNAMIC lists Command::new({argument}), which is gone"
            ));
        }
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

#[test]
fn the_scan_sees_every_way_a_git_spawn_is_written() {
    let refused = [
        r#"let c = Command::new("git");"#,
        "let c = std::process::Command\n    ::new(\n        \"git\"\n    );",
        r#"let c = Command::new("/usr/bin/git");"#,
        r#"let c = Command::new("git.exe");"#,
        r#"let c = Command::new(r"C:\Program Files\Git\cmd\git.exe");"#,
        "let c = Command::new(r#\"GIT\"#);",
        "const GIT: &str = \"git\"; let c = Command::new(GIT);",
        "let program = pick(); let c = Command::new(program);",
        "let c = match Command::new(program) {};",
        "use std::process::Command as Spawn;",
    ];
    for source in refused {
        assert!(
            !violations("probe.rs", source).is_empty(),
            "the scan missed: {source}"
        );
    }
    let passed = [
        r#"let c = Command::new("gh");"#,
        r#"let c = Command::new("gitleaks");"#,
        r#"// Command::new("git")"#,
        r#"/* Command::new("git") */"#,
        r#"let s = "Command::new(\"git\")";"#,
        r#"let s = r"Command::new(program)";"#,
        r#"let c = MyCommand::new("git");"#,
        r#"let c = match Command::new("sh").status() {};"#,
        "let q = '\"'; let c = Command::new(\"sh\");",
    ];
    for source in passed {
        assert!(
            violations("probe.rs", source).is_empty(),
            "the scan refused: {source}: {:?}",
            violations("probe.rs", source)
        );
    }
}
