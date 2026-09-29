use super::*;
use std::collections::HashMap;
use std::path::Path;

/// git in a directory, isolated from the host's config.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = crate::git::command()
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repository on `main` with one commit, and its canonical path.
fn repo_with_commit(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "--quiet", "-b", "main"]);
    std::fs::write(dir.join("README.md"), "x\n").unwrap();
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "--quiet", "-m", "init"]);
    dir.canonicalize().unwrap()
}

fn umbrella_policy() -> GitPolicy {
    GitPolicy {
        root_branch: WORKSPACE_ROOT_BRANCH.to_string(),
        ..GitPolicy::default()
    }
}

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    move |name: &str| map.get(name).cloned()
}

fn write_policy(root: &Path, text: &str) {
    std::fs::create_dir_all(root.join(".codeflow")).unwrap();
    std::fs::write(root.join(".codeflow/policy.json"), text).unwrap();
}

fn warnings(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .filter(|f| f.severity == Severity::Warn)
        .map(ToString::to_string)
        .collect()
}

// ---- the convention -------------------------------------------------------

#[test]
fn the_convention_name_is_a_valid_branch_under_the_integration_prefix() {
    assert_eq!(WORKSPACE_ROOT_BRANCH, "integration/workspace");
    assert!(is_valid_branch_name(WORKSPACE_ROOT_BRANCH));
    assert!(GitPolicy::default()
        .branch_prefixes
        .iter()
        .any(|p| WORKSPACE_ROOT_BRANCH.starts_with(p.as_str())));
}

#[test]
fn the_convention_name_appears_in_no_other_source_file() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut offenders = Vec::new();
    let mut stack = vec![src.canonicalize().unwrap()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !path.ends_with("target") {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs")
                && !path.ends_with("root_checkout.rs")
                && !path.ends_with("root_checkout_tests.rs")
                && std::fs::read_to_string(&path).is_ok_and(|t| t.contains(WORKSPACE_ROOT_BRANCH))
            {
                offenders.push(path.display().to_string());
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these source files repeat the convention name instead of reading \
         root_checkout::WORKSPACE_ROOT_BRANCH: {offenders:?}"
    );
}

/// A shipped asset as `codeflow init` renders it.
fn rendered_asset(file: &str) -> String {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(repo.join(file)).unwrap();
    crate::scaffold::init::build_context(
        "demo",
        "",
        &[],
        "rust",
        crate::scaffold::Tier::Minimal,
        "0",
    )
    .substitute(&text)
}

#[test]
fn the_guide_and_the_contract_name_the_constant() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let guide = std::fs::read_to_string(repo.join("docs/workspace-mode.md")).unwrap();
    assert!(
        guide.contains(&format!("`{WORKSPACE_ROOT_BRANCH}`")),
        "the guide must name the convention {WORKSPACE_ROOT_BRANCH}"
    );
    // The shipped rule renders the name from the constant instead of
    // repeating it.
    let raw = std::fs::read_to_string(repo.join("assets/base/rules/worktrees.md")).unwrap();
    assert!(
        !raw.contains(WORKSPACE_ROOT_BRANCH) && raw.contains("{{WORKSPACE_ROOT_BRANCH}}"),
        "worktrees.md must render the convention from the constant"
    );
    assert!(rendered_asset("assets/base/rules/worktrees.md")
        .contains(&format!("`{WORKSPACE_ROOT_BRANCH}`")));
}

#[test]
fn the_guide_and_the_contract_say_how_a_change_lands() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let guide = std::fs::read_to_string(repo.join("docs/workspace-mode.md")).unwrap();
    let worktrees = rendered_asset("assets/base/rules/worktrees.md");
    let flat = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    for (file, text) in [
        ("docs/workspace-mode.md", &guide),
        ("worktrees.md", &worktrees),
    ] {
        let text = flat(text);
        for pin in [
            "a small edit is a commit on `integration/workspace` at the root checkout.",
            "a short-lived branch in the umbrella's own `.worktrees/<slug>`, cut from the root branch \
             and merged back with `codeflow integrate`.",
            "With no remote, the root branch is the landing line and `main` is a protected \
             checkpoint: at a milestone the operator moves it forward with `codeflow integrate \
             integration/workspace --into main`; agents never do.",
            "With a remote the same holds, the root branch is pushed, and a change into `main` \
             is a pull request a human merges.",
            "every change goes through that repository's own flow, a worktree under its own \
             `.worktrees/<slug>` and a pull request into its integration branch or its `main`.",
            "The umbrella never commits nested files, which it ignores, and agents never merge \
             into any repository's `main`.",
        ] {
            assert!(text.contains(pin), "{file} lost the landing rule: {pin}");
        }
    }
    assert!(
        guide.contains("## How a change lands\n\n```text\n"),
        "the guide shows the landing flow as a figure"
    );
    for (file, text) in [
        ("docs/workspace-mode.md", &guide),
        ("worktrees.md", &worktrees),
    ] {
        assert!(
            !text.contains('\u{2014}') && !text.contains('\u{2013}'),
            "{file} uses an em or en dash"
        );
    }
}

#[test]
fn the_contract_every_tier_ships_states_the_rule() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |file: &str| std::fs::read_to_string(repo.join(file)).unwrap();
    let branch_row = "one worktree per session; no task work at the root checkout";
    for file in [
        "assets/base/AGENTS.md.tmpl",
        "assets/base/AGENTS.minimal.md.tmpl",
        "assets/base/AGENTS.full.md.tmpl",
        "assets/base/rule-map.toml",
    ] {
        assert!(
            read(file).contains(branch_row),
            "{file} lost the branch row's rule"
        );
    }
    let worktrees = read("assets/base/rules/worktrees.md");
    for pin in [
        "The root checkout, the repository's main working tree, stays on its root\nbranch and \
         takes no task work.",
        "(`git.root_checkout_commits`)",
        "only warn a human at their own terminal",
        "`git.worktree_locations`",
    ] {
        assert!(worktrees.contains(pin), "worktrees.md lost: {pin}");
    }
    let git_rules = read("assets/base/rules/git-rules.md");
    for pin in [
        "commits at\nthe root checkout off its root branch",
        "**Root checkout:** task work happens in a linked worktree;",
        "is refused; a human at their own terminal is warned.",
    ] {
        assert!(git_rules.contains(pin), "git-rules.md lost: {pin}");
    }
}

// ---- who is acting --------------------------------------------------------

#[test]
fn each_harness_marker_alone_marks_an_agent() {
    for marker in AGENT_MARKERS {
        let env = env_of(&[(marker, "1")]);
        assert_eq!(actor(&env), Actor::Agent(marker), "{marker}");
    }
}

#[test]
fn the_test_environment_blanks_every_harness_marker() {
    // `.cargo/config.toml` blanks each marker, so a test that commits
    // through real hooks does not inherit the agent session running it.
    let config = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cargo/config.toml"),
    )
    .unwrap();
    for marker in AGENT_MARKERS {
        assert!(
            config.contains(&format!("{marker} = {{ value = \"\", force = true }}")),
            "{marker} is not blanked in .cargo/config.toml"
        );
        assert_eq!(std::env::var(marker).unwrap_or_default(), "", "{marker}");
    }
}

#[test]
fn no_marker_or_an_empty_one_is_unmarked() {
    assert_eq!(actor(&env_of(&[])), Actor::Unmarked);
    assert_eq!(actor(&env_of(&[("CLAUDECODE", "")])), Actor::Unmarked);
}

