//! Unit tests for the path-writer judgment (TSK-1390). Every mutation below
//! is fixture setup in a newly created temporary repository; the commands
//! under test are judged only, never run.
use super::*;

/// What the guard decides for one command.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// Refused: it writes other content on this enforcement path.
    Writes(String),
    /// Refused: the guard cannot tell.
    Unknown,
    /// Allowed.
    Passes,
}

use Outcome::{Passes, Unknown};

fn writes(path: &str) -> Outcome {
    Outcome::Writes(path.to_string())
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = crate::git::command()
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(dir: &Path, path: &str, text: &str) {
    let file = dir.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

/// Words of a test command: whitespace-separated, with single quotes.
fn words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in command.chars() {
        match c {
            '\'' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

/// Judge `command` (`git` after any assignments) as run in `dir`.
fn judged(dir: &Path, command: &str) -> Outcome {
    let all = words(command);
    let at = all
        .iter()
        .position(|word| word == "git")
        .expect("a git command");
    let filled = &all[at + 1..];
    let (verb, rest) = super::super::git_subcommand(filled).expect("a git subcommand");
    if !candidate(verb, rest) {
        return Passes;
    }
    match judge_line(verb, filled, rest, (dir, dir), command) {
        Ok(None) => Passes,
        Ok(Some(Verdict::Writes { path, .. })) => Outcome::Writes(path),
        Ok(Some(Verdict::Unknown(_))) | Err(_) => Unknown,
    }
}

/// Assert each `(command, outcome)` pair; report every mismatch at once.
fn expect<S: AsRef<str>>(dir: &Path, cases: &[(S, Outcome)]) {
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(command, want)| {
            let command = command.as_ref();
            let got = judged(dir, command);
            (got != *want).then(|| format!("{command}: want {want:?}, got {got:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

const POLICY: &str = ".codeflow/policy.json";

/// A repository whose `old` tag holds an older policy and `src/a.txt`,
/// whose `main` (HEAD) holds the current ones, and whose `same` branch
/// changes only `src/a.txt` and `evil` only the policy.
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = crate::portable_path::canonicalize(temp.path())
            .unwrap()
            .join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        write(&root, POLICY, "{\"v\":1}\n");
        write(&root, ".claude/settings.json", "{}\n");
        write(&root, "src/a.txt", "1\n");
        write(&root, "README.md", "readme\n");
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "one"]);
        git(&root, &["tag", "old"]);
        write(&root, POLICY, "{\"v\":2}\n");
        write(&root, "src/a.txt", "2\n");
        git(&root, &["commit", "-q", "-am", "two"]);
        git(&root, &["switch", "-q", "-c", "same"]);
        write(&root, "src/a.txt", "3\n");
        git(&root, &["commit", "-q", "-am", "same"]);
        git(&root, &["switch", "-q", "-c", "evil", "main"]);
        write(&root, POLICY, "{\"v\":3}\n");
        git(&root, &["commit", "-q", "-am", "evil"]);
        git(&root, &["switch", "-q", "main"]);
        Self { _temp: temp, root }
    }

    /// Put `text` at `path` in the index only, as an index plant does.
    fn plant_index(&self, path: &str, text: &str) {
        let blob_file = self.root.join("planted.blob");
        std::fs::write(&blob_file, text).unwrap();
        let blob = git(&self.root, &["hash-object", "-w", "planted.blob"]);
        std::fs::remove_file(blob_file).unwrap();
        git(
            &self.root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{blob},{path}"),
            ],
        );
    }

    fn reset_index(&self) {
        git(&self.root, &["reset", "-q"]);
    }
}

#[test]
fn checkout_of_paths_is_judged_by_the_content_it_writes() {
    let f = Fixture::new();
    expect(
        &f.root,
        &[
            // Issue 633's reported form and the whole-tree forms.
            ("git checkout old -- .codeflow/policy.json", writes(POLICY)),
            ("git checkout old -- .", writes(POLICY)),
            ("git checkout old .", writes(POLICY)),
            ("git checkout old -- .codeflow", writes(POLICY)),
            // Git pathspec wildcards cross `/` and match leading dots.
            ("git checkout old -- '*'", writes(POLICY)),
            ("git checkout old -- '*policy.json'", writes(POLICY)),
            ("git checkout old -- '*.json'", writes(POLICY)),
            ("git checkout old -- '.c?deflow/*'", writes(POLICY)),
            ("git checkout old -- '[.]codeflow'", writes(POLICY)),
            // Patch mode, magic and a pathspec file read the whole tree.
            ("git checkout -p old", writes(POLICY)),
            ("git checkout -p old -- src", Passes),
            ("git checkout old -- ':(top)src'", writes(POLICY)),
            ("git checkout old -- ':/'", writes(POLICY)),
            ("git checkout old --pathspec-from-file=list", writes(POLICY)),
            ("git checkout --end-of-options old -- .", writes(POLICY)),
            ("git checkout --end-of-options old .", writes(POLICY)),
            // Allowed: ordinary files from any revision, and HEAD's content.
            ("git checkout old -- src", Passes),
            ("git checkout old -- src/a.txt", Passes),
            ("git checkout old -- '*.txt'", Passes),
            ("git checkout same -- .", Passes),
            ("git checkout HEAD -- .", Passes),
            ("git checkout -- .", Passes),
            ("git checkout .", Passes),
            ("git checkout -p", Passes),
            // Branch switches keep their verdict (TSK-325 AC-7).
            ("git checkout old", Passes),
            ("git checkout evil", Passes),
            ("git checkout -b fresh old", Passes),
            ("git checkout --detach old", Passes),
            ("git checkout old --", Passes),
            // An option the guard does not read refuses.
            ("git checkout --frobnicate old -- src", Unknown),
        ],
    );
}

#[test]
fn restore_reads_its_source_and_abbreviations_as_git_does() {
    let f = Fixture::new();
    expect(
        &f.root,
        &[
            ("git restore --source=old .", writes(POLICY)),
            (
                "git restore --source old -- .codeflow/policy.json",
                writes(POLICY),
            ),
            ("git restore -s old .codeflow", writes(POLICY)),
            ("git restore -sold .", writes(POLICY)),
            ("git restore --sour=old .", writes(POLICY)),
            ("git restore --so old .", writes(POLICY)),
            (
                "git restore --staged --work --source=old -- .codeflow/policy.json",
                writes(POLICY),
            ),
            ("git restore -SW --source=old .", writes(POLICY)),
            ("git restore -p --source=old", writes(POLICY)),
            ("git restore --source=old '*'", writes(POLICY)),
            // Allowed.
            ("git restore --source=old src", Passes),
            ("git restore .", Passes),
            ("git restore --staged --worktree .", Passes),
            ("git restore --source=same .", Passes),
            // The index alone: judged at the later work-tree write.
            ("git restore --staged --source=old .", Passes),
            ("git restore --sta --source=old .", Passes),
            // `--s` is ambiguous, which Git refuses; so does the guard.
            ("git restore --s old .", Unknown),
        ],
    );
}

#[test]
fn a_two_step_plant_is_refused_at_its_work_tree_step() {
    let f = Fixture::new();
    let steps = [
        ("git checkout -- .", writes(POLICY)),
        ("git restore .", writes(POLICY)),
        ("git checkout-index -a -f", writes(POLICY)),
        (
            "git checkout-index -f -- .codeflow/policy.json",
            writes(POLICY),
        ),
        // `-n` still overwrites files that exist (review PW-2).
        ("git checkout-index -n -f -a", writes(POLICY)),
        ("git checkout-index --no-create -f -a", writes(POLICY)),
        (
            "git checkout-index -n -f -- .codeflow/policy.json",
            writes(POLICY),
        ),
        // Keep-index checks the index tree out after the reset, and
        // `--patch` sets keep-index when it is left unset (review PW-5).
        ("git stash -k", writes(POLICY)),
        ("git stash push --keep-index", writes(POLICY)),
        ("git stash --keep", writes(POLICY)),
        ("git stash save -k", writes(POLICY)),
        ("git stash push -ku", writes(POLICY)),
        ("git stash push -p", writes(POLICY)),
        ("git stash --no-keep-index -k", writes(POLICY)),
        ("git stash", Passes),
        ("git stash push -u", Passes),
        ("git stash -k --no-keep-index", Passes),
        ("git stash push -p --no-keep-index", Passes),
        // `-m` takes the next word as its message.
        ("git stash push -m -k", Passes),
        ("git checkout -- src", Passes),
        ("git checkout-index --temp -a", Passes),
    ];
    // Each index-only first step, run for real in the fixture.
    let firsts: [&[&str]; 3] = [
        &["restore", "--staged", "--source=old", "--", POLICY],
        &["reset", "-q", "old", "--", POLICY],
        &["read-tree", "old"],
    ];
    for first in firsts {
        git(&f.root, first);
        expect(&f.root, &steps);
        f.reset_index();
    }
    f.plant_index(POLICY, "{\"planted\":true}\n");
    expect(&f.root, &steps);
    f.reset_index();
    f.plant_index(".codex/hooks.json", "{}\n");
    expect(
        &f.root,
        &[("git checkout-index -a -f", writes(".codex/hooks.json"))],
    );
    f.reset_index();
    // A clean index writes HEAD's content.
    expect(
        &f.root,
        &[
            ("git checkout-index -a -f", Passes),
            ("git checkout-index -n -f -a", Passes),
            ("git checkout -- .", Passes),
            ("git checkout-index --temp -a", Passes),
            ("git stash -k", Passes),
            ("git stash push -p", Passes),
            ("git stash push --nonesuch", Unknown),
            // A prefix is judged by the shared path check, which reads an
            // enforcement file's name under any directory.
            (
                "git checkout-index --prefix=.codeflow/ -a",
                writes(".codeflow/.codeflow/policy.json"),
            ),
            (
                "git checkout-index --prefix=out/ -a",
                writes("out/.codeflow/policy.json"),
            ),
            ("git checkout-index --prefix=out/ -- src/a.txt", Passes),
        ],
    );
}

#[test]
fn read_tree_with_update_is_judged_by_its_trees() {
    let f = Fixture::new();
    expect(
        &f.root,
        &[
            ("git read-tree -u --reset old", writes(POLICY)),
            ("git read-tree -m -u HEAD old", writes(POLICY)),
            (
                "git read-tree -u --reset --empty",
                writes(".claude/settings.json"),
            ),
            ("git read-tree -u --reset HEAD", Passes),
            ("git read-tree -u --reset same", Passes),
            ("git read-tree old", Passes),
            ("git read-tree -u --prefix=sub/ old", Unknown),
        ],
    );
}

#[test]
fn stash_apply_pop_and_branch_are_judged_by_what_the_stash_changes() {
    let f = Fixture::new();
    expect(
        &f.root,
        &[("git stash pop", Passes), ("git stash apply", Passes)],
    );
    write(&f.root, POLICY, "{\"stashed\":true}\n");
    git(&f.root, &["stash", "-q"]);
    write(&f.root, "src/a.txt", "stashed\n");
    git(&f.root, &["stash", "-q"]);
    expect(
        &f.root,
        &[
            // The newest stash changes only src/a.txt.
            ("git stash apply", Passes),
            ("git stash pop", Passes),
            ("git stash apply stash@{0}", Passes),
            // The older one changes the policy.
            ("git stash apply 1", writes(POLICY)),
            ("git stash pop stash@{1}", writes(POLICY)),
            ("git stash apply --index stash@{1}", writes(POLICY)),
            ("git stash branch b stash@{1}", writes(POLICY)),
            ("git stash branch b", Passes),
            ("git stash apply nonesuch", Unknown),
        ],
    );
    // Untracked files a stash carries are written as new files.
    git(&f.root, &["stash", "drop", "-q"]);
    git(&f.root, &["stash", "drop", "-q"]);
    write(&f.root, ".codex/hooks.json", "{}\n");
    git(&f.root, &["stash", "-q", "-u"]);
    expect(&f.root, &[("git stash pop", writes(".codex/hooks.json"))]);
}

#[test]
fn apply_and_am_are_judged_by_the_paths_their_patches_name() {
    let f = Fixture::new();
    let policy_patch = git(&f.root, &["diff", "main", "evil"]);
    let ordinary_patch = git(&f.root, &["diff", "main", "same"]);
    write(&f.root, "policy.patch", &format!("{policy_patch}\n"));
    write(&f.root, "ordinary.patch", &format!("{ordinary_patch}\n"));
    let mail = git(&f.root, &["format-patch", "--stdout", "main..evil"]);
    write(&f.root, "policy.mbox", &format!("{mail}\n"));
    let mail = git(&f.root, &["format-patch", "--stdout", "main..same"]);
    write(&f.root, "ordinary.mbox", &format!("{mail}\n"));
    write(
        &f.root,
        "quoted.patch",
        "diff --git \"a/.codeflow/policy.json\" \"b/.codeflow/policy.json\"\n",
    );
    write(
        &f.root,
        "rename.patch",
        "diff --git a/x b/y\nrename from x\nrename to .codex/hooks.json\n",
    );
    write(
        &f.root,
        "encoded.mbox",
        "From x\nContent-Transfer-Encoding: base64\n\nZGlmZg==\n",
    );
    expect(
        &f.root,
        &[
            ("git apply policy.patch", writes(POLICY)),
            ("git apply --index policy.patch", writes(POLICY)),
            ("git apply -3 policy.patch", writes(POLICY)),
            (
                "git apply -p2 --directory=.codeflow policy.patch",
                writes(POLICY),
            ),
            ("git apply -p0 --directory=a policy.patch", Passes),
            ("git apply quoted.patch", writes(POLICY)),
            ("git apply rename.patch", writes(".codex/hooks.json")),
            ("git apply", Unknown),
            ("git apply -", Unknown),
            ("git apply missing.patch", Unknown),
            ("git apply ordinary.patch", Passes),
            ("git apply --cached policy.patch", Passes),
            ("git apply --check policy.patch", Passes),
            ("git apply --stat policy.patch", Passes),
            ("git am policy.mbox", writes(POLICY)),
            ("git am -3 policy.mbox", writes(POLICY)),
            ("git am ordinary.mbox", Passes),
            ("git am", Unknown),
            ("git am encoded.mbox", Unknown),
            ("git am --abort", Passes),
            ("git am --continue", Passes),
        ],
    );
}

#[test]
fn no_commit_picks_and_reverts_are_judged_by_the_commits_they_apply() {
    let f = Fixture::new();
    expect(
        &f.root,
        &[
            ("git cherry-pick -n evil", writes(POLICY)),
            ("git cherry-pick --no-commit evil", writes(POLICY)),
            ("git cherry-pick --no-com evil", writes(POLICY)),
            ("git cherry-pick -n main..evil", writes(POLICY)),
            ("git cherry-pick -n ^main evil", writes(POLICY)),
            ("git revert -n HEAD", writes(POLICY)),
            ("git cherry-pick -n same", Passes),
            ("git cherry-pick -n main..same", Passes),
            ("git revert -n same", Passes),
            // Committing forms move HEAD and keep their verdict.
            ("git cherry-pick evil", Passes),
            ("git revert HEAD", Passes),
            ("git cherry-pick -n main...evil", Unknown),
            ("git cherry-pick -n nonesuch", Unknown),
        ],
    );
}

/// Review PW-4: `git sparse-checkout` writes the work tree from the index
/// and removes what the new set leaves out.
#[test]
fn sparse_checkout_is_judged_by_the_index_and_the_set_it_keeps() {
    let f = Fixture::new();
    let settings = ".claude/settings.json";
    expect(
        &f.root,
        &[
            // The set must keep every enforcement path HEAD holds.
            ("git sparse-checkout set --cone src", writes(settings)),
            ("git sparse-checkout set --cone src .claude", writes(POLICY)),
            (
                "git sparse-checkout set --cone src .codeflow .claude",
                Passes,
            ),
            ("git sparse-checkout set --cone .codeflow/ /.claude", Passes),
            // `init` and `reapply` with no patterns keep root files only.
            ("git sparse-checkout init", writes(settings)),
            ("git sparse-checkout reapply", writes(settings)),
            ("git sparse-checkout add src", Passes),
            ("git sparse-checkout disable", Passes),
            ("git sparse-checkout list", Passes),
            // Fail closed.
            ("git sparse-checkout set src .codeflow .claude", Unknown),
            (
                "git sparse-checkout set --no-cone src .codeflow .claude",
                Unknown,
            ),
            ("git sparse-checkout set --cone --stdin", Unknown),
            (
                "git sparse-checkout set --cone 'src/*' .codeflow .claude",
                Unknown,
            ),
            ("git sparse-checkout add --stdin", Unknown),
            ("git sparse-checkout add '!/.codeflow/'", Unknown),
            ("git sparse-checkout set --frobnicate", Unknown),
            ("git sparse-checkout frobnicate", Unknown),
        ],
    );
    // Run from a subdirectory, Git may read the directories from there.
    expect(
        &f.root.join("src"),
        &[(
            "git sparse-checkout set --cone src .codeflow .claude",
            Unknown,
        )],
    );
    git(&f.root, &["config", "core.sparseCheckoutCone", "true"]);
    expect(
        &f.root,
        &[
            ("git sparse-checkout set src", writes(settings)),
            ("git sparse-checkout set src .codeflow .claude", Passes),
        ],
    );
    // The patterns already written decide `init` and `reapply`.
    let patterns = f.root.join(".git/info/sparse-checkout");
    std::fs::create_dir_all(patterns.parent().unwrap()).unwrap();
    std::fs::write(&patterns, "/*\n!/*/\n/.claude/\n/.codeflow/\n/src/\n").unwrap();
    expect(
        &f.root,
        &[
            ("git sparse-checkout reapply", Passes),
            ("git sparse-checkout init", Passes),
        ],
    );
    std::fs::write(&patterns, "/*\n!/*/\n/.claude/\n!/.claude/*/\n/src/\n").unwrap();
    expect(&f.root, &[("git sparse-checkout reapply", writes(POLICY))]);
    std::fs::write(&patterns, "src\n").unwrap();
    expect(&f.root, &[("git sparse-checkout reapply", Unknown)]);
    std::fs::remove_file(&patterns).unwrap();
    // An index plant is written by every form that writes files.
    f.plant_index(POLICY, "{\"planted\":true}\n");
    expect(
        &f.root,
        &[
            ("git sparse-checkout disable", writes(POLICY)),
            ("git sparse-checkout add src", writes(POLICY)),
            (
                "git sparse-checkout set --cone src .codeflow .claude",
                writes(POLICY),
            ),
            ("git sparse-checkout list", Passes),
        ],
    );
}

#[test]
fn reset_to_a_revision_is_judged_by_that_revision() {
    let f = Fixture::new();
    expect(
        &f.root,
        &[
            ("git reset --hard old", writes(POLICY)),
            ("git reset --keep evil", writes(POLICY)),
            ("git reset --merge old", writes(POLICY)),
            ("git reset --hard", Passes),
            ("git reset --hard HEAD", Passes),
            ("git reset --keep same", Passes),
            ("git reset --soft old", Passes),
            ("git reset old", Passes),
            ("git reset -q old -- .codeflow/policy.json", Passes),
        ],
    );
}

#[test]
fn settings_that_move_the_repository_index_or_objects_refuse() {
    let f = Fixture::new();
    expect(
        &f.root,
        &[
            ("git -c core.worktree=/tmp checkout -- src", Unknown),
            ("git --git-dir=.git checkout -- src", Unknown),
            ("git --work-tree=. checkout -- src", Unknown),
            ("git --attr-source=old checkout -- src", Unknown),
            ("GIT_INDEX_FILE=x git checkout-index -a -f", Unknown),
            ("GIT_DIR=elsewhere git apply policy.patch", Unknown),
            ("git -c filter.x.smudge=cat checkout -- src", Unknown),
            ("git -c include.path=other checkout -- src", Unknown),
            ("git -c core.editor=\"$EDITOR\" checkout -- src", Unknown),
            ("git --no-pager checkout -- src", Passes),
            ("git --literal-pathspecs checkout -- src", Passes),
            // Settings that change no written path or content.
            ("git -c core.editor=true checkout -- src", Passes),
            (
                "git -c Color.UI=false -c user.name=t checkout -- src",
                Passes,
            ),
            ("git -c alias.co=checkout checkout -- src", Passes),
            // A form that writes no work-tree file is not refused for them.
            ("git -c filter.x.smudge=cat reset --soft HEAD", Passes),
            ("git -c filter.x.smudge=cat am --show-current-patch", Passes),
            ("git -c filter.x.smudge=cat stash list", Passes),
            (
                "git -c filter.x.smudge=cat checkout-index --temp -a",
                Passes,
            ),
            ("git -c filter.x.smudge=cat sparse-checkout list", Passes),
            ("GIT_INDEX_FILE=x git restore --staged src", Passes),
            // Every other form that writes work-tree files refuses them,
            // its content compared or not (review PW-2 and PW-3).
            (
                "git -c filter.x.smudge=cat checkout-index -n -f -a",
                Unknown,
            ),
            ("git -c filter.x.smudge=cat checkout -b fresh old", Unknown),
            ("git -c filter.x.smudge=cat checkout -B fresh old", Unknown),
            (
                "git -c filter.x.smudge=cat checkout --orphan o old",
                Unknown,
            ),
            ("git -c filter.x.smudge=cat checkout --detach old", Unknown),
            ("git -c filter.x.smudge=cat checkout old --", Unknown),
            ("git -c filter.x.smudge=cat checkout same", Unknown),
            ("git -c filter.x.smudge=cat switch -c fresh old", Unknown),
            ("git -c filter.x.smudge=cat merge same", Unknown),
            ("git -c filter.x.smudge=cat rebase old", Unknown),
            ("git -c filter.x.smudge=cat pull", Unknown),
            ("git -c filter.x.smudge=cat bisect start", Unknown),
            ("git -c filter.x.smudge=cat worktree add ../w", Unknown),
            ("git -c filter.x.smudge=cat stash", Unknown),
            ("git -c filter.x.smudge=cat stash push -u", Unknown),
            ("git -c filter.x.smudge=cat reset --hard", Unknown),
            ("git -c filter.x.smudge=cat am --abort", Unknown),
            ("git -c filter.x.smudge=cat cherry-pick old", Unknown),
            ("git -c include.path=x switch same", Unknown),
            ("GIT_DIR=elsewhere git switch same", Unknown),
            // Without such a setting a branch switch keeps its verdict.
            ("git -c core.editor=true cherry-pick old", Passes),
            ("git -c user.name=t checkout -b fresh old", Passes),
            ("git switch -c fresh old", Passes),
            ("git merge same", Passes),
            ("git stash", Passes),
            ("git am --abort", Passes),
        ],
    );
    git(&f.root, &["replace", "-f", "HEAD", "old"]);
    expect(
        &f.root,
        &[
            ("git checkout -- src", Unknown),
            ("git reset --soft old", Passes),
        ],
    );
}

/// With a moving setting, a repository the guard cannot find is still
/// judged: a command that writes the work tree refuses.
#[test]
fn a_moved_repository_outside_any_checkout_still_refuses() {
    let outside = tempfile::tempdir().unwrap();
    expect(
        outside.path(),
        &[
            ("GIT_DIR=/x/.git git checkout old -- .", Unknown),
            ("git --git-dir=/x/.git reset --hard old", Unknown),
            ("git checkout old -- .", Passes),
            ("GIT_DIR=/x/.git git reset --soft old", Passes),
        ],
    );
}

#[test]
fn a_case_or_unicode_variant_of_an_enforcement_path_lands_on_it() {
    let f = Fixture::new();
    let blob = {
        std::fs::write(f.root.join("v.blob"), "{\"case\":true}\n").unwrap();
        let blob = git(&f.root, &["hash-object", "-w", "v.blob"]);
        std::fs::remove_file(f.root.join("v.blob")).unwrap();
        blob
    };
    // Build trees by plumbing so the file system's case never matters.
    let mut commits = Vec::new();
    for path in [
        ".CODEFLOW/Policy.json",
        ".codeflow/policy\u{17f}.json",
        ".code\u{200c}flow/policy.json",
    ] {
        let index = f.root.join(".git/fixture-index");
        let env_git = |args: &[&str]| {
            let out = crate::git::command()
                .args(args)
                .current_dir(&f.root)
                .env("GIT_INDEX_FILE", &index)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {out:?}");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        env_git(&["read-tree", "HEAD"]);
        env_git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{blob},{path}"),
        ]);
        let tree = env_git(&["write-tree"]);
        let commit = env_git(&["commit-tree", &tree, "-p", "HEAD", "-m", "variant"]);
        std::fs::remove_file(&index).unwrap();
        commits.push((commit, path));
    }
    for (commit, path) in commits {
        let command = format!("git checkout {commit} -- .");
        assert_eq!(judged(&f.root, &command), writes(path), "{path}");
    }
}

#[test]
fn another_worktree_is_judged_in_its_own_checkout() {
    let f = Fixture::new();
    let other = f.root.parent().unwrap().join("other");
    git(
        &f.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "task/other",
            &other.to_string_lossy(),
        ],
    );
    let shown = other.to_string_lossy().to_string();
    expect(
        &f.root,
        &[
            (
                format!("git -C {shown} checkout old -- .codeflow/policy.json"),
                writes(POLICY),
            ),
            (format!("git -C {shown} checkout old -- ."), writes(POLICY)),
            (
                format!("git -C {shown} restore --source=old ."),
                writes(POLICY),
            ),
            (format!("git -C {shown} checkout old -- src/a.txt"), Passes),
            (
                "git -C ../other checkout old -- .".to_string(),
                writes(POLICY),
            ),
        ],
    );
}

#[test]
fn wildcards_match_as_git_reads_them() {
    for (pattern, text, want) in [
        ("*", ".codeflow/policy.json", true),
        ("*policy.json", ".codeflow/policy.json", true),
        ("*.json", ".claude/settings.json", true),
        (".c?deflow/*", ".codeflow/policy.json", true),
        ("[.]codeflow", ".codeflow", true),
        ("src/*", ".codeflow/policy.json", false),
        ("*.txt", ".codeflow/policy.json", false),
        ("a[", "a[", true),
    ] {
        assert_eq!(
            wildmatch(pattern.as_bytes(), text.as_bytes()),
            want,
            "{pattern} {text}"
        );
    }
}

#[test]
fn patch_paths_read_every_header() {
    let paths = patch_paths(
        "diff --git a/x b/y\n--- a/x\n+++ b/y\nrename from p q\ncopy to \"r\\tq\"\n",
        1,
    )
    .unwrap();
    for want in ["x", "y", "p q", "r\tq"] {
        assert!(paths.iter().any(|path| path == want), "{want}: {paths:?}");
    }
    assert!(patch_paths("Content-Transfer-Encoding: quoted-printable\n", 1).is_err());
    assert_eq!(unquote("\"a\\303\\251\"").as_deref(), Some("a\u{e9}"));
    assert_eq!(unquote("\"bad\\q\""), None);
}
