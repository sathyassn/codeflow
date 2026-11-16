//! TSK-1421: a project can require extra sections in its task, epic and spec
//! records. Each test scaffolds a standard-tier project with the binary
//! Cargo built, lists sections in `.codeflow/policy.json`, and drives the
//! callers that judge records: `validate --docs`, `work start`, `codeflow ci`
//! and the pre-push hook, which runs `codeflow ci`. The installed `codeflow`
//! on `PATH` is never used.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/outcome.rs"]
mod outcome;
#[path = "support/process_folders.rs"]
mod process_folders;
use process_folders::isolated_home;

const TASK: &str = "project-management/tasks/TSK-001.md";
const EPIC: &str = "project-management/epics/EPC-001.md";
const SPEC: &str = "project-management/specs/SPC-001.md";
const TARGET: &str = "integration/EPC-001-work";

fn with_env(command: &mut Command) -> &mut Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    // The fixture plays a human: a marker inherited from an agent session
    // would apply the agent-only rules to its commits.
    for name in codeflow_core::root_checkout::AGENT_MARKERS {
        command.env_remove(name);
    }
    command
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.test")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.test")
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

fn git(root: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).trim().to_string()
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

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn commit(root: &Path, message: &str) -> String {
    outcome::state_outcomes(root);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

fn edit(root: &Path, relative: &str, from: &str, to: &str) {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{relative} lacks {from:?}");
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

/// Set `git.<key>` in the project's policy file to `names`; an empty list
/// removes the key.
fn set_sections(root: &Path, key: &str, names: &[&str]) {
    let path = root.join(".codeflow/policy.json");
    let mut policy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let git = policy
        .as_object_mut()
        .unwrap()
        .entry("git")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .unwrap();
    if names.is_empty() {
        git.remove(key);
    } else {
        git.insert(key.to_string(), serde_json::json!(names));
    }
    std::fs::write(&path, serde_json::to_string_pretty(&policy).unwrap() + "\n").unwrap();
}

/// A standard-tier project with no remote: the landed policy is `HEAD`'s.
fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--standard"]),
        "init --standard",
    );
    // Work lands on an integration line; planning happens on its own
    // branch, as a project lives it.
    git(&root, &["switch", "-q", "-c", TARGET]);
    git(&root, &["switch", "-q", "-c", "plan/work"]);
    (dir, root)
}

/// An epic, its spec and one task, made with the verbs and committed.
fn plan(root: &Path) -> String {
    ok(&codeflow(root, &["epic", "new", "outcome"]), "epic new");
    ok(
        &codeflow(root, &["spec", "new", "--for", "EPC-001", "contract"]),
        "spec new",
    );
    ok(
        &codeflow(
            root,
            &[
                "task", "new", "--epic", "EPC-001", "--into", TARGET, "first",
            ],
        ),
        "task new",
    );
    let listed = "- AC-1\n";
    edit(
        root,
        EPIC,
        listed,
        "- AC-1 When used, the system shall work.\n",
    );
    edit(
        root,
        TASK,
        listed,
        "- AC-1 When run, the system shall work (serves EPC-001 AC-1)\n",
    );
    commit(root, "chore: plan the work")
}

fn validate(root: &Path) -> Output {
    codeflow(root, &["validate", "--docs"])
}

#[test]
fn new_records_carry_the_listed_sections_and_validate_enforces_them() {
    // AC-4: each verb writes the sections; AC-2: `validate --docs` refuses
    // a record without one, naming it; AC-7's path in a test.
    let (_dir, root) = project();
    set_sections(&root, "task_required_sections", &["Risks"]);
    set_sections(&root, "epic_required_sections", &["Rollout"]);
    set_sections(&root, "spec_required_sections", &["Threats"]);
    commit(&root, "chore: require record sections");
    plan(&root);
    for (file, heading) in [
        (TASK, "\n## Risks\n"),
        (EPIC, "\n## Rollout\n"),
        (SPEC, "\n## Threats\n"),
    ] {
        let written = std::fs::read_to_string(root.join(file)).unwrap();
        assert!(written.contains(heading), "{file}:\n{written}");
    }
    let out = validate(&root);
    assert!(out.status.success(), "{}", text(&out));

    for (file, heading, key) in [
        (TASK, "## Risks\n", "git.task_required_sections"),
        (EPIC, "## Rollout\n", "git.epic_required_sections"),
        (SPEC, "## Threats\n", "git.spec_required_sections"),
    ] {
        edit(&root, file, heading, "## Notes\n");
        let out = validate(&root);
        let shown = text(&out);
        assert_eq!(out.status.code(), Some(1), "{shown}");
        let name = heading.trim_end();
        assert!(
            shown.contains(&format!("missing section \"{name}\"")) && shown.contains(key),
            "{shown}"
        );
        edit(&root, file, "## Notes\n", heading);
        let out = validate(&root);
        assert!(out.status.success(), "{}", text(&out));
    }
}