#[test]
fn the_human_override_wins_over_a_marker() {
    let env = env_of(&[("CLAUDECODE", "1"), (HUMAN_OVERRIDE_ENV, "1")]);
    assert_eq!(actor(&env), Actor::HumanOverride);
}

#[test]
fn only_the_value_1_counts_as_the_human_override() {
    // The git layer honours the override only when it is exactly "1"
    // (`human_override_present`); any other value leaves a marked shell an agent.
    for value in ["true", "yes", "0", " 1", ""] {
        let env = env_of(&[("CLAUDECODE", "1"), (HUMAN_OVERRIDE_ENV, value)]);
        assert_eq!(
            actor(&env),
            Actor::Agent("CLAUDECODE"),
            "override value {value:?}"
        );
    }
}

#[test]
fn hooks_block_agents_and_only_warn_everyone_else() {
    assert_eq!(
        hook_level(PolicyLevel::Block, Actor::Agent("CLAUDECODE")),
        PolicyLevel::Block
    );
    assert_eq!(
        hook_level(PolicyLevel::Block, Actor::Unmarked),
        PolicyLevel::Warn
    );
    assert_eq!(
        hook_level(PolicyLevel::Block, Actor::HumanOverride),
        PolicyLevel::Warn
    );
    assert_eq!(
        hook_level(PolicyLevel::Warn, Actor::Agent("CODEX_CI")),
        PolicyLevel::Warn
    );
    assert_eq!(
        hook_level(PolicyLevel::Off, Actor::Unmarked),
        PolicyLevel::Off
    );
}

#[test]
fn the_actor_notes_name_how_the_hook_judged() {
    assert_eq!(
        actor_note(Actor::Agent("GROK_SESSION_ID")),
        "GROK_SESSION_ID is set, so this commit comes from an agent session"
    );
    assert_eq!(
        actor_note(Actor::Unmarked),
        "no harness marker is set, so this commit is treated as a human's and proceeds"
    );
    assert_eq!(
        actor_note(Actor::HumanOverride),
        "CODEFLOW_HUMAN_OVERRIDE is 1, so this commit is treated as a human's and proceeds"
    );
}

// ---- branch names and the root branch --------------------------------------

#[test]
fn branch_names_follow_check_ref_format() {
    for ok in ["main", "feat/x", "integration/workspace", "release/1.2"] {
        assert!(is_valid_branch_name(ok), "{ok}");
    }
    for bad in [
        "", "@", "-x", "/x", "x/", "x.", "a..b", "a//b", "a@{b", "a b", "a~b", "a^b", "a:b", "a?b",
        "a*b", "a[b", "a\\b", ".x", "a/.b", "x.lock", "a/b.lock",
    ] {
        assert!(!is_valid_branch_name(bad), "{bad}");
    }
}

#[test]
fn the_root_branch_comes_from_policy_then_origin_head_then_protected_then_main() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    let repo = git2::Repository::open(&root).unwrap();

    let rb = root_branch(&repo, &umbrella_policy());
    assert_eq!(
        (rb.name.as_str(), rb.source),
        (WORKSPACE_ROOT_BRANCH, RootBranchSource::Policy)
    );

    let rb = root_branch(&repo, &GitPolicy::default());
    assert_eq!(
        (rb.name.as_str(), rb.source),
        ("main", RootBranchSource::ProtectedList)
    );

    let fallback = GitPolicy {
        protected_branches: vec!["release/*".into()],
        ..GitPolicy::default()
    };
    let rb = root_branch(&repo, &fallback);
    assert_eq!(
        (rb.name.as_str(), rb.source),
        ("main", RootBranchSource::Fallback)
    );

    git(&root, &["update-ref", "refs/remotes/origin/trunk", "HEAD"]);
    git(
        &root,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ],
    );
    let rb = root_branch(&repo, &GitPolicy::default());
    assert_eq!(
        (rb.name.as_str(), rb.source),
        ("trunk", RootBranchSource::OriginHead)
    );
}

// ---- commits at the root checkout ------------------------------------------

#[test]
fn a_commit_at_the_root_on_its_root_branch_is_fine() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    assert_eq!(commit_finding(&root, &GitPolicy::default()), None);
}

#[test]
fn a_commit_at_the_root_on_a_feature_branch_is_found_with_its_message() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    let finding = commit_finding(&root, &GitPolicy::default()).expect("a finding");
    assert_eq!(
        finding.message(),
        format!(
            "git.root_checkout_commits: a commit at the root checkout of {} on 'feat/x'; its \
             root branch is 'main' (the default branch, from git.protected_branches)",
            root.display()
        )
    );
    assert_eq!(
        &*finding.next_step(),
        "task work belongs in a linked worktree: put the root checkout back on its root branch \
         with `git switch main`, then work and commit in a worktree (`git worktree add \
         .worktrees/<slug> -b <branch>`, or `git worktree add .worktrees/<slug> <branch>` to \
         continue an existing branch)"
    );
}

#[test]
fn a_commit_at_the_root_on_a_detached_head_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    let sha = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["switch", "--quiet", "--detach"]);
    let finding = commit_finding(&root, &GitPolicy::default()).expect("a finding");
    assert_eq!(finding.head, Head::Detached(sha.chars().take(9).collect()));
    assert!(finding.message().contains("on a detached HEAD at"));
}

#[test]
fn a_commit_in_a_linked_worktree_on_a_feature_branch_is_fine() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(
        &root,
        &["worktree", "add", "--quiet", ".worktrees/t", "-b", "feat/t"],
    );
    assert_eq!(
        commit_finding(&root.join(".worktrees/t"), &GitPolicy::default()),
        None
    );
}

#[test]
fn an_umbrella_root_on_its_root_branch_passes_and_on_a_feature_branch_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("u"));
    git(&root, &["switch", "--quiet", "-c", WORKSPACE_ROOT_BRANCH]);
    assert_eq!(commit_finding(&root, &umbrella_policy()), None);
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    let finding = commit_finding(&root, &umbrella_policy()).expect("a finding");
    assert_eq!(
        finding.message(),
        format!(
            "git.root_checkout_commits: a commit at the root checkout of {} on 'feat/x'; its \
             root branch is 'integration/workspace' (set by git.root_branch)",
            root.display()
        )
    );
}

#[test]
fn a_root_branch_named_main_explicitly_adds_no_finding_on_main() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    let policy = GitPolicy {
        root_branch: "main".into(),
        ..GitPolicy::default()
    };
    assert_eq!(commit_finding(&root, &policy), None);
}

// ---- git-guard (AC-2) --------------------------------------------------------

/// Run git-guard on `command` as a session at `cwd` would: the session's
/// branch, policy and root checkout read from `cwd`, and retargets read
/// from disk.
fn guard(cwd: &Path, policy: &GitPolicy, command: &str) -> Vec<Violation> {
    use crate::hooks::git_guard::{evaluate, read_target, GuardContext, Retarget};
    let repo = git2::Repository::discover(cwd).unwrap();
    let branch = crate::hooks::repo::current_branch(&repo);
    let common = repo.commondir().to_path_buf();
    let root = RootCheckout::at(cwd, policy);
    let dir_target = |spec: &Retarget<'_>| read_target(cwd, Some(&common), spec);
    let ctx = GuardContext {
        policy,
        current_branch: &branch,
        integrate_token: false,
        pr_base_lookup: None,
        dir_target_lookup: Some(&dir_target),
        alias_lookup: None,
        root_checkout: root.as_ref(),
    };
    evaluate(command, &ctx)
}

