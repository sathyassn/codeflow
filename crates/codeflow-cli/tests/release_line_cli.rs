//! Release lines judged where each change was introduced (TSK-145, SPC-013
//! R-120). Each test builds a working repository with a bare `origin`
//! holding `main` and two verified epic lines, then judges a release branch
//! through `codeflow ci` and the pre-push hook (the same binary, the same
//! judge) and, for a prospective completion, `task status complete`. The
//! Cargo-built binary runs; the installed `codeflow` is never used.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn clean_env(command: &mut Command) -> &mut Command {
    command
        .env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("CI_MERGE_REQUEST_TARGET_BRANCH_NAME")
        .env_remove("BITBUCKET_PR_DESTINATION_BRANCH")
        .env_remove("CI_PIPELINE_SOURCE")
        .env_remove("CI_MERGE_REQUEST_IID")
        .env_remove("BITBUCKET_PR_ID")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
}

fn output(out: &std::process::Output) -> (i32, String) {
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

const LINE_A: &str = "integration/EPC-001-one";
const LINE_B: &str = "integration/EPC-002-two";
const RELEASE: &str = "integration/release-1";
const HOLDER: &str = "TSK-009";

fn epic(id: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: \"outcome {id}\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-09-26\n---\n\n# {id}: outcome\n\n## Summary\n\nAn outcome.\n\n## Acceptance Criteria\n\n- AC-1 When used, the system shall work.\n"
    )
}

const CRITERIA: &str = "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";
const LOOSER: &str = "- AC-1 When run, the system shall work sometimes.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";
const STRONGER: &str = "- AC-1 When run, the system shall work on every platform.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";

/// A task record of `epic` targeting `target`, with `criteria` and a
/// Closeout; `extra` adds frontmatter lines.
fn task(
    id: &str,
    epic: Option<&str>,
    target: &str,
    status: &str,
    criteria: &str,
    closeout: &str,
    extra: &str,
) -> String {
    let owner = epic.map_or_else(
        || "epic_id: null\nstandalone_reason: \"owns release integration\"".to_string(),
        |epic| format!("epic_id: {epic}\nstandalone_reason: null"),
    );
    format!(
        "---\nid: {id}\n{owner}\nintegration_target: {target}\ntitle: \"work {id}\"\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: []\n{extra}created: 2026-09-26\n---\n\n# {id}: work\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n{criteria}\n## Closeout\n\n{closeout}"
    )
}

/// A valid block for the two criteria, reviewed at `reviewed`.
fn block(reviewed: &str) -> String {
    format!(
        "```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/pr/1#review\n  criteria:\n    AC-1: verified | cargo test | 3 passed\n    AC-2: verified | journey ran\n  journey: verified | tests/journey.rs\n  not_verified: none\n  follow_ups: none: nothing deferred\n  verdict: approved\n```\n"
    )
}

fn path(id: &str) -> String {
    format!("project-management/tasks/{id}.md")
}

/// The line each fixture task belongs to.
fn home(id: &str) -> (Option<&'static str>, &'static str) {
    match id {
        "TSK-001" | "TSK-003" => (Some("EPC-001"), LINE_A),
        "TSK-002" | "TSK-004" => (Some("EPC-002"), LINE_B),
        _ => (None, "main"),
    }
}

/// The record of `id` as the fixture writes it.
fn record(id: &str, status: &str, criteria: &str, closeout: &str) -> String {
    let (epic, target) = home(id);
    let extra = if id == HOLDER {
        "role: release-integration\n"
    } else {
        ""
    };
    task(id, epic, target, status, criteria, closeout, extra)
}

struct Fx {
    _dir: tempfile::TempDir,
    root: PathBuf,
    origin: PathBuf,
}

impl Fx {
    /// `main` holds two epics, tasks TSK-001..004 on their lines and, when
    /// `holder`, the release-integration task; both lines are cut from main
    /// and every branch is pushed to a bare `origin` whose HEAD is main.
    fn new(holder: bool) -> Self {
        Self::with_policy(holder, "")
    }

    fn with_policy(holder: bool, extra_policy: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work");
        let origin = dir.path().join("origin.git");
        std::fs::create_dir_all(&root).unwrap();
        let fx = Self {
            _dir: dir,
            root,
            origin,
        };
        run_git(
            fx.root.parent().unwrap(),
            &["init", "-q", "--bare", "-b", "main", "origin.git"],
        );
        fx.git(&["init", "-q", "-b", "main"]);
        fx.git(&["remote", "add", "origin", fx.origin.to_str().unwrap()]);
        for id in ["EPC-001", "EPC-002"] {
            fx.write(&format!("project-management/epics/{id}.md"), &epic(id));
        }
        let mut ids = vec!["TSK-001", "TSK-002", "TSK-003", "TSK-004"];
        if holder {
            ids.push(HOLDER);
        }
        for id in ids {
            fx.write(&path(id), &record(id, "todo", CRITERIA, "Pending.\n"));
        }
        fx.write(
            ".codeflow/policy.json",
            &format!(
                "{{\n  \"schema_version\": 1,\n  \"git\": {{\"product_paths\": [\"src/**\"]{extra_policy}}}\n}}\n"
            ),
        );
        fx.write("src/lib.rs", "pub fn base() {}\n");
        fx.commit("chore: plan the work");
        for line in [LINE_A, LINE_B] {
            fx.git(&["branch", line, "main"]);
        }
        fx.git(&["push", "-q", "origin", "main", LINE_A, LINE_B]);
        fx
    }

    fn git(&self, args: &[&str]) -> String {
        run_git(&self.root, args)
    }

    fn write(&self, relative: &str, content: &str) {
        let full = self.root.join(relative);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    /// A merge of `branch` into the checked-out branch; returns it.
    fn merge(&self, branch: &str) -> String {
        self.git(&[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("merge: {branch}"),
            branch,
        ]);
        self.head()
    }

    /// Land `branch` on `line` by a merge and publish the line.
    fn land(&self, line: &str, branch: &str) -> String {
        self.git(&["switch", "-q", line]);
        let merge = self.merge(branch);
        self.git(&["push", "-q", "origin", line]);
        merge
    }

    /// On a task branch cut from `line`: build code, then complete `id`
    /// reviewed at that code commit; returns the reviewed commit.
    fn build_and_complete(&self, line: &str, id: &str, file: &str) -> String {
        let branch = format!("task/{id}-work");
        self.git(&["switch", "-q", "-C", &branch, line]);
        self.write(file, &format!("// {id}\n"));
        let reviewed = self.commit("feat: build the work");
        self.write(
            &path(id),
            &record(id, "complete", CRITERIA, &block(&reviewed)),
        );
        self.commit("docs(records): complete the task");
        reviewed
    }

    /// A planning pull request on `line` setting `id`'s criteria; returns
    /// the landing merge.
    fn amend_on_line(&self, line: &str, id: &str, criteria: &str) -> String {
        let branch = format!("plan/amend-{id}");
        self.git(&["switch", "-q", "-C", &branch, line]);
        let current = std::fs::read_to_string(self.root.join(path(id))).unwrap();
        self.write(&path(id), &current.replace(CRITERIA, criteria));
        self.commit("docs(records): amend the criterion");
        self.land(line, &branch)
    }

    /// Cut the release branch from main and publish it there.
    fn cut_release(&self) {
        self.git(&["switch", "-q", "-C", RELEASE, "main"]);
        self.git(&["push", "-q", "origin", RELEASE]);
    }

    /// Import the published tip of `line` into the release branch.
    fn import(&self, line: &str) -> String {
        self.git(&["switch", "-q", RELEASE]);
        self.git(&["fetch", "-q", "origin"]);
        self.git(&[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("merge: import {line}"),
            &format!("origin/{line}"),
        ]);
        self.head()
    }

    /// `codeflow ci` on `base..head` for branch `branch` (into `into`), with
    /// `origin` as the destination.
    fn ci(&self, base: &str, head: &str, branch: &str, into: Option<&str>) -> (i32, String) {
        let mut args = vec![
            "ci",
            "--base",
            base,
            "--head",
            head,
            "--branch",
            branch,
            "--destination",
            self.origin.to_str().unwrap(),
        ];
        if let Some(into) = into {
            args.extend(["--into", into]);
        }
        let out = clean_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
            .args(&args)
            .current_dir(&self.root)
            .output()
            .unwrap();
        output(&out)
    }

    /// The release push as CI judges it: from the published release tip.
    fn ci_release(&self) -> (i32, String) {
        let published = self.git(&["rev-parse", &format!("origin/{RELEASE}")]);
        self.ci(&published, "HEAD", RELEASE, None)
    }

    /// The pre-push hook for pushing `local` to `branch`, which the
    /// destination holds at `remote` (all zeros for a new branch). The
    /// checkout moves off `local` first, so only `codeflow ci` runs.
    fn pre_push(&self, branch: &str, local: &str, remote: &str) -> (i32, String) {
        self.pre_push_to(self.origin.to_str().unwrap(), branch, local, remote)
    }

    /// As [`Fx::pre_push`], pushing to `url` directly.
    fn pre_push_to(&self, url: &str, branch: &str, local: &str, remote: &str) -> (i32, String) {
        let back = self.git(&["rev-parse", "--abbrev-ref", "HEAD"]);
        self.git(&["switch", "-q", "--detach", "main"]);
        let mut child = clean_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
            .args(["git-hook", "pre-push", url, url])
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                format!("refs/heads/{branch} {local} refs/heads/{branch} {remote}\n").as_bytes(),
            )
            .unwrap();
        let out = child.wait_with_output().unwrap();
        self.git(&["switch", "-q", &back]);
        output(&out)
    }

    /// The pre-push hook for the checked-out release head.
    fn pre_push_release(&self) -> (i32, String) {
        let local = self.head();
        let remote = self.git(&["rev-parse", &format!("origin/{RELEASE}")]);
        self.pre_push(RELEASE, &local, &remote)
    }

