//! TSK-147 F4: `scripts/release.py` and `codeflow ci` judge a PR body's
//! Release impact block alike, over a seeded corpus of Markdown structures,
//! not only the hand-picked cases in `scripts/fixtures/release_impact_cases.json`.
//!
//! Both read the body with one parser, this binary
//! (`codeflow ci --read-release-impact`); release.py adds its own rules to
//! the fields it reads. The corpus keeps the two verdicts, and the fields
//! each reports, equal. Before the readers were one, this corpus compared
//! release.py's own Markdown reader with this one; that reader's readings of
//! these 5,000 bodies equal the binary's (TSK-147 round 4 record).

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

/// `SplitMix64`: a tiny seeded generator, so the differential corpus is
/// the same on every run without a new dependency.
struct Corpus(u64);

impl Corpus {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound).unwrap()).unwrap()
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

const CORPUS_SEED: u64 = 0x0147_F4D1_FFE2_0003;
const CORPUS_SIZE: usize = 5000;
const FIELDS: [(&str, &str); 6] = [
    ("Unit", "codeflow"),
    ("Impact", "minor"),
    ("Breaking", "no"),
    ("Rationale", "Adds a function preserving all callers."),
    ("Migration", "none"),
    ("Evidence", "cargo test passed."),
];
const INDENTS: [&str; 14] = [
    "", "", "", "", " ", "  ", "   ", "    ", "     ", "      ", "        ", "\t", " \t", "\t\t",
];
const MARKERS: [&str; 16] = [
    "", "", "", "- ", "* ", "+ ", "1. ", "1) ", "2. ", "10. ", "100) ", "1000. ", "-\t", "1.  ",
    "-     ", "1000)\t",
];

/// A block that can stand before, between or after the field lines:
/// lists with one- to four-digit ordinals, prose, quotes, fences,
/// comments in every position, breaks, HTML containers and indented
/// code, some carrying a decoy `Impact` line, all at a random indent.
fn corpus_block(rng: &mut Corpus) -> String {
    let text = match rng.below(20) {
        0 => format!("{} example\n", rng.pick(&["-", "*", "+"])),
        1 => format!(
            "{}{} example\n",
            rng.pick(&["1", "2", "10", "100", "1000"]),
            rng.pick(&[".", ")"])
        ),
        2 => "- outer\n  - inner\n".into(),
        3 => "1000. outer\n      - inner\n".into(),
        4 => "Outside prose.\n".into(),
        5 => "> quote\n".into(),
        6 => "> - Impact: patch\n".into(),
        7 => format!("{f}\ncode\n{f}\n", f = rng.pick(&["```", "~~~", "````"])),
        8 => format!("{f}\n- Impact: patch\n{f}\n", f = rng.pick(&["```", "~~~"])),
        9 => "```\n".into(),
        10 => "<!-- comment -->\n".into(),
        11 => "<!--\nnote\n-->\n".into(),
        12 => "<!-- note\n- Impact: patch\n-->\n".into(),
        13 => "<!-- note --> Outside prose.\n".into(),
        14 => rng.pick(&["***\n", "---\n", "___\n"]).into(),
        15 => "<details>\n<summary>More</summary>\n".into(),
        16 => "    - Impact: patch\n".into(),
        17 => "Prose then <!-- a\nnote --> more prose.\n".into(),
        18 => "-\n".into(),
        _ => "\n".into(),
    };
    if !rng.chance(30) {
        return text;
    }
    let indent = rng.pick(&INDENTS);
    let mut indented = String::new();
    for line in text.lines() {
        indented.push_str(indent);
        indented.push_str(line);
        indented.push('\n');
    }
    indented
}

fn corpus_field(rng: &mut Corpus, (key, value): (&str, &str), group: (&str, &str)) -> String {
    let (indent, marker) = if rng.chance(75) {
        group
    } else {
        (rng.pick(&INDENTS), rng.pick(&MARKERS))
    };
    let quote = if rng.chance(4) { "> " } else { "" };
    let line = match rng.below(30) {
        0 => format!("<!-- c --> {key}: {value}"),
        1 => format!("{key}: {value} <!-- c -->"),
        2 => format!("<!-- c -->{key}: {value}"),
        3 => format!("{key}: <!-- c -->{value}"),
        4 => format!("{key}: {value} <!-- a\nb -->"),
        _ => format!("{key}: {value}"),
    };
    format!("{indent}{quote}{marker}{line}\n")
}