fn rule_hits(violations: &[Violation]) -> Vec<&Violation> {
    violations
        .iter()
        .filter(|v| v.rule == COMMIT_RULE)
        .collect()
}

#[test]
fn the_guard_refuses_each_commit_creating_command_at_a_root_off_its_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    let policy = GitPolicy::default();
    for command in [
        "git commit -m x",
        "git merge feat/y",
        "git cherry-pick abc123",
        "git revert HEAD",
        "git am patch.mbox",
        "git rebase main",
        "git pull",
    ] {
        let v = guard(&root, &policy, command);
        let hits = rule_hits(&v);
        assert_eq!(hits.len(), 1, "{command}: {v:?}");
        assert_eq!(hits[0].level, PolicyLevel::Block, "{command}");
    }
    let v = guard(&root, &policy, "git commit -m x");
    assert_eq!(
        rule_hits(&v)[0].render("git-guard"),
        format!(
            "codeflow git-guard: BLOCKED \u{2014} policy rule git.root_checkout_commits (block)\n  \
             `git commit` would commit at the root checkout of {} on 'feat/x'; its root branch \
             is 'main' (the default branch, from git.protected_branches)\n  sanctioned: task work \
             belongs in a linked worktree: put the root checkout back on its root branch with \
             `git switch main`, then work and commit in a worktree (`git worktree add \
             .worktrees/<slug> -b <branch>`, or `git worktree add .worktrees/<slug> <branch>` \
             to continue an existing branch)\n  policy file: .codeflow/policy.json",
            root.display()
        )
    );
}

#[test]
fn the_guard_leaves_reads_aborts_and_the_root_branch_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    let umbrella = GitPolicy {
        root_branch: "trunk".into(),
        ..GitPolicy::default()
    };
    git(&root, &["switch", "--quiet", "-c", "trunk"]);
    assert!(rule_hits(&guard(&root, &umbrella, "git commit -m x")).is_empty());
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    for command in [
        "git status",
        "git log --oneline",
        "git merge --abort",
        "git rebase --abort",
        "git cherry-pick --quit",
        "git switch trunk",
    ] {
        assert!(
            rule_hits(&guard(&root, &umbrella, command)).is_empty(),
            "{command}"
        );
    }
}

#[test]
fn an_abort_or_quit_counts_only_as_the_whole_operation() {
    // `--abort` or `--quit` given as a value or next to other arguments is
    // not an operation control: git runs the commit.
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    let policy = GitPolicy::default();
    for command in [
        "git commit -m --abort",
        "git commit -m --quit",
        "git commit --allow-empty -m --abort",
        "git commit --abort",
        "git merge -m --abort feat/y",
        "git pull --abort",
        "git rebase --onto main --quit",
    ] {
        assert_eq!(
            rule_hits(&guard(&root, &policy, command)).len(),
            1,
            "{command}"
        );
    }
    for command in [
        "git merge --abort",
        "git merge --quit",
        "git cherry-pick --abort",
        "git revert --quit",
        "git am --abort",
        "git rebase --quit",
    ] {
        assert!(
            rule_hits(&guard(&root, &policy, command)).is_empty(),
            "{command}"
        );
    }
}

#[test]
fn the_guard_follows_the_level_warn_and_off() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    let warn = GitPolicy {
        root_checkout_commits: PolicyLevel::Warn,
        ..GitPolicy::default()
    };
    let v = guard(&root, &warn, "git commit -m x");
    assert_eq!(rule_hits(&v)[0].level, PolicyLevel::Warn);
    let off = GitPolicy {
        root_checkout_commits: PolicyLevel::Off,
        ..GitPolicy::default()
    };
    assert!(rule_hits(&guard(&root, &off, "git commit -m x")).is_empty());
}

#[test]
fn the_guard_refuses_a_detached_root_and_a_switch_then_commit() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    let policy = GitPolicy::default();
    // On main (the root branch), a line that switches away first is judged
    // on the branch it switched to.
    let v = guard(&root, &policy, "git switch -c feat/x && git commit -m x");
    let hits = rule_hits(&v);
    assert_eq!(hits.len(), 1, "{v:?}");
    assert!(
        hits[0].message.contains("on 'feat/x'"),
        "{}",
        hits[0].message
    );
    git(&root, &["switch", "--quiet", "--detach"]);
    let v = guard(&root, &policy, "git commit -m x");
    let hits = rule_hits(&v);
    assert_eq!(hits.len(), 1, "{v:?}");
    assert!(
        hits[0].message.contains("on a detached HEAD at"),
        "{}",
        hits[0].message
    );
}

#[test]
fn the_guard_judges_git_dash_c_from_a_linked_worktree_by_the_root_it_names() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(
        &root,
        &["worktree", "add", "--quiet", ".worktrees/t", "-b", "feat/t"],
    );
    let worktree = root.join(".worktrees/t");
    let policy = GitPolicy::default();
    // In the worktree itself a commit is fine.
    assert!(rule_hits(&guard(&worktree, &policy, "git commit -m x")).is_empty());
    // Aimed at the root checkout on main, it is fine too; on feat/x it is
    // refused.
    let at_root = format!("git -C {} commit -m x", root.display());
    assert!(rule_hits(&guard(&worktree, &policy, &at_root)).is_empty());
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    let v = guard(&worktree, &policy, &at_root);
    let hits = rule_hits(&v);
    assert_eq!(hits.len(), 1, "{v:?}");
    assert!(
        hits[0]
            .message
            .contains(&format!("root checkout of {} on 'feat/x'", root.display())),
        "{}",
        hits[0].message
    );
}

#[test]
fn the_guard_on_a_protected_root_branch_adds_nothing_to_commit_to_protected() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    for policy in [
        GitPolicy::default(),
        GitPolicy {
            root_branch: "main".into(),
            ..GitPolicy::default()
        },
    ] {
        let v = guard(&root, &policy, "git commit -m x");
        assert!(rule_hits(&v).is_empty(), "{v:?}");
        assert!(
            v.iter().any(|v| v.rule == "git.commit_to_protected"),
            "{v:?}"
        );
    }
}

// ---- the git hooks (AC-3, AC-4) --------------------------------------------

#[test]
fn the_hook_blocks_each_marker_and_warns_without_one_or_with_the_override() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    let policy = GitPolicy::default();
    let head = format!(
        "a commit at the root checkout of {} on 'feat/x'; its root branch is 'main' (the \
         default branch, from git.protected_branches); ",
        root.display()
    );
    for marker in AGENT_MARKERS {
        let v = hook_violation(&root, &policy, &env_of(&[(marker, "1")])).expect(marker);
        assert_eq!(v.level, PolicyLevel::Block, "{marker}");
        assert_eq!(
            v.message,
            format!("{head}{marker} is set, so this commit comes from an agent session")
        );
    }
    let v = hook_violation(&root, &policy, &env_of(&[])).expect("unmarked");
    assert_eq!(v.level, PolicyLevel::Warn);
    assert_eq!(
        v.message,
        format!(
            "{head}no harness marker is set, so this commit is treated as a human's and proceeds"
        )
    );
    let v = hook_violation(
        &root,
        &policy,
        &env_of(&[("CLAUDECODE", "1"), (HUMAN_OVERRIDE_ENV, "1")]),
    )
    .expect("override");
    assert_eq!(v.level, PolicyLevel::Warn);
    assert!(v.message.ends_with(
        "CODEFLOW_HUMAN_OVERRIDE is 1, so this commit is treated as a human's and proceeds"
    ));
    assert!(
        v.remedy.contains("`git switch main`"),
        "{}",
        v.remedy.to_string()
    );
}

