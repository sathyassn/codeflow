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
    if: vars.PUBLISH == 'true'
    steps:
      - run: echo plan
        env:
          TOKEN: ${{ secrets.REGISTRY_TOKEN }}
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

/// The guard's environment for a test: unless the test set its own, no
/// config variables and an account home that does not exist, so no client
/// config file on the host changes a verdict.
fn hermetic() {
    super::TEST_SESSION.with(|session| {
        session
            .borrow_mut()
            .get_or_insert_with(|| (Vec::new(), Some("/nonexistent/account-home".into())));
    });
}

/// Run the checks with the guard's environment set to `values` and the
/// account home `home`.
fn with_session(values: &[(&str, &str)], home: Option<&Path>) {
    let values = values
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    super::TEST_SESSION.with(|session| {
        *session.borrow_mut() = Some((values, home.map(Path::to_path_buf)));
    });
}

fn refused(dir: &Path, command: &str) -> Option<String> {
    hermetic();
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
    hermetic();
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
        "curl -q -X POST https://example.invalid/releases -d x",
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

#[test]
fn sweep_deployment_and_branch_protection_writes_are_refused() {
    let temp = standard();
    let dir = temp.path();
    for (command, names) in [
        ("gh api -X POST repos/acme/widget/deployments -f ref=main -f environment=release", "deployment"),
        ("gh api repos/acme/widget/deployments -f ref=v1", "deployment"),
        ("curl -X POST https://api.github.com/repos/acme/widget/deployments/1/statuses -d '{\"state\":\"success\"}'", "deployment"),
        ("gh api graphql -f query='mutation { createDeployment(input: {}) { clientMutationId } }'", "createDeployment"),
        ("gh api -X PUT repos/acme/widget/branches/main/protection --input p.json", "branch protection"),
        ("gh api -X DELETE repos/acme/widget/branches/main/protection/required_signatures", "branch protection"),
        ("gh api graphql -f query='mutation { deleteBranchProtectionRule(input: {}) { clientMutationId } }'", "deleteBranchProtectionRule"),
    ] {
        assert_refused(dir, command, names);
    }
    for command in [
        "gh api repos/acme/widget/deployments",
        "gh api repos/acme/widget/branches/main/protection",
    ] {
        assert_passes(dir, command);
    }
}

#[test]
fn sweep_enabling_a_release_workflow_is_refused() {
    let temp = standard();
    let dir = temp.path();
    for (command, names) in [
        ("gh workflow enable release.yml", "enables release workflow"),
        ("gh workflow enable Release", "enables release workflow"),
        (
            "gh -R acme/widget workflow enable release.yml",
            "enables release workflow",
        ),
        (
            "gh api -X PUT repos/acme/widget/actions/workflows/release.yml/enable",
            "enables release workflow",
        ),
        ("gh workflow enable", "names no workflow"),
        ("gh workflow enable 123", "workflow id 123"),
        (
            "gh workflow enable -R other/repo codeflow-ci.yml",
            "names repository other/repo",
        ),
    ] {
        assert_refused(dir, command, names);
    }
    for command in [
        "gh workflow enable codeflow-ci.yml",
        "gh api -X PUT repos/acme/widget/actions/workflows/codeflow-ci.yml/enable",
        "gh workflow disable codeflow-ci.yml",
    ] {
        assert_passes(dir, command);
    }
}

/// The stored values the fixture's release workflow reads. The commands
/// are built here so that no line of this file is itself a store command.
fn store(verb: &str, rest: &str) -> String {
    format!("gh {verb} {rest}")
}

#[test]
fn sweep_variables_and_secrets_a_release_workflow_reads_are_refused() {
    let temp = standard();
    let dir = temp.path();
    for (command, names) in [
        (
            store("variable set", "PUBLISH --body true"),
            "variable PUBLISH",
        ),
        (store("variable set", "publish -b true"), "variable publish"),
        (store("variable delete", "PUBLISH"), "variable PUBLISH"),
        (
            store("variable set", "PUBLISH --org acme -b true"),
            "variable PUBLISH",
        ),
        (
            store("variable set", "PUBLISH --env release -b true"),
            "variable PUBLISH",
        ),
        (
            store("secret set", "REGISTRY_TOKEN < token.txt"),
            "secret REGISTRY_TOKEN",
        ),
        (
            store("secret delete", "REGISTRY_TOKEN"),
            "secret REGISTRY_TOKEN",
        ),
        (store("variable set", "-f vars.env"), "cannot name"),
        (store("variable set", "\"$NAME\" -b x"), "cannot name"),
        (
            store("variable set", "OTHER -R other/repo -b 1"),
            "names repository other/repo",
        ),
        (
            store(
                "api",
                "-X POST repos/acme/widget/actions/variables -f name=PUBLISH -f value=true",
            ),
            "variable PUBLISH",
        ),
        (
            store(
                "api",
                "-X PATCH repos/acme/widget/actions/variables/OTHER -f name=PUBLISH",
            ),
            "variable PUBLISH",
        ),
        (
            store(
                "api",
                "-X DELETE repos/acme/widget/actions/variables/PUBLISH",
            ),
            "variable PUBLISH",
        ),
        (
            store(
                "api",
                "-X POST repos/acme/widget/actions/variables --input v.json",
            ),
            "cannot name",
        ),
        (
            store(
                "api",
                "-X PUT repos/acme/widget/actions/secrets/REGISTRY_TOKEN --input s.json",
            ),
            "secret REGISTRY_TOKEN",
        ),
        (
            store(
                "api",
                "-X PUT orgs/acme/actions/secrets/REGISTRY_TOKEN --input s.json",
            ),
            "secret REGISTRY_TOKEN",
        ),
    ] {
        assert_refused(dir, &command, names);
    }
    for command in [
        store("variable set", "OTHER --body 1"),
        store("variable list", ""),
        store("variable get", "PUBLISH"),
        store(
            "api",
            "-X PUT repos/acme/widget/actions/secrets/OTHER --input s.json",
        ),
        store("api", "repos/acme/widget/actions/variables/PUBLISH"),
    ] {
        assert_passes(dir, &command);
    }
    let inherit = "on: workflow_dispatch\npermissions: {}\njobs:\n  a:\n    uses: ./.github/workflows/called.yml\n    secrets: inherit\n";
    let temp = fixture(&[("inherit.yml", inherit), ("called.yml", CALLED)], &[]);
    assert_refused(
        temp.path(),
        &store("secret set", "ANY_NAME < t"),
        "secret ANY_NAME",
    );
    assert_passes(temp.path(), &store("variable set", "ANY_NAME -b 1"));
}

#[test]
fn sweep_other_http_clients_are_read() {
    let temp = standard();
    let dir = temp.path();
    for (command, names) in [
        ("wget --method=POST https://api.github.com/repos/acme/widget/releases --body-data '{\"tag_name\":\"v1\"}'", "release"),
        ("wget --post-data='{\"event_type\":\"ship\"}' https://api.github.com/repos/acme/widget/dispatches", "repository dispatch ship"),
        ("wget --post-file=b.json https://api.github.com/repos/acme/widget/dispatches", "event type the guard cannot read"),
        ("http POST https://api.github.com/repos/acme/widget/actions/workflows/release.yml/dispatches ref=main", "release workflow"),
        ("http api.github.com/repos/acme/widget/dispatches event_type=ship", "repository dispatch ship"),
        ("https PUT api.github.com/repos/acme/widget/environments/release", "environment"),
        ("xh post https://api.github.com/repos/acme/widget/releases tag_name=v1", "release"),
        ("xhs POST api.github.com/repos/acme/widget/actions/runs/1/pending_deployments state=approved", "pending deployment"),
    ] {
        assert_refused(dir, command, names);
    }
    for command in [
        "wget --no-config https://api.github.com/repos/acme/widget/releases",
        "wget --no-config -O out.json https://api.github.com/repos/acme/widget/releases",
        "http GET https://api.github.com/repos/acme/widget/releases",
        "xh https://api.github.com/repos/acme/widget/actions/runs per_page==5",
        "http POST https://api.github.com/repos/acme/widget/dispatches event_type=docs",
        "http POST https://example.invalid/releases a=b",
    ] {
        assert_passes(dir, command);
    }
}

#[test]
fn sweep_gh_repo_names_the_repository() {
    let temp = standard();
    let dir = temp.path();
    for command in [
        "GH_REPO=other/repo gh workflow run codeflow-ci.yml".to_owned(),
        "env GH_REPO=other/repo gh workflow run codeflow-ci.yml".to_owned(),
        "export GH_REPO=other/repo; gh workflow run codeflow-ci.yml".to_owned(),
        "GH_REPO=other/repo gh api -X POST repos/{owner}/{repo}/actions/workflows/codeflow-ci.yml/dispatches -f ref=main".to_owned(),
        format!("GH_REPO=other/repo {}", store("variable set", "OTHER -b 1")),
    ] {
        assert_refused(dir, &command, "names repository other/repo");
    }
    // gh lets `-R` win over GH_REPO; the guard refuses when they disagree.
    assert_refused(
        dir,
        "GH_REPO=other/repo gh workflow run -R acme/widget codeflow-ci.yml",
        "disagree",
    );
    for command in [
        "GH_REPO=acme/widget gh workflow run codeflow-ci.yml",
        "GH_REPO=acme/widget gh workflow run -R Acme/Widget codeflow-ci.yml",
        "GH_REPO=other/repo gh run list",
    ] {
        assert_passes(dir, command);
    }
}

#[test]
fn the_reads_of_a_workflow() {
    let reads = super::Reads::of(
        "if: vars.PUBLISH == 'true'\nenv:\n  A: ${{ secrets.CARGO_TOKEN }}\n  B: ${{ secrets.GITHUB_TOKEN }}\n  C: ${{ myvars.NOT }}\n",
    );
    assert!(reads.vars.covers("publish") && !reads.vars.covers("NOT"));
    assert!(reads.secrets.covers("CARGO_TOKEN") && !reads.secrets.covers("GITHUB_TOKEN"));
    for text in [
        "x: ${{ toJSON(secrets) }}",
        "x: ${{ secrets[format('A')] }}",
        "secrets: inherit",
    ] {
        assert!(super::Reads::of(text).secrets.covers("ANY"), "{text}");
    }
    assert!(super::Reads::of("x: ${{ toJSON(vars) }}")
        .vars
        .covers("ANY"));
    assert!(!super::Reads::of("x: 1").secrets.any());
}

/// Review finding A: a GitHub API host is read whatever its spelling.
#[test]
fn review_a_api_host_spellings_are_read() {
    let temp = standard();
    let dir = temp.path();
    for command in [
        // The reviewer's spellings.
        "curl -X POST https://api.github.com:443/repos/acme/widget/releases -d '{\"tag_name\":\"v1\"}'",
        "curl -X POST https://user@api.github.com/repos/acme/widget/releases -d '{\"tag_name\":\"v1\"}'",
        "gh api -X POST https://api.github.com:443/repos/acme/widget/releases -f tag_name=v1",
        "gh api -X POST https://x@api.github.com/repos/acme/widget/releases -f tag_name=v1",
        "wget --method=POST https://api.github.com:443/repos/acme/widget/releases --body-data '{\"tag_name\":\"v1\"}'",
        "wget --method=POST https://user@api.github.com/repos/acme/widget/releases --body-data '{}'",
        "http POST https://api.github.com:443/repos/acme/widget/releases tag_name=v1",
        "http POST https://user@api.github.com/repos/acme/widget/releases tag_name=v1",
        "xh POST https://api.github.com:443/repos/acme/widget/releases tag_name=v1",
        "xh POST https://user:pw@api.github.com/repos/acme/widget/releases tag_name=v1",
        // The rest of the class: case, a trailing dot, no scheme, a port
        // without a scheme, http, and the uploads host.
        "curl -X POST HTTPS://API.GitHub.COM/repos/acme/widget/releases -d '{}'",
        "curl -X POST https://api.github.com./repos/acme/widget/releases -d '{}'",
        "curl -X POST api.github.com:443/repos/acme/widget/releases -d '{}'",
        "curl -X POST http://api.github.com:80/repos/acme/widget/releases -d '{}'",
        "curl -X POST https://u@uploads.github.com:443/repos/acme/widget/releases/1/assets -T x",
        "curl -X POST -u u:p https://api.github.com/repos/acme/widget/releases -d '{}'",
    ] {
        assert_refused(dir, command, "release");
    }
    for (command, names) in [
        ("curl -X POST 'https://a@b@api.github.com/repos/acme/widget/releases' -d x", "cannot read"),
        ("curl -X POST https://api.github.com/x/../repos/acme/widget/releases -d x", "`..`"),
        ("curl -X POST https://api.github.com/x/%2e%2E/repos/acme/widget/releases -d x", "`..`"),
        ("gh api -X POST repos/acme/x/../widget/releases -f tag_name=v1", "`..`"),
        ("curl -X POST 'https://api.github.com/repos/acme/widget/{releases,x}' -d x", "expands"),
        ("curl -X POST \"https://$HOST/repos/acme/widget/releases\" -d x", "cannot read"),
        ("curl -X POST https:api.github.com/repos/acme/widget/releases -d x", "without `//`"),
        ("curl -X POST https://api.github.com:x/repos/acme/widget/releases -d x", "cannot read"),
        ("curl -X POST -H 'Host: api.github.com' https://203.0.113.1/repos/acme/widget/releases -d x", "Host header"),
        ("curl -X POST --connect-to ::api.github.com:443 https://example.invalid/repos/acme/widget/releases -d x", "other than its URL"),
        ("curl -X POST --request-target /repos/acme/widget/releases https://api.github.com/x -d x", "other than its URL"),
        ("curl -X POST -K request.cfg -d x", "from a file"),
        ("wget --no-config --method=POST -i urls.txt --body-data x", "from a file"),
        ("wget --no-config --method=POST --header='Host: api.github.com' https://203.0.113.1/repos/acme/widget/releases", "Host header"),
        ("http POST https://203.0.113.1/repos/acme/widget/releases Host:api.github.com tag_name=v1", "Host header"),
    ] {
        assert_refused(dir, command, names);
    }
    for command in [
        "curl -s https://api.github.com:443/repos/acme/widget/releases",
        "curl -q -X POST -u u:p https://example.invalid/x -d a",
        "curl -q -X POST http://localhost:3000/api -d x",
        "curl -q -g -X POST 'https://example.invalid/{x}' -d a",
        "curl -q -X POST https://notapi.github.com.example/repos/acme/widget/releases -d x",
    ] {
        assert_passes(dir, command);
    }
    // An HTTPie or xh write to a host other than GitHub's passes.
    assert_passes(dir, "http POST https://example.invalid/x host:=1");
}

/// Review finding B: any GraphQL path on a GitHub API host is GraphQL, for
/// every client.
#[test]
fn review_b_graphql_is_read_on_every_path_and_client() {
    let temp = standard();
    let dir = temp.path();
    for command in [
        // The reviewer's spellings.
        "gh api -X POST /graphql -f query='mutation { createRef(input:{}) { clientMutationId } }'",
        "curl -X POST https://api.github.com/graphql -d '{\"query\":\"mutation { createRef(input:{}) { clientMutationId } }\"}'",
        "wget --method=POST https://api.github.com/graphql --body-data '{\"query\":\"mutation { createRef(input:{}) { clientMutationId } }\"}'",
        "http POST https://api.github.com/graphql query='mutation { createRef(input:{}) { clientMutationId } }'",
        "xh POST https://api.github.com/graphql query='mutation { createRef(input:{}) { clientMutationId } }'",
        // The rest of the class.
        "gh api https://api.github.com/graphql -f query='mutation { a: createRef(input:{}) { clientMutationId } }'",
        "curl https://api.github.com:443/graphql --json '{\"query\":\"mutation { createRef(input:{}) { clientMutationId } }\"}'",
        "curl -X POST https://ghe.example/api/graphql -d '{\"query\":\"mutation { createRef(input:{}) { clientMutationId } }\"}'",
        "wget --post-data='{\"query\":\"mutation { createRef(input:{}) { clientMutationId } }\"}' https://api.github.com/graphql",
    ] {
        assert_refused(dir, command, "createRef");
    }
    for command in [
        "curl -X POST https://api.github.com/graphql -d @query.json",
        "gh api /graphql --input query.json",
        "gh api -X POST graphql -F query=@query.graphql",
        "http POST https://api.github.com/graphql @query.json",
        "wget --post-file=query.json https://api.github.com/graphql",
    ] {
        assert_refused(dir, command, "cannot read");
    }
    for command in [
        "gh api graphql -f query='query { viewer { login } }'",
        "curl -q -X POST https://api.github.com/graphql -d '{\"query\":\"query { viewer { login } }\"}'",
    ] {
        assert_passes(dir, command);
    }
}

/// Review finding C: gh's `{owner}/{repo}` is the repository `-R` or
/// `GH_REPO` names, and sources that disagree are refused.
#[test]
fn review_c_placeholders_resolve_through_the_repo_flag() {
    let temp = standard();
    let dir = temp.path();
    for (command, names) in [
        // The reviewer's spellings.
        ("gh -R other/repo api -X POST repos/{owner}/{repo}/dispatches -f event_type=release", "other/repo"),
        ("gh -R other/repo api -X POST repos/{owner}/{repo}/actions/workflows/codeflow-ci.yml/dispatches -f ref=main", "other/repo"),
        // The rest of the class.
        ("gh api -R other/repo -X POST repos/{owner}/{repo}/actions/workflows/codeflow-ci.yml/dispatches -f ref=main", "other/repo"),
        ("gh --repo=other/repo api -X PUT repos/{owner}/{repo}/actions/workflows/codeflow-ci.yml/enable", "other/repo"),
        ("gh -R other/repo api -X POST repos/acme/{repo}/dispatches -f event_type=docs", "other/repo"),
        ("gh -R other/repo api -X POST repos/{owner}/widget/dispatches -f event_type=docs", "other/repo"),
        ("gh -R acme/widget api -X POST repos/other/repo/dispatches -f event_type=docs", "contradicts"),
        ("GH_REPO=other/repo gh api -X POST repos/acme/widget/dispatches -f event_type=docs", "contradicts"),
        ("GH_REPO=acme/widget gh -R other/repo workflow run codeflow-ci.yml", "disagree"),
        ("gh -R acme/widget workflow run -R other/repo codeflow-ci.yml", "disagree"),
        ("gh -R '{owner}/{repo}' workflow run codeflow-ci.yml", "names repository {owner}/{repo}"),
        ("gh -R evilgithub.com/acme/widget workflow run codeflow-ci.yml", "names repository evilgithub.com/acme/widget"),
        ("gh -R https://github.com.evil/acme/widget workflow run codeflow-ci.yml", "names repository"),
        ("curl -g -X POST 'https://api.github.com/repos/{owner}/{repo}/dispatches' -d '{\"event_type\":\"docs\"}'", "names repository {owner}/{repo}"),
    ] {
        assert_refused(dir, command, names);
    }
    for command in [
        "gh -R acme/widget api -X POST repos/{owner}/{repo}/actions/workflows/codeflow-ci.yml/dispatches -f ref=main",
        "gh api -X POST repos/{owner}/{repo}/dispatches -f event_type=docs",
        "gh -R github.com/acme/widget workflow run codeflow-ci.yml",
        "gh -R https://github.com/Acme/widget workflow run codeflow-ci.yml",
        "gh -R git@github.com:Acme/widget.git workflow run codeflow-ci.yml",
    ] {
        assert_passes(dir, command);
    }
    // gh picks its current repository among the remotes; one that names
    // another repository leaves the guard unsure which.
    let temp = standard();
    git2::Repository::open(temp.path())
        .unwrap()
        .remote("upstream", "https://github.com/other/repo.git")
        .unwrap();
    for command in [
        "gh workflow run codeflow-ci.yml",
        "gh api -X POST repos/{owner}/{repo}/actions/workflows/codeflow-ci.yml/dispatches -f ref=main",
    ] {
        assert_refused(temp.path(), command, "current repository");
    }
    assert_passes(
        temp.path(),
        "gh workflow run -R acme/widget codeflow-ci.yml",
    );
}

/// Review finding D: a token that can write Actions or administration can
/// start or unblock a release run.
#[test]
fn review_d_actions_and_administration_writes_are_publishing() {
    for scope in ["actions", "administration"] {
        let helper = format!(
            "on: workflow_dispatch\npermissions:\n  contents: read\n  {scope}: write\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: gh workflow run release.yml\n"
        );
        let temp = fixture(&[("helper.yml", &helper)], &[]);
        assert_refused(
            temp.path(),
            "gh workflow run helper.yml",
            "publishing scope",
        );
    }
    let temp = fixture(
        &[(
            "helper.yml",
            "on: workflow_dispatch\npermissions:\n  contents: read\n  actions: read\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo\n",
        )],
        &[],
    );
    assert_passes(temp.path(), "gh workflow run helper.yml");
}

#[test]
fn the_github_slug() {
    for (text, slug) in [
        ("Acme/widget", "acme/widget"),
        ("github.com/Acme/widget", "acme/widget"),
        ("https://github.com/Acme/widget.git", "acme/widget"),
        ("https://u@GitHub.com:443/Acme/widget/", "acme/widget"),
        ("ssh://git@github.com/acme/widget.git", "acme/widget"),
        ("git@github.com:acme/widget.git", "acme/widget"),
        ("https://www.github.com/acme/widget", "acme/widget"),
    ] {
        assert_eq!(super::github_slug(text).as_deref(), Some(slug), "{text}");
    }
    for text in [
        "https://evilgithub.com/acme/widget",
        "https://github.com.evil/acme/widget",
        "evilgithub.com/acme/widget",
        "git@gitlab.com:acme/widget.git",
        "https://a@b@github.com/acme/widget",
        "{owner}/{repo}",
        "acme/widget/extra/part",
        "../widget",
    ] {
        assert_eq!(super::github_slug(text), None, "{text}");
    }
}

/// Second review, findings 1 to 3: a `curl` write is refused when it may
/// reach a GitHub API host: a parsed GitHub destination, an option the
/// guard does not know, or a destination that does not parse, in any
/// `--next` request. A write to a parsed host other than GitHub's passes.
#[test]
fn review2_curl_writes_are_allow_listed() {
    let temp = standard();
    let dir = temp.path();
    let body = "-d '{\"tag_name\":\"v1\"}'";
    let release = "https://api.github.com/repos/acme/widget/releases";
    for (command, names) in [
        // The reviewer's spellings, with and without `-q`.
        (
            format!("curl -X POST {release} {body} --next -X GET https://example.invalid/"),
            "release",
        ),
        (
            format!("curl -X POST {release} {body} -: -X GET https://example.invalid/"),
            "release",
        ),
        (
            format!("curl {body} {release} --next -G https://example.invalid/"),
            "release",
        ),
        (
            format!("curl -q -X POST {release} {body} --next -X GET https://example.invalid/"),
            "release",
        ),
        (
            format!("curl -q {body} {release} -: -G https://example.invalid/"),
            "release",
        ),
        (
            format!("curl -X POST --expand-url={release} {body}"),
            "release",
        ),
        (
            format!("curl -X POST https://example.invalid/x --expand-url={release} {body}"),
            "release",
        ),
        (
            format!("curl -q -X POST https://example.invalid/x --expand-url={release} {body}"),
            "release",
        ),
        // The rest of the class: a write in a later request, expansions,
        // unknown options, prefixes, clusters, redirects and URL files.
        (
            format!("curl -q https://example.invalid/ --next -X POST {release} {body}"),
            "release",
        ),
        (
            format!("curl -q -sSd '{{\"tag_name\":\"v1\"}}' {release}"),
            "release",
        ),
        (
            "curl -q -X POST --expand-url 'https://api.github.com/{{path}}' -d x".to_owned(),
            "expand",
        ),
        (
            "curl -q --variable m=POST --expand-request '{{m}}' https://example.invalid/x"
                .to_owned(),
            "expands a variable",
        ),
        (
            "curl -q --frobnicate -X POST https://example.invalid/x -d a".to_owned(),
            "does not know",
        ),
        (
            "curl --frobnicate -X POST https://localhost:8080/x -d a".to_owned(),
            "does not know",
        ),
        (
            "curl -q --requ POST https://example.invalid/x".to_owned(),
            "does not know",
        ),
        (
            "curl -q -L -X POST https://example.invalid/x -d a".to_owned(),
            "redirects",
        ),
        (
            "curl -q -x http://proxy.invalid:8080 -X POST https://example.invalid/x -d a"
                .to_owned(),
            "other than its URL",
        ),
        (
            "curl -q -X POST --url @urls.txt -d a".to_owned(),
            "from a file",
        ),
        (
            "curl -K request.cfg https://example.invalid/x".to_owned(),
            "from a file",
        ),
        (
            "curl -q -X POST https://example.invalid/x -d a --next 'https://a@b@api.github.com/'"
                .to_owned(),
            "cannot read",
        ),
    ] {
        assert_refused(dir, &command, names);
    }
}

/// The second half of the curl allow-list: what passes.
#[test]
fn review2_curl_writes_to_other_hosts_pass() {
    let temp = standard();
    let dir = temp.path();
    for command in [
        // A write whose destinations all parse to hosts other than GitHub's.
        "curl -X POST https://localhost:8080/x -d a",
        "curl -sSXPOST https://example.invalid/x",
        "curl -q -X POST https://example.invalid/x -d a",
        "curl --disable -sS --fail -X POST https://example.invalid/x --json '{}' -o out.json",
        "curl -q -X POST https://example.invalid/x -d a --next https://api.github.com/repos/acme/widget/releases",
        // Calls with no write keep their reading, unknown options included.
        "curl -sSL --frobnicate https://api.github.com/repos/acme/widget/releases",
        "curl -s https://example.invalid/ --next -s https://example.invalid/2",
        "curl -sS -X GET https://api.github.com/repos/acme/widget/releases",
    ] {
        assert_passes(dir, command);
    }
}

/// A directory holding an empty client config file at `file`.
fn config_dir(file: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "").unwrap();
    dir
}

