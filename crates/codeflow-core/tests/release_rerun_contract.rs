//! Every job that publishes refuses its own re-run as its first step
//! (TSK-1346).
//!
//! A re-run of failed jobs, or of one job, runs only those jobs and the jobs
//! that depend on them; a gate job that passed before does not run again.
//! GitHub sets `GITHUB_RUN_ATTEMPT` above 1 on every re-run, so the check
//! sits in each publishing job itself, before any step that publishes. The
//! exec-guard cannot judge a re-run: a run id names no workflow it can read
//! without the network.
//!
//! The workflows checked are the shipped templates and this repository's
//! `.github/workflows`, as the checkout holds them. The check must be one
//! canonical text, byte for byte, so no variant can hide a command, and no
//! later step of the job may run after it fails: a step `if` that calls a
//! status function drops the implicit `success()`.

use std::path::{Path, PathBuf};

use codeflow_core::scaffold::{DirSource, ScaffoldManifest};
use regex::Regex;
use serde_yaml::Value;

/// Step actions that publish, deploy or attest.
const PUBLISHING_ACTIONS: &[&str] = &[
    "actions/deploy-pages",
    "actions/create-release",
    "actions/upload-release-asset",
    "actions/attest",
    "softprops/action-gh-release",
    "ncipollo/release-action",
    "pypa/gh-action-pypi-publish",
];

/// Shell text that publishes: a release write, a dist host, a tag or push,
/// or a registry publish.
const PUBLISHING_RUNS: &[&str] = &[
    r"\bgh\s+release\s+(create|edit|upload|delete)\b",
    r"\bdist\b[^\n]*\bhost\b",
    r"\bgit\s+(push|tag)\b",
    r"\b(cargo|npm)\s+publish\b",
    r"\btwine\s+upload\b",
    r"\bdocker\s+push\b",
];

/// A `gh api` call that writes: a write method, or a field or input, which
/// makes gh send a POST unless the method says otherwise.
const GH_API: &str = r"\bgh\s+api\b[^\n]*";
const WRITE_METHOD: &str = r"(-X|--method)[\s=]*(POST|PUT|PATCH|DELETE)\b";
const GET_METHOD: &str = r"(-X|--method)[\s=]*(GET|HEAD)\b";
const IMPLIED_BODY: &str = r"\s(-f|-F|--field|--raw-field|--input)[\s=]";

/// An HTTP client call to the GitHub API host that sends a body or a write
/// method.
const HTTP_API: &str =
    r"\b(curl|wget|http|https|xh|xhs)\b[^\n]*\b(api|uploads)\.github\.com\b[^\n]*";
const HTTP_WRITE: &str = r"((-X|--request|--method)[\s=]*(POST|PUT|PATCH|DELETE)\b|\s(-d|--data[a-z-]*|--json|-F|--form|-T|--upload-file|--post-data|--post-file|--body-data|--body-file)[\s=]|\s(POST|PUT|PATCH|DELETE)\s)";

/// The re-run check's `run`, as YAML parses it: the only accepted text.
const CANONICAL: &str = "if [ \"$GITHUB_RUN_ATTEMPT\" != 1 ] && [ \"$GITHUB_EVENT_NAME\" != pull_request ]; then
  echo \"::error::This job publishes, and attempt $GITHUB_RUN_ATTEMPT is a re-run. Dispatch a fresh run instead; a re-run would publish without the jobs that passed before.\"
  exit 1
fi
";

/// GitHub's status functions: a step `if` that calls one runs without the
/// implicit `success()`, so it can run after the check has failed.
const STATUS: &str = r"\b(always|failure|cancelled|success)\s*\(";

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

/// The shell text of `run`, with continued lines joined.
fn joined(run: &str) -> String {
    run.replace("\\\n", " ")
}