#[test]
fn the_hook_is_silent_in_a_worktree_on_the_root_branch_and_when_off() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    let agent = env_of(&[("CLAUDECODE", "1")]);
    assert!(hook_violation(&root, &GitPolicy::default(), &agent).is_none());
    git(
        &root,
        &["worktree", "add", "--quiet", ".worktrees/t", "-b", "feat/t"],
    );
    assert!(hook_violation(&root.join(".worktrees/t"), &GitPolicy::default(), &agent).is_none());
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    let off = GitPolicy {
        root_checkout_commits: PolicyLevel::Off,
        ..GitPolicy::default()
    };
    assert!(hook_violation(&root, &off, &agent).is_none());
}

#[test]
fn a_pre_commit_on_a_protected_root_branch_named_explicitly_keeps_todays_protection() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    let policy = GitPolicy {
        root_branch: "main".into(),
        ..GitPolicy::default()
    };
    let report = crate::hooks::git_hook::pre_commit(&root, &policy, false).unwrap();
    let rules: Vec<&str> = report.violations.iter().map(|v| v.rule.as_str()).collect();
    assert_eq!(rules, ["git.commit_to_protected"]);
    let report = crate::hooks::git_hook::pre_merge_commit(&root, &policy, false, false).unwrap();
    let rules: Vec<&str> = report.violations.iter().map(|v| v.rule.as_str()).collect();
    assert_eq!(rules, ["git.merge_to_protected"]);
}

#[test]
fn the_pre_commit_and_pre_merge_commit_hooks_judge_the_root_checkout() {
    // The hooks read the process environment, which differs between an
    // agent session and CI; either way a commit at the root on a feature
    // branch is a finding, blocked or warned by the actor.
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    let policy = GitPolicy::default();
    let expected = hook_level(PolicyLevel::Block, actor(&process_env));
    for report in [
        crate::hooks::git_hook::pre_commit(&root, &policy, false).unwrap(),
        crate::hooks::git_hook::pre_merge_commit(&root, &policy, false, false).unwrap(),
    ] {
        let hits = rule_hits(&report.violations);
        assert_eq!(hits.len(), 1, "{:?}", report.violations);
        assert_eq!(hits[0].level, expected);
    }
}

// ---- doctor's repo-integrity check (AC-5, AC-12) ----------------------------

fn repo_integrity(dir: &Path) -> crate::doctor::CheckResult {
    let opts = crate::doctor::Options {
        project_dir: dir.to_string_lossy().into_owned(),
        exec_command: Some(|program, args| {
            let out = crate::git::process(program)
                .args(args)
                .output()
                .map_err(|e| e.to_string())?;
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        }),
        env_var: Some(|_| None),
        ..crate::doctor::Options::default()
    };
    crate::doctor::run_check("repo-integrity", &opts).unwrap()
}

#[test]
fn doctor_on_a_single_repository_gains_only_the_root_branch_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(
        &root,
        &["worktree", "add", "--quiet", ".worktrees/t", "-b", "feat/t"],
    );
    std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore worktrees"]);
    let before = "repo layout healthy: not bare, no protected branch in a linked worktree";
    let line = format!(
        "git.root_branch: the root checkout of {} stays on 'main' (the default branch, from \
         git.protected_branches)",
        root.display()
    );
    // The same report from the root and from the linked worktree.
    for at in [root.clone(), root.join(".worktrees/t")] {
        let r = repo_integrity(&at);
        assert_eq!(r.status, crate::doctor::Status::Pass, "{}", r.message);
        assert_eq!(r.message, format!("{before}\n      {line}"));
    }
}

#[test]
fn doctor_warns_for_a_root_off_its_branch_with_the_clearing_step() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    let r = repo_integrity(&root);
    assert_eq!(
        r.status,
        crate::doctor::Status::Warn(crate::remedy::DOCTOR_ROOT_CHECKOUT.remedy())
    );
    assert!(
        r.message.starts_with(
            "repo layout: not bare, no protected branch in a linked worktree; the root \
             checkout needs attention\n      git.root_branch: the root checkout of "
        ),
        "{}",
        r.message
    );
    assert!(
        r.message.contains(&format!(
            "\n      git.root_branch: the root checkout of {} is on 'feat/x'; its root branch \
             is 'main' (the default branch, from git.protected_branches). Next: run git switch \
             main at the root, then git worktree add .worktrees/<slug> feat/x to continue it in \
             a worktree",
            root.display()
        )),
        "{}",
        r.message
    );
}

// ---- nested repositories ---------------------------------------------------

/// An umbrella on the workspace branch holding `plain` (a git repository),
/// `proj` (a `CodeFlow` project) and `group/deep` (two levels down).
fn umbrella(dir: &Path) -> PathBuf {
    let root = repo_with_commit(&dir.join("u"));
    repo_with_commit(&root.join("plain"));
    let proj = repo_with_commit(&root.join("proj"));
    std::fs::create_dir_all(proj.join(".codeflow")).unwrap();
    repo_with_commit(&root.join("group/deep"));
    root
}

fn found(root: &Path) -> Vec<NestedRepo> {
    nested_repositories(&git2::Repository::open(root).unwrap())
}

#[test]
fn nested_repositories_are_found_at_any_depth_and_classified() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    let nested = found(&root);
    let summary: Vec<(&str, NestedKind, &IgnoreState)> = nested
        .iter()
        .map(|n| (n.path.as_str(), n.kind, &n.ignore))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("plain", NestedKind::GitRepository, &IgnoreState::NotIgnored),
            (
                "proj",
                NestedKind::CodeflowProject,
                &IgnoreState::NotIgnored
            ),
            (
                "group/deep",
                NestedKind::GitRepository,
                &IgnoreState::NotIgnored
            ),
        ]
    );
}

#[test]
fn a_tracked_gitignore_entry_marks_a_nested_repository_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    std::fs::write(root.join(".gitignore"), "/plain/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    let plain = found(&root)
        .into_iter()
        .find(|n| n.path == "plain")
        .unwrap();
    assert_eq!(plain.ignore, IgnoreState::Tracked);
}

#[test]
fn an_uncommitted_or_local_exclude_counts_only_locally() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    std::fs::write(root.join(".git/info/exclude"), "/plain/\n").unwrap();
    std::fs::write(root.join(".gitignore"), "/proj/\n").unwrap();
    let nested = found(&root);
    let by = |p: &str| nested.iter().find(|n| n.path == p).unwrap().ignore.clone();
    assert_eq!(
        by("plain"),
        IgnoreState::LocalOnly(".git/info/exclude".into())
    );
    assert_eq!(by("proj"), IgnoreState::LocalOnly(".gitignore".into()));
}

#[test]
fn a_folder_already_ignored_is_not_searched() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("u"));
    repo_with_commit(&root.join("vendor/lib"));
    std::fs::write(root.join(".gitignore"), "/vendor/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    assert!(found(&root).is_empty());
}

