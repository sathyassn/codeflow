//! Journeys for operator feedback items (TSK-241, issue 75) through the CLI
//! Cargo built: `codeflow feedback new | status | list`, the shared
//! registry, `validate --docs`, `status` and `ids check`. Each project is a
//! temporary full-tier checkout with a local bare remote; no network, and the
//! `codeflow` on the user's `PATH` is never used.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn with_env(command: &mut Command) -> &mut Command {
    command
        .env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
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

fn refused(out: &Output, what: &str) -> String {
    assert!(!out.status.success(), "{what} should fail:\n{}", text(out));
    text(out)
}

fn state(tier: &str) -> String {
    format!(
        "schema_version = 1\ntier = \"{tier}\"\nscaffold_version = \"3.1.0\"\nstack = \"generic\"\nareas = []\npolicy_armed = true\ngit_hooks = \"unwired\"\npermission_preset = \"default\"\n"
    )
}

/// A project at `tier` whose `main` is pushed to a bare remote. Returns
/// (tempdir, project, bare remote).
fn project(tier: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("remote.git");
    git(
        dir.path(),
        &["init", "-q", "--bare", "-b", "main", "remote.git"],
    );
    let root = dir.path().join("proj");
    std::fs::create_dir_all(root.join(".codeflow")).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join(".codeflow/project.toml"), state(tier)).unwrap();
    std::fs::write(root.join("README.md"), "fixture\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "fixture"]);
    git(&root, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&root, &["push", "-q", "origin", "main"]);
    (dir, root, bare)
}

fn item_path(root: &Path, id: &str) -> PathBuf {
    root.join("project-management/feedback")
        .join(format!("{id}.md"))
}

/// Paste the operator's words into an item's Verbatim section, as the agent
/// does after `feedback new`.
fn quote(root: &Path, id: &str, words: &str) {
    let path = item_path(root, id);
    let text = std::fs::read_to_string(&path).unwrap();
    let text = text.replacen("## Verbatim\n", &format!("## Verbatim\n\n> {words}\n"), 1);
    std::fs::write(&path, text).unwrap();
}

fn new_item(root: &Path, topic: &str, summary: &str) -> String {
    ok(
        &codeflow(
            root,
            &[
                "feedback", "new", "--topic", topic, "--source", "chat", summary,
            ],
        ),
        "feedback new",
    )
}

/// AC-1, AC-2: ids come from the registry in their own sequence; an
/// unlisted topic is refused with the list and writes nothing; the default
/// topics are written once; an offline reservation publishes with sync.
#[test]
fn feedback_new_issues_ids_through_the_registry_and_writes_the_topics_once() {
    let (_dir, root, bare) = project("full");
    let state_path = root.join(".codeflow/project.toml");
    let before = std::fs::read_to_string(&state_path).unwrap();

    let unlisted = refused(
        &codeflow(
            &root,
            &[
                "feedback", "new", "--topic", "mood", "--source", "chat", "x",
            ],
        ),
        "an unlisted topic",
    );
    assert!(
        unlisted.contains("topic `mood` is not in the project's list")
            && unlisted
                .contains("process, design, architecture, writing, tooling, security, scope"),
        "{unlisted}"
    );
    assert!(!root.join("project-management").exists(), "nothing written");
    assert_eq!(std::fs::read_to_string(&state_path).unwrap(), before);

    let first = new_item(&root, "process", "Track the operator's input in order");
    assert!(
        first.contains("FB-001") && first.contains("reserved on the authority"),
        "{first}"
    );
    assert!(first.contains("wrote the default topics"), "{first}");
    let record = std::fs::read_to_string(item_path(&root, "FB-001")).unwrap();
    for line in [
        "id: FB-001\n",
        "title: \"Track the operator's input in order\"\n",
        "topic: \"process\" ",
        "source: chat ",
        "status: received ",
        "# FB-001: Track the operator's input in order\n",
        "## Verbatim\n",
        "## Closure\n",
    ] {
        assert!(record.contains(line), "{line:?} missing from\n{record}");
    }
    let uid = record
        .lines()
        .find_map(|line| line.strip_prefix("uid: "))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap()
        .to_string();
    let entry = git(&bare, &["show", "codeflow/registry:ids/FB/001.toml"]);
    assert!(entry.contains(&format!("uid = \"{uid}\"")), "{entry}");
    assert!(entry.contains("kind = \"FB\""), "{entry}");

    let written = std::fs::read_to_string(&state_path).unwrap();
    assert!(written.starts_with(&before), "the rest of the file is kept");
    assert!(written.contains("[feedback]\ntopics = ["), "{written}");

    let second = new_item(&root, "design", "Plain titles");
    assert!(second.contains("FB-002"), "{second}");
    assert!(!second.contains("wrote the default topics"), "{second}");
    assert_eq!(std::fs::read_to_string(&state_path).unwrap(), written);

    // Offline: pending, then published by `ids sync`.
    let url = bare.to_str().unwrap().to_string();
    git(
        &root,
        &["remote", "set-url", "origin", "/nonexistent/remote.git"],
    );
    let pending = new_item(&root, "scope", "Offline feedback");
    assert!(
        pending.contains("FB-003") && pending.contains("pending, not unique yet"),
        "{pending}"
    );
    git(&root, &["remote", "set-url", "origin", &url]);
    let synced = ok(&codeflow(&root, &["ids", "sync"]), "ids sync");
    assert!(synced.contains("FB-003"), "{synced}");
    assert!(git(
        &bare,
        &["ls-tree", "-r", "--name-only", "codeflow/registry"]
    )
    .lines()
    .any(|path| path == "ids/FB/003.toml"));
}

