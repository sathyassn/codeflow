//! Every catalogued remedy is proven by the step it prints (TSK-147 AC-1,
//! review finding F5).
//!
//! The inventory in `warning_inventory.rs` proves that each remedy names a
//! command that exists. This file proves the stronger claim, for each row
//! of the remedy catalogue in one of three ways, recorded in [`ROWS`]:
//!
//! - [`Proof::Runs`]: a test raises the finding, runs the step exactly as
//!   the output prints it, and sees the finding gone on the next run.
//! - [`Proof::Confirms`]: the finding is a note naming an event that
//!   confirms what doctor cannot read, and stays a note, since doctor keeps
//!   no record of runs. A test runs the printed event and shows that it
//!   tells the states apart; it does not see the note gone.
//! - [`Proof::Excluded`]: the step acts outside this machine, at a boundary
//!   from a closed set ([`Boundary`]).
//!
//! The coverage test fails on a row with none of these, and on an exclusion
//! whose remedy does not name the party beyond the boundary.
//!
//! A proof takes the command it runs from the printed remedy, so a remedy
//! that names a real command which does not clear its finding fails here.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use codeflow_core::remedy::{Clearing, Step, CATALOG};

mod guards;
mod push;
mod records;

/// How a catalogue row is covered.
enum Proof {
    /// A `clears_<row>` test runs the printed step and sees the finding go.
    Runs,
    /// A `confirms_<row>` test runs the printed confirmation event and shows
    /// that its outcome differs between the states the note cannot tell
    /// apart; the note itself stays.
    Confirms,
    /// The step acts beyond this boundary, so no test can take it.
    Excluded(Boundary),
}

/// Where a step acts that no test can reach. The set is closed: setup cost,
/// elapsed time, a local git remote or synthetic planning state is never
/// one of them (TSK-147 review F5).
#[derive(Clone, Copy, Debug)]
enum Boundary {
    /// Settings a hosting platform keeps for a remote, such as branch
    /// rules; a git remote a test can host is not this.
    HostingRemote,
    /// A service reached over the network: a download or a live model run.
    Network,
    /// An approval inside another harness's own interface.
    HarnessApproval,
    /// A decision or a credential that belongs to a human operator.
    HumanAuthority,
}

impl Boundary {
    /// Words one of which the row's remedy must use to name the party
    /// beyond the boundary, so a row cannot claim a boundary its step does
    /// not cross.
    fn named_by(self) -> &'static [&'static str] {
        match self {
            Self::HostingRemote => &["host rules"],
            Self::Network => &["network", "release build", "native canary"],
            Self::HarnessApproval => &["inside that harness"],
            Self::HumanAuthority => &["operator", "a human", "sign in"],
        }
    }
}

use Boundary::{HarnessApproval, HostingRemote, HumanAuthority, Network};
use Proof::{Confirms, Excluded, Runs};

/// Every catalogue row and how it is covered.
const ROWS: &[(&str, Proof)] = &[
    ("DISCARD_LOCAL_WORK", Runs),
    ("OUTWARD_ACTION", Excluded(HumanAuthority)),
    ("PUSH_WITHOUT_FOLLOW_TAGS", Runs),
    ("TASK_STATUS", Runs),
    ("EPIC_STATUS", Runs),
    ("SUPERSEDED_CITATION", Runs),
    ("SPEC_NO_CONSUMER", Runs),
    ("SPEC_WRITTEN_IMPLEMENTED", Runs),
    ("BASELINE_HISTORY", Runs),
    ("DUAL_IDENTITY", Runs),
    ("STANDALONE_SPLIT", Runs),
    ("DOCS_LAYER_ABSENT", Runs),
    ("DOCS_EPICS_UNCHECKED", Runs),
    ("DOCS_SPECS_UNCHECKED", Runs),
    ("DOCS_ADRS_UNCHECKED", Runs),
    ("DOCS_CAPABILITIES_UNCHECKED", Runs),
    ("WORK_START_RECONCILE", Runs),
    ("WORK_START_MERGE_PLANNING", Runs),
    ("TASK_RECORD_MISSING", Runs),
    ("WORKGRAPH_INVALID", Runs),
    ("ID_REGISTRY", Runs),
    ("ID_REGISTRY_UNFETCHED", Runs),
    ("ID_REGISTRY_RETARGET", Runs),
    ("ID_REGISTRY_UID", Runs),
    ("ACCEPTANCE_BINDING", Runs),
    ("EPIC_ACCEPTANCE_BINDING", Runs),
    ("ACCEPTANCE_BOUND", Excluded(HumanAuthority)),
    ("CRITERIA_DELTA", Excluded(HumanAuthority)),
    ("RELEASE_LEGACY_CHANGE", Excluded(HumanAuthority)),
    ("RELEASE_LEGACY_RECORD", Excluded(HumanAuthority)),
    ("JOURNEY_CRITERION", Runs),
    ("RECORD_BASELINE_EXEMPT", Runs),
    ("BASELINE_REVIEW", Runs),
    ("POLICY_DEPRECATED", Runs),
    ("PROTECTED_BRANCH", Runs),
    ("PROTECTED_DELETE", Runs),
    ("PROTECTED_REWRITE", Runs),
    ("REMOTE_TRACKING_REF", Runs),
    ("FORCE_PUSH", Runs),
    ("PR_MERGE_PROTECTED", Runs),
    ("BRANCH_NAME", Runs),
    ("ROOT_CHECKOUT_COMMIT", Runs),
    ("HOOK_INTEGRITY", Runs),
    ("JUDGE_SOURCE_DRIFT", Excluded(HumanAuthority)),
    ("COMMIT_TYPE", Runs),
    ("COMMIT_LENGTH", Runs),
    ("COMMIT_BLANK_LINE", Runs),
    ("COMMIT_BREAKING_FOOTER", Runs),
    ("COMMIT_BODY", Runs),
    ("COMMIT_REQUIRED_FOOTERS", Runs),
    ("COMMIT_TICKET", Runs),
    ("COMMIT_AI_ATTRIBUTION", Runs),
    ("COMMIT_EMOJI", Runs),
    ("COMMIT_POLICY_CHARACTER", Runs),
    ("FILE_POLICY_CHARACTER", Runs),
    ("CONFLICT_MARKER", Runs),
    ("GIT_ATTR_SOURCE_UNSUPPORTED", Excluded(Network)),
    ("BREAKING_WATCH_PATH", Runs),
    ("PR_POLICY_CHARACTER", Runs),
    ("PR_AI_ATTRIBUTION", Runs),
    ("PR_EMOJI", Runs),
    ("PR_BODY_MISSING", Runs),
    ("PR_SECTION_DUPLICATE", Runs),
    ("PR_SECTION_EMPTY", Runs),
    ("PR_SECTION_MISSING", Runs),
    ("PR_TEMPLATE_REMNANT", Runs),
    ("PR_PRESENTATION", Runs),
    ("PR_RELEASE_IMPACT", Runs),
    ("CI_BASE_UNRESOLVED", Runs),
    ("CI_BASE_REFUSED", Runs),
    ("CI_RANGE_UNREADABLE", Runs),
    ("CI_BRANCH_UNRESOLVED", Runs),
    ("CI_BODY_UNSUPPLIED", Runs),
    ("SCAFFOLD_MANIFEST_BROKEN", Excluded(Network)),
    ("ENV_FILE_STAGED", Runs),
    ("SECRET_SCAN_INCOMPLETE", Runs),
    ("SECRET_STAGED", Runs),
    ("PUSH_SET_FAILED", Runs),
    ("PUSH_SET_CI", Runs),
    ("PUSH_SET_VALIDATE", Runs),
    ("PUSH_SET_BY_HAND", Runs),
    ("PUSH_TREE_UNCHECKED", Runs),
    ("PUSH_RANGE_UNRESOLVED", Runs),
    ("PUSH_DESTINATION_SILENT", Runs),
    ("PUSH_REWRITE", Runs),
    ("PUSH_OVER_BUDGET_TARGET", Runs),
    ("PUSH_OVER_BUDGET_BUILTIN", Runs),
    ("PUSH_TARGETS_UNCONFIGURED", Runs),
    ("PUSH_TARGETS_NONE", Runs),
    ("IDS_PENDING_LOCAL", Runs),
    ("IDS_SYNC_FAILED", Runs),
    ("RELEASE_PREFLIGHT_NOTE", Runs),
    ("RELEASE_PREFLIGHT_UNRUN", Runs),
    ("RELEASE_PREFLIGHT", Runs),
    ("TEST_CONFIG_REPAIR", Runs),
    ("DOCTOR_HOOKS_PATH", Runs),
    ("DOCTOR_INIT", Runs),
    ("DOCTOR_TOOL_MISSING", Runs),
    ("DOCTOR_HOOK_MANAGER", Runs),
    ("DOCTOR_HOOK_WIRING_UNSEEN", Confirms),
    ("DOCTOR_GIT_DIR_HOOKS", Runs),
    ("DOCTOR_HARNESS_APPROVAL", Excluded(HarnessApproval)),
    ("DOCTOR_GROK_HOOKS", Runs),
    ("DOCTOR_GROK_UNMANAGED_HOOKS", Runs),
    ("DOCTOR_GROK_MISSING_GUARD", Runs),
    ("DOCTOR_NETWORK", Excluded(Network)),
    ("DOCTOR_DELEGATES", Runs),
    ("DOCTOR_DELEGATES_SIGN_IN", Excluded(HumanAuthority)),
    ("DOCTOR_REQUALIFY", Excluded(HumanAuthority)),
    ("DOCTOR_UNSEEN", Excluded(HarnessApproval)),
    ("DOCTOR_CANARY", Excluded(Network)),
    ("DOCTOR_POLICY_DECISION", Runs),
    ("DOCTOR_RELEASE_BACKEND", Runs),
    ("DOCTOR_CI_PLACEHOLDER", Runs),
    ("DOCTOR_CI_PIN_MISSING", Runs),
    ("DOCTOR_CI_PIN_TARGET", Runs),
    ("DOCTOR_CI_PIN_LOWERED", Runs),
    ("DOCTOR_CI_PIN_ORDER", Runs),
    ("DOCTOR_TRACKING_UNKNOWN", Runs),
    ("DOCTOR_ID_REGISTRY", Runs),
    ("DOCTOR_REGISTRY_UNPROTECTED", Excluded(HostingRemote)),
    ("DOCTOR_MANAGED_DRIFT", Runs),
    ("DOCTOR_CUSTOMIZATION", Runs),
    ("DOCTOR_INSTRUCTIONS", Runs),
    ("DOCTOR_READING", Runs),
    ("DOCTOR_ROOT_CHECKOUT", Runs),
    ("DOCTOR_TEST_CONFIG", Runs),
    ("GUARD_UNCLASSIFIABLE", Runs),
    ("GUARD_UNRESOLVED", Runs),
    ("GUARD_ALIAS", Runs),
    ("HEADLESS_PEER_RUN", Runs),
    ("HOOK_UNEVALUATED", Runs),
    ("HOOK_STDIN_UNREAD", Runs),
    ("GUARD_PAYLOAD_MALFORMED", Runs),
    ("SESSION_SUMMARY_UNWRITTEN", Runs),
    ("REFUSAL_UNRECORDED", Runs),
    ("REGISTRY_UNWRITTEN", Runs),
    ("PRIVILEGE_ESCALATION", Excluded(HumanAuthority)),
];

