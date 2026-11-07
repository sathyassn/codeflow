//! TSK-1349 AC-7: both guards refuse a shell write to any path under
//! `.github/workflows/` or `.github/actions/` (a local action runs as
//! workflow code), new or edited, with any name or extension, however the
//! path is spelled or reached: letter case, `..`, `//`, a symbolic link to
//! the directory, or a hard link to a file there. Commands are judged
//! only: each is fed to the hook as a payload and never run.
use super::{codeflow, git, init_repo, run_with_stdin, write_policy};
use std::path::{Path, PathBuf};
use std::process::Output;

const POLICY: &str =
    r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"}}"#;

/// Writes that reach a workflow. `wf` is a symbolic link to the workflow
/// directory and `linked.yml` a hard link to `release.yml`.
const REFUSED: &[&str] = &[
    "echo x > .github/workflows/codeflow-ci.yml",
    "echo x > .github/workflows/new.yml",
    "echo x > .github/workflows/codeflow-admission.yml",
    "echo x >> .github/workflows/release.yml",
    "touch .github/workflows/notes.txt",
    "cp /tmp/a .github/workflows/new.yaml",
    "mv /tmp/a .github/workflows/new.yml",
    "sed -i.bak s/a/b/ .github/workflows/release.yml",
    "echo x > .GitHub/Workflows/new.yml",
    "echo x > docs/../.github/workflows/new.yml",
    "echo x > ./.github//workflows/new.yml",
    "sh -c 'echo x > .github/workflows/new.yml'",
    "echo x > .github/actions/build/action.yml",
    "echo x > .github/actions/new/action.yaml",
    "touch .github/actions/build/run.sh",
    "echo x > .GitHub/Actions/build/action.yml",
    "echo x > docs/../.github/actions/build/action.yml",
    "sh -c 'echo x > .github/actions/build/action.yml'",
];

/// Interpreter code that opens a workflow. exec-guard reads the paths an
/// interpreter is given; git-guard judges shell words only, so it leaves
/// these to exec-guard, which every harness runs beside it.
const REFUSED_BY_EXEC: &[&str] = &[
    "python3 -c 'open(\".github/workflows/new.yml\", \"w\")'",
    "python3 -c 'open(\".github/actions/build/action.yml\", \"w\")'",
];

/// The link forms, on a host with symbolic and hard links.
#[cfg(unix)]
const REFUSED_LINKS: &[&str] = &[
    "echo x > wf/new.yml",
    "echo x > wf/release.yml",
    "ln -s /tmp/a .github/workflows/l.yml",
    "ln .github/workflows/release.yml hard.yml",
    "ln wf/release.yml hard.yml",
    "cp -l .github/workflows/release.yml hard.yml",
    "echo x > linked.yml",
    "echo x >> linked.yml",
    "echo x > act/build/action.yml",
    "ln .github/actions/build/action.yml hard-action.yml",
    "echo x > linked-action.yml",
];

/// Lines that read workflows or write elsewhere, which stay admitted.
const ADMITTED: &[&str] = &[
    "cat .github/workflows/release.yml",
    "ls .github/workflows",
    "git diff .github/workflows/release.yml",
    "git log -- .github/workflows",
    "cp .github/workflows/release.yml docs/copy.yml",
    "echo x > docs/note.md",
    "echo x > .github/pull_request_template.md",
    "cat .github/actions/build/action.yml",
    // Ordinary work outside enforcement paths: exec-guard now runs
    // git-guard's writer checks, and these stay admitted.
    "echo x > notes.md",
    "printf x > /tmp/codeflow-scratch.txt",
    "cargo build",
    "mkdir -p target/tmp && echo x > target/tmp/out.txt",
    "rm -rf target/tmp",
    "cp notes.md docs/notes-copy.md && mv docs/notes-copy.md docs/moved.md",
    "echo x | tee build.log",
    "sed -i.bak s/a/b/ docs/two.md",
];

/// Ordinary hard links, which are not a workflow's second name.
#[cfg(unix)]
const ADMITTED_LINKS: &[&str] = &["ln docs/two.md docs/four.md", "echo x > docs/three.md"];

