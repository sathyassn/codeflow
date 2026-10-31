//! TSK-330: the hooks stay usable while `.codeflow/project.toml` or
//! `.codeflow/policy.json` has an unresolved merge conflict, and no guard is
//! weaker than with parsed settings.
//!
//! Each scenario makes a real conflict in a scratch repository and runs the
//! guards on hook payloads. A verdict during the conflict is compared with
//! the verdict on the same repository once the working copy of the
//! conflicted file holds its `HEAD` copy, which parses while the index stays
//! unmerged: the guard sees parsed settings and everything else is equal.
//! `TSK330_TEST_BINARY` runs the scenarios with another binary, such as an
//! installed release, for the fail-before record.
use serde_json::json;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn binary() -> PathBuf {
    std::env::var_os("TSK330_TEST_BINARY").map_or_else(
        || PathBuf::from(env!("CARGO_BIN_EXE_codeflow")),
        PathBuf::from,
    )
}

const PROJECT: &str = ".codeflow/project.toml";
const POLICY: &str = ".codeflow/policy.json";
/// The authority's policy: protected `main`, blocked pushes and commits to
/// it; the root-checkout rule is judged in its own tests.
const STRICT: &str = r#"{"git":{"push_to_protected":"block","commit_to_protected":"block","root_checkout_commits":"off"}}"#;
/// A weaker policy committed on the task branch only.
const WEAK: &str = r#"{"git":{"push_to_protected":"off","commit_to_protected":"off","force_push_protected":"off","root_checkout_commits":"off","root_branch":"task/TSK-001-fix"}}"#;
const SETTINGS: &str = "tier = \"standard\"\npolicy_armed = true\n";

struct Repo {
    temp: tempfile::TempDir,
}

