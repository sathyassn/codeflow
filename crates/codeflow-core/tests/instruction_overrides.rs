//! TSK-1302: a project overrides managed instructions in
//! `.codeflow/overrides.toml` and keeps the overrides through updates.
//!
//! The scratch projects are scaffolded from the real asset tree. The second
//! scaffold version is an overlay over it that changes one always rule, which
//! is not overridden, in the kernel and the `AGENTS.md` template alike, and
//! one `cf-present` section, which is not overridden either.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use codeflow_core::doctor::{self, Options, Status};
use codeflow_core::scaffold::overrides::record::{self, Journal, Record};
use codeflow_core::scaffold::update::{self as update_mod, Decision};
use codeflow_core::scaffold::{
    self, Action, AssetSource, DirSource, InitAnswers, InitOptions, Report, Tier, UpdateOptions,
};

#[path = "support/git_isolation.rs"]
mod git_isolation;
use git_isolation::isolate_git;

const AGENTS: &str = "AGENTS.md";
const PRESENT: [&str; 2] = [
    ".claude/skills/cf-present/SKILL.md",
    ".agents/skills/cf-present/SKILL.md",
];

const OUTCOME_SHIPPED: &str = "Name the result, who uses it and the evidence that would establish it; a gate or criterion is evidence toward the result, never the result. Separate what is done here from what still depends on other work.";
const OUTCOME_OURS: &str = "Name the result and who uses it; our release notes are the evidence.";
const COMPLETION_SHIPPED: &str =
    "Return the handoff line, purpose, requested decision, durable outcomes";
const COMPLETION_OURS: &str = "Post the handoff line in the team channel and link the page.\n";

const EVIDENCE_V1: &str = "say what was not verified.";
const EVIDENCE_V2: &str = "say plainly what was not verified.";
const PRESENT_V1: &str = "In a CodeFlow project this is the default surface";
const PRESENT_V2: &str = "In a CodeFlow project this is the usual surface";

const OVERRIDES: &str = r#"schema_version = 1

[[override]]
target = "rule:outcome"
text = "Name the result and who uses it; our release notes are the evidence."

[[override]]
target = "cf-present#completion"
body = """
Post the handoff line in the team channel and link the page.
"""
"#;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn v1() -> DirSource {
    DirSource::new(repo_root().join("assets"))
}

/// The second scaffold version: the current tree with the evidence rule and
/// the first `cf-present` section reworded.
struct V2(DirSource);

fn v2() -> V2 {
    V2(v1())
}

impl AssetSource for V2 {
    fn read(&self, path: &str) -> std::io::Result<Option<Vec<u8>>> {
        let Some(bytes) = self.0.read(path)? else {
            return Ok(None);
        };
        let (from, to) = match path {
            "base/rule-map.toml" | "base/AGENTS.md.tmpl" | "base/AGENTS.full.md.tmpl" => {
                (EVIDENCE_V1, EVIDENCE_V2)
            }
            "base/agents/skills/cf-present/SKILL.md" => (PRESENT_V1, PRESENT_V2),
            _ => return Ok(Some(bytes)),
        };
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains(from), "{path} no longer holds {from:?}");
        Ok(Some(text.replacen(from, to, 1).into_bytes()))
    }
}

fn scaffold() -> tempfile::TempDir {
    isolate_git();
    let dir = tempfile::tempdir().expect("project tempdir");
    git_isolation::own_repository(dir.path());
    let options = InitOptions {
        tier: Some(Tier::Standard),
        force: false,
        binary_version: "3.0.0".to_string(),
        answers: InitAnswers {
            product_one_liner: Some("A billing service for small clinics.".to_string()),
            areas: Some(vec!["api".to_string()]),
            permission_preset: None,
        },
    };
    scaffold::init(&v1(), dir.path(), &options).expect("init succeeds");
    dir
}

fn run(source: &dyn AssetSource, root: &Path, version: &str) -> Result<Report, String> {
    scaffold::update(
        source,
        root,
        &UpdateOptions {
            force: false,
            binary_version: version.to_string(),
            diff_out: None,
        },
    )
    .map_err(|error| error.to_string())
}

fn update_v1(root: &Path) -> Report {
    run(&v1(), root, "3.0.0").unwrap_or_else(|error| panic!("update fails: {error}"))
}

