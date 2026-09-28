//! Adopter journeys for TSK-107 (SPC-013 R-82, R-84, R-104, R-115): trusted
//! automation pull requests judged by `codeflow ci` for a trusted, a spoofed
//! and an unknown actor, and kept brownfield PR templates through `init`,
//! `update` and the first PR after install in each mapping state.
//!
//! Every test runs the real binary in a tempdir repository.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn codeflow_cmd(dir: &Path) -> Command {
    let path = test_path();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    cmd.current_dir(dir)
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_ACTOR")
        .env_remove("GITHUB_EVENT_PATH")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("CI_PIPELINE_SOURCE")
        .env_remove("CI_MERGE_REQUEST_IID")
        .env_remove("BITBUCKET_PR_ID")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE");
    cmd
}

fn codeflow(dir: &Path, args: &[&str]) -> Output {
    codeflow_cmd(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("binary runs")
}

fn codeflow_with_stdin(dir: &Path, args: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    let mut child = codeflow_cmd(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().expect("binary exits")
}

fn test_path() -> std::ffi::OsString {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH")
}

/// Git with the binary under test first on `PATH`, so wired hook shims run
/// this build rather than an installed codeflow.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("PATH", test_path())
        .env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("CODEFLOW_INTEGRATE_TOKEN", "test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn shipped_policy() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../assets/base/policy.json")).unwrap()
}

// ---------------------------------------------------------------------------
// Trusted automation (AC-1)
// ---------------------------------------------------------------------------

const RELEASE_IMPACT: &str =
    "- Impact: patch\n- Breaking: no\n- Rationale: dependency update only.\n- Migration: none";

/// The shipped policy plus a Dependabot and a Changesets profile. The
/// Dependabot profile supplies every section except Release impact when
/// `omit_release_impact` is set, so its body misses that section.
fn policy_with_profiles(omit_release_impact: bool) -> String {
    let mut policy = shipped_policy();
    let mut dependabot = serde_json::json!({
        "Summary": "Automated dependency update opened by Dependabot.",
        "Changes": "- the dependency bump named in the title",
        "Testing": "The full CI suite runs on this pull request.",
        "Reviews": "None: automated update, reviewed at merge.",
        "Release impact": RELEASE_IMPACT,
    });
    if omit_release_impact {
        dependabot.as_object_mut().unwrap().remove("Release impact");
    }
    policy["git"]["automation_profiles"] = serde_json::json!([
        {
            "name": "dependabot",
            "actors": ["dependabot[bot]"],
            "branch_pattern": "dependabot/**",
            "sections": dependabot,
        },
        {
            "name": "changesets",
            "actors": ["github-actions[bot]"],
            "branch_pattern": "changeset-release/*",
            "sections": {
                "Summary": "Release pull request opened by the Changesets action.",
                "Changes": "- version bumps and changelog entries listed below",
                "Testing": "The full CI suite runs on this pull request.",
                "Reviews": "None: automated release, reviewed at merge.",
                "Release impact": "- Impact: minor\n- Breaking: no\n- Rationale: the changesets below.\n- Migration: none",
            },
        },
    ]);
    serde_json::to_string_pretty(&policy).unwrap()
}

/// A repository whose `main` carries `policy` and whose `branch` carries one
/// bot commit with `message`.
fn bot_repo(dir: &Path, policy: &str, branch: &str, message: &str) {
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::create_dir_all(dir.join(".codeflow")).unwrap();
    std::fs::write(dir.join(".codeflow/policy.json"), policy).unwrap();
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "chore: init"]);
    git(dir, &["checkout", "-b", branch]);
    std::fs::write(dir.join("Cargo.lock"), "# bumped\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", message]);
}

/// `codeflow ci` as the enforcing workflow runs it: a GitHub Actions
/// `pull_request_target` event whose actor is `actor`, from `head_repo`
/// (the base repository is `acme/app`; another name is a fork).
fn ci_event(dir: &Path, actor: &str, head_repo: &str, branch: &str, body: &str) -> Output {
    let event = dir.join(".git/codeflow-test-event.json");
    std::fs::write(
        &event,
        serde_json::json!({
            "pull_request": {
                "head": { "repo": { "full_name": head_repo } },
                "base": { "repo": { "full_name": "acme/app" } },
            }
        })
        .to_string(),
    )
    .unwrap();
    codeflow_cmd(dir)
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_EVENT_NAME", "pull_request_target")
        .env("GITHUB_EVENT_PATH", &event)
        .env("GITHUB_ACTOR", actor)
        .args([
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            branch,
            "--actor",
            actor,
            "--pr-body",
            body,
        ])
        .stdin(Stdio::null())
        .output()
        .expect("binary runs")
}

/// A same-repository pull request event for `actor`.
fn ci_as(dir: &Path, actor: &str, branch: &str, body: &str) -> Output {
    ci_event(dir, actor, "acme/app", branch, body)
}

