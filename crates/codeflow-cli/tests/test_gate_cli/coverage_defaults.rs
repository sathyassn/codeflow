//! Coverage rules a project writes once in `defaults.coverage` judge every
//! target that collects coverage and sets no rules of its own (TSK-1014,
//! issue 299), driven through the real binary.

use super::{repo, run, stderr};

/// One file, one of its two lines hit: 50 percent.
const HALF_COVERED: &str = "SF:src/lib.rs\nDA:1,1\nDA:2,0\nend_of_record\n";
/// The same file with both lines hit.
const FULLY_COVERED: &str = "SF:src/lib.rs\nDA:1,1\nDA:2,1\nend_of_record\n";

/// A floor written only in `defaults.coverage`, and one target whose
/// coverage block names its artifact and no rules of its own.
const FLOOR_IN_DEFAULTS: &str = r#"{"schema_version":"1.0",
  "defaults":{"coverage":[{"scope":"global","minimum":90}]},
  "targets":[{"name":"app","runner":"custom",
    "modes":{"full":{"command":"exit 0"}},
    "coverage":{"format":"lcov","path":"cov.lcov"}}]}"#;

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn a_floor_written_only_in_defaults_fails_a_full_gate_below_it() {
    let dir = repo(FLOOR_IN_DEFAULTS);
    let home = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("cov.lcov"), HALF_COVERED).unwrap();
    let below = run(dir.path(), home.path(), &["test", "--mode", "full"]);
    let text = format!("{}{}", stdout(&below), stderr(&below));
    assert!(
        !below.status.success(),
        "a full gate below the default floor must fail: {text}"
    );
    assert!(
        text.contains("app: 50.0% overall — 0 pass, 1 fail (rules from defaults.coverage)"),
        "the verdict names the default rule set: {text}"
    );
    assert!(
        text.contains("below threshold: <global> 50.0% < 90"),
        "the verdict names the default rule: {text}"
    );
    assert!(text.contains("test gate: FAILED"), "{text}");

    std::fs::write(dir.path().join("cov.lcov"), FULLY_COVERED).unwrap();
    let above = run(dir.path(), home.path(), &["test", "--mode", "full"]);
    let text = format!("{}{}", stdout(&above), stderr(&above));
    assert!(
        above.status.success(),
        "a full gate at the default floor passes: {text}"
    );
    assert!(
        text.contains("app: 100.0% overall — 1 pass, 0 fail (rules from defaults.coverage)"),
        "{text}"
    );
}
