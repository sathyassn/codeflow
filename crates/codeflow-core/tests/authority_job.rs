//! TSK-1177 (ADR-0091 "Check identity"): the authority job of the policy
//! workflow and the refusal step of every policy job.
//!
//! The structural tests read the shipped template, its installed baseline
//! and this repository's copy, one per property of AC-1. The behavioural
//! tests (Unix) run the steps' own scripts with `bash`, the real `jq` and
//! `openssl`, and a fake `curl` that answers from canned GitHub responses
//! and logs every request, so the token handling, the check-run calls and
//! each Mode B refusal are exercised without a host. The hosted fixtures
//! of AC-3 are separate.
use serde_yaml::Value;
use std::path::{Path, PathBuf};

const COPIES: [&str; 3] = [
    "assets/base/ci/codeflow-policy.yml",
    ".codeflow/.baseline/.github/workflows/codeflow-policy.yml",
    ".github/workflows/codeflow-policy.yml",
];

const GUARD: &str = "Refuse another producer of a trusted name";
const CREATE: &str = "Create the authority checks";
const HOST_RULES: &str = "Read the host rules for the authority line";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn read(path: &str) -> (String, Value) {
    let text = std::fs::read_to_string(repo_root().join(path)).unwrap();
    let value = serde_yaml::from_str(&text).unwrap_or_else(|error| panic!("{path}: {error}"));
    (text, value)
}

/// Every workflow file this repository runs or ships, with its path.
fn all_workflows() -> Vec<(String, Value)> {
    let mut found = Vec::new();
    for dir in [
        ".github/workflows",
        "assets/base/ci",
        ".codeflow/.baseline/.github/workflows",
    ] {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(repo_root().join(dir))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "yml"))
            .collect();
        paths.sort();
        for path in paths {
            let relative = format!("{dir}/{}", path.file_name().unwrap().to_string_lossy());
            let (_, value) = read(&relative);
            if value.get("jobs").is_some() {
                found.push((relative, value));
            }
        }
    }
    found
}

fn jobs(workflow: &Value) -> Vec<(String, Value)> {
    workflow["jobs"]
        .as_mapping()
        .unwrap()
        .iter()
        .map(|(id, job)| (id.as_str().unwrap().to_owned(), job.clone()))
        .collect()
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn steps(job: &Value) -> Vec<Value> {
    job["steps"].as_sequence().cloned().unwrap_or_default()
}

fn step<'a>(job: &'a Value, name: &str) -> &'a Value {
    job["steps"]
        .as_sequence()
        .unwrap()
        .iter()
        .find(|step| str_of(step, "name") == name)
        .unwrap_or_else(|| panic!("no step {name}"))
}

/// `(job id, name)` of each `trusted:` verdict job.
fn verdict_jobs(workflow: &Value) -> Vec<(String, String)> {
    jobs(workflow)
        .into_iter()
        .map(|(id, job)| (id, str_of(&job, "name").to_owned()))
        .filter(|(_, name)| name.starts_with("trusted: "))
        .collect()
}

fn authority(workflow: &Value) -> Value {
    workflow["jobs"]["authority"].clone()
}

/// The non-empty lines of a block scalar.
fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Every `${{ ... }}` expression in `text`, trimmed.
fn expressions(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("${{") {
        let after = &rest[start + 3..];
        let end = after.find("}}").expect("an expression closes");
        found.push(after[..end].trim().to_owned());
        rest = &after[end + 2..];
    }
    found
}

/// The copies are the shipped template, the baseline byte for byte, and
/// this repository's copy.
#[test]
fn the_baseline_is_the_template() {
    assert_eq!(read(COPIES[0]).0, read(COPIES[1]).0);
}