/// A Dependabot commit as Dependabot writes it: a sentence-case subject, a
/// prose body, a YAML metadata block and a sign-off trailer.
const DEPENDABOT_COMMIT: &str = "Bump serde from 1.0.219 to 1.0.228\n\nBumps [serde](https://github.com/serde-rs/serde) from 1.0.219 to 1.0.228.\n- [Release notes](https://github.com/serde-rs/serde/releases)\n- [Commits](https://github.com/serde-rs/serde/compare/v1.0.219...v1.0.228)\n\n---\nupdated-dependencies:\n- dependency-name: serde\n  dependency-version: 1.0.228\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>\n";

/// A Dependabot pull request body in Dependabot's shape: no headings, a
/// release-notes details block and the command footer.
const DEPENDABOT_BODY: &str = "Bumps [serde](https://github.com/serde-rs/serde) from 1.0.219 to 1.0.228.\n<details>\n<summary>Release notes</summary>\n<p><em>Sourced from <a href=\"https://github.com/serde-rs/serde/releases\">serde's releases</a>.</em></p>\n<blockquote>\n<h2>v1.0.228</h2>\n<ul>\n<li>Allow building documentation with RUSTDOCFLAGS set</li>\n</ul>\n</blockquote>\n</details>\n<details>\n<summary>Commits</summary>\n<ul>\n<li>See full diff in <a href=\"https://github.com/serde-rs/serde/compare/v1.0.219...v1.0.228\">compare view</a></li>\n</ul>\n</details>\n<br />\n\n[![Dependabot compatibility score](https://dependabot-badges.githubapp.com/badges/compatibility_score?dependency-name=serde&package-manager=cargo&previous-version=1.0.219&new-version=1.0.228)](https://docs.github.com/en/github/managing-security-vulnerabilities/about-dependabot-security-updates#about-compatibility-scores)\n\nDependabot will resolve any conflicts with this PR as long as you don't alter it yourself. You can also trigger a rebase manually by commenting `@dependabot rebase`.\n\n---\n\n<details>\n<summary>Dependabot commands and options</summary>\n<br />\n\nYou can trigger Dependabot actions by commenting on this PR:\n- `@dependabot rebase` will rebase this PR\n- `@dependabot recreate` will recreate this PR, overwriting any edits that have been made to it\n</details>\n";

const DEPENDABOT_BRANCH: &str = "dependabot/cargo/serde-1.0.228";

/// A Changesets release pull request body in the action's shape: an
/// introduction, a `# Releases` heading and one `##` per package.
const CHANGESETS_BODY: &str = "This PR was opened by the [Changesets release](https://github.com/changesets/action) GitHub action. When you're ready to do a release, you can merge this and the packages will be published to npm automatically. If you're not ready to do a release yet, that's fine, whenever you add more changesets to main, this PR will be updated.\n\n\n# Releases\n## @acme/widgets@1.3.0\n\n### Minor Changes\n\n-   3f2a9c1: Add the compact widget variant\n\n### Patch Changes\n\n-   7b1e0d4: Fix focus ring colour in dark mode\n";

#[test]
fn trusted_dependabot_pr_passes_with_profile_sections() {
    let dir = tempfile::tempdir().unwrap();
    bot_repo(
        dir.path(),
        &policy_with_profiles(false),
        DEPENDABOT_BRANCH,
        DEPENDABOT_COMMIT,
    );

    let out = ci_as(
        dir.path(),
        "dependabot[bot]",
        DEPENDABOT_BRANCH,
        DEPENDABOT_BODY,
    );
    let all = text(&out);
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(
        all.contains("automation profile 'dependabot' applies"),
        "{all}"
    );
    assert!(
        all.contains("section '## Summary' supplied by automation profile"),
        "{all}"
    );
    assert!(
        all.contains(
            "level branch_naming = block (configured); skipped by automation profile 'dependabot'"
        ),
        "{all}"
    );
    assert!(
        all.contains("level pr_sections = block (configured)"),
        "{all}"
    );
    assert!(
        all.contains("level ai_attribution = block (configured)"),
        "{all}"
    );
}

#[test]
fn trusted_changesets_pr_passes_with_profile_sections() {
    let dir = tempfile::tempdir().unwrap();
    bot_repo(
        dir.path(),
        &policy_with_profiles(false),
        "changeset-release/main",
        "Version Packages\n",
    );
    let out = ci_as(
        dir.path(),
        "github-actions[bot]",
        "changeset-release/main",
        CHANGESETS_BODY,
    );
    let all = text(&out);
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(
        all.contains("automation profile 'changesets' applies"),
        "{all}"
    );
}

