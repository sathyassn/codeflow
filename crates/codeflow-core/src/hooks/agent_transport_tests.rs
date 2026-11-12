//! TSK-1479: an agent's fetch from `origin` and fast-forward push of its own
//! work branch, judged by exec-guard in a repository whose GitHub CLI
//! credential helper is configured for an HTTPS `origin`.
//!
//! Every row is a command string the guard judges; nothing here runs a
//! fetch, a push or a credential helper. The fixtures are built with
//! `git2`, and each test runs in a child of this test binary with the
//! host's Git configuration cut off, so a helper the host configures never
//! changes a verdict.

use super::evaluate_at;
use crate::hooks::policy::{PolicyLevel, SecuritySection};
use crate::hooks::Violation;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The work branch the fixture checks out.
const WORK: &str = "task/agent-work";
/// The fixture's `origin`. The guard never asks it anything.
const ORIGIN: &str = "https://github.com/example/project.git";
/// The policy on `origin/main`, the landed authority.
const LANDED_POLICY: &str = r#"{"git":{"protected_branches":["main","master","task/held-*"],"branch_prefixes":["feat/","fix/","task/","integration/","codeflow/","rel/"],"release_branch_pattern":"rel/v*"}}"#;
/// The work branch's own policy, which adds the prefix `wip/`.
const WORK_POLICY: &str = r#"{"git":{"protected_branches":["main","master","task/held-*"],"branch_prefixes":["feat/","fix/","task/","integration/","codeflow/","rel/","wip/"],"release_branch_pattern":"rel/v*"}}"#;
/// The GitHub CLI helper as `gh auth setup-git` writes it for one host.
const GH_HELPER: &str =
    "[credential \"https://github.com\"]\n\thelper =\n\thelper = !/usr/local/bin/gh auth git-credential\n";
/// The same helper for every host, found on `PATH`.
const PLAIN_HELPER: &str = "[credential]\n\thelper = !gh auth git-credential\n";

/// Runs `test` again in a child of this test binary with the host's Git
/// configuration, home and environment cut off; returns `true` in that
/// child, which then runs the rows.
fn isolated(test: &str) -> bool {
    const CHILD: &str = "CODEFLOW_TSK1479_TRANSPORT_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(test) {
        return true;
    }
    let home = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("hooks::exec_guard::agent_transport_tests::{test}"),
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env_clear()
        .env(CHILD, test)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("TMPDIR", std::env::temp_dir())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

/// A repository on [`WORK`] with `main` and `origin/main` holding
/// [`LANDED_POLICY`], `origin/HEAD` naming `origin/main`, and `config`
/// appended to its configuration.
struct Fixture {
    _dir: tempfile::TempDir,
    /// The directory holding the repository, for `-C` rows.
    parent: PathBuf,
    root: PathBuf,
    work: git2::Oid,
}

impl Fixture {
    fn new(config: &str) -> Self {
        Self::with(config, |_, _| {})
    }

    fn with(config: &str, setup: impl FnOnce(&git2::Repository, &Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let parent = crate::portable_path::canonicalize(dir.path()).unwrap();
        let root = parent.join("repo");
        let mut options = git2::RepositoryInitOptions::new();
        options.initial_head("main");
        let repo = git2::Repository::init_opts(&root, &options).unwrap();
        let main = commit(&repo, LANDED_POLICY, None);
        let work = commit(&repo, WORK_POLICY, Some(main));
        repo.reference("refs/heads/main", main, true, "fixture")
            .unwrap();
        repo.reference(&format!("refs/heads/{WORK}"), work, true, "fixture")
            .unwrap();
        repo.remote("origin", ORIGIN).unwrap();
        repo.reference("refs/remotes/origin/main", main, true, "fixture")
            .unwrap();
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "fixture",
        )
        .unwrap();
        repo.set_head(&format!("refs/heads/{WORK}")).unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        append(&root.join(".git").join("config"), config.as_bytes());
        setup(&repo, &root);
        Self {
            _dir: dir,
            parent,
            root,
            work,
        }
    }
}

