//! Journey (TSK-101 AC-11): the shared id registry through the CLI Cargo
//! built, on a fresh `codeflow init --full` project with a local bare
//! remote and on a project with no remote. The installed git hooks run the
//! binary under test; the `codeflow` on the user's `PATH` is never used.

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

fn run_git(root: &Path, args: &[&str]) -> Output {
    with_env(&mut Command::new("git"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("git runs")
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = run_git(root, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn ok(out: &Output, what: &str) -> String {
    assert!(out.status.success(), "{what} failed:\n{}", text(out));
    text(out)
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

const LINE: &str = "integration/EPC-001-registry";

/// A fresh full-tier project on an integration line, pushed to a bare
/// remote. Returns (tempdir, project, bare remote).
fn project_with_remote() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("remote.git");
    git(
        dir.path(),
        &["init", "-q", "--bare", "-b", "main", "remote.git"],
    );
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    // The destination's default branch holds the scaffold, as a project's
    // remote does; the release scope reads its policy (SPC-013 R-120).
    git(
        &bare,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            "HEAD:refs/heads/main",
        ],
    );
    git(&root, &["switch", "-q", "-c", LINE]);
    git(&root, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&root, &["push", "-q", "origin", LINE]);
    // AC-1 reads landed policy: fetch the advertised main as well as the line.
    git(&root, &["fetch", "-q", "origin"]);
    // `init` committed a scaffold holding records (ADR-0001), so a
    // maintainer seeds the registry once before the first issue (R-6, R-24).
    let before = codeflow(&root, &["epic", "new", "too early"]);
    assert!(!before.status.success());
    assert!(
        text(&before).contains("codeflow ids seed"),
        "{}",
        text(&before)
    );
    let seeded = ok(&codeflow(&root, &["ids", "seed"]), "ids seed");
    assert!(seeded.contains("registered 1 new id"), "{seeded}");
    (dir, root, bare)
}

#[test]
#[allow(clippy::too_many_lines)] // one journey: issue, clones, guards, damage, restore, CI
fn a_fresh_project_issues_unique_ids_and_its_hooks_keep_the_registry_append_only() {
    let (dir, root, bare) = project_with_remote();

    // `new` issues on the authority; the push runs the installed pre-push
    // hook, which accepts a pure addition.
    let epic = ok(
        &codeflow(&root, &["epic", "new", "registry outcome"]),
        "epic new",
    );
    assert!(
        epic.contains("EPC-001") && epic.contains("reserved on the authority"),
        "{epic}"
    );
    let listed = git(
        &bare,
        &["ls-tree", "-r", "--name-only", "codeflow/registry"],
    );
    assert_eq!(listed, "ids/ADR/0001.toml\nids/EPC/001.toml");
    let record = std::fs::read_to_string(root.join("project-management/epics/EPC-001.md")).unwrap();
    let bound = git(&bare, &["show", "codeflow/registry:ids/EPC/001.toml"]);
    let uid_line = record
        .lines()
        .find(|line| line.starts_with("uid: "))
        .unwrap();
    let uid = uid_line[5..].split_whitespace().next().unwrap();
    assert!(
        bound.contains(&format!("uid = \"{uid}\"")),
        "{bound}\n{record}"
    );
    let first = ok(
        &codeflow(
            &root,
            &["task", "new", "--epic", "EPC-001", "--into", LINE, "first"],
        ),
        "task new",
    );
    assert!(first.contains("TSK-001"), "{first}");
    commit(&root, "chore: plan the registry journey");
    git(&root, &["push", "-q", "origin", LINE]);

    // A second clone takes the next number, never the same one.
    let other = dir.path().join("other");
    git(
        dir.path(),
        &[
            "clone",
            "--no-local",
            "-q",
            "-b",
            LINE,
            bare.to_str().unwrap(),
            "other",
        ],
    );
    let second = ok(
        &codeflow(
            &other,
            &["task", "new", "--epic", "EPC-001", "--into", LINE, "second"],
        ),
        "task new in a second clone",
    );
    assert!(second.contains("TSK-002"), "{second}");
    let third = ok(
        &codeflow(
            &root,
            &["task", "new", "--epic", "EPC-001", "--into", LINE, "third"],
        ),
        "task new after the other clone",
    );
    assert!(third.contains("TSK-003"), "{third}");

    // The installed pre-push hook refuses a deletion and a force push of
    // the registry.
    git(&root, &["fetch", "-q", "origin"]);
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            "../reg",
            "origin/codeflow/registry",
        ],
    );
    let reg = dir.path().join("reg");
    git(&reg, &["rm", "-q", "ids/TSK/003.toml"]);
    git(&reg, &["commit", "-q", "-m", "chore: remove a reservation"]);
    // Hooks live under the code tree (`.codeflow/git-hooks`), so the push is
    // made from the project checkout, as an agent or maintainer would.
    let deletion = git(&reg, &["rev-parse", "HEAD"]);
    let refused = run_git(
        &root,
        &[
            "push",
            "origin",
            &format!("{deletion}:refs/heads/codeflow/registry"),
        ],
    );
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("deletes ids/TSK/003.toml"),
        "{}",
        text(&refused)
    );
    let older = git(&reg, &["rev-parse", "HEAD~2"]);
    let forced = run_git(
        &root,
        &[
            "push",
            "--force",
            "origin",
            &format!("{older}:refs/heads/codeflow/registry"),
        ],
    );
    assert!(!forced.status.success(), "{}", text(&forced));
    assert!(text(&forced).contains("force"), "{}", text(&forced));

    // A host without prevention accepts the deletion from a clone without
    // hooks. Allocation reads history: issue refuses and names the restore;
    // `ids check` fails with current damage.
    let plain = dir.path().join("plain");
    git(
        dir.path(),
        &[
            "clone",
            "--no-local",
            "-q",
            "-b",
            "codeflow/registry",
            bare.to_str().unwrap(),
            "plain",
        ],
    );
    git(&plain, &["rm", "-q", "ids/TSK/003.toml"]);
    git(&plain, &["commit", "-q", "-m", "remove"]);
    git(&plain, &["push", "-q", "origin", "HEAD:codeflow/registry"]);
    let blocked = codeflow(
        &root,
        &[
            "task", "new", "--epic", "EPC-001", "--into", LINE, "blocked",
        ],
    );
    assert!(!blocked.status.success());
    assert!(
        text(&blocked).contains("codeflow ids restore TSK-003"),
        "{}",
        text(&blocked)
    );
    let damaged = codeflow(&root, &["ids", "check"]);
    assert_eq!(damaged.status.code(), Some(1), "{}", text(&damaged));
    assert!(
        text(&damaged).contains("current damage"),
        "{}",
        text(&damaged)
    );

    // The typed restore passes the pre-push hook; issue and the check are
    // healthy again, and the deleted top number is not reissued.
    ok(
        &codeflow(&root, &["ids", "restore", "TSK-003"]),
        "ids restore",
    );
    let healthy = ok(
        &codeflow(&root, &["ids", "check"]),
        "ids check after restore",
    );
    assert!(healthy.contains("history (repaired)"), "{healthy}");
    let fourth = ok(
        &codeflow(
            &root,
            &["task", "new", "--epic", "EPC-001", "--into", LINE, "fourth"],
        ),
        "task new after restore",
    );
    assert!(fourth.contains("TSK-004"), "{fourth}");

    // CI: a bound record passes the merge rule; a hand-written one fails
    // until a maintainer admits it.
    commit(&root, "chore: plan more tasks");
    git(&root, &["push", "-q", "origin", LINE]);
    git(&root, &["switch", "-q", "-c", "plan/fork-record"]);
    let hand = root.join("project-management/tasks/TSK-040.md");
    let template =
        std::fs::read_to_string(root.join("project-management/tasks/TSK-004.md")).unwrap();
    let fork_uid = "0b9c7e5a-3f1d-4c2e-9a8b-7d6e5f4a3b21";
    let fork: String = template
        .lines()
        .map(|line| {
            if line.starts_with("uid: ") {
                format!("uid: {fork_uid}")
            } else {
                line.replace("TSK-004", "TSK-040")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&hand, fork + "\n").unwrap();
    commit(&root, "chore: add a hand-written record");
    let head = git(&root, &["rev-parse", "HEAD"]);
    let red = codeflow(&root, &["ids", "check", "--base", LINE, "--head", &head]);
    assert_eq!(red.status.code(), Some(1), "{}", text(&red));
    assert!(
        text(&red).contains("TSK-040: not reserved"),
        "{}",
        text(&red)
    );
    let ci = codeflow(
        &root,
        &[
            "ci",
            "--base",
            LINE,
            "--head",
            &head,
            "--branch",
            "plan/fork-record",
        ],
    );
    assert!(text(&ci).contains("work.id_registry"), "{}", text(&ci));
    ok(
        &codeflow(
            &root,
            &["ids", "admit", "project-management/tasks/TSK-040.md"],
        ),
        "ids admit",
    );
    let green = ok(
        &codeflow(&root, &["ids", "check", "--base", LINE, "--head", &head]),
        "ids check after admission",
    );
    assert!(
        green.contains(&format!("bound: TSK-040 -> {fork_uid}")),
        "{green}"
    );
    let doctor = codeflow(&root, &["doctor", "--check", "id-registry"]);
    assert!(
        text(&doctor).contains("reduced assurance"),
        "{}",
        text(&doctor)
    );
}

/// Run the in-session guard on a harness payload for `command`.
fn guard(root: &Path, command: &str) -> Output {
    use std::io::Write as _;
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": root.to_string_lossy(),
    })
    .to_string();
    let mut child = with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(["hook", "git-guard"])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("guard runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// A fresh checkout of the remote, set up the way the registry workflow
/// fetches it, and its `codeflow ids check` verdict.
fn ci_check(dir: &Path, bare: &Path, name: &str) -> Output {
    git(
        dir,
        &[
            "clone",
            "--no-local",
            "-q",
            "-b",
            LINE,
            bare.to_str().unwrap(),
            name,
        ],
    );
    let checkout = dir.join(name);
    git(
        &checkout,
        &[
            "fetch",
            "-q",
            "--no-tags",
            "origin",
            "refs/heads/codeflow/registry:refs/remotes/origin/codeflow/registry",
        ],
    );
    codeflow(&checkout, &["ids", "check"])
}

/// SPC-013 R-104, R-108: a restore that binds a number to another uid is
/// refused by the pre-push hook, the guard refuses the push that would skip
/// that hook, and CI names the damage when a host without prevention takes
/// it; the typed restore then repairs it.
#[test]
fn a_rebinding_restore_is_refused_by_pre_push_the_guard_and_ci() {
    let (dir, root, bare) = project_with_remote();
    ok(&codeflow(&root, &["epic", "new", "outcome"]), "epic new");
    ok(
        &codeflow(
            &root,
            &["task", "new", "--epic", "EPC-001", "--into", LINE, "first"],
        ),
        "task new",
    );
    commit(&root, "chore: plan the first task");
    git(&root, &["push", "-q", "origin", LINE]);

    // A host without prevention takes a deletion from a clone without hooks.
    git(
        dir.path(),
        &[
            "clone",
            "--no-local",
            "-q",
            "-b",
            "codeflow/registry",
            bare.to_str().unwrap(),
            "plain",
        ],
    );
    let plain = dir.path().join("plain");
    let bound = std::fs::read_to_string(plain.join("ids/TSK/001.toml")).unwrap();
    git(&plain, &["rm", "-q", "ids/TSK/001.toml"]);
    git(&plain, &["commit", "-q", "-m", "remove"]);
    git(&plain, &["push", "-q", "origin", "HEAD:codeflow/registry"]);

    // A hand-made "restore" puts TSK-001 back bound to another uid.
    let uid_line = bound
        .lines()
        .find(|line| line.starts_with("uid = "))
        .expect("the entry binds a uid");
    let forged = bound.replace(uid_line, "uid = \"0b9c7e5a-3f1d-4c2e-9a8b-7d6e5f4a3b21\"");
    assert_ne!(forged, bound);
    std::fs::create_dir_all(plain.join("ids/TSK")).unwrap();
    std::fs::write(plain.join("ids/TSK/001.toml"), forged).unwrap();
    git(&plain, &["add", "ids/TSK/001.toml"]);
    git(&plain, &["commit", "-q", "-m", "restore: TSK-001"]);
    let rebinding = git(&plain, &["rev-parse", "HEAD"]);
    git(&root, &["fetch", "-q", plain.to_str().unwrap(), "HEAD"]);
    let target = format!("{rebinding}:refs/heads/codeflow/registry");

    // Pre-push: the project's hook refuses it and names the rebinding.
    let refused = run_git(&root, &["push", "origin", &target]);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("restore would bind TSK-001 to another uid"),
        "{}",
        text(&refused)
    );
    // Guard: the push that would skip that hook is refused before it runs,
    // and the ordinary push is left to the hook.
    let skipped = guard(&root, &format!("git push --no-verify origin {target}"));
    assert_eq!(skipped.status.code(), Some(2), "{}", text(&skipped));
    assert!(
        text(&skipped).contains("registry.append_only"),
        "{}",
        text(&skipped)
    );
    let plain_push = guard(&root, &format!("git push origin {target}"));
    assert_eq!(plain_push.status.code(), Some(0), "{}", text(&plain_push));

    // CI: a host without prevention takes it from the clone without hooks;
    // the registry check names it as current damage.
    git(&plain, &["push", "-q", "origin", "HEAD:codeflow/registry"]);
    let red = ci_check(dir.path(), &bare, "ci-red");
    assert_eq!(red.status.code(), Some(1), "{}", text(&red));
    assert!(
        text(&red).contains("current damage")
            && text(&red).contains("restore would bind TSK-001 to another uid"),
        "{}",
        text(&red)
    );

    // Control: the typed restore returns the first binding and passes the
    // hook; CI is green with the rebinding kept as repaired history.
    git(&root, &["fetch", "-q", "origin"]);
    ok(
        &codeflow(&root, &["ids", "restore", "TSK-001"]),
        "ids restore",
    );
    assert_eq!(
        git(&bare, &["show", "codeflow/registry:ids/TSK/001.toml"]),
        bound.trim_end()
    );
    let green = ci_check(dir.path(), &bare, "ci-green");
    assert!(green.status.success(), "{}", text(&green));
    assert!(
        text(&green).contains("history (repaired)"),
        "{}",
        text(&green)
    );
}

#[test]
fn a_project_without_a_remote_issues_from_its_own_registry() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("solo");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    git(&root, &["switch", "-q", "-c", LINE]);
    let epic = ok(&codeflow(&root, &["epic", "new", "solo"]), "epic new");
    assert!(epic.contains("reserved in the local registry"), "{epic}");
    let task = ok(
        &codeflow(
            &root,
            &[
                "task",
                "new",
                "--epic",
                "EPC-001",
                "--into",
                LINE,
                "solo task",
            ],
        ),
        "task new",
    );
    assert!(task.contains("TSK-001"), "{task}");
    // The first issue seeded the committed scaffold's ADR locally.
    assert_eq!(
        git(
            &root,
            &["ls-tree", "-r", "--name-only", "codeflow/registry"]
        ),
        "ids/ADR/0001.toml\nids/EPC/001.toml\nids/TSK/001.toml"
    );
    ok(&codeflow(&root, &["ids", "check"]), "ids check");
    // Sync with no authority is a legible no-op.
    let sync = ok(&codeflow(&root, &["ids", "sync"]), "ids sync");
    assert!(sync.contains("no authority remote"), "{sync}");
}

