//! Fixtures for `work remove-candidate` (TSK-1308 AC-2 to AC-5): a bare
//! `origin` holding `main` and the line `integration/x`, and a clone whose
//! local candidate branch merged the same feature the line merged by a
//! different commit.

use super::*;

const LINE: &str = "integration/x";
const CANDIDATE: &str = "candidate/c1";

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

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

struct Fixture {
    clone: tempfile::TempDir,
    _origin: tempfile::TempDir,
}

impl Fixture {
    fn root(&self) -> &Path {
        self.clone.path()
    }
}

/// `main` and `integration/x` on origin; local `feature` adds `src/a.rs`.
/// `policy` is committed on `main` as `.codeflow/policy.json` when given.
fn fixture(policy: Option<&str>) -> Fixture {
    let origin = tempfile::tempdir().unwrap();
    git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    let clone = tempfile::tempdir().unwrap();
    let root = clone.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    write(root, "README.md", "base\n");
    if let Some(policy) = policy {
        write(root, ".codeflow/policy.json", policy);
    }
    commit(root, "chore: base");
    git(root, &["branch", LINE]);
    git(root, &["switch", "-q", "-c", "feature"]);
    write(root, "src/a.rs", "pub fn a() {}\n");
    commit(root, "feat: a");
    git(root, &["switch", "-q", "main"]);
    git(
        root,
        &["remote", "add", "origin", origin.path().to_str().unwrap()],
    );
    git(root, &["push", "-q", "origin", "main", LINE]);
    git(root, &["fetch", "-q", "origin"]);
    git(
        root,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    Fixture {
        clone,
        _origin: origin,
    }
}

/// The candidate merges `feature` onto the line; `extra` adds a file the
/// line never gets.
fn candidate(root: &Path, extra: bool) {
    git(root, &["switch", "-q", "-c", CANDIDATE, LINE]);
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "chore(merge): candidate",
            "feature",
        ],
    );
    if extra {
        write(root, "src/extra.rs", "pub fn extra() {}\n");
        commit(root, "feat: only on the candidate");
    }
    git(root, &["switch", "-q", "main"]);
}

/// The line merges `feature` by a different commit, so it gets the same
/// tree, then `after` more first-parent commits.
fn land(root: &Path, after: usize) {
    git(
        root,
        &["switch", "-q", "-c", "landing", &format!("origin/{LINE}")],
    );
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge pull request #1",
            "feature",
        ],
    );
    for n in 0..after {
        write(root, &format!("later/{n}.txt"), "later\n");
        commit(root, "docs: later work on the line");
    }
    git(root, &["push", "-q", "origin", &format!("landing:{LINE}")]);
    git(root, &["switch", "-q", "main"]);
    git(root, &["branch", "-q", "-D", "landing"]);
}

#[allow(clippy::unnecessary_wraps)] // the shape `OpenPullRequests` takes
fn no_pull_request(_: &str) -> Result<Vec<u64>, String> {
    Ok(Vec::new())
}

fn exists(root: &Path, branch: &str) -> bool {
    Repository::open(root)
        .unwrap()
        .find_reference(&format!("refs/heads/{branch}"))
        .is_ok()
}

/// AC-1: before this route, the only removal was `git branch -D`, which
/// git-guard's `git.discard_uncommitted` refuses for such a candidate
/// (`hooks/git_discard.rs` `delete_branches`): no other ref reaches its
/// commits, though the line holds its tree. That refusal stays.
#[test]
fn the_guard_refuses_a_plain_delete_of_a_landed_candidate() {
    use crate::hooks::git_discard::{inspect, Intent};
    let fixture = fixture(None);
    let root = fixture.root();
    candidate(root, false);
    land(root, 0);
    let refusal = inspect(
        root,
        None,
        &Intent::ForceDeleteBranches(vec![CANDIDATE.to_string()]),
    )
    .unwrap()
    .expect("the plain delete is refused");
    assert_eq!(
        refusal,
        "deleting 'candidate/c1' would leave its commits without another live ref"
    );
}

/// AC-2, AC-4: a line that got the same tree through a different commit
/// proves the candidate landed; the branch goes and the ledger records
/// the branch, its tip, the matching line commit and the tree.
#[test]
fn a_candidate_whose_tree_is_on_the_line_is_removed_and_recorded() {
    let fixture = fixture(None);
    let root = fixture.root();
    candidate(root, false);
    land(root, 2);
    let tip = git(root, &["rev-parse", CANDIDATE]);
    let tree = git(root, &["rev-parse", &format!("{CANDIDATE}^{{tree}}")]);
    let line_tip = git(
        root,
        &["ls-remote", "origin", &format!("refs/heads/{LINE}")],
    );
    let line_tip = line_tip.split('\t').next().unwrap().to_string();
    let landed = git(root, &["rev-parse", &format!("{line_tip}~2")]);
    assert_ne!(tip, landed, "the line landed it by a different commit");
    let removed = remove(root, CANDIDATE, LINE, &no_pull_request).unwrap();
    assert_eq!(removed.tip.to_string(), tip);
    assert_eq!(removed.tree.to_string(), tree);
    assert_eq!(removed.line_commit.to_string(), landed);
    assert_eq!(removed.line_tip.to_string(), line_tip);
    assert_eq!(removed.line, format!("origin/{LINE}"));
    assert!(!exists(root, CANDIDATE));
    let ledger = std::fs::read_to_string(
        root.join(".git/codeflow/ledger/memory-events/memory-events.jsonl"),
    )
    .unwrap();
    let event: serde_json::Value = serde_json::from_str(ledger.lines().last().unwrap()).unwrap();
    assert_eq!(event["event"], EVENT);
    assert_eq!(event["branch"], CANDIDATE);
    assert_eq!(event["tip"], tip);
    assert_eq!(event["line"], format!("origin/{LINE}"));
    assert_eq!(event["line_commit"], landed);
    assert_eq!(event["tree"], tree);
}