#[test]
fn spoofed_and_unknown_actors_get_no_exemption_or_supplied_content() {
    let dir = tempfile::tempdir().unwrap();
    bot_repo(
        dir.path(),
        &policy_with_profiles(false),
        DEPENDABOT_BRANCH,
        DEPENDABOT_COMMIT,
    );

    for actor in ["mallory", "unknown"] {
        let out = ci_as(dir.path(), actor, DEPENDABOT_BRANCH, DEPENDABOT_BODY);
        let all = text(&out);
        assert_eq!(out.status.code(), Some(1), "{actor}: {all}");
        assert!(
            all.contains(&format!(
                "matches automation profile 'dependabot' but actor '{actor}' is not one of its trusted actors"
            )),
            "{actor}: {all}"
        );
        assert!(all.contains("git.branch_naming"), "{actor}: {all}");
        assert!(all.contains("git.commit_format"), "{actor}: {all}");
        assert!(
            all.contains("missing required section '## Summary'"),
            "{actor}: {all}"
        );
        assert!(
            !all.contains("supplied by automation profile"),
            "{actor}: {all}"
        );
    }

    // A local run passes no actor: it is `unknown`.
    let out = codeflow(
        dir.path(),
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            DEPENDABOT_BRANCH,
        ],
    );
    let all = text(&out);
    assert!(all.contains("actor 'unknown'"), "{all}");
    assert_eq!(out.status.code(), Some(1), "{all}");
}

/// Codex review F2: a trusted name is an identity only in a same-repository
/// pull request event. A local run given the bot's name, and a fork event
/// whose actor carries it, stay `unknown`.
#[test]
fn a_bot_name_is_trusted_only_from_a_same_repository_event() {
    let dir = tempfile::tempdir().unwrap();
    bot_repo(
        dir.path(),
        &policy_with_profiles(false),
        DEPENDABOT_BRANCH,
        DEPENDABOT_COMMIT,
    );
    let local = codeflow(
        dir.path(),
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            DEPENDABOT_BRANCH,
            "--actor",
            "dependabot[bot]",
            "--pr-body",
            DEPENDABOT_BODY,
        ],
    );
    let fork = ci_event(
        dir.path(),
        "dependabot[bot]",
        "mallory/app",
        DEPENDABOT_BRANCH,
        DEPENDABOT_BODY,
    );
    for (what, out, why) in [
        ("local", local, "not a GitHub Actions run"),
        (
            "fork",
            fork,
            "a fork pull request carries no trusted identity",
        ),
    ] {
        let all = text(&out);
        assert_eq!(out.status.code(), Some(1), "{what}: {all}");
        assert!(all.contains("actor 'unknown'"), "{what}: {all}");
        assert!(all.contains(why), "{what}: {all}");
        assert!(!all.contains("applies (actor"), "{what}: {all}");
        assert!(all.contains("git.commit_format"), "{what}: {all}");
        assert!(
            !all.contains("supplied by automation profile"),
            "{what}: {all}"
        );
    }
}

#[test]
fn a_bot_body_missing_a_section_fails_for_every_actor() {
    let dir = tempfile::tempdir().unwrap();
    bot_repo(
        dir.path(),
        &policy_with_profiles(true),
        DEPENDABOT_BRANCH,
        DEPENDABOT_COMMIT,
    );
    for actor in ["dependabot[bot]", "mallory", "unknown"] {
        let out = ci_as(dir.path(), actor, DEPENDABOT_BRANCH, DEPENDABOT_BODY);
        let all = text(&out);
        assert_eq!(out.status.code(), Some(1), "{actor}: {all}");
        assert!(
            all.contains("missing required section '## Release impact'"),
            "{actor}: {all}"
        );
    }
    // Trusted: only the section is judged; naming and format are skipped.
    let out = ci_as(
        dir.path(),
        "dependabot[bot]",
        DEPENDABOT_BRANCH,
        DEPENDABOT_BODY,
    );
    let all = text(&out);
    assert!(!all.contains("git.branch_naming [block]"), "{all}");
    assert!(all.contains("1 blocking"), "{all}");
}

#[test]
fn a_profile_added_by_the_head_itself_is_not_trusted() {
    let dir = tempfile::tempdir().unwrap();
    // The target has no profile; the head adds one for its own actor.
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "t@example.com"]);
    git(dir.path(), &["config", "user.name", "t"]);
    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    std::fs::write(
        dir.path().join(".codeflow/policy.json"),
        serde_json::to_string_pretty(&shipped_policy()).unwrap(),
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "chore: init"]);
    git(dir.path(), &["checkout", "-b", "bot/x"]);
    std::fs::write(
        dir.path().join(".codeflow/policy.json"),
        policy_with_profiles(false)
            .replace("dependabot/**", "bot/*")
            .replace("dependabot[bot]", "mallory"),
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "Widen my own checks"]);
    let out = ci_as(dir.path(), "mallory", "bot/x", DEPENDABOT_BODY);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(1), "{all}");
    assert!(!all.contains("applies (actor"), "{all}");
}