#[test]
fn journey_declare_allocate_remove_and_restore_a_task_section() {
    // AC-7 (journey): declare one extra task section in a standard-tier
    // project, allocate a task, remove the section, run `validate --docs`
    // (refused), restore it (passes). Each step's output is printed, so
    // `--nocapture` gives the transcript the pull request cites.
    let (_dir, root) = project();
    let step = |what: &str, out: &Output| {
        println!(
            "$ codeflow {what}\nexit {:?}\n{}",
            out.status.code(),
            text(out).trim_end()
        );
    };
    set_sections(&root, "task_required_sections", &["Risks"]);
    commit(&root, "chore: require a risks section");
    println!("policy: git.task_required_sections = [\"Risks\"], committed");
    let out = codeflow(&root, &["epic", "new", "outcome"]);
    step("epic new outcome", &out);
    let out = codeflow(
        &root,
        &[
            "task", "new", "--epic", "EPC-001", "--into", TARGET, "first",
        ],
    );
    step(
        "task new --epic EPC-001 --into integration/EPC-001-work first",
        &out,
    );
    outcome::state_outcomes(&root);
    edit(
        &root,
        EPIC,
        "- AC-1\n",
        "- AC-1 When used, the system shall work.\n",
    );
    edit(
        &root,
        TASK,
        "- AC-1\n",
        "- AC-1 When run, the system shall work (serves EPC-001 AC-1)\n",
    );
    let written = std::fs::read_to_string(root.join(TASK)).unwrap();
    assert!(written.contains("\n## Risks\n"), "{written}");
    println!("TSK-001.md carries `## Risks`: yes");
    edit(&root, TASK, "## Risks\n", "## Notes\n");
    println!("removed `## Risks` from TSK-001.md");
    let out = validate(&root);
    step("validate --docs", &out);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("missing section \"## Risks\""));
    edit(&root, TASK, "## Notes\n", "## Risks\n");
    println!("restored `## Risks` in TSK-001.md");
    let out = validate(&root);
    step("validate --docs", &out);
    assert!(out.status.success(), "{}", text(&out));
}

#[test]
fn codeflow_sections_stay_required_whatever_the_list_says() {
    // AC-3: a list that leaves out Acceptance Criteria, or names it, changes
    // nothing about it.
    let (_dir, root) = project();
    plan(&root);
    edit(&root, TASK, "## Acceptance Criteria\n", "## Criteria\n");
    for names in [&["Risks"][..], &["Acceptance Criteria"][..]] {
        set_sections(&root, "task_required_sections", names);
        let out = validate(&root);
        let shown = text(&out);
        assert_eq!(out.status.code(), Some(1), "{names:?}: {shown}");
        assert!(
            shown.contains("missing required section \"## Acceptance Criteria\""),
            "{names:?}: {shown}"
        );
    }
}

#[test]
fn an_unchanged_record_is_warned_until_it_next_changes() {
    // AC-1's rule for records that predate a newly listed section.
    let (_dir, root) = project();
    plan(&root);
    set_sections(&root, "task_required_sections", &["Risks"]);
    commit(&root, "chore: require a risks section");
    let out = validate(&root);
    let shown = text(&out);
    assert!(out.status.success(), "{shown}");
    assert!(
        shown.contains("warning") && shown.contains("unchanged since"),
        "{shown}"
    );
    edit(&root, TASK, "first", "first, renamed");
    let out = validate(&root);
    let shown = text(&out);
    assert_eq!(out.status.code(), Some(1), "{shown}");
    assert!(shown.contains("missing section \"## Risks\""), "{shown}");
}

#[test]
fn no_listed_section_judges_records_as_before() {
    // AC-6: a project that lists nothing sees no new finding.
    let (_dir, root) = project();
    plan(&root);
    let out = validate(&root);
    let shown = text(&out);
    assert!(out.status.success(), "{shown}");
    assert!(!shown.contains("required for"), "{shown}");
    assert!(!shown.contains("record sections"), "{shown}");
}

