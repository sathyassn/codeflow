//! The differential for certified prose (TSK-233, issue 66).
//!
//! `evaluate_at` is the 3.0.0 judgment: the raw privilege, headless and
//! dangerous rules, byte for byte as on `origin/main`. `evaluate_in` with
//! `posix = true` adds the certified-prose exemption. Over a corpus of launcher
//! lines (every launcher and variant, every separator, head and tail the
//! guard's history has shown), prose-shaped lines, mutated and spliced lines
//! and quoted launcher words, the two must agree on every line the tokenizer
//! does not certify, and on a certified line the new verdict may only drop
//! privilege, headless-peer or dangerous-command findings, never add one.

use std::collections::BTreeSet;
use std::path::Path;

use codeflow_core::hooks::exec_guard::{evaluate_at, evaluate_in};
use codeflow_core::hooks::policy::{PolicyLevel, SecuritySection};
use codeflow_core::security::prose::{certify_with_writes, writes_are_plain_files};

const LAUNCHERS: &[&str] = &["sudo", "su", "doas", "pkexec", "runuser"];
const VARIANTS: &[&str] = &[
    "sudoedit",
    "sudoreplay",
    "sudo-rs",
    "su-exec",
    "su-to-root",
    "sux",
    "super",
    "doasedit",
];
const SEPS: &[&str] = &[
    "; ", ";", ";  ", ";\t", "&& ", "&&", "|| ", "| ", "|", "\n", " ;", "&",
];
const TAILS: &[&str] = &[
    "", " ", " -", " -i", " id", " sh", "\t-", "\t", "\n", "\r\n", ";", "&", "&&", "|", ")",
    "(id)", "<in", ">out", "''", "\"\"", "'", "\"", "`", "$IFS-", "${IFS}-", "\\\n-", "{,}", "??",
    "[d]o", ".", ".exe", ",", ":", "/x", "@(do)", "+(do)", "^x", "%", "]", "#", "!(x)", "a#", "1#",
    "_#", "-#", "a#b#", "a#x", " -c 'id'", " root", " -u x id", "$(id)", "=1", "*", "?", "[a]",
    "#x", "!", "}", "~",
];
const HEADS: &[&str] = &["true", "cd /tmp", "echo ok && true", "(true"];
const PREFIX_WORDS: &[&str] = &[
    "supersedes",
    "summary",
    "such",
    "such as",
    "subtotal",
    "suit",
    "sue",
    "suffix",
    "doasync",
    "doasx",
    "sudoku",
    "sudoers",
    "pkexecute",
    "pkexec2",
    "runusers",
    "runuser2",
    "su-like",
    "su.",
    "su,",
    "su:",
    "su/x",
    "su_x",
    "su1",
    "sudo-x",
    "sudo.",
    "sudo_x",
    "sudo/",
    "doas.",
    "doas-x",
];
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
const MUTATIONS: &str = "\\'\"`$()[]{}<>|&;*?~!#=%@ \t\n\r\u{a0}\u{0}";
const OTHER: &[&str] = &[
    "{l} -i",
    "{l}",
    "echo x && {l} id",
    "false || {l} id",
    "echo x | {l} tee f",
    "bash -c '{l} id'",
    "eval {l} id",
    "timeout 1 {l} true",
    "/usr/bin/{l} id",
    "env {l} id",
    "command {l} id",
    "x=1 {l} id",
    "ls\n{l} id",
    "ls; /usr/bin/{l} id",
];
const QUOTED: &[&str] = &[
    "echo '{w}'",
    "echo 'a; {w} b'",
    "grep -n '; {w}' f",
    "cat <<'EOF'\n; {w}\nEOF",
    "printf '%s' 'x && {w} y' > n.md",
    "echo a; {w}",
    "echo '{w}'; {w}",
];
const QUOTED_WORDS: &[&str] = &[
    "su",
    "sudo",
    "doas",
    "pkexec",
    "runuser",
    "runas",
    "mkfs",
    "claude -p",
    "codex exec",
    "rm -rf /",
];

