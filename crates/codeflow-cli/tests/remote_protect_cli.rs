//! `codeflow remote protect` at the command line (TSK-1080): the help names
//! the profiles, the check identity, the line pattern and the host-side
//! reduction route; bad flags are refused before anything is read; and two
//! runs against a recorded GitHub write the three owned rulesets once,
//! never touch a foreign ruleset, and change nothing the second time.
//!
//! The host is a `gh` stand-in on `PATH` that answers from files: it records
//! GitHub's REST shapes, and the hosted fixture on a scratch repository is
//! owed (TSK-1080 AC-2, AC-4, AC-6). The TSK-383 journey runs `doctor` and
//! `remote protect --dry-run` against a stand-in that holds split `main`
//! rulesets and a manual registry ruleset, and refuses every write.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/process_folders.rs"]
mod process_folders;
use process_folders::isolated_home;

fn codeflow(root: &Path, path_first: Option<&Path>, args: &[&str]) -> Output {
    let path = std::env::join_paths(path_first.map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .current_dir(root)
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        // Decoys: the repository and host come from `origin` alone, so `gh`
        // never sees these (review of PR 700).
        .env("GH_HOST", "decoy.invalid")
        .env("GH_REPO", "decoy/other")
        // A fixed identity, so a commit never falls back to the host's
        // user name, hostname or `EMAIL` (PR 700).
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env_remove("EMAIL")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .expect("codeflow runs")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A git repository with a policy that protects `main` alone.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();
    assert!(status.success());
    // `origin` names the repository the `gh` stand-in answers for; git in
    // this repository refuses https, so nothing contacts the host.
    git(dir.path(), &["config", "protocol.https.allow", "never"]);
    git(
        dir.path(),
        &["remote", "add", "origin", "https://github.com/o/r.git"],
    );
    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    std::fs::write(
        dir.path().join(".codeflow/policy.json"),
        r#"{ "schema_version": 1, "git": { "protected_branches": ["main"] } }"#,
    )
    .unwrap();
    dir
}

/// AC-5: the help names the profiles, `--check-app`, `--lines` and the
/// host-side reduction route, and there is no `--reduce`.
#[test]
fn the_help_names_profiles_check_identity_lines_and_the_reduction_route() {
    let dir = project();
    let out = codeflow(dir.path(), None, &["remote", "protect", "--help"]);
    assert!(out.status.success(), "{}", text(&out));
    let help = text(&out);
    for expected in [
        "codeflow authority updates",
        "codeflow authority checks",
        "codeflow line checks",
        "--profile <PROFILE>",
        "solo-administrator",
        "independent-review",
        "[default: solo-administrator]",
        "--check-app <APP_ID>",
        "(Mode A)",
        "(Mode B)",
        "--lines <PATTERN>",
        "[default: integration/**]",
        "To reduce a rule, edit it on the host as an administrator; there is no command for it",
        "never lowers an approval count",
    ] {
        assert!(help.contains(expected), "{expected}: {help}");
    }
    assert!(!help.contains("--reduce"), "{help}");
    let out = codeflow(dir.path(), None, &["remote", "protect", "--reduce"]);
    assert!(!out.status.success(), "{}", text(&out));
}

/// Flags that cannot be right are refused before any file or host is read.
#[test]
fn a_check_app_or_line_pattern_that_cannot_be_right_is_refused() {
    let dir = project();
    for (args, why) in [
        (&["--check-app", "15368"][..], "is GitHub Actions"),
        (&["--check-app", "0"], "not 0"),
        (
            &["--lines", "refs/heads/integration/**"],
            "below refs/heads/",
        ),
        (&["--profile", "solo"], "possible values"),
    ] {
        let mut all = vec!["remote", "protect", "--dry-run"];
        all.extend_from_slice(args);
        let out = codeflow(dir.path(), None, &all);
        assert!(!out.status.success(), "{args:?}: {}", text(&out));
        assert!(text(&out).contains(why), "{args:?}: {}", text(&out));
    }
}

/// Without a readable host the dry run prints the intended rules and says
/// they are not the exact requests (TSK-383 AC-3). The `gh` on `PATH` is a
/// stand-in that fails, so no real host is read.
#[cfg(unix)]
#[test]
fn without_a_readable_host_the_dry_run_names_the_intended_rulesets() {
    use std::os::unix::fs::PermissionsExt;
    let dir = project();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("gh"),
        "#!/bin/sh\necho 'gh: no host here' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(bin.join("gh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = codeflow(
        dir.path(),
        Some(&bin),
        &[
            "remote",
            "protect",
            "--dry-run",
            "--profile",
            "independent-review",
            "--check-app",
            "4242",
            "--lines",
            "release/**",
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    let report = text(&out);
    for expected in [
        "profile independent-review; Mode A, the authority: names pinned to GitHub App 4242",
        "ruleset codeflow authority updates on ~DEFAULT_BRANCH:",
        "only the maintain role (RepositoryRole 2) updates the branch",
        "ruleset codeflow authority checks on ~DEFAULT_BRANCH:",
        "a pull request with one approval",
        "ruleset codeflow line checks on refs/heads/release/**:",
        "status: dry-run",
    ] {
        assert!(report.contains(expected), "{expected}: {report}");
    }
    // The dry run read no host, so it says which pattern a live run skips
    // and that these are not the exact requests.
    assert!(
        report
            .contains("a policy pattern that names the default branch gets no ruleset of its own"),
        "{report}"
    );
    assert!(
        report.contains("so these are the intended rules, not the exact requests"),
        "{report}"
    );
}

#[cfg(unix)]
/// The policy workflow on the recorded default branch, with the `trusted:`
/// names TSK-336 declares.
const POLICY: &str = r#"
on: pull_request_target
jobs:
  commit-standards:
    name: "trusted: commit standards"
  secret-scan:
    name: "trusted: secret scan"
"#;

#[cfg(unix)]
const CI: &str = "
on: pull_request
jobs:
  gates:
    name: codeflow gates
  windows-verdict:
    name: windows
";

#[cfg(unix)]
/// This repository's foreign `main` ruleset as recorded on 2026-10-29.
const FOREIGN: &str = r#"{"id":24379074,"name":"main required checks","target":"branch","source_type":"Repository","enforcement":"active","conditions":{"ref_name":{"exclude":[],"include":["~DEFAULT_BRANCH"]}},"rules":[{"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":true,"do_not_enforce_on_create":false,"required_status_checks":[{"context":"codeflow gates","integration_id":15368},{"context":"commit standards","integration_id":15368}]}}],"bypass_actors":[]}"#;

/// A `gh` that answers from `<host>`: `list` holds one ruleset summary per
/// line, `rs-<id>.json` each full ruleset, `wf/` the default branch's
/// workflows; every write is logged to `writes`, stored, and answered as
/// stored.
#[cfg(unix)]
fn recorded_host(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let host = dir.join("host");
    std::fs::create_dir_all(host.join("wf")).unwrap();
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::write(host.join("wf/policy.yml"), POLICY).unwrap();
    std::fs::write(host.join("wf/ci.yml"), CI).unwrap();
    std::fs::write(host.join("rs-24379074.json"), FOREIGN).unwrap();
    std::fs::write(
        host.join("list"),
        "{\"id\":24379074,\"name\":\"main required checks\",\"target\":\"branch\",\"source_type\":\"Repository\"}\n",
    )
    .unwrap();
    let h = host.display();
    let script = format!(
        r#"#!/bin/sh
h='{h}'
wf='repos/o/r/contents/.github/workflows'
case "$*" in
  "auth status --hostname github.com") exit 0 ;;
  "repo view"*) printf '%s' '{{"nameWithOwner":"o/r","isPrivate":false,"defaultBranchRef":{{"name":"main"}}}}' ;;
  "api repos/o/r/rulesets?"*) printf '['; paste -sd, "$h/list" | tr -d '\n'; printf ']' ;;
  "api repos/o/r/rulesets/"*) cat "$h/rs-${{2##*/}}.json" ;;
  "api $wf?ref=main") printf '[{{"path":".github/workflows/codeflow-policy.yml","type":"file"}},{{"path":".github/workflows/codeflow-ci.yml","type":"file"}}]' ;;
  *"$wf/codeflow-policy.yml?ref=main") cat "$h/wf/policy.yml" ;;
  *"$wf/codeflow-ci.yml?ref=main") cat "$h/wf/ci.yml" ;;
  "api -X POST repos/o/r/rulesets --input -")
    n=$(wc -l < "$h/list" | tr -d ' '); id=$((1000 + n))
    body=$(cat); printf 'POST %s\n' "$id" >> "$h/writes"
    printf '%s' "$body" | sed "s/^{{/{{\"id\":$id,/" > "$h/rs-$id.json"
    name=$(printf '%s' "$body" | sed -n 's/.*"name":"\([^"]*\)".*/\1/p')
    printf '{{"id":%s,"name":"%s","target":"branch","source_type":"Repository"}}\n' "$id" "$name" >> "$h/list"
    cat "$h/rs-$id.json" ;;
  "api -X PUT repos/o/r/rulesets/"*)
    id="${{4##*/}}"; body=$(cat); printf 'PUT %s\n' "$id" >> "$h/writes"
    printf '%s' "$body" | sed "s/^{{/{{\"id\":$id,/" > "$h/rs-$id.json"
    cat "$h/rs-$id.json" ;;
  *) echo "unexpected gh call: $*" >&2; exit 1 ;;