/// AC-3: the status verb moves an item only by its table, writes nothing on
/// a refusal, and the result validates.
#[test]
#[allow(clippy::too_many_lines)] // one journey through every row of the table
fn feedback_status_moves_only_by_the_transition_table() {
    let (_dir, root, _bare) = project("full");
    new_item(&root, "process", "Always say API versioning");
    new_item(&root, "process", "A later ruling");
    quote(&root, "FB-001", "always say API versioning");
    quote(&root, "FB-002", "say versioned API instead");
    let path = item_path(&root, "FB-001");
    let before = std::fs::read_to_string(&path).unwrap();

    let off_table = refused(
        &codeflow(
            &root,
            &[
                "feedback",
                "status",
                "FB-001",
                "closed",
                "--evidence",
                "PR 1",
            ],
        ),
        "received to closed",
    );
    assert!(
        off_table.contains("received -> closed is not a feedback transition")
            && off_table.contains("received -> placed: --in"),
        "{off_table}"
    );
    let nowhere = refused(
        &codeflow(
            &root,
            &["feedback", "status", "FB-001", "placed", "--in", "TSK-404"],
        ),
        "an unresolved placement",
    );
    assert!(nowhere.contains("does not resolve"), "{nowhere}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        before,
        "nothing written"
    );

    let placed = ok(
        &codeflow(
            &root,
            &[
                "feedback",
                "status",
                "FB-001",
                "placed",
                "--in",
                "README.md",
            ],
        ),
        "placed",
    );
    assert!(placed.contains("FB-001  received -> placed"), "{placed}");
    ok(
        &codeflow(
            &root,
            &[
                "feedback",
                "status",
                "FB-001",
                "closed",
                "--evidence",
                "README.md line 1",
            ],
        ),
        "closed",
    );
    let back = refused(
        &codeflow(&root, &["feedback", "status", "FB-001", "received"]),
        "closed to received",
    );
    assert!(back.contains("not a feedback transition"), "{back}");

    let unconfirmed = refused(
        &codeflow(
            &root,
            &["feedback", "status", "FB-002", "declined", "--reason", "no"],
        ),
        "a decline without the operator",
    );
    assert!(
        unconfirmed.contains("--confirmed-by operator"),
        "{unconfirmed}"
    );
    let other = refused(
        &codeflow(
            &root,
            &[
                "feedback",
                "status",
                "FB-002",
                "declined",
                "--confirmed-by",
                "agent",
                "--reason",
                "no",
            ],
        ),
        "a decline confirmed by someone else",
    );
    assert!(other.contains("operator"), "{other}");
    ok(
        &codeflow(
            &root,
            &[
                "feedback",
                "status",
                "FB-002",
                "declined",
                "--confirmed-by",
                "operator",
                "--reason",
                "kept as is",
            ],
        ),
        "declined",
    );
    let missing = refused(
        &codeflow(
            &root,
            &[
                "feedback",
                "status",
                "FB-001",
                "superseded",
                "--by",
                "FB-404",
            ],
        ),
        "superseded by a missing item",
    );
    assert!(missing.contains("FB-404 does not exist"), "{missing}");
    ok(
        &codeflow(
            &root,
            &[
                "feedback",
                "status",
                "FB-001",
                "superseded",
                "--by",
                "FB-002",
            ],
        ),
        "superseded",
    );
    let record = std::fs::read_to_string(&path).unwrap();
    assert!(record.contains("status: superseded "), "{record}");
    assert!(record.contains("placed_in: [\"README.md\"] "), "{record}");
    assert!(record.contains("superseded_by: FB-002 "), "{record}");
    assert!(record.contains("- closed by README.md line 1"), "{record}");
    let validated = ok(&codeflow(&root, &["validate", "--docs"]), "validate --docs");
    assert!(validated.contains("doc graph clean"), "{validated}");
}