fn corpus_separator(rng: &mut Corpus) -> &'static str {
    rng.pick(&["", "", "\n", "\n", "\n\n"])
}

/// One whole PR body whose Release impact block mixes the structures.
fn corpus_body(rng: &mut Corpus) -> String {
    let mut body = String::from("## Summary\n\nAdds an item.\n\n## Release impact\n");
    body.push_str(rng.pick(&["\n", "\n", ""]));
    for _ in 0..rng.below(3) {
        body.push_str(&corpus_block(rng));
        body.push_str(corpus_separator(rng));
    }
    let group = (rng.pick(&INDENTS), rng.pick(&MARKERS));
    for (index, field) in FIELDS.into_iter().enumerate() {
        if index > 0 && rng.chance(12) {
            body.push_str(&corpus_block(rng));
            body.push_str(corpus_separator(rng));
        } else if index > 0 && rng.chance(10) {
            body.push('\n');
        }
        body.push_str(&corpus_field(rng, field, group));
    }
    match rng.below(8) {
        0 => body.push_str("\n<!-- end -->\n"),
        1 => body.push_str("\n### Notes\n\n- Impact: patch\n"),
        2 => body.push_str("\n## Reviews\n\nNone.\n"),
        3 => body.push_str(&corpus_block(rng)),
        _ => {}
    }
    body
}

/// What one side makes of a body: its verdict, and the Release impact
/// fields it read, restricted to the six the corpus writes and sorted.
#[derive(Debug, PartialEq)]
struct Reading {
    valid: bool,
    fields: Option<Vec<(String, String)>>,
}

fn corpus_fields(pairs: &serde_json::Value) -> Option<Vec<(String, String)>> {
    let mut fields: Vec<(String, String)> = pairs
        .as_array()?
        .iter()
        .map(|pair| {
            let text = |index: usize| pair[index].as_str().unwrap().to_string();
            (text(0), text(1))
        })
        .filter(|(key, _)| {
            FIELDS
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(key))
        })
        .collect();
    fields.sort();
    Some(fields)
}

fn codeflow() -> &'static str {
    env!("CARGO_BIN_EXE_codeflow")
}

/// `codeflow ci`'s side: the binary's reading and its findings. Its check
/// leaves out release.py's CodeFlow-only rules (one `Unit: codeflow` and one
/// substantive `Evidence`), so they are applied here to the fields it read;
/// the corpus writes only plain values for both.
fn ci_readings(bodies: &[String]) -> Vec<Reading> {
    let mut child = Command::new(codeflow())
        .args(["ci", "--read-release-impact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(child.stdin.take().unwrap(), bodies).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "codeflow ci --read-release-impact failed"
    );
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        answer["protocol"], 1,
        "the reader protocol release.py reads"
    );
    answer["readings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reading| {
            let fields = corpus_fields(&reading["release_impact"]);
            let only = |name: &str| -> Vec<&str> {
                fields
                    .iter()
                    .flatten()
                    .filter(|(key, _)| key == name)
                    .map(|(_, value)| value.as_str())
                    .collect()
            };
            let codeflow_rules = only("unit") == ["codeflow"]
                && matches!(only("evidence").as_slice(), [value] if value.chars().any(char::is_alphanumeric));
            let clean = reading["findings"].as_array().unwrap().is_empty();
            Reading {
                valid: clean && codeflow_rules,
                fields,
            }
        })
        .collect()
}