/// The Director's scope for `curl`: without `-q` first, a default config
/// file that exists, or one the guard cannot check, refuses every call,
/// reads included; with none, the arguments decide.
#[test]
fn review2_a_curlrc_refuses_curl_without_q() {
    let temp = standard();
    let dir = temp.path();
    let home = config_dir(".curlrc");
    let at = home.path().display().to_string();
    let post = "curl -X POST https://localhost:8080/x -d a";
    // No config file: a known-option write to localhost passes.
    assert_passes(dir, post);
    // A config file in the account home: the same write and a read are
    // refused, naming the file and `-q`; `-q` or `--disable` first passes.
    with_session(&[], Some(home.path()));
    for command in [
        post,
        "curl https://example.invalid/",
        "curl -s -q https://example.invalid/",
    ] {
        assert_refused(dir, command, &format!("{at}/.curlrc"));
        assert_refused(dir, command, "put `-q` first");
    }
    for command in [
        "curl -q -X POST https://localhost:8080/x -d a",
        "curl --disable https://example.invalid/",
    ] {
        assert_passes(dir, command);
    }
}

/// The config lookup uses the environment the call runs with: inline
/// assignments, `env`, `export` or an assignment earlier on the line, the
/// guard's own environment and the account home. A value the guard cannot
/// resolve counts as a config file.
#[test]
fn review2_curlrc_is_found_through_every_environment_route() {
    let temp = standard();
    let dir = temp.path();
    let curl_home = config_dir(".curlrc");
    let at = curl_home.path().display().to_string();
    let xdg = config_dir("curlrc");
    let xdg_at = xdg.path().display().to_string();
    let empty = tempfile::tempdir().unwrap();
    let none = empty.path().display().to_string();
    let get = "curl https://example.invalid/";
    for (command, names) in [
        // Inline.
        (format!("CURL_HOME={at} {get}"), format!("{at}/.curlrc")),
        (
            format!("XDG_CONFIG_HOME={xdg_at} {get}"),
            format!("{xdg_at}/curlrc"),
        ),
        (format!("HOME={at} {get}"), format!("{at}/.curlrc")),
        // An env prefix, its known options included.
        (format!("env CURL_HOME={at} {get}"), format!("{at}/.curlrc")),
        (
            format!("env -i -u PATH CURL_HOME={at} {get}"),
            format!("{at}/.curlrc"),
        ),
        // Export or assignment earlier in the compound.
        (
            format!("export CURL_HOME={at}; {get}"),
            format!("{at}/.curlrc"),
        ),
        (format!("CURL_HOME={at}; {get}"), format!("{at}/.curlrc")),
        (
            format!("cd /; export CURL_HOME={at} && {get}"),
            format!("{at}/.curlrc"),
        ),
        // Values the guard cannot resolve.
        (format!("CURL_HOME=$X {get}"), "cannot resolve".to_owned()),
        (format!("CURL_HOME=~/x {get}"), "cannot resolve".to_owned()),
        (
            format!("CURL_HOME=$(pwd) {get}"),
            "cannot resolve".to_owned(),
        ),
        (format!("source ./env.sh; {get}"), "`source`".to_owned()),
        (format!("eval \"$SET\"; {get}"), "`eval`".to_owned()),
        (format!("read CURL_HOME < f; {get}"), "`read`".to_owned()),
        (
            format!("env -S 'CURL_HOME=/x' {get}"),
            "`env -S`".to_owned(),
        ),
        (
            format!(": ${{CURL_HOME:=/x}}; {get}"),
            "expansion".to_owned(),
        ),
    ] {
        assert_refused(dir, &command, &names);
        assert_refused(dir, &command, "put `-q` first");
        assert_passes(dir, &command.replacen("curl ", "curl -q ", 1));
    }
    // A directory with no config file passes.
    assert_passes(
        dir,
        &format!("CURL_HOME={none} XDG_CONFIG_HOME={none} {get}"),
    );
    // The guard's own environment.
    with_session(&[("CURL_HOME", &at)], Some(empty.path()));
    assert_refused(dir, get, &format!("{at}/.curlrc"));
    // The account home's default path.
    with_session(&[], Some(curl_home.path()));
    assert_refused(dir, get, &format!("{at}/.curlrc"));
    // No account home to check.
    with_session(&[], None);
    assert_refused(dir, get, "home directory cannot be read");
}