/// AC-4, AC-6: `validate --docs` fails on a broken item and warns, never
/// fails, on a written index that went stale; an absent index says nothing.
#[test]
fn validate_docs_fails_a_broken_item_and_warns_on_a_stale_index() {
    let (_dir, root, _bare) = project("full");
    new_item(&root, "process", "First");
    quote(&root, "FB-001", "first words");
    let clean = ok(&codeflow(&root, &["validate", "--docs"]), "validate --docs");
    assert!(!clean.contains("INDEX.md"), "{clean}");

    ok(
        &codeflow(&root, &["feedback", "list", "--write"]),
        "list --write",
    );
    let index = std::fs::read_to_string(root.join("project-management/feedback/INDEX.md")).unwrap();
    assert!(index.contains("| [FB-001](FB-001.md) |"), "{index}");
    let fresh = ok(&codeflow(&root, &["validate", "--docs"]), "validate --docs");
    assert!(!fresh.contains("INDEX.md"), "{fresh}");

    new_item(&root, "design", "Second");
    quote(&root, "FB-002", "second words");
    let stale = ok(
        &codeflow(&root, &["validate", "--docs"]),
        "validate --docs with a stale index",
    );
    assert!(
        stale.contains("warning: project-management/feedback/INDEX.md does not match")
            && stale.contains("codeflow feedback list --write"),
        "{stale}"
    );

    // An empty Verbatim fails.
    new_item(&root, "process", "Unquoted");
    let broken = refused(&codeflow(&root, &["validate", "--docs"]), "validate --docs");
    assert!(
        broken.contains("FB-003.md") && broken.contains("the Verbatim section is empty"),
        "{broken}"
    );
}

/// AC-5: `list` groups by topic with open items first and `--json` carries
/// every field; `status` shows the open count only when the directory exists.
#[test]
fn list_groups_open_items_first_and_status_counts_them() {
    let (_dir, root, _bare) = project("full");
    let status = ok(&codeflow(&root, &["status"]), "status");
    assert!(!status.contains("feedback:"), "{status}");

    new_item(&root, "design", "Design one");
    new_item(&root, "process", "Process one");
    new_item(&root, "process", "Process two");
    for id in ["FB-001", "FB-002", "FB-003"] {
        quote(&root, id, "words");
    }
    ok(
        &codeflow(
            &root,
            &[
                "feedback",
                "status",
                "FB-002",
                "declined",
                "--confirmed-by",
                "operator",
                "--reason",
                "no",
            ],
        ),
        "declined",
    );
    let listed = ok(&codeflow(&root, &["feedback", "list"]), "list");
    let order: Vec<&str> = listed
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert_eq!(
        order,
        ["process", "FB-003", "FB-002", "design", "FB-001"],
        "{listed}"
    );
    let open = ok(
        &codeflow(&root, &["feedback", "list", "--open", "--topic", "process"]),
        "list --open",
    );
    assert!(
        open.contains("FB-003") && !open.contains("FB-002") && !open.contains("FB-001"),
        "{open}"
    );

    let json = ok(
        &codeflow(&root, &["feedback", "list", "--json"]),
        "list --json",
    );
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let first = &value[0]["items"][0];
    assert_eq!(value[0]["topic"], "process");
    for field in [
        "path",
        "id",
        "uid",
        "title",
        "topic",
        "also",
        "source",
        "status",
        "placed_in",
        "supersedes",
        "superseded_by",
        "confirmed_by",
        "external_refs",
        "created",
        "verbatim",
        "reading",
        "placement",
        "closure",
    ] {
        assert!(first.get(field).is_some(), "{field} missing from {first}");
    }
    assert_eq!(first["verbatim"], "> words");

    let status = ok(&codeflow(&root, &["status"]), "status");
    assert!(
        status.contains("feedback: 2 open (process 1, design 1)"),
        "{status}"
    );
}

