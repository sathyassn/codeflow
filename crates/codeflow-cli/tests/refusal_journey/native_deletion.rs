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
/// `harness`, with `TMPDIR` naming `sentinel`. The payload names `cwd`;
/// the hook process runs in `process_cwd`, an unrelated directory, so a
/// verdict read from the process's own directory shows.
fn replay(
    project: &Project,
    harness: &str,
    (cwd, process_cwd): (&Path, &Path),
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
                .current_dir(process_cwd)
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

/// The native tool a form's run needs beyond POSIX `sh`, `find`, `xargs`
/// and `rm`; a form whose tool is absent is still judged by the hooks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Native {
    Any,
    /// GNU `parallel`.
    Parallel,
    /// `python3`.
    Python,
    /// BSD `find`, which reads `-f PATH`.
    BsdFind,
    /// GNU `find`, which walks `.` when no start is named.
    GnuFind,
}

/// Whether `command --version` succeeds and prints `mark`.
fn answers(command: &mut Command, mark: &str) -> bool {
    command.arg("--version").output().is_ok_and(|out| {
        out.status.success() && String::from_utf8_lossy(&out.stdout).contains(mark)
    })
}

/// Each bypass form, with `{dir}` for its directory, the entries it needs
/// and the native tool its run needs.
const BYPASSES: &[(&str, &[&str], Native)] = &[
    // PR 84 review: blanks inside a path, framed by NUL, by quotes and by
    // parallel's newline, and the round 19 option clusters.
    (
        r"printf '%s\0' '{dir}/space name/../sentinel' | xargs -0 rm -rf --",
        &["inner/", "space name -> inner"],
        Native::Any,
    ),
    (
        r#"printf '"{dir}/space name/../sentinel"' | xargs rm -rf"#,
        &["inner/", "space name -> inner"],
        Native::Any,
    ),
    (
        r"printf '{dir}/space name/../sentinel\n' | parallel rm -rf",
        &["inner/", "space name -> inner"],
        Native::Parallel,
    ),
    (
        r"printf '%s\0' '{dir}/space name/../sentinel' | xargs -t0 rm -rf --",
        &["inner/", "space name -> inner"],
        Native::Any,
    ),
    (
        r"printf '%s\0' '{dir}/space name/../sentinel' | xargs -rt0 rm -rf --",
        &["inner/", "space name -> inner"],
        Native::Any,
    ),
    (
        r"printf '%s\n' '{dir}/space name/../sentinel' | xargs -I{} rm -rf {}",
        &["inner/", "space name -> inner"],
        Native::Any,
    ),
    (
        r"printf '%s\0' '{dir}/space name/../sentinel' | xargs -0 -I{} rm -rf {}",
        &["inner/", "space name -> inner"],
        Native::Any,
    ),
    (
        r"printf 'sentinel\n' | xargs -I{} rm -rf '{dir}/{}'",
        &[],
        Native::Any,
    ),
    (
        r"printf '%s\n' '{dir}/sentinel' | xargs -n 1 rm -rf",
        &[],
        Native::Any,
    ),
    // Issue 102 and this task: grammars the guard read differently from
    // the native tool.
    (
        r#"printf 'x\n{dir}/sentinel\n' | xargs sh -c 'rm -rf "$2"' _"#,
        &[],
        Native::Any,
    ),
    (
        r"printf '{dir}/sentinel\n' > list; xargs rm -rf < list",
        &[],
        Native::Any,
    ),
    (
        r"printf '{dir}/sentinel\n' > list; cat list | xargs rm -rf",
        &[],
        Native::Any,
    ),
    (
        "cd {dir} && find tree | xargs rm -rf",
        &["tree/", "tree/x sentinel"],
        Native::Any,
    ),
    (
        r#"find {dir}/a -maxdepth 1 -exec sh -c 'rm -rf "$2"/../../sentinel' _ {} +"#,
        &["a/", "a/f/"],
        Native::Any,
    ),
    (
        r"find {dir} -name kept -execdir rm -rf ../sentinel \;",
        &[],
        Native::Any,
    ),
    (
        "find -L {dir}/link -name sentinel -exec rm -rf {} +",
        &["link -> ."],
        Native::Any,
    ),
    (
        r"find {dir} -maxdepth 0 -exec echo {dir}/sentinel \; | xargs rm -rf",
        &[],
        Native::Any,
    ),
    ("parallel 'rm -rf {dir}/sentinel' ::: x", &[], Native::Parallel),
    ("parallel rm -rf {.} ::: {dir}/sentinel.x", &[], Native::Parallel),
    ("parallel rm -rf {1}{2} ::: {dir}/senti ::: nel", &[], Native::Parallel),
    // Review 01 of PR 425: a match that is the protected directory, a path
    // built on a match through a link, and a `+` that is an argument; then
    // the same through `xargs`.
    (r"find {dir} -name sentinel -exec rm -rf {} \;", &[], Native::Any),
    ("find {dir} -name sentinel -exec rm -rf {} +", &[], Native::Any),
    (
        r"find {dir} -name sentinel -execdir rm -rf {} \;",
        &[],
        Native::Any,
    ),
    ("find {dir} -path '*sentinel*' -delete", &[], Native::Any),
    (
        r#"find {dir}/sub -name link -exec sh -c 'rm -rf "$1""/"' _ {} \;"#,
        &["sub/", "sub/link -> ../sentinel"],
        Native::Any,
    ),
    (
        r#"find {dir}/sub -name link -exec sh -c 'cd -P "$1" && rm -rf "$PWD"' _ {} \;"#,
        &["sub/", "sub/link -> ../sentinel"],
        Native::Any,
    ),
    (
        r#"find {dir}/sub -name link -execdir sh -c 'cd -P "$1" && rm -rf "$PWD"' _ {} \;"#,
        &["sub/", "sub/link -> ../sentinel"],
        Native::Any,
    ),
    (
        r"find {dir}/a -exec rm -rf + {dir}/sentinel \;",
        &["a/"],
        Native::Any,
    ),
    (
        "find {dir} -name sentinel -print0 | xargs -0 rm -rf",
        &[],
        Native::Any,
    ),
    (
        r#"find {dir}/sub -name link -print0 | xargs -0 sh -c 'cd -P "$1" && rm -rf "$PWD"' _"#,
        &["sub/", "sub/link -> ../sentinel"],
        Native::Any,
    ),
    // Review 02 of PR 425: a command that is not a plain remover (an
    // interpreter, `mv`) on a protected match, starts after `--` or BSD
    // `-f`, and no explicit start.
    (
        r"find {dir} -name sentinel -exec python3 -c 'import shutil,sys; shutil.rmtree(sys.argv[1])' {} \;",
        &[],
        Native::Python,
    ),
    (
        r"find {dir} -name sentinel -exec perl -e 'use File::Path qw(remove_tree); remove_tree($ARGV[0])' {} \;",
        &[],
        Native::Any,
    ),
    (
        r"find {dir} -name sentinel -exec mv {} {dir}/moved \;",
        &[],
        Native::Any,
    ),
    (
        "find {dir} -name sentinel -print0 | xargs -0 perl -MFile::Path=remove_tree -e 'remove_tree(@ARGV)'",
        &[],
        Native::Any,
    ),
    (
        "find {dir} -name sentinel -print0 | xargs -0 -I{} mv {} {dir}/moved",
        &[],
        Native::Any,
    ),
    (
        r"find -- {dir}/sentinel -maxdepth 0 -exec rm -rf {} \;",
        &[],
        Native::Any,
    ),
    (
        r"find -f {dir}/sentinel -- -exec rm -rf {} \;",
        &[],
        Native::BsdFind,
    ),
    (
        r"find -name sentinel -exec rm -rf {} \;",
        &[],
        Native::GnuFind,
    ),
    // A walk read line by line by `read`, whose values are every match.
    (
        r#"find {dir} -name sentinel | while read f; do rm -rf "$f"; done"#,
        &[],
        Native::Any,
    ),
    // Review 03 of PR 425: a walk given to a program in another language
    // by a pipe, a process substitution or a file.
    (
        r#"find {dir} -name sentinel | perl -ne 'chomp; system("rm", "-rf", $_)'"#,
        &[],
        Native::Any,
    ),
    (
        r#"find {dir} -name sentinel | awk '{ system("rm -rf " $0) }'"#,
        &[],
        Native::Any,
    ),
    (
        r#"find {dir} -name sentinel > {dir}/list; perl -ne 'chomp; system("rm", "-rf", $_)' {dir}/list"#,
        &[],
        Native::Any,
    ),
    (
        r"find {dir} -name sentinel | python3 -c 'import shutil, sys; [shutil.rmtree(p.strip()) for p in sys.stdin]'",
        &[],
        Native::Python,
    ),
    // Review 04 of PR 425: a reader that writes the walk to a file, and
    // the standard output opened read-write.
    (
        r#"find {dir} -name sentinel | cat > {dir}/list; perl -ne 'chomp; system("rm", "-rf", $_)' {dir}/list"#,
        &[],
        Native::Any,
    ),
    (
        r#"find {dir} -name sentinel 1<> {dir}/list; perl -ne 'chomp; system("rm", "-rf", $_)' {dir}/list"#,
        &[],
        Native::Any,
    ),
    (
        r#"find {dir} -name sentinel -print0 | xargs -0 -t true 2> {dir}/list; perl -ne 'chomp; s/^true //; system("rm", "-rf", $_)' {dir}/list"#,
        &[],
        Native::Any,
    ),
    // F5: a relative start, read from the payload's directory; the hooks
    // run from an unrelated directory, so the process's own would hold
    // no protected path.
    (r"find . -name sentinel -exec rm -rf {} \;", &[], Native::Any),
    ("find . -path '*sentinel*' -delete", &[], Native::Any),
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
    // A read-only program of a walk removes nothing.
    (
        "find tree -name x -exec grep -l x {} +",
        &["tree/", "tree/x", "tree/y"],
        &[],
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
    let elsewhere = tempfile::tempdir().unwrap();
    let elsewhere = elsewhere.path().canonicalize().unwrap();
    let gnu_find = answers(&mut Command::new("find"), "GNU");
    let has = |native: Native| match native {
        Native::Any => true,
        Native::Parallel => answers(&mut Command::new("parallel"), "GNU parallel"),
        Native::Python => answers(&mut Command::new("python3"), "Python"),
        Native::BsdFind => !gnu_find,
        Native::GnuFind => gnu_find,
    };
    let mut wrong = Vec::new();
    let mut index = 0;
    for (form, setup, native) in BYPASSES {
        index += 1;
        let dir = form_dir(&project, index, setup);
        let sentinel = dir.join("sentinel");
        let command = form.replace("{dir}", dir.to_str().unwrap());
        for harness in ["claude", "codex"] {
            let outputs = replay(&project, harness, (&dir, &elsewhere), &sentinel, &command);
            if !refused_by_dangerous_commands(&outputs) {
                wrong.push(format!("{harness} allowed bypass: {command}: {outputs:?}"));
            }
        }
        if !has(*native) {
            eprintln!("refused; native run skipped (tool absent on this host): {command}");
            continue;
        }
        let removed = run_native(&dir, &command);
        // A form may empty the sentinel rather than remove it (GNU `rm` on
        // `link/`); either reaches the protected directory.
        if !removed.iter().any(|path| path.starts_with("sentinel")) {
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
            for output in replay(&project, harness, (&dir, &elsewhere), &sentinel, form) {
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
