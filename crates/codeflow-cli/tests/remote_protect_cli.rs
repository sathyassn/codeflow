//! `codeflow remote protect` at the command line (TSK-1080): the help names
//! the profiles, the check identity, the line pattern and the host-side
//! reduction route; bad flags are refused before anything is read; and two
//! runs against a recorded GitHub write the three owned rulesets once,
//! never touch a foreign ruleset, and change nothing the second time.
//!
//! The host is a `gh` stand-in on `PATH` that answers from files: it records
//! GitHub's REST shapes, and the hosted fixture on a scratch repository is
//! owed (TSK-1080 AC-2, AC-4, AC-6).

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

#[test]
fn the_dry_run_names_the_three_rulesets_and_reads_no_host() {
    let dir = project();
    let out = codeflow(
        dir.path(),
        None,
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
    // The dry run reads no host, so it says which pattern a live run skips.
    assert!(
        report
            .contains("a policy pattern that names the default branch gets no ruleset of its own"),
        "{report}"
    );
}

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

const CI: &str = "
on: pull_request
jobs:
  gates:
    name: codeflow gates
  windows-verdict:
    name: windows
";

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

    let second = codeflow(dir.path(), Some(&bin), &["remote", "protect"]);
    let report = text(&second);
    assert!(second.status.success(), "{report}");
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