fn update_v2(root: &Path) -> Report {
    run(&v2(), root, "3.0.1").unwrap_or_else(|error| panic!("update fails: {error}"))
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn action_of(report: &Report, dest: &str) -> Action {
    report
        .files
        .iter()
        .find(|file| file.dest.as_str() == dest)
        .unwrap_or_else(|| panic!("{dest} not reported:\n{report:?}"))
        .action
}

/// Every file of the project but its git folder.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = path.strip_prefix(root).unwrap().to_path_buf();
            if rel == Path::new(".git") {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// Copy every file of the project but its git folder.
fn copy_project(from: &Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git_isolation::own_repository(dir.path());
    for (rel, bytes) in snapshot(from) {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    dir
}

fn assert_overridden(root: &Path) {
    let agents = read(root, AGENTS);
    assert!(agents.contains(OUTCOME_OURS), "{agents}");
    assert!(
        !agents.contains(OUTCOME_SHIPPED),
        "the shipped outcome text stayed"
    );
    assert!(agents.contains("- **Work to the outcome.** Name the result and who uses it"));
    for dest in PRESENT {
        let skill = read(root, dest);
        assert!(
            skill.contains(&format!(
                "<!-- codeflow:section id=completion class=overridable -->\n## Completion\n{COMPLETION_OURS}"
            )),
            "{dest}:\n{skill}"
        );
        assert!(!skill.contains(COMPLETION_SHIPPED), "{dest}");
    }
}

fn doctor_overrides(
    root: &Path,
    plan: fn(&Path) -> Result<scaffold::overrides::Plan, scaffold::ScaffoldError>,
) -> doctor::CheckResult {
    let opts = Options {
        project_dir: root.to_string_lossy().into_owned(),
        override_plan: Some(plan),
        ..Options::default()
    };
    doctor::run_check("overrides", &opts).unwrap()
}

fn plan_v1(root: &Path) -> Result<scaffold::overrides::Plan, scaffold::ScaffoldError> {
    update_mod::override_plan(&v1(), root, "3.0.0")
}

fn plan_v2(root: &Path) -> Result<scaffold::overrides::Plan, scaffold::ScaffoldError> {
    update_mod::override_plan(&v2(), root, "3.0.1")
}

// ---------------------------------------------------------------------------
// AC-2: the overrides stay through updates across scaffold versions.
// ---------------------------------------------------------------------------

#[test]
fn update_keeps_a_row_and_a_section_override_across_two_scaffold_versions() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);

    let pending = doctor_overrides(root, plan_v1);
    assert!(pending.status.is_warn(), "{pending:?}");
    assert!(
        pending.message.contains("not yet applied"),
        "{}",
        pending.message
    );

    let report = update_v1(root);
    assert_eq!(action_of(&report, AGENTS), Action::Changed);
    assert_overridden(root);
    assert!(Record::read(root, AGENTS).unwrap().is_some());
    assert!(Record::read(root, PRESENT[0]).unwrap().is_some());

    let report = update_v2(root);
    assert_overridden(root);
    let agents = read(root, AGENTS);
    assert!(agents.contains(EVIDENCE_V2), "the v2 rule change landed");
    for dest in PRESENT {
        assert_eq!(action_of(&report, dest), Action::Changed, "{dest}");
        assert!(
            read(root, dest).contains(PRESENT_V2),
            "{dest}: the v2 section change landed"
        );
    }

    // A repeated update changes nothing.
    let before = snapshot(root);
    let report = update_v2(root);
    for dest in PRESENT {
        assert_eq!(action_of(&report, dest), Action::Unchanged, "{dest}");
    }
    assert_eq!(action_of(&report, AGENTS), Action::Unchanged);
    let after = snapshot(root);
    let changed: Vec<_> = before
        .keys()
        .filter(|path| before.get(*path) != after.get(*path))
        .filter(|path| {
            !path.starts_with(".codeflow/reports") && !path.starts_with(".codeflow/ledger")
        })
        .collect();
    assert!(
        changed.iter().all(
            |path| path.as_path() == Path::new(".codeflow/manifest.json")
                || path.as_path() == Path::new(".codeflow/project.toml")
        ),
        "{changed:?}"
    );

    // Doctor lists both overrides with the shipped text each replaces.
    let listed = doctor_overrides(root, plan_v2);
    assert_eq!(listed.status, Status::Pass, "{listed:?}");
    for expected in [
        "rule:outcome in AGENTS.md replaces \"- **Work to the outcome.** Name the result, who uses it",
        "cf-present#completion in .agents/skills/cf-present/SKILL.md replaces the section body",
        "cf-present#completion in .claude/skills/cf-present/SKILL.md replaces the section body",
        "starting \"Return the handoff line",
        "all applied",
    ] {
        assert!(listed.message.contains(expected), "{expected:?} in {}", listed.message);
    }
    assert!(
        !listed.message.contains("team channel"),
        "doctor never prints the project's text"
    );
}

#[test]
fn removing_an_override_restores_the_shipped_text_and_its_record() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);
    update_v1(root);
    assert_overridden(root);
    write(root, ".codeflow/overrides.toml", "schema_version = 1\n");
    update_v1(root);
    assert!(read(root, AGENTS).contains(OUTCOME_SHIPPED));
    for dest in PRESENT {
        assert!(read(root, dest).contains(COMPLETION_SHIPPED), "{dest}");
        assert!(Record::read(root, dest).unwrap().is_none(), "{dest}");
        assert_eq!(
            read(root, dest),
            read(
                &repo_root(),
                "assets/base/agents/skills/cf-present/SKILL.md"
            )
        );
    }
    assert!(Record::read(root, AGENTS).unwrap().is_none());
    let none = doctor_overrides(root, plan_v1);
    assert_eq!(none.status, Status::Pass);
    assert_eq!(none.message, "no project overrides");
}

/// The two-update hand-edit case: a hand edit outside the overridden units
/// is merged into the next version and kept by the update after it, and the
/// effective baseline never takes it in.
#[test]
fn a_hand_edit_survives_two_updates_and_stays_out_of_the_effective_baseline() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);
    update_v1(root);
    let dest = PRESENT[0];
    let hand = "Keep raw feedback and exports untracked unless the user explicitly promotes";
    let edited = read(root, dest).replacen(
        hand,
        "Keep raw feedback, exports and our notes untracked unless the user explicitly promotes",
        1,
    );
    write(root, dest, &edited);

    let report = update_v1(root);
    assert_eq!(action_of(&report, dest), Action::KeptUserModified);
    assert_eq!(read(root, dest), edited);

    let report = update_v2(root);
    assert_eq!(action_of(&report, dest), Action::Merged);
    let merged = read(root, dest);
    assert!(merged.contains("our notes"), "the hand edit stays");
    assert!(merged.contains(PRESENT_V2), "the v2 change lands");
    assert!(merged.contains(COMPLETION_OURS), "the override stays");
    let record = Record::read(root, dest).unwrap().unwrap();
    let pristine = String::from_utf8(
        v2().read("base/agents/skills/cf-present/SKILL.md")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let effective = scaffold::overrides::apply_units(&pristine, &record.applied).unwrap();
    assert!(!effective.contains("our notes"));
    assert_eq!(
        record.effective_sha256,
        scaffold::sha256_hex(effective.as_bytes())
    );
    assert_eq!(
        record.written_sha256,
        scaffold::sha256_hex(merged.as_bytes())
    );

    let report = update_v2(root);
    assert_eq!(action_of(&report, dest), Action::KeptUserModified);
    assert_eq!(
        read(root, dest),
        merged,
        "the second update keeps the merge"
    );
}