/// AC-1: the job runs only for `pull_request_target` into the default
/// branch, after the verdict jobs, and also when one of them failed.
#[test]
fn the_authority_job_runs_only_for_a_pull_request_into_the_default_branch() {
    for path in COPIES {
        let (_, workflow) = read(path);
        let job = authority(&workflow);
        assert_eq!(
            str_of(&job, "if"),
            "${{ !cancelled() && github.event_name == 'pull_request_target' && github.event.pull_request.base.ref == github.event.repository.default_branch }}",
            "{path}"
        );
        let resolve = steps(&job)[0].clone();
        assert_eq!(str_of(&resolve, "name"), "Resolve the authority", "{path}");
        let run = str_of(&resolve, "run");
        assert!(run.contains(r#"[ "$GITHUB_REF" != "refs/heads/${DEFAULT_BRANCH}" ]"#));
        assert!(run.contains(r#"[ "$BASE_REF" != "$DEFAULT_BRANCH" ]"#));
        assert!(run.contains(r#"[ "$EVENT" != pull_request_target ]"#));
    }
}

/// AC-1: exactly one job in the repository references the environment
/// `codeflow-authority`, the authority job of the policy workflow; no job
/// names an environment through an expression, which could resolve to it.
#[test]
fn only_the_authority_job_references_the_environment() {
    let mut references = Vec::new();
    for (path, workflow) in all_workflows() {
        for (id, job) in jobs(&workflow) {
            if let Some(environment) = job.get("environment") {
                let name = environment
                    .as_str()
                    .or_else(|| environment.get("name").and_then(Value::as_str))
                    .unwrap_or("(unnamed)");
                if name == "codeflow-authority" || name.contains("${{") || name == "(unnamed)" {
                    references.push((path.clone(), id, environment.clone()));
                }
            }
        }
    }
    let expected: Vec<(String, String, Value)> = [COPIES[2], COPIES[0], COPIES[1]]
        .iter()
        .map(|path| {
            (
                (*path).to_owned(),
                "authority".to_owned(),
                Value::String("codeflow-authority".to_owned()),
            )
        })
        .collect();
    let mut sorted = references.clone();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut want = expected;
    want.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(sorted, want);
}

/// AC-1: the App key is read once, by the step that creates the checks,
/// and the token it mints never leaves that step: no job output, no step
/// output, environment file or path entry, no third-party action, masked,
/// scoped to Checks write on this repository and revoked on exit.
#[test]
fn the_installation_token_is_confined_to_one_step() {
    for (path, workflow) in all_workflows() {
        let text = serde_yaml::to_string(&workflow).unwrap();
        let secrets = text.matches("secrets.").count();
        if COPIES.contains(&path.as_str()) {
            assert_eq!(secrets, 1, "{path}");
        }
        for (id, job) in jobs(&workflow) {
            if id != "authority" {
                assert!(
                    !serde_yaml::to_string(&job)
                        .unwrap()
                        .contains("CODEFLOW_AUTHORITY_APP"),
                    "{path} {id}"
                );
            }
        }
    }
    for path in COPIES {
        let (_, workflow) = read(path);
        let job = authority(&workflow);
        assert!(job.get("outputs").is_none(), "{path}");
        let permissions = serde_yaml::to_string(&job["permissions"]).unwrap();
        assert!(!permissions.contains("write"), "{path}: {permissions}");
        for step in steps(&job) {
            assert!(step.get("uses").is_none(), "{path}: no action runs here");
            assert!(
                step.get("id").is_none(),
                "{path}: no step output is addressable"
            );
            let run = str_of(&step, "run");
            for sink in ["GITHUB_OUTPUT", "GITHUB_ENV", "GITHUB_PATH"] {
                assert!(!run.contains(sink), "{path}: {sink}");
            }
            let env = serde_yaml::to_string(&step["env"]).unwrap();
            if str_of(&step, "name") == CREATE {
                assert!(env.contains("APP_KEY: ${{ secrets.CODEFLOW_AUTHORITY_APP_KEY }}"));
                assert!(env.contains("APP_ID: ${{ vars.CODEFLOW_AUTHORITY_APP_ID }}"));
            } else {
                assert!(!env.contains("CODEFLOW_AUTHORITY_APP"), "{path}");
            }
        }
        let run = str_of(step(&job, CREATE), "run");
        for needed in [
            "unset APP_KEY",
            r#"echo "::add-mask::${jwt}""#,
            r#"echo "::add-mask::${token}""#,
            r#"permissions: {checks: "write"}"#,
            r#"-X DELETE -H @"${work}/token-headers" "${GITHUB_API_URL}/installation/token""#,
            "trap finish EXIT",
            r#"rm -f "${work}/key.pem""#,
        ] {
            assert!(run.contains(needed), "{path}: {needed}");
        }
        assert!(
            !run.contains("Bearer ${"),
            "{path}: a credential reaches curl only through a header file"
        );
    }
}

/// AC-1: the job executes no file from any branch and expands no untrusted
/// field: no checkout or git command, and every expression is one of the
/// trusted values below, used only in `if:` and `env:`, never in a script.
#[test]
fn the_authority_job_runs_no_head_file_and_expands_no_untrusted_field() {
    for path in COPIES {
        let (_, workflow) = read(path);
        let job = authority(&workflow);
        let verdicts: Vec<String> = verdict_jobs(&workflow)
            .into_iter()
            .map(|(id, _)| format!("needs.{id}.result"))
            .collect();
        let allowed = |expression: &str| {
            [
                "github.event_name",
                "github.event.repository.default_branch",
                "github.event.pull_request.base.ref",
                "github.event.pull_request.head.sha",
                "github.sha",
                "github.token",
                "vars.CODEFLOW_AUTHORITY_APP_ID",
                "secrets.CODEFLOW_AUTHORITY_APP_KEY",
                "!cancelled() && github.event_name == 'pull_request_target' && github.event.pull_request.base.ref == github.event.repository.default_branch",
            ]
            .contains(&expression)
                || verdicts.iter().any(|verdict| verdict == expression)
        };
        for expression in expressions(&serde_yaml::to_string(&job).unwrap()) {
            assert!(allowed(&expression), "{path}: ${{{{ {expression} }}}}");
        }
        for step in steps(&job) {
            let run = str_of(&step, "run");
            assert!(
                !run.contains("${{"),
                "{path}: an expression inside a script"
            );
            for forbidden in ["git ", "actions/checkout", "python", "source ", ". ./"] {
                assert!(!run.contains(forbidden), "{path}: {forbidden}");
            }
        }
    }
}

/// AC-1: one `authority:` check per verdict job, each named literally in
/// the workflow (so `remote protect --check-app` and `doctor` find it) and
/// concluded from that job's result through `needs`, created
/// `in_progress` before any is completed.
#[test]
fn each_authority_check_is_concluded_from_its_verdict_job() {
    for path in COPIES {
        let (_, workflow) = read(path);
        let job = authority(&workflow);
        let verdicts = verdict_jobs(&workflow);
        let needs: Vec<String> = job["needs"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|need| need.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            needs,
            verdicts
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>(),
            "{path}"
        );
        let create = step(&job, CREATE);
        let expected: Vec<String> = verdicts
            .iter()
            .map(|(id, name)| {
                format!(
                    "authority: {}=${{{{ needs.{id}.result }}}}",
                    name.trim_start_matches("trusted: ")
                )
            })
            .collect();
        assert_eq!(
            lines(str_of(&create["env"], "VERDICTS")),
            expected,
            "{path}"
        );
        let run = str_of(create, "run");
        let created = run.find(r#"status: "in_progress""#).unwrap();
        let completed = run.find(r#"status: "completed""#).unwrap();
        assert!(created < completed, "{path}");
        assert!(run.contains(r#"if [ "$result" = success ]; then conclusion=success; fi"#));
    }
}

/// AC-1, Mode B: the step of every verdict job just before its last read
/// of the pull request, and the authority job's step just before it
/// creates any check (TSK-1177 review nit: a refusal there leaves the
/// `authority:` checks uncreated), is the same refusal over the trusted
/// names the workflow declares.
#[test]
fn every_policy_job_runs_the_same_refusal() {
    let mut scripts = Vec::new();
    for path in COPIES {
        let (_, workflow) = read(path);
        let declared: Vec<String> = verdict_jobs(&workflow)
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        for (id, job) in jobs(&workflow) {
            let all = steps(&job);
            let last = if id == "authority" {
                let create = all
                    .iter()
                    .position(|step| str_of(step, "name") == CREATE)
                    .unwrap();
                all[create - 1].clone()
            } else {
                // The verdict job ends with the refusal, then the last read
                // of the pull request (TSK-1040), each followed by the
                // checkout assertion.
                let n = all.len();
                for at in [n - 1, n - 3] {
                    assert_eq!(
                        str_of(&all[at], "name"),
                        "Assert the authority checkout",
                        "{path} {id}"
                    );
                }
                assert_eq!(
                    str_of(&all[n - 2], "name"),
                    "the pull request is unchanged since it was judged",
                    "{path} {id}"
                );
                all[n - 4].clone()
            };
            assert_eq!(str_of(&last, "name"), GUARD, "{path} {id}");
            assert_eq!(
                lines(str_of(&last["env"], "TRUSTED_NAMES")),
                declared,
                "{path} {id}"
            );
            assert_eq!(
                str_of(&last["env"], "HEAD_SHA"),
                "${{ github.event.pull_request.head.sha }}"
            );
            let permissions = serde_yaml::to_string(&job["permissions"]).unwrap();
            for read in ["actions: read", "checks: read", "statuses: read"] {
                assert!(permissions.contains(read), "{path} {id}: {read}");
            }
            scripts.push(str_of(&last, "run").to_owned());
        }
    }
    assert!(scripts.len() >= 5);
    assert!(scripts.windows(2).all(|pair| pair[0] == pair[1]));
    let run = &scripts[0];
    for needed in [
        "set -euo pipefail",
        "filter=all",
        r#"[ "$app" != 15368 ]"#,
        "actions/runs?check_suite_id=",
        r#"[ "$event" != pull_request_target ] || [ "$path" != "$workflow" ]"#,
        "/statuses?per_page=100",
        r#"map("authority: " + ltrimstr("trusted: "))"#,
    ] {
        assert!(run.contains(needed), "{needed}");
    }
}

/// AC-2: the commit-standards job reads the host rules into a file the
/// report names, and that step can never fail the verdict.
#[test]
fn the_report_reads_the_host_rules_without_risking_the_verdict() {
    for path in COPIES {
        let (_, workflow) = read(path);
        let job = workflow["jobs"]["commit-lint"].clone();
        let host = step(&job, HOST_RULES);
        let run = str_of(host, "run");
        assert!(run.starts_with("set -uo pipefail\n"), "{path}: no -e");
        assert!(run.contains("rules/branches/${branch}"));
        assert!(run.contains("rulesets?includes_parents=true"));
        let ci = step(&job, "codeflow ci (commit + branch + PR-body standards)");
        assert_eq!(
            str_of(&ci["env"], "CODEFLOW_HOST_RULES"),
            "${{ runner.temp }}/codeflow-host-rules.json"
        );
    }
}

#[cfg(unix)]
mod run {
    use super::{authority, read, step, str_of, CREATE, GUARD, HOST_RULES};
    use std::fmt::Write as _;
    use std::path::Path;
    use std::process::{Command, Output};

    /// A canned answer: method, a text of the URL, and the body or `!fail`.
    type Route<'a> = (&'a str, &'a str, &'a str);
    /// A named case: what it is, its routes, and what the step must print.
    type Case<'a> = (&'a str, Vec<(&'a str, &'a str, String)>, &'a str);

    const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const AUTHORITY: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const WORKFLOW: &str = ".github/workflows/codeflow-policy.yml";

    /// A fake `curl`: each request is logged (argument list, header file,
    /// body) to `calls.log`, and answered from the first route whose method
    /// matches and whose text is in the URL; `!fail` fails the request.
    const FAKE_CURL: &str = r#"#!/usr/bin/env bash
method=GET; url=; out=; data=; headers=
printf '== %s\n' "$*" >> "$FAKE_DIR/calls.log"
while [ $# -gt 0 ]; do
  case "$1" in
    -X) method=$2; shift 2 ;;
    -o) out=$2; shift 2 ;;
    -H) case "$2" in @*) headers=$(cat "${2#@}") ;; esac; shift 2 ;;
    --data) data=$2; shift 2 ;;
    --retry|--max-time) shift 2 ;;
    -*) shift ;;
    *) url=$1; shift ;;
  esac
done
body=
case "$data" in @*) body=$(cat "${data#@}") ;; *) body=$data ;; esac
printf 'request: %s %s\n%s\nbody: %s\n' "$method" "$url" "$headers" "$body" >> "$FAKE_DIR/calls.log"
while IFS=$'\t' read -r m pattern file; do
  if [ "$m" = "$method" ] && [[ "$url" == *"$pattern"* ]]; then
    if [ "$file" = '!fail' ]; then echo "curl: (22) The requested URL returned error: 500" >&2; exit 22; fi
    if [ -n "$out" ]; then cp "$FAKE_DIR/$file" "$out"; else cat "$FAKE_DIR/$file"; fi
    exit 0
  fi
done < "$FAKE_DIR/routes"
echo "curl: (22) no route for $method $url" >&2
exit 22
"#;

    struct Host {
        dir: tempfile::TempDir,
    }

    impl Host {
        fn new(routes: &[(&str, &str, &str)]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let bin = dir.path().join("bin");
            std::fs::create_dir(&bin).unwrap();
            std::fs::create_dir(dir.path().join("tmp")).unwrap();
            let curl = bin.join("curl");
            std::fs::write(&curl, FAKE_CURL).unwrap();
            std::process::Command::new("chmod")
                .args(["+x"])
                .arg(&curl)
                .status()
                .unwrap();
            let mut table = String::new();
            for (index, (method, pattern, body)) in routes.iter().enumerate() {
                let file = if *body == "!fail" {
                    "!fail".to_owned()
                } else {
                    let name = format!("answer-{index}.json");
                    std::fs::write(dir.path().join(&name), body).unwrap();
                    name
                };
                let _ = writeln!(table, "{method}\t{pattern}\t{file}");
            }
            std::fs::write(dir.path().join("routes"), table).unwrap();
            std::fs::write(dir.path().join("calls.log"), "").unwrap();
            Self { dir }
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }

        /// Run `script` as GitHub runs a `run:` step: bash with `-eo
        /// pipefail`, the runner's variables and the step's `env`.
        fn run(&self, script: &str, env: &[(&str, &str)]) -> Output {
            let file = self.path().join("step.sh");
            std::fs::write(&file, script).unwrap();
            let path = format!(
                "{}:{}",
                self.path().join("bin").display(),
                std::env::var("PATH").unwrap_or_default()
            );
            let mut command = Command::new("bash");
            command
                .args(["--noprofile", "--norc", "-eo", "pipefail"])
                .arg(&file)
                .env("PATH", path)
                .env("FAKE_DIR", self.path())
                .env("RUNNER_TEMP", self.path().join("tmp"))
                .env("GITHUB_STEP_SUMMARY", self.path().join("summary"))
                .env("GITHUB_API_URL", "https://api.test")
                .env("GITHUB_SERVER_URL", "https://github.test")
                .env("GITHUB_REPOSITORY", "o/r")
                .env("GITHUB_RUN_ID", "9")
                .env("GITHUB_REF", "refs/heads/main")
                .env(
                    "GITHUB_WORKFLOW_REF",
                    format!("o/r/{WORKFLOW}@refs/heads/main"),
                );
            for (key, value) in env {
                command.env(key, value);
            }
            command.output().expect("bash runs")
        }

        fn calls(&self) -> String {
            std::fs::read_to_string(self.path().join("calls.log")).unwrap()
        }

        fn temp_is_empty(&self) -> bool {
            std::fs::read_dir(self.path().join("tmp"))
                .unwrap()
                .next()
                .is_none()
        }
    }

    fn template_job(id: &str) -> serde_yaml::Value {
        let (_, workflow) = read(super::COPIES[0]);
        if id == "authority" {
            authority(&workflow)
        } else {
            workflow["jobs"][id].clone()
        }
    }

    fn guard_script() -> String {
        str_of(step(&template_job("authority"), GUARD), "run").to_owned()
    }

    fn text(output: &Output) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    // -- the refusal (Mode B) ------------------------------------------------

    const NAMES: &str = "trusted: commit standards\ntrusted: release impact\n";
    const STANDARDS: &str = "check-runs?check_name=trusted%3A%20commit%20standards&filter=all";
    const RELEASE: &str = "check-runs?check_name=trusted%3A%20release%20impact&filter=all";

    fn runs(list: &[(u64, u64, u64)]) -> String {
        let runs: Vec<String> = list
            .iter()
            .map(|(id, app, suite)| {
                format!(
                    r#"{{"id": {id}, "app": {{"id": {app}}}, "check_suite": {{"id": {suite}}}}}"#
                )
            })
            .collect();
        format!(
            r#"{{"total_count": {}, "check_runs": [{}]}}"#,
            list.len(),
            runs.join(",")
        )
    }

    fn suite(event: &str, path: &str) -> String {
        format!(r#"{{"workflow_runs": [{{"event": "{event}", "path": "{path}"}}]}}"#)
    }

    fn guard(routes: &[(&str, &str, &str)]) -> (Host, Output) {
        let host = Host::new(routes);
        let output = host.run(
            &guard_script(),
            &[
                ("GH_TOKEN", "ghs_workflow"),
                ("HEAD_SHA", HEAD),
                ("TRUSTED_NAMES", NAMES),
            ],
        );
        (host, output)
    }

    /// The genuine checks of this workflow, an earlier run's included,
    /// and an unrelated status pass.
    #[test]
    fn the_genuine_verdict_checks_pass() {
        let genuine = suite("pull_request_target", WORKFLOW);
        let (host, output) = guard(&[
            ("GET", STANDARDS, &runs(&[(1, 15368, 10), (2, 15368, 11)])),
            ("GET", RELEASE, &runs(&[(3, 15368, 10)])),
            ("GET", "check_suite_id=10", &genuine),
            ("GET", "check_suite_id=11", &genuine),
            (
                "GET",
                "/statuses?",
                r#"[{"id": 7, "context": "codeflow gates", "state": "success"}]"#,
            ),
        ]);
        assert!(output.status.success(), "{}", text(&output));
        assert!(text(&output).contains(&format!(
            "no other producer of a trusted or authority name is on {HEAD} now; Mode B does not see one that runs later"
        )));
        let calls = host.calls();
        assert_eq!(
            calls.matches("check_suite_id=10").count(),
            2,
            "one lookup per suite: {calls}"
        );
        for line in calls.lines().filter(|line| line.starts_with("== ")) {
            assert!(!line.contains("ghs_workflow"), "{line}");
        }
        assert!(
            calls.contains("Authorization: Bearer ghs_workflow"),
            "{calls}"
        );
        assert!(host.temp_is_empty());
    }

    /// Each named case fails closed and names the producer.
    #[test]
    fn every_other_producer_fails_closed() {
        let genuine = suite("pull_request_target", WORKFLOW);
        let empty = runs(&[]);
        let cases: Vec<Case> = vec![
            (
                "a check run from another app",
                vec![("GET", STANDARDS, runs(&[(1, 4242, 10)]))],
                "check run 1 named 'trusted: commit standards' is on",
            ),
            (
                "a head job of that name on pull_request, skipped or sleeping",
                vec![
                    ("GET", STANDARDS, runs(&[(1, 15368, 10), (2, 15368, 20)])),
                    ("GET", "check_suite_id=10", genuine.clone()),
                    ("GET", "check_suite_id=20", suite("pull_request", WORKFLOW)),
                ],
                "a pull_request run of .github/workflows/codeflow-policy.yml created it",
            ),
            (
                "an expression-built name in another workflow file",
                vec![
                    ("GET", STANDARDS, runs(&[(2, 15368, 20)])),
                    ("GET", "check_suite_id=20", suite("pull_request_target", ".github/workflows/other.yml")),
                ],
                "a pull_request_target run of .github/workflows/other.yml created it",
            ),
            (
                "an Actions check run whose suite has no run",
                vec![
                    ("GET", STANDARDS, runs(&[(2, 15368, 20)])),
                    ("GET", "check_suite_id=20", r#"{"workflow_runs": []}"#.to_owned()),
                ],
                "a (no run) run of (no run) created it",
            ),
            (
                "a commit status of a trusted name",
                vec![(
                    "GET",
                    "/statuses?",
                    r#"[{"id": 8, "context": "trusted: commit standards", "state": "success", "creator": {"login": "agent"}}]"#.to_owned(),
                )],
                "commit status 8 named 'trusted: commit standards' (success, set by agent)",
            ),
            (
                "a commit status of an authority name",
                vec![(
                    "GET",
                    "/statuses?",
                    r#"[{"id": 9, "context": "authority: release impact", "state": "pending"}]"#.to_owned(),
                )],
                "commit status 9 named 'authority: release impact' (pending, set by ?)",
            ),
        ];
        for (case, mut routes, says) in cases {
            routes.push(("GET", "check-runs?", empty.clone()));
            routes.push(("GET", "/statuses?", "[]".to_owned()));
            let borrowed: Vec<(&str, &str, &str)> = routes
                .iter()
                .map(|(m, p, b)| (*m, *p, b.as_str()))
                .collect();
            let (_, output) = guard(&borrowed);
            let out = text(&output);
            assert_eq!(output.status.code(), Some(1), "{case}: {out}");
            assert!(out.contains(says), "{case}: {out}");
            assert!(out.contains("so this check fails;"), "{case}: {out}");
            // A check run cannot be removed from a commit, so the way out
            // is a new head commit, never a re-run (review N4).
            assert!(
                out.contains("then add a new commit to the pull request"),
                "{case}: {out}"
            );
        }
    }

    /// An API error or an answer of the wrong shape fails the step.
    #[test]
    fn an_api_error_fails_closed() {
        let empty = runs(&[]);
        let one = runs(&[(1, 15368, 10)]);
        let cases: Vec<(&str, Vec<Route>)> = vec![
            (
                "check runs unreadable",
                vec![("GET", "check-runs?", "!fail")],
            ),
            (
                "check runs not a list",
                vec![("GET", "check-runs?", r#"{"message": "x"}"#)],
            ),
            (
                "suite unreadable",
                vec![
                    ("GET", STANDARDS, &one),
                    ("GET", "check_suite_id=10", "!fail"),
                ],
            ),
            ("statuses unreadable", vec![("GET", "/statuses?", "!fail")]),
            (
                "statuses not a list",
                vec![("GET", "/statuses?", r#"{"message": "x"}"#)],
            ),
        ];
        for (case, mut routes) in cases {
            routes.push(("GET", "check-runs?", &empty));
            routes.push(("GET", "/statuses?", "[]"));
            let (_, output) = guard(&routes);
            assert!(!output.status.success(), "{case}: {}", text(&output));
        }
    }

    /// A full page of check runs reads the next page.
    #[test]
    fn every_page_is_read() {
        let full: Vec<(u64, u64, u64)> = (1..=100).map(|id| (id, 15368, 10)).collect();
        let (host, output) = guard(&[
            (
                "GET",
                &format!("{STANDARDS}&per_page=100&page=1"),
                &runs(&full),
            ),
            (
                "GET",
                &format!("{STANDARDS}&per_page=100&page=2"),
                &runs(&[(101, 4242, 10)]),
            ),
            (
                "GET",
                "check_suite_id=10",
                &suite("pull_request_target", WORKFLOW),
            ),
            ("GET", "check-runs?", &runs(&[])),
            ("GET", "/statuses?", "[]"),
        ]);
        assert_eq!(output.status.code(), Some(1), "{}", text(&output));
        assert!(text(&output).contains("check run 101"), "{}", text(&output));
        assert!(host.calls().contains("&page=2"));
    }

    // -- the authority checks (Mode A) ---------------------------------------

    fn create_script() -> String {
        str_of(step(&template_job("authority"), CREATE), "run").to_owned()
    }

    fn key(dir: &Path) -> String {
        let key = dir.join("app.pem");
        let status = Command::new("openssl")
            .args([
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:2048",
                "-out",
            ])
            .arg(&key)
            .output()
            .unwrap();
        assert!(status.status.success());
        let public = Command::new("openssl")
            .args(["pkey", "-pubout", "-in"])
            .arg(&key)
            .arg("-out")
            .arg(dir.join("app.pub"))
            .output()
            .unwrap();
        assert!(public.status.success());
        std::fs::read_to_string(key).unwrap()
    }

    fn app_routes() -> Vec<(&'static str, &'static str, &'static str)> {
        vec![
            ("GET", "/repos/o/r/installation", r#"{"id": 77}"#),
            (
                "POST",
                "/app/installations/77/access_tokens",
                r#"{"token": "ghs_installation"}"#,
            ),
            ("POST", "/repos/o/r/check-runs", r#"{"id": 5}"#),
            ("PATCH", "/repos/o/r/check-runs/5", "{}"),
            ("DELETE", "/installation/token", ""),
        ]
    }

    fn create(host: &Host, app_id: &str, app_key: &str, verdicts: &str) -> Output {
        host.run(
            &create_script(),
            &[
                ("APP_ID", app_id),
                ("APP_KEY", app_key),
                ("DEFAULT_BRANCH", "main"),
                ("AUTHORITY_SHA", AUTHORITY),
                ("HEAD_SHA", HEAD),
                ("VERDICTS", verdicts),
            ],
        )
    }

    /// Mode B: no App, no request and no check; half an App fails.
    #[test]
    fn without_the_app_no_check_is_created() {
        let host = Host::new(&[]);
        let output = create(&host, "", "", "authority: commit standards=success\n");
        assert!(output.status.success(), "{}", text(&output));
        assert!(text(&output).contains("Mode B: no authority App is configured"));
        assert_eq!(host.calls(), "");
        for (id, key) in [("123", ""), ("", "a key with no id")] {
            let output = create(&host, id, key, "authority: commit standards=success\n");
            assert_eq!(output.status.code(), Some(1), "{}", text(&output));
            assert!(text(&output).contains("half configured"));
        }
        let output = create(&host, "12;x", "k", "");
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(host.calls(), "");
    }

    /// Mode A: a JWT GitHub accepts (RS256 over the App id, at most ten
    /// minutes), a token scoped to Checks write on this repository and
    /// masked before use, each check created `in_progress` on the head and
    /// concluded from its verdict, the token revoked, and no credential on
    /// any command line or left on disk.
    #[test]
    fn with_the_app_each_check_is_created_and_concluded_from_its_verdict() {
        let host = Host::new(&app_routes());
        let app_key = key(host.path());
        let output = create(
            &host,
            "123",
            &app_key,
            "authority: commit standards=failure\nauthority: release impact=success\n",
        );
        let out = text(&output);
        assert!(output.status.success(), "{out}");
        let calls = host.calls();
        let order: Vec<&str> = calls
            .lines()
            .filter_map(|line| line.strip_prefix("request: "))
            .collect();
        assert_eq!(
            order,
            [
                "GET https://api.test/repos/o/r/installation",
                "POST https://api.test/app/installations/77/access_tokens",
                "POST https://api.test/repos/o/r/check-runs",
                "POST https://api.test/repos/o/r/check-runs",
                "PATCH https://api.test/repos/o/r/check-runs/5",
                "PATCH https://api.test/repos/o/r/check-runs/5",
                "DELETE https://api.test/installation/token",
            ]
        );
        assert!(
            calls.contains(r#"body: {"repositories":["r"],"permissions":{"checks":"write"}}"#),
            "{calls}"
        );
        assert!(calls.contains(&format!(
            r#"body: {{"name":"authority: commit standards","head_sha":"{HEAD}","status":"in_progress","details_url":"https://github.test/o/r/actions/runs/9"}}"#
        )), "{calls}");
        assert!(calls.contains(r#"body: {"status":"completed","conclusion":"failure","output":{"title":"trusted: commit standards failure""#), "{calls}");
        assert!(calls.contains(r#"body: {"status":"completed","conclusion":"success","output":{"title":"trusted: release impact success""#), "{calls}");
        assert!(calls.contains(&format!("judged by main@{AUTHORITY}")));
        // The installation token is used, and revoked, only through header
        // files after it is masked.
        let mask = out
            .find("::add-mask::ghs_installation")
            .expect("token masked");
        assert!(out.find("ghs_installation").unwrap() >= mask);
        for line in calls.lines().filter(|line| line.starts_with("== ")) {
            assert!(
                !line.contains("ghs_installation") && !line.contains("Bearer"),
                "{line}"
            );
            assert!(!line.contains("PRIVATE KEY"), "{line}");
        }
        assert!(host.temp_is_empty(), "the key and token files are removed");
        // The JWT verifies against the App's public key.
        let jwt = calls
            .lines()
            .find_map(|line| line.strip_prefix("Authorization: Bearer "))
            .unwrap()
            .to_owned();
        assert!(out.contains(&format!("::add-mask::{jwt}")));
        let verify = Command::new("bash")
            .arg("-c")
            .arg(
                r#"set -euo pipefail
decode() { local s; s=$(printf '%s' "$1" | tr '_-' '/+'); while [ $(( ${#s} % 4 )) -ne 0 ]; do s="${s}="; done; printf '%s' "$s" | openssl base64 -d -A; }
IFS=. read -r h p s <<< "$JWT"
printf '%s.%s' "$h" "$p" > "$DIR/signed"
decode "$s" > "$DIR/signature"
openssl dgst -sha256 -verify "$DIR/app.pub" -signature "$DIR/signature" "$DIR/signed" >/dev/null
decode "$h" | jq -c .
decode "$p" | jq -c '{iss, span: (.exp - .iat)}'"#,
            )
            .env("JWT", &jwt)
            .env("DIR", host.path())
            .output()
            .unwrap();
        let shown = text(&verify);
        assert!(verify.status.success(), "{shown}");
        assert!(shown.contains(r#"{"typ":"JWT","alg":"RS256"}"#), "{shown}");
        assert!(shown.contains(r#"{"iss":"123","span":600}"#), "{shown}");
    }

    /// A failed conclusion leaves the check `in_progress` (a required
    /// check that never passes), fails the step and still revokes the
    /// token; an App not installed creates nothing.
    #[test]
    fn a_failed_request_fails_closed_and_still_revokes() {
        let mut routes = app_routes();
        routes[3] = ("PATCH", "/repos/o/r/check-runs/5", "!fail");
        let host = Host::new(&routes);
        let app_key = key(host.path());
        let output = create(
            &host,
            "123",
            &app_key,
            "authority: commit standards=success\n",
        );
        assert!(!output.status.success(), "{}", text(&output));
        let calls = host.calls();
        assert!(
            calls.contains("request: DELETE https://api.test/installation/token"),
            "{calls}"
        );
        assert!(host.temp_is_empty());

        let host = Host::new(&[("GET", "/repos/o/r/installation", "!fail")]);
        let app_key = key(host.path());
        let output = create(
            &host,
            "123",
            &app_key,
            "authority: commit standards=success\n",
        );
        assert!(!output.status.success());
        assert!(!host.calls().contains("check-runs"));
        assert!(!host.calls().contains("DELETE"), "no token was minted");
        assert!(host.temp_is_empty());

        let host = Host::new(&app_routes());
        let app_key = key(host.path());
        let output = create(
            &host,
            "123",
            &app_key,
            "trusted: commit standards=success\n",
        );
        assert_eq!(output.status.code(), Some(1), "{}", text(&output));
        assert!(text(&output).contains("is not an authority: name"));
        assert!(!host.calls().contains("check-runs"));
    }

    // -- the host rules for the report ---------------------------------------

    /// The host rules land in the file the report reads, and an API error
    /// writes an unreadable note and still passes.
    #[test]
    fn the_host_rules_step_never_fails() {
        let job = template_job("commit-lint");
        let script = str_of(step(&job, HOST_RULES), "run").to_owned();
        let env = [
            ("GH_TOKEN", "ghs_workflow"),
            ("DEFAULT_BRANCH", "main"),
            ("AUTHORITY_SHA", AUTHORITY),
        ];
        let host = Host::new(&[
            (
                "GET",
                "/rules/branches/main?per_page=100",
                r#"[{"type": "update", "ruleset_id": 1}]"#,
            ),
            (
                "GET",
                "/rulesets?includes_parents=true",
                r#"[{"id": 1, "name": "codeflow authority updates"}]"#,
            ),
        ]);
        let output = host.run(&script, &env);
        assert!(output.status.success(), "{}", text(&output));
        let written: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(host.path().join("tmp/codeflow-host-rules.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(written["branch"], "main");
        assert_eq!(written["authority"], AUTHORITY);
        assert_eq!(written["rules"][0]["ruleset_id"], 1);
        assert_eq!(written["rulesets"][0]["name"], "codeflow authority updates");

        let host = Host::new(&[("GET", "/rules/branches/", "!fail")]);
        let output = host.run(&script, &env);
        assert!(output.status.success(), "{}", text(&output));
        let written: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(host.path().join("tmp/codeflow-host-rules.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            written["unreadable"],
            "the host rules could not be read with this token"
        );
    }
}
