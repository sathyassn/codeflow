//! End-to-end CLI tests for `codeflow recall` and `codeflow remote protect`
//! in tempdir repos with an isolated `CODEFLOW_HOME`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn codeflow() -> Command {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
}

fn run_in(dir: &Path, home: &Path, args: &[&str]) -> Output {
    codeflow()
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", home)
        .output()
        .expect("codeflow binary runs")
}

/// Initialized repo with a fake runtime state dir (no git binary needed).
fn init_repo(root: &Path, name: &str) {
    fs::create_dir_all(root.join(".codeflow")).unwrap();
    fs::write(
        root.join(".codeflow/policy.json"),
        r#"{ "schema_version": 1, "git": { "protected_branches": ["main", "release/*"] } }"#,
    )
    .unwrap();
    fs::write(
        root.join(".codeflow/project.toml"),
        format!("name = \"{name}\"\ntier = \"standard\"\nscaffold_version = \"2.0.0-dev\"\n"),
    )
    .unwrap();
    fs::create_dir_all(root.join(".git")).unwrap();
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn recall_all_answers_why_question_across_two_repos() {
    let home = tempfile::tempdir().unwrap();
    let repo_a = tempfile::tempdir().unwrap();
    let repo_b = tempfile::tempdir().unwrap();
    init_repo(repo_a.path(), "proj-a");
    init_repo(repo_b.path(), "proj-b");

    // Repo A: an ADR holding the "why".
    let adr_dir = repo_a.path().join("docs/decisions");
    fs::create_dir_all(&adr_dir).unwrap();
    fs::write(
        adr_dir.join("ADR-0003-recall-engine.md"),
        "---\ntitle: SQLite FTS5 for recall\nstatus: accepted\n---\n\n\
         # SQLite FTS5 for recall\n\nDecision: bundled rusqlite, no system \
         dependency, index as rebuildable cache.\n",
    )
    .unwrap();

    // Repo B: a session summary in the memory-events ledger.
    let ledger = repo_b.path().join(".git/codeflow/ledger/memory-events");
    fs::create_dir_all(&ledger).unwrap();
    fs::write(
        ledger.join("memory-events.jsonl"),
        "{\"event\":\"session_summary\",\"timestamp\":\"2026-06-11T09:00:00Z\",\
         \"summary\":\"Chose SQLite FTS5 for recall; bundled build avoids system deps.\"}\n",
    )
    .unwrap();

    // Any command inside each repo registers it (touch_registry in dispatch).
    let out_a = run_in(repo_a.path(), home.path(), &["recall", "registerme"]);
    assert!(
        out_a.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&out_a.stderr)
    );
    let out_b = run_in(repo_b.path(), home.path(), &["recall", "registerme"]);
    assert!(out_b.status.success());

    // Cross-repo why-question from a neutral directory (AC #10).
    let neutral = tempfile::tempdir().unwrap();
    let out = run_in(
        neutral.path(),
        home.path(),
        &[
            "recall",
            "--all",
            "why did we choose SQLite FTS5 for recall",
        ],
    );
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("[proj-a]"), "missing repo-a hit:\n{text}");
    assert!(text.contains("[proj-b]"), "missing repo-b hit:\n{text}");
    assert!(
        text.contains("docs/decisions/ADR-0003-recall-engine.md"),
        "repository-relative output must remain portable:\n{text}"
    );
    assert!(
        text.contains("(session)"),
        "session summary should surface:\n{text}"
    );
}

#[test]
fn recall_outside_repo_without_all_fails_with_guidance() {
    let home = tempfile::tempdir().unwrap();
    let neutral = tempfile::tempdir().unwrap();
    let out = run_in(neutral.path(), home.path(), &["recall", "anything"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--all"), "guidance expected, got: {err}");
}

#[test]
fn recall_discloses_unsearched_registered_repos() {
    let home = tempfile::tempdir().unwrap();
    let repo_a = tempfile::tempdir().unwrap();
    let repo_b = tempfile::tempdir().unwrap();
    init_repo(repo_a.path(), "here");
    init_repo(repo_b.path(), "elsewhere");

    run_in(repo_b.path(), home.path(), &["recall", "warmup"]);
    let out = run_in(repo_a.path(), home.path(), &["recall", "warmup"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("not searched — use --all"),
        "coverage note expected:\n{text}"
    );
}

#[test]
fn remote_protect_dry_run_prints_plan_from_policy() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path(), "planned");

    let out = run_in(
        repo.path(),
        home.path(),
        &["remote", "protect", "--dry-run"],
    );
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("main [branch protection]:"), "{text}");
    assert!(
        text.contains("release/* [ruleset (glob pattern)]:"),
        "{text}"
    );
    assert!(
        text.contains("require a pull request before merging"),
        "{text}"
    );
    // Without git.required_checks the shipped CI job names are required, on
    // a branch that is up to date with its base.
    assert!(
        text.contains(
            "require status checks to pass on a branch that is up to date with its base: \
             codeflow gates, secret scan, security review, commit standards"
        ),
        "{text}"
    );
    assert!(text.contains("status: dry-run"), "{text}");

    fs::write(
        repo.path().join(".codeflow/policy.json"),
        r#"{ "schema_version": 1, "git": { "protected_branches": ["main"], "required_checks": ["build", "windows"] } }"#,
    )
    .unwrap();
    let text = stdout(&run_in(
        repo.path(),
        home.path(),
        &["remote", "protect", "--dry-run"],
    ));
    assert!(
        text.contains("up to date with its base: build, windows"),
        "{text}"
    );
}