/// A hand edit that overlaps the next version's change conflicts; once the
/// proposal is resolved, the next update merges against the new effective
/// baseline and keeps the override.
#[test]
fn a_conflict_with_an_override_resolves_onto_the_new_effective_baseline() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);
    update_v1(root);
    let dest = PRESENT[0];
    let edited = read(root, dest).replacen(
        PRESENT_V1,
        "In a CodeFlow project this is our chosen surface",
        1,
    );
    write(root, dest, &edited);
    let report = update_v2(root);
    assert_eq!(action_of(&report, dest), Action::Conflicted);
    assert_eq!(read(root, dest), edited, "the file is untouched");
    let proposal = read(root, &format!("{dest}.new"));
    assert!(
        proposal.contains(COMPLETION_OURS),
        "the proposal keeps the override"
    );
    // Resolve in favour of the project's line.
    let resolved: String = {
        let mut out = String::new();
        let mut skip = false;
        for line in proposal.split_inclusive('\n') {
            if line.starts_with("<<<<<<<") {
                continue;
            }
            if line.starts_with("|||||||") || line.starts_with("=======") {
                skip = true;
                continue;
            }
            if line.starts_with(">>>>>>>") {
                skip = false;
                continue;
            }
            if !skip {
                out.push_str(line);
            }
        }
        out
    };
    write(root, dest, &resolved);
    std::fs::remove_file(root.join(format!("{dest}.new"))).unwrap();
    let report = update_v2(root);
    assert_eq!(
        action_of(&report, dest),
        Action::KeptUserModified,
        "{report:?}"
    );
    let now = read(root, dest);
    assert!(now.contains("our chosen surface"));
    assert!(now.contains(COMPLETION_OURS));
}

#[test]
fn doctor_and_the_update_preview_read_the_effective_text() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);
    update_v1(root);
    let decision = update_mod::decide(&v1(), root, PRESENT[0], "3.0.0").unwrap();
    assert_eq!(decision, Some(Decision::Current));
    let drift = managed_drift(root);
    assert_eq!(drift.status, Status::Pass, "{drift:?}");
    assert_eq!(doctor_overrides(root, plan_v1).status, Status::Pass);
}

// ---------------------------------------------------------------------------
// AC-3: every refusal writes nothing.
// ---------------------------------------------------------------------------

/// Update refuses `overrides` with a message holding `expected`, and the
/// project is byte-identical afterwards; doctor fails with the same text.
fn assert_refused(overrides: &str, expected: &str) {
    let dir = scaffold();
    let root = dir.path();
    update_v1(root);
    write(root, ".codeflow/overrides.toml", overrides);
    let before = snapshot(root);
    let error = run(&v1(), root, "3.0.0").expect_err("update refuses");
    assert!(error.contains(expected), "{expected:?} in {error}");
    assert!(error.contains("update changed nothing"), "{error}");
    assert!(error.contains(".codeflow/overrides.toml"), "{error}");
    assert_eq!(snapshot(root), before, "a refusal writes nothing");
    let doctor = doctor_overrides(root, plan_v1);
    assert_eq!(doctor.status, Status::Fail, "{doctor:?}");
    assert!(doctor.message.contains(expected), "{}", doctor.message);
}

/// As `assert_refused`, and the refusal names the one override by number.
fn assert_override_refused(overrides: &str, expected: &str) {
    assert_refused(overrides, expected);
    let dir = scaffold();
    update_v1(dir.path());
    write(dir.path(), ".codeflow/overrides.toml", overrides);
    let error = run(&v1(), dir.path(), "3.0.0").expect_err("update refuses");
    assert!(
        error.contains("override 1 in .codeflow/overrides.toml"),
        "{error}"
    );
}

fn one(target: &str, field: &str) -> String {
    format!("schema_version = 1\n\n[[override]]\ntarget = \"{target}\"\n{field}\n")
}

#[test]
fn a_git_floor_unit_is_refused_by_its_floor() {
    assert_override_refused(
        &one("rule:git", "text = \"Commit anywhere.\""),
        "it targets a unit of the git floor",
    );
    assert_override_refused(
        &one("moment:secret", "action = \"anything\""),
        "the git floor is never overridden",
    );
}

#[test]
fn a_non_relaxable_unit_is_refused_by_its_floor() {
    assert_override_refused(
        &one("rule:authority", "text = \"Act freely.\""),
        "it targets a unit of the non-relaxable command class",
    );
    assert_override_refused(
        &one("moment:standing-rule", "action = \"accept any rule\""),
        "the non-relaxable command class is never overridden",
    );
}

#[test]
fn a_fixed_unit_is_refused_with_its_reason() {
    assert_override_refused(
        &one("moment:review", "action = \"skip it\""),
        "it targets a fixed unit, which is not a text-override target: Review policy",
    );
}

#[test]
fn the_review_independence_rule_is_refused() {
    assert_override_refused(
        &one("rule:review", "text = \"Self-review is enough.\""),
        "it targets a fixed unit, which is not a text-override target: Review policy",
    );
}

#[test]
fn the_cross_lineage_review_rule_is_refused() {
    assert_override_refused(
        &one("moment:shape", "action = \"plan alone; no other review\""),
        "it targets a fixed unit, which is not a text-override target: The row states the other-lineage review",
    );
    assert_refused(
        &one("moment:shape", "action = \"plan alone\""),
        "not instruction text; update changed nothing",
    );
}

#[test]
fn the_headless_peer_rule_is_refused() {
    assert_override_refused(
        &one("moment:delegate", "action = \"run peers headless\""),
        "it targets a fixed unit",
    );
    assert_override_refused(
        &one("rule:review", "title = \"Headless review is fine.\""),
        "the rule also states the no-headless rule",
    );
}