#[test]
fn task_new_resume_writes_an_interrupted_reservation_once() {
    let (_dir, root, _bare) = project_with_remote();
    ok(
        &codeflow(&root, &["epic", "new", "resume outcome"]),
        "epic new",
    );
    commit(&root, "chore: plan");
    // Reserve as `task new` does, then stop before the record is written.
    // The library's push runs the project's hooks: point them at the binary
    // under test.
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    std::env::set_var("PATH", path);
    std::env::set_var("CODEFLOW_HOME", isolated_home());
    std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
    let mut request = codeflow_core::ids::issue::Request::issue(
        codeflow_core::ids::Kind::Tsk,
        "interrupted",
        LINE,
    );
    request.resume = Some(serde_json::json!({
        "kind": "task", "epic": "EPC-001", "standalone_reason": null,
        "into": LINE, "title": "interrupted",
    }));
    git(&root, &["config", "user.email", "journey@example.test"]);
    git(&root, &["config", "user.name", "Journey"]);
    let reserved = codeflow_core::ids::issue::reserve(&root, &request).unwrap();
    assert_eq!(reserved.id.to_string(), "TSK-001");
    assert!(!root.join("project-management/tasks/TSK-001.md").exists());
    let resumed = ok(
        &codeflow(&root, &["task", "new", "--resume", "TSK-001"]),
        "resume",
    );
    assert!(resumed.contains("TSK-001"), "{resumed}");
    let written =
        std::fs::read_to_string(root.join("project-management/tasks/TSK-001.md")).unwrap();
    assert!(
        written.contains(&format!("uid: {}", reserved.uid)),
        "{written}"
    );
    assert!(written.contains("interrupted"));
    // A second resume is refused: nothing is waiting.
    let again = codeflow(&root, &["task", "new", "--resume", "TSK-001"]);
    assert!(!again.status.success(), "{}", text(&again));
}