esac
"#
    );
    let gh = dir.join("bin/gh");
    std::fs::write(&gh, script).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    host
}

/// AC-2 and AC-6 at the command line: the first run creates A, B and C
/// beside the foreign ruleset; the second writes nothing; the foreign
/// ruleset is byte for byte what it was.
#[cfg(unix)]
#[test]
fn two_runs_write_the_owned_rulesets_once_and_never_the_foreign_one() {
    let dir = project();
    let host = recorded_host(dir.path());
    let bin = dir.path().join("bin");

    let first = codeflow(dir.path(), Some(&bin), &["remote", "protect"]);
    let report = text(&first);
    assert!(first.status.success(), "{report}");
    assert!(report.contains("status: applied"), "{report}");
    for name in [
        "created ruleset 'codeflow authority updates' (1001) on ~DEFAULT_BRANCH: only bypass actors update it, bypass: RepositoryRole 5 (always)",
        "created ruleset 'codeflow authority checks' (1002) on ~DEFAULT_BRANCH",
        "created ruleset 'codeflow line checks' (1003) on refs/heads/integration/**",
        "strict checks: trusted: commit standards (app 15368), trusted: secret scan (app 15368), codeflow gates (app 15368), windows (app 15368)",
    ] {
        assert!(report.contains(name), "{name}: {report}");
    }
    let writes = std::fs::read_to_string(host.join("writes")).unwrap();
    assert_eq!(writes, "POST 1001\nPOST 1002\nPOST 1003\n");

    // An operator excludes a branch from an owned ruleset on the host; the
    // next run keeps that exclude and names it (review of PR 700).
    let checks = host.join("rs-1002.json");
    let excluded = std::fs::read_to_string(&checks).unwrap().replacen(
        r#""exclude":[]"#,
        r#""exclude":["refs/heads/old"]"#,
        1,
    );
    assert!(excluded.contains("refs/heads/old"), "{excluded}");
    std::fs::write(&checks, &excluded).unwrap();

    let second = codeflow(dir.path(), Some(&bin), &["remote", "protect"]);
    let report = text(&second);
    assert!(second.status.success(), "{report}");
    assert!(report.contains("keeps exclude refs/heads/old"), "{report}");
    assert_eq!(std::fs::read_to_string(&checks).unwrap(), excluded);
    assert_eq!(
        report
            .matches("already as intended, nothing written")
            .count(),
        3,
        "{report}"
    );
    assert_eq!(
        std::fs::read_to_string(host.join("writes")).unwrap(),
        writes,
        "the second run wrote: {report}"
    );
    assert_eq!(
        std::fs::read_to_string(host.join("rs-24379074.json")).unwrap(),
        FOREIGN
    );
}