#[test]
fn every_catalogued_row_is_proven_or_excluded_at_a_boundary() {
    let source = [
        include_str!("main.rs"),
        include_str!("guards.rs"),
        include_str!("push.rs"),
        include_str!("records.rs"),
    ]
    .concat();
    let listed: Vec<&str> = ROWS.iter().map(|(name, _)| *name).collect();
    let names: Vec<&str> = CATALOG.iter().map(|clearing| clearing.name).collect();
    let mut missing: Vec<&str> = names
        .iter()
        .filter(|name| !listed.contains(name))
        .copied()
        .collect();
    missing.extend(listed.iter().filter(|name| !names.contains(name)));
    assert!(
        missing.is_empty(),
        "rows missing from ROWS or from the catalogue: {missing:?}"
    );
    for (name, proof) in ROWS {
        match proof {
            Runs => {
                let test = format!("fn clears_{}()", name.to_ascii_lowercase());
                assert!(
                    source.contains(&test),
                    "{name} is marked Runs but has no `{test}` test"
                );
            }
            Confirms => {
                let test = format!("fn confirms_{}()", name.to_ascii_lowercase());
                assert!(
                    source.contains(&test),
                    "{name} is marked Confirms but has no `{test}` test"
                );
            }
            Excluded(boundary) => {
                let text = catalogued(name).text;
                assert!(
                    boundary.named_by().iter().any(|word| text.contains(word)),
                    "{name} is excluded at {boundary:?}, but its remedy names no party \
                     beyond it ({:?}): {text}",
                    boundary.named_by()
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The harness.
// ---------------------------------------------------------------------------

fn exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codeflow"))
}

fn home() -> &'static Path {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().unwrap()).path()
}

/// A command with the binary under test first on `PATH` (the git hooks
/// run it) and no user configuration.
fn command(program: &str, dir: &Path) -> Command {
    let path = std::env::join_paths(exe().parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let mut cmd = Command::new(program);
    cmd.current_dir(dir)
        .env("PATH", path)
        .env("CODEFLOW_HOME", home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Proof")
        .env("GIT_AUTHOR_EMAIL", "proof@example.test")
        .env("GIT_COMMITTER_NAME", "Proof")
        .env("GIT_COMMITTER_EMAIL", "proof@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("BITBUCKET_PR_ID")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME");
    cmd
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn run(program: &str, dir: &Path, args: &[&str]) -> Output {
    command(program, dir).args(args).output().unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let out = run("git", dir, args);
    assert!(out.status.success(), "git {args:?}: {}", text(&out));
}

fn codeflow(dir: &Path, args: &[&str]) -> String {
    text(
        &command(exe().to_str().unwrap(), dir)
            .args(args)
            .output()
            .unwrap(),
    )
}

fn write(dir: &Path, rel: &str, content: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

fn catalogued(row: &str) -> &'static Clearing {
    CATALOG
        .iter()
        .find(|clearing| clearing.name == row)
        .unwrap_or_else(|| panic!("{row} is not catalogued"))
}

/// The printed finding that contains `finding`: its line and the indented
/// lines under it.
fn block<'a>(out: &'a str, finding: &str) -> &'a str {
    let at = out
        .find(finding)
        .unwrap_or_else(|| panic!("no {finding:?} in:\n{out}"));
    let start = out[..at].rfind('\n').map_or(0, |i| i + 1);
    let next = |from: usize| out[from..].find('\n').map_or(out.len(), |i| from + i + 1);
    let mut end = next(at);
    while end < out.len() && out[end..].starts_with("  ") {
        end = next(end);
    }
    &out[start..end]
}

/// The row's remedy text is what the block prints: every fixed part of the
/// catalogued text, around its placeholders, is there.
fn assert_prints_row(block: &str, row: &str) {
    let clearing = catalogued(row);
    let flat = block.split_whitespace().collect::<Vec<_>>().join(" ");
    for part in clearing.text.split(['{', '}']).step_by(2) {
        let part = part.split_whitespace().collect::<Vec<_>>().join(" ");
        if part.len() > 3 {
            assert!(
                flat.contains(&part),
                "{row}: the printed remedy lacks {part:?}:\n{block}"
            );
        }
    }
}

/// The command the block prints for the row's step: the first backticked
/// span that starts with the step, or with `also`, an alternative the same
/// remedy prints.
fn printed_command(block: &str, row: &str, also: Option<&str>) -> String {
    let clearing = catalogued(row);
    let named = match (clearing.step, also) {
        (Step::Codeflow(command) | Step::Git(command), _) => command,
        // An edit's remedy may also print the command that confirms it.
        (Step::Edit(_), Some(alt)) => alt,
        (Step::Edit(_), None) | (Step::Manual(_), _) => panic!("{row} has no command to run"),
    };
    block
        .split('`')
        .skip(1)
        .step_by(2)
        .find(|span| span.starts_with(named) || also.is_some_and(|alt| span.starts_with(alt)))
        .unwrap_or_else(|| panic!("{row}: no `{named} ...` printed in:\n{block}"))
        .to_string()
}

/// Split a printed command into words, honouring double quotes.
fn words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut started = false;
    for c in command.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    out
}

/// Run a printed command in `dir`, with each `(placeholder, value)` filled
/// and `extra` words appended. The command must succeed.
fn run_printed(dir: &Path, printed: &str, fills: &[(&str, &str)], extra: &[&str]) -> String {
    let mut filled = printed.to_string();
    for (placeholder, value) in fills {
        filled = filled.replace(placeholder, value);
    }
    assert!(
        !filled.contains('<'),
        "an unfilled placeholder in `{filled}`"
    );
    let mut parts = words(&filled);
    parts.extend(extra.iter().map(|word| (*word).to_string()));
    let program = match parts[0].as_str() {
        "codeflow" => exe().to_string_lossy().into_owned(),
        other => other.to_string(),
    };
    let out = command(&program, dir).args(&parts[1..]).output().unwrap();
    let said = text(&out);
    assert!(out.status.success(), "`{filled}` failed:\n{said}");
    said
}

/// The proof: `check` prints `finding` with the row's remedy; `clear`
/// applies the printed step to that block; `check` then prints no
/// `finding`.
fn prove(row: &str, finding: &str, check: impl Fn() -> String, clear: impl FnOnce(&str)) {
    let before = check();
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, row);
    clear(&printed);
    let after = check();
    assert!(
        !after.contains(finding),
        "{row}: {finding:?} is still printed after its step:\n{after}"
    );
}

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

/// A plain repository on `feat/x` whose `main` holds a seed commit and the
/// given policy.
fn ci_repo(policy: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(root, ".codeflow/policy.json", policy);
    write(root, "seed.txt", "seed\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "chore: seed the fixture"]);
    git(root, &["switch", "-q", "-c", "feat/x"]);
    dir
}

fn commit(root: &Path, file: &str, message: &str) {
    write(root, file, &format!("{message}\n"));
    git(root, &["add", file]);
    git(root, &["commit", "-q", "-m", message]);
}

fn ci(root: &Path, extra: &[&str]) -> String {
    let mut args = vec![
        "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x",
    ];
    args.extend(extra);
    codeflow(root, &args)
}

/// A fresh `codeflow init --yes <tier>` project, made once per tier and
/// copied for each test.
fn scaffolded(tier: &str) -> tempfile::TempDir {
    static TEMPLATES: OnceLock<std::sync::Mutex<Vec<(String, tempfile::TempDir)>>> =
        OnceLock::new();
    let templates = TEMPLATES.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    let mut made = templates.lock().unwrap();
    if !made.iter().any(|(t, _)| t == tier) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("p");
        std::fs::create_dir(&root).unwrap();
        let out = codeflow(&root, &["init", "--yes", tier]);
        assert!(root.join(".codeflow/project.toml").exists(), "{out}");
        made.push((tier.to_string(), dir));
    }
    let template = &made.iter().find(|(t, _)| t == tier).unwrap().1;
    let copy = tempfile::tempdir().unwrap();
    let status = Command::new("cp")
        .args(["-R"])
        .arg(template.path().join("p"))
        .arg(copy.path().join("p"))
        .status()
        .unwrap();
    assert!(status.success());
    copy
}

fn project(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("p")
}

/// A work branch in a scaffolded project, with its protected branch seeded
/// into a bare destination by fetching (no client hook runs on it).
fn with_destination(root: &Path) -> PathBuf {
    let target = String::from_utf8(run("git", root, &["branch", "--show-current"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    let dest = root.parent().unwrap().join("dest.git");
    git(
        root.parent().unwrap(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            &target,
            dest.to_str().unwrap(),
        ],
    );
    git(
        &dest,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("{target}:{target}"),
        ],
    );
    git(root, &["remote", "add", "origin", dest.to_str().unwrap()]);
    git(root, &["fetch", "-q", "origin"]);
    git(
        root,
        &["switch", "-q", "-c", "feat/x", &format!("origin/{target}")],
    );
    dest
}

fn push(root: &Path, args: &[&str]) -> String {
    let mut all = vec!["push", "-q"];
    all.extend(args);
    text(&run("git", root, &all))
}

// ---------------------------------------------------------------------------
// Commit messages: `git commit --amend`, as printed.
// ---------------------------------------------------------------------------

fn prove_commit(row: &str, policy: &str, bad: &str, good: &str, finding: &str) {
    let dir = ci_repo(policy);
    let root = dir.path();
    commit(root, "x.txt", bad);
    prove(
        row,
        finding,
        || ci(root, &[]),
        |printed| {
            let step = printed_command(printed, row, None);
            assert_eq!(step, "git commit --amend", "{row}");
            run_printed(root, &step, &[], &["-q", "-m", good]);
        },
    );
}

const DEFAULTS: &str = "{}";

#[test]
fn clears_commit_type() {
    prove_commit(
        "COMMIT_TYPE",
        DEFAULTS,
        "Not conventional.",
        "feat: add x",
        "subject does not match",
    );
}

#[test]
fn clears_commit_length() {
    prove_commit(
        "COMMIT_LENGTH",
        DEFAULTS,
        "feat: add a thing whose description runs well past the fifty character budget",
        "feat: add x",
        "git.commit_format",
    );
}

#[test]
fn clears_commit_blank_line() {
    prove_commit(
        "COMMIT_BLANK_LINE",
        DEFAULTS,
        "feat: add x\n- one bullet",
        "feat: add x\n\n- one bullet",
        "git.commit_format",
    );
}

#[test]
fn clears_commit_breaking_footer() {
    prove_commit(
        "COMMIT_BREAKING_FOOTER",
        DEFAULTS,
        "feat: drop y\n\nBreaking change: y is gone",
        "feat: drop y\n\nBREAKING CHANGE: y is gone",
        "breaking-change footer",
    );
}

#[test]
fn clears_commit_body() {
    prove_commit(
        "COMMIT_BODY",
        DEFAULTS,
        "feat: add x\n\nThis paragraph explains the change in prose.",
        "feat: add x\n\n- explain the change",
        "git.commit_body",
    );
}

#[test]
fn clears_commit_required_footers() {
    prove_commit(
        "COMMIT_REQUIRED_FOOTERS",
        r#"{"git": {"commit_required_footers": ["Signed-off-by"], "commit_footer_tokens": ["Signed-off-by"]}}"#,
        "feat: add x",
        "feat: add x\n\nSigned-off-by: Proof <proof@example.test>",
        "git.commit_body",
    );
}

#[test]
fn clears_commit_ticket() {
    prove_commit(
        "COMMIT_TICKET",
        r#"{"git": {"commit_ticket_required": "warn", "commit_ticket_keys": ["Refs"], "commit_footer_tokens": ["Refs"]}}"#,
        "feat: add x",
        "feat: add x\n\nRefs: ABC-1",
        "git.commit_ticket",
    );
}

#[test]
fn clears_commit_ai_attribution() {
    prove_commit(
        "COMMIT_AI_ATTRIBUTION",
        DEFAULTS,
        "feat: add x\n\nCo-Authored-By: Claude <noreply@anthropic.com>",
        "feat: add x",
        "git.ai_attribution",
    );
}

#[test]
fn clears_commit_emoji() {
    prove_commit(
        "COMMIT_EMOJI",
        DEFAULTS,
        "feat: add x \u{1F680}",
        "feat: add x",
        "git.commit_emoji",
    );
}

#[test]
fn clears_commit_policy_character() {
    prove_commit(
        "COMMIT_POLICY_CHARACTER",
        DEFAULTS,
        "feat: add x \u{2014} and y",
        "feat: add x and y",
        "commit subject contains",
    );
}

#[test]
fn clears_branch_name() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    git(root, &["branch", "-m", "wrongname"]);
    commit(root, "x.txt", "feat: add x");
    let check = || {
        let branch =
            String::from_utf8(run("git", root, &["branch", "--show-current"]).stdout).unwrap();
        codeflow(
            root,
            &[
                "ci",
                "--base",
                "main",
                "--head",
                "HEAD",
                "--branch",
                branch.trim(),
            ],
        )
    };
    prove("BRANCH_NAME", "git.branch_naming", check, |printed| {
        let step = printed_command(printed, "BRANCH_NAME", None);
        run_printed(root, &step, &[("<prefix>/<kebab-name>", "feat/x")], &[]);
    });
}

#[test]
fn clears_file_policy_character() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    write(root, "docs/notes.md", "one \u{2014} two\n");
    git(root, &["add", "docs/notes.md"]);
    git(root, &["commit", "-q", "-m", "docs: add notes"]);
    prove(
        "FILE_POLICY_CHARACTER",
        "docs/notes.md:1",
        || ci(root, &[]),
        |printed| {
            assert!(printed.contains("edit docs/notes.md"), "{printed}");
            write(root, "docs/notes.md", "one, two\n");
            git(root, &["commit", "-q", "-am", "docs: reword the notes"]);
        },
    );
}

#[test]
fn clears_conflict_marker() {
    let dir = scaffolded("--minimal");
    let root = project(&dir);
    git(&root, &["switch", "-q", "-c", "feat/x"]);
    // Built at run time, so this file holds no marker line itself.
    let open = "<".repeat(7);
    let close = ">".repeat(7);
    write(
        &root,
        "notes.md",
        &format!("{open} HEAD\nours\n{close} feat/y\n"),
    );
    prove(
        "CONFLICT_MARKER",
        "notes.md:1 adds an unresolved opening conflict marker",
        || commit_all(&root, "docs: add the notes"),
        |printed| {
            assert!(printed.contains("edit notes.md"), "{printed}");
            write(&root, "notes.md", "ours\n");
        },
    );
}

// ---------------------------------------------------------------------------
// Pull request bodies: edit the body, then `codeflow ci --pr-body-file`.
// ---------------------------------------------------------------------------

const BODY: &str = "## Summary\n\nAdds a thing.\n\nTask: TSK-001\n\n## Changes\n\n- one change\n\n\
                    ## Testing\n\n- cargo test: 12 passed\n- Not tested: Windows.\n\n\
                    ## Reviews\n\nNone: pending review.\n\n## Release impact\n\n\
                    - Impact: patch\n- Breaking: no\n- Rationale: Preserve public behavior.\n\
                    - Migration: none\n";

/// Prove a body row: `bad` raises `finding`; writing `good` and running the
/// printed `codeflow ci --pr-body-file <body.md>` clears it.
fn prove_body(row: &str, policy: &str, bad: &str, good: &str, finding: &str) {
    let dir = ci_repo(policy);
    let root = dir.path();
    commit(root, "x.txt", "feat: add x");
    let body = root.parent().unwrap().join(format!("{row}.md"));
    std::fs::write(&body, bad).unwrap();
    let body_path = body.to_str().unwrap().to_string();
    let range = ["--base", "main", "--head", "HEAD", "--branch", "feat/x"];
    let check = || ci(root, &["--pr-body-file", &body_path]);
    let before = check();
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, row);
    std::fs::write(&body, good).unwrap();
    let step = printed_command(&printed, row, None);
    let after = {
        let mut filled = step.replace("<body.md>", &body_path);
        for word in range {
            filled.push(' ');
            filled.push_str(word);
        }
        // The printed rerun is the check itself: it may still exit non-zero
        // on other findings, so read its output rather than its status.
        let parts = words(&filled);
        text(
            &command(exe().to_str().unwrap(), root)
                .args(&parts[1..])
                .output()
                .unwrap(),
        )
    };
    assert!(
        !after.contains(finding),
        "{row}: {finding:?} is still printed after its step:\n{after}"
    );
}

#[test]
fn clears_pr_policy_character() {
    prove_body(
        "PR_POLICY_CHARACTER",
        DEFAULTS,
        &BODY.replace("Adds a thing.", "Adds a thing \u{2014} and more."),
        BODY,
        "PR body line",
    );
}

#[test]
fn clears_pr_ai_attribution() {
    prove_body(
        "PR_AI_ATTRIBUTION",
        DEFAULTS,
        &format!("{BODY}\nGenerated with Claude Code\n"),
        BODY,
        "git.ai_attribution",
    );
}

#[test]
fn clears_pr_emoji() {
    prove_body(
        "PR_EMOJI",
        DEFAULTS,
        &BODY.replace("Adds a thing.", "Adds a thing \u{1F680}"),
        BODY,
        "git.commit_emoji",
    );
}

#[test]
fn clears_pr_body_missing() {
    prove_body(
        "PR_BODY_MISSING",
        DEFAULTS,
        "",
        BODY,
        "PR body is missing or empty",
    );
}

#[test]
fn clears_pr_section_duplicate() {
    prove_body(
        "PR_SECTION_DUPLICATE",
        DEFAULTS,
        &format!("{BODY}\n## Summary\n\nAgain.\n"),
        BODY,
        "Summary",
    );
}

#[test]
fn clears_pr_section_empty() {
    prove_body(
        "PR_SECTION_EMPTY",
        DEFAULTS,
        &BODY.replace("- one change", "-"),
        BODY,
        "Changes",
    );
}

#[test]
fn clears_pr_section_missing() {
    prove_body(
        "PR_SECTION_MISSING",
        DEFAULTS,
        &BODY.replace("## Summary\n\nAdds a thing.\n\n", ""),
        BODY,
        "missing required section '## Summary'",
    );
}

#[test]
fn clears_pr_template_remnant() {
    prove_body(
        "PR_TEMPLATE_REMNANT",
        DEFAULTS,
        &BODY.replace(
            "- Impact: patch",
            "- Impact: `none | patch | minor | major`",
        ),
        BODY,
        "template",
    );
}

#[test]
fn clears_pr_presentation() {
    // TSK-184 (ADR-0071 rule 7): no line count judges a body; the
    // presentation rule keeps the structural findings, such as a Testing
    // section that names nothing as not tested.
    prove_body(
        "PR_PRESENTATION",
        DEFAULTS,
        &BODY.replace("- Not tested: Windows.\n", ""),
        BODY,
        "has no Not tested: line",
    );
}

#[test]
fn clears_pr_release_impact() {
    prove_body(
        "PR_RELEASE_IMPACT",
        DEFAULTS,
        &BODY.replace("- Migration: none\n", ""),
        BODY,
        "non-empty migration field",
    );
}

#[test]
fn clears_breaking_watch_path() {
    let policy = r#"{"git": {"breaking_watch_paths": ["x.txt"]}}"#;
    prove_body(
        "BREAKING_WATCH_PATH",
        policy,
        &BODY.replace("- Rationale: Preserve public behavior.\n", ""),
        BODY,
        "contract surface",
    );
}

#[test]
fn clears_ci_body_unsupplied() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    commit(root, "x.txt", "feat: add x");
    let body = root.parent().unwrap().join("body.md");
    std::fs::write(&body, BODY).unwrap();
    let finding = "pull request body was not supplied";
    let before = text(
        &command(exe().to_str().unwrap(), root)
            .args([
                "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x",
            ])
            .env("BITBUCKET_PR_ID", "7")
            .output()
            .unwrap(),
    );
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "CI_BODY_UNSUPPLIED");
    let step = printed_command(&printed, "CI_BODY_UNSUPPLIED", None);
    let filled = step.replace("<body.md>", body.to_str().unwrap());
    let parts = words(&filled);
    let after = text(
        &command(exe().to_str().unwrap(), root)
            .args(&parts[1..])
            .args(["--base", "main", "--head", "HEAD", "--branch", "feat/x"])
            .env("BITBUCKET_PR_ID", "7")
            .output()
            .unwrap(),
    );
    assert!(!after.contains(finding), "{after}");
}