/// Second review, the sibling sweep: wget, `HTTPie` and xh follow the same
/// scope as curl, and gh aliases are read from every config gh may read.
#[test]
fn review2_other_clients_and_gh_aliases_are_read() {
    let temp = standard();
    let dir = temp.path();
    let release = "https://api.github.com/repos/acme/widget/releases";
    for (command, names) in [
        // wget reads a system wgetrc the guard cannot check.
        ("wget https://example.invalid/x".to_owned(), "--no-config"),
        ("wget -qO- https://example.invalid/x".to_owned(), "--no-config"),
        ("wget --method=POST --body-data=x https://example.invalid/x".to_owned(), "--no-config"),
        ("wget --no-config --method=POST --body-data=x https://example.invalid/x".to_owned(), "redirects"),
        ("wget --no-config --max-redirect=0 --frob --method=POST https://example.invalid/x".to_owned(), "does not know"),
        (format!("wget --no-config --max-redirect=0 --meth=POST {release}"), "does not know"),
        ("wget --no-config -e method=POST https://example.invalid/x".to_owned(), "file or command"),
        (format!("wget --no-config --max-redirect=0 --method=POST --body-data=x https://example.invalid/x {release}"), "release"),
        (format!("wget --method=POST --body-data=x {release}"), "release"),
        // HTTPie and xh writes that may reach GitHub.
        ("http --follow POST https://example.invalid/x a=b".to_owned(), "redirects"),
        ("http --frob POST https://example.invalid/x a=b".to_owned(), "does not know"),
        ("xh --session=s PUT https://example.invalid/x".to_owned(), "session file"),
        ("http POST https://203.0.113.1/x Host:api.github.com".to_owned(), "Host header"),
        ("gh api -X POST graphql -f owner=acme".to_owned(), "cannot read from its body"),
        ("curl -q -X POST 'https://api.github.com/graphql?query=mutation' -d '{}'".to_owned(), "cannot read from its body"),
        ("gh api -X POST repositories/1/releases -f tag_name=v1".to_owned(), "release"),
        ("gh api -X POST repositories/1/dispatches -f event_type=docs".to_owned(), "cannot resolve"),
    ] {
        assert_refused(dir, &command, names);
    }
    for command in [
        "wget --no-config --max-redirect=0 --method=POST --body-data=x https://example.invalid/x",
        "wget --no-config -qO- https://api.github.com/repos/acme/widget/releases",
        "http POST https://localhost:8080/x a=b",
        "xh PUT https://example.invalid/x a=b",
        "https GET https://api.github.com/repos/acme/widget/releases",
        "gh api -X POST repositories/1/issues/1/comments -f body=hi",
    ] {
        assert_passes(dir, command);
    }
    // HTTPie and xh config files, through the variable and the home.
    let httpie = config_dir("config.json");
    let httpie_at = httpie.path().display().to_string();
    let home = config_dir(".config/xh/config.json");
    for (command, names) in [
        (
            format!("HTTPIE_CONFIG_DIR={httpie_at} http https://example.invalid/"),
            "HTTPIE_CONFIG_DIR",
        ),
        (
            format!("XH_CONFIG_DIR={httpie_at} xh POST https://localhost:8080/x a=b"),
            "XH_CONFIG_DIR",
        ),
        (
            format!("HOME={} xh https://example.invalid/", home.path().display()),
            "xh/config.json",
        ),
        (
            "HTTPIE_CONFIG_DIR=$X http https://example.invalid/".to_owned(),
            "cannot resolve",
        ),
    ] {
        assert_refused(dir, &command, names);
        assert_refused(dir, &command, "config");
    }
    with_session(&[], Some(home.path()));
    assert_refused(
        dir,
        "xh POST https://localhost:8080/x a=b",
        "xh/config.json",
    );
    assert_passes(dir, "http POST https://localhost:8080/x a=b");
}