/// Run `git` in `root` with the test's isolated configuration.
fn git(root: &Path, args: &[&str]) -> String {
    // `codeflow init` installs hooks in these repositories, and the hooks
    // run `codeflow`: this build's binary comes first on PATH, as for
    // `codeflow()`, so no installed binary answers for it.
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(std::iter::once(exe.parent().unwrap().to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        // A fixed identity, so a commit never falls back to the host's
        // user name, hostname or `EMAIL` (PR 700).
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env_remove("EMAIL")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Issue 200's host: `main` split across two rulesets with different bypass
/// lists, and a `codeflow/registry` ruleset an administrator made by hand.
#[cfg(unix)]
const SPLIT_MAIN_CHECKS: &str = r#"{"id":301,"name":"main checks","target":"branch","source_type":"Repository","enforcement":"active","conditions":{"ref_name":{"exclude":[],"include":["~DEFAULT_BRANCH"]}},"rules":[{"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":true,"required_status_checks":[{"context":"codeflow gates","integration_id":15368}]}}],"bypass_actors":[{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"always"}]}"#;
#[cfg(unix)]
const SPLIT_MAIN_HISTORY: &str = r#"{"id":302,"name":"main history","target":"branch","source_type":"Repository","enforcement":"active","conditions":{"ref_name":{"exclude":[],"include":["~DEFAULT_BRANCH"]}},"rules":[{"type":"non_fast_forward"},{"type":"deletion"}],"bypass_actors":[]}"#;
#[cfg(unix)]
const MANUAL_REGISTRY: &str = r#"{"id":303,"name":"registry guard","target":"branch","source_type":"Repository","enforcement":"active","conditions":{"ref_name":{"exclude":[],"include":["refs/heads/codeflow/registry"]}},"rules":[{"type":"non_fast_forward"},{"type":"deletion"}],"bypass_actors":[{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"always"}]}"#;
/// The same registry ruleset with nobody able to bypass it.
#[cfg(unix)]
const MANUAL_REGISTRY_BOUND: &str = r#"{"id":303,"name":"registry guard","target":"branch","source_type":"Repository","enforcement":"active","conditions":{"ref_name":{"exclude":[],"include":["refs/heads/codeflow/registry"]}},"rules":[{"type":"non_fast_forward"},{"type":"deletion"}],"bypass_actors":[]}"#;

/// A `gh` for issue 200's host that answers reads only: every call is
/// logged to `calls`, and a write is logged to `writes` and refused.
#[cfg(unix)]
fn split_host(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let host = dir.join("host");
    std::fs::create_dir_all(host.join("wf")).unwrap();
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::write(host.join("wf/policy.yml"), POLICY).unwrap();
    std::fs::write(host.join("wf/ci.yml"), CI).unwrap();
    for (id, body) in [
        (301, SPLIT_MAIN_CHECKS),
        (302, SPLIT_MAIN_HISTORY),
        (303, MANUAL_REGISTRY),
    ] {
        std::fs::write(host.join(format!("rs-{id}.json")), body).unwrap();
    }
    let h = host.display();
    let script = format!(
        r#"#!/bin/sh
h='{h}'
wf='repos/o/r/contents/.github/workflows'
printf '%s\n' "$*" >> "$h/calls"
printf 'GH_HOST=%s GH_REPO=%s\n' "${{GH_HOST-unset}}" "${{GH_REPO-unset}}" >> "$h/env"
case "$*" in
  "api -X "*) printf '%s\n' "$*" >> "$h/writes"; echo "this host refuses writes" >&2; exit 1 ;;
  "auth status --hostname github.com") exit 0 ;;
  "repo view"*) printf '%s' '{{"nameWithOwner":"o/r","isPrivate":false,"defaultBranchRef":{{"name":"main"}}}}' ;;
  "api repos/o/r/rulesets?"*) printf '[{{"id":301,"name":"main checks","target":"branch","source_type":"Repository"}},{{"id":302,"name":"main history","target":"branch","source_type":"Repository"}},{{"id":303,"name":"registry guard","target":"branch","source_type":"Repository"}}]' ;;
  "api repos/o/r/rulesets/"*) cat "$h/rs-${{2##*/}}.json" ;;
  "api $wf?ref=main") printf '[{{"path":".github/workflows/codeflow-policy.yml","type":"file"}},{{"path":".github/workflows/codeflow-ci.yml","type":"file"}}]' ;;
  *"$wf/codeflow-policy.yml?ref=main") cat "$h/wf/policy.yml" ;;
  *"$wf/codeflow-ci.yml?ref=main") cat "$h/wf/ci.yml" ;;
  "api repos/o/r/branches/main") printf '{{"name":"main"}}' ;;
  "api repos/o/r/rules/branches/codeflow/registry?"*) [ -f "$h/rules-down" ] && {{ echo "gh: HTTP 502 Bad Gateway" >&2; exit 1; }}; printf '[{{"type":"non_fast_forward","ruleset_source_type":"Repository","ruleset_source":"o/r","ruleset_id":303}},{{"type":"deletion","ruleset_source_type":"Repository","ruleset_source":"o/r","ruleset_id":303}}]' ;;
  "api repos/o/r/branches/codeflow/registry/protection") echo "gh: Branch not protected (HTTP 404)" >&2; exit 1 ;;
  "api repos/o/r/branches/"*) echo "gh: Branch not found (HTTP 404)" >&2; exit 1 ;;
  *) echo "unexpected gh call: $*" >&2; exit 1 ;;