fn hook(hook: &str, root: &Path, command: &str) -> Output {
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "cwd": root,
    });
    run_with_stdin(
        codeflow()
            .args(["hook", hook, "--contract", "3"])
            .current_dir(root)
            .env_remove("CLAUDE_PROJECT_DIR"),
        &payload.to_string(),
    )
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A project on an unprotected branch with two workflows and a local
/// action, a symbolic link to each directory, a hard link to one file in
/// each and an ordinary pair of hard-linked notes.
fn fixture(root: &Path) -> PathBuf {
    init_repo(root, "feat/w");
    write_policy(root, POLICY);
    let workflows = root.join(".github/workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    for name in ["codeflow-ci.yml", "release.yml"] {
        std::fs::write(workflows.join(name), "on: push\n").unwrap();
    }
    std::fs::write(root.join(".github/pull_request_template.md"), "x\n").unwrap();
    let actions = root.join(".github/actions/build");
    std::fs::create_dir_all(&actions).unwrap();
    std::fs::write(actions.join("action.yml"), "runs:\n  using: composite\n").unwrap();
    std::fs::write(root.join("docs/two.md"), "x\n").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&workflows, root.join("wf")).unwrap();
        std::fs::hard_link(workflows.join("release.yml"), root.join("linked.yml")).unwrap();
        std::fs::hard_link(root.join("docs/two.md"), root.join("docs/three.md")).unwrap();
        std::os::unix::fs::symlink(root.join(".github/actions"), root.join("act")).unwrap();
        std::fs::hard_link(actions.join("action.yml"), root.join("linked-action.yml")).unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: fixture"]);
    root.to_path_buf()
}

fn refused() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut lines = REFUSED.to_vec();
    #[cfg(unix)]
    lines.extend(REFUSED_LINKS);
    lines
}

fn admitted() -> Vec<String> {
    let mut lines: Vec<String> = ADMITTED.iter().map(|line| (*line).to_string()).collect();
    #[cfg(unix)]
    lines.extend(ADMITTED_LINKS.iter().map(|line| (*line).to_string()));
    // A scratch file in the temporary folder, named as a literal path: an
    // unresolved `$TMPDIR` is refused by exec-guard's startup-file rule,
    // which this task does not change.
    let scratch = codeflow_core::portable_path::slashed(&std::env::temp_dir().join("tsk1349.txt"));
    lines.push(format!("echo x > {scratch}"));
    lines
}

#[test]
fn both_guards_refuse_every_write_under_the_workflow_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = fixture(&project(dir.path()));
    let mut admitted = Vec::new();
    for command in refused() {
        for name in ["git-guard", "exec-guard"] {
            let out = hook(name, &root, command);
            if out.status.code() != Some(2)
                || !stderr(&out).contains("policy rule git.hook_integrity")
            {
                admitted.push(format!("{name}: {command}\n{}", stderr(&out)));
            }
        }
    }
    for command in REFUSED_BY_EXEC {
        let out = hook("exec-guard", &root, command);
        if out.status.code() != Some(2) || !stderr(&out).contains("policy rule git.hook_integrity")
        {
            admitted.push(format!("exec-guard: {command}\n{}", stderr(&out)));
        }
    }
    assert!(
        admitted.is_empty(),
        "workflow writes admitted:\n{}",
        admitted.join("\n")
    );
}

#[test]
fn neither_guard_refuses_a_workflow_read_or_an_ordinary_write() {
    let dir = tempfile::tempdir().unwrap();
    let root = fixture(&project(dir.path()));
    let mut refused = Vec::new();
    for command in admitted() {
        for name in ["git-guard", "exec-guard"] {
            let out = hook(name, &root, &command);
            if out.status.code() != Some(0) {
                refused.push(format!("{name}: {command}\n{}", stderr(&out)));
            }
        }
    }
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}

/// The harness's own file tools reach the same protected set through
/// edit-guard: a new or edited workflow refuses, a note elsewhere does not.
#[test]
fn edit_guard_refuses_a_tool_write_under_the_workflow_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = fixture(&project(dir.path()));
    let edit = |tool: &str, path: PathBuf| {
        let payload = serde_json::json!({
            "tool_name": tool,
            "tool_input": { "file_path": path, "content": "on: push\n",
                            "old_string": "on: push", "new_string": "on: pull_request" },
            "cwd": root,
        });
        run_with_stdin(
            codeflow()
                .args(["hook", "edit-guard", "--contract", "3"])
                .current_dir(&root)
                .env_remove("CLAUDE_PROJECT_DIR"),
            &payload.to_string(),
        )
    };
    let workflows = root.join(".github/workflows");
    for (tool, path) in [
        ("Write", workflows.join("codeflow-admission.yml")),
        ("Edit", workflows.join("release.yml")),
        ("Write", root.join(".GitHub/Workflows/new.yml")),
        ("Write", root.join("docs/../.github/workflows/new.yml")),
        ("Edit", root.join(".github/actions/build/action.yml")),
        ("Write", root.join(".github/actions/new/action.yml")),
    ] {
        let out = edit(tool, path.clone());
        assert_eq!(
            out.status.code(),
            Some(2),
            "{tool} {}: {}",
            path.display(),
            stderr(&out)
        );
    }
    let out = edit("Write", root.join("docs/note.md"));
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

