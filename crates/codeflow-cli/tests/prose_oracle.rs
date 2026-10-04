//! The shell oracle for certified prose (TSK-233, issue 66).
//!
//! `security::prose::certify` is a strict tokenizer: when it certifies a line,
//! the line must run nothing but `echo`, `printf`, `grep` and `cat`, in every
//! shell. This test checks that claim against real shells and real programs.
//! It builds lines from the grammar, mutates each by inserting, replacing and
//! deleting every printable ASCII character, whitespace form, control character
//! and a few non-ASCII characters at every position, splices seed lines with
//! every separator, and for each line `certify` accepts it runs the line in
//! bash (`extglob` and `expand_aliases` on, both Homebrew's and the system
//! bash when present) and zsh (`extendedglob` on). The real `echo`, `printf`,
//! `grep` and `cat` run. Every other program the line could reach (privilege
//! launchers and their variants, headless peers, destructive commands, shells,
//! interpreters, `git`) is a logging stub earlier on `PATH`, in a directory
//! that also holds a plain file named after each of them. A certified line must
//! call no stub and create no file except its redirect targets.
//!
//! The stubs only record that they ran. Nothing here runs a real launcher.
#![cfg(unix)]

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use codeflow_core::security::prose::certify;

/// Programs a mutated line could reach by name; each is a logging stub.
const STUBS: &[&str] = &[
    "sudo",
    "su",
    "doas",
    "pkexec",
    "runuser",
    "gsudo",
    "runas",
    "sudoedit",
    "sudoreplay",
    "sudo-rs",
    "su-exec",
    "su-to-root",
    "sux",
    "super",
    "doasedit",
    "claude",
    "codex",
    "grok",
    "mkfs",
    "mkfs.ext4",
    "dd",
    "chmod",
    "chown",
    "rm",
    "env",
    "sh",
    "bash",
    "zsh",
    "xargs",
    "find",
    "git",
    "gh",
    "awk",
    "sed",
    "tee",
    "python3",
    "node",
    "perl",
    "ruby",
    "rg",
    "ls",
    "touch",
    "mv",
    "cp",
    "ln",
    "curl",
    "open",
    "osascript",
];

/// A shell found on this machine, with the lines that switch on its pattern
/// features.
struct Shell {
    program: String,
    prelude: &'static str,
}