esac
"#
    );
    let gh = dir.join("bin/gh");
    std::fs::write(&gh, script).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    host
}

/// A full-tier project whose registry is seeded and whose `origin` names
/// `o/r` on GitHub, with issue 200's host on `PATH`: the project root, the
/// host's folder and the `bin` that holds its `gh`.
#[cfg(unix)]
fn seeded_split_project(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let bare = dir.join("remote.git");
    git(dir, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
    let root = dir.join("proj");
    std::fs::create_dir(&root).unwrap();
    // The installed hooks run the binary under test, never the one on the
    // user's PATH.
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let exe_dir = exe.parent().unwrap();
    let init = codeflow(&root, Some(exe_dir), &["init", "--yes", "--full"]);
    assert!(init.status.success(), "{}", text(&init));
    // The remote's default branch holds the scaffold, as a project's does.
    git(
        &bare,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            "HEAD:refs/heads/main",
        ],
    );
    git(&root, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&root, &["fetch", "-q", "origin"]);
    git(&root, &["remote", "set-head", "origin", "-a"]);
    let seed = codeflow(&root, Some(exe_dir), &["ids", "seed"]);
    assert!(seed.status.success(), "{}", text(&seed));
    git(&root, &["fetch", "-q", "origin"]);
    // The fetched refs stay; from here the host is the stand-in. The URL
    // only names the repository to the gh stub: git in this repository
    // refuses https, so no git transport reaches the network.
    git(&root, &["config", "protocol.https.allow", "never"]);
    git(
        &root,
        &["remote", "set-url", "origin", "https://github.com/o/r.git"],
    );
    let host = split_host(&root);
    let bin = root.join("bin");
    (root, host, bin)
}

