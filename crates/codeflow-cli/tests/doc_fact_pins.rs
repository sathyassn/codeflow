//! The counts the guide states that drift when the binary grows: how many
//! subcommands `codeflow --help` lists and how many checks `codeflow doctor`
//! runs. Every such statement in the live guide (the `docs/` pages outside the
//! decision, plan and verification records, the README and the portal's
//! figure declarations) must equal what the built binary reports, whether it
//! is written in digits or in words.

use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn codeflow(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "codeflow {args:?} failed");
    String::from_utf8(output.stdout).unwrap()
}

/// The subcommands `codeflow --help` lists, without clap's own `help`.
fn subcommand_count() -> usize {
    let help = codeflow(&["--help"]);
    let (_, commands) = help.split_once("Commands:\n").unwrap();
    commands
        .lines()
        .take_while(|line| line.starts_with("  "))
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| *name != "help")
        .count()
}

fn doctor_check_count() -> usize {
    codeflow(&["doctor", "--list"])
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

/// A count written in digits or as an English number word up to ninety-nine.
fn parse_count(text: &str) -> Option<usize> {
    const ONES: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 8] = [
        "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    let text = text.to_lowercase();
    if let Ok(number) = text.parse() {
        return Some(number);
    }
    let word = |w: &str| ONES.iter().position(|one| *one == w);
    let tens = |w: &str| TENS.iter().position(|ten| *ten == w).map(|i| (i + 2) * 10);
    match text.split_once('-') {
        Some((high, low)) => Some(tens(high)? + word(low)?),
        None => word(&text).or_else(|| tens(&text)),
    }
}

fn collect(dir: &Path, extension: &str, skip: &[&str], files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let relative = path.strip_prefix(repository()).unwrap().to_string_lossy();
        if skip.iter().any(|prefix| relative.starts_with(prefix)) {
            continue;
        }
        if path.is_dir() {
            collect(&path, extension, skip, files);
        } else if path.extension().is_some_and(|ext| ext == extension) {
            files.push(path);
        }
    }
}

/// The live guide: its pages, the README and the figure declarations.
fn corpus() -> Vec<(String, String)> {
    let root = repository();
    let mut files = vec![root.join("README.md")];
    collect(
        &root.join("docs"),
        "md",
        &["docs/decisions", "docs/plan", "docs/verification"],
        &mut files,
    );
    collect(&root.join("docs-portal/figures"), "json", &[], &mut files);
    files
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            let relative = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            // Fold line breaks so a count wrapped onto the next line is read.
            (
                relative,
                text.split_whitespace().collect::<Vec<_>>().join(" "),
            )
        })
        .collect()
}

const NUMBER: &str = r"(\d+|[a-z]+(?:-[a-z]+)?)";

/// Every count a pattern captures in the corpus, as (file, phrase, count).
fn stated(pattern: &str, only: Option<&[&str]>) -> Vec<(String, String, Option<usize>)> {
    let regex = Regex::new(&format!("(?i){}", pattern.replace("{N}", NUMBER))).unwrap();
    let mut found = Vec::new();
    for (file, text) in corpus() {
        if only.is_some_and(|prefixes| !prefixes.iter().any(|p| file.starts_with(p))) {
            continue;
        }
        for capture in regex.captures_iter(&text) {
            let phrase = capture[0].to_string();
            let count = parse_count(&capture[1]);
            // Words that are not numbers ("the named checks") are not counts.
            if count.is_some() || capture[1].chars().all(|c| c.is_ascii_digit()) {
                found.push((file.clone(), phrase, count));
            }
        }
    }
    found
}

fn assert_counts(what: &str, actual: usize, statements: &[(String, String, Option<usize>)]) {
    assert!(
        !statements.is_empty(),
        "no statement of the {what} count was found; the pin has gone vacuous"
    );
    let wrong: Vec<String> = statements
        .iter()
        .filter(|(_, _, count)| *count != Some(actual))
        .map(|(file, phrase, _)| format!("{file}: \"{phrase}\""))
        .collect();
    assert!(
        wrong.is_empty(),
        "the binary has {actual} {what}, but the guide says otherwise:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn the_guide_states_the_subcommand_count_the_binary_has() {
    let statements = stated(r"\b{N} subcommands\b", None);
    assert_counts("subcommands", subcommand_count(), &statements);
}

#[test]
fn the_guide_states_the_doctor_check_count_the_binary_has() {
    let mut statements = stated(r"\b{N} health checks\b", None);
    statements.extend(stated(r"\({N} checks:", None));
    // On the doctor's own pages, "the nineteen checks" means all of them.
    statements.extend(stated(
        r"\bthe {N} checks\b",
        Some(&[
            "docs/troubleshooting.md",
            "docs-portal/figures/troubleshooting/",
        ]),
    ));
    assert_counts("doctor checks", doctor_check_count(), &statements);
}

#[test]
fn counts_are_read_in_digits_and_words() {
    assert_eq!(parse_count("19"), Some(19));
    assert_eq!(parse_count("Nineteen"), Some(19));
    assert_eq!(parse_count("twenty-six"), Some(26));
    assert_eq!(parse_count("thirty"), Some(30));
    assert_eq!(parse_count("named"), None);
}