fn corpus() -> BTreeSet<String> {
    let mut cases = BTreeSet::new();
    for head in HEADS {
        for sep in SEPS {
            for launcher in LAUNCHERS {
                for tail in TAILS {
                    cases.insert(format!("{head}{sep}{launcher}{tail}"));
                }
            }
            for variant in VARIANTS {
                for tail in ["", " -i", " /etc/hosts", "\t-"] {
                    cases.insert(format!("{head}{sep}{variant}{tail}"));
                }
            }
            for word in PREFIX_WORDS {
                cases.insert(format!("{head}{sep}{word}"));
                cases.insert(format!("echo '{head}{sep}{word} more text'"));
            }
        }
    }
    for launcher in LAUNCHERS {
        for template in OTHER {
            cases.insert(template.replace("{l}", launcher));
        }
    }
    for word in QUOTED_WORDS {
        for template in QUOTED {
            cases.insert(template.replace("{w}", word));
        }
    }
    for seed in SEEDS {
        cases.insert(seed.to_string());
        for second in SEEDS {
            for sep in [" ; ", "\n", " && ", " || ", " | ", ";", " ", ""] {
                cases.insert(format!("{seed}{sep}{second}"));
            }
        }
        let chars: Vec<char> = seed.chars().collect();
        for at in 0..=chars.len() {
            for c in MUTATIONS.chars() {
                let mut inserted = chars.clone();
                inserted.insert(at, c);
                cases.insert(inserted.into_iter().collect());
                if at < chars.len() {
                    let mut replaced = chars.clone();
                    replaced[at] = c;
                    cases.insert(replaced.into_iter().collect());
                }
            }
        }
    }
    cases
}

fn verdict(
    command: &str,
    posix: bool,
    cwd: &Path,
    levels: &SecuritySection,
) -> BTreeSet<(String, String)> {
    evaluate_in(command, posix, levels, PolicyLevel::Block, cwd, cwd)
        .into_iter()
        .map(|violation| (violation.rule, violation.message))
        .collect()
}

/// One line: the 3.0.0 verdict against the new one. Returns whether the line
/// was certified and whether its verdict changed.
fn compare(command: &str, cwd: &Path, levels: &SecuritySection) -> (bool, bool) {
    const RAW: [&str; 3] = [
        "security.privilege_escalation",
        "security.headless_peer_runs",
        "security.dangerous_commands",
    ];
    let floor = verdict(command, false, cwd, levels);
    let new = verdict(command, true, cwd, levels);
    let certified =
        certify_with_writes(command).is_some_and(|c| writes_are_plain_files(&c.writes, cwd));
    if !certified {
        assert_eq!(new, floor, "an uncertified line changed: {command:?}");
        return (false, false);
    }
    assert!(
        new.is_subset(&floor),
        "a certified line gained a finding: {command:?}"
    );
    for (rule, _) in floor.difference(&new) {
        assert!(RAW.contains(&rule.as_str()), "dropped {rule}: {command:?}");
    }
    (true, new != floor)
}

#[test]
fn only_certified_lines_change_and_they_only_lose_the_three_raw_findings() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();
    let levels = SecuritySection::default();
    // `evaluate_at` is the 3.0.0 judgment and `posix = false` is the same code.
    assert_eq!(
        verdict("true; su -", false, cwd, &levels),
        evaluate_at("true; su -", &levels, PolicyLevel::Block, cwd, cwd)
            .into_iter()
            .map(|violation| (violation.rule, violation.message))
            .collect()
    );
    // Every fortieth line by default; `CODEFLOW_ORACLE_FULL=1` runs them all.
    let stride = if std::env::var_os("CODEFLOW_ORACLE_FULL").is_some() {
        1
    } else {
        40
    };
    let cases: Vec<String> = corpus()
        .into_iter()
        .enumerate()
        .filter_map(|(index, line)| (index % stride == 0).then_some(line))
        .collect();
    assert!(
        cases.len() > 10_000 / stride,
        "corpus has {} lines",
        cases.len()
    );
    let workers = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let (certified, changed) = std::thread::scope(|scope| {
        let handles: Vec<_> = cases
            .chunks(cases.len().div_ceil(workers))
            .map(|chunk| {
                let levels = &levels;
                scope.spawn(move || {
                    chunk
                        .iter()
                        .fold((0usize, 0usize), |(certified, changed), line| {
                            let (is_certified, is_changed) = compare(line, cwd, levels);
                            (
                                certified + usize::from(is_certified),
                                changed + usize::from(is_changed),
                            )
                        })
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .fold((0, 0), |(a, b), (c, d)| (a + c, b + d))
    });
    eprintln!(
        "prose differential: {} lines, {certified} certified, {changed} changed",
        cases.len()
    );
    assert!(changed > 5, "the corpus changed only {changed} verdicts");
}
