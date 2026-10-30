//! TSK-286 AC-5 (issue 102): the installed hooks judge each bypass form
//! from the PR 84 review and this issue, and each allowed form deletes
//! exactly what the native tool deletes.
//!
//! A real protected target cannot be deleted in a test, so each form gets
//! its own disposable directory under the fixture project with a folder
//! named `sentinel`, and the hooks run with `TMPDIR` set to it. The guard
//! protects the temp root `TMPDIR` names, so the sentinel stands in for a
//! protected directory. Each bypass form is first refused by the installed
//! hooks, then run natively to show it really deletes the sentinel. Each
//! allowed form is accepted by the hooks, then run natively, and must
//! delete exactly the paths listed for it.

use super::Project;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Replay a Bash payload through every installed `PreToolUse` hook of
/// `harness`, with `TMPDIR` naming `sentinel`.
fn replay(
    project: &Project,
    harness: &str,
    cwd: &Path,
    sentinel: &Path,
    command: &str,
) -> Vec<Output> {
    let file = match harness {
        "claude" => ".claude/settings.json",
        "codex" => ".codex/hooks.json",
        _ => unreachable!(),
    };
    let payload = json!({"tool_name": "Bash", "cwd": cwd, "tool_input": {"command": command}});
    let config: Value =
        serde_json::from_slice(&std::fs::read(project.root.join(file)).unwrap()).unwrap();
    let mut outputs = Vec::new();
    for entry in config["hooks"]["PreToolUse"].as_array().unwrap() {
        if !regex::Regex::new(entry["matcher"].as_str().unwrap())
            .unwrap()
            .is_match("Bash")
        {
            continue;
        }
        for hook in entry["hooks"].as_array().unwrap() {
            let mut child = project
                .command("sh")
                .args(["-c", hook["command"].as_str().unwrap()])
                .env("TMPDIR", sentinel)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(payload.to_string().as_bytes())
                .unwrap();
            outputs.push(child.wait_with_output().unwrap());
        }
    }
    assert!(!outputs.is_empty(), "no installed Bash hook for {harness}");
    outputs
}

/// A fresh directory for one form, holding `sentinel/` and the entries
/// listed (`dir/`, `file`, `link -> target`).
fn form_dir(project: &Project, index: usize, setup: &[&str]) -> PathBuf {
    let dir = project.root.join("native").join(index.to_string());
    std::fs::create_dir_all(dir.join("sentinel")).unwrap();
    std::fs::write(dir.join("sentinel").join("kept"), "x\n").unwrap();
    for entry in setup {
        if let Some((link, target)) = entry.split_once(" -> ") {
            std::os::unix::fs::symlink(target, dir.join(link)).unwrap();
        } else if let Some(sub) = entry.strip_suffix('/') {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        } else {
            std::fs::write(dir.join(entry), "x\n").unwrap();
        }
    }
    dir.canonicalize().unwrap()
}