/// gh aliases are read from every config gh may read.
#[test]
fn review2_gh_aliases_are_read_from_gh_config() {
    let temp = standard();
    let dir = temp.path();
    // gh aliases, read from the config the line points gh at.
    let config = tempfile::tempdir().unwrap();
    std::fs::write(
        config.path().join("config.yml"),
        "aliases:\n  rel: api -X POST repos/{owner}/{repo}/releases -f tag_name=$1\n  disp: workflow run $1\n  shell: '!gh pr list'\n  prs: pr list\n",
    )
    .unwrap();
    let at = config.path().display().to_string();
    for (command, names) in [
        (format!("GH_CONFIG_DIR={at} gh rel v1"), "release"),
        (format!("export GH_CONFIG_DIR={at}; gh rel v1"), "release"),
        (
            format!("env GH_CONFIG_DIR={at} gh disp release.yml"),
            "release workflow",
        ),
        (format!("GH_CONFIG_DIR={at} gh shell"), "shell alias"),
        ("GH_CONFIG_DIR=$X gh rel v1".to_owned(), "cannot resolve"),
        ("source ./gh.sh; gh rel v1".to_owned(), "`source`"),
    ] {
        assert_refused(dir, &command, names);
    }
    for command in [
        format!("GH_CONFIG_DIR={at} gh prs"),
        format!("GH_CONFIG_DIR={at} gh disp codeflow-ci.yml"),
        format!("GH_CONFIG_DIR={at} gh some-extension"),
    ] {
        assert_passes(dir, &command);
    }
    let broken = tempfile::tempdir().unwrap();
    std::fs::create_dir(broken.path().join("config.yml")).unwrap();
    assert_refused(
        dir,
        &format!(
            "GH_CONFIG_DIR={} gh some-extension",
            broken.path().display()
        ),
        "cannot be read",
    );
    // An alias in the guard's own config dir is read too.
    with_session(&[("GH_CONFIG_DIR", &at)], None);
    assert_refused(dir, "gh rel v1", "release");
}