/// `adr new` issues the next ADR number through the registry and writes
/// the bound `uid` into the proposed record (TSK-104 AC-4).
#[test]
fn adr_new_takes_its_number_from_the_registry() {
    let (dir, root, bare) = project_with_remote();
    // Another clone takes ADR-0002 first; this checkout cannot see it.
    let other = dir.path().join("other");
    git(
        dir.path(),
        &[
            "clone",
            "--no-local",
            "-q",
            "-b",
            LINE,
            bare.to_str().unwrap(),
            "other",
        ],
    );
    let first = ok(
        &codeflow(&other, &["adr", "new", "Keep one registry"]),
        "adr new in another clone",
    );
    assert!(first.contains("ADR-0002"), "{first}");
    let adr = ok(
        &codeflow(&root, &["adr", "new", "Adopt a cache: keep it small"]),
        "adr new",
    );
    assert!(
        adr.contains("ADR-0003") && adr.contains("reserved on the authority"),
        "{adr}"
    );
    let path = root.join(adr.split_whitespace().nth(1).unwrap());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\nstatus: proposed "), "{text}");
    let uid_line = text.lines().find(|line| line.starts_with("uid: ")).unwrap();
    let uid = &uid_line[5..];
    let bound = git(&bare, &["show", "codeflow/registry:ids/ADR/0003.toml"]);
    assert!(
        bound.contains(&format!("uid = \"{uid}\"")),
        "{bound}\n{text}"
    );
    ok(
        &codeflow(&root, &["validate", "--docs"]),
        "validate --docs after adr new",
    );
}