/// Codex review F3: the enforcing job runs from the target checkout. A head
/// whose policy carries a key this binary cannot read fails there with the
/// two-step upgrade order, even though the rules are judged by the target's
/// policy and the head's own workflow no longer validates anything.
#[test]
fn a_head_policy_this_binary_cannot_read_fails_from_the_target_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    std::fs::create_dir_all(root.join(".codeflow")).unwrap();
    std::fs::create_dir_all(root.join(".github/workflows")).unwrap();
    std::fs::write(
        root.join(".codeflow/policy.json"),
        serde_json::to_string_pretty(&shipped_policy()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join(".github/workflows/codeflow-ci.yml"),
        include_str!("../../../assets/base/ci/codeflow-ci.yml"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "chore: init"]);
    git(root, &["checkout", "-b", "feat/unknown-policy"]);
    let mut raised = shipped_policy();
    raised["git"]["future_policy_key"] = serde_json::json!("block");
    std::fs::write(
        root.join(".codeflow/policy.json"),
        serde_json::to_string_pretty(&raised).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join(".github/workflows/codeflow-ci.yml"),
        "name: codeflow-ci\non: pull_request\njobs:\n  gates:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: echo validated\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "feat: adopt a future policy key"]);
    git(root, &["checkout", "main"]);

    let out = codeflow(
        root,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "feat/unknown-policy",
            "--branch",
            "feat/unknown-policy",
            "--pr-body",
            SHIPPED_BODY,
        ],
    );
    let all = text(&out);
    assert_eq!(out.status.code(), Some(2), "{all}");
    assert!(
        all.contains("head policy error: unknown key git.future_policy_key"),
        "{all}"
    );
    assert!(all.contains("Upgrades take two pull requests"), "{all}");

    // The same range with a head the binary reads is judged normally.
    git(root, &["checkout", "feat/unknown-policy"]);
    std::fs::write(
        root.join(".codeflow/policy.json"),
        serde_json::to_string_pretty(&shipped_policy()).unwrap(),
    )
    .unwrap();
    git(root, &["commit", "-am", "fix: drop the future key"]);
    git(root, &["checkout", "main"]);
    let out = codeflow(
        root,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "feat/unknown-policy",
            "--branch",
            "feat/unknown-policy",
            "--pr-body",
            SHIPPED_BODY,
        ],
    );
    let all = text(&out);
    assert_ne!(out.status.code(), Some(2), "{all}");
    assert!(!all.contains("head policy error"), "{all}");
}

#[test]
fn a_planted_secret_is_blocked_on_a_bot_branch() {
    // The profile never reaches the secret scan: the pre-commit scan has no
    // actor at all, and the CI secret-scan job runs for every pull request.
    let dir = tempfile::tempdir().unwrap();
    bot_repo(
        dir.path(),
        &policy_with_profiles(false),
        DEPENDABOT_BRANCH,
        DEPENDABOT_COMMIT,
    );
    std::fs::write(
        dir.path().join("config.rs"),
        // Assembled at run time so the fixture itself is not a staged secret.
        format!("const KEY: &str = \"{}IOSFODNN7EXAMPLQ\";\n", "AKIA"),
    )
    .unwrap();
    git(dir.path(), &["add", "config.rs"]);
    let out = codeflow(dir.path(), &["git-hook", "pre-commit"]);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(1), "{all}");
    assert!(all.contains("git.secret_scan"), "{all}");

    let workflow = include_str!("../../../assets/base/ci/codeflow-ci.yml");
    let secret_job = workflow
        .split("\n  secret-scan:\n")
        .nth(1)
        .and_then(|rest| rest.split("\n\n  ").next())
        .expect("secret-scan job");
    assert!(!secret_job.contains("if:"), "{secret_job}");
    assert!(!secret_job.contains("actor"), "{secret_job}");
}

// ---------------------------------------------------------------------------
// Kept brownfield PR templates (AC-2)
// ---------------------------------------------------------------------------

/// A brownfield template whose headings all have counterparts.
const PROJECT_TEMPLATE: &str = "## Description\n\n<!-- What and why. -->\n\n## Changes\n\n- \n\n## How has this been tested?\n\n## Checklist\n\n- [ ] Docs updated\n\n## Release notes\n";

const TEMPLATE_PATH: &str = ".github/PULL_REQUEST_TEMPLATE.md";

/// A PR body written in the project's template.
const PROJECT_BODY: &str = "## Description\n\nAdds the widget endpoint.\n\n## Changes\n\n- add the endpoint\n\n## How has this been tested?\n\nUnit tests pass locally.\n\n## Checklist\n\nNone: reviewed by the team.\n\n## Release notes\n\n- Impact: minor\n- Breaking: no\n- Rationale: new endpoint.\n- Migration: none\n";

/// A PR body written in `CodeFlow`'s shipped sections.
const SHIPPED_BODY: &str = "## Summary\n\nAdds the widget endpoint.\n\n## Changes\n\n- add the endpoint\n\n## Testing\n\nUnit tests pass locally.\n\n## Reviews\n\nNone: reviewed by the team.\n\n## Release impact\n\n- Impact: minor\n- Breaking: no\n- Rationale: new endpoint.\n- Migration: none\n";

/// An existing repository with a kept template and, when `prior_policy` is
/// given, an existing `CodeFlow` policy file (an upgrade).
fn brownfield(dir: &Path, prior_policy: Option<&str>) {
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::create_dir_all(dir.join(".github")).unwrap();
    std::fs::write(dir.join(TEMPLATE_PATH), PROJECT_TEMPLATE).unwrap();
    std::fs::write(dir.join("app.rs"), "fn main() {}\n").unwrap();
    if let Some(policy) = prior_policy {
        std::fs::create_dir_all(dir.join(".codeflow")).unwrap();
        std::fs::write(dir.join(".codeflow/policy.json"), policy).unwrap();
    }
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "chore: existing project"]);
}