#[test]
fn an_unreadable_override_file_is_refused() {
    let unreadable = "the override file cannot be read";
    let dir = scaffold();
    let root = dir.path();
    update_v1(root);
    std::fs::create_dir(root.join(".codeflow/overrides.toml")).unwrap();
    let before = snapshot(root);
    let error = run(&v1(), root, "3.0.0").expect_err("a directory is refused");
    assert!(error.contains(unreadable), "{error}");
    assert!(error.contains(".codeflow/overrides.toml"), "{error}");
    assert_eq!(snapshot(root), before, "a refusal writes nothing");
    assert_eq!(doctor_overrides(root, plan_v1).status, Status::Fail);

    let dir = scaffold();
    let root = dir.path();
    update_v1(root);
    std::fs::write(root.join(".codeflow/overrides.toml"), [0xff, 0xfe, 0x00]).unwrap();
    let error = run(&v1(), root, "3.0.0").expect_err("invalid UTF-8 is refused");
    assert!(error.contains(unreadable), "{error}");

    #[cfg(unix)]
    {
        let dir = scaffold();
        let root = dir.path();
        update_v1(root);
        write(root, "elsewhere.toml", &one("rule:outcome", "text = \"x\""));
        std::os::unix::fs::symlink(
            root.join("elsewhere.toml"),
            root.join(".codeflow/overrides.toml"),
        )
        .unwrap();
        let error = run(&v1(), root, "3.0.0").expect_err("a symlink is refused");
        assert!(error.contains(unreadable), "{error}");
    }
}

#[test]
fn an_unknown_or_removed_unit_is_refused() {
    let removed = "it names a unit this build does not declare";
    assert_override_refused(&one("rule:no-such-rule", "text = \"x\""), removed);
    assert_override_refused(&one("moment:no-such-row", "action = \"x\""), removed);
    assert_override_refused(&one("cf-present#no-such-section", "body = \"x\""), removed);
    assert_override_refused(&one("cf-no-such-skill#completion", "body = \"x\""), removed);
    assert_override_refused(
        &one("outcome", "text = \"x\""),
        "its target is not a declared name",
    );
}

#[test]
fn a_duplicate_target_is_refused() {
    let twice = format!(
        "{}\n[[override]]\ntarget = \"rule:outcome\"\ntitle = \"Again.\"\n",
        one("rule:outcome", "text = \"Once.\"")
    );
    assert_refused(&twice, "an earlier override already targets the same unit");
    assert_refused(&twice, "override 2 in .codeflow/overrides.toml");
}

#[test]
fn a_structural_replacement_is_refused() {
    assert_refused(
        &one(
            "cf-present#completion",
            "body = \"\"\"\n## Smuggled\n\ntext\n\"\"\"",
        ),
        "would end the section early",
    );
    assert_refused(
        &one(
            "cf-present#completion",
            "body = \"\"\"\n<!-- codeflow:section id=x class=overridable -->\n\"\"\"",
        ),
        "holds a codeflow marker",
    );
    assert_refused(
        &one(
            "cf-present#completion",
            "body = \"\"\"\n```\nopen fence\n\"\"\"",
        ),
        "leaves a code fence open",
    );
    assert_refused(
        &one("rule:outcome", "text = \"two\\nlines\""),
        "a rule override sets only `text` and `title`",
    );
    assert_refused(
        &one("rule:outcome", "text = \"end <!-- codeflow:end -->\""),
        "a rule override sets only `text` and `title`",
    );
    assert_refused(
        &one("moment:build", "action = \"a | b\""),
        "a moment override sets only `action`",
    );
    assert_refused(
        &one("rule:outcome", "action = \"x\""),
        "a rule override sets only",
    );
    assert_refused(
        &one("cf-present#completion", "text = \"x\""),
        "a section override sets only `body`",
    );
    assert_refused(
        &one("rule:outcome", "text = \"x\"\nsurprise = 1"),
        "the override file does not parse",
    );
    assert_refused("schema_version = 2\n", "unknown schema_version");
}

// ---------------------------------------------------------------------------
// Interruption: the journal rolls a landed write forward, drops one that never
// landed and refuses any other state.
// ---------------------------------------------------------------------------

/// A project after an overridden v1 update, and the same project after the v2
/// update: the state an interrupted v2 update was writing.
fn interrupted() -> (tempfile::TempDir, tempfile::TempDir) {
    let dir = scaffold();
    write(dir.path(), ".codeflow/overrides.toml", OVERRIDES);
    update_v1(dir.path());
    let done = copy_project(dir.path());
    update_v2(done.path());
    (dir, done)
}

fn baseline(root: &Path, dest: &str) -> String {
    read(root, &format!(".codeflow/.baseline/{dest}"))
}