/// A follow-up is a task: where durable work is tracked its number comes
/// from the registry, bound to its `uid`, and an interrupted follow-up
/// resumes as a follow-up of the same source.
#[test]
fn a_follow_up_task_takes_its_number_from_the_registry() {
    let (_dir, root, bare) = project_with_remote();
    ok(
        &codeflow(&root, &["epic", "new", "follow-up outcome"]),
        "epic new",
    );
    ok(
        &codeflow(
            &root,
            &["task", "new", "--epic", "EPC-001", "--into", LINE, "source"],
        ),
        "task new",
    );
    commit(&root, "chore: plan the source task");
    git(&root, &["switch", "-q", "-c", "plan/follow-up"]);
    let follow = ok(
        &codeflow(
            &root,
            &["task", "new", "--follow-up-of", "TSK-001", "a follow-up"],
        ),
        "task new --follow-up-of",
    );
    assert!(
        follow.contains("TSK-002") && follow.contains("reserved on the authority"),
        "{follow}"
    );
    let record = std::fs::read_to_string(root.join("project-management/tasks/TSK-002.md")).unwrap();
    assert!(record.contains("follow_up_of: TSK-001"), "{record}");
    let uid_line = record
        .lines()
        .find(|line| line.starts_with("uid: "))
        .unwrap();
    let uid = uid_line[5..].split_whitespace().next().unwrap();
    let bound = git(&bare, &["show", "codeflow/registry:ids/TSK/002.toml"]);
    assert!(bound.contains(&format!("uid = \"{uid}\"")), "{bound}");

    // Reserve a follow-up as `task new` does, stop before the write, resume.
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    std::env::set_var("PATH", path);
    std::env::set_var("CODEFLOW_HOME", isolated_home());
    std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
    let mut request = codeflow_core::ids::issue::Request::issue(
        codeflow_core::ids::Kind::Tsk,
        "interrupted follow-up",
        LINE,
    );
    request.resume = Some(serde_json::json!({
        "kind": "task", "follow_up_of": "TSK-001", "title": "interrupted follow-up",
    }));
    git(&root, &["config", "user.email", "journey@example.test"]);
    git(&root, &["config", "user.name", "Journey"]);
    let reserved = codeflow_core::ids::issue::reserve(&root, &request).unwrap();
    assert_eq!(reserved.id.to_string(), "TSK-003");
    let resumed = ok(
        &codeflow(&root, &["task", "new", "--resume", "TSK-003"]),
        "resume a follow-up",
    );
    assert!(resumed.contains("TSK-003"), "{resumed}");
    let written =
        std::fs::read_to_string(root.join("project-management/tasks/TSK-003.md")).unwrap();
    assert!(written.contains("follow_up_of: TSK-001"), "{written}");
    assert!(written.contains("epic_id: EPC-001"), "{written}");
    assert!(
        written.contains(&format!("uid: {}", reserved.uid)),
        "{written}"
    );
}