/// Commit the scaffold on a feature branch with one code commit, the shape
/// of the first PR after install.
fn first_pr(dir: &Path) {
    git(dir, &["checkout", "-b", "feat/widget"]);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "chore: adopt codeflow"]);
    std::fs::write(dir.join("widget.rs"), "fn widget() {}\n").unwrap();
    git(dir, &["add", "widget.rs"]);
    git(dir, &["commit", "-m", "feat: add the widget endpoint"]);
}

fn ci_pr(dir: &Path, body: &str) -> Output {
    codeflow(
        dir,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "feat/widget",
            "--pr-body",
            body,
        ],
    )
}

fn policy_value(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join(".codeflow/policy.json")).unwrap())
        .unwrap()
}

#[test]
fn first_install_diagnoses_the_kept_template_and_warns_until_decided() {
    let dir = tempfile::tempdir().unwrap();
    brownfield(dir.path(), None);
    let out = codeflow(dir.path(), &["init", "--minimal", "--yes"]);
    let all = text(&out);
    assert!(out.status.success(), "{all}");
    assert!(
        all.contains("recorded git.pr_section_mapping = diagnosed"),
        "{all}"
    );
    assert!(all.contains("Summary -> Description"), "{all}");
    assert!(all.contains("warn (diagnosed)"), "{all}");
    // No second template: the only template is the project's.
    assert_eq!(
        std::fs::read_to_string(dir.path().join(TEMPLATE_PATH)).unwrap(),
        PROJECT_TEMPLATE
    );
    let policy = policy_value(dir.path());
    assert!(policy["git"].get("pr_sections").is_none());
    assert_eq!(policy["git"]["pr_section_mapping"]["state"], "diagnosed");
    assert_eq!(policy["git"]["pr_section_mapping"]["decided"], "none");

    first_pr(dir.path());
    let out = ci_pr(dir.path(), PROJECT_BODY);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(
        all.contains("level pr_sections = warn (diagnosed)"),
        "{all}"
    );
    assert!(
        all.contains("missing required section '## Summary'"),
        "{all}"
    );
    assert!(
        all.contains("level commit_format = block (configured)"),
        "{all}"
    );

    let doctor = codeflow(dir.path(), &["doctor", "--check", "adopter-fit"]);
    let all = text(&doctor);
    assert!(all.contains("effective level warn (diagnosed)"), "{all}");
    assert!(all.contains("awaits a decision"), "{all}");

    // Repeated updates keep the template and the policy, with no sidecar.
    let before = std::fs::read(dir.path().join(".codeflow/policy.json")).unwrap();
    for _ in 0..2 {
        let out = codeflow(dir.path(), &["update"]);
        let all = text(&out);
        assert_eq!(out.status.code(), Some(0), "{all}");
        assert!(all.contains("git.pr_section_mapping is diagnosed"), "{all}");
    }
    assert_eq!(
        std::fs::read(dir.path().join(".codeflow/policy.json")).unwrap(),
        before
    );
    assert!(!dir
        .path()
        .join(".github/pull_request_template.md.new")
        .exists());
    assert!(!dir.path().join(format!("{TEMPLATE_PATH}.new")).exists());
}

#[test]
fn upgrade_keeps_an_explicit_block_equal_to_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let prior = serde_json::to_string_pretty(&shipped_policy()).unwrap();
    brownfield(dir.path(), Some(&prior));
    let out = codeflow(dir.path(), &["init", "--minimal", "--yes"]);
    let all = text(&out);
    assert!(out.status.success(), "{all}");
    assert!(all.contains("keeps its configured level"), "{all}");
    let after = std::fs::read_to_string(dir.path().join(".codeflow/policy.json")).unwrap();
    let without_mapping: String = after
        .lines()
        .filter(|l| !l.contains("\"pr_section_mapping\""))
        .flat_map(|l| [l, "\n"])
        .collect();
    assert_eq!(
        without_mapping.trim_end(),
        prior.trim_end(),
        "every prior policy byte kept"
    );
    assert_eq!(policy_value(dir.path())["git"]["pr_sections"], "block");

    first_pr(dir.path());
    let out = ci_pr(dir.path(), PROJECT_BODY);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(1), "{all}");
    assert!(
        all.contains("level pr_sections = block (configured)"),
        "{all}"
    );
}