/// TSK-383 AC-5: against a host holding split `main` rulesets and a manual
/// `codeflow/registry` ruleset, the built `doctor` judges the registry from
/// the live rules: a ruleset an actor can bypass does not protect it from
/// that actor and the actor is named (review of PR 700, finding 1), and the
/// same ruleset binding everyone passes, though it was made by hand.
/// `remote protect --dry-run` plans nothing for either `main` ruleset or
/// for the absent `master`, and sends no write.
#[cfg(unix)]
#[test]
fn doctor_reads_the_live_registry_rules_and_the_dry_run_leaves_split_main_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (root, host, bin) = seeded_split_project(dir.path());

    let doctor = codeflow(&root, Some(&bin), &["doctor", "--check", "id-registry"]);
    let report = text(&doctor);
    println!("{report}");
    assert!(
        report.contains(
            "reduced assurance: the live host rules on codeflow/registry do not block force pushes for everyone (ruleset 303 lets RepositoryRole 5 (always) bypass it)"
        ),
        "{report}"
    );
    assert!(
        !report.contains("host rules on codeflow/registry block"),
        "{report}"
    );

    std::fs::write(host.join("rs-303.json"), MANUAL_REGISTRY_BOUND).unwrap();
    let doctor = codeflow(&root, Some(&bin), &["doctor", "--check", "id-registry"]);
    let report = text(&doctor);
    println!("{report}");
    assert!(
        report.contains(
            "host rules on codeflow/registry block force pushes and deletion (ruleset 303)"
        ),
        "{report}"
    );
    assert!(!report.contains("reduced assurance"), "{report}");

    let dry = codeflow(&root, Some(&bin), &["remote", "protect", "--dry-run"]);
    let report = text(&dry);
    println!("{report}");
    assert!(dry.status.success(), "{report}");
    assert!(report.contains("status: dry-run"), "{report}");
    assert!(
        report.contains(
            "policy branch 'master' does not exist on o/r, so no ruleset is planned for it"
        ),
        "{report}"
    );
    let requests: Vec<&str> = report
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("request: "))
        .collect();
    assert_eq!(requests.len(), 4, "A, B, C and the registry: {report}");
    assert!(
        requests
            .iter()
            .all(|request| request.starts_with("POST repos/o/r/rulesets {")),
        "{report}"
    );
    for untouched in [
        "rulesets/301",
        "rulesets/302",
        "rulesets/303",
        "codeflow protect master",
    ] {
        assert!(
            requests.iter().all(|request| !request.contains(untouched)),
            "{untouched}: {report}"
        );
    }
    assert!(!host.join("writes").exists(), "a write was sent: {report}");
    let calls = std::fs::read_to_string(host.join("calls")).unwrap();
    assert!(!calls.contains("-X"), "{calls}");
    for (id, body) in [
        (301, SPLIT_MAIN_CHECKS),
        (302, SPLIT_MAIN_HISTORY),
        (303, MANUAL_REGISTRY_BOUND),
    ] {
        assert_eq!(
            std::fs::read_to_string(host.join(format!("rs-{id}.json"))).unwrap(),
            body
        );
    }
}