/// The enforcing registry check is a step of the policy workflow (TSK-184:
/// one target build serves both). It must run on `pull_request_target`
/// (its workflow from the default branch, never the PR), check out the PR's
/// target, read the PR head only as git data, and hold a read-only token:
/// then a PR that edits this file changes nothing about the verdict it
/// receives. GitHub's own dispatch is not exercised here.
#[test]
fn the_enforcing_registry_workflow_cannot_be_changed_by_the_pull_request_it_judges() {
    let asset =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/ci/codeflow-policy.yml");
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(&asset).unwrap()).unwrap();
    let on = &workflow["on"];
    assert!(
        on.get("pull_request_target").is_some(),
        "judged by a workflow the pull request cannot edit"
    );
    assert!(
        on.get("pull_request").is_none(),
        "never from the PR's own workflow file"
    );
    assert!(on.get("schedule").is_some() && on.get("push").is_some());
    assert_eq!(workflow["permissions"]["contents"], "read");
    let job = &workflow["jobs"]["commit-lint"];
    assert!(
        job["steps"]
            .as_sequence()
            .unwrap()
            .iter()
            .any(|step| step.get("name").and_then(serde_yaml::Value::as_str)
                == Some("codeflow ids check")),
        "the registry check is a step of the enforcing job"
    );
    let raw = std::fs::read_to_string(&asset).unwrap();
    assert!(!raw.contains("secrets."), "no secret reaches the job");
    for step in job["steps"].as_sequence().unwrap() {
        // GitHub's default checkout for pull_request_target is the default
        // branch, so a pull request's run checks out its base commit; other
        // events keep their own commit. Nothing selects the head.
        if let Some(with) = step.get("with") {
            if let Some(selected) = with.get("ref") {
                assert_eq!(
                    selected.as_str(),
                    Some(
                        "${{ github.event_name == 'pull_request_target' && github.event.pull_request.base.sha || '' }}"
                    ),
                    "checkout selects the target: {step:?}"
                );
            }
            assert!(with.get("repository").is_none(), "{step:?}");
        }
        // The pinned install runs only the verified release download, the
        // same script in every enforcing job (TSK-107, `ci_pin.rs`).
        if step.get("name").and_then(serde_yaml::Value::as_str)
            == Some("Install codeflow (target-pinned, checksum-verified)")
        {
            continue;
        }
        let run = step
            .get("run")
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or_default();
        for line in run.lines().map(str::trim) {
            // Only codeflow, git data transfer and shell control flow run:
            // nothing from the PR tree is executed.
            let allowed = [
                "codeflow ",
                "git fetch",
                "if ",
                "else",
                "fi",
                "then",
                "echo ",
                "exit ",
                "#",
                "",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix));
            assert!(allowed, "unexpected command in the enforcing job: {line}");
        }
    }
    assert!(raw.contains(
        "ref: ${{ github.event_name == 'pull_request_target' && github.event.pull_request.base.sha || '' }}"
    ));
    assert!(raw.contains("refs/pull/${PR_NUMBER}/head:refs/codeflow/pr-head"));
    assert!(raw.contains("codeflow ids check --base \"$BASE_SHA\" --head refs/codeflow/pr-head"));
}