fn installed_sha(root: &Path, dest: &str) -> String {
    let manifest: serde_json::Value =
        serde_json::from_str(&read(root, ".codeflow/manifest.json")).unwrap();
    manifest["files"][dest]["sha256"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Journal the v2 write of `dest` in `root` as update does before it writes.
fn journal_v2_write(root: &Path, done: &Path, dest: &str) {
    let old = Journal::observe(root, dest, Some(&installed_sha(root, dest))).unwrap();
    let new_record = Record::read(done, dest).unwrap();
    let journal = Journal::new(
        old,
        &read(done, dest),
        &baseline(done, dest),
        new_record,
        &installed_sha(done, dest),
    );
    journal.write(root, dest).unwrap();
}

#[test]
fn a_landed_write_rolls_forward() {
    let (dir, done) = interrupted();
    let (root, done) = (dir.path(), done.path());
    let dest = PRESENT[0];
    journal_v2_write(root, done, dest);
    write(root, dest, &read(done, dest));
    let report = update_v2(root);
    assert!(
        report
            .notes
            .iter()
            .any(|note| note.as_str().contains("interrupted update had journaled")),
        "{report:?}"
    );
    assert_eq!(read(root, dest), read(done, dest));
    assert_eq!(baseline(root, dest), baseline(done, dest));
    assert_eq!(
        Record::read(root, dest).unwrap(),
        Record::read(done, dest).unwrap()
    );
    assert!(!root.join(record::journal_path(dest)).exists());
}

#[test]
fn a_write_that_never_landed_is_dropped() {
    let (dir, done) = interrupted();
    let (root, done) = (dir.path(), done.path());
    let dest = PRESENT[0];
    journal_v2_write(root, done, dest);
    update_v2(root);
    assert_eq!(read(root, dest), read(done, dest));
    assert_eq!(
        Record::read(root, dest).unwrap(),
        Record::read(done, dest).unwrap()
    );
    assert!(!root.join(record::journal_path(dest)).exists());
}

#[test]
fn a_state_that_matches_neither_generation_is_refused() {
    let (dir, done) = interrupted();
    let (root, done) = (dir.path(), done.path());
    let dest = PRESENT[0];
    journal_v2_write(root, done, dest);
    write(root, dest, "a third text\n");
    let before = snapshot(root);
    let error = run(&v2(), root, "3.0.1").expect_err("refused");
    assert!(error.contains("matches neither"), "{error}");
    assert_eq!(snapshot(root), before);
}

#[test]
fn a_landed_write_with_a_foreign_record_is_refused() {
    let (dir, done) = interrupted();
    let (root, done) = (dir.path(), done.path());
    let dest = PRESENT[0];
    journal_v2_write(root, done, dest);
    write(root, dest, &read(done, dest));
    let mut foreign = Record::read(done, dest).unwrap().unwrap();
    foreign.generation += 7;
    foreign.write(root, dest).unwrap();
    let before = snapshot(root);
    let error = run(&v2(), root, "3.0.1").expect_err("refused");
    assert!(error.contains("matches neither"), "{error}");
    assert_eq!(snapshot(root), before);
}

#[test]
fn a_record_that_does_not_match_the_baseline_is_refused() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);
    update_v1(root);
    let dest = PRESENT[0];
    let mut record = Record::read(root, dest).unwrap().unwrap();
    record.applied[0].replacement = "something else\n".into();
    record.write(root, dest).unwrap();
    let before = snapshot(root);
    let error = run(&v1(), root, "3.0.0").expect_err("refused");
    assert!(
        error.contains("does not match the installed baseline"),
        "{error}"
    );
    assert_eq!(snapshot(root), before);
}

// ---------------------------------------------------------------------------
// Security review of the unit (Codex, CHANGES at c31e131be): forged state is
// never health evidence, recovery checks everything before it writes, and
// structural checks read Markdown as CommonMark does.
// ---------------------------------------------------------------------------

fn managed_drift(root: &Path) -> doctor::CheckResult {
    doctor::run_check(
        "managed-drift",
        &Options {
            project_dir: root.to_string_lossy().into_owned(),
            override_plan: Some(plan_v1),
            ..Options::default()
        },
    )
    .unwrap()
}

/// A floor edited inside the managed block, with the record's effective
/// digest changed to the edited block's hash, is drift, fails the overrides
/// check and is refused by update; nothing reads the project's digest as
/// proof.
#[test]
fn a_forged_record_digest_is_not_health_evidence() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);
    update_v1(root);
    assert_eq!(managed_drift(root).status, Status::Pass);
    let agents = read(root, AGENTS);
    let floor = agents
        .lines()
        .find(|line| line.starts_with("- **Git floor [enforced].**"))
        .expect("the git floor row")
        .to_string();
    let tampered = agents.replacen(&floor, "- **Git floor.** Commit anywhere.", 1);
    write(root, AGENTS, &tampered);
    let block = scaffold::region::extract_block(&tampered, scaffold::RegionFormat::Markdown)
        .expect("the managed block");
    let mut record = Record::read(root, AGENTS).unwrap().unwrap();
    record.effective_sha256 = scaffold::sha256_hex(block.as_bytes());
    record.write(root, AGENTS).unwrap();

    let drift = managed_drift(root);
    assert!(drift.status.is_warn(), "{drift:?}");
    assert!(drift.message.contains(AGENTS), "{}", drift.message);
    let overrides = doctor_overrides(root, plan_v1);
    assert_eq!(overrides.status, Status::Fail, "{overrides:?}");
    let before = snapshot(root);
    let error = run(&v1(), root, "3.0.0").expect_err("update refuses the record");
    assert!(
        error.contains("does not match the installed baseline"),
        "{error}"
    );
    assert_eq!(snapshot(root), before);
}

/// An edit no override declares, with the record untouched, fails the
/// overrides check instead of reading as applied.
#[test]
fn an_undeclared_edit_fails_the_overrides_check() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);
    update_v1(root);
    let dest = PRESENT[0];
    write(
        root,
        dest,
        &read(root, dest).replacen(COMPLETION_OURS, "Skip the handoff.\n", 1),
    );
    let overrides = doctor_overrides(root, plan_v1);
    assert_eq!(overrides.status, Status::Fail, "{overrides:?}");
    assert!(overrides.message.contains(dest), "{}", overrides.message);
}

/// A journal left behind fails the overrides check, as update finishes or
/// refuses it first.
#[test]
fn a_pending_journal_fails_the_overrides_check() {
    let (dir, done) = interrupted();
    let (root, done) = (dir.path(), done.path());
    journal_v2_write(root, done, PRESENT[0]);
    let overrides = doctor_overrides(root, plan_v2);
    assert_eq!(overrides.status, Status::Fail, "{overrides:?}");
    assert!(
        overrides
            .message
            .contains("interrupted update left a journal"),
        "{}",
        overrides.message
    );
}

/// A journal whose digests match the state on disk but whose payload is not
/// what update writes (a record whose applied set differs from the declared
/// overrides, coherent with its own digests) is refused, and nothing is
/// written.
#[test]
fn a_forged_coherent_journal_payload_is_refused() {
    let (dir, done) = interrupted();
    let (root, done) = (dir.path(), done.path());
    let dest = PRESENT[0];
    let mut forged = Record::read(done, dest).unwrap().unwrap();
    forged.applied[0].replacement = "Skip the handoff.\n".into();
    let pristine = baseline(done, dest);
    let effective = scaffold::overrides::apply_units(&pristine, &forged.applied).unwrap();
    forged.effective_sha256 = scaffold::sha256_hex(effective.as_bytes());
    let old = Journal::observe(root, dest, Some(&installed_sha(root, dest))).unwrap();
    let journal = Journal::new(
        old,
        &read(done, dest),
        &pristine,
        Some(forged),
        &installed_sha(done, dest),
    );
    journal.write(root, dest).unwrap();
    write(root, dest, &read(done, dest));
    let before = snapshot(root);
    let error = run(&v2(), root, "3.0.1").expect_err("refused");
    assert!(
        error.contains("declared overrides this update applies"),
        "{error}"
    );
    assert_eq!(snapshot(root), before, "a refusal writes nothing");
}