/// AC-7: `ids check`, as the policy workflow runs it on a push and on a pull
/// request, passes on a registry and a branch holding a feedback record.
#[test]
fn ids_check_passes_with_feedback_records_as_the_policy_workflow_runs_it() {
    let (dir, root, bare) = project("full");
    git(&root, &["switch", "-q", "-c", "task/feedback"]);
    new_item(&root, "process", "Checked by CI");
    quote(&root, "FB-001", "words");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "feedback record"]);
    git(&root, &["push", "-q", "origin", "task/feedback"]);

    let checkout = dir.path().join("ci");
    git(
        dir.path(),
        &[
            "clone",
            "-q",
            "-b",
            "task/feedback",
            bare.to_str().unwrap(),
            "ci",
        ],
    );
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
    let push = ok(&codeflow(&checkout, &["ids", "check"]), "ids check");
    assert!(push.contains("FB: highest 1"), "{push}");
    let main_sha = git(&checkout, &["rev-parse", "origin/main"]);
    let pr = ok(
        &codeflow(
            &checkout,
            &["ids", "check", "--base", &main_sha, "--head", "HEAD"],
        ),
        "ids check on the pull request range",
    );
    assert!(pr.contains("bound: FB-001"), "{pr}");
}

/// AC-8: at the standard and minimal tiers `feedback new` refuses with the
/// tier reason and writes nothing, also in a project that already keeps a
/// task record (which turns durable tracking on below the full tier).
#[test]
fn lower_tiers_refuse_feedback_new_and_write_nothing() {
    for (tier, with_task) in [
        ("standard", false),
        ("minimal", false),
        ("standard", true),
        ("minimal", true),
    ] {
        let (_dir, root, bare) = project(tier);
        if with_task {
            let tasks = root.join("project-management/tasks");
            std::fs::create_dir_all(&tasks).unwrap();
            std::fs::write(
                tasks.join("TSK-001.md"),
                "---\nid: TSK-001\ntitle: x\nstatus: todo\n---\n",
            )
            .unwrap();
        }
        let before = std::fs::read_to_string(root.join(".codeflow/project.toml")).unwrap();
        let out = refused(
            &codeflow(
                &root,
                &[
                    "feedback", "new", "--topic", "process", "--source", "chat", "x",
                ],
            ),
            "feedback new below the full tier",
        );
        assert!(
            out.contains("which the full tier installs")
                && out.contains(&format!("at the {tier} tier")),
            "{out}"
        );
        assert!(!root.join("project-management/feedback").exists());
        assert_eq!(
            std::fs::read_to_string(root.join(".codeflow/project.toml")).unwrap(),
            before
        );
        let branches = git(&bare, &["branch", "--list", "codeflow/registry"]);
        assert!(branches.is_empty(), "no registry entry: {branches}");
    }
}

/// Topics and placements that read as other YAML types (`null`, `[ux]`, a
/// file named `true`) are written as strings and read back unchanged.
#[test]
fn topics_and_placements_keep_their_exact_strings() {
    let (_dir, root, _bare) = project("full");
    let state_path = root.join(".codeflow/project.toml");
    let mut state_text = std::fs::read_to_string(&state_path).unwrap();
    state_text.push_str("\n[feedback]\ntopics = [\"null\", \"[ux]\"]\n");
    std::fs::write(&state_path, state_text).unwrap();
    new_item(&root, "null", "first");
    new_item(&root, "[ux]", "second");
    std::fs::write(root.join("true"), "x\n").unwrap();
    ok(
        &codeflow(
            &root,
            &["feedback", "status", "FB-001", "placed", "--in", "true"],
        ),
        "placed in a file named true",
    );
    let out = codeflow(&root, &["feedback", "list", "--json"]);
    ok(&out, "list --json");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value[0]["topic"], "null", "{value}");
    assert_eq!(value[0]["items"][0]["topic"], "null", "{value}");
    assert_eq!(
        value[0]["items"][0]["placed_in"],
        serde_json::json!(["true"]),
        "{value}"
    );
    assert_eq!(value[1]["items"][0]["topic"], "[ux]", "{value}");
    quote(&root, "FB-001", "first words");
    quote(&root, "FB-002", "second words");
    let validated = ok(&codeflow(&root, &["validate", "--docs"]), "validate --docs");
    assert!(validated.contains("doc graph clean"), "{validated}");
}

