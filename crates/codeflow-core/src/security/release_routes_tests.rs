//! TSK-1346: release routes through `gh`, the API and `curl` are judged as
//! strings against a fixture repository; nothing is dispatched or sent.
use super::super::outward::evaluate_at;
use crate::hooks::policy::SecuritySection;
use std::path::Path;

const RELEASE: &str = "name: Release
permissions:
  contents: write
on:
  pull_request:
  workflow_dispatch:
    inputs:
      tag:
        default: dry-run
jobs:
  plan:
    runs-on: ubuntu-latest
    steps:
      - run: echo plan
";

const CI: &str = "name: codeflow-ci
on:
  push:
  workflow_dispatch:
permissions:
  contents: read
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: cargo test
";

const LISTENER: &str = "name: ship
on:
  repository_dispatch:
    types: [ship]
jobs:
  ship:
    runs-on: ubuntu-latest
    environment: release
    steps:
      - run: echo ship
";

const PAGES: &str = "name: pages
on: [workflow_dispatch]
jobs:
  deploy:
    permissions:
      pages: write
      id-token: write
    runs-on: ubuntu-latest
    steps:
      - run: echo deploy
";

const CALLED: &str = "name: called
on:
  workflow_call:
permissions:
  contents: write
jobs: {}
";

/// A repository whose `origin` default branch holds `files`, and whose
/// tracking ref for `feat/x` holds `branch_files`.
fn fixture(files: &[(&str, &str)], branch_files: &[(&str, &str)]) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(temp.path()).unwrap();
    repo.remote("origin", "https://github.com/Acme/widget.git")
        .unwrap();
    let commit = |files: &[(&str, &str)]| {
        let mut flows = repo.treebuilder(None).unwrap();
        for (name, text) in files {
            let blob = repo.blob(text.as_bytes()).unwrap();
            flows.insert(name, blob, 0o100_644).unwrap();
        }
        let flows = flows.write().unwrap();
        let mut github = repo.treebuilder(None).unwrap();
        github.insert("workflows", flows, 0o040_000).unwrap();
        let github = github.write().unwrap();
        let mut root = repo.treebuilder(None).unwrap();
        root.insert(".github", github, 0o040_000).unwrap();
        let tree = repo.find_tree(root.write().unwrap()).unwrap();
        let who = git2::Signature::now("t", "t@example.invalid").unwrap();
        repo.commit(None, &who, &who, "chore: fixture", &tree, &[])
            .unwrap()
    };
    let main = commit(files);
    let branch = commit(branch_files);
    repo.reference("refs/remotes/origin/main", main, true, "fixture")
        .unwrap();
    repo.reference("refs/remotes/origin/feat/x", branch, true, "fixture")
        .unwrap();
    repo.reference_symbolic(
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
        true,
        "fixture",
    )
    .unwrap();
    repo.reference("refs/heads/main", main, true, "fixture")
        .unwrap();
    repo.set_head("refs/heads/main").unwrap();
    temp
}

fn standard() -> tempfile::TempDir {
    fixture(
        &[
            ("release.yml", RELEASE),
            ("codeflow-ci.yml", CI),
            ("ship.yml", LISTENER),
            ("pages.yml", PAGES),
            ("called.yml", CALLED),
        ],
        &[("release.yml", RELEASE), ("codeflow-ci.yml", RELEASE)],
    )
}

fn refused(dir: &Path, command: &str) -> Option<String> {
    let found = evaluate_at(command, &SecuritySection::default(), Some(dir));
    let messages: Vec<_> = found
        .iter()
        .filter(|v| v.rule == "security.outward_actions")
        .map(|v| v.message.as_str())
        .collect();
    (!messages.is_empty()).then(|| messages.join(" | "))
}

#[track_caller]
fn assert_refused(dir: &Path, command: &str, names: &str) {
    let message = refused(dir, command).unwrap_or_else(|| panic!("passed: {command}"));
    assert!(
        message.contains(names),
        "{command}: {message} does not name {names}"
    );
}

#[track_caller]
fn assert_passes(dir: &Path, command: &str) {
    let found = evaluate_at(command, &SecuritySection::default(), Some(dir));
    assert!(found.is_empty(), "{command}: {found:?}");
}