/// Review of PR 700, finding 2: when the live rules cannot be read, the
/// registry check warns with the read error and the step that restores the
/// read, even though `remote protect` recorded the rules as applied.
#[cfg(unix)]
#[test]
fn an_unread_host_is_never_a_pass_even_with_the_local_record() {
    let dir = tempfile::tempdir().unwrap();
    let (root, host, bin) = seeded_split_project(dir.path());
    std::fs::write(host.join("rs-303.json"), MANUAL_REGISTRY_BOUND).unwrap();
    // The record `remote protect` keeps once it applied the registry rules.
    let state = root.join(".git/codeflow/registry-state.json");
    let mut record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&state).unwrap_or_else(|_| "{}".into()))
            .unwrap();
    record["data_profile"] = serde_json::json!({ "origin": "2026-11-07T00:00:00Z" });
    std::fs::write(&state, record.to_string()).unwrap();
    std::fs::write(host.join("rules-down"), "").unwrap();

    let doctor = codeflow(&root, Some(&bin), &["doctor", "--check", "id-registry"]);
    let report = text(&doctor);
    println!("{report}");
    assert!(
        report.contains(
            "reduced assurance: `remote protect` recorded the host rules as applied, but the live rules on codeflow/registry were not read"
        ),
        "{report}"
    );
    assert!(report.contains("HTTP 502"), "{report}");
    assert!(
        report.contains("`gh auth status` shows a login whose token can read"),
        "{report}"
    );
    assert!(!report.contains("ok    id-registry"), "{report}");
}