fn append(path: &Path, bytes: &[u8]) {
    use std::io::Write as _;
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

fn commit(repo: &git2::Repository, policy: &str, parent: Option<git2::Oid>) -> git2::Oid {
    let blob = repo.blob(policy.as_bytes()).unwrap();
    let mut inner = repo.treebuilder(None).unwrap();
    inner.insert("policy.json", blob, 0o100_644).unwrap();
    let inner = inner.write().unwrap();
    let readme = repo.blob(b"fixture\n").unwrap();
    let mut top = repo.treebuilder(None).unwrap();
    top.insert(".codeflow", inner, 0o040_000).unwrap();
    top.insert("README.md", readme, 0o100_644).unwrap();
    let tree = repo.find_tree(top.write().unwrap()).unwrap();
    let sig = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let parents: Vec<git2::Commit<'_>> = parent
        .map(|oid| repo.find_commit(oid).unwrap())
        .into_iter()
        .collect();
    let parents: Vec<&git2::Commit<'_>> = parents.iter().collect();
    repo.commit(None, &sig, &sig, "fixture", &tree, &parents)
        .unwrap()
}

fn verdict(cwd: &Path, command: &str) -> Vec<Violation> {
    evaluate_at(
        command,
        &SecuritySection::default(),
        PolicyLevel::Block,
        cwd,
        cwd,
    )
}

/// The finding the GitHub CLI helper body draws from the account rule.
fn account(found: &[Violation]) -> bool {
    found.iter().any(|finding| {
        finding.rule == "security.outward_actions" && finding.message.starts_with("account action:")
    })
}

fn blocked(found: &[Violation]) -> bool {
    found
        .iter()
        .any(|finding| finding.level == PolicyLevel::Block)
}

fn shown(found: &[Violation]) -> String {
    found
        .iter()
        .map(|finding| format!("{}: {}", finding.rule, finding.message))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Each row passes with no finding.
fn expect_pass(cwd: &Path, rows: &[String], wrong: &mut Vec<String>) {
    for row in rows {
        let found = verdict(cwd, row);
        if !found.is_empty() {
            wrong.push(format!("should pass: {row}: {}", shown(&found)));
        }
    }
}

/// Each row keeps the account finding on the helper body, so the
/// admission did not apply and the line is refused.
fn expect_account(cwd: &Path, rows: &[String], wrong: &mut Vec<String>) {
    for row in rows {
        let found = verdict(cwd, row);
        if !(account(&found) && blocked(&found)) {
            wrong.push(format!(
                "should keep the account finding: {row}: {}",
                shown(&found)
            ));
        }
    }
}

/// Each row is refused, by the account finding or by another rule that
/// stops the guard reading the line's programs.
fn expect_refused(cwd: &Path, rows: &[String], wrong: &mut Vec<String>) {
    for row in rows {
        let found = verdict(cwd, row);
        if !blocked(&found) {
            wrong.push(format!("should refuse: {row}: {}", shown(&found)));
        }
    }
}

fn rows(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_string()).collect()
}

fn report(wrong: &[String]) {
    assert!(
        wrong.is_empty(),
        "{} row(s) judged otherwise:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// AC-10: the admitted fetch and push forms, refused before the change
/// and passing after, with the helper for one host and for every host.
#[test]
fn admitted_forms_pass_with_the_github_cli_helper() {
    if !isolated("admitted_forms_pass_with_the_github_cli_helper") {
        return;
    }
    let mut wrong = Vec::new();
    for helper in [GH_HELPER, PLAIN_HELPER] {
        let f = Fixture::new(helper);
        let sha = f.work.to_string();
        let mut lines = rows(&[
            "git fetch",
            "git fetch origin",
            "git fetch -q --prune origin main",
            "git fetch --quiet -p origin main task/agent-work",
            "git push origin HEAD:refs/heads/task/agent-work",
            "git push -u origin HEAD",
            "git push origin task/agent-work",
            "git push --set-upstream -q origin task/agent-work:refs/heads/task/agent-work",
            "git push origin HEAD:refs/heads/fix/agent-other",
            "git push origin HEAD:refs/heads/codeflow/notes",
            "git push origin HEAD:refs/heads/rel/notes",
            "git\tpush  origin   HEAD:refs/heads/feat/agent-x",
        ]);
        lines.push(format!("git push origin {sha}:refs/heads/task/agent-work"));
        expect_pass(&f.root, &lines, &mut wrong);
        expect_pass(
            &f.parent,
            &rows(&[
                "git -C repo fetch origin",
                "git -C repo push origin HEAD:refs/heads/task/agent-work",
            ]),
            &mut wrong,
        );
    }
    // A URL rewrite that does not match `origin` changes nothing.
    let f = Fixture::new(&format!(
        "{GH_HELPER}[url \"https://mirror.example.invalid/\"]\n\tinsteadOf = https://elsewhere.example.invalid/\n"
    ));
    expect_pass(
        &f.root,
        &rows(&[
            "git fetch origin",
            "git push origin HEAD:refs/heads/task/agent-work",
        ]),
        &mut wrong,
    );
    report(&wrong);
}

/// AC-1: fetch forms outside the closed form stay refused.
#[test]
fn refused_fetch_forms_keep_the_account_rule() {
    if !isolated("refused_fetch_forms_keep_the_account_rule") {
        return;
    }
    let f = Fixture::new(GH_HELPER);
    let mut wrong = Vec::new();
    expect_account(
        &f.root,
        &rows(&[
            "git fetch --upload-pack=git-upload-pack origin",
            "git fetch --upload-pack git-upload-pack origin",
            "git fetch https://github.com/example/project.git",
            "git fetch https://github.com/example/other.git main",
            "git fetch /tmp/elsewhere.git",
            "git fetch --all",
            "git fetch --multiple origin",
            "git fetch --recurse-submodules origin",
            "git fetch --recurse-submodules=yes origin",
            "git fetch origin main:main",
            "git fetch origin main:refs/heads/main",
            "git fetch origin +main",
            "git fetch origin +refs/heads/main:refs/remotes/origin/main",
            "git fetch origin tag v1",
            "git fetch --tags origin",
            "git fetch --depth 1 origin",
            "git fetch -v origin",
            "git fetch -- origin",
            "git fetch upstream",
            "git fetch main",
            "git pull",
            "git pull origin main",
            "git pull --rebase origin main",
            "git fetch origin; git fetch origin",
            "git fetch origin && true",
            "git fetch origin | cat",
            "git fetch origin > fetched.txt",
            "git fetch 'origin'",
            "git fetch \"origin\"",
            "git fetch origin # note",
        ]),
        &mut wrong,
    );
    // An ext:: transport stops the program reader, which refuses the line.
    expect_refused(
        &f.root,
        &rows(&[
            "git fetch ext::sh",
            "git fetch origin --upload-pack=ext::sh",
        ]),
        &mut wrong,
    );
    // A configured fetch mapping whose destination is outside
    // `refs/remotes/origin/`.
    for mapping in [
        "+refs/heads/*:refs/heads/*",
        "+refs/heads/*:refs/remotes/other/*",
        "+refs/heads/main:refs/remotes/originx/main",
        "refs/tags/*:refs/tags/*",
    ] {
        let f = Fixture::new(&format!(
            "{GH_HELPER}[remote \"origin\"]\n\tfetch = {mapping}\n"
        ));
        expect_account(
            &f.root,
            &rows(&["git fetch", "git fetch origin"]),
            &mut wrong,
        );
    }
    // A bare fetch whose branch names another remote or a path.
    let f = Fixture::new(&format!(
        "{GH_HELPER}[branch \"task/agent-work\"]\n\tremote = /tmp/elsewhere.git\n"
    ));
    expect_account(&f.root, &rows(&["git fetch"]), &mut wrong);
    report(&wrong);
}

/// AC-2: push forms outside the closed form stay refused.
#[test]
fn refused_push_forms_keep_the_account_rule() {
    if !isolated("refused_push_forms_keep_the_account_rule") {
        return;
    }
    let f = Fixture::with(GH_HELPER, |repo, _| {
        let head = repo.head().unwrap().target().unwrap();
        let object = repo.find_object(head, None).unwrap();
        repo.tag_lightweight("v1", &object, false).unwrap();
    });
    let mut wrong = Vec::new();
    expect_account(
        &f.root,
        &rows(&[
            "git push -f origin HEAD:refs/heads/task/agent-work",
            "git push --force origin HEAD:refs/heads/task/agent-work",
            "git push --force-with-lease origin HEAD:refs/heads/task/agent-work",
            "git push --force-if-includes origin HEAD:refs/heads/task/agent-work",
            "git push origin +HEAD:refs/heads/task/agent-work",
            "git push -d origin task/agent-work",
            "git push --delete origin task/agent-work",
            "git push origin :refs/heads/task/agent-work",
            "git push --mirror origin",
            "git push --all origin",
            "git push --branches origin",
            "git push --tags origin",
            "git push --follow-tags origin HEAD:refs/heads/task/agent-work",
            "git push origin HEAD:refs/tags/v2",
            "git push origin v1",
            "git push origin refs/tags/v1:refs/tags/v1",
            "git push --no-verify origin HEAD:refs/heads/task/agent-work",
            "git push -o ci.skip origin HEAD:refs/heads/task/agent-work",
            "git push --push-option ci.skip origin HEAD:refs/heads/task/agent-work",
            "git push --receive-pack git-receive-pack origin HEAD:refs/heads/task/agent-work",
            "git push --exec git-receive-pack origin HEAD:refs/heads/task/agent-work",
            "git push --repo origin HEAD:refs/heads/task/agent-work",
            "git push --signed origin HEAD:refs/heads/task/agent-work",
            "git push --atomic origin HEAD:refs/heads/task/agent-work",
            "git push --prune origin HEAD:refs/heads/task/agent-work",
            "git push --recurse-submodules=check origin HEAD:refs/heads/task/agent-work",
            "git push -v origin HEAD:refs/heads/task/agent-work",
            "git push",
            "git push origin",
            "git push -u origin",
            "git push upstream HEAD:refs/heads/task/agent-work",
            "git push https://github.com/example/project.git HEAD:refs/heads/task/agent-work",
            "git push /tmp/elsewhere.git HEAD:refs/heads/task/agent-work",
            "git push origin HEAD:refs/heads/task/agent-work HEAD:refs/heads/task/agent-two",
            "git push origin HEAD:task/agent-work",
            "git push origin HEAD:refs/remotes/origin/task/agent-work",
            "git push origin HEAD:refs/heads/task/agent-work:x",
            "git push origin nosuchbranch:refs/heads/task/agent-work",
            "git push origin v1:refs/heads/task/agent-work",
            "git push origin origin/main:refs/heads/task/agent-work",
            "git push origin HEAD~1:refs/heads/task/agent-work",
            "git push origin @:refs/heads/task/agent-work",
            "git push origin task/agent-other",
            "git push origin HEAD:refs/heads/task/agent-work; true",
            "git push origin 'HEAD:refs/heads/task/agent-work'",
        ]),
        &mut wrong,
    );
    // A wildcard refspec stops the program reader, which refuses the line.
    expect_refused(
        &f.root,
        &rows(&["git push origin refs/heads/task/*:refs/heads/task/*"]),
        &mut wrong,
    );
    // The bare forms on a checkout whose current branch is `main`.
    let on_main = Fixture::with(GH_HELPER, |repo, _| {
        repo.set_head("refs/heads/main").unwrap();
    });
    expect_account(
        &on_main.root,
        &rows(&[
            "git push origin HEAD",
            "git push -u origin HEAD",
            "git push origin main",
            "git push origin task/agent-work",
        ]),
        &mut wrong,
    );
    // The bare forms on a detached checkout.
    let detached = Fixture::with(GH_HELPER, |repo, _| {
        let head = repo.head().unwrap().target().unwrap();
        repo.set_head_detached(head).unwrap();
    });
    expect_account(
        &detached.root,
        &rows(&["git push origin HEAD", "git push origin task/agent-work"]),
        &mut wrong,
    );
    // A local tag with the work branch's name.
    let tagged = Fixture::with(GH_HELPER, |repo, _| {
        let head = repo.head().unwrap().target().unwrap();
        let object = repo.find_object(head, None).unwrap();
        repo.tag_lightweight(WORK, &object, false).unwrap();
    });
    expect_account(
        &tagged.root,
        &rows(&[
            "git push origin HEAD",
            "git push origin task/agent-work",
            "git push origin HEAD:refs/heads/task/agent-work",
            "git push origin task/agent-work:refs/heads/task/agent-work",
        ]),
        &mut wrong,
    );
    report(&wrong);
}

/// AC-3: names outside the landed work-branch prefixes, or protected,
/// stay refused; a prefix only the pushed branch's own policy adds does
/// not count.
#[test]
fn refused_names_keep_the_account_rule() {
    if !isolated("refused_names_keep_the_account_rule") {
        return;
    }
    let f = Fixture::new(GH_HELPER);
    let mut wrong = Vec::new();
    let names = [
        "main",
        "master",
        "MAIN",
        "integration/EPC-001-line",
        "integration/release-1",
        "integration",
        "rel/v1",
        "rel/v2.0",
        "codeflow/registry",
        "task/held-one",
        "topic",
        "wip/agent-work",
        "release/1",
        "v1.2.3",
        "task/",
        "task/a..b",
        "task/x.lock",
        "task//x",
        "task/.hidden",
        "HEAD",
    ];
    let lines: Vec<String> = names
        .iter()
        .map(|name| format!("git push origin HEAD:refs/heads/{name}"))
        .collect();
    expect_account(&f.root, &lines, &mut wrong);
    // A checkout whose current branch is a refused name: the bare forms.
    for name in [
        "integration/EPC-001-line",
        "wip/agent-work",
        "task/held-one",
    ] {
        let f = Fixture::with(GH_HELPER, |repo, _| {
            let head = repo.head().unwrap().target().unwrap();
            let commit = repo.find_commit(head).unwrap();
            repo.branch(name, &commit, true).unwrap();
            repo.set_head(&format!("refs/heads/{name}")).unwrap();
        });
        expect_account(
            &f.root,
            &[
                "git push origin HEAD".to_string(),
                format!("git push origin {name}"),
            ],
            &mut wrong,
        );
    }
    report(&wrong);
}

/// AC-4 and AC-5, and AC-1's other configured programs: push and remote
/// settings withhold the admission.
#[test]
fn push_and_remote_settings_withhold_the_admission() {
    if !isolated("push_and_remote_settings_withhold_the_admission") {
        return;
    }
    let mut wrong = Vec::new();
    for setting in [
        "[remote \"origin\"]\n\tpushurl = https://github.com/example/other.git\n",
        "[remote \"origin\"]\n\tpushurl = https://github.com/example/project.git\n",
        "[remote \"origin\"]\n\tpush = refs/heads/*:refs/heads/*\n",
        "[remote \"origin\"]\n\tpush = HEAD:refs/heads/main\n",
        "[remote \"origin\"]\n\tmirror = true\n",
        "[remote \"origin\"]\n\tmirror = false\n",
        "[push]\n\tpushOption = ci.skip\n",
        "[push]\n\trecurseSubmodules = check\n",
        "[push]\n\tgpgSign = if-asked\n",
        "[submodule]\n\trecurse = true\n",
        "[fetch]\n\trecurseSubmodules = yes\n",
        "[url \"https://github.com/attacker/\"]\n\tinsteadOf = https://github.com/example/\n",
        "[url \"https://github.com/attacker/\"]\n\tpushInsteadOf = https://github.com/example/\n",
        // AC-5: a second remote, and a second URL for `origin`.
        "[remote \"upstream\"]\n\turl = https://github.com/example/upstream.git\n",
        "[remote \"origin\"]\n\turl = https://github.com/example/other.git\n",
        "[remote \"origin\"]\n\tvcs = fixture\n",
        // AC-1: a configured program other than the helper.
        "[core]\n\tsshCommand = ssh\n",
        "[core]\n\taskPass = /usr/bin/true\n",
        "[remote \"origin\"]\n\tuploadpack = git-upload-pack\n",
        "[remote \"origin\"]\n\treceivepack = git-receive-pack\n",
        "[credential]\n\thelper = osxkeychain\n",
        "[hook \"fixture\"]\n\tcommand = /usr/bin/true\n",
    ] {
        let f = Fixture::new(&format!("{GH_HELPER}{setting}"));
        let lines = rows(&[
            "git fetch origin",
            "git push origin HEAD:refs/heads/task/agent-work",
        ]);
        let before = wrong.len();
        expect_account(&f.root, &lines, &mut wrong);
        for entry in &mut wrong[before..] {
            write!(entry, " [setting: {}]", setting.replace('\n', " ")).unwrap();
        }
    }
    // AC-5: the one remote has another name.
    let renamed = Fixture::with(GH_HELPER, |repo, _| {
        repo.remote_rename("origin", "fork").unwrap();
    });
    expect_account(
        &renamed.root,
        &rows(&[
            "git fetch",
            "git fetch fork",
            "git push fork HEAD:refs/heads/task/agent-work",
        ]),
        &mut wrong,
    );
    // AC-5: an `origin` on a plain-text HTTP URL, or a remote helper URL.
    for url in [
        "http://github.com/example/project.git",
        "fixture::https://github.com/example/project.git",
    ] {
        let f = Fixture::with(GH_HELPER, |repo, _| {
            repo.remote_set_url("origin", url).unwrap();
        });
        expect_refused(
            &f.root,
            &rows(&[
                "git fetch origin",
                "git push origin HEAD:refs/heads/task/agent-work",
            ]),
            &mut wrong,
        );
    }
    // A submodule file at the top of the checkout.
    let modules = Fixture::with(GH_HELPER, |_, root| {
        std::fs::write(
            root.join(".gitmodules"),
            "[submodule \"m\"]\n\tpath = m\n\turl = https://github.com/example/m.git\n",
        )
        .unwrap();
    });
    expect_account(
        &modules.root,
        &rows(&[
            "git fetch origin",
            "git push origin HEAD:refs/heads/task/agent-work",
        ]),
        &mut wrong,
    );
    report(&wrong);
}

/// AC-6 and AC-7: configuration overrides on the line, and program
/// spellings other than the plain `git` word, withhold the admission.
#[test]
fn line_overrides_and_program_spellings_withhold_the_admission() {
    if !isolated("line_overrides_and_program_spellings_withhold_the_admission") {
        return;
    }
    let f = Fixture::new(GH_HELPER);
    let mut wrong = Vec::new();
    expect_account(
        &f.root,
        &rows(&[
            "git -c credential.helper= fetch origin",
            "git -c credential.helper fetch origin",
            "git -c core.askPass=true fetch origin",
            "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=credential.helper GIT_CONFIG_VALUE_0= git fetch origin",
            "GIT_CONFIG_PARAMETERS= git fetch origin",
            "git --git-dir .git fetch origin",
            "git --work-tree . fetch origin",
            "git -C . -C . fetch origin",
            "git --no-pager fetch origin",
            "git -c push.default=current push origin HEAD:refs/heads/task/agent-work",
            "/usr/bin/git fetch origin",
            "command git fetch origin",
            "env git fetch origin",
            "nice git fetch origin",
            "exec git fetch origin",
            "\\git fetch origin",
            "\"git\" fetch origin",
            "'git' fetch origin",
            "time git fetch origin",
        ]),
        &mut wrong,
    );
    // The program reader cannot read these lines, so they are refused
    // before any runner body is judged.
    expect_refused(
        &f.root,
        &rows(&[
            "git --config-env=credential.helper=HELPER fetch origin",
            "git --config-env credential.helper=HELPER fetch origin",
            "git -C -q fetch origin",
        ]),
        &mut wrong,
    );
    report(&wrong);
}

/// AC-8: only the exact helper body is exempt, and only on an admitted
/// line.
#[test]
fn near_miss_helper_bodies_and_other_lines_keep_the_account_rule() {
    if !isolated("near_miss_helper_bodies_and_other_lines_keep_the_account_rule") {
        return;
    }
    let mut wrong = Vec::new();
    for helper in [
        "!/usr/local/bin/git-credential-gh auth git-credential",
        "!/usr/local/bin/gh auth git-credential --hostname github.com",
        "!/usr/local/bin/gh auth token",
        "!/usr/local/bin/gh  auth git-credential",
        "!/usr/bin/env gh auth git-credential",
        // Quoted, so the `;` is part of the value and not a comment.
        "\"!gh auth git-credential; true\"",
        "!gh auth git-credential && true",
        "!git-credential-osxkeychain",
        "git-credential-store",
        "gh auth git-credential",
    ] {
        let f = Fixture::new(&format!(
            "[credential \"https://github.com\"]\n\thelper =\n\thelper = {helper}\n"
        ));
        let before = wrong.len();
        expect_account(
            &f.root,
            &rows(&[
                "git fetch origin",
                "git push origin HEAD:refs/heads/task/agent-work",
            ]),
            &mut wrong,
        );
        for entry in &mut wrong[before..] {
            write!(entry, " [helper: {helper}]").unwrap();
        }
    }
    // The exact body on lines that are not admitted.
    let f = Fixture::new(GH_HELPER);
    expect_account(
        &f.root,
        &rows(&[
            "git push origin HEAD:refs/heads/main",
            "git push origin HEAD:refs/heads/integration/EPC-001-line",
            "git ls-remote origin",
            "git pull origin main",
            "git fetch --all",
            "git clone https://github.com/example/project.git copy",
            "git credential fill",
        ]),
        &mut wrong,
    );
    // A helper under another base name that the account rule does not
    // match draws no account finding, before the admission and after it:
    // the admission never changes its verdict.
    for helper in [
        "!/usr/local/bin/gh2 auth git-credential",
        "!/usr/local/bin/xgh auth git-credential",
    ] {
        let f = Fixture::new(&format!(
            "[credential \"https://github.com\"]\n\thelper =\n\thelper = {helper}\n"
        ));
        for row in [
            "git fetch origin",
            "git push origin HEAD:refs/heads/task/agent-work",
        ] {
            if account(&verdict(&f.root, row)) {
                wrong.push(format!(
                    "no account finding expected: {row} [helper: {helper}]"
                ));
            }
        }
    }
    report(&wrong);
}

/// AC-9: a configuration, ref, policy or remote read failure withholds the
/// admission.
#[test]
fn read_failures_withhold_the_admission() {
    type Setup = Box<dyn Fn(&git2::Repository, &Path)>;
    if !isolated("read_failures_withhold_the_admission") {
        return;
    }
    let every = rows(&[
        "git fetch",
        "git fetch origin",
        "git push origin HEAD:refs/heads/task/agent-work",
        "git push origin HEAD",
        "git push origin task/agent-work",
    ]);
    let pushes = rows(&[
        "git push origin HEAD:refs/heads/task/agent-work",
        "git push origin HEAD",
        "git push origin task/agent-work",
    ]);
    let bare_fetch = rows(&["git fetch"]);
    let mut wrong = Vec::new();
    let cases: Vec<(&str, &[String], Setup)> = vec![
        (
            "no origin/HEAD",
            every.as_slice(),
            Box::new(|repo, _| {
                repo.find_reference("refs/remotes/origin/HEAD")
                    .unwrap()
                    .delete()
                    .unwrap();
            }),
        ),
        (
            "origin/HEAD names a missing branch",
            every.as_slice(),
            Box::new(|repo, _| {
                repo.reference_symbolic(
                    "refs/remotes/origin/HEAD",
                    "refs/remotes/origin/gone",
                    true,
                    "fixture",
                )
                .unwrap();
            }),
        ),
        (
            "unreadable landed policy",
            every.as_slice(),
            Box::new(|repo, _| {
                let broken = commit(repo, "{ not json", None);
                repo.reference("refs/remotes/origin/main", broken, true, "fixture")
                    .unwrap();
            }),
        ),
        (
            "no remote-tracking refs",
            every.as_slice(),
            Box::new(|repo, _| {
                repo.find_reference("refs/remotes/origin/HEAD")
                    .unwrap()
                    .delete()
                    .unwrap();
                repo.find_reference("refs/remotes/origin/main")
                    .unwrap()
                    .delete()
                    .unwrap();
            }),
        ),
        (
            "unreadable local tag",
            pushes.as_slice(),
            Box::new(|_, root| {
                let tags = root.join(".git/refs/tags/task");
                std::fs::create_dir_all(&tags).unwrap();
                std::fs::write(tags.join("agent-work"), "garbage\n").unwrap();
            }),
        ),
        (
            "origin URL that is not UTF-8",
            every.as_slice(),
            Box::new(|_, root| {
                append(
                    &root.join(".git").join("config"),
                    b"[remote \"origin\"]\n\turl = https://github.com/\xff.git\n",
                );
            }),
        ),
        (
            "fetch mapping that is not UTF-8",
            every.as_slice(),
            Box::new(|_, root| {
                append(
                    &root.join(".git").join("config"),
                    b"[remote \"origin\"]\n\tfetch = +refs/heads/*:refs/remotes/origin/\xff*\n",
                );
            }),
        ),
        (
            "branch remote that is not UTF-8",
            bare_fetch.as_slice(),
            Box::new(|_, root| {
                append(
                    &root.join(".git").join("config"),
                    b"[branch \"task/agent-work\"]\n\tremote = \xff\n",
                );
            }),
        ),
    ];
    for (case, lines, setup) in &cases {
        let f = Fixture::with(GH_HELPER, |repo, root| setup(repo, root));
        let before = wrong.len();
        expect_refused(&f.root, lines, &mut wrong);
        for entry in &mut wrong[before..] {
            write!(entry, " [{case}]").unwrap();
        }
    }
    report(&wrong);
}