    /// `task status <id> complete` in the working tree.
    fn status_complete(&self, id: &str) -> (i32, String) {
        let out = clean_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
            .args(["task", "status", id, "complete"])
            .current_dir(&self.root)
            .output()
            .unwrap();
        output(&out)
    }
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let out = clean_env(&mut Command::new("git"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

const SCOPE_LINE: &str = "release range ('";
const NO_OWNER: &str = "no release-integration task to own it";

fn passes(result: &(i32, String), what: &str) {
    assert_eq!(result.0, 0, "{what}:\n{}", result.1);
}

fn blocks(result: &(i32, String), what: &str, needles: &[&str]) {
    assert_eq!(result.0, 1, "{what}:\n{}", result.1);
    for needle in needles {
        assert!(
            result.1.contains(needle),
            "{what}: missing {needle:?} in\n{}",
            result.1
        );
    }
}

/// The acceptance findings of one run: each rule with its message line.
fn findings(result: &(i32, String)) -> Vec<String> {
    let lines: Vec<&str> = result.1.lines().collect();
    let mut found: Vec<String> = lines
        .windows(2)
        .filter(|pair| {
            pair[0].contains("work.acceptance_binding") || pair[0].contains("work.criteria_frozen")
        })
        .map(|pair| {
            let rule = if pair[0].contains("work.criteria_frozen") {
                "work.criteria_frozen"
            } else {
                "work.acceptance_binding"
            };
            format!("{rule}: {}", pair[1].trim())
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Pre-push and CI on the same committed range return the same findings.
fn agree(fx: &Fx, what: &str) -> (i32, String) {
    let ci = fx.ci_release();
    let hook = fx.pre_push_release();
    assert_eq!(
        findings(&ci),
        findings(&hook),
        "{what}: CI and pre-push disagree\nCI:\n{}\npre-push:\n{}",
        ci.1,
        hook.1
    );
    assert_eq!(ci.0 == 0, hook.0 == 0, "{what}: exit codes differ");
    assert!(
        ci.1.contains(SCOPE_LINE),
        "{what}: not judged as a release range:\n{}",
        ci.1
    );
    ci
}

// ---------------------------------------------------------------------------
// AC-1: brought completions and amendments
// ---------------------------------------------------------------------------

/// AC-1, AC-6's shape: two lines, each landing a task completed in its own
/// pull request, and a planning amendment on line A, are merged into a
/// release branch; the completions bind where they were introduced and the
/// amendment is brought. CI and pre-push agree and pass.
#[test]
fn two_lines_merged_into_a_release_bring_completions_and_an_amendment() {
    let fx = Fx::new(false);
    fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.amend_on_line(LINE_A, "TSK-003", STRONGER);
    fx.build_and_complete(LINE_B, "TSK-002", "src/two.rs");
    fx.land(LINE_B, "task/TSK-002-work");
    fx.cut_release();
    fx.import(LINE_A);
    fx.import(LINE_B);
    let result = agree(&fx, "two lines");
    passes(&result, "two lines into a release");

    // The same imports as a pull request into main are judged under R-120.
    let into_main = fx.ci("main", "HEAD", RELEASE, Some("main"));
    passes(&into_main, "the release pull request into main");
    assert!(into_main.1.contains(SCOPE_LINE), "{}", into_main.1);
}

/// AC-1 negative twin: a criteria loosening reachable from line A's ref but
/// landed there by a merge that also changes code is refused as frozen when
/// it reaches the release line.
#[test]
fn a_criteria_change_landed_with_code_on_its_line_is_frozen() {
    let fx = Fx::new(false);
    // Bring TSK-003's current criteria into the release first.
    fx.cut_release();
    fx.git(&["switch", "-q", "-C", "task/TSK-001-mixed", LINE_A]);
    fx.write("src/mixed.rs", "// mixed\n");
    let current = std::fs::read_to_string(fx.root.join(path("TSK-003"))).unwrap();
    fx.write(&path("TSK-003"), &current.replace(CRITERIA, LOOSER));
    fx.commit("feat: code and a looser criterion");
    let landing = fx.land(LINE_A, "task/TSK-001-mixed");
    fx.import(LINE_A);
    let result = agree(&fx, "criteria landed with code");
    blocks(
        &result,
        "criteria landed with code",
        &[
            "work.criteria_frozen",
            &format!(
                "TSK-003 changes its criteria on its line at {}",
                &landing[..9]
            ),
            "which also changes src/mixed.rs",
        ],
    );
}

/// A project config for the baseline fixtures to extend.
const PROJECT: &str = "schema_version = 1\ntier = \"full\"\nscaffold_version = \"3.0.0\"\nstack = \"rust\"\nareas = []\npolicy_armed = true\ngit_hooks = \"wired\"\npermission_preset = \"default\"\n";

/// Land a looser TSK-003 criterion with code on line A; returns the landing.
fn land_mixed(fx: &Fx, file: &str) -> String {
    fx.git(&["switch", "-q", "-C", "task/TSK-001-mixed", LINE_A]);
    fx.write(file, "// mixed\n");
    let current = std::fs::read_to_string(fx.root.join(path("TSK-003"))).unwrap();
    fx.write(&path("TSK-003"), &current.replace(CRITERIA, LOOSER));
    fx.commit("feat: code and a looser criterion");
    fx.land(LINE_A, "task/TSK-001-mixed")
}

/// Record `entries` (`line = "commit"` lines) as the release-rule cutoffs
/// on main and publish main; returns main's new tip.
fn record_cutoffs(fx: &Fx, entries: &str) -> String {
    fx.git(&["switch", "-q", "main"]);
    fx.write(
        ".codeflow/project.toml",
        &format!("{PROJECT}\n[release_rule_baseline]\n{entries}"),
    );
    let tip = fx.commit("chore: record the release-rule cutoffs");
    fx.git(&["push", "-q", "origin", "main"]);
    tip
}

fn frozen_at(landing: &str) -> String {
    format!(
        "TSK-003 changes its criteria on its line at {}",
        &landing[..9]
    )
}

/// AC-10: a line's release-rule cutoff, read at the default target's tip,
/// turns a brought criteria change landed with code at or before it into
/// information naming the task, line, landing, cutoff and policy commit.
#[test]
fn a_criteria_change_at_or_before_the_line_cutoff_is_information() {
    // At the cutoff.
    let fx = Fx::new(false);
    let landing = land_mixed(&fx, "src/mixed.rs");
    let policy = record_cutoffs(&fx, &format!("\"{LINE_A}\" = \"{landing}\"\n"));
    fx.cut_release();
    fx.import(LINE_A);
    let result = agree(&fx, "at the cutoff");
    passes(&result, "a landing at the cutoff");
    let note = format!(
        "legacy criteria change, landed before the release rule: TSK-003 on {LINE_A}, landing {}, cutoff {}, policy main at {}",
        &landing[..9],
        &landing[..9],
        &policy[..9]
    );
    assert!(result.1.contains(&note), "{}", result.1);
    let hook = fx.pre_push_release();
    assert!(hook.1.contains(&note), "the hook shows it:\n{}", hook.1);

    // Before the cutoff: a later planning landing on the line is the cutoff.
    let fx = Fx::new(false);
    let landing = land_mixed(&fx, "src/mixed.rs");
    let cutoff = fx.amend_on_line(LINE_A, "TSK-004", STRONGER);
    record_cutoffs(&fx, &format!("\"{LINE_A}\" = \"{cutoff}\"\n"));
    fx.cut_release();
    fx.import(LINE_A);
    let result = agree(&fx, "before the cutoff");
    passes(&result, "a landing before the cutoff");
    assert!(
        result.1.contains(&format!(
            "landing {}, cutoff {}",
            &landing[..9],
            &cutoff[..9]
        )),
        "{}",
        result.1
    );
}

/// AC-10 negative twins: after the cutoff, a topic made before the cutoff
/// but landed after it, a cutoff recorded for another line, and a cutoff
/// from another line's chain.
#[test]
fn a_criteria_change_the_cutoff_does_not_cover_is_refused() {
    // After the cutoff.
    let fx = Fx::new(false);
    let before = fx.git(&["rev-parse", LINE_A]);
    let landing = land_mixed(&fx, "src/mixed.rs");
    record_cutoffs(&fx, &format!("\"{LINE_A}\" = \"{before}\"\n"));
    fx.cut_release();
    fx.import(LINE_A);
    blocks(
        &agree(&fx, "after the cutoff"),
        "a landing after the cutoff",
        &[
            "work.criteria_frozen",
            &frozen_at(&landing),
            "release-rule cutoff",
        ],
    );

    // A backdated topic: committed before the cutoff, landed after it.
    let fx = Fx::new(false);
    fx.git(&["switch", "-q", "-C", "task/TSK-001-early", LINE_A]);
    fx.write("src/early.rs", "// early\n");
    let current = std::fs::read_to_string(fx.root.join(path("TSK-003"))).unwrap();
    fx.write(&path("TSK-003"), &current.replace(CRITERIA, LOOSER));
    fx.git(&["add", "-A"]);
    fx.git(&[
        "-c",
        "user.name=early",
        "commit",
        "-q",
        "--date=2001-01-01T00:00:00",
        "-m",
        "feat: an early topic",
    ]);
    let cutoff = fx.amend_on_line(LINE_A, "TSK-004", STRONGER);
    let landing = fx.land(LINE_A, "task/TSK-001-early");
    record_cutoffs(&fx, &format!("\"{LINE_A}\" = \"{cutoff}\"\n"));
    fx.cut_release();
    fx.import(LINE_A);
    blocks(
        &agree(&fx, "backdated"),
        "a topic landed after the cutoff",
        &["work.criteria_frozen", &frozen_at(&landing)],
    );

    // The wrong line: the cutoff is recorded for line B.
    let fx = Fx::new(false);
    let landing = land_mixed(&fx, "src/mixed.rs");
    record_cutoffs(&fx, &format!("\"{LINE_B}\" = \"{landing}\"\n"));
    fx.cut_release();
    fx.import(LINE_A);
    blocks(
        &agree(&fx, "wrong line"),
        "a cutoff recorded for another line",
        &["work.criteria_frozen", &frozen_at(&landing)],
    );

    // A cutoff from another line's chain that holds the landing (line B
    // synced line A): ancestry alone never covers it.
    let fx = Fx::new(false);
    let landing = land_mixed(&fx, "src/mixed.rs");
    fx.git(&["switch", "-q", LINE_B]);
    let sync = fx.merge(LINE_A);
    fx.git(&["push", "-q", "origin", LINE_B]);
    record_cutoffs(&fx, &format!("\"{LINE_A}\" = \"{sync}\"\n"));
    fx.cut_release();
    fx.import(LINE_A);
    blocks(
        &agree(&fx, "foreign cutoff"),
        "a cutoff from another line's chain",
        &["work.criteria_frozen", &frozen_at(&landing)],
    );
}

/// AC-10 (Codex R145-R2-1): the cutoff is the one of the line the task
/// targets. Another verified line whose first-parent chain holds the same
/// landing (a shadow line advertised at it, or a line stacked on it) never
/// lends its cutoff; a task's own line covers a landing it holds through
/// stacking.
#[test]
fn the_cutoff_comes_from_the_line_the_task_targets() {
    // A shadow line, sorted before line A, advertised at the landing with
    // that landing as its cutoff; line A's own cutoff is earlier.
    let fx = Fx::new(false);
    let shadow = "integration/EPC-000-shadow";
    fx.git(&["switch", "-q", "main"]);
    fx.write("project-management/epics/EPC-000.md", &epic("EPC-000"));
    fx.write(
        &path("TSK-005"),
        &task(
            "TSK-005",
            Some("EPC-000"),
            shadow,
            "todo",
            CRITERIA,
            "Pending.\n",
            "",
        ),
    );
    fx.commit("docs(records): plan the shadow line");
    fx.git(&["push", "-q", "origin", "main"]);
    let before = fx.git(&["rev-parse", LINE_A]);
    let landing = land_mixed(&fx, "src/mixed.rs");
    fx.git(&[
        "push",
        "-q",
        "origin",
        &format!("{landing}:refs/heads/{shadow}"),
    ]);
    record_cutoffs(
        &fx,
        &format!("\"{shadow}\" = \"{landing}\"\n\"{LINE_A}\" = \"{before}\"\n"),
    );
    fx.cut_release();
    fx.import(LINE_A);
    let result = agree(&fx, "a shadow line");
    blocks(
        &result,
        "another line's cutoff on a shared chain",
        &["work.criteria_frozen", &frozen_at(&landing)],
    );
    assert!(!result.1.contains("legacy criteria change"), "{}", result.1);
    blocks(
        &fx.ci("main", "HEAD", RELEASE, Some("main")),
        "the final pull request",
        &[&frozen_at(&landing)],
    );

    // Line B stacked on line A after the landing: line B's cutoff covers
    // the landing on its chain, but the task targets line A.
    let fx = Fx::new(false);
    let before = fx.git(&["rev-parse", LINE_A]);
    let landing = land_mixed(&fx, "src/mixed.rs");
    fx.git(&[
        "push",
        "-q",
        "--force",
        "origin",
        &format!("{landing}:refs/heads/{LINE_B}"),
    ]);
    record_cutoffs(
        &fx,
        &format!("\"{LINE_A}\" = \"{before}\"\n\"{LINE_B}\" = \"{landing}\"\n"),
    );
    fx.cut_release();
    fx.import(LINE_B);
    blocks(
        &agree(&fx, "a stacked line"),
        "a stacked line's cutoff",
        &["work.criteria_frozen", &frozen_at(&landing)],
    );

    // The same stacking with line A's own cutoff at the landing: covered by
    // the task's line, whichever line brings it.
    let fx = Fx::new(false);
    let landing = land_mixed(&fx, "src/mixed.rs");
    fx.git(&[
        "push",
        "-q",
        "--force",
        "origin",
        &format!("{landing}:refs/heads/{LINE_B}"),
    ]);
    record_cutoffs(&fx, &format!("\"{LINE_A}\" = \"{landing}\"\n"));
    fx.cut_release();
    fx.import(LINE_B);
    let result = agree(&fx, "stacked, own cutoff");
    passes(&result, "a stacked import covered by the task's own line");
    assert!(
        result
            .1
            .contains(&format!("TSK-003 on {LINE_A}, landing {}", &landing[..9])),
        "{}",
        result.1
    );
}

/// AC-10 negative twins, continued: a line rewritten so the cutoff left its
/// chain, a cutoff written only on the release branch, a resolution
/// changing criteria at a covered import, and a malformed entry.
#[test]
fn a_moved_or_malformed_cutoff_covers_nothing() {
    // A rewrite: line A is rebuilt, so the recorded cutoff left its chain.
    let fx = Fx::new(false);
    let old = land_mixed(&fx, "src/mixed.rs");
    record_cutoffs(&fx, &format!("\"{LINE_A}\" = \"{old}\"\n"));
    fx.git(&["switch", "-q", LINE_A]);
    fx.git(&["reset", "-q", "--hard", "main~1"]);
    fx.git(&["push", "-q", "--force", "origin", LINE_A]);
    let landing = land_mixed(&fx, "src/rewritten.rs");
    fx.cut_release();
    fx.import(LINE_A);
    blocks(
        &agree(&fx, "rewrite"),
        "a rewritten line",
        &["work.criteria_frozen", &frozen_at(&landing)],
    );

    // A cutoff written only on the pushed release branch is ignored.
    let fx = Fx::new(false);
    let landing = land_mixed(&fx, "src/mixed.rs");
    fx.cut_release();
    fx.import(LINE_A);
    fx.write(
        ".codeflow/project.toml",
        &format!("{PROJECT}\n[release_rule_baseline]\n\"{LINE_A}\" = \"{landing}\"\n"),
    );
    fx.commit("chore: move the cutoff on the release branch");
    blocks(
        &agree(&fx, "a spoofed cutoff"),
        "a cutoff on the pushed branch",
        &["work.criteria_frozen", &frozen_at(&landing)],
    );

    // A covered import whose resolution changes the criterion again.
    let fx = Fx::new(false);
    let landing = land_mixed(&fx, "src/mixed.rs");
    record_cutoffs(&fx, &format!("\"{LINE_A}\" = \"{landing}\"\n"));
    fx.cut_release();
    fx.git(&["switch", "-q", RELEASE]);
    fx.git(&["fetch", "-q", "origin"]);
    fx.git(&[
        "merge",
        "-q",
        "--no-ff",
        "--no-commit",
        &format!("origin/{LINE_A}"),
    ]);
    let brought = std::fs::read_to_string(fx.root.join(path("TSK-003"))).unwrap();
    fx.write(
        &path("TSK-003"),
        &brought.replace(LOOSER, &LOOSER.replace("sometimes", "rarely")),
    );
    fx.commit("merge: import line A, resolving TSK-003");
    blocks(
        &agree(&fx, "a direct resolution"),
        "a resolution at a covered import",
        &["TSK-003 changes its criteria directly on the release line (the resolution of merge"],
    );

    // A malformed entry at the default target fails closed.
    let fx = Fx::new(false);
    land_mixed(&fx, "src/mixed.rs");
    record_cutoffs(&fx, &format!("\"{LINE_A}\" = \"abc123\"\n"));
    fx.cut_release();
    fx.import(LINE_A);
    blocks(
        &agree(&fx, "a malformed cutoff"),
        "a malformed cutoff",
        &["release_rule_baseline entry for", "full 40-character"],
    );
}

/// Write main's project config with `marker` (a `release_rules` line, or
/// nothing) and publish main; returns main's new tip.
fn set_marker(fx: &Fx, marker: &str) -> String {
    fx.git(&["switch", "-q", "main"]);
    fx.write(".codeflow/project.toml", &format!("{PROJECT}{marker}"));
    let tip = fx.commit("chore: write the project config");
    fx.git(&["push", "-q", "origin", "main"]);
    tip
}

/// AC-13, first condition: the adoption marker never decides whether
/// R-120 applies. The same criteria change landed with code is refused
/// the same way with no marker and with `release_rules = 1`.
#[test]
fn the_adoption_marker_never_decides_enforcement() {
    for marker in ["", "release_rules = 1\n"] {
        let fx = Fx::new(false);
        let landing = land_mixed(&fx, "src/mixed.rs");
        set_marker(&fx, marker);
        fx.cut_release();
        fx.import(LINE_A);
        blocks(
            &agree(&fx, "marker and enforcement"),
            &format!("marker `{}`", marker.trim()),
            &["work.criteria_frozen", &frozen_at(&landing)],
        );
    }
}

/// AC-13, second condition: once the default target carries
/// `release_rules = 1`, removing it, changing its value, removing and
/// re-adding it, or removing it on the judged range refuses every release
/// check, as a table edit does; so does a first appearance other than 1.
/// A marker kept as written passes.
#[test]
fn the_adoption_marker_is_written_once() {
    let judged = |changes: &[&str], on_release: bool| {
        let fx = Fx::new(false);
        fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
        fx.land(LINE_A, "task/TSK-001-work");
        let mut tips = Vec::new();
        for marker in changes {
            tips.push(set_marker(&fx, marker));
        }
        fx.cut_release();
        fx.import(LINE_A);
        if on_release {
            fx.write(".codeflow/project.toml", PROJECT);
            tips.push(fx.commit("chore: drop the marker on the release line"));
        }
        (agree(&fx, "the adoption marker"), tips)
    };
    let one = "release_rules = 1\n";

    let (kept, _) = judged(&[one], false);
    passes(&kept, "a marker kept as written");

    let (result, tips) = judged(&[one, ""], false);
    let needle = format!(
        "set on main at {} is removed at {} on main",
        &tips[0][..9],
        &tips[1][..9]
    );
    blocks(
        &result,
        "the marker removed",
        &[&needle, "every release check refuses"],
    );

    let (result, tips) = judged(&[one, "release_rules = 2\n"], false);
    let needle = format!(
        "is changed to `release_rules = 2` at {} on main",
        &tips[1][..9]
    );
    blocks(&result, "the marker changed", &[&needle]);

    let (result, tips) = judged(&[one, "", one], false);
    let needle = format!("is removed at {} on main", &tips[1][..9]);
    blocks(&result, "the marker removed and re-added", &[&needle]);

    let (result, tips) = judged(&["release_rules = 0\n"], false);
    let needle = format!("first appears at {} as `release_rules = 0`", &tips[0][..9]);
    blocks(&result, "a first value other than 1", &[&needle]);

    let (result, tips) = judged(&[one], true);
    let needle = format!("is removed at {} in the judged range", &tips[1][..9]);
    blocks(
        &result,
        "the marker removed on the release line",
        &[&needle],
    );
}

/// AC-3 (Codex R145-1): the freeze follows the record's identity, so
/// deleting a task record and re-creating it with looser criteria in a
/// later commit is refused, and so is the deletion alone.
#[test]
fn deleting_and_recreating_a_record_keeps_the_freeze() {
    let fx = Fx::new(false);
    fx.cut_release();
    let original = std::fs::read_to_string(fx.root.join(path("TSK-003"))).unwrap();
    std::fs::remove_file(fx.root.join(path("TSK-003"))).unwrap();
    let deleted = fx.commit("docs(records): drop TSK-003");
    fx.write(&path("TSK-003"), &original.replace(CRITERIA, LOOSER));
    let recreated = fx.commit("docs(records): bring TSK-003 back");
    blocks(
        &agree(&fx, "delete and recreate"),
        "a deleted and re-created record",
        &[
            &format!(
                "TSK-003 changes its criteria directly on the release line ({})",
                &deleted[..9]
            ),
            &format!(
                "TSK-003 changes its criteria directly on the release line ({})",
                &recreated[..9]
            ),
        ],
    );
}

/// AC-5 (Codex R145-2): a new release branch whose range the destination's
/// tips do not resolve is still judged or refused, never pushed unjudged.
#[test]
fn an_unresolved_new_release_branch_is_not_pushed_unjudged() {
    let zero = "0000000000000000000000000000000000000000";
    let fx = Fx::new(false);
    let parent = fx.root.parent().unwrap().to_path_buf();
    // A destination with unrelated history and a default it does not have.
    run_git(&parent, &["init", "-q", "-b", "other", "unrelated"]);
    let unrelated = parent.join("unrelated");
    std::fs::write(unrelated.join("x"), "x\n").unwrap();
    run_git(&unrelated, &["add", "-A"]);
    run_git(&unrelated, &["commit", "-q", "-m", "chore: unrelated"]);
    run_git(
        &parent,
        &["clone", "-q", "--bare", "unrelated", "dangling.git"],
    );
    let dangling = parent.join("dangling.git");
    run_git(&dangling, &["symbolic-ref", "HEAD", "refs/heads/nope"]);
    let head = fx.head();
    let hook = fx.pre_push_to(dangling.to_str().unwrap(), RELEASE, &head, zero);
    blocks(
        &hook,
        "a dangling destination",
        &["cannot be decided", "default branch"],
    );

    // An empty destination: no default target to judge the release against.
    run_git(
        &parent,
        &["init", "-q", "--bare", "-b", "main", "empty.git"],
    );
    let empty = parent.join("empty.git");
    let hook = fx.pre_push_to(empty.to_str().unwrap(), RELEASE, &head, zero);
    blocks(&hook, "an empty destination", &["no default target yet"]);
    // An ordinary branch there keeps the note that CI checks it.
    let hook = fx.pre_push_to(empty.to_str().unwrap(), "feat/any", &head, zero);
    passes(&hook, "an ordinary branch to an empty destination");

    // A destination that does not answer cannot say which names its policy
    // makes release branches (Codex R145-R2-2): every name is refused.
    for name in [RELEASE, "feat/any", "feat/release-probe"] {
        let hook = fx.pre_push_to("/nonexistent/destination.git", name, &head, zero);
        blocks(&hook, "an unreachable destination", &["did not answer"]);
    }
}

/// AC-6, AC-9: a new release branch cut from the default target and
/// importing two lines meets the destination's history at several commits.
/// Its push is judged from the default target's tip, as its pull request
/// is, never from one line's tip, which would leave the default target's
/// own later history inside the range.
#[test]
fn a_new_release_branch_is_pushed_from_the_default_tip() {
    let zero = "0000000000000000000000000000000000000000";
    let fx = Fx::new(false);
    fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.build_and_complete(LINE_B, "TSK-002", "src/two.rs");
    fx.land(LINE_B, "task/TSK-002-work");
    // The default target moves on after both lines were cut.
    fx.git(&["switch", "-q", "main"]);
    fx.write("docs/later.md", "later\n");
    let main = fx.commit("docs: a later page");
    fx.git(&["push", "-q", "origin", "main"]);
    fx.git(&["switch", "-q", "-C", RELEASE, "main"]);
    fx.import(LINE_A);
    fx.import(LINE_B);
    let head = fx.head();
    let hook = fx.pre_push(RELEASE, &head, zero);
    passes(&hook, "a new release branch of two imports");
    let judged = format!("codeflow ci --base {main} --head {head}");
    assert!(hook.1.contains(&judged), "{}", hook.1);
    assert!(
        hook.1
            .contains("is a release branch: `codeflow ci` judges everything it adds to main"),
        "{}",
        hook.1
    );
    let ci = fx.ci("main", &head, RELEASE, Some("main"));
    assert_eq!(findings(&ci), findings(&hook), "{}\n{}", ci.1, hook.1);

    // An ordinary branch with the same shape keeps its boundary base.
    let hook = fx.pre_push("feat/two-lines", &head, zero);
    assert!(!hook.1.contains("is a release branch"), "{}", hook.1);
}

/// AC-5, AC-9 (Codex R145-R3-1): a release branch's push covers everything
/// it adds to the default target, however the destination already holds
/// its commits. A direct criteria weakening published under an ordinary
/// branch is refused when a release branch is pushed at that commit, and
/// when one more planning-only commit follows it.
#[test]
fn a_release_push_judges_all_it_adds_to_the_default_tip() {
    let zero = "0000000000000000000000000000000000000000";
    let fx = Fx::new(false);
    fx.git(&["switch", "-q", "-C", "plan/ordinary", "main"]);
    let text = std::fs::read_to_string(fx.root.join(path("TSK-001"))).unwrap();
    assert!(text.contains(CRITERIA), "{text}");
    fx.write(&path("TSK-001"), &text.replace(CRITERIA, LOOSER));
    fx.commit("docs(records): weaken a criterion");
    fx.git(&["push", "-q", "origin", "plan/ordinary"]);
    let frozen = "TSK-001 changes its criteria directly on the release line";

    // Already advertised under the ordinary name.
    let head = fx.head();
    let hook = fx.pre_push(RELEASE, &head, zero);
    blocks(&hook, "a release push of advertised history", &[frozen]);
    let ci = fx.ci("main", &head, RELEASE, Some("main"));
    assert_eq!(findings(&ci), findings(&hook), "{}\n{}", ci.1, hook.1);

    // One new commit after the advertised history.
    fx.write("project-management/note.md", "release metadata\n");
    let head = fx.commit("docs: a release note");
    let hook = fx.pre_push(RELEASE, &head, zero);
    blocks(
        &hook,
        "a release push one commit past advertised history",
        &[frozen],
    );

    // The same push as an existing release branch the destination holds.
    fx.git(&[
        "push",
        "-q",
        "origin",
        &format!("HEAD~1:refs/heads/{RELEASE}"),
    ]);
    let published = fx.git(&["rev-parse", "HEAD~1"]);
    let hook = fx.pre_push(RELEASE, &head, &published);
    blocks(&hook, "an update of a release branch holding it", &[frozen]);

    // Control: the ordinary branch is judged from what the destination holds.
    let hook = fx.pre_push("plan/ordinary-two", &head, zero);
    assert!(!hook.1.contains(frozen), "{}", hook.1);
}

/// AC-5 (Codex R145-3): an existing default target with no policy file
/// cannot say what its release branches are, so the check fails closed.
/// The pre-push hook in `dir` for a new branch `branch` at `local`,
/// pushed to `url`.
fn pre_push_new(dir: &Path, url: &str, branch: &str, local: &str) -> (i32, String) {
    let mut child = clean_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(["git-hook", "pre-push", url, url])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let zeros = "0".repeat(40);
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("refs/heads/{branch} {local} refs/heads/{branch} {zeros}\n").as_bytes())
        .unwrap();
    output(&child.wait_with_output().unwrap())
}

/// A task branch that merges its own line again after other work landed
/// there pushes cleanly: what the faithful merges bring (another task's
/// criteria amendment and completion) is the line's, not the task's, so the
/// range starts at the newest line commit the destination holds, as its
/// pull request does. The branch was published before the first merge, so
/// the old head, the first merged line tip and the newest one all bound the
/// push. A criteria edit made on the task branch itself is still refused.
/// Both an update of the published branch and a first push are checked.
#[test]
fn a_task_branch_that_merges_its_line_is_judged_on_its_own_work() {
    let fx = Fx::new(false);
    let branch = "task/TSK-001-work";
    fx.git(&["switch", "-q", "-C", branch, LINE_A]);
    fx.write("src/one.rs", "// one\n");
    let published = fx.commit("feat: build the work");
    fx.git(&["push", "-q", "origin", branch]);

    // The line moves; the task merges it, then works on.
    fx.git(&["switch", "-q", "-C", "feat/line-work", LINE_A]);
    fx.write("src/line.rs", "// line\n");
    fx.commit("feat: line work");
    fx.land(LINE_A, "feat/line-work");
    fx.git(&["switch", "-q", branch]);
    fx.merge(LINE_A);
    fx.write("src/two.rs", "// two\n");
    fx.commit("feat: more work");

    // The line gains another task's criteria amendment and completion.
    fx.amend_on_line(LINE_A, "TSK-003", STRONGER);
    fx.build_and_complete(LINE_A, "TSK-003", "src/three.rs");
    fx.land(LINE_A, "task/TSK-003-work");
    fx.git(&["switch", "-q", branch]);
    let merged = fx.merge(LINE_A);
    let zeros = "0".repeat(40);
    passes(
        &fx.pre_push(branch, &merged, &published),
        "an update after merging the line",
    );
    passes(
        &fx.pre_push("task/TSK-001-again", &merged, &zeros),
        "a first push after merging the line",
    );

    let current = std::fs::read_to_string(fx.root.join(path("TSK-001"))).unwrap();
    fx.write(&path("TSK-001"), &current.replace(CRITERIA, LOOSER));
    let edited = fx.commit("docs(records): loosen the criterion");
    blocks(
        &fx.pre_push(branch, &edited, &published),
        "a criteria edit on the task branch",
        &["work.criteria_frozen", "TSK-001"],
    );
}

/// AC-5 (Codex round 4): a push of three branches asks the destination
/// once. The hook reads one advertisement and hands it to each `codeflow
/// ci` it runs, which does not ask again. A `git` shim on `PATH` counts the
/// `ls-remote` calls.
#[cfg(unix)]
#[test]
fn a_push_asks_the_destination_once() {
    use std::fmt::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    let fx = Fx::new(true);
    let shim = fx.root.parent().unwrap().join("shim");
    std::fs::create_dir_all(&shim).unwrap();
    let log = fx.root.parent().unwrap().join("git-calls.log");
    let real = String::from_utf8(
        Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let wrapper = shim.join("git");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec '{}' \"$@\"\n",
            log.display(),
            real.trim()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let head = fx.git(&["rev-parse", "main"]);
    let zeros = "0".repeat(40);
    let mut refs = String::new();
    for i in 0..3 {
        writeln!(
            refs,
            "refs/heads/main {head} refs/heads/feat/count-{i} {zeros}"
        )
        .unwrap();
    }
    let url = fx.origin.to_str().unwrap();
    let path = format!("{}:{}", shim.display(), std::env::var("PATH").unwrap());
    let mut child = clean_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(["git-hook", "pre-push", url, url])
        .env("PATH", path)
        .current_dir(&fx.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(refs.as_bytes())
        .unwrap();
    let result = output(&child.wait_with_output().unwrap());
    passes(&result, "three ordinary branches");
    let calls = std::fs::read_to_string(&log).unwrap();
    let asked: Vec<&str> = calls
        .lines()
        .filter(|line| line.contains("ls-remote"))
        .collect();
    assert_eq!(asked.len(), 1, "{asked:#?}\n{}", result.1);
}

/// AC-5 (Codex R145-R4-1): whether the release rules apply is read at the
/// destination's default tip even when this clone predates it. A clone cut
/// before `main` adopted durable work publishes an unowned code commit as
/// an ordinary branch, then pushes that head as a release branch without
/// fetching: the hook fetches the default tip, reads tracking there and
/// refuses. Where the project state there does not parse, the push is
/// refused too: an unreadable state never counts as off. Where `main`
/// never adopted durable work, the same push is ordinary.
#[test]
fn a_release_push_reads_tracking_at_a_default_tip_it_lacks() {
    for (adopted, readable) in [(true, true), (true, false), (false, true)] {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let url = origin.to_str().unwrap();
        run_git(
            dir.path(),
            &["init", "-q", "--bare", "-b", "main", "origin.git"],
        );
        let up = dir.path().join("up");
        std::fs::create_dir_all(&up).unwrap();
        let write = |root: &Path, relative: &str, content: &str| {
            let full = root.join(relative);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, content).unwrap();
        };
        run_git(&up, &["init", "-q", "-b", "main"]);
        write(
            &up,
            ".codeflow/policy.json",
            "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\"]}\n}\n",
        );
        write(&up, "src/lib.rs", "pub fn base() {}\n");
        run_git(&up, &["add", "-A"]);
        run_git(&up, &["commit", "-q", "-m", "chore: start"]);
        run_git(&up, &["push", "-q", url, "main"]);
        run_git(dir.path(), &["clone", "-q", url, "client"]);
        let client = dir.path().join("client");
        if adopted && !readable {
            write(&up, ".codeflow/project.toml", "tier = \"full\"\ntier = [\n");
            run_git(&up, &["add", "-A"]);
            run_git(&up, &["commit", "-q", "-m", "chore: break the state"]);
            run_git(&up, &["push", "-q", url, "main"]);
        } else if adopted {
            write(&up, "project-management/epics/EPC-001.md", &epic("EPC-001"));
            write(
                &up,
                &path("TSK-001"),
                &record("TSK-001", "todo", CRITERIA, "Pending.\n"),
            );
            run_git(&up, &["add", "-A"]);
            run_git(&up, &["commit", "-q", "-m", "docs: plan the work"]);
            run_git(&up, &["push", "-q", url, "main"]);
        }
        let tip = run_git(&up, &["rev-parse", "HEAD"]);

        run_git(&client, &["switch", "-q", "-c", "feat/unowned"]);
        write(&client, "src/unowned.rs", "// unowned\n");
        run_git(&client, &["add", "-A"]);
        run_git(&client, &["commit", "-q", "-m", "feat: unowned change"]);
        run_git(&client, &["push", "-q", "origin", "feat/unowned"]);
        let head = run_git(&client, &["rev-parse", "HEAD"]);
        assert_eq!(
            run_git_status(&client, &["cat-file", "-e", &format!("{tip}^{{commit}}")]),
            !adopted,
            "the clone must lack the adopted default tip"
        );

        let pushed = pre_push_new(&client, url, "integration/release-unseen", &head);
        if !readable {
            blocks(
                &pushed,
                "an unreadable state at the default tip",
                &["whether the release rules apply to 'integration/release-unseen' cannot be read"],
            );
        } else if adopted {
            blocks(
                &pushed,
                "a release push past an unseen adoption",
                &[NO_OWNER],
            );
        } else {
            passes(
                &pushed,
                "a release push where main never adopted durable work",
            );
        }
    }
}

#[test]
fn a_default_target_without_a_policy_fails_closed() {
    let fx = Fx::new(false);
    fx.git(&["switch", "-q", "main"]);
    std::fs::remove_file(fx.root.join(".codeflow/policy.json")).unwrap();
    fx.commit("chore: drop the policy");
    fx.git(&["push", "-q", "origin", "main"]);
    fx.cut_release();
    fx.import(LINE_A);
    blocks(
        &fx.ci("main", "HEAD", RELEASE, None),
        "no policy at the default target",
        &["cannot be decided", ".codeflow/policy.json is missing"],
    );
}

/// AC-5 (Codex R145-R2-3): a task that targets the release branch, not the
/// release-integration owner, gets the same binding from `task status` on
/// its fix branch as CI gives its pull request into the release branch.
#[test]
fn a_completion_into_the_release_branch_binds_alike_in_status_and_ci() {
    let fx = Fx::new(false);
    fx.cut_release();
    fx.git(&["switch", "-q", "main"]);
    fx.write(
        &path("TSK-006"),
        &task("TSK-006", None, RELEASE, "todo", CRITERIA, "Pending.\n", ""),
    );
    fx.commit("docs(records): plan a release fix");
    fx.git(&["push", "-q", "origin", "main"]);
    fx.git(&["switch", "-q", RELEASE]);
    fx.git(&["merge", "-q", "--no-ff", "-m", "merge: sync main", "main"]);
    fx.git(&["push", "-q", "origin", RELEASE]);
    fx.git(&["switch", "-q", "-C", "side", RELEASE]);
    fx.write("src/side.rs", "// side\n");
    let reviewed = fx.commit("fix: the reviewed side");
    fx.git(&["switch", "-q", "-C", "task/TSK-006-fix", RELEASE]);
    fx.write("src/other.rs", "// other\n");
    fx.commit("fix: another change");
    fx.merge("side");
    let proposed = |status: &str| {
        task(
            "TSK-006",
            None,
            RELEASE,
            status,
            CRITERIA,
            &block(&reviewed),
            "",
        )
    };
    fx.write(&path("TSK-006"), &proposed("todo"));
    let needle = format!("src/other.rs changed after the reviewed commit {reviewed}");
    blocks(&fx.status_complete("TSK-006"), "the verb", &[&needle]);
    fx.write(&path("TSK-006"), &proposed("complete"));
    fx.commit("docs(records): complete TSK-006");
    blocks(
        &fx.ci(RELEASE, "HEAD", "task/TSK-006-fix", Some(RELEASE)),
        "CI into the release branch",
        &[&needle],
    );
}

/// AC-5, AC-7 (Codex R145-4): the release-integration task's completion on
/// a fix branch binds to the head in `task status` as in CI into the
/// release branch; a clean merge of the reviewed side does not carry it.
#[test]
fn the_release_owner_binds_at_the_head_in_task_status() {
    let fx = Fx::new(true);
    fx.cut_release();
    fx.git(&["push", "-q", "origin", RELEASE]);
    fx.git(&["switch", "-q", "-C", "side", RELEASE]);
    fx.write("src/side.rs", "// side\n");
    let reviewed = fx.commit("fix: the reviewed side");
    fx.git(&["switch", "-q", "-C", "task/TSK-009-fix", RELEASE]);
    fx.write("src/other.rs", "// other\n");
    fx.commit("fix: another change");
    fx.merge("side");
    fx.write(
        &path(HOLDER),
        &record(HOLDER, "todo", CRITERIA, &block(&reviewed)),
    );
    let needle = format!("src/other.rs changed after the reviewed commit {reviewed}");
    blocks(&fx.status_complete(HOLDER), "the verb", &[&needle]);
    fx.write(
        &path(HOLDER),
        &record(HOLDER, "complete", CRITERIA, &block(&reviewed)),
    );
    fx.commit("docs(records): complete the release integration");
    blocks(
        &fx.ci(RELEASE, "HEAD", "task/TSK-009-fix", Some(RELEASE)),
        "CI into the release branch",
        &[&needle],
    );
}

/// AC-1: a completion made on its line after the task's landing merge binds
/// when the reviewed commit precedes the merge's second parent by status and
/// Closeout changes only, and is refused when an intervening commit touches
/// anything else (TSK-037's shape: a criterion ticked and a sentence
/// changed after review).
#[test]
fn a_late_completion_binds_through_a_closeout_only_ancestor() {
    for (what, intervening, refused) in [
        ("closeout only", None, false),
        (
            "a description change",
            Some(("Work.\n", "Other work.\n")),
            true,
        ),
        (
            "a criterion change",
            Some(("shall work.", "shall work well.")),
            true,
        ),
    ] {
        let fx = Fx::new(false);
        fx.git(&["switch", "-q", "-C", "task/TSK-001-work", LINE_A]);
        fx.write("src/one.rs", "// one\n");
        let reviewed = fx.commit("feat: build the work");
        let mut later = record(
            "TSK-001",
            "todo",
            CRITERIA,
            "Reviewed; waiting for the gate.\n",
        );
        if let Some((from, to)) = intervening {
            later = later.replace(from, to);
        }
        fx.write(&path("TSK-001"), &later);
        fx.commit("docs(records): note the review");
        let landing = fx.land(LINE_A, "task/TSK-001-work");
        // The late completion on the line: a planning pull request.
        fx.git(&["switch", "-q", "-C", "plan/complete-tsk001", LINE_A]);
        let done = later
            .replace("status: todo", "status: complete")
            .replace("Reviewed; waiting for the gate.\n", &block(&reviewed));
        fx.write(&path("TSK-001"), &done);
        fx.commit("docs(records): complete TSK-001");
        let on_line = fx.ci(LINE_A, "HEAD", "plan/complete-tsk001", None);
        fx.land(LINE_A, "plan/complete-tsk001");
        fx.cut_release();
        fx.import(LINE_A);
        let result = agree(&fx, what);
        if refused {
            let needle = format!("the landing merge {landing} brings");
            blocks(&on_line, what, &["work.acceptance_binding", &needle]);
            blocks(&result, what, &["work.acceptance_binding", &needle]);
        } else {
            passes(&on_line, what);
            passes(&result, what);
        }
    }
}

// ---------------------------------------------------------------------------
// AC-2 and AC-3: direct completions and direct criteria changes
// ---------------------------------------------------------------------------

/// AC-2: a completion committed directly on the release line with code
/// after it is refused; the same completion as a prospective one through
/// `task status complete` gets the same finding. A completion at the head
/// passes both.
#[test]
fn a_direct_completion_binds_to_the_release_head() {
    let fx = Fx::new(true);
    fx.cut_release();
    fx.import(LINE_A);
    fx.write("src/fix.rs", "// fix\n");
    let reviewed = fx.commit("fix: integrate");
    fx.write(
        &path("TSK-002"),
        &record("TSK-002", "todo", CRITERIA, &block(&reviewed)),
    );
    let verb = fx.status_complete("TSK-002");
    passes(&verb, "the verb at the reviewed head");
    let made = fx.commit("docs(records): complete TSK-002 on the release line");
    fx.write("src/later.rs", "// later\n");
    let later = fx.commit("fix: a later change");
    let result = agree(&fx, "a direct completion followed by code");
    let needle = format!("src/later.rs changed after the reviewed commit {reviewed}");
    // The finding names the commit that made the completion, and the path
    // report shows how each commit was judged.
    let named = format!(
        "TSK-002 (completed directly on the release line at {}): {needle}",
        &made[..9]
    );
    let completion = format!(
        "release path: {}: direct commit, planning records only",
        &made[..9]
    );
    let code = format!("release path: {}: direct commit, direct work", &later[..9]);
    blocks(
        &result,
        "a direct completion followed by code",
        &[&named, &completion, &code],
    );

    // The verb, given the same completion with the later code as HEAD,
    // returns the same finding.
    fx.git(&["reset", "-q", "--hard", "HEAD~2"]);
    fx.write("src/later.rs", "// later\n");
    fx.commit("fix: a later change");
    fx.write(
        &path("TSK-002"),
        &record("TSK-002", "todo", CRITERIA, &block(&reviewed)),
    );
    let verb = fx.status_complete("TSK-002");
    blocks(
        &verb,
        "the verb after later code",
        &[&format!("TSK-002: {needle}")],
    );

    // Control: a task already complete gets the status refusal, apart from
    // any acceptance finding.
    fx.git(&["checkout", "-q", "--", "."]);
    fx.write(
        &path("TSK-002"),
        &record("TSK-002", "complete", CRITERIA, &block(&reviewed)),
    );
    fx.commit("docs(records): complete TSK-002");
    let again = fx.status_complete("TSK-002");
    assert_eq!(again.0, 1, "{}", again.1);
    assert!(again.1.contains("already complete"), "{}", again.1);
    assert!(!again.1.contains("work.acceptance_binding"), "{}", again.1);
}

/// How a line repairs a completion it landed without a valid binding.
#[derive(Clone, Copy, PartialEq)]
enum Repair {
    /// Reopened by a planning pull request and completed again at the
    /// reviewed head of a task pull request.
    Valid,
    /// The same, with the new block naming a reviewed commit that code
    /// follows.
    Invalid,
    /// The valid repair, reaching the release only through another line
    /// that synced it.
    ThroughAnotherLine,
}

/// `codeflow` with `args` in the fixture.
fn verb(fx: &Fx, args: &[&str]) -> (i32, String) {
    let out = clean_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(args)
        .current_dir(&fx.root)
        .output()
        .unwrap();
    output(&out)
}

/// The release judged after line A landed TSK-001 completed with code
/// between its reviewed commit and the completion, the release imported
/// that, and line A then repaired it as `repair` says; returns the release
/// pull request's result.
fn after_a_brought_repair(repair: Repair) -> (i32, String) {
    let fx = Fx::new(false);
    fx.git(&["switch", "-q", "-C", "task/TSK-001-work", LINE_A]);
    fx.write("src/one.rs", "// one\n");
    let reviewed = fx.commit("feat: build the work");
    fx.write("src/late.rs", "// late\n");
    fx.commit("feat: a late change");
    fx.write(
        &path("TSK-001"),
        &record("TSK-001", "complete", CRITERIA, &block(&reviewed)),
    );
    fx.commit("docs(records): complete the task");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.cut_release();
    fx.import(LINE_A);
    let first = fx.ci("main", "HEAD", RELEASE, Some("main"));
    blocks(
        &first,
        "the brought completion with a late change",
        &["TSK-001", "src/late.rs changed after the reviewed commit"],
    );

    // Line A reopens the task by a planning pull request, then completes
    // it again at the reviewed head of a task pull request.
    fx.git(&["switch", "-q", "-C", "plan/reopen-TSK-001", LINE_A]);
    let reopened = verb(
        &fx,
        &[
            "task",
            "status",
            "TSK-001",
            "todo",
            "--reason",
            "late change",
        ],
    );
    passes(&reopened, "the reopen verb");
    fx.commit("docs(records): reopen TSK-001");
    fx.land(LINE_A, "plan/reopen-TSK-001");
    fx.git(&["switch", "-q", "-C", "task/TSK-001-again", LINE_A]);
    let before = fx.head();
    fx.write("src/late.rs", "// late, reviewed\n");
    let again = fx.commit("fix: review the late change");
    let yaml = fx.root.parent().unwrap().join("again.yaml");
    std::fs::write(
        &yaml,
        format!(
            "acceptance:\n  reviewed: {again}\n  review: https://example.test/pr/2#review\n  criteria:\n    AC-1: verified | cargo test | 3 passed\n    AC-2: verified | journey ran\n  journey: verified | tests/journey.rs\n  not_verified: none\n  follow_ups: none: nothing deferred\n  verdict: approved\n"
        ),
    )
    .unwrap();
    let completed = verb(
        &fx,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            yaml.to_str().unwrap(),
        ],
    );
    passes(&completed, "the re-completion verb");
    fx.commit("docs(records): complete TSK-001 again");
    if repair == Repair::Invalid {
        let file = fx.root.join(path("TSK-001"));
        let text = std::fs::read_to_string(&file).unwrap();
        let named = format!("reviewed: {again}");
        assert!(text.contains(&named), "{text}");
        std::fs::write(&file, text.replace(&named, &format!("reviewed: {before}"))).unwrap();
        fx.commit("docs(records): name the earlier reviewed commit");
    }
    fx.land(LINE_A, "task/TSK-001-again");
    let line = if repair == Repair::ThroughAnotherLine {
        fx.git(&["switch", "-q", LINE_B]);
        fx.merge(LINE_A);
        fx.git(&["push", "-q", "origin", LINE_B]);
        LINE_B
    } else {
        LINE_A
    };
    fx.import(line);
    fx.ci("main", "HEAD", RELEASE, Some("main"))
}

/// AC-1, AC-11: a later completion brought from the task's own line that
/// binds where it was introduced there supersedes an earlier brought
/// completion; the earlier one is never accepted. An invalid later one, or
/// one reaching the release through another line, leaves the refusal.
#[test]
fn a_valid_re_completion_from_its_line_supersedes_a_brought_one() {
    let old = "src/late.rs changed after the reviewed commit";
    let count = |result: &(i32, String)| {
        findings(result)
            .iter()
            .filter(|found| found.contains("TSK-001") && found.contains(old))
            .count()
    };
    let valid = after_a_brought_repair(Repair::Valid);
    passes(&valid, "a valid re-completion from its own line");
    assert_eq!(count(&valid), 0, "{}", valid.1);

    // The old completion's finding stays beside the new one's.
    let invalid = after_a_brought_repair(Repair::Invalid);
    blocks(
        &invalid,
        "an invalid re-completion from its own line",
        &[old],
    );
    assert_eq!(count(&invalid), 2, "{}", invalid.1);

    let other = after_a_brought_repair(Repair::ThroughAnotherLine);
    blocks(&other, "a re-completion through another line", &[old]);
    assert_eq!(count(&other), 1, "{}", other.1);
}

/// Line A lands, one after another, TSK-001 completed with code between
/// its reviewed commit and the completion (L1), then reopened and
/// completed again at its reviewed head (L2), then completed once more
/// with a block naming a reviewed commit that new code follows (L3). The
/// release imports L1. Returns the fixture and the L2 and L3 tips.
fn three_completions_on_line_a() -> (Fx, String, String) {
    let fx = Fx::new(false);
    fx.git(&["switch", "-q", "-C", "task/TSK-001-work", LINE_A]);
    fx.write("src/one.rs", "// one\n");
    let reviewed = fx.commit("feat: build the work");
    fx.write("src/late.rs", "// late\n");
    fx.commit("feat: a late change");
    fx.write(
        &path("TSK-001"),
        &record("TSK-001", "complete", CRITERIA, &block(&reviewed)),
    );
    fx.commit("docs(records): complete the task");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.cut_release();
    fx.import(LINE_A);

    fx.git(&["switch", "-q", "-C", "plan/reopen-TSK-001", LINE_A]);
    let reopened = verb(
        &fx,
        &[
            "task",
            "status",
            "TSK-001",
            "todo",
            "--reason",
            "late change",
        ],
    );
    passes(&reopened, "the reopen verb");
    fx.commit("docs(records): reopen TSK-001");
    fx.land(LINE_A, "plan/reopen-TSK-001");
    fx.git(&["switch", "-q", "-C", "task/TSK-001-again", LINE_A]);
    fx.write("src/late.rs", "// late, reviewed\n");
    let again = fx.commit("fix: review the late change");
    let yaml = fx.root.parent().unwrap().join("again.yaml");
    std::fs::write(
        &yaml,
        format!(
            "acceptance:\n  reviewed: {again}\n  review: https://example.test/pr/2#review\n  criteria:\n    AC-1: verified | cargo test | 3 passed\n    AC-2: verified | journey ran\n  journey: verified | tests/journey.rs\n  not_verified: none\n  follow_ups: none: nothing deferred\n  verdict: approved\n"
        ),
    )
    .unwrap();
    let completed = verb(
        &fx,
        &[
            "task",
            "status",
            "TSK-001",
            "complete",
            "--acceptance",
            yaml.to_str().unwrap(),
        ],
    );
    passes(&completed, "the re-completion verb");
    fx.commit("docs(records): complete TSK-001 again");
    let l2 = fx.land(LINE_A, "task/TSK-001-again");

    fx.git(&["switch", "-q", "-C", "task/TSK-001-third", LINE_A]);
    fx.write("src/newlate.rs", "// new, unreviewed\n");
    fx.commit("feat: a change after the review");
    let file = fx.root.join(path("TSK-001"));
    let text = std::fs::read_to_string(&file).unwrap();
    let named = format!("reviewed: {again}");
    assert!(text.contains(&named), "{text}");
    std::fs::write(&file, text.replace(&named, &format!("reviewed: {l2}"))).unwrap();
    fx.commit("docs(records): name the line's reviewed commit");
    let l3 = fx.land(LINE_A, "task/TSK-001-third");
    fx.git(&["switch", "-q", RELEASE]);
    fx.git(&["fetch", "-q", "origin"]);
    (fx, l2, l3)
}

/// AC-8, AC-11 (Codex R145-R3-2): an octopus that names the task line's
/// newest tip and, as a later parent, an older tip of the same line cannot
/// make the older completion the expected import or the one in force: the
/// merge restoring it is a resolution, and L3's invalid completion stands.
/// The real git merge of the same tips is the control.
#[test]
fn an_older_octopus_parent_cannot_hide_a_newer_completion() {
    let needle = "src/newlate.rs changed after the reviewed commit";
    let (fx, l2, l3) = three_completions_on_line_a();
    let release = fx.head();
    // The real merge: git drops L2, which L3 contains.
    fx.git(&["merge", "-q", "--no-ff", "-m", "merge: import", &l3, &l2]);
    let control = fx.ci("main", "HEAD", RELEASE, Some("main"));
    blocks(&control, "the real merge of both tips", &[needle]);

    // The synthetic octopus: L3's tree with L2's record, parents in the
    // order release, L3, L2.
    fx.git(&["reset", "-q", "--hard", &release]);
    fx.git(&["merge", "-q", "--no-ff", "--no-commit", &l3]);
    let older = fx.git(&["show", &format!("{l2}:{}", path("TSK-001"))]);
    fx.write(&path("TSK-001"), &format!("{older}\n"));
    fx.git(&["add", "-A"]);
    let tree = fx.git(&["write-tree"]);
    fx.git(&["merge", "--abort"]);
    let octopus = fx.git(&[
        "commit-tree",
        &tree,
        "-p",
        &release,
        "-p",
        &l3,
        "-p",
        &l2,
        "-m",
        "merge: import line A",
    ]);
    fx.git(&["reset", "-q", "--hard", &octopus]);
    let result = fx.ci("main", "HEAD", RELEASE, Some("main"));
    // The earlier completion's finding still stands.
    blocks(
        &result,
        "the octopus restoring L2",
        &["TSK-001", "src/late.rs changed after the reviewed commit"],
    );
    assert!(
        result.1.contains(&format!(
            "release path: {}: import with 1 resolved path(s)",
            &octopus[..9]
        )),
        "{}",
        result.1
    );
    let pushed = fx.pre_push(
        RELEASE,
        &octopus,
        &fx.git(&["rev-parse", &format!("origin/{RELEASE}")]),
    );
    assert_eq!(pushed.0, 1, "{}", pushed.1);
}

/// Merge `side` into the checked-out branch, taking `side`'s entry for a
/// path that conflicts; returns the merge.
fn merge_take(fx: &Fx, side: &str) -> String {
    if !run_git_status(&fx.root, &["merge", "-q", "--no-ff", "--no-commit", side]) {
        for file in fx.git(&["diff", "--name-only", "--diff-filter=U"]).lines() {
            fx.git(&["checkout", "-q", "--theirs", "--", file]);
            fx.git(&["add", "--", file]);
        }
    }
    fx.commit(&format!("merge: take {side}"))
}

/// A merge commit of `tree` with `parents`, checked out on the release
/// branch; asserts git's own merge of the same parents writes that tree,
/// so the merge is a faithful import.
fn faithful_merge(fx: &Fx, tree: &str, parents: &[&str]) -> String {
    fx.git(&["switch", "-q", "--detach", parents[0]]);
    let mut args = vec!["merge", "-q", "--no-ff", "--no-commit"];
    args.extend_from_slice(&parents[1..]);
    fx.git(&args);
    assert_eq!(
        fx.git(&["write-tree"]),
        tree,
        "git merges {parents:?} to another tree"
    );
    fx.git(&["merge", "--abort"]);
    let mut args = vec!["commit-tree", tree];
    for parent in parents {
        args.extend(["-p", parent]);
    }
    args.extend(["-m", "merge: import"]);
    let merge = fx.git(&args);
    fx.git(&["switch", "-q", RELEASE]);
    fx.git(&["reset", "-q", "--hard", &merge]);
    merge
}

/// AC-11 (Codex round 4): both supersession guards are reachable through
/// faithful imports. Line A lands TSK-001 three times: L1 with a late
/// unreviewed change, L2 a valid repair, L3 a stale completion after new
/// code. Line B takes A at L3, then a merge with L2 restores L2's record.
/// - Newest parent: the release imports L3 and B together. L2's record
///   arrives through B, but A's newest parent is L3, whose record differs,
///   so L1's finding stands.
/// - Order: the release first imports L3, then B together with the older
///   L2. L2's completion lands before L3's on A, so it cannot supersede
///   L3's finding.
#[test]
fn both_supersession_guards_hold_through_faithful_imports() {
    let fx = Fx::new(false);
    fx.git(&["switch", "-q", "-C", "task/TSK-001-work", LINE_A]);
    fx.write("src/one.rs", "// one\n");
    let reviewed = fx.commit("feat: build the work");
    fx.write("src/late.rs", "// late\n");
    fx.commit("feat: a late change");
    fx.write(
        &path("TSK-001"),
        &record("TSK-001", "complete", CRITERIA, &block(&reviewed)),
    );
    fx.commit("docs(records): complete the task");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.cut_release();
    let release_l1 = fx.import(LINE_A);

    fx.git(&["switch", "-q", "-C", "task/TSK-001-repair", LINE_A]);
    fx.write(
        &path("TSK-001"),
        &record("TSK-001", "todo", CRITERIA, "Pending.\n"),
    );
    fx.write("src/late.rs", "// late, reviewed\n");
    let repaired = fx.commit("fix: review the late change");
    fx.write(
        &path("TSK-001"),
        &record("TSK-001", "complete", CRITERIA, &block(&repaired)),
    );
    fx.commit("docs(records): complete the repair");
    let l2 = fx.land(LINE_A, "task/TSK-001-repair");

    fx.git(&["switch", "-q", "-C", "task/TSK-001-third", LINE_A]);
    fx.write("src/newlate.rs", "// new, unreviewed\n");
    fx.commit("feat: a change after the review");
    let main = fx.git(&["rev-parse", "main"]);
    fx.write(
        &path("TSK-001"),
        &record("TSK-001", "complete", CRITERIA, &block(&main)),
    );
    fx.commit("docs(records): a stale completion");
    let l3 = fx.land(LINE_A, "task/TSK-001-third");

    fx.git(&["switch", "-q", LINE_B]);
    let synced = merge_take(&fx, LINE_A);
    let older = fx.git(&["show", &format!("{l2}:{}", path("TSK-001"))]);
    fx.write(&path("TSK-001"), &format!("{older}\n"));
    fx.git(&["add", "-A"]);
    let tree = fx.git(&["write-tree"]);
    let foreign = fx.git(&[
        "commit-tree",
        &tree,
        "-p",
        &synced,
        "-p",
        &l2,
        "-m",
        "merge: restore L2's record",
    ]);
    fx.git(&["reset", "-q", "--hard", &foreign]);
    fx.git(&["push", "-q", "origin", LINE_B]);

    // L1's finding names its reviewed commit, L3's names main.
    let l1_finding = format!("src/late.rs changed after the reviewed commit {reviewed}");
    let l3_finding = format!("src/late.rs changed after the reviewed commit {main}");
    for sides in [[&l3, &foreign], [&foreign, &l3]] {
        let merge = faithful_merge(&fx, &tree, &[&release_l1, sides[0], sides[1]]);
        let result = fx.ci("main", "HEAD", RELEASE, Some("main"));
        assert!(
            result
                .1
                .contains(&format!("release path: {}: import", &merge[..9])),
            "{}",
            result.1
        );
        blocks(&result, "L2 through B beside the newer L3", &[&l1_finding]);
    }

    let l3_tree = fx.git(&["rev-parse", &format!("{l3}^{{tree}}")]);
    let newer = faithful_merge(&fx, &l3_tree, &[&release_l1, &l3]);
    for sides in [[&l2, &foreign], [&foreign, &l2]] {
        faithful_merge(&fx, &tree, &[&newer, sides[0], sides[1]]);
        let result = fx.ci("main", "HEAD", RELEASE, Some("main"));
        blocks(&result, "the older L2 after L3", &[&l3_finding]);
    }
}

/// AC-2, AC-11 (Codex R145-R3-3): a completion made directly on the release
/// line stays bound at the head when a valid completion of the task arrives
/// through another line; only a valid completion from the task's own line
/// supersedes it.
#[test]
fn a_foreign_import_never_clears_a_direct_completion() {
    for (through, clears) in [(LINE_B, false), (LINE_A, true)] {
        let fx = Fx::new(true);
        fx.cut_release();
        fx.write("src/fix.rs", "// fix\n");
        let reviewed = fx.commit("fix: integrate");
        fx.write(
            &path("TSK-001"),
            &record("TSK-001", "complete", CRITERIA, &block(&reviewed)),
        );
        let made = fx.commit("docs(records): complete TSK-001 on the release line");
        fx.write("src/later.rs", "// later\n");
        fx.commit("fix: a later integration change");
        let stale = format!(
            "TSK-001 (completed directly on the release line at {})",
            &made[..9]
        );

        // TSK-001 completes validly on its own line; line B syncs it.
        fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
        fx.land(LINE_A, "task/TSK-001-work");
        if through == LINE_B {
            fx.git(&["switch", "-q", LINE_B]);
            fx.merge(LINE_A);
            fx.git(&["push", "-q", "origin", LINE_B]);
        }
        fx.git(&["switch", "-q", RELEASE]);
        fx.git(&["fetch", "-q", "origin"]);
        let line = format!("origin/{through}");
        let conflicted =
            !run_git_status(&fx.root, &["merge", "-q", "--no-ff", "--no-commit", &line]);
        if conflicted {
            fx.git(&["checkout", "-q", "--theirs", "--", &path("TSK-001")]);
            fx.git(&["add", "--", &path("TSK-001")]);
        }
        let import = fx.commit("merge: import the line");
        fx.write(
            &path(HOLDER),
            &record(HOLDER, "todo", CRITERIA, &block(&import)),
        );
        passes(&fx.status_complete(HOLDER), "the holder completes");
        fx.commit("docs(records): complete the release integration");
        let result = fx.ci("main", "HEAD", RELEASE, Some("main"));
        if clears {
            passes(&result, "a valid completion from the task's own line");
            assert!(!result.1.contains(&stale), "{}", result.1);
        } else {
            blocks(
                &result,
                "a valid completion through another line",
                &[&stale],
            );
            let pushed = fx.pre_push_release();
            assert_eq!(findings(&pushed), findings(&result), "{}", pushed.1);
        }
    }
}

/// AC-2, AC-11 (Codex R145-R4-2): a completion made directly on the
/// release line is judged as it was made, not as a later foreign import
/// rewrites it. Line A lands TSK-001's valid completion; the release line
/// replaces its block with a stale one; line B takes that state and lands a
/// completion of TSK-001 reviewed at its own head; the release imports B.
/// The direct completion's finding stands, since only the task's own line
/// supersedes it.
#[test]
fn a_foreign_completion_never_replaces_a_direct_one() {
    let fx = Fx::new(false);
    fx.build_and_complete(LINE_A, "TSK-001", "src/work.rs");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.cut_release();
    fx.import(LINE_A);
    let main = fx.git(&["rev-parse", "main"]);
    fx.write(
        &path("TSK-001"),
        &record("TSK-001", "complete", CRITERIA, &block(&main)),
    );
    let direct = fx.commit("docs(records): replace the completion");
    let stale = format!(
        "TSK-001 (completed directly on the release line at {}): src/work.rs changed after the reviewed commit",
        &direct[..9]
    );
    let before = fx.ci("main", "HEAD", RELEASE, Some("main"));
    blocks(&before, "the stale direct completion", &[&stale]);

    // Line B takes the release state and completes TSK-001 at its head.
    fx.git(&["switch", "-q", LINE_B]);
    let synced = fx.merge(RELEASE);
    fx.git(&["switch", "-q", "-C", "task/TSK-001-foreign", LINE_B]);
    fx.write(
        &path("TSK-001"),
        &record("TSK-001", "complete", CRITERIA, &block(&synced)),
    );
    fx.commit("docs(records): complete TSK-001 through line B");
    fx.land(LINE_B, "task/TSK-001-foreign");
    let import = fx.import(LINE_B);

    let after = fx.ci("main", "HEAD", RELEASE, Some("main"));
    assert!(
        after
            .1
            .contains(&format!("release path: {}: import", &import[..9])),
        "{}",
        after.1
    );
    blocks(
        &after,
        "a foreign completion after the direct one",
        &[&stale],
    );
    let pushed = fx.pre_push_release();
    assert_eq!(findings(&pushed), findings(&after), "{}", pushed.1);
    assert_eq!(pushed.0, 1, "{}", pushed.1);
}

/// AC-1, AC-6: a completion made on the release line and later brought,
/// block and all, from the task's own line is judged where the line landed
/// it; the earlier direct completion no longer stands at the head.
#[test]
fn a_completion_brought_from_its_line_replaces_a_direct_one() {
    let fx = Fx::new(true);
    fx.cut_release();
    fx.write("src/fix.rs", "// fix\n");
    let early = fx.commit("fix: integrate");
    fx.write(
        &path("TSK-001"),
        &record("TSK-001", "complete", CRITERIA, &block(&early)),
    );
    let made = fx.commit("docs(records): complete TSK-001 on the release line");
    fx.write("src/later.rs", "// later\n");
    fx.commit("fix: a later change");
    let stale = format!(
        "TSK-001 (completed directly on the release line at {})",
        &made[..9]
    );

    // The task's own line lands its completion; the release takes the
    // line's record whole.
    fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.git(&["switch", "-q", RELEASE]);
    fx.git(&["fetch", "-q", "origin"]);
    let line = format!("origin/{LINE_A}");
    let clean = run_git_status(&fx.root, &["merge", "-q", "--no-ff", "--no-commit", &line]);
    assert!(
        !clean,
        "both sides completed TSK-001, so the merge conflicts"
    );
    fx.git(&["checkout", "-q", "--theirs", "--", &path("TSK-001")]);
    fx.git(&["add", "--", &path("TSK-001")]);
    let import = fx.commit("merge: import the line");
    fx.write(
        &path(HOLDER),
        &record(HOLDER, "todo", CRITERIA, &block(&import)),
    );
    passes(&fx.status_complete(HOLDER), "the holder completes");
    fx.commit("docs(records): complete the release integration");

    let result = fx.ci("main", "HEAD", RELEASE, Some("main"));
    passes(&result, "the brought completion replaces the direct one");
    assert!(!result.1.contains(&stale), "{}", result.1);
    assert!(
        result
            .1
            .contains(&format!("release path: {}: import", &import[..9])),
        "{}",
        result.1
    );
}

/// `git` in `dir`, returning whether it succeeded (a conflicted merge
/// fails and is resolved by the caller).
fn run_git_status(dir: &Path, args: &[&str]) -> bool {
    clean_env(&mut Command::new("git"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
        .status
        .success()
}

/// AC-2, AC-3: an import merge that adds a completion or a criteria change
/// of its own (a clean merge's evil change) is judged as a resolution.
#[test]
fn an_evil_import_merge_is_a_resolution() {
    for (what, criteria, closeout, needle) in [
        (
            "an evil completion",
            CRITERIA,
            true,
            "work.acceptance_binding",
        ),
        (
            "an evil criteria change",
            LOOSER,
            false,
            "work.criteria_frozen",
        ),
    ] {
        let fx = Fx::new(true);
        fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
        fx.land(LINE_A, "task/TSK-001-work");
        fx.cut_release();
        let old = fx.head();
        fx.git(&["switch", "-q", RELEASE]);
        fx.git(&["fetch", "-q", "origin"]);
        fx.git(&[
            "merge",
            "-q",
            "--no-ff",
            "--no-commit",
            &format!("origin/{LINE_A}"),
        ]);
        let body = if closeout {
            block(&old)
        } else {
            "Pending.\n".to_string()
        };
        let status = if closeout { "complete" } else { "todo" };
        fx.write(
            &path("TSK-002"),
            &record("TSK-002", status, criteria, &body),
        );
        fx.git(&["add", "-A"]);
        fx.git(&["commit", "-q", "-m", "merge: import with an evil change"]);
        let result = agree(&fx, what);
        blocks(
            &result,
            what,
            &[needle, "resolves project-management/tasks/TSK-002.md"],
        );
    }
}

/// AC-2, AC-3 (Codex R2-1): the line reopens TSK-001, strengthens its
/// criterion, fixes code and completes it again; the release merge keeps the
/// old record, so the record has no first-parent diff. The judge compares
/// with the import and refuses both the stale completion and the weaker
/// criterion.
#[test]
fn a_resolution_restoring_an_older_record_is_refused() {
    let fx = Fx::new(true);
    let first = fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.cut_release();
    fx.import(LINE_A);
    fx.git(&["push", "-q", "origin", RELEASE]);
    let old_record = record("TSK-001", "complete", CRITERIA, &block(&first));
    // On the line: reopen, strengthen, fix and complete again.
    fx.git(&["switch", "-q", "-C", "task/TSK-001-again", LINE_A]);
    fx.write("src/one.rs", "// one, fixed\n");
    let second = fx.commit("fix: the work again");
    fx.write(
        &path("TSK-001"),
        &record("TSK-001", "complete", STRONGER, &block(&second)),
    );
    fx.commit("docs(records): complete TSK-001 again");
    fx.land(LINE_A, "task/TSK-001-again");
    fx.git(&["switch", "-q", RELEASE]);
    fx.git(&["fetch", "-q", "origin"]);
    fx.git(&[
        "merge",
        "-q",
        "--no-ff",
        "--no-commit",
        &format!("origin/{LINE_A}"),
    ]);
    fx.write(&path("TSK-001"), &old_record);
    fx.git(&["add", "-A"]);
    fx.git(&[
        "commit",
        "-q",
        "-m",
        "merge: import the line, keeping the old record",
    ]);
    let diff = fx.git(&["diff", "HEAD^1", "HEAD", "--", &path("TSK-001")]);
    assert!(
        diff.is_empty(),
        "the record has no first-parent diff: {diff}"
    );
    let merge = fx.head();
    let result = agree(&fx, "restoration");
    blocks(
        &result,
        "restoration",
        &[
            "work.criteria_frozen",
            "TSK-001 changes its criteria directly on the release line (the resolution of merge",
            "work.acceptance_binding",
            &format!(
                "TSK-001 (completed directly on the release line at {}): src/one.rs changed after the reviewed commit {first}",
                &merge[..9]
            ),
        ],
    );
}

/// AC-3 (Codex R3-2, R4-1): the same records-only loosening of a criterion
/// is refused alone, as its own pushed slice with its base at the import
/// (`base=M`), inside a code-bearing range, in a merge resolution and in the
/// release task's fix pull request into the release branch. Its positives:
/// the amendment landed on a line by a planning pull request and brought,
/// and an ordinary amendment on an epic line.
#[test]
fn a_records_only_criterion_is_frozen_on_every_release_path() {
    let loosen = |fx: &Fx| {
        let current = std::fs::read_to_string(fx.root.join(path("TSK-003"))).unwrap();
        fx.write(&path("TSK-003"), &current.replace(CRITERIA, LOOSER));
    };
    let frozen = "TSK-003 changes its criteria directly on the release line";

    // Alone, pushed as its own slice with base = the import merge.
    let fx = Fx::new(true);
    fx.cut_release();
    let import = fx.import(LINE_A);
    fx.git(&["push", "-q", "origin", RELEASE]);
    loosen(&fx);
    fx.commit("docs(records): loosen a criterion");
    assert_eq!(fx.git(&["rev-parse", &format!("origin/{RELEASE}")]), import);
    let alone = agree(&fx, "base=M slice");
    blocks(
        &alone,
        "records only, base=M",
        &["work.criteria_frozen", frozen],
    );
    assert!(
        !alone.1.contains(NO_OWNER),
        "records-only needs no owner:\n{}",
        alone.1
    );

    // Inside a code-bearing range.
    fx.write("src/fix.rs", "// fix\n");
    fx.commit("fix: integrate");
    let mixed = agree(&fx, "code-bearing range");
    blocks(
        &mixed,
        "inside a code-bearing range",
        &["work.criteria_frozen", frozen],
    );

    // In the release task's fix pull request into the release branch,
    // judged as the merge it would create.
    let fx = Fx::new(true);
    fx.cut_release();
    fx.git(&["switch", "-q", "-C", "task/TSK-009-fix", RELEASE]);
    loosen(&fx);
    fx.commit("docs(records): loosen a criterion");
    let pull = fx.ci(RELEASE, "HEAD", "task/TSK-009-fix", Some(RELEASE));
    blocks(
        &pull,
        "the fix pull request",
        &["work.criteria_frozen", frozen],
    );
    assert!(
        pull.1.contains("into 'integration/release-1'"),
        "{}",
        pull.1
    );

    // In a merge resolution.
    let fx = Fx::new(true);
    fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.cut_release();
    fx.git(&["switch", "-q", RELEASE]);
    fx.git(&["fetch", "-q", "origin"]);
    fx.git(&[
        "merge",
        "-q",
        "--no-ff",
        "--no-commit",
        &format!("origin/{LINE_A}"),
    ]);
    loosen(&fx);
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "merge: import, loosening a criterion"]);
    let resolved = agree(&fx, "resolution");
    blocks(
        &resolved,
        "a merge resolution",
        &["work.criteria_frozen", frozen],
    );

    // Positive: the amendment landed on line A by a planning pull request
    // and brought by the import.
    let fx = Fx::new(true);
    fx.amend_on_line(LINE_A, "TSK-003", LOOSER);
    fx.cut_release();
    fx.import(LINE_A);
    passes(&agree(&fx, "brought amendment"), "a brought amendment");

    // Positive: an ordinary planning amendment on an epic line.
    let fx = Fx::new(true);
    fx.git(&["switch", "-q", "-C", "plan/amend", LINE_A]);
    loosen(&fx);
    fx.commit("docs(records): loosen a criterion");
    let line = fx.ci(LINE_A, "HEAD", "plan/amend", None);
    passes(&line, "an ordinary amendment on an epic line");
    assert!(!line.1.contains(SCOPE_LINE), "{}", line.1);
}

// ---------------------------------------------------------------------------
// AC-4: merges that do not qualify
// ---------------------------------------------------------------------------

/// AC-4: a merge whose parent is not on a verified line's first-parent
/// chain is judged wholly as direct work: a branch that fails the epic-line
/// check, an unpushed branch, a forged merge message naming a real line, a
/// line rebased after the merge, and a side branch pushed only to the
/// release branch.
#[test]
fn a_merge_with_an_unqualified_parent_is_direct_work() {
    let direct = "is no import";

    // A line with a commit made directly on it fails the epic-line check.
    let fx = Fx::new(false);
    fx.git(&["switch", "-q", LINE_A]);
    fx.write("src/direct.rs", "// direct\n");
    fx.commit("feat: directly on the line");
    fx.git(&["push", "-q", "origin", LINE_A]);
    fx.cut_release();
    fx.import(LINE_A);
    blocks(
        &agree(&fx, "unverified line"),
        "an unverified line",
        &[direct, NO_OWNER],
    );

    // An unpushed branch, merged under a forged message naming line A.
    let fx = Fx::new(false);
    fx.git(&["switch", "-q", "-C", "local-work", LINE_A]);
    fx.write("src/local.rs", "// local\n");
    fx.commit("feat: local only");
    fx.cut_release();
    fx.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        &format!("Merge branch '{LINE_A}'"),
        "local-work",
    ]);
    blocks(
        &agree(&fx, "forged"),
        "an unpushed branch under a forged message",
        &[direct, NO_OWNER],
    );

    // A line rebased after the merge: the imported tip leaves its chain.
    let fx = Fx::new(false);
    fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.cut_release();
    fx.import(LINE_A);
    fx.git(&["switch", "-q", LINE_A]);
    fx.git(&["reset", "-q", "--hard", "main"]);
    fx.git(&["switch", "-q", "-C", "task/TSK-003-other", LINE_A]);
    fx.write("src/other.rs", "// other\n");
    fx.commit("feat: other work");
    fx.git(&["switch", "-q", LINE_A]);
    fx.merge("task/TSK-003-other");
    fx.git(&["push", "-q", "--force", "origin", LINE_A]);
    fx.git(&["switch", "-q", RELEASE]);
    blocks(
        &agree(&fx, "rebased line"),
        "a line rewritten after the merge",
        &[direct],
    );

    // A side branch pushed only as the release branch, then merged there.
    let fx = Fx::new(false);
    fx.git(&["switch", "-q", "-C", RELEASE, "main"]);
    fx.write("src/side.rs", "// side\n");
    let side = fx.commit("feat: side work");
    fx.git(&["push", "-q", "origin", RELEASE]);
    fx.git(&["reset", "-q", "--hard", "main"]);
    fx.git(&["merge", "-q", "--no-ff", "-m", "merge: side", &side]);
    let result = fx.ci("main", "HEAD", RELEASE, Some("main"));
    blocks(
        &result,
        "a side branch known only through the release ref",
        &[direct, NO_OWNER],
    );
}

/// AC-4 (Codex R2-2): a clean octopus whose second parent is verified line
/// A and whose third is an unverified branch carrying a completion and an
/// unreviewed code change is judged wholly as direct work; the verified
/// parent's qualification is not inherited.
#[test]
fn a_clean_octopus_with_an_unverified_parent_is_direct_work() {
    let fx = Fx::new(false);
    fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
    fx.land(LINE_A, "task/TSK-001-work");
    let stale = fx.head();
    fx.git(&["switch", "-q", "-C", "untrusted", "main"]);
    fx.write(
        &path("TSK-002"),
        &record("TSK-002", "complete", CRITERIA, &block(&stale)),
    );
    fx.commit("docs(records): complete TSK-002");
    fx.write("src/untrusted.rs", "// unreviewed\n");
    fx.commit("feat: unreviewed code");
    fx.cut_release();
    fx.git(&["fetch", "-q", "origin"]);
    fx.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "merge: octopus",
        &format!("origin/{LINE_A}"),
        "untrusted",
    ]);
    assert_eq!(
        fx.git(&["show", "-s", "--format=%P", "HEAD"])
            .split(' ')
            .count(),
        3
    );
    let octopus = fx.head();
    let result = agree(&fx, "octopus");
    blocks(
        &result,
        "an octopus with an unverified parent",
        &[
            "is no import",
            NO_OWNER,
            &format!(
                "TSK-002 (completed directly on the release line at {}): src/untrusted.rs changed after the reviewed commit",
                &octopus[..9]
            ),
        ],
    );
}

// ---------------------------------------------------------------------------
// AC-5: callers
// ---------------------------------------------------------------------------

/// AC-5: a new release branch is judged by its name, even when its head is
/// already published under an unrelated ref (the pushed range is then empty
/// and its pull request into main judges it); a non-fast-forward rewrite
/// keeps the scope.
#[test]
fn callers_scope_by_name() {
    let zero = "0000000000000000000000000000000000000000";

    // A new branch whose head is published as `feat/unrelated` is judged
    // by its name, on everything it adds to main, as its pull request is
    // (Codex R145-R3-1).
    let fx = Fx::new(false);
    fx.git(&["switch", "-q", "-C", "feat/unrelated", "main"]);
    fx.write("src/unrelated.rs", "// unrelated\n");
    let head = fx.commit("feat: unrelated");
    fx.git(&["push", "-q", "origin", "feat/unrelated"]);
    let hook = fx.pre_push(RELEASE, &head, zero);
    blocks(
        &hook,
        "a new release branch at a published head",
        &[SCOPE_LINE, NO_OWNER],
    );
    let pull = fx.ci("main", &head, RELEASE, Some("main"));
    blocks(&pull, "its pull request into main", &[NO_OWNER]);
    assert_eq!(findings(&pull), findings(&hook), "{}\n{}", pull.1, hook.1);

    // A non-fast-forward rewrite of the release branch.
    let fx = Fx::new(false);
    fx.cut_release();
    fx.import(LINE_A);
    fx.git(&["push", "-q", "origin", RELEASE]);
    let published = fx.head();
    fx.git(&["reset", "-q", "--hard", "main"]);
    let current = std::fs::read_to_string(fx.root.join(path("TSK-003"))).unwrap();
    fx.write(&path("TSK-003"), &current.replace(CRITERIA, LOOSER));
    let rewrite = fx.commit("docs(records): loosen a criterion");
    let hook = fx.pre_push(RELEASE, &rewrite, &published);
    blocks(&hook, "a rewrite", &["work.criteria_frozen", SCOPE_LINE]);
}

/// AC-5: a default target whose tip object is missing and cannot be
/// fetched, a destination naming a default branch it lacks, and an
/// unreachable destination each fail closed naming what is missing; an
/// empty destination uses the built-in pattern.
#[test]
fn callers_fail_closed_when_scope_cannot_be_read() {
    // The default target's tip is advertised but its object cannot be had:
    // another clone advanced main, and the destination cannot send it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let fx = Fx::new(false);
        let parent = fx.root.parent().unwrap();
        run_git(
            parent,
            &["clone", "-q", fx.origin.to_str().unwrap(), "other"],
        );
        let other = parent.join("other");
        run_git(
            &other,
            &["commit", "-q", "--allow-empty", "-m", "chore: advance main"],
        );
        run_git(&other, &["push", "-q", "origin", "main"]);
        let missing = run_git(&other, &["rev-parse", "HEAD"]);
        let object = fx
            .origin
            .join("objects")
            .join(&missing[..2])
            .join(&missing[2..]);
        std::fs::set_permissions(&object, std::fs::Permissions::from_mode(0o000)).unwrap();
        let result = fx.ci("HEAD~0", "HEAD", "feat/any", None);
        std::fs::set_permissions(&object, std::fs::Permissions::from_mode(0o444)).unwrap();
        blocks(
            &result,
            "a missing object",
            &["cannot be decided", &missing, "could not be fetched"],
        );
    }

