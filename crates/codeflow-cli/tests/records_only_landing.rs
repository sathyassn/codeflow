//! Exercise the real CLI's selected gate and its local landing evidence.

use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, program: &str, args: &[&str], home: &Path) -> Output {
    Command::new(program)
        .args(args)
        .current_dir(root)
        .env("CODEFLOW_HOME", home)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.test")
        .env("GIT_COMMITTER_EMAIL", "test@example.test")
        .env("CLAUDECODE", "")
        .env("CODEX_SESSION_ID", "")
        .env("CODEX_THREAD_ID", "")
        .env("GROK_SESSION_ID", "")
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn git(root: &Path, args: &[&str], home: &Path) -> String {
    let output = run(root, "git", args, home);
    assert!(output.status.success(), "git {args:?}: {}", text(&output));
    String::from_utf8(output.stdout).unwrap().trim().into()
}

#[test]
fn records_run_quick_and_record_checks_while_code_requires_full() {
    for scenario in ["records", "code", "secret"] {
        let code = scenario == "code";
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"], home.path());
        std::fs::create_dir(root.join(".codeflow")).unwrap();
        std::fs::create_dir(root.join("docs")).unwrap();
        std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
        std::fs::write(root.join(".codeflow/policy.json"), "{}").unwrap();
        std::fs::write(root.join(".codeflow/test-config.json"), serde_json::json!({
            "schema_version": "1.0", "targets": [{"name": "fixture", "runner": "custom", "modes": {
                "quick": {"command": "echo quick > quick-ran.txt"},
                "full": {"command": "echo full > full-ran.txt && exit 19"}
            }}]
        }).to_string()).unwrap();
        git(root, &["add", "."], home.path());
        git(
            root,
            &["commit", "-qm", "test: establish fixture"],
            home.path(),
        );
        git(root, &["branch", "integration/landing"], home.path());
        git(root, &["checkout", "-qb", "feat/reviewed"], home.path());
        let file = if code { "source.rs" } else { "docs/guide.md" };
        let contents = if scenario == "secret" {
            format!("ghp_{}", "a".repeat(36))
        } else {
            "A new guide\n".into()
        };
        std::fs::write(root.join(file), contents).unwrap();
        git(root, &["add", file], home.path());
        git(
            root,
            &["commit", "-qm", "docs: explain the change"],
            home.path(),
        );
        let linked = root.join(".worktrees/integrator");
        git(
            root,
            &[
                "worktree",
                "add",
                "-b",
                "chore/integrator",
                linked.to_str().unwrap(),
                "main",
            ],
            home.path(),
        );
        let before = git(root, &["rev-parse", "integration/landing"], home.path());
        let output = run(
            &linked,
            env!("CARGO_BIN_EXE_codeflow"),
            &[
                "integrate",
                "feat/reviewed",
                "--into",
                "integration/landing",
                "--merge",
            ],
            home.path(),
        );
        let said = text(&output);
        let after = git(root, &["rev-parse", "integration/landing"], home.path());
        if scenario == "secret" {
            assert!(!output.status.success(), "{said}");
            assert_eq!(before, after);
            assert!(said.contains("secret scan found"), "{said}");
            assert!(!linked.join("quick-ran.txt").exists());
        } else if code {
            assert!(!output.status.success(), "{said}");
            assert_eq!(before, after);
            assert!(linked.join("full-ran.txt").exists(), "{said}");
            assert!(!linked.join("quick-ran.txt").exists());
        } else {
            assert!(output.status.success(), "{said}");
            assert_ne!(before, after);
            assert!(linked.join("quick-ran.txt").exists());
            assert!(!linked.join("full-ran.txt").exists());
            assert!(said.contains("landing class: records-only"), "{said}");
            let record: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(root.join(format!(".git/codeflow/landings/{after}.json")))
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(record["class"], "records-only");
            assert_eq!(record["base"], before);
            assert_eq!(record["checks_ran"].as_array().unwrap().len(), 4);
        }
    }
}