/// Every path below `dir`, relative, without following links.
fn listing(dir: &Path) -> BTreeSet<String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            out.insert(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
            if entry.file_type().unwrap().is_dir() {
                walk(base, &path, out);
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(dir, dir, &mut out);
    out
}

/// Run `command` natively in `dir` and return what it removed.
fn run_native(dir: &Path, command: &str) -> BTreeSet<String> {
    let before = listing(dir);
    let output = Command::new("sh")
        .args(["-c", command])
        .current_dir(dir)
        .output()
        .unwrap();
    let after = listing(dir);
    let removed: BTreeSet<String> = before.difference(&after).cloned().collect();
    assert!(
        !removed.is_empty() || output.status.success(),
        "{command}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    removed
}

fn has_gnu_parallel() -> bool {
    Command::new("parallel")
        .arg("--version")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("GNU parallel"))
}

/// Each bypass form, with `{dir}` for its directory, the entries it needs
/// and whether its native run needs GNU `parallel`.
const BYPASSES: &[(&str, &[&str], bool)] = &[
    // PR 84 review: blanks inside a path, framed by NUL, by quotes and by
    // parallel's newline, and the round 19 option clusters.
    (
        r"printf '%s\0' '{dir}/space name/../sentinel' | xargs -0 rm -rf --",
        &["inner/", "space name -> inner"],
        false,
    ),
    (
        r#"printf '"{dir}/space name/../sentinel"' | xargs rm -rf"#,
        &["inner/", "space name -> inner"],
        false,
    ),
    (
        r"printf '{dir}/space name/../sentinel\n' | parallel rm -rf",
        &["inner/", "space name -> inner"],
        true,
    ),
    (
        r"printf '%s\0' '{dir}/space name/../sentinel' | xargs -t0 rm -rf --",
        &["inner/", "space name -> inner"],
        false,
    ),
    (
        r"printf '%s\0' '{dir}/space name/../sentinel' | xargs -rt0 rm -rf --",
        &["inner/", "space name -> inner"],
        false,
    ),
    (
        r"printf '%s\n' '{dir}/space name/../sentinel' | xargs -I{} rm -rf {}",
        &["inner/", "space name -> inner"],
        false,
    ),
    (
        r"printf '%s\0' '{dir}/space name/../sentinel' | xargs -0 -I{} rm -rf {}",
        &["inner/", "space name -> inner"],
        false,
    ),
    (
        r"printf 'sentinel\n' | xargs -I{} rm -rf '{dir}/{}'",
        &[],
        false,
    ),
    (
        r"printf '%s\n' '{dir}/sentinel' | xargs -n 1 rm -rf",
        &[],
        false,
    ),
    // Issue 102 and this task: grammars the guard read differently from
    // the native tool.
    (
        r#"printf 'x\n{dir}/sentinel\n' | xargs sh -c 'rm -rf "$2"' _"#,
        &[],
        false,
    ),
    (
        r"printf '{dir}/sentinel\n' > list; xargs rm -rf < list",
        &[],
        false,
    ),
    (
        r"printf '{dir}/sentinel\n' > list; cat list | xargs rm -rf",
        &[],
        false,
    ),
    (
        "cd {dir} && find tree | xargs rm -rf",
        &["tree/", "tree/x sentinel"],
        false,
    ),
    (
        r#"find {dir}/a -maxdepth 1 -exec sh -c 'rm -rf "$2"/../../sentinel' _ {} +"#,
        &["a/", "a/f/"],
        false,
    ),
    (
        r"find {dir} -name kept -execdir rm -rf ../sentinel \;",
        &[],
        false,
    ),
    (
        "find -L {dir}/link -name sentinel -exec rm -rf {} +",
        &["link -> ."],
        false,
    ),
    (
        r"find {dir} -maxdepth 0 -exec echo {dir}/sentinel \; | xargs rm -rf",
        &[],
        false,
    ),
    ("parallel 'rm -rf {dir}/sentinel' ::: x", &[], true),
    ("parallel rm -rf {.} ::: {dir}/sentinel.x", &[], true),
    ("parallel rm -rf {1}{2} ::: {dir}/senti ::: nel", &[], true),
];

/// Each allowed form, the entries it needs, and exactly what it removes.
const ALLOWED: &[(&str, &[&str], &[&str])] = &[
    (
        "find tree -print0 | xargs -0 rm -rf",
        &["tree/", "tree/a b/", "tree/a b/c"],
        &["tree", "tree/a b", "tree/a b/c"],
    ),
    (
        r#"printf 'a\n"b c"\n' | xargs rm -rf"#,
        &["a", "b c", "d"],
        &["a", "b c"],
    ),
    (
        r"printf 'b c\0a\0' | xargs -0 rm -rf --",
        &["a", "b c", "d"],
        &["a", "b c"],
    ),
    (
        r"printf 'b c\n' | xargs -I{} rm -rf {}",
        &["a", "b c"],
        &["b c"],
    ),
    (
        r#"printf 'a\nd\n' | xargs sh -c 'rm -rf "$2"' _"#,
        &["a", "d"],
        &["d"],
    ),
    (
        "find tree -name x -exec rm -rf {} +",
        &["tree/", "tree/x/", "tree/x/f", "tree/y"],
        &["tree/x", "tree/x/f"],
    ),
    (
        "find tree -name y -delete",
        &["tree/", "tree/x", "tree/y"],
        &["tree/y"],
    ),
];

fn refused_by_dangerous_commands(outputs: &[Output]) -> bool {
    outputs.iter().any(|out| {
        out.status.code() == Some(2)
            && String::from_utf8_lossy(&out.stderr).contains("security.dangerous_commands")
    })
}

#[test]
fn installed_hooks_refuse_each_bypass_and_allowed_forms_delete_what_native_tools_do() {
    let project = Project::new();
    let parallel = has_gnu_parallel();
    let mut wrong = Vec::new();
    let mut index = 0;
    for (form, setup, needs_parallel) in BYPASSES {
        index += 1;
        let dir = form_dir(&project, index, setup);
        let sentinel = dir.join("sentinel");
        let command = form.replace("{dir}", dir.to_str().unwrap());
        for harness in ["claude", "codex"] {
            let outputs = replay(&project, harness, &dir, &sentinel, &command);
            if !refused_by_dangerous_commands(&outputs) {
                wrong.push(format!("{harness} allowed bypass: {command}: {outputs:?}"));
            }
        }
        if *needs_parallel && !parallel {
            eprintln!("refused; native run skipped (no GNU parallel): {command}");
            continue;
        }
        let removed = run_native(&dir, &command);
        if !removed.contains("sentinel") {
            wrong.push(format!(
                "native run did not reach the sentinel: {command}: {removed:?}"
            ));
        }
        eprintln!("refused; native run removes the sentinel: {command}");
    }
    for (form, setup, expected) in ALLOWED {
        index += 1;
        let dir = form_dir(&project, index, setup);
        let sentinel = dir.join("sentinel");
        for harness in ["claude", "codex"] {
            for output in replay(&project, harness, &dir, &sentinel, form) {
                if !output.status.success() {
                    wrong.push(format!(
                        "{harness} refused allowed form {form}: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
            }
        }
        let removed = run_native(&dir, form);
        let expected: BTreeSet<String> = expected.iter().map(|s| (*s).to_string()).collect();
        if removed != expected {
            wrong.push(format!(
                "{form}: native removed {removed:?}, expected {expected:?}"
            ));
        }
        eprintln!("allowed; native run removes exactly {expected:?}: {form}");
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
