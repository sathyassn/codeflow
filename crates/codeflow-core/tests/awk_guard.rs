//! TSK-1067: one table of visible sources and residuals through both guards.
//! Baseline classifications must be filled from the preserved base binary
//! before review; this table states the intended implementation verdicts.

use codeflow_core::hooks::git_guard::{self, GuardContext};
use codeflow_core::hooks::policy::{GitPolicy, PolicyLevel, SecuritySection};
use codeflow_core::hooks::sandbox_state::Outcome;
use codeflow_core::hooks::{any_blocking, exec_guard, Violation};
use std::path::Path;

const INTEGRITY: &str = "git.hook_integrity";

struct Row {
    command: String,
    exec: Option<&'static str>,
    git: Option<&'static str>,
    difference: &'static str,
}

fn row(command: impl Into<String>, exec: Option<&'static str>, git: Option<&'static str>) -> Row {
    Row {
        command: command.into(),
        exec,
        git,
        difference: if exec != git {
            "exec-guard reads interpreter code and arguments; git-guard judges shell writers"
        } else {
            "same verdict"
        },
    }
}

#[allow(clippy::too_many_lines)] // One matrix keeps every guard difference reviewable.
fn table() -> Vec<Row> {
    let mut rows = Vec::new();
    for dialect in ["awk", "gawk", "mawk", "nawk", "busybox awk"] {
        for body in [
            r#"BEGIN { print "x" > ".claude/settings.json" }"#,
            r#"BEGIN { print "x" >> ".claude/settings.json" }"#,
            r#"BEGIN { print "x" > "\056claude/settings.json" }"#,
            r#"BEGIN { system("tee .claude/settings.json") }"#,
            r#"BEGIN { print "x" | "tee .claude/settings.json" }"#,
            r#"BEGIN { "tee .claude/settings.json" | getline line }"#,
            r#"BEGIN { system("sed -i s/a/b/ .claude/settings.json") }"#,
            r#"BEGIN { print "x" | "sed -i s/a/b/ .claude/settings.json" }"#,
            r#"BEGIN { "sed -i s/a/b/ .claude/settings.json" | getline line }"#,
            r#"BEGIN { system("sudo true") }"#,
            r#"BEGIN { print "x" | "sudo true" }"#,
            r#"BEGIN { "sudo true" | getline line }"#,
        ] {
            rows.push(row(format!("{dialect} '{body}'"), Some(INTEGRITY), None));
            if dialect == "gawk" {
                for option in ["-e ", "-e", "--source ", "--source="] {
                    rows.push(row(
                        format!("{dialect} {option}'{body}'"),
                        Some(INTEGRITY),
                        None,
                    ));
                }
            }
        }
        rows.push(row(format!("{dialect} -f prog.awk"), None, None));
        rows.push(row(
            format!("{dialect} -f prog.awk .claude/settings.json"),
            Some(INTEGRITY),
            None,
        ));
        for body in [
            r#"BEGIN { p=".cl" "aude/settings.json"; print "x" > p }"#,
            r#"BEGIN { cmd="tee .cl" "aude/settings.json"; system(cmd) }"#,
            r#"BEGIN { cmd="tee .cl" "aude/settings.json"; print "x" | cmd }"#,
            r#"BEGIN { cmd="tee .cl" "aude/settings.json"; cmd | getline line }"#,
            "{print $1}",
            r#"BEGIN { print "x" > "out.txt" }"#,
            r#"BEGIN { getline line < "notes.txt" }"#,
        ] {
            rows.push(row(format!("{dialect} '{body}'"), None, None));
        }
        rows.push(row(
            format!("{dialect} '{{print $1}}' data.txt"),
            None,
            None,
        ));
        rows.push(row(
            format!("{dialect} 'BEGIN {{ cmd=\"tee .claude/settings.json\"; system(cmd) }}'"),
            Some(INTEGRITY),
            None,
        ));
    }
    for source in ["'{print}'", "-f prog.awk", "-e '{print}'"] {
        rows.push(row(
            format!("gawk -i inplace {source} .claude/settings.json"),
            Some(INTEGRITY),
            Some(INTEGRITY),
        ));
    }
    for include in ["-i lib.awk", "--include lib.awk", "--include=lib.awk"] {
        rows.push(row(format!("gawk {include} '{{print}}'"), None, None));
        rows.push(row(
            format!("gawk {include} '{{print}}' .claude/settings.json"),
            Some(INTEGRITY),
            None,
        ));
        rows.push(row(
            format!("gawk {include} 'BEGIN {{ print 1 > \".claude/settings.json\" }}'"),
            Some(INTEGRITY),
            None,
        ));
    }
    rows.extend([
        row("gawk '@include \"lib.awk\"\n{print}'", None, None),
        row(
            "gawk '@include \"lib.awk\"\n{print}' .claude/settings.json",
            Some(INTEGRITY),
            None,
        ),
        row(
            "gawk '@include \"lib.awk\"\nBEGIN { print 1 > \".claude/settings.json\" }'",
            Some(INTEGRITY),
            None,
        ),
        row("gawk -F, '{print $2}' notes.csv", None, None),
        row("awk \"$(printf %s '{print 1}')\"", None, None),
        row(
            "awk \"$(printf %s 'BEGIN { print 1 > \".claude/settings.json\" }')\"",
            Some(INTEGRITY),
            None,
        ),
        row(
            "python3 -c 'open(\".claude/settings.json\", \"w\").write(\"x\")'",
            Some(INTEGRITY),
            None,
        ),
        row(
            "perl -e 'open(F, \">.claude/settings.json\"); print F \"x\"'",
            Some(INTEGRITY),
            None,
        ),
        row(
            "node -e 'require(\"fs\").writeFileSync(\".claude/settings.json\", \"x\")'",
            Some(INTEGRITY),
            None,
        ),
        row(
            "ruby -e 'File.write(\".claude/settings.json\", \"x\")'",
            Some(INTEGRITY),
            None,
        ),
        // Direct writer controls preserve the git guard's existing rule.
        row("tee .claude/settings.json", None, Some(INTEGRITY)),
        row("sed -i s/a/b/ .claude/settings.json", None, Some(INTEGRITY)),
        row("printf x > .claude/settings.json", None, Some(INTEGRITY)),
    ]);
    rows
}