/// A journal whose record is not the bytes its new state names is refused.
#[test]
fn a_journal_payload_off_its_digests_is_refused() {
    let (dir, done) = interrupted();
    let (root, done) = (dir.path(), done.path());
    let dest = PRESENT[0];
    journal_v2_write(root, done, dest);
    write(root, dest, &read(done, dest));
    let path = root.join(record::journal_path(dest));
    let mut journal: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    journal["record"]["applied"][0]["replacement"] = "Skip the handoff.\n".into();
    std::fs::write(&path, serde_json::to_string_pretty(&journal).unwrap()).unwrap();
    let before = snapshot(root);
    let error = run(&v2(), root, "3.0.1").expect_err("refused");
    assert!(error.contains("does not match the digests"), "{error}");
    assert_eq!(snapshot(root), before);
}

/// A recoverable journal for one file and a bad record for another: the
/// whole update refuses before recovery writes, so the first file and all
/// its metadata stay byte-identical.
#[test]
fn recovery_writes_nothing_when_another_record_refuses() {
    let (dir, done) = interrupted();
    let (root, done) = (dir.path(), done.path());
    let a = PRESENT[0];
    journal_v2_write(root, done, a);
    write(root, a, &read(done, a));
    let mut bad = Record::read(root, AGENTS).unwrap().unwrap();
    bad.applied[0].replacement = "- **Work to the outcome.** Something else.".into();
    bad.write(root, AGENTS).unwrap();
    let before = snapshot(root);
    let error = run(&v2(), root, "3.0.1").expect_err("refused");
    assert!(
        error.contains("does not match the installed baseline"),
        "{error}"
    );
    assert_eq!(
        snapshot(root),
        before,
        "file A and its metadata are untouched"
    );
    assert!(root.join(record::journal_path(a)).exists());
}

#[test]
fn markdown_that_would_hide_later_text_is_refused() {
    let body = |text: &str| one("cf-present#completion", &format!("body = '''\n{text}'''"));
    let open_fence = "leaves a code fence open";
    assert_refused(&body("````\nfour then three\n```\n"), open_fence);
    assert_refused(&body("~~~\ntilde then backticks\n```\n"), open_fence);
    assert_refused(&body("```\nbackticks then tilde\n~~~\n"), open_fence);
    let ends = "would end the section early";
    assert_refused(&body("Smuggled\n===\n"), ends);
    assert_refused(&body("Smuggled\n---\n"), ends);
    let html = "leaves a raw HTML block open";
    assert_refused(&body("<!-- hide the rest\n"), html);
    assert_refused(&body("<pre>\nhide the rest\n"), html);
}

#[test]
fn closed_fences_and_thematic_breaks_are_accepted() {
    let dir = scaffold();
    let root = dir.path();
    let body = "````\n```\nnested\n```\n````\n\n---\n\n<!-- a note -->\nend\n";
    write(
        root,
        ".codeflow/overrides.toml",
        &one("cf-present#completion", &format!("body = '''\n{body}'''")),
    );
    update_v1(root);
    for dest in PRESENT {
        assert!(read(root, dest).contains(body), "{dest}");
    }
}

// ---------------------------------------------------------------------------
// Security delta at e9b8536ef (Codex, CHANGES): a record consistent with
// itself still names only units the binary lets a project override, at the
// place the binary finds them; Markdown forms parsers disagree on are refused.
// ---------------------------------------------------------------------------

const GIT_FLOOR_ROW: &str = "- **Git floor [enforced].**";
const WEAKENED_FLOOR: &str = "- **Git floor [enforced].** Commit anywhere.";

/// The shipped git floor row of the pristine `AGENTS.md` block.
fn floor_row(root: &Path) -> String {
    baseline(root, AGENTS)
        .lines()
        .find(|line| line.starts_with(GIT_FLOOR_ROW))
        .expect("the git floor row")
        .to_string()
}

/// Write `record` for `AGENTS.md` with its effective digest recomputed, and
/// put the block it claims into the file: a forgery consistent with itself.
fn forge_agents(root: &Path, mut record: Record) {
    let pristine = baseline(root, AGENTS);
    let effective = scaffold::overrides::apply_units(&pristine, &record.applied)
        .expect("the forged units apply");
    record.effective_sha256 = scaffold::sha256_hex(effective.as_bytes());
    record.write(root, AGENTS).unwrap();
    let current = read(root, AGENTS);
    let block = scaffold::region::extract_block(&current, scaffold::RegionFormat::Markdown)
        .expect("the managed block");
    write(root, AGENTS, &current.replacen(&block, &effective, 1));
}

fn assert_forgery_refused(root: &Path) {
    let overrides = doctor_overrides(root, plan_v1);
    assert_eq!(overrides.status, Status::Fail, "{overrides:?}");
    let before = snapshot(root);
    let error = run(&v1(), root, "3.0.0").expect_err("update refuses the record");
    assert!(
        error.contains("does not match the installed baseline"),
        "{error}"
    );
    assert_eq!(snapshot(root), before, "a refusal writes nothing");
}

/// A record naming `rule:git`, which carries the shipped floor row as
/// replaced and a weakened floor as replacement, with every digest
/// recomputed: doctor fails, never "not yet applied", and update refuses.
#[test]
fn a_coherent_record_naming_a_floor_unit_is_refused() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);
    update_v1(root);
    let mut record = Record::read(root, AGENTS).unwrap().unwrap();
    record.applied.push(scaffold::overrides::AppliedUnit {
        entry: scaffold::overrides::Entry {
            target: "rule:git".into(),
            title: None,
            text: Some("Commit anywhere.".into()),
            action: None,
            body: None,
        },
        replaced: floor_row(root),
        replacement: WEAKENED_FLOOR.into(),
    });
    forge_agents(root, record);
    assert!(read(root, AGENTS).contains(WEAKENED_FLOOR));
    assert_forgery_refused(root);
}