/// Decide interactively at `init` and return the repository.
/// Codex review F1: a `.github` linked outside the repository is never the
/// project's template, and a refusal never appends through the link.
#[cfg(unix)]
#[test]
fn a_template_behind_a_symlinked_github_is_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("external-github");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("PULL_REQUEST_TEMPLATE.md"), PROJECT_TEMPLATE).unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "t"]);
    std::os::unix::fs::symlink("../external-github", repo.join(".github")).unwrap();
    std::fs::write(repo.join("app.rs"), "fn main() {}\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "chore: existing project"]);

    let out = codeflow_with_stdin(&repo, &["init", "--minimal"], "\n\n\nrefuse\n");
    let all = text(&out);
    assert_eq!(
        std::fs::read_to_string(outside.join("PULL_REQUEST_TEMPLATE.md")).unwrap(),
        PROJECT_TEMPLATE,
        "{all}"
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1, "{all}");
    assert!(!all.contains("PR template decision recorded"), "{all}");
    assert!(policy_value(&repo)["git"]
        .get("pr_section_mapping")
        .is_none());
}

/// `text` with the inserted member `key` (and the separator the insertion
/// added) taken out again.
fn without_member(text: &str, key: &str) -> String {
    let name = format!("\"{key}\": ");
    let start = text.find(&name).expect("the member");
    let value = start + name.len();
    let mut stream =
        serde_json::Deserializer::from_str(&text[value..]).into_iter::<serde_json::Value>();
    stream.next().unwrap().unwrap();
    let mut end = value + stream.byte_offset();
    if text[end..].starts_with(',') {
        end += 1 + text[end + 1..].len() - text[end + 1..].trim_start_matches(' ').len();
    }
    let line = text[..start]
        .rfind('\n')
        .filter(|at| text[at + 1..start].trim().is_empty());
    let begin = line.unwrap_or(start);
    format!("{}{}", &text[..begin], &text[end..])
}

/// Codex review F5: a valid sparse policy (no `git` object) gains one for
/// the mapping; nothing else changes and the effective default holds.
#[test]
fn a_sparse_prior_policy_gains_a_git_object_for_the_mapping() {
    for prior in [
        "{}\n",
        "{\"schema_version\": 1}\n",
        "{\n  \"recall\": {\n    \"share\": false\n  }\n}\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        brownfield(dir.path(), Some(prior));
        let out = codeflow(dir.path(), &["init", "--minimal", "--yes"]);
        let all = text(&out);
        assert!(out.status.success(), "{prior}: {all}");
        let before: serde_json::Value = serde_json::from_str(prior).unwrap();
        let after = policy_value(dir.path());
        for (key, value) in before.as_object().unwrap() {
            assert_eq!(&after[key], value, "{prior}: {all}");
        }
        assert_eq!(after["git"]["pr_sections"], "block", "{prior}: {all}");
        assert_eq!(
            after["git"]["pr_section_mapping"]["state"], "diagnosed",
            "{prior}: {all}"
        );
        let text_after = std::fs::read_to_string(dir.path().join(".codeflow/policy.json")).unwrap();
        assert_eq!(without_member(&text_after, "git"), prior, "{text_after}");

        // A later update keeps the recorded decision and every byte.
        let out = codeflow(dir.path(), &["update"]);
        let all = text(&out);
        assert!(out.status.success(), "{prior}: {all}");
        assert_eq!(
            policy_value(dir.path())["git"]["pr_section_mapping"]["state"],
            "diagnosed",
            "{prior}: {all}"
        );
    }
}

/// Codex review F6: an update from an older shipped policy adds the new
/// default keys into the adopter's own bytes (here a compact file), and a
/// kept template's mapping lands the same way; no other byte changes.
#[test]
fn an_old_baseline_update_splices_new_keys_into_the_adopter_bytes() {
    let dir = tempfile::tempdir().unwrap();
    brownfield(dir.path(), None);
    let out = codeflow(dir.path(), &["init", "--minimal", "--yes"]);
    assert!(out.status.success(), "{}", text(&out));
    // Rewind to a release before `automation_profiles`: the shipped
    // baseline and the adopter's file both lack it; the adopter keeps a
    // compact file with its own level, and its template is undecided.
    let old_baseline = {
        let mut v = shipped_policy();
        v["git"]
            .as_object_mut()
            .unwrap()
            .remove("automation_profiles");
        serde_json::to_string_pretty(&v).unwrap()
    };
    std::fs::write(
        dir.path().join(".codeflow/.baseline/.codeflow/policy.json"),
        old_baseline,
    )
    .unwrap();
    let mut adopter = policy_value(dir.path());
    let git_obj = adopter["git"].as_object_mut().unwrap();
    git_obj.remove("automation_profiles");
    git_obj.remove("pr_section_mapping");
    git_obj.insert("pr_sections".to_string(), serde_json::json!("block"));
    git_obj.insert("commit_format".to_string(), serde_json::json!("warn"));
    let compact = format!("{}\n", serde_json::to_string(&adopter).unwrap());
    std::fs::write(dir.path().join(".codeflow/policy.json"), &compact).unwrap();

    let out = codeflow(dir.path(), &["update"]);
    let all = text(&out);
    assert!(out.status.success(), "{all}");
    assert!(all.contains("added key git.automation_profiles"), "{all}");
    let after = std::fs::read_to_string(dir.path().join(".codeflow/policy.json")).unwrap();
    let restored = without_member(
        &without_member(&after, "pr_section_mapping"),
        "automation_profiles",
    );
    assert_eq!(restored, compact, "{after}");
    let value = policy_value(dir.path());
    assert_eq!(value["git"]["automation_profiles"], serde_json::json!([]));
    assert_eq!(value["git"]["commit_format"], "warn");
    assert_eq!(value["git"]["pr_section_mapping"]["state"], "diagnosed");
}