/// Review round 1, SL-3, through the CLI: a registered record is deleted
/// and a different record takes its number. The old add is still an
/// ancestor, but it belongs to the record the deletion ended, so CI and the
/// range check refuse the reuse.
#[test]
fn ci_refuses_a_different_record_that_reuses_a_registered_number() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("reuse");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("readme"), "seed\n").unwrap();
    commit(&root, "chore: root");
    let task = root.join("project-management/tasks/TSK-001.md");
    std::fs::create_dir_all(task.parent().unwrap()).unwrap();
    std::fs::write(
        &task,
        "---\nid: TSK-001\ntitle: \"original\"\nstatus: todo\n---\n\n# original\n",
    )
    .unwrap();
    commit(&root, "docs: original");
    ok(&codeflow(&root, &["ids", "seed"]), "ids seed");
    std::fs::remove_file(&task).unwrap();
    commit(&root, "docs: delete");
    let deleted = git(&root, &["rev-parse", "HEAD"]);
    std::fs::write(
        &task,
        "---\nid: TSK-001\ntitle: \"different work\"\nstatus: todo\nepic_id: null\n\
         standalone_reason: \"fixture work\"\nintegration_target: main\nspecs: []\n\
         depends_on: []\nwork_type: feat\ncreated: 2026-09-27\n---\n\n# Different work\n\n\
         ## Description\n\nDifferent work.\n\n## Acceptance Criteria\n\n\
         - AC-1 Shall do different work.\n",
    )
    .unwrap();
    commit(&root, "docs: reuse number");
    let reused = git(&root, &["rev-parse", "HEAD"]);

    let ci = codeflow(
        &root,
        &[
            "ci",
            "--base",
            &deleted,
            "--head",
            &reused,
            "--branch",
            "fix/probe",
        ],
    );
    assert!(!ci.status.success(), "ci passed the reuse:\n{}", text(&ci));
    assert!(
        text(&ci).contains("TSK-001") && text(&ci).contains("provenance does not match"),
        "{}",
        text(&ci)
    );
    let range = codeflow(
        &root,
        &["ids", "check", "--base", &deleted, "--head", &reused],
    );
    assert!(!range.status.success(), "{}", text(&range));
    assert!(
        !text(&range).contains("bound by provenance: TSK-001"),
        "{}",
        text(&range)
    );
}