// ---------------------------------------------------------------------------
// `codeflow ci` itself.
// ---------------------------------------------------------------------------

#[test]
fn clears_ci_base_unresolved() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    commit(root, "x.txt", "feat: add x");
    let finding = "could not resolve a base ref";
    let before = codeflow(root, &["ci", "--base", "nowhere", "--branch", "feat/x"]);
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "CI_BASE_UNRESOLVED");
    let step = printed_command(&printed, "CI_BASE_UNRESOLVED", None);
    let after = run_printed(
        root,
        &step,
        &[
            ("--base <ref>", "--base main"),
            ("--head <ref>", "--head HEAD"),
        ],
        &["--branch", "feat/x"],
    );
    assert!(!after.contains(finding), "{after}");
}

#[test]
fn clears_ci_base_refused() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    commit(root, "x.txt", "feat: add x");
    // A replace ref git cannot follow stops git on the base; git refuses to
    // write one, so the ref file is written directly.
    let base = text(&run("git", root, &["rev-parse", "main"]));
    let replace = format!(".git/refs/replace/{}", base.trim());
    write(root, &replace, "0123456789012345678901234567890123456789\n");
    let finding = "git refused it";
    prove(
        "CI_BASE_REFUSED",
        finding,
        || ci(root, &[]),
        |printed| {
            assert!(printed.contains("fatal: replacement"), "{printed}");
            std::fs::remove_file(root.join(&replace)).unwrap();
            let step = printed_command(printed, "CI_BASE_REFUSED", None);
            run_printed(
                root,
                &step,
                &[
                    ("--base <ref>", "--base main"),
                    ("--head <ref>", "--head HEAD"),
                ],
                &["--branch", "feat/x"],
            );
        },
    );
}

#[test]
fn clears_ci_branch_unresolved() {
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    commit(root, "x.txt", "feat: add x");
    git(root, &["switch", "-q", "--detach"]);
    let finding = "no branch name resolved";
    let before = codeflow(root, &["ci", "--base", "main", "--head", "HEAD"]);
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "CI_BRANCH_UNRESOLVED");
    let step = printed_command(&printed, "CI_BRANCH_UNRESOLVED", None);
    let after = run_printed(
        root,
        &step,
        &[("<name>", "feat/x")],
        &["--base", "main", "--head", "HEAD"],
    );
    assert!(!after.contains(finding), "{after}");
}

// ---------------------------------------------------------------------------
// Records and the documentation graph: `codeflow validate --docs`.
// ---------------------------------------------------------------------------

/// A standard project on `plan/x` holding EPC-001, its SPC-001 and its
/// task TSK-001, committed.
fn planned() -> tempfile::TempDir {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    git(&root, &["switch", "-q", "-c", "plan/x"]);
    for args in [
        vec!["epic", "new", "an outcome"],
        vec!["spec", "new", "--for", "EPC-001", "a contract"],
        vec!["task", "new", "--epic", "EPC-001", "a task"],
    ] {
        let out = codeflow(&root, &args);
        assert!(!out.contains("error"), "{args:?}: {out}");
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "docs: plan the work"]);
    dir
}