#[test]
fn a_parent_ignored_only_locally_is_still_searched() {
    // Only a tracked .gitignore may hide a folder from the search: a local
    // exclude, a global excludes file or an uncommitted .gitignore is not
    // shared, so the nested repository under it is still found.
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global-excludes");
    std::fs::write(&global, "/group/\n").unwrap();
    for (i, source) in [".git/info/exclude", ".gitignore", "global"]
        .into_iter()
        .enumerate()
    {
        let root = repo_with_commit(&dir.path().join(format!("u{i}")));
        repo_with_commit(&root.join("group/nested"));
        if source == "global" {
            git(
                &root,
                &["config", "core.excludesFile", global.to_str().unwrap()],
            );
        } else {
            std::fs::write(root.join(source), "/group/\n").unwrap();
        }
        let nested = found(&root);
        assert_eq!(nested.len(), 1, "{source}: {nested:?}");
        assert_eq!(nested[0].path, "group/nested", "{source}");
        assert!(
            matches!(nested[0].ignore, IgnoreState::LocalOnly(_)),
            "{source}: {:?}",
            nested[0].ignore
        );
    }
}

#[test]
fn the_shared_rule_check_matches_case_as_the_repository_does() {
    // The throwaway repository behind the shared-rule check must match case
    // as this repository does, whatever the file system's default.
    let dir = tempfile::tempdir().unwrap();
    for (i, ignore_case) in ["false", "true"].into_iter().enumerate() {
        let root = repo_with_commit(&dir.path().join(format!("u{i}")));
        git(&root, &["config", "core.ignoreCase", ignore_case]);
        std::fs::write(root.join(".gitignore"), "/NESTED/\n").unwrap();
        git(&root, &["add", ".gitignore"]);
        git(&root, &["commit", "--quiet", "-m", "ignore"]);
        repo_with_commit(&root.join("nested"));
        let nested = found(&root);
        let expected = if ignore_case == "true" {
            IgnoreState::Tracked
        } else {
            IgnoreState::NotIgnored
        };
        assert_eq!(nested[0].ignore, expected, "core.ignoreCase={ignore_case}");
        assert_eq!(git_ignores(&root, "nested"), ignore_case == "true");
    }
}

#[test]
fn finish_on_a_case_sensitive_repository_writes_the_literal_rule() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("u"));
    git(&root, &["config", "core.ignoreCase", "false"]);
    std::fs::write(root.join(".gitignore"), "/NESTED/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore"]);
    repo_with_commit(&root.join("nested"));
    write_policy(&root, "{\n  \"git\": {}\n}\n");
    let report = finish(&root, prepare_branch(&root, &GitPolicy::default()).unwrap()).unwrap();
    assert_eq!(report.ignored.len(), 1, "{report:?}");
    assert!(git_ignores(&root, "nested"));
    let written = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(written.lines().any(|l| l == "/nested/"), "{written}");
    let again = finish(&root, prepare_branch(&root, &umbrella_policy()).unwrap()).unwrap();
    assert!(again.ignored.is_empty(), "{again:?}");
    assert_eq!(
        std::fs::read_to_string(root.join(".gitignore")).unwrap(),
        written
    );
}

/// Whether git ignores `path` in `root` (`git check-ignore`).
fn git_ignores(root: &Path, path: &str) -> bool {
    crate::git::command()
        .args(["check-ignore", "-q", "--no-index", path])
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("git runs")
        .success()
}

#[cfg(unix)]
#[test]
fn ignore_lines_match_the_literal_folder_name_through_git() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("u"));
    let names = [
        "project[1]",
        "a*b",
        "q?x",
        "back\\slash",
        "!bang",
        "#hash",
        "trail ",
    ];
    for name in names {
        repo_with_commit(&root.join(name));
    }
    // Plain folders the glob reading of those names would also match.
    for decoy in ["project1", "aXb", "qZx"] {
        std::fs::create_dir_all(root.join(decoy)).unwrap();
        std::fs::write(root.join(decoy).join("f"), "f\n").unwrap();
    }
    let lines: String = found(&root)
        .iter()
        .map(|n| n.ignore_line() + "\n")
        .collect();
    std::fs::write(root.join(".gitignore"), lines).unwrap();
    for name in names {
        assert!(git_ignores(&root, name), "{name} is not ignored");
    }
    for decoy in ["project1", "aXb", "qZx"] {
        assert!(!git_ignores(&root, decoy), "{decoy} is ignored");
    }
}

#[test]
fn linked_worktrees_and_the_worktrees_folder_are_not_nested_repositories() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("u"));
    git(
        &root,
        &["worktree", "add", "--quiet", ".worktrees/t", "-b", "feat/t"],
    );
    git(
        &root,
        &[
            "worktree",
            "add",
            "--quiet",
            ".claude/worktrees/c",
            "-b",
            "feat/c",
        ],
    );
    assert!(found(&root).is_empty());
}

#[test]
fn a_registered_submodule_stays_tracked_and_unflagged() {
    let dir = tempfile::tempdir().unwrap();
    let lib = repo_with_commit(&dir.path().join("lib"));
    let root = repo_with_commit(&dir.path().join("u"));
    git(
        &root,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "--quiet",
            lib.to_str().unwrap(),
            "libs/lib",
        ],
    );
    git(&root, &["commit", "--quiet", "-m", "add submodule"]);
    let repo = git2::Repository::open(&root).unwrap();
    assert!(nested_repositories(&repo).is_empty());
    let subs = submodule_paths(&repo);
    assert!(stray_gitlinks(&repo, &subs).is_empty());
    let report = doctor_report(&root, &GitPolicy::default(), &env_of(&[]));
    assert!(warnings(&report).is_empty(), "{report:?}");
}

#[test]
fn nested_findings_name_the_kind_the_path_and_the_line_to_add() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    std::fs::write(root.join(".git/info/exclude"), "/plain/\n").unwrap();
    let label = root.display().to_string();
    let lines: Vec<String> = nested_findings(&label, &found(&root), &["tools/x".to_string()])
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        lines,
        vec![
            format!(
                "workspace.nested_repositories: {label} contains the nested git repository \
                 'plain', which only .git/info/exclude ignores; other clones do not share that \
                 file. Next: add the line /plain/ to the tracked .gitignore"
            ),
            format!(
                "workspace.nested_repositories: {label} contains the nested CodeFlow project \
                 'proj', which no tracked .gitignore ignores, so git add . would embed it. \
                 Next: add the line /proj/ to .gitignore (codeflow init --workspace adds one \
                 for every nested repository)"
            ),
            format!(
                "workspace.nested_repositories: {label} contains the nested git repository \
                 'group/deep', which no tracked .gitignore ignores, so git add . would embed \
                 it. Next: add the line /group/deep/ to .gitignore (codeflow init --workspace \
                 adds one for every nested repository)"
            ),
            format!(
                "workspace.nested_repositories: {label} tracks 'tools/x' as a gitlink that \
                 .gitmodules does not register. Next: register it with git submodule add, or \
                 stop tracking it with git rm --cached tools/x and add /tools/x/ to .gitignore"
            ),
        ]
    );
}

#[test]
fn a_gitlink_without_a_submodule_entry_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("u"));
    repo_with_commit(&root.join("embedded"));
    git(&root, &["add", "embedded"]);
    let repo = git2::Repository::open(&root).unwrap();
    assert_eq!(
        stray_gitlinks(&repo, &submodule_paths(&repo)),
        vec!["embedded".to_string()]
    );
}

// ---- worktree locations ----------------------------------------------------