fn git_findings(command: &str, cwd: &Path) -> Vec<Violation> {
    let policy = GitPolicy::default();
    let ctx = GuardContext {
        policy: &policy,
        current_branch: "main",
        integrate_token: false,
        pr_base_lookup: None,
        dir_target_lookup: None,
        alias_lookup: None,
        discard_lookup: None,
        branch_lookup: None,
        root_checkout: None,
    };
    git_guard::evaluate_report_at(command, &ctx, cwd).violations
}

fn assert_verdict(
    command: &str,
    guard: &str,
    findings: &[Violation],
    expected: Option<&str>,
    why: &str,
) {
    assert_eq!(
        any_blocking(findings),
        expected.is_some(),
        "{guard}: {command}: {why}: {findings:?}"
    );
    if let Some(rule) = expected {
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == rule && finding.level == PolicyLevel::Block),
            "{guard}: {command}: expected {rule}: {findings:?}"
        );
    }
}

#[test]
fn awk_guard_table_preserves_both_guard_contracts() {
    let temp = tempfile::tempdir().unwrap();
    for checkout in [false, true] {
        let root = temp.path().join(if checkout { "repo" } else { "outside" });
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        if checkout {
            git2::Repository::init(&root).unwrap();
        }
        std::fs::write(root.join(".claude/settings.json"), "{}\n").unwrap();
        // The files really hold a protected write; the guard must not read them.
        for file in ["prog.awk", "lib.awk"] {
            std::fs::write(
                root.join(file),
                "BEGIN { print 1 > \".claude/settings.json\" }\n",
            )
            .unwrap();
        }
        for outcome in [
            Outcome::Relaxed,
            Outcome::Strict("test strict default".into()),
        ] {
            for case in table() {
                let exec = exec_guard::evaluate_ruled(
                    &case.command,
                    true,
                    &SecuritySection::default(),
                    PolicyLevel::Block,
                    &root,
                    &root,
                    &outcome,
                );
                let git = git_findings(&case.command, &root);
                assert_verdict(&case.command, "exec", &exec, case.exec, case.difference);
                assert_verdict(&case.command, "git", &git, case.git, case.difference);
                if case.exec == Some(INTEGRITY)
                    && !case.command.starts_with("python")
                    && !case.command.starts_with("perl")
                    && !case.command.starts_with("node")
                    && !case.command.starts_with("ruby")
                {
                    assert!(
                        any_blocking(&exec),
                        "exec-only route must refuse: {}",
                        case.command
                    );
                }
            }
            let control = exec_guard::evaluate_ruled(
                "python3 -c \"$UNSET\"",
                true,
                &SecuritySection::default(),
                PolicyLevel::Block,
                &root,
                &root,
                &outcome,
            );
            for dialect in ["awk", "gawk", "mawk", "nawk", "busybox awk"] {
                for source in ["\"$P\"", "\"$UNSET\"", "\"$(cat p)\""] {
                    let command = format!("{dialect} {source}");
                    let exec = exec_guard::evaluate_ruled(
                        &command,
                        true,
                        &SecuritySection::default(),
                        PolicyLevel::Block,
                        &root,
                        &root,
                        &outcome,
                    );
                    let expected = checkout.then_some(INTEGRITY);
                    assert_verdict(
                        &command,
                        "python control",
                        &control,
                        expected,
                        "same session and policy",
                    );
                    assert_verdict(
                        &command,
                        "exec",
                        &exec,
                        expected,
                        "unresolved code uses the Python rule",
                    );
                    if checkout {
                        assert!(
                            exec.iter().any(|finding| finding.rule == INTEGRITY
                                && finding
                                    .message
                                    .contains("cannot verify interpreter expansion")
                                && finding
                                    .message
                                    .contains("where a checkout or enforcement path is reachable")),
                            "{command}: {exec:?}"
                        );
                    }
                    assert_verdict(
                        &command,
                        "git",
                        &git_findings(&command, &root),
                        None,
                        "git-guard does not read unresolved interpreter code",
                    );
                }
            }
        }
    }
}