fn decided(answer: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    brownfield(dir.path(), None);
    let out = codeflow_with_stdin(
        dir.path(),
        &["init", "--minimal"],
        &format!("\n\n\n{answer}\n"),
    );
    let all = text(&out);
    assert!(out.status.success(), "{all}");
    assert!(all.contains("PR template decision recorded"), "{all}");
    let policy = policy_value(dir.path());
    assert_ne!(policy["git"]["pr_section_mapping"]["decided"], "none");
    first_pr(dir.path());
    dir
}

#[test]
fn accepted_mapping_checks_the_template_headings_at_block() {
    let dir = decided("accept");
    assert_eq!(
        policy_value(dir.path())["git"]["pr_section_mapping"]["state"],
        "accepted"
    );
    let out = ci_pr(dir.path(), PROJECT_BODY);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(
        all.contains("level pr_sections = block (shipped default)"),
        "{all}"
    );
    let out = ci_pr(dir.path(), "## Description\n\nOnly this.\n");
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
}

#[test]
fn refused_mapping_appends_the_headings_and_checks_the_shipped_ones() {
    let dir = decided("refuse");
    let template = std::fs::read_to_string(dir.path().join(TEMPLATE_PATH)).unwrap();
    for heading in ["Summary", "Reviews", "Release impact", "Testing"] {
        assert!(
            template.contains(&format!("\n## {heading}\n")),
            "{template}"
        );
    }
    let out = ci_pr(dir.path(), SHIPPED_BODY);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let out = ci_pr(dir.path(), PROJECT_BODY);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
}

#[test]
fn custom_mapping_checks_the_policy_list_the_project_sets() {
    let dir = decided("custom");
    assert_eq!(
        policy_value(dir.path())["git"]["pr_section_mapping"]["state"],
        "custom"
    );
    let out = ci_pr(dir.path(), PROJECT_BODY);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    // The reviewed human change: the project's own section list.
    let path = dir.path().join(".codeflow/policy.json");
    let edited = std::fs::read_to_string(&path).unwrap().replace(
        "\"pr_required_sections\": [\"Summary\", \"Changes\", \"Reviews\", \"Release impact\"]",
        "\"pr_required_sections\": [\"Description\", \"Changes\", \"Checklist\", \"Release notes\"]",
    ).replace(
        "\"pr_code_sections\": [\"Testing\"]",
        "\"pr_code_sections\": [\"How has this been tested?\"]",
    );
    std::fs::write(&path, edited).unwrap();
    let out = ci_pr(dir.path(), PROJECT_BODY);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
}

/// SPC-013 R-104: a kept template on upgrade in each mapping state. Repeated
/// updates keep the decision, the policy and the template byte for byte,
/// write no sidecar, and leave the verdict the decision gave.
#[test]
fn each_mapping_state_survives_an_upgrade() {
    for (answer, state, passing, failing) in [
        (
            "accept",
            "accepted",
            PROJECT_BODY,
            "## Description\n\nOnly this.\n",
        ),
        ("refuse", "refused", SHIPPED_BODY, PROJECT_BODY),
        ("custom", "custom", SHIPPED_BODY, PROJECT_BODY),
    ] {
        let dir = decided(answer);
        let policy_path = dir.path().join(".codeflow/policy.json");
        let policy = std::fs::read(&policy_path).unwrap();
        let template = std::fs::read(dir.path().join(TEMPLATE_PATH)).unwrap();
        for _ in 0..2 {
            let out = codeflow(dir.path(), &["update"]);
            let all = text(&out);
            assert_eq!(out.status.code(), Some(0), "{state}: {all}");
            assert!(!all.contains("awaits a decision"), "{state}: {all}");
        }
        assert_eq!(std::fs::read(&policy_path).unwrap(), policy, "{state}");
        assert_eq!(
            std::fs::read(dir.path().join(TEMPLATE_PATH)).unwrap(),
            template,
            "{state}"
        );
        assert!(!dir.path().join(format!("{TEMPLATE_PATH}.new")).exists());
        assert_eq!(
            policy_value(dir.path())["git"]["pr_section_mapping"]["state"],
            state
        );
        let out = ci_pr(dir.path(), passing);
        assert_eq!(out.status.code(), Some(0), "{state}: {}", text(&out));
        let out = ci_pr(dir.path(), failing);
        assert_eq!(out.status.code(), Some(1), "{state}: {}", text(&out));
    }
}