#[test]
fn locations_expand_relative_home_and_harness_homes() {
    let root = Path::new("/r");
    let env = env_of(&[("HOME", "/h")]);
    let ex = |e: &str| expand_location(e, root, &env).unwrap();
    assert_eq!(ex(".worktrees"), PathBuf::from("/r/.worktrees"));
    assert_eq!(ex("~/wt"), PathBuf::from("/h/wt"));
    assert_eq!(
        ex("$CODEX_HOME/worktrees"),
        PathBuf::from("/h/.codex/worktrees")
    );
    assert_eq!(
        ex("$GROK_HOME/worktree_pool"),
        PathBuf::from("/h/.grok/worktree_pool")
    );
    assert_eq!(ex("/abs"), PathBuf::from("/abs"));
    let env = env_of(&[("HOME", "/h"), ("CODEX_HOME", "/c"), ("GROK_HOME", "/g")]);
    assert_eq!(
        expand_location("$CODEX_HOME/worktrees", root, &env),
        Some(PathBuf::from("/c/worktrees"))
    );
    assert_eq!(
        expand_location("$GROK_HOME/worktrees", root, &env),
        Some(PathBuf::from("/g/worktrees"))
    );
}

#[test]
fn only_known_variables_are_valid_locations() {
    for ok in DEFAULT_WORKTREE_LOCATIONS {
        assert!(is_valid_location(ok), "{ok}");
    }
    assert!(!is_valid_location("$HOME/wt"));
    assert!(!is_valid_location(""));
}

#[test]
fn worktrees_in_the_default_locations_pass() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".codex/worktrees")).unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    std::fs::write(
        root.join(".gitignore"),
        "/.worktrees/\n/.claude/worktrees/\n",
    )
    .unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore"]);
    git(
        &root,
        &["worktree", "add", "--quiet", ".worktrees/a", "-b", "feat/a"],
    );
    git(
        &root,
        &[
            "worktree",
            "add",
            "--quiet",
            ".claude/worktrees/b",
            "-b",
            "feat/b",
        ],
    );
    let codex = home.join(".codex/worktrees/c");
    git(
        &root,
        &[
            "worktree",
            "add",
            "--quiet",
            codex.to_str().unwrap(),
            "-b",
            "feat/c",
        ],
    );
    let repo = git2::Repository::open(&root).unwrap();
    let env = env_of(&[("HOME", home.to_str().unwrap())]);
    assert!(worktree_findings(&repo, &GitPolicy::default(), &env).is_empty());
}

#[test]
fn a_sibling_worktree_and_an_unignored_one_inside_the_tree_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    let sibling = dir.path().join("r-feature");
    git(
        &root,
        &[
            "worktree",
            "add",
            "--quiet",
            sibling.to_str().unwrap(),
            "-b",
            "feat/s",
        ],
    );
    git(
        &root,
        &[
            "worktree",
            "add",
            "--quiet",
            ".worktrees/in",
            "-b",
            "feat/in",
        ],
    );
    let repo = git2::Repository::open(&root).unwrap();
    let env = env_of(&[("HOME", "/nonexistent-home")]);
    let lines: Vec<String> = worktree_findings(&repo, &GitPolicy::default(), &env)
        .iter()
        .map(ToString::to_string)
        .collect();
    let sibling = sibling.canonicalize().unwrap();
    assert!(
        lines.contains(&format!(
            "git.worktree_locations: linked worktree {s} of {r} is outside every \
         git.worktree_locations entry. Next: move it with git worktree move {s} \
         .worktrees/<slug>, or add its folder to git.worktree_locations in \
         .codeflow/policy.json",
            s = sibling.display(),
            r = root.display()
        )),
        "{lines:?}"
    );
    assert!(
        lines.contains(&format!(
            "git.worktree_locations: linked worktree '.worktrees/in' sits inside the working tree \
         of {r} and no ignore rule covers it. Next: ignore its folder in .gitignore, as the \
         managed block does for .worktrees/ (for example /.worktrees/in/)",
            r = root.display()
        )),
        "{lines:?}"
    );
}

// ---- doctor's report -------------------------------------------------------

#[test]
fn a_single_repository_on_its_root_branch_gets_only_the_root_branch_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    let report = doctor_report(&root, &GitPolicy::default(), &env_of(&[]));
    assert_eq!(
        report.iter().map(ToString::to_string).collect::<Vec<_>>(),
        vec![format!(
            "git.root_branch: the root checkout of {} stays on 'main' (the default branch, from \
             git.protected_branches)",
            root.display()
        )]
    );
}

#[test]
fn a_root_on_a_feature_branch_with_work_is_reported_with_its_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    std::fs::write(root.join("README.md"), "changed\n").unwrap();
    let report = doctor_report(&root, &GitPolicy::default(), &env_of(&[]));
    assert_eq!(
        warnings(&report),
        vec![format!(
            "git.root_branch: the root checkout of {} is on 'feat/x', with uncommitted changes \
             to 1 tracked file; its root branch is 'main' (the default branch, from \
             git.protected_branches). Next: commit or stash that work, run git switch main at \
             the root, then git worktree add .worktrees/<slug> feat/x to continue it in a \
             worktree, and git stash pop inside it if you stashed",
            root.display()
        )]
    );
}

#[test]
fn a_root_on_a_detached_head_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    git(&root, &["switch", "--quiet", "--detach"]);
    let id = git(&root, &["rev-parse", "--short=9", "HEAD"]);
    let report = doctor_report(&root, &GitPolicy::default(), &env_of(&[]));
    assert_eq!(
        warnings(&report),
        vec![format!(
            "git.root_branch: the root checkout of {} is on a detached HEAD at {id}; its root \
             branch is 'main' (the default branch, from git.protected_branches). Next: run git \
             switch main at the root",
            root.display()
        )]
    );
}

#[test]
fn a_protected_root_holding_task_edits_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    std::fs::write(root.join("README.md"), "changed\n").unwrap();
    let report = doctor_report(&root, &GitPolicy::default(), &env_of(&[]));
    assert_eq!(
        warnings(&report),
        vec![format!(
            "git.root_branch: the root checkout of {} is on its protected root branch 'main' and \
             holds uncommitted changes to 1 tracked file; the root takes no task work. Next: \
             move them to a worktree: git stash, then git worktree add .worktrees/<slug> -b \
             <branch>, then git stash pop inside the worktree",
            root.display()
        )]
    );
}

#[test]
fn an_umbrella_on_its_root_branch_with_ignored_projects_is_healthy() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    std::fs::write(root.join(".gitignore"), "/plain/\n/proj/\n/group/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore nested"]);
    git(&root, &["switch", "--quiet", "-c", WORKSPACE_ROOT_BRANCH]);
    std::fs::write(root.join("README.md"), "live coordination edit\n").unwrap();
    let report = doctor_report(&root, &umbrella_policy(), &env_of(&[]));
    assert!(warnings(&report).is_empty(), "{report:?}");
    assert_eq!(
        report[0].to_string(),
        format!(
            "git.root_branch: workspace mode: the root checkout of {} stays on \
             'integration/workspace' (set by git.root_branch); it holds 2 nested repositories \
             (1 with CodeFlow)",
            root.display()
        )
    );
}

#[test]
fn an_umbrella_switched_to_a_feature_branch_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    std::fs::write(root.join(".gitignore"), "/plain/\n/proj/\n/group/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore nested"]);
    git(&root, &["branch", WORKSPACE_ROOT_BRANCH]);
    git(&root, &["switch", "--quiet", "-c", "feat/x"]);
    let report = doctor_report(&root, &umbrella_policy(), &env_of(&[]));
    assert_eq!(
        warnings(&report),
        vec![format!(
            "git.root_branch: the root checkout of {} is on 'feat/x'; its root branch is \
             'integration/workspace' (set by git.root_branch). Next: run git switch \
             integration/workspace at the root, then git worktree add .worktrees/<slug> feat/x \
             to continue it in a worktree",
            root.display()
        )]
    );
}