    // The destination names a default branch it does not have.
    let fx = Fx::new(false);
    run_git(&fx.origin, &["symbolic-ref", "HEAD", "refs/heads/nope"]);
    let result = fx.ci("HEAD~0", "HEAD", "feat/any", None);
    blocks(
        &result,
        "an unresolved default target",
        &["cannot be decided", "default branch"],
    );

    // An unreachable destination.
    let fx = Fx::new(false);
    let out = clean_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args([
            "ci",
            "--base",
            "HEAD",
            "--head",
            "HEAD",
            "--branch",
            "feat/any",
            "--destination",
            "/nonexistent/destination.git",
        ])
        .current_dir(&fx.root)
        .output()
        .unwrap();
    blocks(
        &output(&out),
        "an unreachable destination",
        &["cannot be decided", "ls-remote"],
    );

    // A destination with no branch yet: the built-in pattern.
    let fx = Fx::new(false);
    let empty = fx.root.parent().unwrap().join("empty.git");
    run_git(
        fx.root.parent().unwrap(),
        &["init", "-q", "--bare", "-b", "main", "empty.git"],
    );
    let out = clean_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args([
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            RELEASE,
            "--destination",
            empty.to_str().unwrap(),
        ])
        .current_dir(&fx.root)
        .output()
        .unwrap();
    let result = output(&out);
    passes(&result, "an empty destination");
    assert!(
        result
            .1
            .contains("built in (the destination has no default target yet)"),
        "{}",
        result.1
    );
}