#[test]
fn ac2_workflow_dispatch_of_a_release_workflow_is_refused() {
    let temp = standard();
    let dir = temp.path();
    for command in [
        "gh workflow run release.yml",
        "gh workflow run release.yml -f tag=v9.9.9",
        "gh workflow run -R Acme/widget release.yml -f tag=v9.9.9",
        "gh workflow run --repo=acme/widget release.yml",
        "gh -R acme/widget workflow run release.yml",
        "gh workflow run .github/workflows/release.yml",
        "gh workflow run Release",
        "gh workflow run release.yml --ref main",
        "gh workflow run pages.yml",
        "env GH_PAGER= gh workflow run release.yml",
        "bash -c 'gh workflow run release.yml'",
    ] {
        assert_refused(dir, command, "release workflow");
    }
    for command in [
        "gh api --method POST repos/acme/widget/actions/workflows/release.yml/dispatches",
        "curl -X POST https://api.github.com/repos/acme/widget/actions/workflows/release.yml/dispatches",
        "gh api -X POST repos/acme/widget/actions/workflows/release.yml/dispatches -f ref=main",
        "gh api repos/acme/widget/actions/workflows/release.yml/dispatches -f ref=main",
        "gh api repos/{owner}/{repo}/actions/workflows/release.yml/dispatches -F ref=main",
        "gh api --method POST /repos/acme/widget/actions/workflows/release.yml/dispatches -f ref=main",
        "gh api --method=post repos/acme/widget/actions/workflows/release.yml/dispatches -f ref=main",
        "gh api -XPOST repos/acme/widget/actions/workflows/release.yml/dispatches -fref=main",
        "gh api -X=POST https://api.github.com/repos/acme/widget/actions/workflows/release.yml/dispatches -f ref=main",
        "curl -X POST https://api.github.com/repos/acme/widget/actions/workflows/release.yml/dispatches -d '{\"ref\":\"main\"}'",
        "curl --request=POST https://api.github.com/repos/acme/widget/actions/workflows/release.yml/dispatches --data '{\"ref\":\"main\",\"inputs\":{\"tag\":\"v1\"}}'",
    ] {
        assert_refused(dir, command, "release workflow");
    }
}

#[test]
fn ac2_repository_dispatch_a_release_workflow_listens_for_is_refused() {
    let temp = standard();
    let dir = temp.path();
    for command in [
        "gh api -X POST repos/acme/widget/dispatches -f event_type=ship",
        "gh api repos/acme/widget/dispatches -f event_type=ship",
        "curl -X POST https://api.github.com/repos/acme/widget/dispatches -d '{\"event_type\":\"ship\"}'",
        "curl https://api.github.com/repos/acme/widget/dispatches --json '{\"event_type\":\"ship\"}'",
    ] {
        assert_refused(dir, command, "repository dispatch ship");
    }
    for command in [
        "gh api repos/acme/widget/dispatches --input body.json",
        "curl -X POST https://api.github.com/repos/acme/widget/dispatches -d @body.json",
    ] {
        assert_refused(dir, command, "event type the guard cannot read");
    }
    assert_passes(
        dir,
        "gh api -X POST repos/acme/widget/dispatches -f event_type=docs",
    );
}

#[test]
fn ac2_deployment_approval_release_and_tag_creation_are_refused() {
    let temp = standard();
    let dir = temp.path();
    for (command, names) in [
        ("gh api -X POST repos/acme/widget/actions/runs/1/pending_deployments -f state=approved", "pending deployment"),
        ("gh api repos/acme/widget/actions/runs/1/pending_deployments -F environment_ids[]=1", "pending deployment"),
        ("curl -X POST https://api.github.com/repos/acme/widget/actions/runs/1/pending_deployments -d '{}'", "pending deployment"),
        ("gh api -X POST repos/acme/widget/actions/runs/1/deployment_protection_rule", "protection rule"),
        ("gh api -X POST repos/acme/widget/releases -f tag_name=v9.9.9", "release"),
        ("gh api -X PATCH repos/acme/widget/releases/1 -F draft=false", "release"),
        ("gh api --method PATCH repos/acme/widget/releases/1 -F draft=false", "release"),
        ("curl -X POST https://uploads.github.com/repos/acme/widget/releases/1/assets?name=x -T x", "release"),
        ("gh api -X POST repos/acme/widget/git/refs -f ref=refs/tags/v9.9.9 -f sha=abc", "refs/tags/v9.9.9"),
        ("gh api repos/acme/widget/git/refs -f ref=refs/tags/v9.9.9 -f sha=abc", "refs/tags/v9.9.9"),
        ("gh api repos/acme/widget/git/refs --input ref.json", "cannot read"),
        ("gh api -X PATCH repos/acme/widget/git/refs/tags/v1 -f sha=abc", "release tag"),
        ("gh api -X POST repos/acme/widget/git/tags -f tag=v1", "tag object"),
        ("curl -X POST https://api.github.com/repos/acme/widget/git/refs -d '{\"ref\":\"refs/tags/v1\",\"sha\":\"a\"}'", "refs/tags/v1"),
        ("gh api graphql -f query='mutation { createRef(input: {}) { clientMutationId } }'", "createRef"),
        ("gh api graphql -F query=@mutation.graphql", "cannot read"),
    ] {
        assert_refused(dir, command, names);
    }
    assert_passes(
        dir,
        "gh api -X POST repos/acme/widget/git/refs -f ref=refs/heads/task/x -f sha=abc",
    );
}