/// Replace the frontmatter line `field: ...` of a record with
/// `field: value`.
fn set_field(root: &Path, rel: &str, field: &str, value: &str) {
    let text = read(root, rel);
    let prefix = format!("{field}:");
    let mut found = false;
    let edited = text
        .lines()
        .map(|line| {
            if !found && line.starts_with(&prefix) {
                found = true;
                format!("{field}: {value}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(found, "{rel} has no {field}");
    write(root, rel, &format!("{edited}\n"));
}

fn validate(root: &Path) -> String {
    codeflow(root, &["validate", "--docs"])
}

const TASK: &str = "project-management/tasks/TSK-001.md";
const EPIC: &str = "project-management/epics/EPC-001.md";
const SPEC: &str = "project-management/specs/SPC-001.md";

#[test]
fn clears_task_status() {
    let dir = planned();
    let root = project(&dir);
    set_field(&root, TASK, "status", "in_progress");
    prove(
        "TASK_STATUS",
        "in_progress with no active branch carrying TSK-001",
        || validate(&root),
        |printed| {
            let step = printed_command(printed, "TASK_STATUS", None);
            run_printed(&root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_epic_status() {
    let dir = planned();
    let root = project(&dir);
    set_field(&root, EPIC, "status", "planning");
    set_field(&root, TASK, "status", "complete");
    prove(
        "EPIC_STATUS",
        "planning epic has complete tasks",
        || validate(&root),
        |printed| {
            assert!(printed.contains(&format!("in {EPIC}")), "{printed}");
            set_field(&root, EPIC, "status", "in_progress");
        },
    );
}

#[test]
fn clears_superseded_citation() {
    let dir = planned();
    let root = project(&dir);
    set_field(&root, TASK, "specs", "[SPC-001]");
    set_field(&root, TASK, "status", "complete");
    set_field(&root, SPEC, "status", "superseded");
    prove(
        "SUPERSEDED_CITATION",
        "acceptance cited superseded spec SPC-001",
        || validate(&root),
        |printed| {
            let step = printed_command(printed, "SUPERSEDED_CITATION", None);
            run_printed(&root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_spec_no_consumer() {
    let dir = planned();
    let root = project(&dir);
    let out = codeflow(&root, &["spec", "status", "SPC-001", "approved"]);
    assert!(out.contains("approved"), "{out}");
    set_field(&root, EPIC, "specs", "[]");
    prove(
        "SPEC_NO_CONSUMER",
        "no delivering consumer",
        || validate(&root),
        |printed| {
            let step = printed_command(printed, "SPEC_NO_CONSUMER", None);
            run_printed(
                &root,
                &step,
                &[],
                &["--standalone-reason", "delivers the contract", "deliver it"],
            );
            set_field(
                &root,
                "project-management/tasks/TSK-002.md",
                "specs",
                "[SPC-001]",
            );
        },
    );
}

#[test]
fn clears_spec_written_implemented() {
    let dir = planned();
    let root = project(&dir);
    set_field(&root, SPEC, "status", "implemented");
    prove(
        "SPEC_WRITTEN_IMPLEMENTED",
        "written `implemented` disagrees",
        || validate(&root),
        |printed| {
            assert!(printed.contains(&format!("in {SPEC}")), "{printed}");
            set_field(&root, SPEC, "status", "approved");
        },
    );
}

#[test]
fn clears_dual_identity() {
    let dir = planned();
    let root = project(&dir);
    set_field(&root, TASK, "id", "task-legacy-slug\nformat_id: TSK-001");
    prove(
        "DUAL_IDENTITY",
        "historical dual-identity record",
        || validate(&root),
        |printed| {
            assert!(printed.contains(TASK), "{printed}");
            // As printed: the `format_id` value becomes the `id`, and the
            // `format_id` line goes.
            set_field(&root, TASK, "id", "TSK-001");
            let text = read(&root, TASK)
                .lines()
                .filter(|line| !line.starts_with("format_id:"))
                .collect::<Vec<_>>()
                .join("\n");
            write(&root, TASK, &format!("{text}\n"));
        },
    );
}

const CAPABILITY: &str = "\n## CAP-001 thing\n\n```yaml\nid: CAP-001\nname: thing\narea: engine\n\
                          status: building\nverified_by: []\nepics: []\nadrs: []\n```\n";

#[test]
fn clears_docs_layer_absent() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    std::fs::remove_file(root.join("docs/capabilities.md")).unwrap();
    prove(
        "DOCS_LAYER_ABSENT",
        "docs/capabilities.md absent",
        || validate(&root),
        |printed| {
            let step = printed_command(printed, "DOCS_LAYER_ABSENT", None);
            run_printed(&root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_docs_epics_unchecked() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let registry = read(&root, "docs/capabilities.md");
    write(
        &root,
        "docs/capabilities.md",
        &format!(
            "{registry}{}",
            CAPABILITY.replace("epics: []", "epics: [EPC-001]")
        ),
    );
    prove(
        "DOCS_EPICS_UNCHECKED",
        "project-management/epics/ is absent",
        || validate(&root),
        |printed| {
            let step = printed_command(printed, "DOCS_EPICS_UNCHECKED", None);
            run_printed(&root, &step, &[], &["the outcome"]);
        },
    );
}

#[test]
fn clears_docs_specs_unchecked() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    git(&root, &["switch", "-q", "-c", "plan/x"]);
    codeflow(&root, &["epic", "new", "an outcome"]);
    set_field(&root, EPIC, "specs", "[SPC-001]");
    prove(
        "DOCS_SPECS_UNCHECKED",
        "project-management/specs/ is absent",
        || validate(&root),
        |printed| {
            let step = printed_command(printed, "DOCS_SPECS_UNCHECKED", None);
            run_printed(&root, &step, &[("<id>", "EPC-001")], &["a contract"]);
        },
    );
}

#[test]
fn clears_docs_adrs_unchecked() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    std::fs::remove_dir_all(root.join("docs/decisions")).unwrap();
    let registry = read(&root, "docs/capabilities.md");
    write(
        &root,
        "docs/capabilities.md",
        &format!(
            "{registry}{}",
            CAPABILITY.replace("adrs: []", "adrs: [ADR-0001]")
        ),
    );
    prove(
        "DOCS_ADRS_UNCHECKED",
        "docs/decisions/ is absent",
        || validate(&root),
        |printed| {
            let step = printed_command(printed, "DOCS_ADRS_UNCHECKED", None);
            run_printed(&root, &step, &[], &["a decision"]);
        },
    );
}

#[test]
fn clears_docs_capabilities_unchecked() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    git(&root, &["switch", "-q", "-c", "plan/x"]);
    codeflow(&root, &["epic", "new", "an outcome"]);
    set_field(&root, EPIC, "capabilities", "[CAP-001]");
    let registry = read(&root, "docs/capabilities.md");
    std::fs::remove_file(root.join("docs/capabilities.md")).unwrap();
    prove(
        "DOCS_CAPABILITIES_UNCHECKED",
        "docs/capabilities.md is absent",
        || validate(&root),
        |printed| {
            assert!(printed.contains("docs/capabilities.md"), "{printed}");
            write(
                &root,
                "docs/capabilities.md",
                &format!(
                    "{registry}{}",
                    CAPABILITY.replace("epics: []", "epics: [EPC-001]")
                ),
            );
        },
    );
}

#[test]
fn clears_workgraph_invalid() {
    let dir = planned();
    let root = project(&dir);
    set_field(&root, TASK, "depends_on", "[TSK-404]");
    git(
        &root,
        &["commit", "-q", "-am", "docs: depend on a missing task"],
    );
    let check = || {
        codeflow(
            &root,
            // A task branch is tracked work, whose graph ci checks.
            &[
                "ci",
                "--base",
                "HEAD~1",
                "--head",
                "HEAD",
                "--branch",
                "task/TSK-001-work",
            ],
        )
    };
    prove(
        "WORKGRAPH_INVALID",
        "visible durable workgraph is invalid",
        check,
        |printed| {
            set_field(&root, TASK, "depends_on", "[]");
            git(
                &root,
                &["commit", "-q", "-am", "docs: drop the missing dependency"],
            );
            let step = printed_command(printed, "WORKGRAPH_INVALID", None);
            run_printed(&root, &step, &[], &[]);
        },
    );
}

// ---------------------------------------------------------------------------
// Policy, secrets and the local target.
// ---------------------------------------------------------------------------

#[test]
fn clears_policy_deprecated() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let policy = read(&root, ".codeflow/policy.json");
    let mut value: serde_json::Value = serde_json::from_str(&policy).unwrap();
    value["human_authorization"] = serde_json::json!({});
    write(
        &root,
        ".codeflow/policy.json",
        &serde_json::to_string_pretty(&value).unwrap(),
    );
    prove(
        "POLICY_DEPRECATED",
        "human_authorization is deprecated",
        || {
            codeflow(
                &root,
                &[
                    "ci", "--base", "HEAD", "--head", "HEAD", "--branch", "feat/x",
                ],
            )
        },
        |printed| {
            let step = printed_command(printed, "POLICY_DEPRECATED", None);
            run_printed(&root, &step, &[], &[]);
        },
    );
}

fn commit_all(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    text(&run("git", root, &["commit", "-q", "-m", message]))
}

#[test]
fn clears_env_file_staged() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    git(&root, &["switch", "-q", "-c", "feat/x"]);
    write(&root, ".env", "MODE=local\n");
    write(&root, "app.txt", "app\n");
    // Staged past the scaffold's own ignore rule.
    git(&root, &["add", "-f", ".env", "app.txt"]);
    prove(
        "ENV_FILE_STAGED",
        "dotenv file staged for commit: .env",
        || {
            text(&run(
                "git",
                &root,
                &["commit", "-q", "-m", "feat: add the app"],
            ))
        },
        |printed| {
            let step = printed_command(printed, "ENV_FILE_STAGED", Some("git restore"));
            run_printed(&root, &step, &[("<file>", ".env")], &[]);
            let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
            write(&root, ".gitignore", &format!("{ignore}.env\n"));
        },
    );
}

#[test]
fn clears_secret_staged() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    git(&root, &["switch", "-q", "-c", "feat/x"]);
    // Built at run time, so this file carries no key shape itself.
    let key = format!("AKIA{}", "IOSFODNN7EXAMPLF");
    write(&root, "config.txt", &format!("key = {key}\n"));
    prove(
        "SECRET_STAGED",
        "possible secret",
        || commit_all(&root, "feat: add the config"),
        |printed| {
            let step = printed_command(printed, "SECRET_STAGED", None);
            run_printed(&root, &step, &[("<file>", "config.txt")], &[]);
            write(&root, "config.txt", "key = read from the environment\n");
        },
    );
}

/// A local target strictly behind its upstream (sathyassn/codeflow#27):
/// `work start` anchors on the upstream and names it, and prints no step.
/// The stale branch is checked out in the root checkout, and the guards
/// refuse each way an agent in a task worktree could fast-forward it.
#[test]
fn a_target_behind_its_upstream_prints_no_step() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    // The destination's target moves on; the local target stays behind.
    let base = String::from_utf8(run("git", &dest, &["symbolic-ref", "--short", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    git(
        &root,
        &["switch", "-q", "-c", "plan/work", &format!("origin/{base}")],
    );
    let out = codeflow(
        &root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "a fixture",
            "the work",
        ],
    );
    assert!(out.contains("TSK-001"), "{out}");
    commit_all(&root, "docs: plan the work");
    git(
        &dest,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("plan/work:{base}"),
        ],
    );
    git(&root, &["fetch", "-q", "origin"]);
    git(
        &root,
        &[
            "branch",
            "-q",
            "--set-upstream-to",
            &format!("origin/{base}"),
            &base,
        ],
    );
    git(&root, &["switch", "-q", &base]);
    let task = root.parent().unwrap().join("task");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "task/TSK-001-work",
            task.to_str().unwrap(),
            &format!("origin/{base}"),
        ],
    );
    let out = codeflow(&task, &["work", "start", "TSK-001"]);
    assert!(
        out.contains(&format!("-> refs/remotes/origin/{base}")),
        "{out}"
    );
    assert!(
        !out.contains("behind its upstream") && !out.contains("clear it"),
        "{out}"
    );
}

// ---------------------------------------------------------------------------
// The push set: a real push through the scaffolded pre-push hook.
// ---------------------------------------------------------------------------

const QUICK_CONFIG: &str = r#"{"schema_version": "1.0", "targets": [{"name": "check", "enabled": true, "runner": "custom", "modes": {"quick": {"command": "true"}}}]}"#;

fn pushed(root: &Path) -> String {
    push(root, &["origin", "HEAD"])
}

#[test]
fn clears_push_targets_unconfigured() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    with_destination(&root);
    write(&root, "x.txt", "x\n");
    commit_all(&root, "feat: add x");
    prove(
        "PUSH_TARGETS_UNCONFIGURED",
        "no .codeflow/test-config.json",
        || pushed(&root),
        |printed| {
            assert!(
                printed.contains("create .codeflow/test-config.json"),
                "{printed}"
            );
            write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
            commit_all(&root, "chore: add a quick target");
        },
    );
}

#[test]
fn clears_push_targets_none() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    with_destination(&root);
    write(
        &root,
        ".codeflow/test-config.json",
        &QUICK_CONFIG.replace("\"quick\"", "\"full\""),
    );
    commit_all(&root, "chore: add a full target");
    prove(
        "PUSH_TARGETS_NONE",
        "quick targets skipped",
        || pushed(&root),
        |printed| {
            assert!(printed.contains(".codeflow/test-config.json"), "{printed}");
            write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
            commit_all(&root, "chore: give the target a quick mode");
        },
    );
}

#[test]
fn clears_push_set_failed() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    with_destination(&root);
    write(
        &root,
        ".codeflow/test-config.json",
        &QUICK_CONFIG.replace("\"true\"", "\"false\""),
    );
    commit_all(&root, "chore: add a failing target");
    prove(
        "PUSH_SET_FAILED",
        "push set failed for: check",
        || pushed(&root),
        |printed| {
            write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
            commit_all(&root, "chore: fix the target");
            let step = printed_command(printed, "PUSH_SET_FAILED", None);
            run_printed(&root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_test_config_repair() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    with_destination(&root);
    write(&root, ".codeflow/test-config.json", "{ not json");
    commit_all(&root, "chore: add a broken test config");
    prove(
        "TEST_CONFIG_REPAIR",
        "could not load test-config.json",
        || pushed(&root),
        |printed| {
            assert!(
                printed.contains("repair .codeflow/test-config.json"),
                "{printed}"
            );
            write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
            commit_all(&root, "chore: repair the test config");
            let step = printed_command(printed, "TEST_CONFIG_REPAIR", Some("codeflow test"));
            run_printed(&root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_push_set_ci() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    commit_all(&root, "chore: add a quick target");
    write(&root, "x.txt", "x\n");
    git(&root, &["add", "x.txt"]);
    // The commit-msg hook would refuse this subject; the push set is what
    // this row is about, so the message is written without it.
    let out = run(
        "git",
        &root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-q",
            "-m",
            "Not conventional.",
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    prove(
        "PUSH_SET_CI",
        "push set check failed: `codeflow ci",
        || pushed(&root),
        |printed| {
            assert!(printed.contains("then push again"), "{printed}");
            git(&root, &["commit", "-q", "--amend", "-m", "feat: add x"]);
        },
    );
}

#[test]
fn clears_push_set_validate() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    let registry = read(&root, "docs/capabilities.md");
    write(
        &root,
        "docs/capabilities.md",
        &format!(
            "{registry}{}",
            CAPABILITY.replace("adrs: []", "adrs: [ADR-0777]")
        ),
    );
    commit_all(&root, "docs: add a capability");
    prove(
        "PUSH_SET_VALIDATE",
        "push set check failed: `codeflow validate",
        || pushed(&root),
        |printed| {
            write(
                &root,
                "docs/capabilities.md",
                &format!("{registry}{CAPABILITY}"),
            );
            commit_all(&root, "docs: drop the missing decision");
            let step = printed_command(printed, "PUSH_SET_VALIDATE", None);
            run_printed(&root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_push_tree_unchecked() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    commit_all(&root, "chore: add a quick target");
    git(&root, &["branch", "feat/y"]);
    write(&root, "x.txt", "x\n");
    commit_all(&root, "feat: add x");
    // Pushing feat/y from a checkout of feat/x.
    let check = || push(&root, &["origin", "feat/y"]);
    let before = check();
    let printed = block(&before, "did not run for").to_string();
    assert_prints_row(&printed, "PUSH_TREE_UNCHECKED");
    assert!(
        printed.contains("push from a clean checkout of the pushed commit"),
        "{printed}"
    );
    git(&root, &["switch", "-q", "feat/y"]);
    write(&root, "y.txt", "y\n");
    commit_all(&root, "feat: add y");
    let after = check();
    assert!(!after.contains("did not run for"), "{after}");
}

#[test]
fn clears_push_rewrite() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    commit_all(&root, "chore: add a quick target");
    write(&root, "b.txt", "b\n");
    commit_all(&root, "feat: add b");
    pushed(&root);
    git(&root, &["reset", "-q", "--hard", "HEAD~1"]);
    write(&root, "c.txt", "c\n");
    commit_all(&root, "feat: add c");
    let finding = "rewrites the destination's";
    let before = push(&root, &["--force-with-lease", "origin", "HEAD"]);
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "PUSH_REWRITE");
    // The rewrite pushed; take the step against a fresh rewrite of it.
    git(&root, &["reset", "-q", "--hard", "HEAD~1"]);
    write(&root, "d.txt", "d\n");
    commit_all(&root, "feat: add d");
    let step = printed_command(&printed, "PUSH_REWRITE", None);
    run_printed(&root, &step, &[], &["-q", "origin", "feat/x"]);
    let after = pushed(&root);
    assert!(!after.contains(finding), "{after}");
}

#[test]
fn clears_push_range_unresolved() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    commit_all(&root, "chore: add a quick target");
    // The push fetches the default tip (SPC-013 R-120), so only a branch
    // sharing no history with it can lack a base: one from a line with its
    // own history, which moves on in another clone while this clone
    // forgets its tracking refs.
    git(&root, &["checkout", "-q", "--orphan", "chore/archive"]);
    git(&root, &["commit", "-q", "-m", "chore: start the archive"]);
    let started = push(&root, &["origin", "chore/archive"]);
    assert!(
        text(&run(
            "git",
            &root,
            &["ls-remote", "origin", "chore/archive"]
        ))
        .contains("chore/archive"),
        "{started}"
    );
    let other = root.parent().unwrap().join("other");
    git(
        root.parent().unwrap(),
        &[
            "clone",
            "-q",
            "-b",
            "chore/archive",
            dest.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    write(&other, "o.txt", "o\n");
    git(&other, &["add", "o.txt"]);
    git(&other, &["commit", "-q", "-m", "feat: add o"]);
    git(&other, &["push", "-q", "origin", "HEAD"]);
    git(&root, &["switch", "-q", "-c", "feat/archived"]);
    commit(&root, "x.txt", "feat: add x");
    let refs = String::from_utf8(
        run(
            "git",
            &root,
            &["for-each-ref", "--format=%(refname)", "refs/remotes/origin"],
        )
        .stdout,
    )
    .unwrap();
    for name in refs.lines() {
        git(&root, &["update-ref", "-d", name]);
    }
    let finding = "range unresolved";
    let before = pushed(&root);
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "PUSH_RANGE_UNRESOLVED");
    let step = printed_command(&printed, "PUSH_RANGE_UNRESOLVED", None);
    run_printed(&root, &step, &[("<remote>", "origin")], &[]);
    let after = pushed(&root);
    assert!(!after.contains(finding), "{after}");
}

// ---------------------------------------------------------------------------
// Doctor.
// ---------------------------------------------------------------------------

fn doctor(root: &Path, check: &str) -> String {
    codeflow(root, &["doctor", "--check", check])
}

#[test]
fn clears_doctor_hooks_path() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let absolute = root.join(".codeflow/git-hooks");
    git(
        &root,
        &["config", "core.hooksPath", absolute.to_str().unwrap()],
    );
    prove(
        "DOCTOR_HOOKS_PATH",
        "core.hooksPath is absolute",
        || doctor(&root, "hooks"),
        |printed| {
            let step = printed_command(printed, "DOCTOR_HOOKS_PATH", None);
            run_printed(&root, &step, &[], &[]);
        },
    );
}

/// TSK-215 (issue 29): a `CodeFlow` hook command with a `$`, as 3.0.0
/// shipped it, in a file grok reads is named; `codeflow update` rewrites the
/// `CodeFlow` hook entries, and the guard canary then runs the real binary and
/// sees it refuse.
#[test]
fn clears_doctor_grok_hooks() {
    let dir = scaffolded("--minimal");
    let root = project(&dir);
    let mut settings: serde_json::Value =
        serde_json::from_str(&read(&root, ".claude/settings.json")).unwrap();
    settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"] =
        "codeflow hook git-guard --contract 3; codeflow_status=$?; if [ \"$codeflow_status\" -ne 0 ]; then exit 2; fi".into();
    write(
        &root,
        ".claude/settings.json",
        &serde_json::to_string_pretty(&settings).unwrap(),
    );
    prove(
        "DOCTOR_GROK_HOOKS",
        "grok skips the CodeFlow hook commands in .claude/settings.json",
        || doctor(&root, "grok"),
        |printed| {
            let step = printed_command(printed, "DOCTOR_GROK_HOOKS", None);
            run_printed(&root, &step, &[], &[]);
        },
    );
    let after = doctor(&root, "grok");
    assert!(
        after.contains("canary: the shipped shell guard grok runs refused a dangerous command"),
        "{after}"
    );
}

/// The grok hook file as 3.0.0 shipped it: each wrapper tail carried
/// `$` variables grok reads as unset templates.
fn grok_hooks_3_0_0() -> String {
    let current = include_str!("../../../../assets/base/grok/hooks.json");
    let start = current.find(" || { ").unwrap();
    let end = current[start..].find("exit 2; }").unwrap() + start + "exit 2; }".len();
    current.replace(
        &current[start..end],
        "; codeflow_status=$?; if [ \\\"$codeflow_status\\\" -ne 0 ]; then exit 2; fi",
    )
}

/// PR 35 review finding 5: an adopter who edited the 3.0.0 grok hook file
/// keeps the edit. `codeflow update` leaves the file as it is and writes a
/// `.new` 3-way merge; doctor still names the file and the `.new` file, and
/// resolving the `.new` file as the remedy says clears the finding with the
/// adopter's change kept.
#[test]
fn clears_doctor_grok_hooks_in_an_edited_hook_file() {
    let dir = scaffolded("--minimal");
    let root = project(&dir);
    let old = grok_hooks_3_0_0();
    assert!(old.contains("codeflow_status=$?"), "{old}");
    // The 3.0.0 install: the shipped file is the baseline; the adopter
    // raised the exec-guard timeout.
    write(&root, ".codeflow/.baseline/.grok/hooks/codeflow.json", &old);
    let exec_line = old
        .lines()
        .find(|line| line.contains("hook exec-guard"))
        .unwrap();
    let edited = old.replace(
        exec_line,
        &exec_line.replace("\"timeout\": 10", "\"timeout\": 15"),
    );
    assert_ne!(edited, old);
    write(&root, ".grok/hooks/codeflow.json", &edited);

    let finding = "grok skips the CodeFlow hook commands in .grok/hooks/codeflow.json";
    prove(
        "DOCTOR_GROK_HOOKS",
        finding,
        || doctor(&root, "grok"),
        |printed| {
            let step = printed_command(printed, "DOCTOR_GROK_HOOKS", None);
            assert_eq!(step, "codeflow update");
            // It exits nonzero on the conflict it reports.
            let said = codeflow(&root, &["update"]);
            assert!(
                said.contains("codeflow.json.new holds the 3-way merge"),
                "{said}"
            );
            // Never overwritten: the edit stands and the merge waits.
            assert_eq!(read(&root, ".grok/hooks/codeflow.json"), edited);
            let merge = read(&root, ".grok/hooks/codeflow.json.new");
            assert!(merge.contains("<<<<<<<"), "{merge}");
            let still = doctor(&root, "grok");
            assert!(still.contains(finding), "{still}");
            assert!(
                still.contains("wrote .grok/hooks/codeflow.json.new, which waits to be resolved"),
                "{still}"
            );
            // Resolve as the remedy says: the shipped commands, the
            // adopter's timeout; then the `.new` file goes.
            let shipped = include_str!("../../../../assets/base/grok/hooks.json");
            let line = shipped
                .lines()
                .find(|line| line.contains("hook exec-guard"))
                .unwrap();
            let resolved =
                shipped.replace(line, &line.replace("\"timeout\": 10", "\"timeout\": 15"));
            write(&root, ".grok/hooks/codeflow.json", &resolved);
            std::fs::remove_file(root.join(".grok/hooks/codeflow.json.new")).unwrap();
        },
    );
    assert!(read(&root, ".grok/hooks/codeflow.json").contains("\"timeout\": 15"));
    let after = doctor(&root, "grok");
    assert!(
        after.contains("canary: the shipped shell guard grok runs refused a dangerous command"),
        "{after}"
    );
}

/// PR 35 review rounds 3 and 4, finding 2 and 3: doctor offers `codeflow
/// update` only where update's own steps rewrite the file or propose a
/// merge beside it. An ownership update rejects, a fabricated record for a
/// file update does not ship, a file `[scaffold] ignore` opts out, a hook
/// file update skips as a symlink, and an edit update keeps all get the
/// hand edit instead, with the reason update leaves the file.
#[test]
fn the_grok_update_remedy_follows_what_update_repairs() {
    let stale = grok_hooks_3_0_0();
    let mut failures = Vec::new();
    let mut hand_edit = |case: &str, root: &Path, path: &str, why: &str| {
        let said = doctor(root, "grok");
        if !said.contains(&format!("`codeflow update` does not rewrite {path}"))
            || said.contains("run `codeflow update`")
            || !said.contains(why)
        {
            failures.push(format!("{case}: {said}"));
        }
    };
    let manifest = |root: &Path| -> serde_json::Value {
        serde_json::from_str(&read(root, ".codeflow/manifest.json")).unwrap()
    };

    // An ownership update rejects.
    let dir = scaffolded("--minimal");
    let root = project(&dir);
    write(&root, ".grok/hooks/codeflow.json", &stale);
    let mut record = manifest(&root);
    record["files"][".grok/hooks/codeflow.json"]["ownership"] = "managed-nonsense".into();
    write(&root, ".codeflow/manifest.json", &record.to_string());
    hand_edit("rejected ownership", &root, ".grok/hooks/codeflow.json", "");

    // A managed record for a file update does not ship.
    let dir = scaffolded("--minimal");
    let root = project(&dir);
    write(&root, ".grok/hooks/custom.json", &stale);
    let mut record = manifest(&root);
    record["files"][".grok/hooks/custom.json"] =
        record["files"][".grok/hooks/codeflow.json"].clone();
    write(&root, ".codeflow/manifest.json", &record.to_string());
    hand_edit("fabricated record", &root, ".grok/hooks/custom.json", "");

    // A shipped file the project opted out of.
    let dir = scaffolded("--minimal");
    let root = project(&dir);
    write(&root, ".grok/hooks/codeflow.json", &stale);
    let project_toml = read(&root, ".codeflow/project.toml");
    write(
        &root,
        ".codeflow/project.toml",
        &format!("{project_toml}\n[scaffold]\nignore = [\".grok/hooks/codeflow.json\"]\n"),
    );
    hand_edit(
        "ignored",
        &root,
        ".grok/hooks/codeflow.json",
        "`codeflow update` skips .grok/hooks/codeflow.json: ignored via [scaffold] ignore",
    );

    // PR 35 review round 4, finding 3: update skips a hook file that is a
    // symlink, or sits beneath one, and writes nothing for it.
    #[cfg(unix)]
    for link in [".grok/hooks/codeflow.json", ".grok/hooks"] {
        let dir = scaffolded("--minimal");
        let root = project(&dir);
        std::fs::remove_dir_all(root.join(".grok/hooks")).unwrap();
        write(&root, "shared/hooks/codeflow.json", &stale);
        if link == ".grok/hooks" {
            std::os::unix::fs::symlink(root.join("shared/hooks"), root.join(link)).unwrap();
        } else {
            std::fs::create_dir_all(root.join(".grok/hooks")).unwrap();
            std::os::unix::fs::symlink(root.join("shared/hooks/codeflow.json"), root.join(link))
                .unwrap();
        }
        codeflow(&root, &["update"]);
        assert_eq!(read(&root, "shared/hooks/codeflow.json"), stale, "{link}");
        assert!(
            !root.join("shared/hooks/codeflow.json.new").exists(),
            "{link}"
        );
        hand_edit(
            &format!("symlink at {link}"),
            &root,
            ".grok/hooks/codeflow.json",
            &format!("`codeflow update` skips .grok/hooks/codeflow.json: {link} is a symlink"),
        );
    }

    // PR 35 review round 4, finding 3: the adopter restores the old
    // wrapper after an install of the current version. The shipped file
    // equals the baseline, so update keeps the edit and writes no `.new`:
    // offering update again would loop.
    let dir = scaffolded("--minimal");
    let root = project(&dir);
    write(&root, ".grok/hooks/codeflow.json", &stale);
    codeflow(&root, &["update"]);
    assert_eq!(read(&root, ".grok/hooks/codeflow.json"), stale);
    assert!(!root.join(".grok/hooks/codeflow.json.new").exists());
    hand_edit(
        "kept modification",
        &root,
        ".grok/hooks/codeflow.json",
        "`codeflow update` leaves your edit to .grok/hooks/codeflow.json as it is and writes no `.new`, since the shipped version has not changed since it was last installed, and .codeflow/.baseline/.grok/hooks/codeflow.json holds that shipped version",
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // The edit the finding names clears it: the baseline is the shipped
    // version.
    let shipped = read(&root, ".codeflow/.baseline/.grok/hooks/codeflow.json");
    write(&root, ".grok/hooks/codeflow.json", &shipped);
    let after = doctor(&root, "grok");
    assert!(
        after.contains("canary: the shipped shell guard grok runs refused"),
        "{after}"
    );
}

/// PR 35 review finding 5: a stale `CodeFlow` hook in
/// `.claude/settings.local.json`, which grok reads, survives
/// `codeflow update`, which does not manage the file; doctor names it with
/// the edit step, and the edit clears it.
#[test]
fn clears_doctor_grok_unmanaged_hooks() {
    let dir = scaffolded("--minimal");
    let root = project(&dir);
    let stale = serde_json::json!({"hooks": {"PreToolUse": [{
        "matcher": "Bash",
        "hooks": [{"type": "command", "command": "codeflow hook git-guard --contract 3; codeflow_status=$?; if [ \"$codeflow_status\" -ne 0 ]; then exit 2; fi"}]
    }]}});
    let local = serde_json::to_string_pretty(&stale).unwrap();
    write(&root, ".claude/settings.local.json", &local);
    codeflow(&root, &["update"]);
    assert_eq!(read(&root, ".claude/settings.local.json"), local);
    prove(
        "DOCTOR_GROK_UNMANAGED_HOOKS",
        "grok skips the CodeFlow hook commands in .claude/settings.local.json",
        || doctor(&root, "grok"),
        |printed| {
            assert!(!printed.contains("run `codeflow update`"), "{printed}");
            let settings: serde_json::Value =
                serde_json::from_str(&read(&root, ".claude/settings.json")).unwrap();
            let shipped = settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"].clone();
            assert!(
                shipped
                    .as_str()
                    .unwrap()
                    .starts_with("codeflow hook git-guard"),
                "{shipped}"
            );
            let mut fixed = stale.clone();
            fixed["hooks"]["PreToolUse"][0]["hooks"][0]["command"] = shipped;
            write(
                &root,
                ".claude/settings.local.json",
                &serde_json::to_string_pretty(&fixed).unwrap(),
            );
        },
    );
}

/// The hook file with every `PreToolUse` exec-guard handler removed.
fn without_exec_guard(text: &str) -> String {
    let mut value: serde_json::Value = serde_json::from_str(text).unwrap();
    for group in value["hooks"]["PreToolUse"].as_array_mut().unwrap() {
        group["hooks"].as_array_mut().unwrap().retain(|h| {
            !h["command"]
                .as_str()
                .unwrap_or("")
                .contains("hook exec-guard")
        });
    }
    serde_json::to_string_pretty(&value).unwrap()
}

/// PR 35 review round 5: no exec-guard is bound in any file grok reads,
/// and `codeflow update` cannot bring it back, since the project opted both
/// files that ship it out with `[scaffold] ignore`. Doctor gives the hand
/// edit that restores the binding, quoting the shipped guard group, never
/// `codeflow update`; adding the quoted group clears it. Where update does
/// restore it, through the Claude settings it still manages, doctor gives
/// `codeflow update` and running it clears the finding.
#[test]
fn clears_doctor_grok_missing_guard() {
    let dir = scaffolded("--minimal");
    let root = project(&dir);
    let project_toml = read(&root, ".codeflow/project.toml");
    write(
        &root,
        ".codeflow/project.toml",
        &format!("{project_toml}\n[scaffold]\nignore = [\".grok/hooks/codeflow.json\", \".claude/settings.json\"]\n"),
    );
    for path in [".grok/hooks/codeflow.json", ".claude/settings.json"] {
        let text = without_exec_guard(&read(&root, path));
        write(&root, path, &text);
    }
    codeflow(&root, &["update"]);
    let finding = "no CodeFlow exec-guard is bound to PreToolUse";
    let before = doctor(&root, "grok");
    assert!(before.contains(finding), "{before}");
    assert!(!before.contains("run `codeflow update`"), "{before}");
    assert!(
        before.contains("`codeflow update` skips .grok/hooks/codeflow.json: ignored"),
        "{before}"
    );
    prove(
        "DOCTOR_GROK_MISSING_GUARD",
        finding,
        || doctor(&root, "grok"),
        |printed| {
            let flat = printed.split_whitespace().collect::<Vec<_>>().join(" ");
            let start = flat.find("the shipped guard group is `").unwrap()
                + "the shipped guard group is `".len();
            let end = flat[start..].find('`').unwrap() + start;
            let group: serde_json::Value = serde_json::from_str(&flat[start..end]).unwrap();
            let path = ".grok/hooks/codeflow.json";
            let mut hooks: serde_json::Value = serde_json::from_str(&read(&root, path)).unwrap();
            hooks["hooks"]["PreToolUse"]
                .as_array_mut()
                .unwrap()
                .push(group);
            write(&root, path, &serde_json::to_string_pretty(&hooks).unwrap());
        },
    );
    let after = doctor(&root, "grok");
    assert!(
        after.contains("canary: the shipped shell guard grok runs refused a dangerous command"),
        "{after}"
    );

    let dir = scaffolded("--minimal");
    let root = project(&dir);
    for path in [".grok/hooks/codeflow.json", ".claude/settings.json"] {
        let text = without_exec_guard(&read(&root, path));
        write(&root, path, &text);
    }
    let before = doctor(&root, "grok");
    assert!(before.contains(finding), "{before}");
    assert!(
        before.contains(
            "run `codeflow update` so the CodeFlow hook commands in .claude/settings.json"
        ),
        "{before}"
    );
    codeflow(&root, &["update"]);
    let after = doctor(&root, "grok");
    assert!(
        after.contains("canary: the shipped shell guard grok runs refused a dangerous command"),
        "{after}"
    );
}

/// PR 35 review round 5: `.grok/hooks/codeflow.json`, which update
/// rewrites, and `.claude/settings.local.json`, which it does not manage,
/// both hold old wrappers. Doctor names both files and gives both steps:
/// `codeflow update` for the first and the hand edit for the second;
/// following both clears the finding.
#[test]
fn clears_doctor_grok_hooks_with_an_unmanaged_file_too() {
    let dir = scaffolded("--minimal");
    let root = project(&dir);
    let old = grok_hooks_3_0_0();
    write(&root, ".codeflow/.baseline/.grok/hooks/codeflow.json", &old);
    write(&root, ".grok/hooks/codeflow.json", &old);
    let stale = serde_json::json!({"hooks": {"PreToolUse": [{
        "matcher": "Bash",
        "hooks": [{"type": "command", "command": "codeflow hook git-guard --contract 3; codeflow_status=$?; if [ \"$codeflow_status\" -ne 0 ]; then exit 2; fi"}]
    }]}});
    write(
        &root,
        ".claude/settings.local.json",
        &serde_json::to_string_pretty(&stale).unwrap(),
    );
    let finding = "grok skips the CodeFlow hook commands in .grok/hooks/codeflow.json, .claude/settings.local.json";
    let before = doctor(&root, "grok");
    assert!(before.contains(finding), "{before}");
    assert!(
        before.contains(
            "run `codeflow update` so the CodeFlow hook commands in .grok/hooks/codeflow.json match"
        ),
        "{before}"
    );
    assert!(
        before.contains("`codeflow update` does not rewrite .claude/settings.local.json"),
        "{before}"
    );
    prove(
        "DOCTOR_GROK_HOOKS",
        finding,
        || doctor(&root, "grok"),
        |printed| {
            assert_prints_row(printed, "DOCTOR_GROK_UNMANAGED_HOOKS");
            let step = printed_command(printed, "DOCTOR_GROK_HOOKS", None);
            run_printed(&root, &step, &[], &[]);
            let settings: serde_json::Value =
                serde_json::from_str(&read(&root, ".claude/settings.json")).unwrap();
            let mut fixed = stale.clone();
            fixed["hooks"]["PreToolUse"][0]["hooks"][0]["command"] =
                settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"].clone();
            write(
                &root,
                ".claude/settings.local.json",
                &serde_json::to_string_pretty(&fixed).unwrap(),
            );
        },
    );
    let after = doctor(&root, "grok");
    assert!(
        after.contains("canary: the shipped shell guard grok runs refused"),
        "{after}"
    );
}

#[test]
fn clears_doctor_git_dir_hooks() {
    // A hook git ran from its own folder until `init` wired the shims.
    let dir = scaffolded("--standard");
    let root = project(&dir);
    write(&root, ".git/hooks/pre-commit", "#!/bin/sh\ntrue\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.join(".git/hooks/pre-commit"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    prove(
        "DOCTOR_GIT_DIR_HOOKS",
        "git does not run the hooks in",
        || doctor(&root, "hooks"),
        |printed| {
            assert!(printed.contains("git-hooks: pre-commit"), "{printed}");
            // The adopter moved the check to CI; the file leaves the folder.
            std::fs::remove_file(root.join(".git/hooks/pre-commit")).unwrap();
        },
    );
}

#[test]
fn clears_doctor_init() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    prove(
        "DOCTOR_INIT",
        ".codeflow/ directory not found",
        || doctor(&root, "config"),
        |printed| {
            let step = printed_command(printed, "DOCTOR_INIT", None);
            run_printed(&root, &step, &[], &["--yes", "--minimal"]);
        },
    );
}

#[test]
fn clears_doctor_policy_decision() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let policy = read(&root, ".codeflow/policy.json");
    write(&root, ".codeflow/policy.json", "{ not json");
    prove(
        "DOCTOR_POLICY_DECISION",
        "git.pr_sections effective level",
        || {
            let out = doctor(&root, "adopter-fit");
            if out.starts_with("warn") {
                out
            } else {
                String::new()
            }
        },
        |printed| {
            assert!(printed.contains(".codeflow/policy.json"), "{printed}");
            write(&root, ".codeflow/policy.json", &policy);
            let step = printed_command(printed, "DOCTOR_POLICY_DECISION", Some("codeflow doctor"));
            run_printed(&root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_doctor_release_backend() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    write(&root, "release-please-config.json", "{}\n");
    prove(
        "DOCTOR_RELEASE_BACKEND",
        "release-please owns versions here",
        || doctor(&root, "adopter-fit"),
        |printed| {
            assert!(printed.contains(".codeflow/project.toml"), "{printed}");
            let state = read(&root, ".codeflow/project.toml");
            let edited = if state.contains("[release]") {
                state.replace("backend = \"none\"", "backend = \"external\"")
            } else {
                format!("{state}\n[release]\nbackend = \"external\"\n")
            };
            write(&root, ".codeflow/project.toml", &edited);
        },
    );
}

#[test]
fn clears_doctor_ci_placeholder() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let path = ".github/workflows/codeflow-ci.yml";
    let wired = read(&root, path);
    // The install step as an older release shipped it.
    write(
        &root,
        path,
        &format!("{wired}\n# echo \"::error::codeflow install step is an unwired PLACEHOLDER\"\n"),
    );
    prove(
        "DOCTOR_CI_PLACEHOLDER",
        "install step is still the PLACEHOLDER",
        || doctor(&root, "ci-perimeter"),
        |printed| {
            assert!(printed.contains(&format!("in {path}")), "{printed}");
            // The release installer, as `codeflow init` writes it now.
            write(&root, path, &wired);
        },
    );
}

const STATE: &str = ".codeflow/project.toml";

/// The `scaffold_version = "..."` line of a project state.
fn pin_line(state: &str) -> &str {
    state
        .lines()
        .find(|line| line.starts_with("scaffold_version"))
        .unwrap_or_else(|| panic!("no scaffold_version in:\n{state}"))
}

/// The state with its pin set to `version`.
fn with_pin(state: &str, version: &str) -> String {
    state.replace(
        pin_line(state),
        &format!("scaffold_version = \"{version}\""),
    )
}

/// The branch checked out at `root`.
fn current_branch(root: &Path) -> String {
    String::from_utf8(run("git", root, &["branch", "--show-current"]).stdout)
        .unwrap()
        .trim()
        .to_string()
}

/// Land `branch` on the destination's `target`, as the host merges a pull
/// request, and fetch it back (no client hook runs on the destination).
fn land(root: &Path, dest: &Path, branch: &str, target: &str) {
    git(
        dest,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("{branch}:{target}"),
        ],
    );
    git(root, &["fetch", "-q", "origin"]);
}

fn commit_checked(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

#[test]
fn clears_doctor_ci_pin_missing() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let target = current_branch(&root);
    git(&root, &["switch", "-q", "-c", "feat/x"]);
    let state = read(&root, STATE);
    let pin = pin_line(&state).to_string();
    write(&root, STATE, &state.replace(&format!("{pin}\n"), ""));
    prove(
        "DOCTOR_CI_PIN_MISSING",
        "no scaffold_version is pinned",
        || doctor(&root, "ci-perimeter"),
        |printed| {
            assert!(printed.contains(STATE), "{printed}");
            // The version CI installs: the one the target pins.
            let edited = format!("{}{pin}\n", read(&root, STATE));
            write(&root, STATE, &edited);
            let after = doctor(&root, "ci-perimeter");
            assert!(
                after.contains(&format!("the version {target} pins")),
                "{after}"
            );
        },
    );
}

#[test]
fn clears_doctor_ci_pin_target() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let target = current_branch(&root);
    let dest = with_destination(&root);
    let state = read(&root, STATE);
    let pin = pin_line(&state).to_string();
    // A target that pins nothing, as a project scaffolded before the pin.
    git(&root, &["switch", "-q", "-c", "chore/unpin"]);
    write(&root, STATE, &state.replace(&format!("{pin}\n"), ""));
    commit_checked(&root, "chore: drop the codeflow pin");
    land(&root, &dest, "chore/unpin", &target);
    git(&root, &["switch", "-q", "feat/x"]);
    assert_eq!(read(&root, STATE), state);
    prove(
        "DOCTOR_CI_PIN_TARGET",
        "pins no scaffold_version",
        || doctor(&root, "ci-perimeter"),
        |printed| {
            let on = format!("origin/{target}");
            assert!(printed.contains(&format!("on {on} first")), "{printed}");
            // A pull request into the target that changes only the pin line.
            git(&root, &["switch", "-q", "-c", "chore/pin", &on]);
            let unpinned = read(&root, STATE);
            write(&root, STATE, &format!("{unpinned}{pin}\n"));
            commit_checked(&root, "chore: pin the codeflow version");
            let changed = text(&run("git", &root, &["diff", "--numstat", &on, "HEAD"]));
            assert_eq!(changed.trim(), format!("1\t0\t{STATE}"));
            land(&root, &dest, "chore/pin", &target);
            git(&root, &["switch", "-q", "feat/x"]);
        },
    );
}

#[test]
fn clears_doctor_ci_pin_lowered() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let target = current_branch(&root);
    git(&root, &["switch", "-q", "-c", "feat/x"]);
    let state = read(&root, STATE);
    let pinned = pin_line(&state).split('"').nth(1).unwrap().to_string();
    write(&root, STATE, &with_pin(&state, "0.1.0"));
    prove(
        "DOCTOR_CI_PIN_LOWERED",
        "lowers scaffold_version",
        || doctor(&root, "ci-perimeter"),
        |printed| {
            let named = format!("back to {pinned}, the version {target} pins");
            assert!(
                printed
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .contains(&named),
                "{printed}"
            );
            let edited = with_pin(&read(&root, STATE), &pinned);
            write(&root, STATE, &edited);
        },
    );
}

#[test]
fn clears_doctor_ci_pin_order() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let target = current_branch(&root);
    let dest = with_destination(&root);
    let state = read(&root, STATE);
    let policy_path = ".codeflow/policy.json";
    let policy = read(&root, policy_path);
    // What `codeflow update` to a newer release carries: a policy key the
    // target's policy lacks.
    let mut updated: serde_json::Value = serde_json::from_str(&policy).unwrap();
    updated["git"]["future_key"] = serde_json::json!("warn");
    let updated = serde_json::to_string_pretty(&updated).unwrap();
    let raised = with_pin(&state, "999.0.0");
    write(&root, STATE, &raised);
    write(&root, policy_path, &updated);
    prove(
        "DOCTOR_CI_PIN_ORDER",
        "also carries",
        || doctor(&root, "ci-perimeter"),
        |printed| {
            assert!(printed.contains(STATE), "{printed}");
            // First: raise only scaffold_version, and land it.
            write(&root, policy_path, &policy);
            let step_one = doctor(&root, "ci-perimeter");
            assert!(step_one.contains("upgrade step one"), "{step_one}");
            commit_checked(&root, "chore: raise the codeflow pin");
            land(&root, &dest, "feat/x", &target);
            // Then the update, on a new branch from the landed raise.
            git(
                &root,
                &[
                    "switch",
                    "-q",
                    "-c",
                    "chore/update",
                    &format!("origin/{target}"),
                ],
            );
            assert_eq!(read(&root, STATE), raised);
            write(&root, policy_path, &updated);
        },
    );
}

#[test]
fn clears_doctor_tracking_unknown() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let state = read(&root, ".codeflow/project.toml");
    write(&root, ".codeflow/project.toml", "tier = \"standard\"\n");
    prove(
        "DOCTOR_TRACKING_UNKNOWN",
        "durable-work tracking cannot be determined",
        || doctor(&root, "id-registry"),
        |printed| {
            assert!(
                printed.contains("repair .codeflow/project.toml"),
                "{printed}"
            );
            write(&root, ".codeflow/project.toml", &state);
        },
    );
}

#[test]
fn clears_doctor_managed_drift() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let agents = read(&root, "AGENTS.md");
    let edited = agents.replacen("## Always rules", "## Always rules\n\nA hand edit.", 1);
    assert_ne!(agents, edited);
    write(&root, "AGENTS.md", &edited);
    prove(
        "DOCTOR_MANAGED_DRIFT",
        "hand-edited inside codeflow markers",
        || doctor(&root, "managed-drift"),
        |printed| {
            let step = printed_command(printed, "DOCTOR_MANAGED_DRIFT", None);
            run_printed(&root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_doctor_customization() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    prove(
        "DOCTOR_CUSTOMIZATION",
        "consuming-project context still needs reconciliation",
        || doctor(&root, "customization"),
        |printed| {
            for path in ["docs/product.md", "docs/architecture.md", "AGENTS.md"] {
                if !printed.contains(path) {
                    continue;
                }
                let mut text = read(&root, path).replace(
                    "<!-- Add project-specific notes here. -->",
                    "This project ships a thing.",
                );
                while let Some(start) = text.find("{{") {
                    let end = text[start..]
                        .find("}}")
                        .map_or(text.len(), |i| start + i + 2);
                    text.replace_range(start..end, "This project ships a thing.");
                }
                write(&root, path, &text);
            }
        },
    );
}

#[test]
fn clears_doctor_instructions() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let agents = read(&root, "AGENTS.md");
    let long = "Project detail that belongs in a file the section points at.\n".repeat(700);
    write(&root, "AGENTS.md", &format!("{agents}\n{long}"));
    prove(
        "DOCTOR_INSTRUCTIONS",
        "instruction limit",
        || {
            let out = doctor(&root, "instructions");
            if out.starts_with("warn") {
                out
            } else {
                String::new()
            }
        },
        |printed| {
            assert!(printed.contains("AGENTS.md"), "{printed}");
            write(&root, "docs/project-detail.md", &long);
            write(
                &root,
                "AGENTS.md",
                &format!("{agents}\nSee docs/project-detail.md.\n"),
            );
        },
    );
}

#[test]
fn clears_doctor_reading() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let skill = ".claude/skills/cf-stack/SKILL.md";
    let original = read(&root, skill);
    write(&root, skill, &original.repeat(3));
    // The finding is this skill's own: another skill over its guideline
    // (TSK-108 owns one on this line) does not hide the clearing.
    prove(
        "DOCTOR_READING",
        ".claude/skills/cf-stack/",
        || doctor(&root, "reading"),
        |_| write(&root, skill, &original),
    );
}

#[test]
fn clears_doctor_root_checkout() {
    // The root checkout left on a feature branch (TSK-165): the finding
    // names `git switch main`; after it, the check passes.
    let dir = ci_repo(DEFAULTS);
    let root = dir.path();
    prove(
        "DOCTOR_ROOT_CHECKOUT",
        "the root checkout needs attention",
        || doctor(root, "repo-integrity"),
        |printed| {
            assert!(
                printed.contains("is on 'feat/x'") && printed.contains("Next: run git switch main"),
                "{printed}"
            );
            git(root, &["switch", "-q", "main"]);
            let step = printed_command(printed, "DOCTOR_ROOT_CHECKOUT", None);
            run_printed(root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_doctor_test_config() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    write(
        &root,
        ".codeflow/test-config.json",
        &QUICK_CONFIG.replace(
            "\"runner\": \"custom\",",
            "\"runner\": \"custom\", \"structural\": {\"source_glob\": [\"src/**/*.rs\"]},",
        ),
    );
    prove(
        "DOCTOR_TEST_CONFIG",
        "test-config warning(s)",
        || doctor(&root, "test-config"),
        |printed| {
            assert!(printed.contains(".codeflow/test-config.json"), "{printed}");
            write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
            let step = printed_command(printed, "DOCTOR_TEST_CONFIG", Some("codeflow doctor"));
            run_printed(&root, &step, &[], &[]);
        },
    );
}

#[test]
fn clears_doctor_tool_missing() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    // PATH holds only this directory, so doctor finds exactly what is here.
    let bin = tempfile::tempdir().unwrap();
    let check = || {
        text(
            &command(exe().to_str().unwrap(), &root)
                .env("PATH", bin.path())
                .args(["doctor", "--check", "claude"])
                .output()
                .unwrap(),
        )
    };
    prove(
        "DOCTOR_TOOL_MISSING",
        "claude CLI not found",
        check,
        |printed| {
            assert!(
                printed.contains("install the claude CLI on PATH"),
                "{printed}"
            );
            // Doctor looks the CLI up on PATH; a stand-in is what it can see.
            // On Windows the npm install is a `claude.cmd`, found through
            // PATHEXT.
            #[cfg(windows)]
            let claude = bin.path().join("claude.cmd");
            #[cfg(not(windows))]
            let claude = bin.path().join("claude");
            std::fs::write(&claude, "#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        },
    );
}

// Cross-vendor delegation runs peers in tmux, which native Windows lacks;
// `codeflow delegate init` refuses there and points at WSL2.
#[cfg(unix)]
#[test]
fn clears_doctor_delegates() {
    // TSK-147 round 3 F5: the gaps this machine closes are proven here;
    // only the sign-in, the operator's own account, is excluded.
    let dir = scaffolded("--standard");
    let root = project(&dir);
    // PATH holds only this directory, so doctor finds exactly what is here:
    // a signed-in codex, and a claude whose Codex plugin is not enabled.
    let bin = tempfile::tempdir().unwrap();
    let stand_in = |name: &str, script: &str| {
        let path = bin.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    };
    let plugin = |enabled: bool| {
        format!(
            "if [ \"$1\" = plugin ]; then echo '[{{\"id\":\"codex@openai-codex\",\"enabled\":{enabled}}}]'; fi\nexit 0\n"
        )
    };
    stand_in("codex", "exit 0\n");
    stand_in("claude", &plugin(false));
    let check = || {
        text(
            &command(exe().to_str().unwrap(), &root)
                .env("PATH", bin.path())
                .args(["doctor", "--check", "delegates"])
                .output()
                .unwrap(),
        )
    };
    prove(
        "DOCTOR_DELEGATES",
        "cross-vendor delegation is partially unavailable",
        check,
        |printed| {
            assert!(printed.contains("tmux missing from PATH"), "{printed}");
            assert!(printed.contains("plugin not enabled"), "{printed}");
            // Install tmux and enable the plugin: stand-ins are what doctor
            // can see of both.
            stand_in("tmux", "exit 0\n");
            stand_in("claude", &plugin(true));
        },
    );
}

/// TSK-147 round 4 F6: manager hooks that name every shim are a note,
/// never a pass, because reading cannot show that a hook runs its shim. A
/// note is not cleared by reading, so this proof shows the printed step is
/// the confirmation: the same real git event is refused by a hook that
/// runs its shim and accepted by each inactive form doctor cannot tell
/// apart from it (the reviewer's probes).
#[cfg(unix)]
#[test]
fn confirms_doctor_hook_wiring_unseen() {
    // (label, the hook each shim gets, whether it runs the shim)
    type Form<'a> = (&'a str, &'a dyn Fn(&str) -> String, bool);
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    write(&root, ".husky/pre-commit", "#!/bin/sh\ntrue\n");
    git(&root, &["config", "core.hooksPath", ".husky"]);
    codeflow(&root, &["init", "--yes", "--standard"]);
    let shims: Vec<String> = std::fs::read_dir(root.join(".codeflow/git-hooks"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    git(&root, &["switch", "-q", "-c", "feat/x"]);
    let call = |name: &str| format!(".codeflow/git-hooks/{name} \"$@\"");
    let forms: [Form; 8] = [
        (
            "after exit",
            &|n| format!("#!/bin/sh\nexit 0\n{}\n", call(n)),
            false,
        ),
        (
            "false branch",
            &|n| format!("#!/bin/sh\nif false; then\n  {}\nfi\nexit 0\n", call(n)),
            false,
        ),
        (
            "unused function",
            &|n| format!("#!/bin/sh\nunused() {{\n  {}\n}}\nexit 0\n", call(n)),
            false,
        ),
        (
            "echo only",
            &|n| format!("#!/bin/sh\nprintf '%s\\n' '.codeflow/git-hooks/{n}'\nexit 0\n"),
            false,
        ),
        (
            "colon data",
            &|n| format!("#!/bin/sh\n: '.codeflow/git-hooks/{n}'\nexit 0\n"),
            false,
        ),
        (
            "here document",
            &|n| {
                format!(
                    "#!/bin/sh\ncat >/dev/null <<'EOF'\n{}\nEOF\nexit 0\n",
                    call(n)
                )
            },
            false,
        ),
        (
            "masked failure",
            &|n| format!("#!/bin/sh\n{} || true\n", call(n)),
            false,
        ),
        ("live call", &|n| format!("#!/bin/sh\n{}\n", call(n)), true),
    ];
    for (label, hook, runs) in forms {
        for name in &shims {
            let path = root.join(".husky").join(name);
            std::fs::write(&path, hook(name)).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let out = doctor(&root, "hooks");
        let printed = block(&out, "wiring not verified").to_string();
        assert_prints_row(&printed, "DOCTOR_HOOK_WIRING_UNSEEN");
        assert!(
            out.lines()
                .any(|line| line.starts_with("note") && line.contains("hooks")),
            "{label}: a note, never a pass:\n{out}"
        );
        let step = printed_command(&printed, "DOCTOR_HOOK_WIRING_UNSEEN", None);
        let parts = words(&step);
        let event = command(&parts[0], &root)
            .args(&parts[1..])
            .output()
            .unwrap();
        if runs {
            assert!(!event.status.success(), "{label}: {}", text(&event));
            assert!(
                text(&event).contains("git.commit_format"),
                "{label}: {}",
                text(&event)
            );
        } else {
            assert!(event.status.success(), "{label}: {}", text(&event));
            git(&root, &["reset", "-q", "--soft", "HEAD~1"]);
        }
    }
}

#[test]
fn clears_doctor_hook_manager() {
    // A project whose git hooks another manager owned before `codeflow init`,
    // which records them as unwired and leaves them alone.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    write(&root, ".husky/pre-commit", "#!/bin/sh\ntrue\n");
    git(&root, &["config", "core.hooksPath", ".husky"]);
    let out = codeflow(&root, &["init", "--yes", "--standard"]);
    assert!(
        read(&root, ".codeflow/project.toml").contains("git_hooks = \"unwired\""),
        "{out}"
    );
    let shims: Vec<String> = std::fs::read_dir(root.join(".codeflow/git-hooks"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    git(&root, &["switch", "-q", "-c", "feat/x"]);
    // A real git event: a commit with a bad subject. While the manager's
    // hooks do not call the shims, git lets it through.
    let bad_commit = || {
        command("git", &root)
            .args(["commit", "-q", "--allow-empty", "-m", "Bad subject."])
            .output()
            .unwrap()
    };
    assert!(bad_commit().status.success(), "no shim runs yet");
    git(&root, &["reset", "-q", "--soft", "HEAD~1"]);
    let exec = |path: &Path| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = path;
    };
    prove(
        "DOCTOR_HOOK_MANAGER",
        "another hook manager owns",
        || doctor(&root, "hooks"),
        |printed| {
            assert!(printed.contains("in .husky executable"), "{printed}");
            // Each manager hook calls its codeflow shim on a live line. The
            // call alone is not enough: TSK-147 round 3 F6 found a hook git
            // skips (not executable) passing on its text, so doctor must
            // still warn until the file is executable.
            for name in &shims {
                assert!(
                    printed.contains(name.as_str()),
                    "{name} not named:\n{printed}"
                );
                write(
                    &root,
                    &format!(".husky/{name}"),
                    &format!("#!/bin/sh\ntrue\n.codeflow/git-hooks/{name} \"$@\"\n"),
                );
            }
            #[cfg(unix)]
            {
                let still = doctor(&root, "hooks");
                assert!(still.contains("not executable"), "{still}");
                assert!(
                    bad_commit().status.success(),
                    "git skips a hook that is not executable"
                );
                git(&root, &["reset", "-q", "--soft", "HEAD~1"]);
            }
            for name in &shims {
                exec(&root.join(".husky").join(name));
            }
        },
    );
    // The same real git event is now refused by the codeflow shim.
    let refused = bad_commit();
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(text(&refused).contains("commit"), "{}", text(&refused));
}