// ---------------------------------------------------------------------------
// AC-7: the release-integration task
// ---------------------------------------------------------------------------

/// AC-7: a records-only direct commit needs no owner; a code commit needs
/// the release-integration task's completion inside the range bound to the
/// head: with it at the head it passes, without it or with code after it
/// it is refused, and with no task carrying the role it is refused as
/// having no owner.
#[test]
fn direct_code_belongs_to_the_release_integration_task() {
    // A records-only commit, no owner at all.
    let fx = Fx::new(false);
    fx.cut_release();
    fx.write(
        "project-management/epics/EPC-001.md",
        &epic("EPC-001").replace("An outcome.", "An outcome, restated."),
    );
    fx.commit("docs(records): restate the outcome");
    passes(&agree(&fx, "records only"), "a records-only direct commit");

    // Code with no task carrying the role.
    fx.write("src/fix.rs", "// fix\n");
    fx.commit("fix: integrate");
    blocks(&agree(&fx, "no holder"), "code with no holder", &[NO_OWNER]);

    // Code without the holder's completion.
    let fx = Fx::new(true);
    fx.cut_release();
    fx.write("src/fix.rs", "// fix\n");
    let reviewed = fx.commit("fix: integrate");
    blocks(
        &agree(&fx, "holder open"),
        "code without the completion",
        &["TSK-009 (`role: release-integration`) owns"],
    );

    // With the holder's completion at the head (through the verb).
    fx.write(
        &path(HOLDER),
        &record(HOLDER, "todo", CRITERIA, &block(&reviewed)),
    );
    passes(
        &fx.status_complete(HOLDER),
        "the holder completes at the head",
    );
    let completed = fx.commit("docs(records): complete the release integration");
    passes(
        &agree(&fx, "holder complete"),
        "code with the completion at the head",
    );

    // Code after the completion.
    fx.write("src/after.rs", "// after\n");
    fx.commit("fix: after the completion");
    let result = fx.ci("main", "HEAD", RELEASE, Some("main"));
    blocks(
        &result,
        "code after the completion",
        &[&format!(
            "TSK-009 (completed directly on the release line at {}): src/after.rs changed after the reviewed commit",
            &completed[..9]
        )],
    );
}