#[test]
fn update_installs_the_enforcing_workflow_for_an_existing_adopter() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "t@example.com"]);
    git(dir.path(), &["config", "user.name", "t"]);
    std::fs::write(dir.path().join("app.rs"), "fn main() {}\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "chore: existing project"]);
    assert!(codeflow(dir.path(), &["init", "--minimal", "--yes"])
        .status
        .success());
    // An adopter from before the enforcing workflow existed.
    std::fs::remove_file(dir.path().join(".github/workflows/codeflow-policy.yml")).unwrap();
    let out = codeflow(dir.path(), &["update"]);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(0), "{all}");
    let policy =
        std::fs::read_to_string(dir.path().join(".github/workflows/codeflow-policy.yml")).unwrap();
    assert!(policy.contains("pull_request_target:"));
    let ci = std::fs::read_to_string(dir.path().join(".github/workflows/codeflow-ci.yml")).unwrap();
    assert!(!ci.contains("name: commit standards"));
}

// ---------------------------------------------------------------------------
// Record creation below full tier and project templates (AC-3, AC-10)
// ---------------------------------------------------------------------------

fn existing_repo(dir: &Path) {
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("app.rs"), "fn main() {}\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "chore: existing project"]);
}

fn created(out: &Output) -> PathBuf {
    let all = text(out);
    assert_eq!(out.status.code(), Some(0), "{all}");
    PathBuf::from(
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .nth(1)
            .expect("the created path"),
    )
}

#[test]
fn records_are_created_at_the_minimal_and_standard_tiers() {
    for tier in ["--minimal", "--standard"] {
        let dir = tempfile::tempdir().unwrap();
        existing_repo(dir.path());
        assert!(codeflow(dir.path(), &["init", tier, "--yes"])
            .status
            .success());
        created(&codeflow(dir.path(), &["epic", "new", "first epic"]));
        created(&codeflow(
            dir.path(),
            &["task", "new", "--epic", "EPC-001", "first task"],
        ));
        created(&codeflow(
            dir.path(),
            &["spec", "new", "--for", "TSK-001", "first spec"],
        ));
        let out = codeflow(dir.path(), &["validate", "--docs"]);
        assert_eq!(out.status.code(), Some(0), "{tier}: {}", text(&out));
        assert!(
            text(&out).contains("3 record(s) clean"),
            "{tier}: {}",
            text(&out)
        );
    }
}

#[test]
fn project_templates_are_used_when_valid_and_fall_back_when_not() {
    let dir = tempfile::tempdir().unwrap();
    existing_repo(dir.path());
    assert!(codeflow(dir.path(), &["init", "--minimal", "--yes"])
        .status
        .success());
    assert_eq!(policy_value(dir.path())["git"]["work_records"], "block");
    let templates = dir.path().join("project-management/templates");
    std::fs::create_dir_all(&templates).unwrap();
    let shipped = include_str!("../../../assets/base/pm/task.md.tmpl");
    let custom = shipped.replace(
        "## Description\n",
        "## Description\n\nTeam rule: link the ticket in external_refs.\n",
    );
    assert_ne!(custom, shipped);
    std::fs::write(templates.join("task.md"), &custom).unwrap();
    created(&codeflow(dir.path(), &["epic", "new", "first epic"]));
    let path = created(&codeflow(
        dir.path(),
        &["task", "new", "--epic", "EPC-001", "custom task"],
    ));
    let record = std::fs::read_to_string(path).unwrap();
    assert!(record.contains("Team rule: link the ticket"), "{record}");
    // Rendered from the project template, the record passes the validator
    // with the work-record rules at block.
    let out = codeflow(dir.path(), &["validate", "--docs"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));

    // A template that cannot produce a valid record is refused by name and
    // the embedded template is used.
    std::fs::write(templates.join("task.md"), "# my task\n").unwrap();
    let out = codeflow(
        dir.path(),
        &["task", "new", "--epic", "EPC-001", "fallback task"],
    );
    let all = text(&out);
    assert!(all.contains("not a usable task template"), "{all}");
    let record = std::fs::read_to_string(created(&out)).unwrap();
    assert!(record.contains("## Acceptance Criteria"), "{record}");
    let out = codeflow(dir.path(), &["validate", "--docs"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));

    // Codex review F4: placeholders kept only in comments beside fixed
    // values would write the wrong target or id; the template falls back.
    git(dir.path(), &["branch", "integration/EPC-001-custom"]);
    for broken in [
        shipped.replace(
            "integration_target: {{TARGET_BRANCH}}",
            "integration_target: main # {{TARGET_BRANCH}}",
        ),
        shipped.replace("id: TSK-{{NNN}}", "id: TSK-999 # {{NNN}}"),
    ] {
        std::fs::write(templates.join("task.md"), &broken).unwrap();
        let out = codeflow(
            dir.path(),
            &[
                "task",
                "new",
                "--standalone-reason",
                "bounded review fixture",
                "--into",
                "integration/EPC-001-custom",
                "custom target",
            ],
        );
        let all = text(&out);
        assert!(all.contains("not a usable task template"), "{all}");
        let path = created(&out);
        let record = std::fs::read_to_string(&path).unwrap();
        assert!(
            record.contains("integration_target: \"integration/EPC-001-custom\""),
            "{record}"
        );
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        assert!(record.contains(&format!("id: {stem}\n")), "{record}");
        let out = codeflow(dir.path(), &["validate", "--docs"]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    }
}