/// The doctor reads the default branch's live rules through `gh` and warns
/// when they do not require an up-to-date branch (TSK-261).
#[cfg(unix)]
#[test]
fn doctor_warns_when_the_default_branch_does_not_require_up_to_date_checks() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path(), "racy");
    // A git directory libgit2 opens, with a GitHub origin; no git binary.
    let git = repo.path().join(".git");
    fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::create_dir_all(git.join("objects")).unwrap();
    fs::create_dir_all(git.join("refs/heads")).unwrap();
    fs::write(
        git.join("config"),
        "[core]\n\trepositoryformatversion = 0\n\tbare = false\n[remote \"origin\"]\n\turl = https://github.com/o/r.git\n",
    )
    .unwrap();

    let shims = tempfile::tempdir().unwrap();
    let gh = shims.path().join("gh");
    fs::write(
        &gh,
        r#"#!/bin/sh
case "$*" in
  "api repos/o/r") printf '%s' '{"default_branch":"main"}' ;;
  "api repos/o/r/rules/branches/main") printf '%s' '[{"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":false,"required_status_checks":[{"context":"codeflow gates","integration_id":15368},{"context":"secret scan","integration_id":15368},{"context":"security review","integration_id":15368},{"context":"commit standards","integration_id":15368}]},"ruleset_id":7}]' ;;
  "api repos/o/r/rulesets/7") printf '%s' '{"id":7,"bypass_actors":[]}' ;;
  *) echo 'gh: Branch not protected (HTTP 404)' >&2; exit 1 ;;
esac
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&gh).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&gh, perms).unwrap();
    let path = format!(
        "{}:{}",
        shims.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let out = codeflow()
        .args(["doctor", "--check", "remote-perimeter"])
        .current_dir(repo.path())
        .env("CODEFLOW_HOME", home.path())
        .env("PATH", path)
        .output()
        .unwrap();
    let text = stdout(&out);
    assert!(out.status.success(), "a warning never fails doctor: {text}");
    assert!(
        text.contains(
            "warn  remote-perimeter: o/r: main requires status checks but not that a branch be up to date (ruleset 7)"
        ),
        "{text}"
    );
    assert!(
        text.contains("Require branches to be up to date before merging"),
        "{text}"
    );
}

#[cfg(unix)]
#[test]
fn remote_protect_degrades_legibly_on_free_plan_403() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path(), "private-free");

    // gh shim on PATH: private repo, every api call hits the Free-plan wall.
    let shims = tempfile::tempdir().unwrap();
    let gh = shims.path().join("gh");
    fs::write(
        &gh,
        "#!/bin/sh\n\
         if [ \"$1\" = \"repo\" ]; then\n\
           printf '%s' '{\"nameWithOwner\":\"sathyassn/private-free\",\"isPrivate\":true}'\n\
           exit 0\n\
         fi\n\
         echo 'HTTP 403: Upgrade to GitHub Pro or make this repository public to enable this feature.' >&2\n\
         exit 1\n",
    )
    .unwrap();
    let mut perms = fs::metadata(&gh).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&gh, perms).unwrap();

    let path = format!(
        "{}:{}",
        shims.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = codeflow()
        .args(["remote", "protect"])
        .current_dir(repo.path())
        .env("CODEFLOW_HOME", home.path())
        .env("PATH", path)
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "degraded must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    assert!(text.contains("Upgrade to GitHub Pro"), "{text}");
    assert!(text.contains("Manual checklist"), "{text}");
    assert!(
        text.contains("[ ] main: require a pull request before merging"),
        "{text}"
    );
    assert!(text.contains("status: degraded"), "{text}");
}

#[test]
fn any_command_touches_registry() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path(), "touched");

    let out = run_in(
        repo.path(),
        home.path(),
        &["remote", "protect", "--dry-run"],
    );
    assert!(out.status.success());

    let registry = fs::read_to_string(home.path().join("registry.json")).unwrap();
    assert!(registry.contains("\"touched\""), "registry: {registry}");
    assert!(
        registry.contains("\"tier\": \"standard\""),
        "registry: {registry}"
    );
}

/// TSK-137 AC-4: hook, ci and read-only commands leave the registry alone,
/// other commands still record the repo, and a home the process may not
/// write (a sandbox, a read-only directory) is silent.
#[cfg(unix)]
#[test]
fn registry_touch_skips_hooks_and_is_silent_in_a_read_only_home() {
    use std::os::unix::fs::PermissionsExt;

    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path(), "proj");
    let home = tempfile::tempdir().unwrap();
    let registry = home.path().join("registry.json");

    for args in [
        &["git-hook", "commit-msg", "missing-file"][..],
        &["ci", "--base", "HEAD", "--head", "HEAD"],
        &["validate"],
        &["work", "start", "TSK-001"],
    ] {
        run_in(repo.path(), home.path(), args);
        assert!(!registry.exists(), "{args:?} touched the registry");
    }
    // The session-start orient is the main sign a repository is in use.
    run_in(repo.path(), home.path(), &["orient"]);
    assert!(registry.exists(), "orient records the repo");

    let locked = tempfile::tempdir().unwrap();
    let read_only = locked.path().join("home");
    fs::create_dir_all(&read_only).unwrap();
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o555)).unwrap();
    let out = run_in(repo.path(), &read_only, &["recall", "anything"]);
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o755)).unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("registry touch failed"), "{stderr}");
    assert!(!read_only.join("registry.json").exists());

    // A home used before keeps a writable lock; the denial then comes from
    // the atomic registry write, and is just as quiet (T137-4).
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o555)).unwrap();
    assert!(home.path().join("registry.json.lock").exists());
    let before = fs::read_to_string(&registry).unwrap();
    let out = run_in(repo.path(), home.path(), &["recall", "anything"]);
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("registry touch failed"), "{stderr}");
    assert_eq!(fs::read_to_string(&registry).unwrap(), before);
}