/// AC-7: the release task's fix pull request into the release branch is
/// direct work bound to its reviewed head; the final head, where the
/// owner's completion is the last change, passes into main.
#[test]
fn the_release_fix_pull_request_binds_to_its_head() {
    let fx = Fx::new(true);
    fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.cut_release();
    fx.import(LINE_A);
    fx.git(&["push", "-q", "origin", RELEASE]);
    fx.git(&["switch", "-q", "-C", "task/TSK-009-fix", RELEASE]);
    fx.write("src/fix.rs", "// fix\n");
    let reviewed = fx.commit("fix: integrate");
    let open = fx.ci(RELEASE, "HEAD", "task/TSK-009-fix", Some(RELEASE));
    blocks(
        &open,
        "the fix pull request before the completion",
        &["TSK-009 (`role: release-integration`) owns"],
    );
    fx.write(
        &path(HOLDER),
        &record(HOLDER, "todo", CRITERIA, &block(&reviewed)),
    );
    passes(&fx.status_complete(HOLDER), "the verb on the fix branch");
    fx.commit("docs(records): complete the release integration");
    passes(
        &fx.ci(RELEASE, "HEAD", "task/TSK-009-fix", Some(RELEASE)),
        "the fix pull request",
    );
    fx.git(&["switch", "-q", RELEASE]);
    fx.merge("task/TSK-009-fix");
    let final_pr = fx.ci("main", "HEAD", RELEASE, Some("main"));
    // The merge of the fix branch lands after the completion, so the final
    // head carries it only through the merge: the completion is bound to
    // the fix pull request's head, which the merge changes nothing beyond.
    passes(&final_pr, "the final release pull request");
}

