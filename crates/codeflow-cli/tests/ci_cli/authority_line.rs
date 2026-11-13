//! TSK-1177 AC-2: the hosted report prints the authority line after the
//! content verdict, from the host-rules file the policy workflow names in
//! `CODEFLOW_HOST_RULES`, and the line never changes the verdict: the exit
//! code is the same with and without it, clean or failing, and an
//! unreadable file only says so.

use std::path::Path;
use std::process::Output;

use super::{codeflow, git, repo_with_range};

const MODE_A: &str = r#"{
  "branch": "main",
  "authority": "0123456789abcdef0123456789abcdef01234567",
  "rules": [
    { "type": "update", "ruleset_id": 11 },
    { "type": "pull_request", "ruleset_id": 12,
      "parameters": { "required_approving_review_count": 0 } },
    { "type": "required_status_checks", "ruleset_id": 12, "parameters": {
        "strict_required_status_checks_policy": true,
        "required_status_checks": [
          { "context": "authority: commit standards", "integration_id": 4242 },
          { "context": "codeflow gates", "integration_id": 15368 }
        ] } }
  ],
  "rulesets": [
    { "id": 11, "name": "codeflow authority updates" },
    { "id": 12, "name": "codeflow authority checks" }
  ]
}"#;

const LINE: &str = "codeflow ci: authority: judged by main@0123456789abcdef0123456789abcdef01234567; check identity: App 4242; host rules read from rules/branches/main: A present, B present, approvals 0, strict yes, pins: authority: commit standards pinned to App 4242; bypass actors not visible to this token; run `codeflow doctor --check remote-perimeter` as an administrator";

fn ci(dir: &Path, rules: Option<&Path>) -> Output {
    let mut command = codeflow();
    command
        .args([
            "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x",
        ])
        .current_dir(dir)
        .env_remove("CODEFLOW_HOST_RULES");
    if let Some(rules) = rules {
        command.env("CODEFLOW_HOST_RULES", rules);
    }
    command.output().expect("binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A clean range: exit 0 either way, and the line follows the verdict.
#[test]
fn the_line_follows_a_clean_verdict_and_keeps_its_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let rules = dir.path().join(".git").join("host-rules.json");
    std::fs::write(&rules, MODE_A).unwrap();

    let without = ci(dir.path(), None);
    let with = ci(dir.path(), Some(&rules));
    assert_eq!(without.status.code(), Some(0), "{}", stdout(&without));
    assert_eq!(with.status.code(), without.status.code());
    assert!(
        !stdout(&without).contains("authority:"),
        "{}",
        stdout(&without)
    );
    let out = stdout(&with);
    let verdict = out.find("codeflow ci: clean").expect("the content verdict");
    let line = out.find(LINE).unwrap_or_else(|| panic!("{out}"));
    assert!(
        verdict < line,
        "the authority line follows the verdict: {out}"
    );
    assert_eq!(out.matches("codeflow ci: authority:").count(), 1);
}

/// A failing range stays failing with the line; a Mode B or unreadable
/// line never turns it, or a clean one, around.
#[test]
fn the_line_never_changes_a_failing_verdict() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    std::fs::write(dir.path().join("more.rs"), "fn more() {}\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "not a conventional subject"]);
    let rules = dir.path().join(".git").join("host-rules.json");

    let without = ci(dir.path(), None);
    assert_eq!(without.status.code(), Some(1), "{}", stdout(&without));
    for document in [
        MODE_A.to_owned(),
        r#"{"branch": "main", "authority": "x", "unreadable": "the host rules could not be read with this token"}"#.to_owned(),
        "{".to_owned(),
    ] {
        std::fs::write(&rules, &document).unwrap();
        let with = ci(dir.path(), Some(&rules));
        assert_eq!(with.status.code(), Some(1), "{document}: {}", stdout(&with));
        assert!(stdout(&with).contains("codeflow ci: authority:"), "{}", stdout(&with));
    }
    std::fs::write(
        &rules,
        r#"{"branch": "main", "authority": "x", "unreadable": "the host rules could not be read with this token"}"#,
    )
    .unwrap();
    let out = stdout(&ci(dir.path(), Some(&rules)));
    assert!(
        out.contains("check identity and host rules not readable (the host rules could not be read with this token); the verdict above stands"),
        "{out}"
    );
    let missing = ci(dir.path(), Some(&dir.path().join("absent.json")));
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        stdout(&missing).contains("is not readable"),
        "{}",
        stdout(&missing)
    );
}
