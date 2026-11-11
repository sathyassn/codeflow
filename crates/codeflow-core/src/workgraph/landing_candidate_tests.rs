//! TSK-1351 AC-2: a landing candidate binds to its line only when every
//! condition holds, and each refusal names its cause. A bare `origin` holds
//! `main`, whose records bind `integration/EPC-001-line`, and the line; the
//! clone's members are cut from the line's fetched tip.

use super::*;

const LINE: &str = "integration/EPC-001-line";
const CANDIDATE: &str = "candidate/epc-001-c1";

fn git(dir: &Path, args: &[&str]) -> String {
    let out = crate::git::command()
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

const EPIC: &str = "---\nid: EPC-001\ntitle: \"outcome\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-11-10\n---\n\n# EPC-001: outcome\n\n## Summary\n\nAn outcome.\n";

fn task(id: &str) -> String {
    format!(
        "---\nid: {id}\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: {LINE}\ntitle: \"work {id}\"\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-11-10\n---\n\n# {id}: work\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n"
    )
}

struct Fixture {
    clone: tempfile::TempDir,
    _origin: tempfile::TempDir,
    /// The line's fetched tip.
    tip: String,
    /// `main`'s tip.
    main: String,
}

impl Fixture {
    fn root(&self) -> &Path {
        self.clone.path()
    }

    /// A member branch `name`, cut from the line's tip, adding `path`.
    fn member(&self, name: &str, path: &str) {
        let root = self.root();
        git(root, &["switch", "-q", "-c", name, &self.tip]);
        write(root, path, &format!("// {name}\n"));
        commit(root, &format!("feat: {name}"));
    }

    /// Merge `branch` onto the checked-out branch with a `--no-ff` merge.
    fn merge(&self, branch: &str) {
        git(
            self.root(),
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                &format!("chore(merge): {branch}"),
                branch,
            ],
        );
    }

    /// The candidate: the line's tip with `members` merged in order.
    fn candidate(&self, members: &[&str]) -> String {
        git(self.root(), &["switch", "-q", "-C", CANDIDATE, &self.tip]);
        for member in members {
            self.merge(member);
        }
        git(self.root(), &["rev-parse", "HEAD"])
    }

    fn bind_with(
        &self,
        head: &str,
        advertised: &BTreeMap<String, String>,
        policy: &GitPolicy,
    ) -> Result<BoundLine, String> {
        bind(
            self.root(),
            &Inputs {
                branch: CANDIDATE,
                head,
                policy,
                default_tip: &self.main,
                advertised,
                line: None,
            },
        )
    }

    fn bind(&self, head: &str) -> Result<BoundLine, String> {
        self.bind_with(head, &self.advertised(), &GitPolicy::default())
    }

    /// The branch tips origin advertises now.
    fn advertised(&self) -> BTreeMap<String, String> {
        let listed = git(self.root(), &["ls-remote", "--heads", "origin"]);
        listed
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .filter_map(|(sha, name)| {
                Some((
                    name.strip_prefix("refs/heads/")?.to_string(),
                    sha.to_string(),
                ))
            })
            .collect()
    }

    /// Publish `branch` at `at` on origin and fetch it.
    fn publish(&self, at: &str, branch: &str) {
        git(
            self.root(),
            &["push", "-q", "origin", &format!("{at}:refs/heads/{branch}")],
        );
        git(self.root(), &["fetch", "-q", "origin"]);
    }
}

fn fixture() -> Fixture {
    let origin = tempfile::tempdir().unwrap();
    git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    let clone = tempfile::tempdir().unwrap();
    let root = clone.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    write(root, "project-management/epics/EPC-001.md", EPIC);
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &task("TSK-001"),
    );
    write(root, "README.md", "base\n");
    let main = commit(root, "chore: plan the line");
    git(root, &["branch", LINE]);
    git(
        root,
        &["remote", "add", "origin", origin.path().to_str().unwrap()],
    );
    git(root, &["push", "-q", "origin", "main", LINE]);
    git(root, &["fetch", "-q", "origin"]);
    let fixture = Fixture {
        clone,
        _origin: origin,
        tip: main.clone(),
        main,
    };
    fixture.member("task/TSK-001-one", "src/one.rs");
    fixture.member("task/TSK-002-two", "src/two.rs");
    fixture
}