#[test]
fn an_umbrella_with_an_unignored_nested_repository_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    std::fs::write(root.join(".gitignore"), "/proj/\n/group/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore some"]);
    git(&root, &["switch", "--quiet", "-c", WORKSPACE_ROOT_BRANCH]);
    let report = doctor_report(&root, &umbrella_policy(), &env_of(&[]));
    let warns = warnings(&report);
    assert_eq!(warns.len(), 1, "{warns:?}");
    assert!(
        warns[0].contains("the nested git repository 'plain'"),
        "{warns:?}"
    );
}

#[test]
fn an_umbrella_without_a_root_branch_gets_the_recommendation() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    std::fs::write(root.join(".gitignore"), "/plain/\n/proj/\n/group/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore nested"]);
    let report = doctor_report(&root, &GitPolicy::default(), &env_of(&[]));
    assert!(warnings(&report).is_empty(), "{report:?}");
    assert_eq!(
        report[1].to_string(),
        format!(
            "git.root_branch: {} contains 2 nested repositories (1 with CodeFlow) (plain, proj) \
             and sets no git.root_branch, so it looks like a workspace; nothing was switched. Next: for an umbrella workspace, run codeflow init --workspace (root \
             branch integration/workspace); a single project can ignore this",
            root.display()
        )
    );
}

#[test]
fn a_root_branch_the_repository_lacks_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("r"));
    let report = doctor_report(&root, &umbrella_policy(), &env_of(&[]));
    let warns = warnings(&report);
    assert!(
        warns.contains(&format!(
            "git.root_branch: git.root_branch names 'integration/workspace', but {} has no local \
         branch 'integration/workspace'. Next: create it from the default branch (codeflow init \
         --workspace does this for an umbrella), or correct git.root_branch in \
         .codeflow/policy.json",
            root.display()
        )),
        "{warns:?}"
    );
}

// ---- init --workspace -------------------------------------------------------

#[test]
fn prepare_branch_creates_the_workspace_branch_from_the_default_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    let step = prepare_branch(&root, &GitPolicy::default()).unwrap();
    assert_eq!(
        step,
        BranchStep::Created {
            branch: WORKSPACE_ROOT_BRANCH.into(),
            from: "main".into()
        }
    );
    assert_eq!(
        git(&root, &["branch", "--show-current"]),
        WORKSPACE_ROOT_BRANCH
    );
    assert_eq!(
        prepare_branch(&root, &GitPolicy::default()).unwrap(),
        BranchStep::AlreadyOn(WORKSPACE_ROOT_BRANCH.into())
    );
}

#[test]
fn prepare_branch_reuses_an_existing_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("u"));
    git(&root, &["branch", WORKSPACE_ROOT_BRANCH]);
    assert_eq!(
        prepare_branch(&root, &GitPolicy::default()).unwrap(),
        BranchStep::Reused(WORKSPACE_ROOT_BRANCH.into())
    );
}

#[test]
fn prepare_branch_refuses_over_uncommitted_changes_and_names_them() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("u"));
    std::fs::write(root.join("README.md"), "changed\n").unwrap();
    let err = prepare_branch(&root, &GitPolicy::default()).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "codeflow init --workspace: codeflow init --workspace would switch the root checkout \
             of {} from 'main' to 'integration/workspace', but these tracked files have \
             uncommitted changes: README.md. Next: commit or stash them, then rerun codeflow \
             init --workspace",
            root.display()
        )
    );
    assert_eq!(git(&root, &["branch", "--show-current"]), "main");
}

#[test]
fn prepare_branch_honours_a_configured_root_branch() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("u"));
    let policy = GitPolicy {
        root_branch: "integration/hub".into(),
        ..GitPolicy::default()
    };
    let step = prepare_branch(&root, &policy).unwrap();
    assert_eq!(step.name(), "integration/hub");
}

#[test]
fn finish_sets_the_key_ignores_every_nested_repository_and_a_rerun_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    write_policy(
        &root,
        "{\n  \"git\": {\n    \"protected_branches\": [\"main\"]\n  }\n}\n",
    );
    std::fs::write(root.join(".gitignore"), "# own\n.DS_Store\n").unwrap();
    let step = prepare_branch(&root, &GitPolicy::default()).unwrap();
    let report = finish(&root, step).unwrap();
    assert!(report.policy_changed);
    assert_eq!(
        report
            .ignored
            .iter()
            .map(|n| n.path.as_str())
            .collect::<Vec<_>>(),
        vec!["plain", "proj", "group/deep"]
    );
    let policy = std::fs::read_to_string(root.join(".codeflow/policy.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&policy).unwrap();
    assert_eq!(parsed["git"]["root_branch"], WORKSPACE_ROOT_BRANCH);
    assert_eq!(
        std::fs::read_to_string(root.join(".gitignore")).unwrap(),
        "# own\n.DS_Store\n\n# nested repositories, each its own git repository (codeflow init \
         --workspace)\n/plain/\n/proj/\n/group/deep/\n"
    );
    git(&root, &["add", ".gitignore", ".codeflow/policy.json"]);
    git(&root, &["commit", "--quiet", "-m", "workspace"]);
    let before = git(&root, &["status", "--porcelain"]);

    let again = finish(&root, prepare_branch(&root, &umbrella_policy()).unwrap()).unwrap();
    assert_eq!(
        again.branch,
        BranchStep::AlreadyOn(WORKSPACE_ROOT_BRANCH.into())
    );
    assert!(!again.policy_changed);
    assert!(again.ignored.is_empty());
    assert_eq!(again.already_ignored.len(), 3);
    assert_eq!(git(&root, &["status", "--porcelain"]), before);
}

#[test]
fn finish_leaves_a_registered_submodule_tracked() {
    let dir = tempfile::tempdir().unwrap();
    let lib = repo_with_commit(&dir.path().join("lib"));
    let root = umbrella(dir.path());
    git(
        &root,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "--quiet",
            lib.to_str().unwrap(),
            "libs/lib",
        ],
    );
    git(&root, &["commit", "--quiet", "-m", "add submodule"]);
    write_policy(&root, "{\n  \"git\": {}\n}\n");
    let report = finish(&root, prepare_branch(&root, &GitPolicy::default()).unwrap()).unwrap();
    assert!(report.ignored.iter().all(|n| n.path != "libs/lib"));
    let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(!ignore.contains("libs/lib"), "{ignore}");
}

#[test]
fn the_report_lists_what_changed_and_the_next_steps() {
    let report = WorkspaceReport {
        branch: BranchStep::Created {
            branch: WORKSPACE_ROOT_BRANCH.into(),
            from: "main".into(),
        },
        policy_changed: true,
        ignored: vec![NestedRepo {
            path: "proj".into(),
            kind: NestedKind::CodeflowProject,
            ignore: IgnoreState::NotIgnored,
        }],
        already_ignored: vec![NestedRepo {
            path: "plain".into(),
            kind: NestedKind::GitRepository,
            ignore: IgnoreState::Tracked,
        }],
    };
    assert_eq!(
        report.to_string(),
        "workspace mode:\n  created 'integration/workspace' from 'main' and switched the root \
         checkout to it\n  set git.root_branch in .codeflow/policy.json\n  ignored the nested \
         CodeFlow project /proj/ in .gitignore\n  the nested git repository 'plain' is already \
         in .gitignore\nnext steps:\n  - commit the files init wrote, including .gitignore and \
         .codeflow/policy.json, on the root branch\n  - bind the nested repositories with the nested-repository inventory once the \
         harness permissions work ships it\n  - main is the milestone checkpoint: at a milestone \
         the operator moves it forward with codeflow integrate integration/workspace --into \
         main; agents never do\n  - do larger or \
         parallel work in a worktree: git worktree add .worktrees/<slug> -b <branch>"
    );
}