impl Repo {
    /// `main` holding the strict policy and settings, with `origin` holding
    /// it and remote `HEAD` naming it, unless `remote` is false.
    fn new(remote: bool) -> Self {
        let repo = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir(repo.work()).unwrap();
        repo.git(&["init", "-q", "-b", "main"]);
        repo.commit(
            &[(POLICY, STRICT), (PROJECT, SETTINGS), ("README.md", "x\n")],
            "chore: adopt codeflow",
        );
        if remote {
            let bare = repo.temp.path().join("remote.git");
            repo.git(&["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()]);
            repo.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
            repo.git(&["push", "-q", "origin", "main"]);
            repo.git(&["fetch", "-q", "origin"]);
            repo.git(&[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ]);
        }
        repo
    }

    fn work(&self) -> PathBuf {
        self.temp.path().join("work")
    }

    fn command(&self, program: &Path) -> Command {
        let mut c = Command::new(program);
        let home = self.temp.path();
        c.current_dir(self.work())
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("CODEFLOW_HOME", home.join("state"))
            .env("GIT_CONFIG_GLOBAL", home.join("global.config"))
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("CODEFLOW_INTEGRATE_TOKEN")
            .env_remove("CODEFLOW_HUMAN_OVERRIDE");
        c
    }

    fn git_output(&self, args: &[&str]) -> Output {
        self.command(Path::new("git")).args(args).output().unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        let out = self.git_output(args);
        assert!(out.status.success(), "git {args:?}: {}", said(&out));
        String::from_utf8_lossy(&out.stdout).trim().into()
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.work().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.work().join(path)).unwrap()
    }

    fn commit(&self, files: &[(&str, &str)], message: &str) {
        for (path, text) in files {
            self.write(path, text);
            self.git(&["add", path]);
        }
        self.git(&["commit", "-q", "-m", message]);
    }

    /// Leave each of `files` unmerged: a side branch and the current branch
    /// change it differently, then the side branch is merged. `ours` is
    /// committed on the current branch first, when given, so `HEAD` holds
    /// it.
    fn conflict(&self, files: &[(&str, &str, &str)]) {
        let branch = self.git(&["branch", "--show-current"]);
        self.git(&["checkout", "-q", "-b", "side"]);
        let theirs: Vec<_> = files.iter().map(|(p, _, t)| (*p, *t)).collect();
        self.commit(&theirs, "chore: side settings");
        self.git(&["checkout", "-q", &branch]);
        let ours: Vec<_> = files.iter().map(|(p, o, _)| (*p, *o)).collect();
        self.commit(&ours, "chore: our settings");
        let out = self.git_output(&["merge", "-q", "side"]);
        assert!(!out.status.success(), "the merge conflicts: {}", said(&out));
        for (path, _, _) in files {
            assert!(
                self.git(&["ls-files", "-u", "--", path]).lines().count() >= 2,
                "{path} is unmerged"
            );
            assert!(self.read(path).contains("<<<<<<<"), "{path} has markers");
        }
    }

    /// Run `codeflow <args>` with `stdin`.
    fn codeflow(&self, args: &[&str], stdin: &str) -> Output {
        let mut child = self
            .command(&binary())
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn bash(&self, guard: &str, command: &str) -> Output {
        let payload =
            json!({"tool_name":"Bash","cwd":self.work(),"tool_input":{"command":command}});
        self.codeflow(&["hook", guard], &payload.to_string())
    }

    fn edit(&self, path: &str) -> Output {
        let payload = json!({"tool_name":"Edit","cwd":self.work(),"tool_input":{
            "file_path": self.work().join(path), "old_string":"x", "new_string":"y"}});
        self.codeflow(&["hook", "edit-guard"], &payload.to_string())
    }

    /// Every guard's verdict on the commands that resolve or abandon a
    /// conflict in `file`, the planted ones, and an unrelated edit.
    fn verdicts(&self, file: &str) -> Verdicts {
        let mut out = Vec::new();
        for command in [
            "git status".to_string(),
            format!("git diff -- {file}"),
            format!("git checkout --ours -- {file}"),
            format!("git checkout --theirs -- {file}"),
            format!("git add -- {file}"),
            "git commit -q --no-edit".to_string(),
            "git merge --abort".to_string(),
            "git rebase --abort".to_string(),
            "ls".to_string(),
            "git push origin HEAD:main".to_string(),
            "git commit -q -m 'fix: planted'".to_string(),
            "echo '{}' > .codeflow/policy.json".to_string(),
        ] {
            for guard in ["git-guard", "exec-guard"] {
                out.push((
                    format!("{guard}: {command}"),
                    Verdict::of(&self.bash(guard, &command)),
                ));
            }
        }
        out.push((
            "edit-guard: README.md".into(),
            Verdict::of(&self.edit("README.md")),
        ));
        out
    }

    /// The verdicts during the conflict, then with each file's working copy
    /// replaced by its `HEAD` copy (parsed settings, index still unmerged),
    /// after which the conflicted text is put back.
    fn compare(&self, files: &[&str]) -> (Verdicts, Verdicts, String) {
        let conflicted = self.verdicts(files[0]);
        let notes = self.bash("git-guard", "git status");
        let saved: Vec<_> = files.iter().map(|f| (*f, self.read(f))).collect();
        for file in files {
            let head = self.git(&["show", &format!("HEAD:{file}")]);
            self.write(file, &format!("{head}\n"));
        }
        let parsed = self.verdicts(files[0]);
        for (file, text) in saved {
            self.write(file, &text);
        }
        (conflicted, parsed, said(&notes))
    }
}

/// Each command with its verdict, in the order they ran.
type Verdicts = Vec<(String, Verdict)>;

#[derive(Debug, PartialEq, Eq)]
struct Verdict {
    code: Option<i32>,
    rules: Vec<String>,
}

impl Verdict {
    fn of(out: &Output) -> Self {
        let text = said(out);
        let mut rules: Vec<String> = text
            .lines()
            .filter_map(|l| l.split("policy rule ").nth(1))
            .map(|r| r.split_whitespace().next().unwrap_or("").to_string())
            .collect();
        rules.sort();
        rules.dedup();
        Self {
            code: out.status.code(),
            rules,
        }
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn verdict<'a>(list: &'a [(String, Verdict)], key: &str) -> &'a Verdict {
    &list.iter().find(|(k, _)| k == key).unwrap().1
}

/// The verdicts match with parsed settings, every resolving command and the
/// unrelated edit pass, and no guard reports the parse error.
fn assert_usable(conflicted: &[(String, Verdict)], parsed: &[(String, Verdict)], file: &str) {
    assert_eq!(conflicted, parsed, "verdicts differ from parsed settings");
    for key in [
        "git-guard: git status".to_string(),
        format!("git-guard: git diff -- {file}"),
        format!("git-guard: git add -- {file}"),
        "git-guard: git merge --abort".to_string(),
        "exec-guard: ls".to_string(),
        "edit-guard: README.md".to_string(),
    ] {
        assert_eq!(verdict(conflicted, &key).code, Some(0), "{key} passes");
    }
}

/// The planted commands keep the strict authority's refusals.
fn assert_planted_refused(conflicted: &[(String, Verdict)]) {
    let push = verdict(conflicted, "git-guard: git push origin HEAD:main");
    assert_eq!(push.code, Some(2), "{push:?}");
    assert!(
        push.rules.contains(&"git.push_to_protected".into()),
        "{push:?}"
    );
    let write = verdict(conflicted, "git-guard: echo '{}' > .codeflow/policy.json");
    assert_eq!(write.code, Some(2), "{write:?}");
    assert!(
        write.rules.contains(&"git.hook_integrity".into()),
        "{write:?}"
    );
}

fn notes_for(text: &str, file: &str) -> usize {
    text.matches(&format!("note: {file} has an unresolved merge conflict"))
        .count()
}

/// AC-1, AC-2, AC-3: `project.toml` unmerged on a task branch whose `HEAD`
/// disarms the policy, while `origin/main` stays strict.
#[test]
fn an_unmerged_project_toml_keeps_every_guard_usable_and_strict() {
    let repo = Repo::new(true);
    repo.git(&["checkout", "-q", "-b", "task/TSK-001-fix"]);
    repo.conflict(&[(
        PROJECT,
        "tier = \"minimal\"\npolicy_armed = false\n",
        "tier = \"full\"\npolicy_armed = true\n",
    )]);
    let (conflicted, parsed, notes) = repo.compare(&[PROJECT]);
    assert_usable(&conflicted, &parsed, PROJECT);
    assert_planted_refused(&conflicted);
    assert_eq!(notes_for(&notes, PROJECT), 1, "{notes}");
    assert!(!notes.contains("cannot parse"), "{notes}");
}

/// AC-1, AC-3: `policy.json` unmerged, with a lower push level, commit
/// level and another root branch committed at `HEAD`; the verdicts follow
/// the stricter `origin/main`.
#[test]
fn an_unmerged_policy_json_with_a_weaker_head_follows_the_authority() {
    let repo = Repo::new(true);
    repo.git(&["checkout", "-q", "-b", "task/TSK-001-fix"]);
    repo.conflict(&[(POLICY, WEAK, r#"{"git":{"push_to_protected":"warn"}}"#)]);
    let (conflicted, parsed, notes) = repo.compare(&[POLICY]);
    assert_usable(&conflicted, &parsed, POLICY);
    assert_planted_refused(&conflicted);
    assert_eq!(notes_for(&notes, POLICY), 1, "{notes}");
}

/// AC-3: both settings files unmerged at once; one note for each.
#[test]
fn both_settings_files_unmerged_at_once() {
    let repo = Repo::new(true);
    repo.git(&["checkout", "-q", "-b", "task/TSK-001-fix"]);
    repo.conflict(&[
        (POLICY, WEAK, r#"{"git":{"push_to_protected":"warn"}}"#),
        (
            PROJECT,
            "tier = \"minimal\"\npolicy_armed = false\n",
            "tier = \"full\"\n",
        ),
    ]);
    let (conflicted, parsed, notes) = repo.compare(&[POLICY, PROJECT]);
    assert_usable(&conflicted, &parsed, POLICY);
    assert_planted_refused(&conflicted);
    // The guard reads the local project settings only when the local policy
    // matches the authority, so its note may not be printed; never twice.
    assert_eq!(notes_for(&notes, POLICY), 1, "{notes}");
    assert!(notes_for(&notes, PROJECT) <= 1, "{notes}");
    // The git hooks read both files, one note each.
    let head = repo.git(&["rev-parse", "HEAD"]);
    let hooks = said(&repo.codeflow(
        &["git-hook", "reference-transaction", "prepared"],
        &format!("{head} {head} refs/heads/task/TSK-001-fix\n"),
    ));
    assert_eq!(notes_for(&hooks, POLICY), 1, "{hooks}");
    assert_eq!(notes_for(&hooks, PROJECT), 1, "{hooks}");
}

/// AC-3: a conflict on the protected branch itself keeps the commit and
/// push refusals, with and without a remote (`HEAD` is the authority
/// without one).
#[test]
fn a_conflict_on_a_protected_branch_keeps_its_refusals() {
    for remote in [true, false] {
        let repo = Repo::new(remote);
        repo.conflict(&[(
            PROJECT,
            "tier = \"minimal\"\npolicy_armed = true\n",
            "tier = \"full\"\npolicy_armed = true\n",
        )]);
        let (conflicted, parsed, _) = repo.compare(&[PROJECT]);
        assert_eq!(conflicted, parsed, "remote={remote}");
        let commit = verdict(&conflicted, "git-guard: git commit -q -m 'fix: planted'");
        assert_eq!(commit.code, Some(2), "remote={remote} {commit:?}");
        assert!(
            commit.rules.contains(&"git.commit_to_protected".into()),
            "{commit:?}"
        );
        assert_eq!(
            verdict(&conflicted, "git-guard: git status").code,
            Some(0),
            "remote={remote}"
        );
    }
}

/// AC-4: an unreadable settings file with no unmerged entry refuses, naming
/// the file, conflict marker text included.
#[test]
fn a_parse_error_without_an_unmerged_entry_still_refuses() {
    for (file, text, named) in [
        (
            PROJECT,
            "tier = = 1\n",
            "cannot parse project settings .codeflow/project.toml",
        ),
        (POLICY, "{not json\n", "cannot parse policy"),
        (
            PROJECT,
            "<<<<<<< HEAD\ntier = \"minimal\"\n=======\ntier = \"full\"\n>>>>>>> side\n",
            "cannot parse project settings .codeflow/project.toml",
        ),
        (
            POLICY,
            "<<<<<<< HEAD\n{}\n=======\n{\"git\":{}}\n>>>>>>> side\n",
            "cannot parse policy",
        ),
    ] {
        let repo = Repo::new(true);
        repo.git(&["checkout", "-q", "-b", "task/TSK-001-fix"]);
        repo.write(file, text);
        for out in [
            repo.bash("git-guard", "git status"),
            repo.bash("exec-guard", "ls"),
            repo.edit("README.md"),
        ] {
            let text = said(&out);
            assert_eq!(out.status.code(), Some(2), "{file}: {text}");
            assert!(text.contains(named), "{file}: {text}");
            assert!(text.contains(".codeflow/"), "{file}: {text}");
            assert!(
                !text.contains("unresolved merge conflict"),
                "{file}: {text}"
            );
        }
    }
}

/// Stage unmerged entries for `file` by hand: base, ours and theirs.
fn stage_unmerged(repo: &Repo, file: &str, sides: [&str; 3]) {
    let mut info = String::new();
    for (stage, text) in sides.iter().enumerate() {
        repo.write("blob.tmp", text);
        let id = repo.git(&["hash-object", "-w", "blob.tmp"]);
        info.push_str(&format!("100644 {id} {}\t{file}\n", stage + 1));
    }
    std::fs::remove_file(repo.work().join("blob.tmp")).unwrap();
    repo.git(&["update-index", "--force-remove", "--", file]);
    let mut child = repo
        .command(Path::new("git"))
        .args(["update-index", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(info.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    repo.write(
        file,
        "<<<<<<< ours\nx = 1\n=======\nx = 2\n>>>>>>> theirs\n",
    );
}

/// AC-4, AC-5: no copy to stand in. An unborn `HEAD`, a `HEAD` with no copy
/// of the file, a `HEAD` copy that does not parse: each refuses, naming the
/// conflict and the operator's recovery, never reading defaults.
#[test]
fn an_unmerged_file_without_a_usable_head_copy_refuses() {
    let unborn = Repo {
        temp: tempfile::tempdir().unwrap(),
    };
    std::fs::create_dir(unborn.work()).unwrap();
    unborn.git(&["init", "-q", "-b", "main"]);
    unborn.write(POLICY, STRICT);
    stage_unmerged(&unborn, PROJECT, ["a = 1\n", "a = 2\n", "a = 3\n"]);

    let absent = Repo::new(true);
    absent.git(&["checkout", "-q", "-b", "task/TSK-001-fix"]);
    absent.git(&["rm", "-q", "--", PROJECT]);
    absent.git(&["commit", "-q", "-m", "chore: drop settings"]);
    stage_unmerged(&absent, PROJECT, ["a = 1\n", "a = 2\n", "a = 3\n"]);

    let broken = Repo::new(true);
    broken.git(&["checkout", "-q", "-b", "task/TSK-001-fix"]);
    broken.conflict(&[(PROJECT, "tier = = 1\n", "tier = \"full\"\n")]);

    for (repo, why) in [
        (&unborn, "HEAD has no commit"),
        (&absent, "HEAD holds no copy"),
        (&broken, "its copy at HEAD does not parse"),
    ] {
        for out in [
            repo.bash("git-guard", "git status"),
            repo.bash("git-guard", "git merge --abort"),
            repo.bash("exec-guard", "ls"),
            repo.edit("README.md"),
        ] {
            let text = said(&out);
            assert_eq!(out.status.code(), Some(2), "{why}: {text}");
            assert!(text.contains("unresolved merge conflict"), "{why}: {text}");
            assert!(text.contains(why), "{why}: {text}");
            assert!(text.contains("git merge --abort"), "{why}: {text}");
            assert!(text.contains("outside the session"), "{why}: {text}");
        }
    }
}

/// AC-5: an unreadable authority still refuses during a conflict, naming
/// the source and `git fetch`.
#[test]
fn an_unreadable_authority_refuses_during_a_conflict() {
    let repo = Repo::new(true);
    repo.git(&["checkout", "-q", "-b", "task/TSK-001-fix"]);
    repo.conflict(&[(PROJECT, "tier = \"minimal\"\n", "tier = \"full\"\n")]);
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/gone",
    ]);
    for out in [
        repo.bash("git-guard", "git status"),
        repo.bash("exec-guard", "ls"),
        repo.edit("README.md"),
    ] {
        let text = said(&out);
        assert_eq!(out.status.code(), Some(2), "{text}");
        assert!(text.contains("refs/remotes/origin/"), "{text}");
        assert!(text.contains("git fetch"), "{text}");
    }
}

/// AC-4: the bootstrap control. Parseable settings with `policy_armed =
/// false` before the first commit still let the first commit through.
#[test]
fn the_bootstrap_window_still_lets_the_first_commit_through() {
    let repo = Repo {
        temp: tempfile::tempdir().unwrap(),
    };
    std::fs::create_dir(repo.work()).unwrap();
    repo.git(&["init", "-q", "-b", "main"]);
    repo.write(POLICY, STRICT);
    repo.write(PROJECT, "tier = \"standard\"\npolicy_armed = false\n");
    let out = repo.bash("git-guard", "git commit -q -m 'chore: scaffold'");
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let out = repo.codeflow(&["git-hook", "pre-commit"], "");
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
}

/// The scaffolded git hooks during a conflict: `reference-transaction` on
/// `git merge --abort`, and pre-push with the same verdict as with parsed
/// settings (the git hooks read their levels from the working copy).
#[test]
fn the_git_hooks_read_head_for_an_unmerged_settings_file() {
    let repo = Repo::new(true);
    repo.git(&["checkout", "-q", "-b", "task/TSK-001-fix"]);
    repo.conflict(&[(
        POLICY,
        STRICT
            .replace("block\",\"commit", "warn\",\"commit")
            .as_str(),
        WEAK,
    )]);
    let head = repo.git(&["rev-parse", "HEAD"]);
    let push = format!(
        "refs/heads/task/TSK-001-fix {head} refs/heads/main {}\n",
        "0".repeat(40)
    );
    let transaction = format!("{head} {head} refs/heads/task/TSK-001-fix\n");
    let url = repo.git(&["remote", "get-url", "origin"]);
    let run = |repo: &Repo| {
        (
            Verdict::of(&repo.codeflow(&["git-hook", "pre-push", "origin", &url], &push)),
            Verdict::of(&repo.codeflow(
                &["git-hook", "reference-transaction", "prepared"],
                &transaction,
            )),
        )
    };
    let conflicted = run(&repo);
    let head_copy = repo.git(&["show", &format!("HEAD:{POLICY}")]);
    repo.write(POLICY, &format!("{head_copy}\n"));
    let parsed = run(&repo);
    assert_eq!(conflicted, parsed);
    assert_eq!(conflicted.1.code, Some(0), "{conflicted:?}");
}

/// AC-6 (journey): a scratch adopter made by `codeflow init --standard`,
/// with its git hooks wired to the binary under test, resolves a conflict
/// in `project.toml` through hook payloads and abandons a second one that
/// would also lower the push level, while a planted protected push is
/// refused. Run with `--nocapture` for the journey output.
#[test]
#[allow(clippy::too_many_lines)] // One journey keeps both conflicts in the order an adopter meets them.
fn journey_an_adopter_resolves_and_abandons_a_settings_conflict() {
    let repo = Repo {
        temp: tempfile::tempdir().unwrap(),
    };
    std::fs::create_dir(repo.work()).unwrap();
    let hooked = |program: &str, dir: &Path, args: &[&str]| {
        let mut c = repo.command(Path::new(program));
        c.current_dir(dir)
            .env("CODEFLOW_HOOK_BINARY", binary())
            .env_remove("CLAUDECODE")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CODEX_CI")
            .env_remove("GROK_SESSION_ID")
            .args(args);
        c.output().unwrap()
    };
    let init = hooked(
        binary().to_str().unwrap(),
        &repo.work(),
        &["init", "--yes", "--standard"],
    );
    assert!(init.status.success(), "{}", said(&init));
    println!("init --standard: exit {:?}", init.status.code());
    let main = repo.git(&["branch", "--show-current"]);
    let bare = repo.temp.path().join("remote.git");
    repo.git(&["init", "-q", "--bare", "-b", &main, bare.to_str().unwrap()]);
    let seed = hooked(
        "git",
        &bare,
        &[
            "fetch",
            "-q",
            repo.work().to_str().unwrap(),
            &format!("{main}:{main}"),
        ],
    );
    assert!(seed.status.success(), "{}", said(&seed));
    repo.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    let fetch = hooked("git", &repo.work(), &["fetch", "-q", "origin"]);
    assert!(fetch.status.success(), "{}", said(&fetch));
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        &format!("refs/remotes/origin/{main}"),
    ]);
    let task = repo.work().join(".worktrees/t");
    let add = hooked(
        "git",
        &repo.work(),
        &[
            "worktree",
            "add",
            "-q",
            task.to_str().unwrap(),
            "-b",
            "task/TSK-001-fix",
        ],
    );
    assert!(add.status.success(), "{}", said(&add));

    let git = |args: &[&str]| {
        let out = hooked("git", &task, args);
        println!("  git {}: exit {:?}", args.join(" "), out.status.code());
        out
    };
    let guard = |which: &str, command: &str| {
        let payload = json!({"tool_name":"Bash","cwd":task,"tool_input":{"command":command}});
        let mut child = repo
            .command(&binary())
            .current_dir(&task)
            .args(["hook", which])
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
        let out = child.wait_with_output().unwrap();
        println!("  {which} `{command}`: exit {:?}", out.status.code());
        out
    };
    let edit = |path: &str, from: &str, to: &str| {
        let file = task.join(path);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains(from), "{path} holds {from}");
        std::fs::write(&file, text.replacen(from, to, 1)).unwrap();
    };
    let commit = |message: &str| {
        let out = git(&["commit", "-q", "-am", message]);
        assert!(out.status.success(), "{}", said(&out));
    };
    let push_off = "\"push_to_protected\": \"off\"";
    let push_block = "\"push_to_protected\": \"block\"";
    let tier = "tier = \"standard\"";

    // The first conflict, in project.toml, resolved in the session.
    println!("conflict 1: {PROJECT}, resolved");
    assert!(git(&["switch", "-q", "-c", "side-1"]).status.success());
    edit(PROJECT, tier, "tier = \"full\"");
    commit("chore: side tier");
    assert!(git(&["switch", "-q", "task/TSK-001-fix"]).status.success());
    edit(PROJECT, tier, "tier = \"minimal\"");
    commit("chore: our tier");
    let merge = git(&["merge", "-q", "side-1"]);
    assert!(!merge.status.success());
    let unmerged = hooked("git", &task, &["ls-files", "-u", "--", PROJECT]);
    assert!(
        !String::from_utf8_lossy(&unmerged.stdout).trim().is_empty(),
        "the merge stops with {PROJECT} unmerged: {}",
        said(&merge)
    );
    let status = guard("git-guard", "git status");
    assert_eq!(status.status.code(), Some(0), "{}", said(&status));
    assert!(
        said(&status).contains("has an unresolved merge conflict"),
        "{}",
        said(&status)
    );
    assert!(git(&["status", "--short"]).status.success());
    let planted = guard("git-guard", "git push origin HEAD:main");
    assert_eq!(planted.status.code(), Some(2), "{}", said(&planted));
    // Writing a settings file stays refused, as it is with parsed settings
    // (`git.hook_integrity`), so the operator takes their side outside the
    // session; the guard is not asked about that step.
    let theirs = guard("git-guard", &format!("git checkout --theirs -- {PROJECT}"));
    assert_eq!(theirs.status.code(), Some(2), "{}", said(&theirs));
    assert!(
        said(&theirs).contains("git.hook_integrity"),
        "{}",
        said(&theirs)
    );
    let operator = git(&["checkout", "--theirs", "--", PROJECT]);
    assert!(operator.status.success(), "{}", said(&operator));
    println!("  (the operator ran that checkout outside the session)");
    for (command, args) in [
        (format!("git add -- {PROJECT}"), vec!["add", "--", PROJECT]),
        (
            "git commit -q --no-edit".to_string(),
            vec!["commit", "-q", "--no-edit"],
        ),
    ] {
        let verdict = guard("git-guard", &command);
        assert_eq!(
            verdict.status.code(),
            Some(0),
            "{command}: {}",
            said(&verdict)
        );
        let ran = git(&args);
        assert!(ran.status.success(), "{command}: {}", said(&ran));
    }
    assert!(std::fs::read_to_string(task.join(PROJECT))
        .unwrap()
        .contains("tier = \"full\""));

    // The second conflict also lowers the push level on its side; it is
    // abandoned in the session.
    println!("conflict 2: {PROJECT} and {POLICY}, the side lowers the push level, abandoned");
    let tier = "tier = \"full\"";
    assert!(git(&["switch", "-q", "-c", "side-2"]).status.success());
    edit(PROJECT, tier, "tier = \"standard\"");
    edit(POLICY, push_block, push_off);
    commit("chore: side lowers the push level");
    assert!(git(&["switch", "-q", "task/TSK-001-fix"]).status.success());
    edit(PROJECT, tier, "tier = \"minimal\"");
    edit(POLICY, push_block, "\"push_to_protected\": \"warn\"");
    commit("chore: our push level");
    assert!(!git(&["merge", "-q", "side-2"]).status.success());
    let planted = guard("git-guard", "git push origin HEAD:main");
    assert_eq!(planted.status.code(), Some(2), "{}", said(&planted));
    assert!(
        said(&planted).contains("git.push_to_protected"),
        "{}",
        said(&planted)
    );
    let exec = guard("exec-guard", "ls");
    assert_eq!(exec.status.code(), Some(0), "{}", said(&exec));
    let abort = guard("git-guard", "git merge --abort");
    assert_eq!(abort.status.code(), Some(0), "{}", said(&abort));
    let ran = git(&["merge", "--abort"]);
    assert!(ran.status.success(), "{}", said(&ran));
    let clean = hooked("git", &task, &["status", "--porcelain"]);
    assert!(String::from_utf8_lossy(&clean.stdout).trim().is_empty());
}

/// A merge that stops on a conflict writes `AUTO_MERGE` before its index
/// records the conflict. That transaction passes the reference-transaction
/// hook with the markers already in the settings file; a branch update in
/// the same state still refuses, since nothing records a conflict yet.
#[test]
fn the_merge_state_ref_passes_before_the_index_records_the_conflict() {
    let repo = Repo::new(true);
    repo.git(&["checkout", "-q", "-b", "task/TSK-001-fix"]);
    repo.write(
        PROJECT,
        "<<<<<<< HEAD\ntier = \"minimal\"\n=======\ntier = \"full\"\n>>>>>>> side\n",
    );
    let head = repo.git(&["rev-parse", "HEAD"]);
    let zero = "0".repeat(40);
    let state = repo.codeflow(
        &["git-hook", "reference-transaction", "prepared"],
        &format!("{zero} {head} AUTO_MERGE\n"),
    );
    assert_eq!(state.status.code(), Some(0), "{}", said(&state));
    let branch = repo.codeflow(
        &["git-hook", "reference-transaction", "prepared"],
        &format!("{head} {head} refs/heads/task/TSK-001-fix\n"),
    );
    assert_eq!(branch.status.code(), Some(2), "{}", said(&branch));
    assert!(
        said(&branch).contains("cannot parse project settings"),
        "{}",
        said(&branch)
    );
}