/// release.py's side, reading through the same binary. python3 is required:
/// release.py is part of this repository's release gate, so a missing
/// interpreter fails here rather than skipping the comparison.
fn release_py_readings(bodies: &[String]) -> Vec<Reading> {
    const SCRIPT: &str = "import json, sys\n\
        sys.path.insert(0, sys.argv[1])\n\
        import release\n\
        bodies = json.load(sys.stdin)\n\
        out = []\n\
        for body, reading in zip(bodies, release.read_pr_bodies(bodies)):\n\
        \x20   try:\n\
        \x20       release.parse_release_impact(body, reading=reading)\n\
        \x20       valid = True\n\
        \x20   except release.ReleaseError:\n\
        \x20       valid = False\n\
        \x20   out.append({'valid': valid, 'fields': release.release_impact_fields(reading)})\n\
        json.dump(out, sys.stdout)\n";
    let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts");
    let mut child = Command::new("python3")
        .args(["-B", "-c", SCRIPT])
        .arg(&scripts)
        .env("CODEFLOW_BIN", codeflow())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("python3 runs scripts/release.py; install python3 to run this test");
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(serde_json::to_string(bodies).unwrap().as_bytes())
        .unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let values: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    values
        .iter()
        .map(|value| Reading {
            valid: value["valid"].as_bool().unwrap(),
            fields: corpus_fields(&value["fields"]),
        })
        .collect()
}

/// Drops lines from each disagreeing body while the sides still disagree,
/// so a failure shows the smallest body that reproduces it.
fn minimize(mut bodies: Vec<String>) -> Vec<String> {
    loop {
        let mut candidates = Vec::new();
        for (owner, body) in bodies.iter().enumerate() {
            let lines: Vec<&str> = body.split_inclusive('\n').collect();
            for skip in 0..lines.len() {
                let candidate: String = lines
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != skip)
                    .map(|(_, line)| *line)
                    .collect();
                candidates.push((owner, candidate));
            }
        }
        let texts: Vec<String> = candidates.iter().map(|(_, text)| text.clone()).collect();
        let (ci, python) = (ci_readings(&texts), release_py_readings(&texts));
        let mut changed = false;
        let mut taken = vec![false; bodies.len()];
        for (((owner, candidate), ci), python) in candidates.into_iter().zip(ci).zip(python) {
            if !taken[owner] && ci != python {
                bodies[owner] = candidate;
                taken[owner] = true;
                changed = true;
            }
        }
        if !changed {
            return bodies;
        }
    }
}

#[test]
fn release_py_and_codeflow_ci_agree_on_a_generated_corpus() {
    use std::fmt::Write as _;
    let mut rng = Corpus(CORPUS_SEED);
    let mut bodies: Vec<String> = (0..CORPUS_SIZE).map(|_| corpus_body(&mut rng)).collect();
    // Every hand-picked shared case rides along, so a class found in review
    // (TSK-147 round 5: Breaking change sections holding only a template
    // alternative or a self-reference, a Kelvin-sign key) stays compared.
    let shared: serde_json::Value = serde_json::from_str(include_str!(
        "../../../scripts/fixtures/release_impact_cases.json"
    ))
    .unwrap();
    bodies.extend(
        shared["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|case| case["body"].as_str().unwrap().to_string()),
    );
    let ci = ci_readings(&bodies);
    let python = release_py_readings(&bodies);
    let accepted = ci.iter().filter(|reading| reading.valid).count();
    assert!(
        accepted * 20 >= CORPUS_SIZE && accepted * 20 <= CORPUS_SIZE * 19,
        "the corpus mixes valid and invalid bodies: {accepted} valid"
    );
    let disagreeing: Vec<usize> = (0..bodies.len())
        .filter(|index| ci[*index] != python[*index])
        .collect();
    eprintln!("{CORPUS_SIZE} bodies (seed {CORPUS_SEED:#x}): {accepted} valid");
    if disagreeing.is_empty() {
        return;
    }
    let shown: Vec<usize> = disagreeing.iter().copied().take(12).collect();
    let minimized = minimize(shown.iter().map(|index| bodies[*index].clone()).collect());
    let mut report = format!(
        "{} of {} bodies (seed {CORPUS_SEED:#x} and the shared cases) are judged differently; first {} minimized:\n",
        disagreeing.len(),
        bodies.len(),
        shown.len()
    );
    let (ci, python) = (ci_readings(&minimized), release_py_readings(&minimized));
    for (((index, body), ci), python) in shown.iter().zip(&minimized).zip(ci).zip(python) {
        writeln!(report, "\n--- body {index}\n{body}--- codeflow ci: {ci:?}").unwrap();
        writeln!(report, "--- release.py:  {python:?}").unwrap();
    }
    panic!("{report}");
}