fn refused(root: &Path, branch: &str, line: &str, prs: OpenPullRequests<'_>, needle: &str) {
    let error = remove(root, branch, line, prs).unwrap_err();
    assert!(error.contains(needle), "{needle}: {error}");
    assert!(exists(root, CANDIDATE), "a refusal keeps the candidate");
}

/// AC-3: different trees, and a tree only on a side branch, refuse.
#[test]
fn a_tree_the_line_did_not_land_refuses() {
    // The trees differ.
    let fixture_a = fixture(None);
    candidate(fixture_a.root(), true);
    land(fixture_a.root(), 0);
    refused(
        fixture_a.root(),
        CANDIDATE,
        LINE,
        &no_pull_request,
        "the trees differ",
    );

    // The tree is only on a side branch: the line merged a commit with the
    // candidate's tree as its second parent, after its own change.
    let fixture_b = fixture(None);
    let root = fixture_b.root();
    candidate(root, false);
    git(
        root,
        &["switch", "-q", "-c", "side", &format!("origin/{LINE}")],
    );
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "chore(merge): side",
            "feature",
        ],
    );
    git(
        root,
        &["switch", "-q", "-c", "landing", &format!("origin/{LINE}")],
    );
    write(root, "other.txt", "other\n");
    commit(root, "docs: other work first");
    git(
        root,
        &["merge", "-q", "--no-ff", "-m", "Merge the side", "side"],
    );
    git(root, &["push", "-q", "origin", &format!("landing:{LINE}")]);
    git(root, &["switch", "-q", "main"]);
    refused(
        root,
        CANDIDATE,
        LINE,
        &no_pull_request,
        "only on a side branch",
    );
}

/// AC-3: the line, then the branch, cannot be read; an open pull request,
/// or one that cannot be asked about; a worktree uses it; not a candidate.
/// Each refuses, names its cause, and keeps the branch.
#[test]
fn each_other_cause_refuses_and_keeps_the_branch() {
    let fixture_c = fixture(None);
    let root = fixture_c.root();
    candidate(root, false);
    land(root, 0);
    refused(
        root,
        CANDIDATE,
        "integration/absent",
        &no_pull_request,
        "cannot be fetched from origin",
    );
    refused(
        root,
        "candidate/absent",
        LINE,
        &no_pull_request,
        "the branch `candidate/absent` cannot be read",
    );
    refused(
        root,
        CANDIDATE,
        LINE,
        &|_| Ok(vec![7]),
        "has an open pull request (#7)",
    );
    refused(
        root,
        CANDIDATE,
        LINE,
        &|_| Err("gh is not signed in".into()),
        "cannot tell whether `candidate/c1` has an open pull request: gh is not signed in",
    );
    git(root, &["branch", "task/TSK-001-x", CANDIDATE]);
    refused(
        root,
        "task/TSK-001-x",
        LINE,
        &no_pull_request,
        "is not a landing candidate branch",
    );
    assert!(exists(root, "task/TSK-001-x"));
    let trees = tempfile::tempdir().unwrap();
    let at = trees.path().join("c1");
    git(
        root,
        &["worktree", "add", "-q", at.to_str().unwrap(), CANDIDATE],
    );
    refused(
        root,
        CANDIDATE,
        LINE,
        &no_pull_request,
        "a worktree has `candidate/c1` checked out",
    );
}

/// AC-2, review finding 1: the line must be a line. A copy of the
/// candidate's tree pushed to an unprotected branch, named as `--line`,
/// proves nothing: that branch can be deleted or force-pushed later.
#[test]
fn a_branch_that_is_not_a_line_proves_nothing() {
    let fixture = fixture(None);
    let root = fixture.root();
    candidate(root, false);
    git(
        root,
        &[
            "push",
            "-q",
            "origin",
            &format!("{CANDIDATE}:task/TSK-002-copy"),
        ],
    );
    refused(
        root,
        CANDIDATE,
        "task/TSK-002-copy",
        &no_pull_request,
        "`task/TSK-002-copy` is not a line",
    );
}

/// AC-5: the naming and the depth are policy settings with defaults read
/// at the policy authority. The default depth finds a tree 3 commits
/// deep; an override of 2 does not, and an override prefix changes which
/// names are candidates.
#[test]
fn naming_and_depth_are_settings_with_defaults() {
    let fixture_default = fixture(None);
    let root = fixture_default.root();
    candidate(root, false);
    land(root, 2);
    assert!(prove(root, CANDIDATE, LINE, &no_pull_request).is_ok());

    let policy = r#"{"schema_version": 1, "git": {"candidate_prefix": "land/", "candidate_search_depth": 2}}"#;
    let fixture_override = fixture(Some(policy));
    let root = fixture_override.root();
    candidate(root, false);
    land(root, 2);
    let error = prove(root, CANDIDATE, LINE, &no_pull_request).unwrap_err();
    assert!(error.contains("does not start with `land/`"), "{error}");
    git(root, &["branch", "land/c1", CANDIDATE]);
    let error = prove(root, "land/c1", LINE, &no_pull_request).unwrap_err();
    assert!(
        error.contains("deeper than the 2 commits git.candidate_search_depth searches"),
        "{error}"
    );
}
