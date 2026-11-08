//! TSK-1390 (issue 633): the built git-guard judges every Git command that
//! writes work-tree files without moving HEAD by the content it would put on
//! an enforcement path, in the session's worktree and in another one named
//! with `git -C`. Every Git mutation below is fixture setup in a newly
//! created temporary repository; the commands under test are judged only.
use super::{
    expansion_codeflow, git, guard_payload, init_repo, run_with_stdin, shell_path,
    write_agent_policy,
};
use std::path::{Path, PathBuf};
use std::process::Output;

/// The project's own rules relaxed where AC-4 says they must not matter:
/// discarding uncommitted work and secret reads are off.
const RELAXED_POLICY: &str = r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off","discard_uncommitted":"off"},"security":{"secret_reads":"off"}}"#;

const POLICY: &str = ".codeflow/policy.json";

/// A repository on `main` with two linked worktrees, `a` (the session) and
/// `b` (the other), and a branch `stale` whose policy and `src/a.txt`
/// differ from `main`'s.
struct Journey {
    temp: tempfile::TempDir,
    a: PathBuf,
    b: PathBuf,
}

impl Journey {
    fn new() -> Self {
        let parent = codeflow_core::portable_path::canonicalize(&std::env::temp_dir()).unwrap();
        let temp = tempfile::tempdir_in(parent).unwrap();
        let base = codeflow_core::portable_path::canonicalize(temp.path()).unwrap();
        let root = base.join("repo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        init_repo(&root, "main");
        std::fs::write(root.join("src/a.txt"), "current\n").unwrap();
        git(&root, &["add", "src/a.txt"]);
        git(&root, &["commit", "-q", "-m", "chore: fixture source"]);
        write_agent_policy(&root, RELAXED_POLICY);
        git(&root, &["switch", "-q", "-c", "stale"]);
        std::fs::write(root.join(POLICY), r#"{"git":{"hook_integrity":"off"}}"#).unwrap();
        std::fs::write(root.join("src/a.txt"), "older\n").unwrap();
        git(
            &root,
            &["commit", "-q", "-am", "chore: fixture stale policy"],
        );
        git(&root, &["switch", "-q", "main"]);
        let a = root.join(".worktrees/a");
        let b = root.join(".worktrees/b");
        git(
            &root,
            &["worktree", "add", "-q", "-b", "task/a", &shell_path(&a)],
        );
        git(
            &root,
            &["worktree", "add", "-q", "-b", "task/b", &shell_path(&b)],
        );
        Self { temp, a, b }
    }

    fn guard(&self, command: &str) -> Output {
        let mut cmd = expansion_codeflow();
        cmd.args(["hook", "git-guard", "--contract", "3"])
            .current_dir(&self.a)
            .env("HOME", self.temp.path())
            .env("USERPROFILE", self.temp.path())
            .env("SHELL", "/bin/zsh");
        run_with_stdin(&mut cmd, &guard_payload(command, &self.a))
    }

    /// Every command in `refused` refuses under `git.hook_integrity`; every
    /// one in `allowed` passes. The failure names each wrong verdict.
    fn assert(&self, refused: &[String], allowed: &[String]) {
        let mut wrong = Vec::new();
        for command in refused {
            let out = self.guard(command);
            let err = String::from_utf8_lossy(&out.stderr);
            if out.status.code() != Some(2) || !err.contains("git.hook_integrity") {
                wrong.push(format!("should refuse: {command}: {err}"));
            }
        }
        for command in allowed {
            let out = self.guard(command);
            if out.status.code() != Some(0) {
                let err = String::from_utf8_lossy(&out.stderr);
                wrong.push(format!("should allow: {command}: {err}"));
            }
        }
        assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
    }

    /// `commands` aimed at worktree `b` with `git -C`, and at `a` itself.
    fn both(&self, commands: &[&str]) -> Vec<String> {
        let b = shell_path(&self.b);
        commands
            .iter()
            .flat_map(|command| {
                let rest = command.strip_prefix("git ").expect("a git command");
                [format!("git -C '{b}' {rest}"), (*command).to_string()]
            })
            .collect()
    }
}

/// AC-1, AC-2, AC-4 and AC-9: issue 633's reported command and every path
/// writer, aimed at the other worktree and at the session's own, refuse;
/// HEAD's content and ordinary files from an older revision pass.
#[test]
fn tsk1390_path_writers_refuse_other_content_on_enforcement_paths() {
    let j = Journey::new();
    let root = j.a.parent().unwrap().parent().unwrap().to_path_buf();
    // Patches and a mailbox that change the policy, in each worktree.
    let patch = git_output(&root, &["diff", "main", "stale", "--", POLICY]);
    let mail = git_output(&root, &["format-patch", "--stdout", "main..stale"]);
    for dir in [&j.a, &j.b] {
        std::fs::write(dir.join("policy.patch"), &patch).unwrap();
        std::fs::write(dir.join("policy.mbox"), &mail).unwrap();
    }
    let refused = j.both(&[
        // Issue 633 as reported, and the forms main still allowed.
        "git checkout stale -- .codeflow/policy.json",
        "git checkout stale -- .",
        "git checkout stale .",
        "git restore --source=stale .",
        "git restore --source=stale -- .codeflow",
        "git checkout stale -- '*policy.json'",
        "git checkout stale -- '*.json'",
        "git restore --staged --work --source=stale -- .codeflow/policy.json",
        // Patch mode with discard_uncommitted off, and pathspec magic with
        // secret_reads off (AC-4).
        "git checkout -p stale",
        "git restore -p --source=stale",
        "git checkout stale -- ':(top)'",
        "git checkout stale -- ':/'",
        // The other path writers.
        "git read-tree -u --reset stale",
        "git cherry-pick -n stale",
        // HEAD's commit added the policy, so reverting it removes it.
        "git revert -n HEAD",
        "git apply policy.patch",
        "git apply --index policy.patch",
        "git am policy.mbox",
        "git reset --hard stale",
        // Fail closed.
        "git apply",
        "git -c core.worktree=x checkout -- src",
    ]);
    let allowed = j.both(&[
        "git checkout stale -- src/a.txt",
        "git restore --source=stale src",
        "git checkout -- .",
        "git restore .",
        "git checkout-index -a -f",
        "git checkout main -- .",
        "git apply --cached policy.patch",
        "git apply --check policy.patch",
        "git cherry-pick stale",
        // Reverting `stale` writes its parent's policy, which is HEAD's.
        "git revert -n stale",
    ]);
    j.assert(&refused, &allowed);
}

/// AC-5: an index-only plant passes, and the work-tree write that would
/// follow it refuses, in either worktree.
#[test]
fn tsk1390_a_two_step_plant_refuses_at_the_work_tree_write() {
    let j = Journey::new();
    for dir in [&j.b, &j.a] {
        git(
            dir,
            &["restore", "--staged", "--source=stale", "--", POLICY],
        );
    }
    let refused = j.both(&[
        "git checkout -- .",
        "git restore .",
        "git checkout-index -a -f",
    ]);
    j.assert(
        &refused,
        &j.both(&["git checkout -- src", "git restore --staged ."]),
    );
}

/// AC-1: a stash that changes the policy refuses at apply, pop and branch;
/// stashes live in the common directory, so either worktree can apply it.
#[test]
fn tsk1390_a_stash_that_changes_the_policy_refuses() {
    let j = Journey::new();
    std::fs::write(j.b.join(POLICY), r#"{"git":{"hook_integrity":"off"}}"#).unwrap();
    git(&j.b, &["stash", "-q"]);
    let refused = j.both(&["git stash apply", "git stash pop", "git stash branch fresh"]);
    j.assert(&refused, &[]);
    std::fs::write(j.b.join("src/a.txt"), "ordinary\n").unwrap();
    git(&j.b, &["stash", "-q"]);
    j.assert(
        &j.both(&["git stash apply 1"]),
        &j.both(&["git stash apply"]),
    );
}

fn git_output(dir: &Path, args: &[&str]) -> String {
    let out = super::git_command()
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}