fn refused(result: Result<BoundLine, String>, needle: &str) {
    match result {
        Ok(bound) => panic!("bound {bound:?}, expected a refusal naming {needle:?}"),
        Err(cause) => assert!(cause.contains(needle), "{needle:?} not in: {cause}"),
    }
}

/// A clean stack of member merges on the line's fetched, advertised tip is
/// bound to the line, its tip and its merges.
#[test]
fn a_clean_stack_of_member_merges_binds_to_its_line() {
    let f = fixture();
    let head = f.candidate(&["task/TSK-001-one", "task/TSK-002-two"]);
    assert_eq!(
        f.bind(&head).unwrap(),
        BoundLine {
            line: LINE.to_string(),
            tip: f.tip.clone(),
            merges: 2,
        }
    );
}

/// A direct commit on the candidate is refused, naming the commit.
#[test]
fn a_direct_commit_on_the_candidate_is_refused() {
    let f = fixture();
    f.candidate(&["task/TSK-001-one"]);
    write(f.root(), "src/direct.rs", "// direct\n");
    let direct = commit(f.root(), "feat: a direct commit");
    refused(
        f.bind(&direct),
        &format!("{} on its first-parent chain", &direct[..9]),
    );
    refused(f.bind(&direct), "is a direct commit");
}

/// A member merge whose tree is not the clean merge of its parents is
/// refused, naming the merge and the path it adds.
#[test]
fn an_evil_merge_is_refused() {
    let f = fixture();
    git(f.root(), &["switch", "-q", "-C", CANDIDATE, &f.tip]);
    git(
        f.root(),
        &["merge", "-q", "--no-ff", "--no-commit", "task/TSK-001-one"],
    );
    write(f.root(), "src/evil.rs", "// evil\n");
    let evil = commit(f.root(), "chore(merge): task/TSK-001-one");
    refused(
        f.bind(&evil),
        &format!("merge {} is not the clean merge", &evil[..9]),
    );
    refused(f.bind(&evil), "src/evil.rs");
}

/// A candidate whose first parent is a member head puts that member's own
/// commit on the first-parent chain, so it is refused as a direct commit.
#[test]
fn a_swapped_first_parent_is_refused() {
    let f = fixture();
    git(
        f.root(),
        &["switch", "-q", "-C", CANDIDATE, "task/TSK-001-one"],
    );
    f.merge("task/TSK-002-two");
    let head = git(f.root(), &["rev-parse", "HEAD"]);
    let member = git(f.root(), &["rev-parse", "task/TSK-001-one"]);
    refused(
        f.bind(&head),
        &format!("{} on its first-parent chain", &member[..9]),
    );
}

/// A candidate cut from an `integration/` copy that no epic declares is
/// refused: the name shape binds nothing.
#[test]
fn a_cut_from_an_undeclared_integration_copy_is_refused() {
    let f = fixture();
    let copy = f.candidate(&["task/TSK-001-one"]);
    f.publish(&copy, "integration/EPC-009-copy");
    f.merge("task/TSK-002-two");
    let head = git(f.root(), &["rev-parse", "HEAD"]);
    // The real line's tip also lies on the chain, so a search depth that
    // stops short of it leaves the copy as the only line found.
    let shallow = GitPolicy {
        candidate_search_depth: 1,
        ..GitPolicy::default()
    };
    let bound = f.bind_with(&head, &f.advertised(), &shallow);
    refused(bound, "'integration/EPC-009-copy' names EPC-009");
    refused(
        f.bind_with(&head, &f.advertised(), &shallow),
        "not a bound epic line",
    );
}