/// Why `job` publishes: a deployment environment, a reusable workflow, a
/// publishing action or a publishing command. Empty when it does not.
fn publishing(job: &Value) -> Vec<String> {
    let runs: Vec<Regex> = PUBLISHING_RUNS.iter().map(|p| re(p)).collect();
    let (gh_api, write, get, implied) = (
        re(GH_API),
        re(WRITE_METHOD),
        re(GET_METHOD),
        re(IMPLIED_BODY),
    );
    let (http_api, http_write) = (re(HTTP_API), re(HTTP_WRITE));
    let mut why = Vec::new();
    if job.get("environment").is_some() {
        why.push("it deploys to an environment".to_owned());
    }
    if let Some(uses) = job["uses"].as_str() {
        why.push(format!("it calls reusable workflow {uses}"));
    }
    for step in job["steps"].as_sequence().into_iter().flatten() {
        if let Some(uses) = step["uses"].as_str() {
            if PUBLISHING_ACTIONS.iter().any(|a| uses.starts_with(a)) {
                why.push(format!("it uses {uses}"));
            }
        }
        let Some(run) = step["run"].as_str() else {
            continue;
        };
        let run = joined(run);
        if let Some(found) = runs.iter().find_map(|r| r.find(&run)) {
            why.push(format!("it runs `{}`", found.as_str()));
        }
        for call in gh_api.find_iter(&run) {
            let call = call.as_str();
            if write.is_match(call) || (implied.is_match(call) && !get.is_match(call)) {
                why.push(format!("it runs `{call}`"));
            }
        }
        for call in http_api.find_iter(&run) {
            if http_write.is_match(call.as_str()) {
                why.push(format!("it runs `{}`", call.as_str()));
            }
        }
    }
    why
}

/// Whether `key` is set in the `env` of `level`.
fn sets(level: &Value, key: &str) -> bool {
    level["env"].get(key).is_some()
}

/// Why `step` is not the re-run refusal, or `None` when it is: it always
/// runs in bash, nothing overrides the variables it reads, and its `run` is
/// the canonical text.
fn not_a_refusal(step: &Value, job: &Value, workflow: &Value) -> Option<String> {
    if step.get("if").is_some() || step.get("continue-on-error").is_some() {
        return Some("it has an `if` or `continue-on-error`".into());
    }
    if job.get("continue-on-error").is_some() {
        return Some("its job has `continue-on-error`".into());
    }
    if step["shell"].as_str() != Some("bash") {
        return Some("it does not run in bash".into());
    }
    for name in ["GITHUB_RUN_ATTEMPT", "GITHUB_EVENT_NAME"] {
        if [step, job, workflow].iter().any(|level| sets(level, name)) {
            return Some(format!("an `env` sets {name}"));
        }
    }
    if step["run"].as_str() != Some(CANONICAL) {
        return Some("its `run` is not the canonical re-run check".into());
    }
    None
}

/// The first later step of `steps` whose `if` can run it after the check
/// has failed.
fn runs_after_failure(steps: &[Value]) -> Option<String> {
    let status = re(STATUS);
    steps.iter().skip(1).find_map(|step| {
        let condition = match &step["if"] {
            Value::Null => return None,
            Value::String(text) => text.clone(),
            other => format!("{other:?}"),
        };
        status.is_match(&condition).then(|| {
            format!("a later step has `if: {condition}`, which runs after the check fails")
        })
    })
}

/// The publishing jobs of `text` and each fault: a job whose first step is
/// not the re-run refusal, or a call to a reusable workflow this test
/// cannot read. `local` holds the workflow files that are checked too.
fn judge(label: &str, text: &str, local: &[String]) -> (Vec<String>, Vec<String>) {
    let workflow: Value = serde_yaml::from_str(text).unwrap_or_else(|e| panic!("{label}: {e}"));
    let mut publishing_jobs = Vec::new();
    let mut faults = Vec::new();
    for (name, job) in workflow["jobs"].as_mapping().into_iter().flatten() {
        let name = format!("{label} jobs.{}", name.as_str().unwrap_or("?"));
        let why = publishing(job);
        if why.is_empty() {
            continue;
        }
        publishing_jobs.push(name.clone());
        if let Some(uses) = job["uses"].as_str() {
            // The called workflow's own jobs carry the check, when it is
            // one of the files this test reads.
            if !uses
                .strip_prefix("./")
                .is_some_and(|path| local.iter().any(|l| l == path))
            {
                faults.push(format!(
                    "{name} calls {uses}, a reusable workflow this test cannot read"
                ));
            }
            continue;
        }
        let steps = job["steps"].as_sequence().map_or(&[][..], Vec::as_slice);
        let fault = match steps.first() {
            None => Some("it has no steps".to_owned()),
            Some(step) => not_a_refusal(step, job, &workflow).or_else(|| runs_after_failure(steps)),
        };
        if let Some(fault) = fault {
            faults.push(format!(
                "{name} publishes ({}) but its first step does not refuse a re-run: {fault}",
                why.join("; ")
            ));
        }
    }
    (publishing_jobs, faults)
}