/// The absolute path of `program` on the real `PATH`, so a stub of the same
/// name on the oracle's `PATH` is never picked.
fn locate(program: &str) -> Option<String> {
    if program.starts_with('/') {
        return Path::new(program).exists().then(|| program.to_string());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

fn shells() -> Vec<Shell> {
    let mut found: Vec<Shell> = Vec::new();
    for (program, prelude) in [
        ("bash", "shopt -s extglob expand_aliases\n"),
        ("/bin/bash", "shopt -s extglob expand_aliases\n"),
        ("zsh", "setopt extendedglob\n"),
    ] {
        let Some(program) = locate(program) else {
            continue;
        };
        if found.iter().any(|shell| shell.program == program) {
            continue;
        }
        if Command::new(&program)
            .arg("-c")
            .arg("true")
            .output()
            .is_ok_and(|out| out.status.success())
        {
            found.push(Shell { program, prelude });
        }
    }
    found
}

/// The sandbox directory: stubs in `bin`, decoys in `work`, stub calls in
/// `logs`.
struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["bin", "tmp", "logs"] {
            fs::create_dir(root.path().join(dir)).unwrap();
        }
        let logs = root.path().join("logs");
        for name in STUBS {
            let path = root.path().join("bin").join(name);
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s\\0' \"{name}\" \"$@\" > '{}/'$$\n",
                    logs.display()
                ),
            )
            .unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { root }
    }

    fn work(&self) -> PathBuf {
        self.root.path().join("work")
    }

    /// A clean working directory holding a plain file named after every stub.
    fn reset(&self) {
        let work = self.work();
        let _ = fs::remove_dir_all(&work);
        fs::create_dir(&work).unwrap();
        for name in STUBS {
            fs::write(work.join(name), "decoy").unwrap();
        }
        let logs = self.root.path().join("logs");
        let _ = fs::remove_dir_all(&logs);
        fs::create_dir(&logs).unwrap();
    }

    fn files(&self) -> BTreeSet<String> {
        fs::read_dir(self.work())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    /// Run `line` in `shell`; the stub calls it made.
    fn run(&self, shell: &Shell, line: &str) -> Vec<Vec<String>> {
        self.reset();
        let script = self.root.path().join("script.sh");
        let tmp = self.root.path().join("tmp");
        fs::write(
            &script,
            format!("{}TMPPREFIX={}/zsh\n{line}\n", shell.prelude, tmp.display()),
        )
        .unwrap();
        let path = format!("{}:/usr/bin:/bin", self.root.path().join("bin").display());
        let mut child = Command::new(&shell.program)
            .arg(&script)
            .current_dir(self.work())
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.work())
            .env("TMPDIR", &tmp)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // A certified `grep -r` or `cat` may read a huge tree or a device when a
        // mutation turns a path into `/`; it executes nothing, so stop it.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        fs::read_dir(self.root.path().join("logs"))
            .unwrap()
            .map(|entry| {
                let record = fs::read(entry.unwrap().path()).unwrap();
                record
                    .split(|byte| *byte == 0)
                    .map(|word| String::from_utf8_lossy(word).into_owned())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

/// A file a certified line may create: a document file, by extension.
fn is_document(name: &str) -> bool {
    ["md", "markdown", "txt", "rst", "log"]
        .iter()
        .any(|extension| {
            name.strip_suffix(extension)
                .is_some_and(|stem| stem.ends_with('.') && stem.len() > 1)
        })
}

const SEEDS: &[&str] = &[
    "echo hi",
    "echo 'a; sudo id && su - | doas sh'",
    "echo \"(...; supersedes A3's ...)\"",
    "printf '%s\\n' 'first; such as this' > note.md",
    "printf '%s %s%%' a b c",
    "grep -n '; su' file.txt",
    "grep -rn -i 'chained su' docs/ | cat",
    "grep -c x f && echo ok; echo done",
    "cat <<'EOF' > a.md\nA3 (supersedes; summary below)\nEOF",
    "cat <<\"EOF\"\n$(sudo id)\n`su -`\nEOF\n",
    "cat file.md | grep 'x y' | cat",
    "echo a >> log.txt 2>&1",
    "echo err >&2",
    "echo a || echo b",
    "echo a\necho b",
    "cat a.md b.md > c.md",
    "echo a 2> err.log",
];

/// Characters inserted or substituted at every position: the shell's own
/// metacharacters, expansion and glob characters, quotes, every whitespace and
/// control form, and a few non-ASCII characters.
fn mutation_chars() -> Vec<char> {
    let mut chars: Vec<char> = "\\'\"`$()[]{}<>|&;*?~!#=%@^+,:/-.aAsS0".chars().collect();
    chars.extend([
        ' ', '\t', '\n', '\r', '\u{b}', '\u{0}', '\u{a0}', '\u{2003}', 'é', '\u{200b}',
    ]);
    chars
}

fn mutate(seed: &str) -> Vec<String> {
    let chars: Vec<char> = seed.chars().collect();
    let inserts = mutation_chars();
    let mut out = Vec::new();
    for at in 0..=chars.len() {
        for c in &inserts {
            let mut inserted = chars.clone();
            inserted.insert(at, *c);
            out.push(inserted.iter().collect());
            if at < chars.len() {
                let mut replaced = chars.clone();
                replaced[at] = *c;
                out.push(replaced.iter().collect());
            }
        }
        if at < chars.len() {
            let mut deleted = chars.clone();
            deleted.remove(at);
            out.push(deleted.iter().collect());
        }
    }
    out
}

/// Seed lines joined pairwise with every separator the grammar knows.
fn spliced() -> Vec<String> {
    let mut out = Vec::new();
    for first in SEEDS {
        for second in SEEDS {
            for sep in [" ; ", "\n", " && ", " || ", " | ", ";", " ", ""] {
                out.push(format!("{first}{sep}{second}"));
            }
        }
    }
    out
}

/// Every line to try that `certify` accepts, without duplicates.
fn certified_lines() -> BTreeSet<String> {
    let mut lines: BTreeSet<String> = BTreeSet::new();
    let candidates = SEEDS
        .iter()
        .map(ToString::to_string)
        .chain(spliced())
        .chain(SEEDS.iter().flat_map(|seed| mutate(seed)));
    for line in candidates {
        if certify(&line).is_some() {
            lines.insert(line);
        }
    }
    lines
}

fn check(sandbox: &Sandbox, shells: &[Shell], line: &str) -> Result<(), String> {
    let allowed: BTreeSet<String> = STUBS.iter().map(ToString::to_string).collect();
    for shell in shells {
        let calls = sandbox.run(shell, line);
        if !calls.is_empty() {
            return Err(format!(
                "{}: certified line ran {calls:?}: {line:?}",
                shell.program
            ));
        }
        let files = sandbox.files();
        let extra: Vec<_> = files
            .difference(&allowed)
            .filter(|name| !is_document(name))
            .collect();
        if !extra.is_empty() {
            return Err(format!(
                "{}: certified line created {extra:?}: {line:?}",
                shell.program
            ));
        }
    }
    Ok(())
}

#[test]
fn certified_lines_run_only_the_four_programs_in_bash_and_zsh() {
    let shells = shells();
    assert!(
        !shells.is_empty(),
        "no bash or zsh found to run the oracle against"
    );
    // Every fifth certified line by default; `CODEFLOW_ORACLE_FULL=1` runs all.
    let stride = if std::env::var_os("CODEFLOW_ORACLE_FULL").is_some() {
        1
    } else {
        5
    };
    let lines: Vec<String> = certified_lines()
        .into_iter()
        .enumerate()
        .filter_map(|(index, line)| (index % stride == 0).then_some(line))
        .collect();
    eprintln!("oracle: {} certified lines", lines.len());
    assert!(
        lines.len() > SEEDS.len() * 4,
        "the oracle exercised only {} certified lines",
        lines.len()
    );
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZero::get)
        .min(8);
    let failures: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = lines
            .chunks(lines.len().div_ceil(workers))
            .map(|chunk| {
                let shells = &shells;
                scope.spawn(move || {
                    let sandbox = Sandbox::new();
                    chunk
                        .iter()
                        .filter_map(|line| check(&sandbox, shells, line).err())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect()
    });
    assert!(
        failures.is_empty(),
        "{} of {} certified lines ran something else:\n{}",
        failures.len(),
        lines.len(),
        failures
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Names a shell can reach through a pattern: every launcher, peer and
/// destructive command with each character, and each run of characters,
/// replaced by a wildcard or expansion, as a command word on a line the
/// tokenizer sees. None is certified.
#[test]
fn a_wildcard_inside_a_dangerous_name_is_never_certified() {
    let wildcards = [
        "?", "*", "[x]", "{a,b}", "@(a)", "+(a)", "!(a)", "#", "(#c0,1)", "(e:'x':)", "$x", "`x`",
        "\\x", "''", "\"\"", "~",
    ];
    for name in STUBS {
        let chars: Vec<char> = name.chars().collect();
        for from in 0..=chars.len() {
            for to in from..=chars.len() {
                for wildcard in wildcards {
                    let word: String = chars[..from]
                        .iter()
                        .copied()
                        .chain(wildcard.chars())
                        .chain(chars[to..].iter().copied())
                        .collect();
                    for line in [
                        format!("true; {word} -n id"),
                        format!("echo a; {word}"),
                        format!("{word} -n id"),
                        format!("echo a && {word} x | cat"),
                        format!("cat <<'EOF'\nx\nEOF\n{word} id"),
                    ] {
                        assert!(
                            certify(&line).is_none(),
                            "certified a line with a command word: {line:?}"
                        );
                    }
                }
            }
        }
    }
}

/// A certified line may write a document file, so it may write a file whose
/// text holds launcher words; the same line with any other target, or a path
/// that is not a document, is not certified.
#[test]
fn only_document_files_may_be_written_by_a_certified_line() {
    for line in [
        "echo 'true; sudo id' > a.md",
        "printf '%s' 'x' >> notes/b.txt",
        "cat <<'EOF' > c.markdown\nx\nEOF",
        "echo a 2> d.log",
        "echo a > e.rst",
    ] {
        assert!(certify(line).is_some(), "{line:?}");
    }
    for line in [
        "echo 'true; sudo id' > a.sh",
        "echo x > a",
        "echo x > a.md.sh",
        "echo x > .md",
        "echo x > a.MD",
        "echo x > /dev/null",
        "echo x > ../a.py",
        "cat <<'EOF' > run\nx\nEOF",
    ] {
        assert!(certify(line).is_none(), "{line:?}");
    }
}