// ---------------------------------------------------------------------------
// AC-8: full tree entries
// ---------------------------------------------------------------------------

/// AC-8 (Codex R3-1): a qualifying import that changes only a path's mode
/// or its entry type, with the same object id, is a resolution and direct
/// work; an exact entry is brought.
#[cfg(unix)]
#[test]
fn a_mode_or_type_change_in_an_import_is_a_resolution() {
    for what in ["mode", "type", "exact"] {
        let fx = Fx::new(false);
        fx.git(&["switch", "-q", "-C", "task/TSK-001-work", LINE_A]);
        fx.write("src/work.rs", "src/lib.rs");
        fx.commit("feat: add work");
        fx.land(LINE_A, "task/TSK-001-work");
        fx.cut_release();
        fx.git(&["fetch", "-q", "origin"]);
        fx.git(&[
            "merge",
            "-q",
            "--no-ff",
            "--no-commit",
            &format!("origin/{LINE_A}"),
        ]);
        match what {
            "mode" => fx.git(&["update-index", "--chmod=+x", "src/work.rs"]),
            "type" => {
                let blob = fx.git(&["rev-parse", ":src/work.rs"]);
                fx.git(&[
                    "update-index",
                    "--cacheinfo",
                    &format!("120000,{blob},src/work.rs"),
                ])
            }
            _ => String::new(),
        };
        fx.git(&["commit", "-q", "-m", "merge: import"]);
        fx.git(&["reset", "-q", "--hard", "HEAD"]);
        let entry = fx.git(&["ls-tree", "HEAD", "src/work.rs"]);
        let result = agree(&fx, what);
        if what == "exact" {
            assert!(entry.starts_with("100644"), "{entry}");
            passes(&result, "an exact entry");
        } else {
            assert!(!entry.starts_with("100644"), "{entry}");
            blocks(
                &result,
                what,
                &["resolves src/work.rs differently from the import", NO_OWNER],
            );
        }
    }
}