#[test]
fn unresolved_awk_uses_protected_checkout_names_even_from_outside() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("protected");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    git2::Repository::init(&root).unwrap();
    for outcome in [
        Outcome::Relaxed,
        Outcome::Strict("test strict default".into()),
    ] {
        for command in [
            "python3 -c \"$UNSET\"",
            "awk \"$P\"",
            "awk \"$UNSET\"",
            "awk \"$(cat p)\"",
        ] {
            let findings = exec_guard::evaluate_ruled(
                command,
                true,
                &SecuritySection::default(),
                PolicyLevel::Block,
                &outside,
                &root,
                &outcome,
            );
            assert_verdict(
                command,
                "exec",
                &findings,
                Some(INTEGRITY),
                "the protected-name set contains a checkout even though cwd is outside",
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn unresolved_awk_refuses_an_unknown_protected_name_set() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("unknown");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(".git", root.join(".git")).unwrap();
    for outcome in [
        Outcome::Relaxed,
        Outcome::Strict("test strict default".into()),
    ] {
        for command in [
            "python3 -c \"$UNSET\"",
            "awk \"$P\"",
            "awk \"$UNSET\"",
            "awk \"$(cat p)\"",
        ] {
            let findings = exec_guard::evaluate_ruled(
                command,
                true,
                &SecuritySection::default(),
                PolicyLevel::Block,
                &outside,
                &root,
                &outcome,
            );
            assert_verdict(
                command,
                "exec",
                &findings,
                Some(INTEGRITY),
                "the protected-name set cannot be read",
            );
        }
    }
}
