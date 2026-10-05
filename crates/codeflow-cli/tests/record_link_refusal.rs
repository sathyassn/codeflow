//! Journey (TSK-244, issue 94): on a fresh `codeflow init --full` project, a
//! symbolic link committed at `project-management` or `docs/decisions`
//! makes every record verb refuse with the link named, and nothing is
//! written, created or deleted where the link points. The binary under test
//! is the one Cargo built; the installed `codeflow` on `PATH` is never used.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn with_env(command: &mut Command) -> &mut Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    command
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_EVENT_NAME")
}

fn codeflow(root: &Path, args: &[&str]) -> Output {
    with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(args)
        .current_dir(root)
        .output()
        .expect("codeflow runs")
}

fn git(root: &Path, args: &[&str]) {
    let out = with_env(&mut Command::new("git"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn ok(out: &Output, what: &str) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{what} failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// Every file and folder beneath `dir`, read without following.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).unwrap().flatten() {
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                stack.push(path.clone());
            }
            let bytes = if kind.is_file() {
                std::fs::read(&path).unwrap()
            } else {
                Vec::new()
            };
            out.insert(path.strip_prefix(dir).unwrap().to_path_buf(), bytes);
        }
    }
    out
}

/// Move `relative` outside and leave a link to it in its place.
fn plant(root: &Path, relative: &str, outside: &Path) -> PathBuf {
    let moved = outside.join(relative.replace('/', "-"));
    std::fs::rename(root.join(relative), &moved).unwrap();
    std::os::unix::fs::symlink(&moved, root.join(relative)).unwrap();
    moved
}

fn unplant(root: &Path, relative: &str, moved: &Path) {
    std::fs::remove_file(root.join(relative)).unwrap();
    std::fs::rename(moved, root.join(relative)).unwrap();
}

/// Run a verb that must fail, name the link, and leave the outside tree
/// unchanged; a breach is collected so every verb is judged in one run.
fn refused(root: &Path, outside: &Path, args: &[&str], link: &str, breaches: &mut Vec<String>) {
    let before = snapshot(outside);
    let out = codeflow(root, args);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        breaches.push(format!("{args:?} succeeded:\n{text}"));
    } else if !text.contains(&format!("{link} is a symbolic link")) {
        breaches.push(format!("{args:?} does not name the link:\n{text}"));
    }
    if snapshot(outside) != before {
        breaches.push(format!("{args:?} changed the link target"));
    }
}

#[test]
#[allow(clippy::too_many_lines)] // One journey plants each link in the order a project meets them.
fn record_verbs_refuse_a_link_above_the_record() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    let target = "integration/EPC-001-links";
    git(&root, &["switch", "-q", "-c", target]);
    git(&root, &["switch", "-q", "-c", "plan/links"]);
    ok(&codeflow(&root, &["epic", "new", "outcome"]), "epic new");
    ok(
        &codeflow(&root, &["spec", "new", "--for", "EPC-001", "contract"]),
        "spec new",
    );
    let task = [
        "task", "new", "--epic", "EPC-001", "--into", target, "first",
    ];
    ok(&codeflow(&root, &task), "task new");
    git(&root, &["add", "-A"]);
    git(
        &root,
        &["commit", "-q", "-m", "chore: plan the link journey"],
    );
    let mut breaches = Vec::new();

    // A linked project-management: every creating verb and the status verb
    // refuse; status and orient say why the work summary is missing.
    let moved = plant(&root, "project-management", &outside);
    let pm = "project-management";
    refused(
        &root,
        &outside,
        &["epic", "new", "second"],
        pm,
        &mut breaches,
    );
    refused(&root, &outside, &task, pm, &mut breaches);
    let spec = ["spec", "new", "--for", "EPC-001", "more"];
    refused(&root, &outside, &spec, pm, &mut breaches);
    let block = [
        "task",
        "status",
        "TSK-001",
        "blocked",
        "--reason",
        "wait",
        "--owner",
        "primary",
        "--revisit",
        "later",
    ];
    refused(&root, &outside, &block, pm, &mut breaches);
    for verb in ["status", "orient"] {
        let before = snapshot(&outside);
        let out = codeflow(&root, &[verb]);
        let text = String::from_utf8_lossy(&out.stdout);
        if !text.contains("project-management is a symbolic link") {
            breaches.push(format!("{verb} does not name the link:\n{text}"));
        }
        if snapshot(&outside) != before {
            breaches.push(format!("{verb} changed the link target"));
        }
    }
    unplant(&root, pm, &moved);

    // A linked kind folder.
    let moved = plant(&root, "project-management/tasks", &outside);
    refused(
        &root,
        &outside,
        &task,
        "project-management/tasks",
        &mut breaches,
    );
    unplant(&root, "project-management/tasks", &moved);

    // A linked docs/decisions.
    std::fs::create_dir_all(root.join("docs/decisions")).unwrap();
    let moved = plant(&root, "docs/decisions", &outside);
    let adr = ["adr", "new", "Adopt a cache"];
    refused(&root, &outside, &adr, "docs/decisions", &mut breaches);
    unplant(&root, "docs/decisions", &moved);

    // The old temporary name beside a new follow-up is never written
    // through: links planted there stay untouched and the verb succeeds.
    let sentinel = outside.join("sentinel");
    std::fs::write(&sentinel, "outside").unwrap();
    for n in 2..=8 {
        let tmp = root.join(format!("project-management/tasks/TSK-{n:03}.tmp"));
        std::os::unix::fs::symlink(&sentinel, tmp).unwrap();
    }
    let follow = ["task", "new", "--follow-up-of", "TSK-001", "Tidy"];
    let out = codeflow(&root, &follow);
    if !out.status.success() {
        breaches.push(format!(
            "{follow:?} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    if std::fs::read_to_string(&sentinel).unwrap() != "outside" {
        breaches.push(format!("{follow:?} wrote through the old temporary name"));
    }
    assert!(breaches.is_empty(), "{}", breaches.join("\n\n"));
}