/// A spelling in another case is matched by the guard, not by the disk. The
/// project holds no `.github`, so no disk can fold the spelling onto an
/// existing directory: this runs the same on every host. The admitted lines
/// show that only the protected paths in those directories are folded.
#[test]
fn the_guards_fold_letter_case_without_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let root = project_without_enforcement_folders(dir.path());
    let mut wrong = Vec::new();
    for name in ["git-guard", "exec-guard"] {
        for command in [
            "echo x > .GitHub/Workflows/new.yml",
            "echo x > .GITHUB/WORKFLOWS/new.yml",
            "echo x > .github/Workflows/new.yml",
            "echo x > .GitHub/Actions/build/action.yml",
            "touch .GitHub/Workflows/new.yml",
            "echo x > .CodeFlow/policy.json",
            "echo x > .CODEFLOW/git-hooks/pre-commit",
            "echo x > .Claude/settings.json",
            "echo x > .claude/Settings.json",
            "echo x > .Codex/hooks.json",
            "echo x > .Grok/sandbox.toml",
        ] {
            let out = hook(name, &root, command);
            if out.status.code() != Some(2)
                || !stderr(&out).contains("policy rule git.hook_integrity")
            {
                wrong.push(format!("{name} admitted {command}\n{}", stderr(&out)));
            }
        }
        for command in [
            "echo x > .GitHub/notes.md",
            "echo x > .GitHub/Workflows-old/new.yml",
            "echo x > .Claude/notes.md",
            "echo x > .CodeFlow/notes.md",
        ] {
            let out = hook(name, &root, command);
            if out.status.code() != Some(0) {
                wrong.push(format!("{name} refused {command}\n{}", stderr(&out)));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The project in `dir`, committed, with no `.github` and no git hooks
/// folder, so a spelling cannot reach an existing entry through the disk.
fn project_without_enforcement_folders(dir: &Path) -> PathBuf {
    let root = project(dir);
    init_repo(&root, "feat/w");
    write_policy(&root, POLICY);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "chore: fixture"]);
    root
}

/// The exit code of edit-guard for a file-tool write to `path` under `root`.
fn tool_write(root: &Path, path: &str) -> (Option<i32>, String) {
    tool_write_from(root, None, path)
}

/// [`tool_write`], with `home` as the hook's home directory when given.
fn tool_write_from(root: &Path, home: Option<&Path>, path: &str) -> (Option<i32>, String) {
    let payload = serde_json::json!({
        "tool_name": "Write",
        "tool_input": { "file_path": root.join(path), "content": "on: push\n" },
        "cwd": root,
    });
    let mut command = codeflow();
    command
        .args(["hook", "edit-guard", "--contract", "3"])
        .current_dir(root)
        .env_remove("CLAUDE_PROJECT_DIR");
    if let Some(home) = home {
        command.env("HOME", home).env("USERPROFILE", home);
    }
    let out = run_with_stdin(&mut command, &payload.to_string());
    (out.status.code(), stderr(&out))
}

/// The file tools fold case the same way, with no disk to do it for them.
#[test]
fn the_file_tools_fold_letter_case_without_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let root = project_without_enforcement_folders(dir.path());
    for (path, code) in [
        (".GitHub/Workflows/new.yml", 2),
        (".GitHub/Actions/build/action.yml", 2),
        (".Claude/settings.json", 2),
        (".GitHub/notes.md", 0),
    ] {
        let (got, said) = tool_write(&root, path);
        assert_eq!(got, Some(code), "{path}: {said}");
    }
}