/// A record naming the overridable `rule:outcome` whose replaced text is
/// the git floor row: the range is not the one the binary finds for that
/// name, so the record is refused.
#[test]
fn a_record_whose_range_is_not_its_unit_is_refused() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);
    update_v1(root);
    let mut record = Record::read(root, AGENTS).unwrap().unwrap();
    let outcome = record
        .applied
        .iter_mut()
        .find(|unit| unit.entry.target == "rule:outcome")
        .expect("the outcome unit");
    outcome.replaced = floor_row(root);
    outcome.replacement = WEAKENED_FLOOR.into();
    forge_agents(root, record);
    assert!(read(root, AGENTS).contains(WEAKENED_FLOOR));
    assert_forgery_refused(root);
}

/// A third scaffold version that rewords the overridden outcome rule, so a
/// record made from the first version holds a row the binary no longer
/// renders.
struct V3(DirSource);

const OUTCOME_V1: &str = "who uses it and the evidence that would establish it";
const OUTCOME_V3: &str = "who uses it and the proof that would establish it";

impl AssetSource for V3 {
    fn read(&self, path: &str) -> std::io::Result<Option<Vec<u8>>> {
        let Some(bytes) = self.0.read(path)? else {
            return Ok(None);
        };
        if !matches!(
            path,
            "base/rule-map.toml" | "base/AGENTS.md.tmpl" | "base/AGENTS.full.md.tmpl"
        ) {
            return Ok(Some(bytes));
        }
        let text = String::from_utf8(bytes).unwrap();
        assert!(
            text.contains(OUTCOME_V1),
            "{path} no longer holds {OUTCOME_V1:?}"
        );
        Ok(Some(text.replacen(OUTCOME_V1, OUTCOME_V3, 1).into_bytes()))
    }
}

fn plan_v3(root: &Path) -> Result<scaffold::overrides::Plan, scaffold::ScaffoldError> {
    update_mod::override_plan(&V3(v1()), root, "3.0.2")
}

/// A record made from an older shipped text stays valid: it is checked
/// against the baseline it was made from, where the binary finds the unit by
/// its identity even though the binary now renders the row differently.
#[test]
fn an_older_legitimate_record_still_passes() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);
    update_v1(root);
    let pending = doctor_overrides(root, plan_v3);
    assert!(pending.status.is_warn(), "{pending:?}");
    run(&V3(v1()), root, "3.0.2").unwrap_or_else(|error| panic!("update fails: {error}"));
    assert_overridden(root);
    assert!(!baseline(root, AGENTS).contains(OUTCOME_V1));
    assert!(baseline(root, AGENTS).contains(OUTCOME_V3));
    assert_eq!(doctor_overrides(root, plan_v3).status, Status::Pass);
}

#[test]
fn markdown_forms_parsers_disagree_on_are_refused() {
    let body = |text: &str| one("cf-present#completion", &format!("body = '''\n{text}'''"));
    let nested = "inside a list item or block quote, or under indentation";
    assert_refused(&body("- ```\n  ```\n```\n"), nested);
    assert_refused(&body("> ```\n> quoted fence\n"), nested);
    assert_refused(&body("text\n\n    indented code\n"), "indented code block");
    assert_refused(
        &one("cf-present#completion", "body = \"text\\r```\\nhidden\\n\""),
        "carriage return",
    );
    assert_refused(&body("- an item at the end\n"), "ends inside a list item");
}

// ---------------------------------------------------------------------------
// Security delta 2 at f00f46165 (Codex, CHANGES): a record made from an older
// shipped row may replace only the fields its entry declares; the row's
// pointers, `MUST OPEN` marks included, stay as that baseline has them.
// ---------------------------------------------------------------------------

/// A fourth scaffold version that rewords the action of the landing moment
/// and the text of the plain-writing rule, so records made from the first
/// version hold rows with pointers that this binary no longer renders.
struct V4(DirSource);

const REWORDS_V4: [(&str, &str); 2] = [
    ("one home per record", "one home for each record"),
    (
        "State the fact directly, and check",
        "State each fact directly, and check",
    ),
];

impl AssetSource for V4 {
    fn read(&self, path: &str) -> std::io::Result<Option<Vec<u8>>> {
        let Some(bytes) = self.0.read(path)? else {
            return Ok(None);
        };
        if !matches!(
            path,
            "base/rule-map.toml" | "base/AGENTS.md.tmpl" | "base/AGENTS.full.md.tmpl"
        ) {
            return Ok(Some(bytes));
        }
        let mut text = String::from_utf8(bytes).unwrap();
        for (from, to) in REWORDS_V4 {
            assert!(text.contains(from), "{path} no longer holds {from:?}");
            text = text.replacen(from, to, 1);
        }
        Ok(Some(text.into_bytes()))
    }
}

fn plan_v4(root: &Path) -> Result<scaffold::overrides::Plan, scaffold::ScaffoldError> {
    update_mod::override_plan(&V4(v1()), root, "3.0.3")
}

const ROWS: &str = r#"schema_version = 1

[[override]]
target = "moment:land"
action = "land as the team agrees"

[[override]]
target = "rule:plain"
text = "Write short sentences."
"#;

const LAND_WHEN: &str = "| land a batch on the integration line | ";
const LAND_OURS: &str = "| land a batch on the integration line | land as the team agrees | MUST OPEN `/cf-method` \"Managing a body of work\" (landing modes and drift control) |";
const PLAIN_OURS: &str =
    "- **Write plainly.** Write short sentences. See `.agents/skills/cf-editorial-review/references/editorial-smells.md`.";

/// A project on the first version with the landing moment and the
/// plain-writing rule overridden.
fn rows_overridden() -> tempfile::TempDir {
    let dir = scaffold();
    write(dir.path(), ".codeflow/overrides.toml", ROWS);
    update_v1(dir.path());
    let agents = read(dir.path(), AGENTS);
    assert!(agents.contains(LAND_OURS), "{agents}");
    assert!(agents.contains(PLAIN_OURS), "{agents}");
    dir
}