/// A fetched tip the destination no longer advertises is stale, and a line
/// this clone never fetched leaves no tip on the chain: both are refused.
#[test]
fn a_stale_or_unfetched_tip_is_refused() {
    let f = fixture();
    let head = f.candidate(&["task/TSK-001-one"]);
    let mut moved = f.advertised();
    moved.insert(LINE.to_string(), "1".repeat(40));
    refused(
        f.bind_with(&head, &moved, &GitPolicy::default()),
        "the fetched tip is stale",
    );
    let mut missing = f.advertised();
    missing.remove(LINE);
    refused(
        f.bind_with(&head, &missing, &GitPolicy::default()),
        &format!("the destination does not advertise '{LINE}'"),
    );
    // Advertised but never fetched here: the line's records cannot be read.
    git(
        f.root(),
        &["update-ref", "-d", &format!("refs/remotes/origin/{LINE}")],
    );
    refused(f.bind(&head), &format!("'origin/{LINE}' is missing"));
    // Neither advertised nor fetched: no line tip on the chain.
    refused(
        f.bind_with(&head, &missing, &GitPolicy::default()),
        "reaches no epic line tip",
    );
}

/// Two epic lines fetched at the same commit leave the line ambiguous.
#[test]
fn two_lines_at_one_commit_are_refused() {
    let f = fixture();
    f.publish(&f.tip, "integration/EPC-001-twin");
    let head = f.candidate(&["task/TSK-001-one"]);
    refused(f.bind(&head), "the tips of several epic lines");
}

/// Two lines the destination advertises at one commit stay ambiguous when
/// this clone lacks the tracking ref of one of them: the destination's
/// inventory decides, never the local tracking refs.
#[test]
fn two_advertised_lines_at_one_commit_are_refused_with_one_tracking_ref_missing() {
    let f = fixture();
    f.publish(&f.tip, "integration/EPC-001-twin");
    git(
        f.root(),
        &[
            "update-ref",
            "-d",
            "refs/remotes/origin/integration/EPC-001-twin",
        ],
    );
    let head = f.candidate(&["task/TSK-001-one"]);
    refused(f.bind(&head), "the tips of several epic lines");
    refused(f.bind(&head), "'integration/EPC-001-twin'");
}

/// Two line tips at different points of the first-parent chain leave the
/// line ambiguous: the walk does not stop at the first tip it meets.
#[test]
fn two_line_tips_at_different_points_of_the_chain_are_refused() {
    let f = fixture();
    let first = f.candidate(&["task/TSK-001-one"]);
    f.publish(&first, "integration/EPC-002-later");
    f.merge("task/TSK-002-two");
    let head = git(f.root(), &["rev-parse", "HEAD"]);
    refused(f.bind(&head), "the tips of several epic lines");
    refused(
        f.bind(&head),
        &format!("'integration/EPC-002-later' at {}", &first[..9]),
    );
    refused(f.bind(&head), &format!("'{LINE}' at {}", &f.tip[..9]));
}

/// The walk stops at `git.candidate_search_depth`, and the line a push
/// updates must be the line the candidate is cut from.
#[test]
fn the_depth_and_the_pushed_line_bound_the_walk() {
    let f = fixture();
    let head = f.candidate(&["task/TSK-001-one", "task/TSK-002-two"]);
    let shallow = GitPolicy {
        candidate_search_depth: 1,
        ..GitPolicy::default()
    };
    refused(
        f.bind_with(&head, &f.advertised(), &shallow),
        "within 1 commits",
    );
    let elsewhere = bind(
        f.root(),
        &Inputs {
            branch: CANDIDATE,
            head: &head,
            policy: &GitPolicy::default(),
            default_tip: &f.main,
            advertised: &f.advertised(),
            line: Some("integration/EPC-002-other"),
        },
    );
    refused(elsewhere, "not of 'integration/EPC-002-other'");
    let unnamed = bind(
        f.root(),
        &Inputs {
            branch: "feat/x",
            head: &head,
            policy: &GitPolicy::default(),
            default_tip: &f.main,
            advertised: &f.advertised(),
            line: None,
        },
    );
    refused(unnamed, "does not start with `candidate/`");
}