#[test]
fn ac2_writes_to_the_remote_floors_are_refused() {
    let temp = standard();
    let dir = temp.path();
    for (command, names) in [
        ("gh api -X POST repos/acme/widget/actions/policies --input p.json", "Actions policy"),
        ("gh api -X PUT repos/acme/widget/actions/policies/7 --input p.json", "Actions policy"),
        ("gh api -X DELETE repos/acme/widget/actions/policies/7", "Actions policy"),
        ("gh api -X POST orgs/acme/actions/policies --input p.json", "Actions policy"),
        ("gh api -X PUT orgs/acme/actions/policies/7 --input p.json", "Actions policy"),
        ("gh api -X PUT repos/acme/widget/environments/release", "environment"),
        ("gh api -X DELETE repos/acme/widget/environments/release", "environment"),
        ("gh api -X POST repos/acme/widget/rulesets --input r.json", "ruleset"),
        ("gh api -X PUT repos/acme/widget/rulesets/1 --input r.json", "ruleset"),
        ("gh api -X POST orgs/acme/rulesets --input r.json", "ruleset"),
        ("curl -X PUT https://api.github.com/repos/acme/widget/environments/release -d '{}'", "environment"),
        ("gh api graphql -f query='mutation { updateRepositoryRuleset(input: {}) { clientMutationId } }'", "updateRepositoryRuleset"),
    ] {
        assert_refused(dir, command, names);
    }
}

#[test]
fn ac4_forms_the_guard_cannot_map_are_refused() {
    let temp = standard();
    let dir = temp.path();
    for (command, names) in [
        ("gh workflow run 123456", "workflow id 123456"),
        ("gh api -X POST repos/acme/widget/actions/workflows/123456/dispatches -f ref=main", "workflow id 123456"),
        ("gh api -XPOST repos/acme/widget/actions/workflows/123456/dispatches", "workflow id 123456"),
        ("gh workflow run 'Nightly Publish'", "matches no workflow"),
        ("gh workflow run", "names no workflow"),
        ("gh workflow run \"$WORKFLOW\"", "cannot resolve"),
        ("gh workflow run release.yml --ref feat/x", "release workflow"),
        ("gh workflow run codeflow-ci.yml --ref feat/x", "where it is a release workflow"),
        ("gh workflow run codeflow-ci.yml --ref feat/unfetched", "cannot read here"),
        ("gh workflow run -R other/repo codeflow-ci.yml", "names repository other/repo"),
        ("gh workflow run -R \"$REPO\" codeflow-ci.yml", "cannot resolve"),
        ("gh api -X POST repos/acme/widget/actions/workflows/codeflow-ci.yml/dispatches -f ref=feat/x", "where it is a release workflow"),
        ("gh api -X POST repos/acme/widget/actions/workflows/codeflow-ci.yml/dispatches --input b.json", "ref the guard cannot read"),
        ("gh api -X POST \"repos/acme/widget/actions/workflows/$ID/dispatches\" -f ref=main", "cannot resolve"),
        ("gh api -X POST \"repos/$REPO/releases\" -f tag_name=v1", "release"),
        ("gh api -X POST \"$ENDPOINT\" -f a=b", "cannot read the endpoint"),
        ("gh api -X \"$METHOD\" repos/acme/widget/releases", "release"),
        ("gh api -X POST \"repos/acme/widget/actions/$REST\"", "unresolved part"),
    ] {
        assert_refused(dir, command, names);
    }
}