/// Forge the recorded replacement of `target` to `forged`, with every
/// digest recomputed, and check that the fourth version refuses it.
fn assert_older_forgery_refused(target: &str, forged: &str) {
    let dir = rows_overridden();
    let root = dir.path();
    let mut record = Record::read(root, AGENTS).unwrap().unwrap();
    record
        .applied
        .iter_mut()
        .find(|unit| unit.entry.target == target)
        .expect("the unit")
        .replacement = forged.to_string();
    forge_agents(root, record);
    assert!(read(root, AGENTS).contains(forged));
    let overrides = doctor_overrides(root, plan_v4);
    assert_eq!(overrides.status, Status::Fail, "{forged}: {overrides:?}");
    let before = snapshot(root);
    let error = run(&V4(v1()), root, "3.0.3").expect_err("update refuses the record");
    assert!(
        error.contains("does not match the installed baseline"),
        "{error}"
    );
    assert_eq!(snapshot(root), before, "a refusal writes nothing");
}

/// An older moment whose recorded replacement swaps the `MUST OPEN` pointer
/// cell for another one, or keeps it but changes the action from the
/// entry's: doctor fails and update refuses.
#[test]
fn a_forged_older_moment_cannot_change_its_pointer() {
    assert_older_forgery_refused(
        "moment:land",
        &format!("{LAND_WHEN}land as the team agrees | read `docs/our-landing.md` first |"),
    );
    assert_older_forgery_refused(
        "moment:land",
        &LAND_OURS.replace("land as the team agrees", "land without review"),
    );
}

/// An older rule whose recorded replacement drops its pointer, or keeps it
/// with text other than the entry's: doctor fails and update refuses.
#[test]
fn a_forged_older_rule_cannot_change_its_pointer() {
    assert_older_forgery_refused("rule:plain", "- **Write plainly.** Write short sentences.");
    assert_older_forgery_refused(
        "rule:plain",
        &PLAIN_OURS.replace("Write short sentences.", "Write anything."),
    );
}

/// A fifth scaffold version that renames the plain-writing rule's title.
struct V5(DirSource);

impl AssetSource for V5 {
    fn read(&self, path: &str) -> std::io::Result<Option<Vec<u8>>> {
        let Some(bytes) = self.0.read(path)? else {
            return Ok(None);
        };
        if !matches!(
            path,
            "base/rule-map.toml" | "base/AGENTS.md.tmpl" | "base/AGENTS.full.md.tmpl"
        ) {
            return Ok(Some(bytes));
        }
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("Write plainly."), "{path}");
        Ok(Some(
            text.replacen("Write plainly.", "Write simply.", 1)
                .into_bytes(),
        ))
    }
}

fn plan_v5(root: &Path) -> Result<scaffold::overrides::Plan, scaffold::ScaffoldError> {
    update_mod::override_plan(&V5(v1()), root, "3.0.4")
}

/// An upstream rename of an overridden rule's title makes update refuse the
/// old record, also once the entry is gone, since the record is still there.
/// Removing the record recovers: update applies the current entries to the
/// renamed rule, with its pointer.
#[test]
fn an_upstream_rename_recovers_once_its_record_is_removed() {
    let dir = rows_overridden();
    let root = dir.path();
    let before = snapshot(root);
    let error = run(&V5(v1()), root, "3.0.4").expect_err("update refuses the old record");
    assert!(
        error.contains("does not match the installed baseline"),
        "{error}"
    );
    assert_eq!(snapshot(root), before, "a refusal writes nothing");
    assert_eq!(doctor_overrides(root, plan_v5).status, Status::Fail);
    std::fs::remove_file(root.join(".codeflow/.baseline/AGENTS.md.overrides.json")).unwrap();
    run(&V5(v1()), root, "3.0.4").unwrap_or_else(|error| panic!("update fails: {error}"));
    let agents = read(root, AGENTS);
    assert!(
        agents.contains(&PLAIN_OURS.replace("Write plainly.", "Write simply.")),
        "{agents}"
    );
    assert!(agents.contains(LAND_OURS), "{agents}");
    assert_eq!(doctor_overrides(root, plan_v5).status, Status::Pass);
}

/// A record whose generation is the last `u64`, which no digest covers, is
/// refused before anything is written, by an update that would rewrite the
/// file and advance it; doctor fails.
#[test]
fn a_record_at_the_last_generation_is_refused() {
    let dir = scaffold();
    let root = dir.path();
    write(root, ".codeflow/overrides.toml", OVERRIDES);
    update_v1(root);
    let mut record = Record::read(root, AGENTS).unwrap().unwrap();
    record.generation = u64::MAX;
    record.write(root, AGENTS).unwrap();
    let before = snapshot(root);
    let error = run(&v2(), root, "3.0.1").expect_err("update refuses the record");
    assert!(
        error.contains("does not match the installed baseline"),
        "{error}"
    );
    assert_eq!(snapshot(root), before, "a refusal writes nothing");
    assert_eq!(doctor_overrides(root, plan_v2).status, Status::Fail);
}

/// Records made from the first version's rows still apply under the fourth,
/// with the rows' pointers kept.
#[test]
fn older_legitimate_rows_with_pointers_still_pass() {
    let dir = rows_overridden();
    let root = dir.path();
    let pending = doctor_overrides(root, plan_v4);
    assert!(pending.status.is_warn(), "{pending:?}");
    run(&V4(v1()), root, "3.0.3").unwrap_or_else(|error| panic!("update fails: {error}"));
    let agents = read(root, AGENTS);
    assert!(agents.contains(LAND_OURS), "{agents}");
    assert!(agents.contains(PLAIN_OURS), "{agents}");
    assert!(baseline(root, AGENTS).contains("one home for each record"));
    assert_eq!(doctor_overrides(root, plan_v4).status, Status::Pass);
}