fn workflows(root: &Path) -> Vec<PathBuf> {
    let assets = DirSource::new(root.join("assets"));
    let manifest = ScaffoldManifest::load(&assets).unwrap();
    let mut files: Vec<PathBuf> = manifest
        .entries
        .iter()
        .filter(|entry| entry.dest.starts_with(".github/workflows/"))
        .map(|entry| root.join("assets/base").join(&entry.src))
        .collect();
    assert!(!files.is_empty(), "shipped workflows must be checked");
    let mut own: Vec<PathBuf> = std::fs::read_dir(root.join(".github/workflows"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "yml" || e == "yaml"))
        .collect();
    own.sort();
    files.extend(own);
    files
}

#[test]
fn every_publishing_job_refuses_a_rerun_first() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let files = workflows(&root);
    let labels: Vec<String> = files
        .iter()
        .map(|path| {
            path.strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    let mut found = Vec::new();
    let mut faults = Vec::new();
    for (path, label) in files.iter().zip(&labels) {
        let (jobs, wrong) = judge(label, &std::fs::read_to_string(path).unwrap(), &labels);
        found.extend(jobs);
        faults.extend(wrong);
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
    // The markers must keep finding the jobs that publish today.
    for job in [
        ".github/workflows/release.yml jobs.plan",
        ".github/workflows/release.yml jobs.host",
        ".github/workflows/release.yml jobs.announce",
        ".github/workflows/release-plan-authority.yml jobs.verify",
        ".github/workflows/portal-pages.yml jobs.deploy",
    ] {
        assert!(
            found.iter().any(|f| f == job),
            "{job} is not found as publishing: {found:?}"
        );
    }
}

/// The step `docs/releasing.md` gives adopters is the canonical text, so an
/// adopter who copies the example passes this test.
#[test]
fn the_documented_check_is_the_canonical_text() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let docs = std::fs::read_to_string(root.join("docs/releasing.md"))
        .unwrap()
        .replace("\r\n", "\n");
    let block = docs
        .split("```yaml\n")
        .skip(1)
        .filter_map(|rest| rest.split_once("```").map(|(block, _)| block))
        .find(|block| block.contains("Refuse a re-run of a publishing job"))
        .expect("docs/releasing.md shows the re-run check");
    let steps: Value = serde_yaml::from_str(block).unwrap();
    assert_eq!(steps[0]["shell"].as_str(), Some("bash"));
    assert_eq!(steps[0]["run"].as_str(), Some(CANONICAL));
}

const CHECK: &str = "      - name: Refuse a re-run of a publishing job
        shell: bash
        run: |
          if [ \"$GITHUB_RUN_ATTEMPT\" != 1 ] && [ \"$GITHUB_EVENT_NAME\" != pull_request ]; then
            echo \"::error::This job publishes, and attempt $GITHUB_RUN_ATTEMPT is a re-run. Dispatch a fresh run instead; a re-run would publish without the jobs that passed before.\"
            exit 1
          fi
";

fn fixture(job: &str, steps: &str) -> String {
    format!("on: workflow_dispatch\njobs:\n  ship:\n    runs-on: ubuntu-latest\n{job}    steps:\n{steps}")
}

fn judged(job: &str, steps: &str) -> (usize, usize) {
    let (jobs, faults) = judge("fixture", &fixture(job, steps), &[]);
    (jobs.len(), faults.len())
}

#[test]
fn a_publishing_job_without_the_check_first_is_found() {
    let publish = "      - run: gh release create v1 --notes x\n";
    let checkout = "      - uses: actions/checkout@v6\n";
    let with_check = |from: &str, to: &str| CHECK.replace(from, to) + publish;
    for (job, steps) in [
        ("", publish.to_owned()),
        ("", format!("{checkout}{CHECK}{publish}")),
        ("", with_check("        shell: bash\n", "        if: always()\n        shell: bash\n")),
        ("", with_check("        shell: bash\n", "        continue-on-error: true\n        shell: bash\n")),
        ("", with_check("        shell: bash\n", "")),
        ("", with_check("        shell: bash\n", "        shell: bash\n        env:\n          GITHUB_RUN_ATTEMPT: 1\n")),
        ("    continue-on-error: true\n", format!("{CHECK}{publish}")),
        ("    env:\n      GITHUB_EVENT_NAME: pull_request\n", format!("{CHECK}{publish}")),
        // An inverted or always-false comparison.
        ("", with_check("!= 1 ]", "== 1 ]")),
        ("", with_check("!= 1 ]", "!= 0 ]")),
        ("", with_check("!= 1 ]", "!= 1 ] || true")),
        ("", with_check("!= pull_request ]", "!= pull_request ] && false")),
        // A branch that echoes the exit, exits zero, or runs first.
        ("", with_check("exit 1", "echo exit 1")),
        ("", with_check("exit 1", "exit 0")),
        ("", with_check("            exit 1\n", "            true\n            exit 1\n")),
        ("", with_check("            exit 1\n", "          else\n            exit 1\n")),
        // A comparison that is not the first command.
        ("", with_check("          if [", "          exit 0\n          if [")),
        // A compound behind an echo, or any other text than the canonical.
        ("", with_check("            exit 1\n", "            echo refused && gh release create v1\n            exit 1\n")),
        ("", with_check("            exit 1\n", "            echo $(gh release create v1)\n            exit 1\n")),
        ("", with_check("exit 1", "exit 2")),
        ("", with_check(" && [ \"$GITHUB_EVENT_NAME\" != pull_request ]", "")),
        // A later step that runs after the check fails.
        ("", format!("{CHECK}      - if: always()\n        run: gh release create v1\n")),
        ("", format!("{CHECK}      - if: ${{{{ failure() }}}}\n        run: gh release create v1\n")),
        ("", format!("{CHECK}      - if: ${{{{ !success() }}}}\n        run: gh release create v1\n")),
        ("", format!("{CHECK}      - if: cancelled()\n        run: gh release create v1\n")),
        ("", format!("{CHECK}      - if: success() || github.ref != ''\n        run: gh release create v1\n")),
        // Other ways to publish.
        ("    environment: release\n", "      - run: echo deploy\n".to_owned()),
        ("", "      - uses: actions/deploy-pages@v4\n".to_owned()),
        ("", "      - run: git push origin v1\n".to_owned()),
        ("", "      - run: gh api -X POST repos/o/r/releases -f tag_name=v1\n".to_owned()),
        ("", "      - run: gh api repos/o/r/releases -f tag_name=v1\n".to_owned()),
        ("", "      - run: |\n          gh api repos/o/r/git/refs \\\n            -f ref=refs/tags/v1 -f sha=x\n".to_owned()),
        ("", "      - run: curl -X POST https://api.github.com/repos/o/r/releases -d '{}'\n".to_owned()),
        ("", "      - run: curl https://uploads.github.com/repos/o/r/releases/1/assets -T x\n".to_owned()),
    ] {
        assert_eq!(judged(job, &steps), (1, 1), "{job}{steps}");
    }
    for (job, steps) in [
        ("", format!("{CHECK}{publish}")),
        (
            "",
            format!("{CHECK}      - if: github.ref == 'refs/heads/main'\n        run: gh release create v1\n"),
        ),
        (
            "    environment: release\n",
            format!("{CHECK}      - run: echo deploy\n"),
        ),
    ] {
        assert_eq!(judged(job, &steps), (1, 0), "{job}{steps}");
    }
    // Reads publish nothing.
    for steps in [
        "      - run: cargo test\n      - run: gh api repos/o/r/releases\n",
        "      - run: gh api -X GET repos/o/r/rulesets -f per_page=100\n",
        "      - run: curl -sSfL https://github.com/o/r/releases/download/v1/x -o x\n",
        "      - run: curl -s https://api.github.com/repos/o/r/releases\n",
    ] {
        assert_eq!(judged("", steps), (0, 0), "{steps}");
    }
    // A call to a reusable workflow is read through the called file.
    let calls = |uses: &str, local: &[String]| {
        let text = format!("on: workflow_dispatch\njobs:\n  call:\n    uses: {uses}\n");
        let (jobs, faults) = judge("fixture", &text, local);
        (jobs.len(), faults.len())
    };
    let local = [".github/workflows/publish.yml".to_owned()];
    assert_eq!(calls("./.github/workflows/publish.yml", &local), (1, 0));
    assert_eq!(calls("./.github/workflows/other.yml", &local), (1, 1));
    assert_eq!(
        calls("o/r/.github/workflows/publish.yml@v1", &local),
        (1, 1)
    );
}