#[test]
fn ac4_an_unreadable_workflow_counts_as_release() {
    let temp = fixture(&[("broken.yml", "on: [workflow_dispatch\n")], &[]);
    assert_refused(temp.path(), "gh workflow run broken.yml", "cannot parse");
    let temp = fixture(
        &[(
            "default.yml",
            "on: workflow_dispatch\njobs:\n  a:\n    runs-on: x\n",
        )],
        &[],
    );
    assert_refused(
        temp.path(),
        "gh workflow run default.yml",
        "default token permissions",
    );
    let temp = fixture(
        &[(
            "token.yml",
            "on: workflow_dispatch\npermissions: {}\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo ${{ secrets.CARGO_TOKEN }}\n",
        )],
        &[],
    );
    assert_refused(temp.path(), "gh workflow run token.yml", "CARGO_TOKEN");
    let none = tempfile::tempdir().unwrap();
    assert_refused(
        none.path(),
        "gh workflow run codeflow-ci.yml",
        "cannot be read",
    );
}

#[test]
fn ac5_a_dry_run_dispatch_of_a_release_workflow_is_refused() {
    let temp = standard();
    for command in [
        "gh workflow run release.yml -f tag=dry-run",
        "gh workflow run release.yml --ref main",
        "gh api repos/acme/widget/actions/workflows/release.yml/dispatches -f ref=main -f inputs[tag]=dry-run",
    ] {
        assert_refused(temp.path(), command, "a dry run is refused too");
    }
}

#[test]
fn ac6_other_dispatches_reads_and_run_queries_still_pass() {
    let temp = standard();
    let dir = temp.path();
    for command in [
        "gh workflow run codeflow-ci.yml",
        "gh workflow run codeflow-ci.yml --ref main",
        "gh workflow run codeflow-ci.yml --ref refs/heads/main",
        "gh workflow run codeflow-ci -f x=y",
        "gh workflow run -R acme/widget codeflow-ci.yml --ref main",
        "gh api -X POST repos/acme/widget/actions/workflows/codeflow-ci.yml/dispatches -f ref=main",
        "gh workflow list",
        "gh workflow view release.yml",
        "gh run list -R acme/widget",
        "gh run view 1 --log",
        "gh run watch 1",
        "gh release view v1",
        "gh release list",
        "gh api repos/acme/widget/actions/runs",
        "gh api repos/acme/widget/releases",
        "gh api repos/acme/widget/actions/runs/1/pending_deployments",
        "gh api -X GET repos/acme/widget/rulesets -f per_page=100",
        "gh api repos/acme/widget/git/refs/tags",
        "gh api -X POST repos/acme/widget/issues/1/comments -f body=hi",
        "gh api -X POST \"repos/acme/widget/issues/$N/comments\" -f body=hi",
        "curl -s https://api.github.com/repos/acme/widget/releases",
        "curl -X POST https://example.invalid/releases -d x",
        // coordination/scripts/candidate-hosted-check.sh, as the script runs them.
        "gh pr view \"$pr\" -R \"$REPO\" --json headRefOid --jq .headRefOid",
        "gh api \"repos/$REPO/commits/$head/check-runs?per_page=100\" --paginate --jq '.check_runs[]'",
        "gh api \"repos/$REPO/actions/runs?head_sha=$head&per_page=100\" --paginate --jq '.workflow_runs[]'",
        "gh api \"repos/$REPO/rules/branches/$base\" --jq '.[].type'",
        "gh api \"repos/$REPO/pulls/$pr/files?per_page=100\" --paginate --jq '.[].filename'",
        // scripts/proofs/registry-proof/run.sh runs as a script; the guard
        // judges the script call, never the lines inside it.
        "REPO=acme/proof WORK=/tmp/w OUT=/tmp/o sh scripts/proofs/registry-proof/run.sh delete",
    ] {
        assert_passes(dir, command);
    }
}

#[test]
fn reasons_name_the_form_and_escape_remote_text() {
    let temp = standard();
    let message = refused(temp.path(), "gh workflow run 'Rel\u{1b}[31mease'").unwrap();
    assert!(!message.contains('\u{1b}'), "{message}");
    let message = refused(
        temp.path(),
        "gh api -X POST repos/acme/widget/actions/runs/1/pending_deployments",
    )
    .unwrap();
    assert!(
        message.starts_with("release action: gh api -X POST"),
        "{message}"
    );
}

#[test]
fn the_release_tag_shape() {
    for name in ["v1", "v9.9.9", "1.2.3", "vendor"] {
        assert!(super::release_tag_shaped(name), "{name}");
    }
    for name in ["HEAD", "main", "task/x", "refs/tags/v1", "tags-x"] {
        assert!(!super::release_tag_shaped(name), "{name}");
    }
}
