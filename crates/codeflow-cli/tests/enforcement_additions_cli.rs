//! TSK-401: report additions at the CLI and retain them through update.
//! These tests initialize the candidate. The core regression suite supplies
//! the real prior-release inputs; the installed 3.1.1 journey is separate.

use std::path::Path;
use std::process::{Command, Output};

fn output_text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn isolated(root: &Path, executable: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(executable);
    cmd.current_dir(root)
        .env("CODEFLOW_HOME", root.join("personal"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn run(root: &Path, args: &[&str]) -> Output {
    isolated(root, env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .output()
        .unwrap()
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let out = isolated(dir.path(), "git")
        .args(["init", "-q", "-b", "main"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", output_text(&out));
    let out = run(dir.path(), &["init", "--full", "--yes"]);
    assert!(out.status.success(), "{}", output_text(&out));
    dir
}

fn append(root: &Path, relative: &str, addition: &str) {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path, format!("{text}{addition}")).unwrap();
}

#[test]
fn doctor_names_project_additions_before_and_after_update_without_printing_values() {
    let dir = fixture();
    let root = dir.path();
    for (relative, addition, anchor) in [
        (
            ".codeflow/policy.json",
            "\"blocked_names\" : [ \"private-project-value\" ],",
            "\"git\": {",
        ),
        (
            ".claude/settings.json",
            "\t\"Read(private-project-value/\\u002a\\u002a)\" ,",
            "\"deny\": [",
        ),
    ] {
        let path = root.join(relative);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(anchor));
        std::fs::write(
            path,
            text.replacen(anchor, &format!("{anchor}\n{addition}"), 1),
        )
        .unwrap();
    }
    let toml = "\n# Personal extension.\n[project_tools]\nname = 'private-project-value'\n";
    append(root, ".codeflow/project.toml", toml);
    let rule = "\nprefix_rule(pattern = [\"private-project-value\"], decision = \"forbidden\")\n";
    append(root, ".codex/rules/codeflow.rules", rule);
    std::fs::write(root.join(".codex/rules/project.rules"), rule).unwrap();

    for updated in [false, true] {
        if updated {
            let out = run(root, &["update"]);
            assert!(out.status.success(), "{}", output_text(&out));
            for relative in [
                ".codeflow/policy.json",
                ".claude/settings.json",
                ".codex/rules/codeflow.rules",
                ".codeflow/project.toml",
            ] {
                assert!(!root.join(format!("{relative}.new")).exists(), "{relative}");
            }
            assert!(std::fs::read_to_string(root.join(".codeflow/project.toml"))
                .unwrap()
                .contains(toml));
            assert!(
                std::fs::read_to_string(root.join(".codex/rules/codeflow.rules"))
                    .unwrap()
                    .contains(rule)
            );
            assert_eq!(
                std::fs::read_to_string(root.join(".codex/rules/project.rules")).unwrap(),
                rule
            );
        }
        let out = run(root, &["doctor", "--check", "managed-drift"]);
        let text = output_text(&out);
        assert!(out.status.success(), "{text}");
        for entry in [
            "project additions",
            ".codeflow/policy.json: entry /git/blocked_names",
            ".claude/settings.json: entry /permissions/deny/",
            ".codeflow/project.toml: entry project_tools",
            ".codex/rules/codeflow.rules: line ",
            ".codex/rules/project.rules: line ",
            "enforcement not verified",
        ] {
            assert!(text.contains(entry), "missing {entry:?}: {text}");
        }
        assert!(!text.contains("private-project-value"), "{text}");
        assert!(!text.contains("regenerate and lose"), "{text}");
    }
}

#[test]
fn additions_do_not_hide_managed_text_drift() {
    let dir = fixture();
    let path = dir.path().join("AGENTS.md");
    let text = std::fs::read_to_string(&path).unwrap();
    let begin = text.find("<!-- codeflow:managed:begin").unwrap();
    let end = text[begin..].find('\n').unwrap() + begin;
    let (before, after) = text.split_at(end);
    std::fs::write(
        path,
        format!("{before}\nChanged managed instruction.{after}"),
    )
    .unwrap();
    append(
        dir.path(),
        ".codeflow/project.toml",
        "\n[project_tools]\nname = 'local'\n",
    );
    let out = run(dir.path(), &["doctor", "--check", "managed-drift"]);
    let text = output_text(&out);
    assert!(text.contains("regenerate and lose"), "{text}");
    assert!(text.contains("project additions"), "{text}");
}

#[test]
fn missing_baselines_are_reported_as_unclassified() {
    let dir = fixture();
    std::fs::remove_file(dir.path().join(".codeflow/.baseline/.claude/settings.json")).unwrap();
    let out = run(dir.path(), &["doctor", "--check", "managed-drift"]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(".claude/settings.json: additions unclassified (no shipped baseline)"),
        "{text}"
    );
}

#[test]
fn corrupt_baselines_fail_instead_of_claiming_project_additions() {
    let dir = fixture();
    std::fs::write(
        dir.path().join(".codeflow/.baseline/.claude/settings.json"),
        "{}",
    )
    .unwrap();
    let out = run(dir.path(), &["doctor", "--check", "managed-drift"]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("baseline does not match"), "{text}");
}

#[cfg(unix)]
#[test]
fn additions_inventory_refuses_symlinks_and_nonregular_files() {
    let dir = fixture();
    let path = dir.path().join(".codex/rules/codeflow.rules");
    std::fs::remove_file(&path).unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), &path).unwrap();
    let out = run(dir.path(), &["doctor", "--check", "managed-drift"]);
    assert!(!out.status.success(), "{}", output_text(&out));
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let out = run(dir.path(), &["doctor", "--check", "managed-drift"]);
    assert!(!out.status.success(), "{}", output_text(&out));
}