/// Review of PR 700, finding 4: a push URL for another repository leaves
/// `origin` naming no one repository, so doctor does not vouch for the rules
/// it reads, and `remote protect` asks the host nothing.
#[cfg(unix)]
#[test]
fn a_push_url_for_another_repository_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (root, host, bin) = seeded_split_project(dir.path());
    std::fs::write(host.join("rs-303.json"), MANUAL_REGISTRY_BOUND).unwrap();
    git(
        &root,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            "https://github.com/x/y.git",
        ],
    );
    let doctor = codeflow(&root, Some(&bin), &["doctor", "--check", "id-registry"]);
    let report = text(&doctor);
    println!("{report}");
    assert!(
        report.contains("`origin` pushes to https://github.com/x/y.git, not to o/r"),
        "{report}"
    );
    assert!(report.contains("reduced assurance"), "{report}");
    let before = std::fs::read_to_string(host.join("calls")).unwrap_or_default();
    let refused = codeflow(&root, Some(&bin), &["remote", "protect"]);
    let report = text(&refused);
    println!("{report}");
    assert!(
        report.contains("`origin` pushes to https://github.com/x/y.git, not to o/r"),
        "{report}"
    );
    assert_eq!(
        std::fs::read_to_string(host.join("calls")).unwrap_or_default(),
        before,
        "remote protect asked the host about a repository it cannot name: {report}"
    );
}

/// Review of PR 700, finding 4: an inherited `GH_HOST` or `GH_REPO` never
/// reaches `gh`, so neither can point a read or a write at another host or
/// repository. `codeflow()` sets both to decoys. Doctor reads github.com
/// only, so its `gh` is pinned to github.com, never to `gh`'s own default
/// host (review of PR 700, delta); `remote protect` pins the host `origin`
/// names on every call, the login check first.
#[cfg(unix)]
#[test]
fn gh_never_sees_an_inherited_host_or_repository() {
    let dir = tempfile::tempdir().unwrap();
    let (root, host, bin) = seeded_split_project(dir.path());
    let doctor = codeflow(&root, Some(&bin), &["doctor", "--check", "id-registry"]);
    println!("{}", text(&doctor));
    let env = std::fs::read_to_string(host.join("env")).unwrap();
    assert!(!env.is_empty());
    assert!(
        env.lines()
            .all(|line| line == "GH_HOST=github.com GH_REPO=unset"),
        "{env}"
    );
    std::fs::remove_file(host.join("env")).unwrap();
    std::fs::remove_file(host.join("calls")).unwrap();
    let dry = codeflow(&root, Some(&bin), &["remote", "protect", "--dry-run"]);
    println!("{}", text(&dry));
    let env = std::fs::read_to_string(host.join("env")).unwrap();
    assert!(env.lines().count() > 2, "{env}");
    assert!(
        env.lines()
            .all(|line| line == "GH_HOST=github.com GH_REPO=unset"),
        "{env}"
    );
    let calls = std::fs::read_to_string(host.join("calls")).unwrap();
    assert_eq!(
        calls.lines().next(),
        Some("auth status --hostname github.com"),
        "{calls}"
    );
}