fn write_task(root: &Path, title: &str) {
    let task = root.join("project-management/tasks/TSK-001.md");
    std::fs::create_dir_all(task.parent().unwrap()).unwrap();
    std::fs::write(
        &task,
        format!("---\nid: TSK-001\ntitle: \"{title}\"\nstatus: todo\n---\n\n# {title}\n"),
    )
    .unwrap();
}

/// TSK-109, through the CLI: a shallow clone neither seeds nor checks until
/// it is unshallowed (R-111), and after the seed `ids check` accepts a
/// rewritten copy of the landed record by its landing, as seed joined it,
/// while a different record under the id warns off a landing line and
/// fails the check on one.
#[test]
fn a_shallow_clone_is_refused_and_ids_check_judges_copies_by_their_landing() {
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("remote.git");
    git(
        dir.path(),
        &["init", "-q", "--bare", "-b", "main", "remote.git"],
    );
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["remote", "add", "origin", bare.to_str().unwrap()]);
    std::fs::write(root.join("readme"), "seed\n").unwrap();
    commit(&root, "chore: root");
    let root_commit = git(&root, &["rev-parse", "HEAD"]);
    write_task(&root, "landed");
    commit(&root, "docs: land the record");
    write_task(&root, "landed and edited");
    commit(&root, "docs: edit the record");
    git(&root, &["push", "-q", "origin", "main"]);

    // A depth-one clone would take the edit for the introduction: refused.
    let url = format!("file://{}", bare.display());
    git(
        dir.path(),
        &["clone", "--no-local", "-q", "--depth", "1", &url, "shallow"],
    );
    let shallow = dir.path().join("shallow");
    let seed = codeflow(&shallow, &["ids", "seed"]);
    assert!(!seed.status.success(), "{}", text(&seed));
    assert!(
        text(&seed).contains("git fetch --unshallow"),
        "{}",
        text(&seed)
    );
    assert!(
        !run_git(
            &bare,
            &[
                "rev-parse",
                "--verify",
                "-q",
                "refs/heads/codeflow/registry"
            ]
        )
        .status
        .success(),
        "a shallow seed wrote nothing"
    );
    let check = codeflow(&shallow, &["ids", "check"]);
    assert!(!check.status.success(), "{}", text(&check));
    assert!(
        text(&check).contains("git fetch --unshallow"),
        "{}",
        text(&check)
    );

    // The full clone seeds; a rewritten copy of the landed file on another
    // commit is the same record, judged by its landing.
    ok(&codeflow(&root, &["ids", "seed"]), "ids seed");
    git(
        &root,
        &["switch", "-q", "-c", "feat/rewritten", &root_commit],
    );
    write_task(&root, "landed");
    commit(&root, "docs: the record, rewritten");
    git(&root, &["switch", "-q", "main"]);
    let check = ok(&codeflow(&root, &["ids", "check"]), "ids check");
    assert!(!check.contains("collision"), "{check}");

    // A different record under the id warns off a landing line ...
    git(&root, &["switch", "-q", "-c", "feat/other", &root_commit]);
    write_task(&root, "other work");
    commit(&root, "docs: other work");
    git(&root, &["switch", "-q", "main"]);
    let check = ok(&codeflow(&root, &["ids", "check"]), "ids check");
    assert!(
        check.contains("collision: TSK-001 on feat/other"),
        "{check}"
    );
    assert!(!check.contains("feat/rewritten"), "{check}");
    // ... and fails the check on one.
    git(&root, &["branch", "-q", "integration/other", "feat/other"]);
    let check = codeflow(&root, &["ids", "check"]);
    assert!(!check.status.success(), "{}", text(&check));
    assert!(
        text(&check).contains("collision: TSK-001 on integration/other"),
        "{}",
        text(&check)
    );

    // Unshallowed, the clone checks like any other.
    git(&shallow, &["fetch", "-q", "--unshallow"]);
    git(
        &shallow,
        &[
            "fetch",
            "-q",
            "origin",
            "refs/heads/codeflow/registry:refs/remotes/origin/codeflow/registry",
        ],
    );
    ok(
        &codeflow(&shallow, &["ids", "check"]),
        "ids check after unshallow",
    );
}