#[test]
fn the_new_verbs_name_an_unreadable_policy_and_write_nothing() {
    // A section listed only in the working policy would be left out of the
    // new record, so each verb names the read failure, as `work start` does.
    let (_dir, root) = project();
    plan(&root);
    std::fs::write(root.join(".codeflow/policy.json"), "{ not json\n").unwrap();
    let records = |root: &Path| {
        ["tasks", "epics", "specs"]
            .iter()
            .map(|dir| {
                std::fs::read_dir(root.join("project-management").join(dir))
                    .unwrap()
                    .count()
            })
            .collect::<Vec<_>>()
    };
    let before = records(&root);
    for args in [
        &["epic", "new", "second"][..],
        &["spec", "new", "--for", "EPC-001", "second"][..],
        &[
            "task", "new", "--epic", "EPC-001", "--into", TARGET, "second",
        ][..],
    ] {
        let out = codeflow(&root, args);
        let shown = text(&out);
        assert!(!out.status.success(), "{args:?}: {shown}");
        // The verb itself names the failure before it allocates an id; a
        // refusal only from the registry ref's hook is not this check.
        assert!(
            shown.contains("error: cannot read policy: cannot parse policy"),
            "{args:?}: {shown}"
        );
        assert!(!shown.contains("update-ref"), "{args:?}: {shown}");
        assert_eq!(records(&root), before, "{args:?}: {shown}");
    }
}

/// A project whose `main` lists `Risks` for tasks and holds TSK-001 with it,
/// and a task branch that drops the key from the policy and the section
/// from the record.
fn dropped_on_a_branch() -> (tempfile::TempDir, PathBuf, String) {
    let (dir, root) = project();
    set_sections(&root, "task_required_sections", &["Risks"]);
    commit(&root, "chore: require a risks section");
    let base = plan(&root);
    git(&root, &["switch", "-q", "-c", "task/TSK-001-first"]);
    set_sections(&root, "task_required_sections", &[]);
    edit(&root, TASK, "## Risks\n", "## Notes\n");
    std::fs::write(root.join("work.txt"), "work\n").unwrap();
    commit(&root, "feat: do the work");
    (dir, root, base)
}

#[test]
fn ci_judges_the_sections_the_base_lists_when_the_head_drops_them() {
    // AC-2 (CI) and AC-5: the head cannot drop a section it is judged by.
    let (_dir, root, base) = dropped_on_a_branch();
    let out = codeflow(
        &root,
        &[
            "ci",
            "--base",
            &base,
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-first",
        ],
    );
    let shown = text(&out);
    assert_eq!(out.status.code(), Some(1), "{shown}");
    assert!(shown.contains("work.valid_graph"), "{shown}");
    assert!(shown.contains("missing section \"## Risks\""), "{shown}");
}

#[test]
fn work_start_refuses_a_changed_record_without_a_listed_section() {
    // AC-2 (`work start`): the landed policy (`main` here, read at `HEAD`
    // in a project without a remote) is joined with the working copy's.
    let (_dir, root) = project();
    plan(&root);
    git(&root, &["switch", "-q", "-c", "task/TSK-001-first"]);
    set_sections(&root, "task_required_sections", &["Risks"]);
    edit(&root, TASK, "first", "first, renamed");
    let out = codeflow(&root, &["work", "start", "TSK-001"]);
    let shown = text(&out);
    assert_eq!(out.status.code(), Some(1), "{shown}");
    assert!(shown.contains("missing section \"## Risks\""), "{shown}");
}

#[cfg(unix)]
#[test]
fn the_pre_push_hook_refuses_through_codeflow_ci() {
    // AC-2 (the hook plane): pre-push runs `codeflow ci` on the pushed
    // range, so the branch that drops the section is refused before it
    // leaves. Pre-commit does not re-validate the graph (TSK-133).
    use std::process::Stdio;
    const ZERO_SHA: &str = "0000000000000000000000000000000000000000";
    let (dir, root, base) = dropped_on_a_branch();
    let destination = dir.path().join("destination.git");
    git(
        &root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            destination.to_str().unwrap(),
        ],
    );
    git(
        &destination,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("{base}:refs/heads/main"),
            &format!("{base}:refs/heads/{TARGET}"),
        ],
    );
    git(
        &root,
        &["remote", "add", "upstream", destination.to_str().unwrap()],
    );
    git(&root, &["fetch", "-q", "upstream"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    let mut child = with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args([
            "git-hook",
            "pre-push",
            "upstream",
            destination.to_str().unwrap(),
        ])
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().unwrap();
        writeln!(
            stdin,
            "refs/heads/task/TSK-001-first {head} refs/heads/task/TSK-001-first {ZERO_SHA}"
        )
        .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    let shown = text(&out);
    assert_eq!(out.status.code(), Some(1), "{shown}");
    assert!(shown.contains("missing section \"## Risks\""), "{shown}");
}