// ---------------------------------------------------------------------------
// AC-9: scope
// ---------------------------------------------------------------------------

/// AC-9: epic lines sharing a fork commit and a stacked line syncing from
/// its parent line keep R-52 and R-60; an unmatched branch is ordinary; a
/// configured pattern read from the default target selects `release/*` and
/// drops the built-in; an epic line pushed under a matching name gets
/// release rules.
#[test]
fn scope_comes_from_the_name_under_the_default_targets_policy() {
    // Line B cut from a commit of line A, then syncing from line A: both
    // ordinary.
    let fx = Fx::new(false);
    fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
    fx.land(LINE_A, "task/TSK-001-work");
    fx.git(&["switch", "-q", LINE_B]);
    fx.git(&["reset", "-q", "--hard", LINE_A]);
    fx.git(&["push", "-q", "--force", "origin", LINE_B]);
    fx.build_and_complete(LINE_A, "TSK-003", "src/three.rs");
    fx.land(LINE_A, "task/TSK-003-work");
    fx.git(&["switch", "-q", LINE_B]);
    fx.merge(LINE_A);
    let sync = fx.ci("main", "HEAD", LINE_B, Some("main"));
    assert!(
        !sync.1.contains(SCOPE_LINE),
        "a stacked line is ordinary:\n{}",
        sync.1
    );
    let unmatched = fx.ci("main", "HEAD", "feat/combined", None);
    assert!(!unmatched.1.contains(SCOPE_LINE), "{}", unmatched.1);

    // A configured pattern on main.
    let fx = Fx::with_policy(false, ", \"release_branch_pattern\": \"release/*\"");
    fx.git(&["switch", "-q", "-C", "release/1", "main"]);
    fx.write("src/fix.rs", "// fix\n");
    fx.commit("fix: integrate");
    let configured = fx.ci("main", "HEAD", "release/1", None);
    blocks(
        &configured,
        "a configured pattern",
        &[NO_OWNER, "under pattern 'release/*' (main at"],
    );
    let builtin = fx.ci("main", "HEAD", RELEASE, None);
    assert!(!builtin.1.contains(SCOPE_LINE), "{}", builtin.1);

    // An epic line pushed under a matching name: its own landings are direct.
    let fx = Fx::new(false);
    fx.build_and_complete(LINE_A, "TSK-001", "src/one.rs");
    let landing = fx.land(LINE_A, "task/TSK-001-work");
    let renamed = fx.ci("main", LINE_A, RELEASE, Some("main"));
    blocks(
        &renamed,
        "an epic line under a release name",
        &[&format!("merge {} is no import", &landing[..9]), NO_OWNER],
    );
}