#[test]
fn the_policy_key_is_inserted_or_replaced_in_place() {
    let text = "{\n  \"git\": {\n    \"protected_branches\": [\"main\"]\n  }\n}\n";
    let inserted = policy_with_root_branch(text, WORKSPACE_ROOT_BRANCH)
        .unwrap()
        .unwrap();
    assert_eq!(
        inserted,
        "{\n  \"git\": {\n    \"root_branch\": \"integration/workspace\",\n    \
         \"protected_branches\": [\"main\"]\n  }\n}\n"
    );
    assert_eq!(
        policy_with_root_branch(&inserted, WORKSPACE_ROOT_BRANCH),
        Ok(None)
    );
    let replaced = policy_with_root_branch(&inserted, "integration/hub")
        .unwrap()
        .unwrap();
    assert!(replaced.contains("\"root_branch\": \"integration/hub\""));
}

#[test]
fn the_policy_key_is_added_to_a_policy_without_a_git_object() {
    for text in ["{}", "{}\n", "{\n  \"schema_version\": 1\n}\n"] {
        let updated = policy_with_root_branch(text, WORKSPACE_ROOT_BRANCH)
            .unwrap()
            .unwrap_or_else(|| panic!("{text:?} gained no key"));
        let parsed: serde_json::Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(
            parsed["git"]["root_branch"], WORKSPACE_ROOT_BRANCH,
            "{text:?}"
        );
        if text.contains("schema_version") {
            assert_eq!(parsed["schema_version"], 1);
        }
    }
    for text in ["{\"git\": 1}", "not json", "[]"] {
        assert!(
            policy_with_root_branch(text, WORKSPACE_ROOT_BRANCH).is_err(),
            "{text:?}"
        );
    }
}

#[test]
fn finish_on_a_policy_without_a_git_object_sets_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let root = umbrella(dir.path());
    write_policy(&root, "{}");
    let report = finish(&root, prepare_branch(&root, &GitPolicy::default()).unwrap()).unwrap();
    assert!(report.policy_changed);
    let (policy, _) = crate::hooks::policy::Policy::load_effective(&root);
    assert_eq!(policy.git.root_branch, WORKSPACE_ROOT_BRANCH);
}

#[test]
fn finish_makes_an_existing_but_negated_ignore_line_effective() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("u"));
    repo_with_commit(&root.join("nested"));
    std::fs::write(root.join(".gitignore"), "/nested/\n!/nested/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore"]);
    write_policy(&root, "{\n  \"git\": {}\n}\n");
    let report = finish(&root, prepare_branch(&root, &GitPolicy::default()).unwrap()).unwrap();
    assert_eq!(report.ignored.len(), 1, "{report:?}");
    assert!(git_ignores(&root, "nested"));
    let written = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    let again = finish(&root, prepare_branch(&root, &umbrella_policy()).unwrap()).unwrap();
    assert!(again.ignored.is_empty(), "{again:?}");
    assert_eq!(again.already_ignored.len(), 1);
    assert_eq!(
        std::fs::read_to_string(root.join(".gitignore")).unwrap(),
        written
    );
    assert!(git_ignores(&root, "nested"));
}

#[test]
fn finish_ignores_a_nested_repository_under_a_locally_ignored_parent() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_with_commit(&dir.path().join("u"));
    repo_with_commit(&root.join("group/nested"));
    std::fs::write(root.join(".git/info/exclude"), "/group/\n").unwrap();
    write_policy(&root, "{\n  \"git\": {}\n}\n");
    let report = finish(&root, prepare_branch(&root, &GitPolicy::default()).unwrap()).unwrap();
    assert_eq!(
        report
            .ignored
            .iter()
            .map(|n| n.path.as_str())
            .collect::<Vec<_>>(),
        vec!["group/nested"]
    );
    let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(ignore.lines().any(|l| l == "/group/nested/"), "{ignore}");
    // A rerun, before and after the commit, adds nothing.
    let again = finish(&root, prepare_branch(&root, &umbrella_policy()).unwrap()).unwrap();
    assert!(again.ignored.is_empty(), "{again:?}");
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore"]);
    let nested = found(&root);
    assert_eq!(nested[0].ignore, IgnoreState::Tracked, "{nested:?}");
    assert_eq!(
        std::fs::read_to_string(root.join(".gitignore")).unwrap(),
        ignore
    );
}

// ---- plain init and update --------------------------------------------------

#[test]
fn the_workspace_hint_appears_only_for_nested_repositories_without_a_root_branch() {
    let dir = tempfile::tempdir().unwrap();
    let single = repo_with_commit(&dir.path().join("single"));
    assert_eq!(workspace_hint(&single, &GitPolicy::default()), None);
    let root = umbrella(&dir.path().join("x"));
    let hint = workspace_hint(&root, &GitPolicy::default()).expect("a hint");
    assert!(hint
        .message
        .ends_with("so it looks like a workspace; nothing was switched"));
    assert_eq!(workspace_hint(&root, &umbrella_policy()), None);
    assert_eq!(git(&root, &["branch", "--show-current"]), "main");
}

// ---- the policy keys (AC-1) -------------------------------------------------

#[test]
fn the_policy_defaults_leave_a_single_repository_on_its_default_branch() {
    let p = GitPolicy::default();
    assert_eq!(p.root_branch, "");
    assert_eq!(p.root_checkout_commits, PolicyLevel::Block);
    assert_eq!(
        p.worktree_locations,
        DEFAULT_WORKTREE_LOCATIONS.map(String::from).to_vec()
    );
}

#[test]
fn the_shipped_policy_carries_the_three_keys_with_their_defaults() {
    let shipped = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/policy.json"),
    )
    .unwrap();
    crate::hooks::policy_schema::validate_policy_str(&shipped).unwrap();
    let policy: crate::hooks::policy::Policy = serde_json::from_str(&shipped).unwrap();
    let defaults = GitPolicy::default();
    assert_eq!(policy.git.root_branch, defaults.root_branch);
    assert_eq!(
        policy.git.root_checkout_commits,
        defaults.root_checkout_commits
    );
    assert_eq!(policy.git.worktree_locations, defaults.worktree_locations);
}

#[test]
fn an_invalid_root_branch_or_location_is_a_policy_error() {
    use crate::hooks::policy_schema::validate_policy_str;
    let errs = validate_policy_str(r#"{"git":{"root_branch":"feat/../x"}}"#).unwrap_err();
    assert_eq!(errs[0].key, "git.root_branch");
    assert_eq!(
        errs[0].message,
        "invalid value 'feat/../x' for git.root_branch; not a valid branch name (empty means the \
         default branch)"
    );
    let errs = validate_policy_str(r#"{"git":{"worktree_locations":["$HOME/wt"]}}"#).unwrap_err();
    assert_eq!(errs[0].key, "git.worktree_locations");
    validate_policy_str(r#"{"git":{"root_branch":"integration/workspace"}}"#).unwrap();
    validate_policy_str(r#"{"git":{"root_checkout_commits":"warn"}}"#).unwrap();
}

#[test]
fn bootstrap_grace_suspends_the_rule() {
    let mut p = GitPolicy::default();
    p.suspend_for_bootstrap();
    assert_eq!(p.root_checkout_commits, PolicyLevel::Off);
}