/// No feedback read or write goes through a symbolic link: a linked index
/// is not written through, a placement reached through a link does not
/// resolve, and a linked feedback directory takes no new item.
#[cfg(unix)]
#[test]
fn feedback_files_are_never_written_or_resolved_through_a_symbolic_link() {
    use std::os::unix::fs::symlink;
    let (dir, root, _bare) = project("full");
    std::fs::write(root.join("AGENTS.md"), "rules\n").unwrap();
    new_item(&root, "process", "first");
    quote(&root, "FB-001", "first words");
    let index = root.join("project-management/feedback/INDEX.md");
    symlink("../../AGENTS.md", &index).unwrap();
    let out = refused(
        &codeflow(&root, &["feedback", "list", "--write"]),
        "list --write through a link",
    );
    assert!(out.contains("symbolic link"), "{out}");
    assert_eq!(
        std::fs::read_to_string(root.join("AGENTS.md")).unwrap(),
        "rules\n"
    );

    symlink("AGENTS.md", root.join("linked.md")).unwrap();
    let out = refused(
        &codeflow(
            &root,
            &[
                "feedback",
                "status",
                "FB-001",
                "placed",
                "--in",
                "linked.md",
            ],
        ),
        "placed through a link",
    );
    assert!(out.contains("does not resolve"), "{out}");

    std::fs::remove_file(&index).unwrap();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let feedback_dir = root.join("project-management/feedback");
    std::fs::rename(&feedback_dir, root.join("moved")).unwrap();
    symlink(&elsewhere, &feedback_dir).unwrap();
    let out = refused(
        &codeflow(
            &root,
            &[
                "feedback", "new", "--topic", "process", "--source", "chat", "x",
            ],
        ),
        "feedback new into a linked directory",
    );
    assert!(out.contains("symbolic link"), "{out}");
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}

/// The index is written only from items that pass the lint: an id that is
/// not its file's name, or a topic that is not listed (here one that spans
/// lines), refuses `--write` and leaves no index behind.
#[test]
fn the_index_is_written_only_from_items_that_pass_the_lint() {
    let (_dir, root, _bare) = project("full");
    new_item(&root, "process", "first");
    quote(&root, "FB-001", "first words");
    let path = item_path(&root, "FB-001");
    let clean = std::fs::read_to_string(&path).unwrap();
    let index = root.join("project-management/feedback/INDEX.md");
    for injected in [
        clean.replacen("id: FB-001", "id: \"x](https://example.invalid)\"", 1),
        clean.replacen(
            "topic: \"process\"",
            "topic: \"process\\n\\n## injected\"",
            1,
        ),
    ] {
        std::fs::write(&path, &injected).unwrap();
        let out = refused(
            &codeflow(&root, &["feedback", "list", "--write"]),
            "list --write over a failing item",
        );
        assert!(out.contains("INDEX.md not written"), "{out}");
        assert!(!index.exists(), "no index after a refusal");
    }
    std::fs::write(&path, &clean).unwrap();
    ok(
        &codeflow(&root, &["feedback", "list", "--write"]),
        "list --write",
    );
    assert!(std::fs::read_to_string(&index)
        .unwrap()
        .contains("| [FB-001](FB-001.md) |"));
}

/// The project template is read through no symbolic link and only up to
/// its size limit; otherwise the shipped template is used, with a warning.
#[cfg(unix)]
#[test]
fn an_unsafe_project_template_falls_back_to_the_shipped_one() {
    use std::os::unix::fs::symlink;
    let (_dir, root, _bare) = project("full");
    let templates = root.join("project-management/templates");
    std::fs::create_dir_all(&templates).unwrap();
    let template = templates.join("feedback.md");
    symlink("/dev/zero", &template).unwrap();
    let out = new_item(&root, "process", "linked template");
    assert!(
        out.contains("symbolic link") && out.contains("the shipped template is used"),
        "{out}"
    );
    std::fs::remove_file(&template).unwrap();
    std::fs::write(&template, "x".repeat(70 * 1024)).unwrap();
    let out = new_item(&root, "process", "large template");
    assert!(out.contains("over the 65536-byte limit"), "{out}");
    let record = std::fs::read_to_string(item_path(&root, "FB-002")).unwrap();
    assert!(record.contains("## Verbatim"), "{record}");
}

/// Feedback writes keep each file's permissions: a private project state
/// file stays private when the first item adds the default topics, and a
/// private item stays private through a status change.
#[cfg(unix)]
#[test]
fn feedback_writes_keep_private_files_private() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let (_dir, root, _bare) = project("full");
    let state_path = root.join(".codeflow/project.toml");
    std::fs::set_permissions(&state_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    new_item(&root, "process", "first");
    assert!(std::fs::read_to_string(&state_path)
        .unwrap()
        .contains("[feedback]"));
    assert_eq!(mode(&state_path), 0o600);
    let item = item_path(&root, "FB-001");
    std::fs::set_permissions(&item, std::fs::Permissions::from_mode(0o600)).unwrap();
    ok(
        &codeflow(
            &root,
            &[
                "feedback",
                "status",
                "FB-001",
                "placed",
                "--in",
                "README.md",
            ],
        ),
        "placed",
    );
    assert!(std::fs::read_to_string(&item)
        .unwrap()
        .contains("status: placed"));
    assert_eq!(mode(&item), 0o600);
}