/// AC-9 (Codex R6-2): two successive pushes, the first editing only the
/// holder's target and role, leave the scope unchanged, and the second
/// push's criterion edit is refused.
#[test]
fn a_records_only_retarget_does_not_change_the_next_push() {
    let fx = Fx::new(true);
    fx.cut_release();
    let holder = std::fs::read_to_string(fx.root.join(path(HOLDER))).unwrap();
    fx.write(
        &path(HOLDER),
        &holder
            .replace(
                "integration_target: main",
                "integration_target: integration/other",
            )
            .replace("role: release-integration\n", ""),
    );
    let first = fx.commit("docs(records): retarget the release task");
    let one = fx.pre_push(
        RELEASE,
        &first,
        &fx.git(&["rev-parse", &format!("origin/{RELEASE}")]),
    );
    passes(&one, "the records-only retarget");
    assert!(one.1.contains(SCOPE_LINE), "{}", one.1);
    fx.git(&["push", "-q", "origin", RELEASE]);
    let current = std::fs::read_to_string(fx.root.join(path("TSK-003"))).unwrap();
    fx.write(&path("TSK-003"), &current.replace(CRITERIA, LOOSER));
    let second = fx.commit("docs(records): loosen a criterion");
    let two = fx.pre_push(RELEASE, &second, &first);
    blocks(
        &two,
        "the second push",
        &["work.criteria_frozen", SCOPE_LINE],
    );
}

/// AC-4: a merge whose parent lies on the default target's first-parent
/// chain is an import, so syncing a fix that landed on main into the
/// release branch needs no release-integration owner; the same code from a
/// side branch is direct work.
#[test]
fn a_sync_from_the_default_target_is_an_import() {
    let fx = Fx::new(false);
    fx.cut_release();
    fx.git(&["switch", "-q", "main"]);
    fx.write("src/hotfix.rs", "// hotfix\n");
    fx.commit("fix: a hotfix on main");
    fx.git(&["push", "-q", "origin", "main"]);
    let merge = fx.import("main");
    let result = agree(&fx, "a sync from main");
    passes(&result, "a sync from the default target");
    assert!(
        result
            .1
            .contains(&format!("release path: {}: import", &merge[..9])),
        "{}",
        result.1
    );

    // Control: the same change from a branch the destination never had.
    let fx = Fx::new(false);
    fx.cut_release();
    fx.git(&["switch", "-q", "-C", "fix/local", "main"]);
    fx.write("src/hotfix.rs", "// hotfix\n");
    fx.commit("fix: a hotfix on a side branch");
    fx.git(&["switch", "-q", RELEASE]);
    fx.merge("fix/local");
    blocks(
        &agree(&fx, "a side branch"),
        "the same change from a side branch",
        &["is no import", NO_OWNER],
    );
}

/// AC-9 (Codex R6-4): before and after landing, a verified line's pull
/// request into the release branch carrying a planning amendment is
/// brought, and the release task's pull request changing a criterion is
/// frozen.
#[test]
fn an_import_is_judged_the_same_however_it_arrives() {
    let fx = Fx::new(true);
    fx.amend_on_line(LINE_A, "TSK-003", STRONGER);
    fx.cut_release();
    let before = fx.ci(RELEASE, LINE_A, LINE_A, Some(RELEASE));
    passes(&before, "the line's pull request into the release branch");
    assert!(before.1.contains(SCOPE_LINE), "{}", before.1);
    fx.import(LINE_A);
    passes(&agree(&fx, "the landed import"), "the landed import");
    fx.git(&["push", "-q", "origin", RELEASE]);

    fx.git(&["switch", "-q", "-C", "task/TSK-009-criterion", RELEASE]);
    let current = std::fs::read_to_string(fx.root.join(path("TSK-004"))).unwrap();
    fx.write(&path("TSK-004"), &current.replace(CRITERIA, LOOSER));
    fx.commit("docs(records): loosen a criterion");
    let pull = fx.ci(RELEASE, "HEAD", "task/TSK-009-criterion", Some(RELEASE));
    blocks(
        &pull,
        "the release task's pull request",
        &[
            "work.criteria_frozen",
            "TSK-004 changes its criteria directly",
        ],
    );
    fx.git(&["switch", "-q", RELEASE]);
    fx.merge("task/TSK-009-criterion");
    blocks(
        &agree(&fx, "landed"),
        "after landing",
        &[
            "work.criteria_frozen",
            "TSK-004 changes its criteria directly",
        ],
    );
}

/// AC-9: the policy check refuses a release pattern that could match the
/// default target or an epic line, and the validator refuses a second open
/// release-integration task.
#[test]
fn the_policy_and_the_validator_refuse_what_would_blur_scope() {
    for pattern in [
        "*",
        "integration/*",
        "integration/EPC-*",
        "integration/EPC-999-*",
        "main",
        "ma?n",
    ] {
        let fx = Fx::with_policy(
            false,
            &format!(", \"release_branch_pattern\": \"{pattern}\""),
        );
        let out = clean_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
            .args([
                "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x",
            ])
            .current_dir(&fx.root)
            .output()
            .unwrap();
        let result = output(&out);
        assert_ne!(result.0, 0, "{pattern}: {}", result.1);
        assert!(
            result.1.contains("git.release_branch_pattern"),
            "{pattern}: {}",
            result.1
        );
    }
    let fx = Fx::new(true);
    let second = record("TSK-004", "todo", CRITERIA, "Pending.\n")
        .replace("created:", "role: release-integration\ncreated:");
    fx.write(&path("TSK-004"), &second);
    let out = clean_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(["validate", "--docs"])
        .current_dir(&fx.root)
        .output()
        .unwrap();
    let result = output(&out);
    assert_ne!(result.0, 0, "{}", result.1);
    assert!(
        result
            .1
            .contains("are both open with `role: release-integration`"),
        "{}",
        result.1
    );
}
