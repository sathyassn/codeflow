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
/// The fixture's `origin`, on the loopback host: a guard asks it what it
/// holds only when its tracking refs are missing (a table row deletes
/// them), and a loopback question never leaves this machine
/// (`test_remotes_contract`).
const ORIGIN: &str = "https://localhost/example/project.git";
/// The policy on `origin/main`, the landed authority, which names
/// [`ORIGIN`] in `git.agent_remote_urls`.
const LANDED_POLICY: &str = r#"{"git":{"protected_branches":["main","master","task/held-*"],"branch_prefixes":["feat/","fix/","task/","integration/","codeflow/","rel/"],"release_branch_pattern":"rel/v*","agent_remote_urls":["https://localhost/example/project.git"]}}"#;

/// [`LANDED_POLICY`] with `git.agent_remote_urls` set to `urls`, written as
/// JSON, or left out when `None`.
fn landed_urls(urls: Option<&str>) -> String {
    let key = urls.map_or_else(String::new, |urls| {
        format!(r#","agent_remote_urls":{urls}"#)
    });
    format!(
        r#"{{"git":{{"protected_branches":["main","master","task/held-*"],"branch_prefixes":["feat/","fix/","task/","integration/","codeflow/","rel/"],"release_branch_pattern":"rel/v*"{key}}}}}"#
    )
}

/// Moves `main` and `origin/main` to a new commit holding `policy`, so it
/// becomes the landed policy.
fn reland(repo: &git2::Repository, policy: &str) {
    let head = repo
        .find_reference("refs/remotes/origin/main")
        .unwrap()
        .target()
        .unwrap();
    let landed = commit(repo, policy, Some(head));
    repo.reference("refs/heads/main", landed, true, "fixture")
        .unwrap();
    repo.reference("refs/remotes/origin/main", landed, true, "fixture")
        .unwrap();
}
/// The work branch's own policy, which adds the prefix `wip/`.
const WORK_POLICY: &str = r#"{"git":{"protected_branches":["main","master","task/held-*"],"branch_prefixes":["feat/","fix/","task/","integration/","codeflow/","rel/","wip/"],"release_branch_pattern":"rel/v*"}}"#;
/// The GitHub CLI helper as `gh auth setup-git` writes it for one host,
/// here [`ORIGIN`]'s.
const GH_HELPER: &str =
    "[credential \"https://localhost\"]\n\thelper =\n\thelper = !/usr/local/bin/gh auth git-credential\n";
/// The same helper for every host, found on `PATH`.
const PLAIN_HELPER: &str = "[credential]\n\thelper = !gh auth git-credential\n";

/// Runs `test` again in a child of this test binary with the host's Git
/// configuration, home and environment cut off; returns `true` in that
/// child, which then runs the rows.
fn isolated(test: &str) -> bool {
    isolated_with(test, None)
}

/// [`isolated`], with `system` as the child's system Git configuration
/// file when given.
fn isolated_with(test: &str, system: Option<&str>) -> bool {
    const CHILD: &str = "CODEFLOW_TSK1479_TRANSPORT_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(test) {
        return true;
    }
    let home = tempfile::tempdir().unwrap();
    let system_file = home.path().join("system-gitconfig");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
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
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    if let Some(contents) = system {
        std::fs::write(&system_file, contents).unwrap();
        command.env("GIT_CONFIG_SYSTEM", &system_file);
    } else {
        command
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
    }
    let output = command.output().unwrap();
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
        Self::landed(LANDED_POLICY, config, setup)
    }

    /// [`Fixture::with`], with `landed` as the policy on `main` and
    /// `origin/main`.
    fn landed(landed: &str, config: &str, setup: impl FnOnce(&git2::Repository, &Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let parent = crate::portable_path::canonicalize(dir.path()).unwrap();
        let root = parent.join("repo");
        let mut options = git2::RepositoryInitOptions::new();
        options.initial_head("main");
        let repo = git2::Repository::init_opts(&root, &options).unwrap();
        let main = commit(&repo, landed, None);
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
            "git fetch https://localhost/example/project.git",
            "git fetch https://localhost/example/other.git main",
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
            "git push https://localhost/example/project.git HEAD:refs/heads/task/agent-work",
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
        "[remote \"origin\"]\n\tpushurl = https://localhost/example/other.git\n",
        "[remote \"origin\"]\n\tpushurl = https://localhost/example/project.git\n",
        "[remote \"origin\"]\n\tpush = refs/heads/*:refs/heads/*\n",
        "[remote \"origin\"]\n\tpush = HEAD:refs/heads/main\n",
        "[remote \"origin\"]\n\tmirror = true\n",
        "[remote \"origin\"]\n\tmirror = false\n",
        "[push]\n\tpushOption = ci.skip\n",
        "[push]\n\trecurseSubmodules = check\n",
        "[push]\n\tgpgSign = if-asked\n",
        "[submodule]\n\trecurse = true\n",
        "[fetch]\n\trecurseSubmodules = yes\n",
        "[url \"https://localhost/attacker/\"]\n\tinsteadOf = https://localhost/example/\n",
        "[url \"https://localhost/attacker/\"]\n\tpushInsteadOf = https://localhost/example/\n",
        // AC-5: a second remote, and a second URL for `origin`.
        "[remote \"upstream\"]\n\turl = https://localhost/example/upstream.git\n",
        "[remote \"origin\"]\n\turl = https://localhost/example/other.git\n",
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
        "http://localhost/example/project.git",
        "fixture::https://localhost/example/project.git",
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
            "[submodule \"m\"]\n\tpath = m\n\turl = https://localhost/example/m.git\n",
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

/// AC-5: `origin`'s one URL must equal an entry of `git.agent_remote_urls`
/// in the landed policy, exactly. The sole URL replaced by another
/// repository on the same host, its count kept at one, an unset, empty,
/// malformed or non-matching list, a near spelling, and a list that is
/// only uncommitted or branch-local all withhold the admission.
#[test]
fn origin_must_match_a_landed_remote_url() {
    type Setup = Box<dyn Fn(&git2::Repository, &Path)>;
    if !isolated("origin_must_match_a_landed_remote_url") {
        return;
    }
    let lines = rows(&[
        "git fetch",
        "git fetch origin",
        "git push origin HEAD:refs/heads/task/agent-work",
        "git push origin HEAD",
    ]);
    let none = landed_urls(None);
    let mut wrong = Vec::new();
    let mut refused: Vec<(&str, String, Setup)> = vec![(
        "the sole URL replaced by another repository on the same host",
        LANDED_POLICY.to_string(),
        Box::new(|repo, _| {
            repo.remote_set_url("origin", "https://localhost/attacker/project.git")
                .unwrap();
        }),
    )];
    for (case, urls) in [
        ("unset", None),
        ("empty", Some("[]")),
        (
            "another repository",
            Some(r#"["https://localhost/example/other.git"]"#),
        ),
        (
            "a trailing slash",
            Some(r#"["https://localhost/example/project.git/"]"#),
        ),
        (
            "without .git",
            Some(r#"["https://localhost/example/project"]"#),
        ),
        (
            "host in capitals",
            Some(r#"["https://LOCALHOST/example/project.git"]"#),
        ),
        (
            "a string, not a list",
            Some(r#""https://localhost/example/project.git""#),
        ),
        ("a number", Some("[1]")),
        (
            "a null beside the URL",
            Some(r#"["https://localhost/example/project.git",null]"#),
        ),
        ("null", Some("null")),
    ] {
        refused.push((case, landed_urls(urls), Box::new(|_, _| {})));
    }
    // The list only in the working tree, or only on the work branch.
    refused.push((
        "an uncommitted edit",
        none.clone(),
        Box::new(|_, root| {
            std::fs::write(root.join(".codeflow").join("policy.json"), LANDED_POLICY).unwrap();
        }),
    ));
    refused.push((
        "a branch-local commit",
        none.clone(),
        Box::new(|repo, _| {
            let head = repo.head().unwrap().target().unwrap();
            let local = commit(repo, LANDED_POLICY, Some(head));
            repo.reference(&format!("refs/heads/{WORK}"), local, true, "fixture")
                .unwrap();
            repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
                .unwrap();
        }),
    ));
    for (case, landed, setup) in &refused {
        let f = Fixture::landed(landed, GH_HELPER, |repo, root| setup(repo, root));
        let before = wrong.len();
        expect_account(&f.root, &lines, &mut wrong);
        for entry in &mut wrong[before..] {
            write!(entry, " [{case}]").unwrap();
        }
    }
    // The exact URL, alone or beside another entry, is admitted.
    for urls in [
        r#"["https://localhost/example/project.git"]"#,
        r#"["https://localhost/example/other.git","https://localhost/example/project.git"]"#,
    ] {
        let f = Fixture::landed(&landed_urls(Some(urls)), GH_HELPER, |_, _| {});
        let before = wrong.len();
        expect_pass(&f.root, &lines, &mut wrong);
        for entry in &mut wrong[before..] {
            write!(entry, " [list {urls}]").unwrap();
        }
    }
    report(&wrong);
}

/// AC-1 and AC-5: only an `origin` whose transport programs are accounted
/// for is admitted: an `https://` URL, which Git serves with its own HTTPS
/// helper, or a local absolute path. Git starts a remote helper program
/// named after any other scheme, so such a URL keeps the account finding.
/// The rows are payload strings; no transport or helper program runs.
#[test]
fn unaccounted_transports_withhold_the_admission() {
    if !isolated("unaccounted_transports_withhold_the_admission") {
        return;
    }
    let lines = rows(&[
        "git fetch",
        "git fetch origin",
        "git push origin HEAD:refs/heads/task/agent-work",
        "git push origin HEAD",
    ]);
    let mut wrong = Vec::new();
    for url in [
        // Schemes Git hands to a remote helper program it finds by name.
        "reviewproto://localhost/example/project.git",
        "persistent-https://localhost/example/project.git",
        "ftp://localhost/example/project.git",
        "HTTPS://localhost/example/project.git",
        "Https://localhost/example/project.git",
        // Transports that run a program with its own configuration, or
        // that are not the admitted ones.
        "ssh://localhost/example/project.git",
        "git+ssh://localhost/example/project.git",
        "localhost:example/project.git",
        "git://localhost/example/project.git",
        "file:///example/project.git",
        "example/project.git",
        "./project.git",
    ] {
        // The landed list names the URL, so only its transport refuses.
        let f = Fixture::with(GH_HELPER, |repo, _| {
            repo.remote_set_url("origin", url).unwrap();
            reland(
                repo,
                &landed_urls(Some(&serde_json::json!([url]).to_string())),
            );
        });
        let before = wrong.len();
        expect_account(&f.root, &lines, &mut wrong);
        for entry in &mut wrong[before..] {
            write!(entry, " [origin {url}]").unwrap();
        }
    }
    // A local absolute path that the landed `git.agent_remote_urls` names:
    // Git serves it with its own upload and receive programs, whose
    // configured replacements withhold the admission.
    let f = Fixture::with(GH_HELPER, |repo, root| {
        let path = crate::portable_path::slashed(&root.parent().unwrap().join("origin.git"));
        repo.remote_set_url("origin", &path).unwrap();
        let urls = serde_json::json!([path]).to_string();
        reland(repo, &landed_urls(Some(&urls)));
    });
    expect_pass(&f.root, &lines, &mut wrong);
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
            "[credential \"https://localhost\"]\n\thelper =\n\thelper = {helper}\n"
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
            "git clone https://localhost/example/project.git copy",
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
            "[credential \"https://localhost\"]\n\thelper =\n\thelper = {helper}\n"
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

/// `git remote` subcommands that contact a remote, and the other verbs that
/// do, start the credential helper, so they keep the account rule; the
/// admission covers only the fetch and push forms. Before this change the
/// guard read no helper for `git remote`, so `git remote update` passed.
#[test]
fn remote_contacting_verbs_keep_the_account_rule() {
    if !isolated("remote_contacting_verbs_keep_the_account_rule") {
        return;
    }
    let f = Fixture::new(GH_HELPER);
    let mut wrong = Vec::new();
    expect_account(
        &f.root,
        &rows(&[
            "git remote update",
            "git remote update origin",
            "git remote -v update",
            "git remote prune origin",
            "git remote prune -n origin",
            "git remote show origin",
            "git remote set-head origin -a",
            "git remote set-head origin --auto",
        ]),
        &mut wrong,
    );
    expect_refused(
        &f.root,
        &rows(&[
            "git remote frobnicate origin",
            "git remote add -f upstream https://localhost/example/upstream.git",
            "git remote add --fetch upstream https://localhost/example/upstream.git",
            "git maintenance run --task=prefetch",
            "git request-pull HEAD origin",
        ]),
        &mut wrong,
    );
    // `git remote` calls that only read local configuration start no
    // helper and stay ordinary work.
    expect_pass(
        &f.root,
        &rows(&[
            "git remote",
            "git remote -v",
            "git remote get-url origin",
            "git remote get-url --push origin",
            "git remote show -n origin",
        ]),
        &mut wrong,
    );
    report(&wrong);
}

/// The system configuration's `osxkeychain` helper, as Homebrew's Git
/// ships it, and a later empty `helper =` for the host, as
/// `gh auth setup-git` writes it: Git runs the GitHub CLI helper alone, so
/// the admission applies. Before this change the guard did not model the
/// empty value, and the `osxkeychain` body withheld the admission.
#[test]
fn system_helper_cleared_by_a_later_reset_is_admitted() {
    if !isolated_with(
        "system_helper_cleared_by_a_later_reset_is_admitted",
        Some("[credential]\n\thelper = osxkeychain\n"),
    ) {
        return;
    }
    let lines = rows(&[
        "git fetch origin",
        "git push origin HEAD:refs/heads/task/agent-work",
    ]);
    let mut wrong = Vec::new();
    expect_pass(&Fixture::new(GH_HELPER).root, &lines, &mut wrong);
    for config in [
        // A reset for another host clears nothing for `origin`.
        "[credential \"https://gitlab.com\"]\n\thelper =\n[credential \"https://localhost\"]\n\thelper = !/usr/local/bin/gh auth git-credential\n",
        // A helper after the GitHub CLI helper runs too.
        "[credential \"https://localhost\"]\n\thelper =\n\thelper = !/usr/local/bin/gh auth git-credential\n[credential]\n\thelper = store\n",
        // Patterns the model does not read: a wildcard host and a path.
        "[credential \"https://*.localhost\"]\n\thelper =\n[credential \"https://localhost\"]\n\thelper = !/usr/local/bin/gh auth git-credential\n",
        "[credential \"https://localhost/example\"]\n\thelper =\n[credential \"https://localhost\"]\n\thelper = !/usr/local/bin/gh auth git-credential\n",
        // A user in the pattern.
        "[credential \"https://agent@localhost\"]\n\thelper =\n[credential \"https://localhost\"]\n\thelper = !/usr/local/bin/gh auth git-credential\n",
    ] {
        let before = wrong.len();
        expect_account(&Fixture::new(config).root, &lines, &mut wrong);
        for entry in &mut wrong[before..] {
            write!(entry, " [config: {}]", config.replace('\n', " ")).unwrap();
        }
    }
    report(&wrong);
}

/// The system `osxkeychain` helper with no reset after it: Git runs both
/// helpers, so the account rule stands.
#[test]
fn system_helper_without_a_reset_keeps_the_account_rule() {
    if !isolated_with(
        "system_helper_without_a_reset_keeps_the_account_rule",
        Some("[credential]\n\thelper = osxkeychain\n"),
    ) {
        return;
    }
    let f = Fixture::new(
        "[credential \"https://localhost\"]\n\thelper = !/usr/local/bin/gh auth git-credential\n",
    );
    let mut wrong = Vec::new();
    expect_account(
        &f.root,
        &rows(&[
            "git fetch origin",
            "git push origin HEAD:refs/heads/task/agent-work",
        ]),
        &mut wrong,
    );
    report(&wrong);
}

/// A reset that comes before the system `osxkeychain` helper clears
/// nothing after it: Git runs both helpers, so the account rule stands.
#[test]
fn reset_before_the_system_helper_keeps_the_account_rule() {
    if !isolated_with(
        "reset_before_the_system_helper_keeps_the_account_rule",
        Some("[credential \"https://localhost\"]\n\thelper =\n[credential]\n\thelper = osxkeychain\n"),
    ) {
        return;
    }
    let f = Fixture::new(
        "[credential \"https://localhost\"]\n\thelper = !/usr/local/bin/gh auth git-credential\n",
    );
    let mut wrong = Vec::new();
    expect_account(
        &f.root,
        &rows(&[
            "git fetch origin",
            "git push origin HEAD:refs/heads/task/agent-work",
        ]),
        &mut wrong,
    );
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
                    b"[remote \"origin\"]\n\turl = https://localhost/\xff.git\n",
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
        (
            // A bare fetch goes to the current branch's remote, which the
            // guard cannot look up when the branch name is not UTF-8; such a
            // branch is never read as a detached `HEAD`.
            "current branch name that is not UTF-8",
            bare_fetch.as_slice(),
            Box::new(|repo, root| {
                let work = repo.head().unwrap().target().unwrap();
                let mut packed = format!("{work} refs/heads/task/").into_bytes();
                packed.extend_from_slice(b"\xff\n");
                std::fs::write(root.join(".git").join("packed-refs"), packed).unwrap();
                std::fs::write(
                    root.join(".git").join("HEAD"),
                    b"ref: refs/heads/task/\xff\n",
                )
                .unwrap();
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

/// The rule AC-15 adds.
const AGENT_PUSH: &str = "git.agent_push_refused_branches";

/// A credential helper other than the GitHub CLI's.
const KEYCHAIN_HELPER: &str = "[credential]\n\thelper = osxkeychain\n";

/// The landed policy with `git.agent_push_refused_branches` set to `list`,
/// written as JSON.
fn landed_with(list: &str) -> String {
    format!(
        r#"{{"git":{{"protected_branches":["main","master","task/held-*"],"branch_prefixes":["feat/","fix/","task/","integration/","codeflow/","rel/"],"release_branch_pattern":"rel/v*","agent_push_refused_branches":{list}}}}}"#
    )
}

/// git-guard's verdict on `command` run in `cwd`, its context built as the
/// hook builds it: the landed policy, the current branch, and the readers
/// for retargets, aliases and branch expressions.
fn git_guard_verdict(cwd: &Path, command: &str) -> Vec<Violation> {
    use crate::hooks::git_guard::{self, AliasQuery, GuardContext, Retarget};
    let policy = crate::hooks::landed_policy::load_local(cwd)
        .map(|landed| landed.policy.git)
        .unwrap_or_default();
    let repo = crate::hooks::repo::open(cwd).unwrap();
    let branch = repo
        .as_ref()
        .map(|repo| crate::hooks::repo::current_branch(repo).unwrap())
        .unwrap_or_default();
    let common = repo.as_ref().map(|repo| repo.commondir().to_path_buf());
    let target = |spec: &Retarget<'_>| git_guard::read_target(cwd, common.as_deref(), spec);
    let alias = |query: &AliasQuery<'_>| git_guard::read_alias(cwd, query);
    let branch_name =
        |target: Option<&Retarget<'_>>, name: &str| git_guard::read_branch_name(cwd, target, name);
    let ctx = GuardContext {
        policy: &policy,
        current_branch: &branch,
        integrate_token: false,
        pr_base_lookup: None,
        dir_target_lookup: Some(&target),
        alias_lookup: Some(&alias),
        branch_lookup: Some(&branch_name),
        discard_lookup: None,
        root_checkout: None,
    };
    git_guard::evaluate_report_at(command, &ctx, cwd).violations
}

fn agent_push_refused(found: &[Violation]) -> bool {
    found
        .iter()
        .any(|finding| finding.rule == AGENT_PUSH && finding.level == PolicyLevel::Block)
}

/// Each row is refused by exec-guard and by git-guard under AC-15's rule.
fn expect_both_refuse(cwd: &Path, rows: &[String], wrong: &mut Vec<String>) {
    for row in rows {
        for (guard, found) in [
            ("exec-guard", verdict(cwd, row)),
            ("git-guard", git_guard_verdict(cwd, row)),
        ] {
            if !agent_push_refused(&found) {
                wrong.push(format!(
                    "{guard} should refuse under {AGENT_PUSH}: {row}: {}",
                    shown(&found)
                ));
            }
        }
    }
}

/// Neither guard finds AC-15's rule on any row.
fn expect_rule_silent(cwd: &Path, rows: &[String], wrong: &mut Vec<String>) {
    for row in rows {
        for (guard, found) in [
            ("exec-guard", verdict(cwd, row)),
            ("git-guard", git_guard_verdict(cwd, row)),
        ] {
            if found.iter().any(|finding| finding.rule == AGENT_PUSH) {
                wrong.push(format!(
                    "{guard} should not apply {AGENT_PUSH}: {row}: {}",
                    shown(&found)
                ));
            }
        }
    }
}

/// AC-15: an agent push of an integration line or the default branch is
/// refused by both guards in every push form and whatever credential is
/// configured; git-guard passed such a push before this rule. A work branch
/// push draws no finding from the rule.
#[test]
fn agent_pushes_of_lines_and_the_default_branch_are_refused() {
    if !isolated("agent_pushes_of_lines_and_the_default_branch_are_refused") {
        return;
    }
    let mut wrong = Vec::new();
    for helper in ["", KEYCHAIN_HELPER, GH_HELPER] {
        let f = Fixture::new(helper);
        let before = wrong.len();
        expect_both_refuse(
            &f.root,
            &rows(&[
                "git push origin HEAD:refs/heads/integration/EPC-001-line",
                "git push origin HEAD:integration/EPC-001-line",
                "git push -u origin HEAD:refs/heads/integration",
                "git push origin integration/EPC-001-line",
                "git push origin HEAD:heads/integration/EPC-001-line",
                "git push origin :integration/EPC-001-line",
                "git push --delete origin integration/EPC-001-line",
                "git push origin HEAD:refs/heads/main",
                "git push origin HEAD:master",
                "git push origin task/agent-work HEAD:refs/heads/integration/EPC-002-line",
                "git push --all origin",
                "git push --branches origin",
                "git push --mirror origin",
                "git push origin refs/heads/*:refs/heads/*",
                "git push origin :",
                "git push https://localhost/example/project.git HEAD:refs/heads/integration/x",
                "git send-pack origin HEAD:refs/heads/task/agent-work",
                "git http-push https://localhost/example/project.git HEAD",
                "git subtree push -P lib origin task/agent-work",
                "git remote-https origin https://localhost/example/project.git",
            ]),
            &mut wrong,
        );
        expect_both_refuse(
            &f.parent,
            &rows(&["git -C repo push origin HEAD:refs/heads/integration/EPC-001-line"]),
            &mut wrong,
        );
        expect_rule_silent(
            &f.root,
            &rows(&[
                "git push origin HEAD:refs/heads/task/agent-work",
                "git push -u origin HEAD",
                "git push origin task/agent-work",
                "git push origin",
                "git push",
                "git push origin HEAD:refs/heads/feat/integration",
                "git fetch origin",
            ]),
            &mut wrong,
        );
        for entry in &mut wrong[before..] {
            write!(entry, " [helper: {helper:?}]").unwrap();
        }
    }
    report(&wrong);
}

/// AC-15: exec-guard reads a line push behind a spelled or expanded program
/// word as a line push, and still refuses one behind a subcommand it cannot
/// resolve.
#[test]
fn spelled_or_expanded_program_words_still_refuse_a_line_push() {
    if !isolated("spelled_or_expanded_program_words_still_refuse_a_line_push") {
        return;
    }
    let f = Fixture::new("");
    let mut wrong = Vec::new();
    expect_both_refuse(
        &f.root,
        &rows(&[
            "P=git; $P push origin HEAD:refs/heads/integration/EPC-001-line",
            "g\"\"it push origin HEAD:refs/heads/integration/EPC-001-line",
            "\\git push origin HEAD:refs/heads/integration/EPC-001-line",
            "'git' push origin HEAD:refs/heads/integration/EPC-001-line",
            "/usr/bin/git push origin HEAD:refs/heads/integration/EPC-001-line",
            "env git push origin HEAD:refs/heads/integration/EPC-001-line",
            "bash -c 'git push origin HEAD:refs/heads/integration/EPC-001-line'",
            "git -c alias.p=push p origin HEAD:refs/heads/integration/EPC-001-line",
        ]),
        &mut wrong,
    );
    for row in rows(&["git $SUB origin HEAD:refs/heads/integration/EPC-001-line"]) {
        let found = verdict(&f.root, &row);
        if !blocked(&found) {
            wrong.push(format!(
                "exec-guard should refuse: {row}: {}",
                shown(&found)
            ));
        }
    }
    report(&wrong);
}

/// AC-15 with TSK-1490: a line push after a launcher option the guards do
/// not read, or carried by another program's option, is refused by both
/// guards under the rule, with hook integrity at block, at warn and off: an
/// earlier finding on the same command never hides the rule.
#[test]
fn launched_or_carried_line_pushes_still_refuse() {
    if !isolated("launched_or_carried_line_pushes_still_refuse") {
        return;
    }
    let mut wrong = Vec::new();
    for level in ["block", "warn", "off"] {
        let landed = LANDED_POLICY.replacen(
            r#"{"git":{"#,
            &format!(r#"{{"git":{{"hook_integrity":"{level}","#),
            1,
        );
        let f = Fixture::landed(&landed, "", |_, _| {});
        let before = wrong.len();
        expect_both_refuse(
            &f.root,
            &rows(&[
                "taskset --cpu-l 0 git push origin HEAD:refs/heads/integration/EPC-001-line",
                "env --bogus v git push origin HEAD:refs/heads/integration/EPC-001-line",
                "nice -n 5 git push origin HEAD:refs/heads/integration/EPC-001-line",
                "fd -x git push origin HEAD:refs/heads/integration/EPC-001-line",
                "man -P 'git push origin HEAD:refs/heads/integration/EPC-001-line' ls",
            ]),
            &mut wrong,
        );
        for entry in &mut wrong[before..] {
            write!(entry, " [hook integrity: {level}]").unwrap();
        }
    }
    report(&wrong);
}

/// AC-15: a push into another repository whose authority the guard cannot
/// read is refused by exec-guard under the rule, explicit refspecs
/// included, instead of being judged by the session's policy. Here the
/// session's default branch is `main`, and the other repository's is
/// `trunk`, whose landed policy does not parse.
#[test]
fn a_push_into_an_unreadable_repository_is_refused() {
    if !isolated("a_push_into_an_unreadable_repository_is_refused") {
        return;
    }
    let f = Fixture::new("");
    let other = f.parent.join("other");
    let mut options = git2::RepositoryInitOptions::new();
    options.initial_head("trunk");
    let repo = git2::Repository::init_opts(&other, &options).unwrap();
    let trunk = commit(&repo, "{ not json", None);
    repo.reference("refs/heads/trunk", trunk, true, "fixture")
        .unwrap();
    repo.remote("origin", "https://localhost/example/other.git")
        .unwrap();
    repo.reference("refs/remotes/origin/trunk", trunk, true, "fixture")
        .unwrap();
    repo.reference_symbolic(
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/trunk",
        true,
        "fixture",
    )
    .unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    let other = crate::portable_path::slashed(&other);
    let missing = crate::portable_path::slashed(&f.parent.join("missing"));
    let lines: Vec<String> = [
        format!("git -C {other} push origin HEAD:refs/heads/trunk"),
        format!("git -C {other} push origin HEAD:trunk"),
        format!("git -C {other} push origin trunk"),
        format!("git -C {other} push origin HEAD:refs/heads/task/agent-work"),
        format!("git -C {missing} push origin HEAD:refs/heads/task/agent-work"),
    ]
    .into();
    let mut wrong = Vec::new();
    for row in &lines {
        let found = verdict(&f.root, row);
        if !agent_push_refused(&found) {
            wrong.push(format!(
                "exec-guard should refuse under {AGENT_PUSH}: {row}: {}",
                shown(&found)
            ));
        }
        let found = git_guard_verdict(&f.root, row);
        if !blocked(&found) {
            wrong.push(format!("git-guard should refuse: {row}: {}", shown(&found)));
        }
    }
    // A fetch there is not a push, and the session's own push is judged
    // as before.
    expect_rule_silent(
        &f.root,
        &[
            format!("git -C {other} fetch origin"),
            "git push origin HEAD:refs/heads/task/agent-work".to_string(),
        ],
        &mut wrong,
    );
    report(&wrong);
}

/// AC-15: a push whose destination comes from configuration is refused
/// unless the guards read it as the source's own name.
#[test]
fn push_destinations_left_to_configuration() {
    if !isolated("push_destinations_left_to_configuration") {
        return;
    }
    let implicit = rows(&[
        "git push",
        "git push origin",
        "git push origin task/agent-work",
        "git push -u origin HEAD",
    ]);
    let explicit = rows(&["git push origin HEAD:refs/heads/task/agent-work"]);
    let mut wrong = Vec::new();
    for (case, config, refused) in [
        (
            "push.default upstream",
            "[push]\n\tdefault = upstream\n",
            true,
        ),
        (
            "push.default matching",
            "[push]\n\tdefault = matching\n",
            true,
        ),
        (
            "push.default nothing",
            "[push]\n\tdefault = nothing\n",
            true,
        ),
        (
            "remote push mapping",
            "[remote \"origin\"]\n\tpush = refs/heads/*:refs/heads/integration/*\n",
            true,
        ),
        (
            "push.default current",
            "[push]\n\tdefault = current\n",
            false,
        ),
        ("push.default simple", "[push]\n\tdefault = simple\n", false),
        ("unset", "", false),
    ] {
        let f = Fixture::new(config);
        let before = wrong.len();
        if refused {
            expect_both_refuse(&f.root, &implicit, &mut wrong);
        } else {
            expect_rule_silent(&f.root, &implicit, &mut wrong);
        }
        expect_rule_silent(&f.root, &explicit, &mut wrong);
        for entry in &mut wrong[before..] {
            write!(entry, " [{case}]").unwrap();
        }
    }
    let f = Fixture::new("");
    expect_both_refuse(
        &f.root,
        &rows(&[
            "git -c push.default=upstream push",
            "git config push.default upstream && git push",
            "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=push.default GIT_CONFIG_VALUE_0=upstream git push",
        ]),
        &mut wrong,
    );
    report(&wrong);
}

/// AC-15: a project's list replaces the default; an empty list refuses no
/// line; neither removes the floor of `main`, `master` and `origin`'s
/// default branch.
#[test]
fn the_landed_list_replaces_the_default_and_keeps_the_floor() {
    if !isolated("the_landed_list_replaces_the_default_and_keeps_the_floor") {
        return;
    }
    let mut wrong = Vec::new();
    let floor = rows(&[
        "git push origin HEAD:refs/heads/main",
        "git push origin HEAD:refs/heads/master",
        "git push origin :main",
    ]);

    let f = Fixture::landed(&landed_with(r#"["release/*"]"#), "", |_, _| {});
    expect_both_refuse(
        &f.root,
        &rows(&["git push origin HEAD:refs/heads/release/v2"]),
        &mut wrong,
    );
    expect_both_refuse(&f.root, &floor, &mut wrong);
    expect_rule_silent(
        &f.root,
        &rows(&["git push origin HEAD:refs/heads/integration/EPC-001-line"]),
        &mut wrong,
    );

    let f = Fixture::landed(&landed_with("[]"), "", |_, _| {});
    expect_both_refuse(&f.root, &floor, &mut wrong);
    expect_rule_silent(
        &f.root,
        &rows(&[
            "git push origin HEAD:refs/heads/integration/EPC-001-line",
            "git push origin HEAD:refs/heads/integration",
        ]),
        &mut wrong,
    );

    // `origin`'s default branch is `trunk`: the list cannot remove it.
    let f = Fixture::landed(&landed_with("[]"), "", |repo, _| {
        let main = repo
            .find_reference("refs/remotes/origin/main")
            .unwrap()
            .target()
            .unwrap();
        repo.reference("refs/remotes/origin/trunk", main, true, "fixture")
            .unwrap();
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
            true,
            "fixture",
        )
        .unwrap();
    });
    expect_both_refuse(
        &f.root,
        &rows(&[
            "git push origin HEAD:refs/heads/trunk",
            "git push origin :trunk",
        ]),
        &mut wrong,
    );
    expect_both_refuse(&f.root, &floor, &mut wrong);
    report(&wrong);
}

/// AC-15: the list is read only from the landed policy, so an uncommitted
/// edit or a commit on the work branch that empties it has no effect.
#[test]
fn uncommitted_or_branch_local_lists_have_no_effect() {
    if !isolated("uncommitted_or_branch_local_lists_have_no_effect") {
        return;
    }
    let lines = rows(&[
        "git push origin HEAD:refs/heads/integration/EPC-001-line",
        "git push origin HEAD:refs/heads/integration",
    ]);
    let mut wrong = Vec::new();
    let emptied = landed_with("[]");
    let f = Fixture::with("", |_, root| {
        std::fs::write(root.join(".codeflow").join("policy.json"), &emptied).unwrap();
    });
    let before = wrong.len();
    expect_both_refuse(&f.root, &lines, &mut wrong);
    for entry in &mut wrong[before..] {
        write!(entry, " [uncommitted edit]").unwrap();
    }
    let f = Fixture::with("", |repo, _| {
        let head = repo.head().unwrap().target().unwrap();
        let local = commit(repo, &emptied, Some(head));
        repo.reference(&format!("refs/heads/{WORK}"), local, true, "fixture")
            .unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
    });
    let before = wrong.len();
    expect_both_refuse(&f.root, &lines, &mut wrong);
    for entry in &mut wrong[before..] {
        write!(entry, " [branch-local commit]").unwrap();
    }
    report(&wrong);
}

/// AC-15: a value that is not an array of non-empty strings without a space
/// or control character reads as the default list.
#[test]
fn a_malformed_list_reads_as_the_default() {
    if !isolated("a_malformed_list_reads_as_the_default") {
        return;
    }
    let mut wrong = Vec::new();
    for value in [
        r#""release/*""#,
        "[1]",
        r#"[""]"#,
        r#"["release/* integration/*"]"#,
        r#"["release/*\tintegration/*"]"#,
        r#"["release/*", null]"#,
        "null",
        "{}",
        "true",
    ] {
        let f = Fixture::landed(&landed_with(value), "", |_, _| {});
        let before = wrong.len();
        expect_both_refuse(
            &f.root,
            &rows(&[
                "git push origin HEAD:refs/heads/integration/EPC-001-line",
                "git push origin HEAD:refs/heads/integration",
                "git push origin HEAD:refs/heads/main",
            ]),
            &mut wrong,
        );
        expect_rule_silent(
            &f.root,
            &rows(&["git push origin HEAD:refs/heads/release/v2"]),
            &mut wrong,
        );
        for entry in &mut wrong[before..] {
            write!(entry, " [value {value}]").unwrap();
        }
    }
    report(&wrong);
}