/// Spellings APFS folds onto a protected name (`ſ` U+017F, the Kelvin sign
/// U+212A, the `fl` ligature U+FB02), and ones HFS+ ignores (zero-width
/// joiner U+200D). Each is a name the guard cannot normalise for every disk,
/// so it refuses on every host. Payloads only, judged and never executed.
const NON_ASCII_WRITES: &[&str] = &[
    "mkdir -p .github/workflow\u{17f} && echo x > .github/workflow\u{17f}/ci.yml",
    "mkdir -p .github/action\u{17f} && echo x > .github/action\u{17f}/run/action.yml",
    "mkdir -p .codeflow/git-hook\u{17f} && echo x > .codeflow/git-hook\u{17f}/pre-commit",
    "echo x > .github/work\u{fb02}ows/ci.yml",
    "echo x > .codeflow/git-hoo\u{212a}s/pre-commit",
    "echo x > .codeflow/policy.j\u{17f}on",
    "echo x > .claude/\u{17f}ettings.json",
    "echo x > .gro\u{212a}/sandbox.toml",
    "echo x > .code\u{fb02}ow/policy.json",
    "mkdir -p .code\u{fb02}ow/git-hooks",
    "echo x > .gi\u{200d}thub/workflows/ci.yml",
    "echo x > \u{200d}.github/workflows/ci.yml",
    "touch .github/workflow\u{17f}/ci.yml",
    "cp notes.md .github/workflow\u{17f}/ci.yml",
    "mv notes.md .github/workflow\u{17f}/ci.yml",
    "sh -c 'echo x > .github/workflow\u{17f}/ci.yml'",
    "ln -s .github/workflow\u{17f} wf && echo x > wf/ci.yml",
    "rm -rf .code\u{fb02}ow",
];

/// A non-ASCII spelling of a protected name refuses before the entry exists,
/// on both hooks and through the file tools; ordinary non-ASCII names stay
/// admitted.
#[test]
fn a_non_ascii_spelling_of_a_protected_name_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let root = project_without_enforcement_folders(dir.path());
    let mut wrong = Vec::new();
    for name in ["git-guard", "exec-guard"] {
        for command in NON_ASCII_WRITES {
            let out = hook(name, &root, command);
            if out.status.code() != Some(2)
                || !stderr(&out).contains("policy rule git.hook_integrity")
            {
                wrong.push(format!("{name} admitted {command}\n{}", stderr(&out)));
            }
        }
        for command in [
            "echo x > docs/caf\u{e9}.md",
            "echo x > \u{65e5}\u{672c}\u{8a9e}.txt",
            "mkdir -p docs/workflow\u{17f}",
        ] {
            let out = hook(name, &root, command);
            if out.status.code() != Some(0) {
                wrong.push(format!("{name} refused {command}\n{}", stderr(&out)));
            }
        }
    }
    for (path, code) in [
        (".github/workflow\u{17f}/ci.yml", 2),
        (".codeflow/git-hook\u{17f}/pre-commit", 2),
        (".gro\u{212a}/sandbox.toml", 2),
        (".code\u{fb02}ow/policy.json", 2),
        ("docs/caf\u{e9}.md", 0),
        ("\u{65e5}\u{672c}\u{8a9e}.txt", 0),
    ] {
        let (got, said) = tool_write(&root, path);
        if got != Some(code) {
            wrong.push(format!("edit-guard gave {got:?} for {path}\n{said}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// One line per group of comparing checks: each compares a name as ASCII, so
/// each needs the line in the spelling the disk would read. Payloads only.
const NON_ASCII_BY_CHECK: &[(&str, &str)] = &[
    (
        "interpreter arguments",
        "python3 -c 'print(open(\".aw\u{17f}/credentials\").read())'",
    ),
    ("secret reads", "cat ~/.aw\u{17f}/credentials"),
    ("startup files", "echo x >> ~/.ba\u{17f}hrc"),
    ("git config files", "echo x > ~/.gitcon\u{fb01}g"),
    ("git-guard creation", "git init .code\u{fb02}ow/x"),
    ("program names", "/usr/bin/\u{17f}udo ls"),
];

/// The installed pair refuses a non-ASCII spelling at every kind of check,
/// and the file tools refuse it for a path anchored at home.
#[test]
fn every_comparing_check_reads_the_ascii_spelling() {
    let dir = tempfile::tempdir().unwrap();
    let root = project_without_enforcement_folders(dir.path());
    let mut wrong = Vec::new();
    for (check, command) in NON_ASCII_BY_CHECK {
        let refused = ["git-guard", "exec-guard"]
            .iter()
            .any(|name| hook(name, &root, command).status.code() == Some(2));
        if !refused {
            wrong.push(format!("{check}: the pair admitted {command}"));
        }
    }
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    for name in [".gro\u{212a}/config.toml", ".codex/rule\u{17f}/x"] {
        let path = home.join(name);
        let (got, said) = tool_write_from(&root, Some(&home), &path.to_string_lossy());
        if got != Some(2) {
            wrong.push(format!(
                "home rule: edit-guard gave {got:?} for {name}\n{said}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// An empty project folder inside `dir`.
fn project(dir: &Path) -> PathBuf {
    let root = dir.join("p");
    std::fs::create_dir_all(&root).unwrap();
    root
}
