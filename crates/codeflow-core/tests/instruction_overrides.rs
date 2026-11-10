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
    let drift = doctor::run_check(
        "managed-drift",
        &Options {
            project_dir: root.to_string_lossy().into_owned(),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(drift.status, Status::Pass, "{drift:?}");
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
