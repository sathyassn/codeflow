//! The release qualification's record rows (TSK-1068): `qualify_records`
//! from `tests/release-qualification/lib.sh`, the function `qualify.sh`
//! calls, run against this binary on a full-tier sample prepared as the
//! harness lands its scaffold. No hosted job runs the qualification, so this
//! is the check that its record rows pass on the current record rules.
//!
//! The control runs the same rows with `state_sample_outcomes` disabled,
//! which is how the rows read before TSK-1068: the graph check and the
//! first `work start` then fail on the Outcome rule (TSK-435).
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn harness() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/release-qualification")
}

/// Prepare a full-tier sample at `work/sample` the way `qualify.sh` does
/// (a seeded repository, `init --full`, the scaffold landed through
/// `integrate`), then run `qualify_records` on it. With `outcomes` false the
/// Outcome step is disabled. Returns the script's exit status, its output
/// and the result rows as `(command, check, status, observed)`.
fn run_records(work: &Path, outcomes: bool) -> (bool, String, Vec<[String; 4]>) {
    let script = r#"
set -eu
. "$HARNESS/lib.sh"
DIR=$WORK/sample
RESULTS=$WORK/results.tsv
TRANSCRIPT=$WORK/transcript.log
SAMPLE=test
TIER=full
FAILURES=0
: >"$RESULTS"
: >"$TRANSCRIPT"
if [ "$OUTCOMES" = no ]; then
  state_sample_outcomes() { :; }
fi
must() {
  cf "$@"
  [ "$CF_STATUS" = 0 ] || { printf 'codeflow %s failed:\n%s\n' "$*" "$CF_OUT" >&2; exit 1; }
}
seed_git "$DIR"
printf '# sample\n' >"$DIR/README.md"
commit_in "$DIR" "chore: seed the sample"
cd "$DIR"
must init --full --yes
git checkout -q -b chore/codeflow-scaffold
git add -A
git commit -q -m "chore: scaffold codeflow full tier"
must integrate chore/codeflow-scaffold --into main
git checkout -q main
qualify_records
"#;
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(binary.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    let mut command = Command::new("sh");
    command
        .args(["-c", script])
        .env("HARNESS", harness())
        .env("WORK", work)
        .env("OUTCOMES", if outcomes { "yes" } else { "no" })
        .env("BINARY", &binary)
        .env("CODEFLOW_HOOK_BINARY", &binary)
        .env("CODEFLOW_HOME", work.join("codeflow-home"))
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CARGO_TARGET_DIR");
    // The harness runs as the operator does; the hooks a sample installs
    // judge an agent session's commit at the sample's root checkout.
    for name in codeflow_core::root_checkout::AGENT_MARKERS {
        command.env_remove(name);
    }
    let out = command.output().expect("sh runs");
    let rows = std::fs::read_to_string(work.join("results.tsv"))
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let cells: Vec<&str> = line.split('\t').collect();
            [cells[2], cells[3], cells[4], cells[6]].map(str::to_string)
        })
        .collect();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text, rows)
}

fn row<'a>(rows: &'a [[String; 4]], command: &str, check: &str) -> &'a [String; 4] {
    rows.iter()
        .find(|row| row[0] == command && row[1].starts_with(check))
        .unwrap_or_else(|| panic!("no `{command}` / `{check}` row in {rows:#?}"))
}

#[test]
fn the_qualification_record_rows_pass_on_a_fresh_full_tier_sample() {
    let work = tempfile::tempdir().unwrap();
    let (ok, text, rows) = run_records(work.path(), true);
    assert!(ok, "the rows did not finish:\n{text}");
    let checks: Vec<(&str, &str)> = rows
        .iter()
        .map(|row| (row[0].as_str(), row[2].as_str()))
        .collect();
    assert_eq!(
        checks,
        [
            ("epic new", "passed"),
            ("task new", "passed"),
            ("spec new", "passed"),
            ("validate --docs", "passed"),
            ("work start", "passed"),
            ("work start", "passed"),
        ],
        "{rows:#?}"
    );
}

#[test]
fn without_the_outcome_step_the_graph_and_refusal_rows_fail_on_the_outcome() {
    let work = tempfile::tempdir().unwrap();
    let (_, text, rows) = run_records(work.path(), false);
    for (command, check) in [
        ("validate --docs", "the freshly allocated graph"),
        ("work start", "rejecting"),
    ] {
        let found = row(&rows, command, check);
        assert_eq!(found[2], "failed", "{found:?}\n{text}");
    }
    // A row keeps 400 characters of output, so the reason is read from the
    // run's transcript, where each command's output is whole.
    let transcript = std::fs::read_to_string(work.path().join("transcript.log")).unwrap();
    for command in ["validate --docs", "work start TSK-001 --into main"] {
        let output = transcript
            .split("\n$ codeflow ")
            .find(|section| section.starts_with(command))
            .unwrap_or_else(|| panic!("no `{command}` in the transcript:\n{transcript}"));
        assert!(
            output.contains("outcome: record created after this project adopted the `## Outcome` section has none filled"),
            "{output}"
        );
    }
}

/// `qualify.sh` runs the rows this test runs: it calls `qualify_records`
/// and keeps no copy of its own.
#[test]
fn qualify_sh_calls_the_record_rows_from_lib_sh() {
    let qualify = std::fs::read_to_string(harness().join("qualify.sh")).unwrap();
    let lib = std::fs::read_to_string(harness().join("lib.sh")).unwrap();
    assert!(qualify.contains("\n    qualify_records\n"));
    assert!(!qualify.contains("qualify_records() {"));
    assert!(lib.contains("\nqualify_records() {\n"));
    assert!(lib.contains("\n  state_sample_outcomes \"$DIR\"\n"));
}