/// TSK-141 AC-5: in a full-tier project, `task new` issues through the
/// registry push while an older `codeflow` that refuses it is first on
/// PATH, since the hook runs the calling binary (SPC-013 R-85).
#[cfg(unix)]
#[test]
fn task_new_issues_with_an_older_codeflow_first_on_path() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, root, bare) = project_with_remote();
    ok(&codeflow(&root, &["epic", "new", "dispatch"]), "epic new");
    let older = dir.path().join("older");
    std::fs::create_dir(&older).unwrap();
    let stub = older.join("codeflow");
    std::fs::write(
        &stub,
        "#!/bin/sh\n\
         if [ \"$1 $2\" = \"git-hook capabilities\" ]; then echo 'hooks 1'; exit 0; fi\n\
         echo \"older codeflow refused: $*\" >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(
        [older.clone()]
            .into_iter()
            .chain(exe.parent().map(Path::to_path_buf))
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
    )
    .unwrap();
    let out = with_env(&mut Command::new(&exe))
        .env("PATH", &path)
        .args([
            "task",
            "new",
            "--epic",
            "EPC-001",
            "--into",
            LINE,
            "dispatched",
        ])
        .current_dir(&root)
        .output()
        .unwrap();
    let said = ok(&out, "task new with an older codeflow first on PATH");
    assert!(
        said.contains("TSK-001") && !said.contains("older codeflow"),
        "{said}"
    );
    let listed = git(
        &bare,
        &["ls-tree", "-r", "--name-only", "codeflow/registry"],
    );
    assert!(listed.contains("ids/TSK/001.toml"), "{listed}");
}
