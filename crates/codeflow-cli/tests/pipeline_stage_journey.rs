//! TSK-370: the shipped pipeline verifies each change with the affected
//! targeted checks and the quick gate, and an existing copy is never
//! rewritten: `codeflow update` and `codeflow doctor` name the two-line edit
//! (the review line, so the quick gate runs once, and the verify line).
//!
//! Every test runs the real binary in a tempdir project.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[path = "support/process_folders.rs"]
mod process_folders;
use process_folders::isolated_home;

const PIPELINE: &str = ".claude/workflows/pipeline.workflow.js";
const BASELINE: &str = ".codeflow/.baseline/.claude/workflows/pipeline.workflow.js";
/// The pipeline 3.1.0 shipped, byte for byte (`git show
/// v3.1.0:assets/base/claude/workflows/pipeline.workflow.js`).
const PIPELINE_3_1_0: &str = include_str!("fixtures/pipeline-3.1.0.workflow.js");
const SHIPPED: &str = include_str!("../../../assets/base/claude/workflows/pipeline.workflow.js");
const OLD_START: &str =
    "'Run the affected targeted checks, then `codeflow test --mode essential --strict`";
const OLD_RUN: &str = "`codeflow test --mode essential --strict`";
/// The 3.1.0 review stage line, which runs the quick gate itself.
const OLD_REVIEW_START: &str =
    "'Run `codeflow validate` and `codeflow test --mode quick --strict`;";
const NEW_REVIEW_START: &str = "'Run `codeflow validate`' + (";
const QUICK: &str = "`codeflow test --mode quick --strict`";
const EDIT_LEAD: &str = "keeping its indentation: ";
/// What doctor says when the copy holds no old stage: only that, since a
/// user-owned copy's other stages are not checked.
const DOCTOR_PASS: &str = "the pipeline holds no `codeflow test --mode essential --strict` run (the 3.1.x verify stage); what its stages run otherwise is user-owned and not checked";
/// The claim doctor used to make without checking it.
const UNCHECKED_CLAIM: &str = "runs targeted checks and the quick gate";

fn test_path() -> std::ffi::OsString {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH")
}

fn codeflow(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", test_path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .stdin(Stdio::null())
        .output()
        .expect("binary runs")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn init_standard() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    let out = codeflow(&root, &["init", "--yes", "--standard"]);
    assert!(out.status.success(), "init failed:\n{}", text(&out));
    (dir, root)
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn write(root: &Path, rel: &str, content: &str) {
    std::fs::write(root.join(rel), content).unwrap_or_else(|e| panic!("write {rel}: {e}"));
}

/// Turn a fresh project into one installed by 3.1.0 as far as the pipeline
/// goes: the live copy and its recorded baseline hold the 3.1.0 bytes.
fn as_installed_by_3_1_0(root: &Path, live: &str) {
    write(root, PIPELINE, live);
    write(root, BASELINE, PIPELINE_3_1_0);
}

fn update(root: &Path) -> String {
    let out = codeflow(root, &["update"]);
    let printed = text(&out);
    assert!(out.status.success(), "update failed:\n{printed}");
    printed
}

fn doctor(root: &Path) -> String {
    text(&codeflow(root, &["doctor", "--check", "pipeline"]))
}

/// The two lines the output prints, in order: the review stage line, then
/// the verify stage line, each cut after its edit lead.
fn printed_lines(printed: &str) -> (String, String) {
    let cut = |from: usize, end_mark: &str| -> (String, usize) {
        let start = printed[from..]
            .find(EDIT_LEAD)
            .unwrap_or_else(|| panic!("no edit printed:\n{printed}"))
            + from
            + EDIT_LEAD.len();
        let rest = &printed[start..];
        let end = rest
            .find(end_mark)
            .unwrap_or_else(|| panic!("the printed line is cut short:\n{rest}"))
            + end_mark.len();
        (rest[..end].to_string(), start + end)
    };
    let (review, after) = cut(0, "never as a pass.'),");
    let (verify, _) = cut(after, "approved.',");
    (review, verify)
}

/// Apply the printed edit as a person would: replace each line that starts
/// with the old text, keeping its indentation.
fn apply_printed_edit(root: &Path, review: &str, verify: &str) {
    let copy = read(root, PIPELINE);
    let (mut reviews, mut verifies) = (0, 0);
    let edited: Vec<String> = copy
        .split('\n')
        .map(|l| {
            let body = l.trim_start();
            let indent = &l[..l.len() - body.len()];
            if body.starts_with(OLD_REVIEW_START) {
                reviews += 1;
                format!("{indent}{review}")
            } else if body.starts_with(OLD_START) {
                verifies += 1;
                format!("{indent}{verify}")
            } else {
                l.to_string()
            }
        })
        .collect();
    assert_eq!((reviews, verifies), (1, 1), "one old line of each to replace");
    write(root, PIPELINE, &edited.join("\n"));
}

/// The text between `open` and the first `close` after it.
fn between<'a>(text: &'a str, open: &str, close: &str) -> &'a str {
    let start = text
        .find(open)
        .unwrap_or_else(|| panic!("no {open:?} in:\n{text}"))
        + open.len();
    let end = text[start..]
        .find(close)
        .unwrap_or_else(|| panic!("no {close:?} after {open:?} in:\n{text}"))
        + start;
    &text[start..end]
}

/// A stage block of a pipeline source, such as `build` or `verify`.
fn stage_block<'a>(source: &'a str, name: &str) -> &'a str {
    let head = format!("  {name}: {{");
    let start = source.find(&head).unwrap_or_else(|| panic!("a {name} stage"));
    let end = source[start..].find("\n  },").expect("its end") + start;
    &source[start..end]
}

/// The final verification stage of a pipeline source.
fn verify_stage(source: &str) -> &str {
    let start = source.find("  verify: {").expect("a verify stage");
    let end = source[start..].find("\n  },").expect("its end") + start;
    &source[start..end]
}

/// AC-1, AC-2: the shipped stage runs the affected targeted checks and the
/// quick gate, never a whole essential gate, says the primary runs one full
/// gate on the landing candidate, and keeps the 3.1.0 strictness text.
#[test]
fn the_shipped_verify_stage_runs_targeted_checks_and_quick() {
    let stage = verify_stage(SHIPPED);
    assert!(
        stage.contains("Run the affected targeted checks"),
        "{stage}"
    );
    assert!(
        stage.contains("`codeflow test --mode essential --only <target>`"),
        "a check defined only in a heavier mode runs through --only:\n{stage}"
    );
    assert!(
        stage.contains("then `codeflow test --mode quick --strict`"),
        "{stage}"
    );
    assert!(
        !stage.contains(OLD_RUN),
        "the stage still runs a whole essential gate:\n{stage}"
    );
    assert!(
        stage.contains("never run a whole essential or full gate here"),
        "{stage}"
    );
    assert!(
        stage.contains("the primary runs one full gate on the exact landing candidate"),
        "{stage}"
    );

    // AC-2: the strictness text 3.1.0 shipped is kept word for word.
    let old_stage = verify_stage(PIPELINE_3_1_0);
    for kept in [
        "any nonzero exit is changes_requested.",
        "`--strict` makes a NoTargets run (the loud \"nothing to run\" banner — zero tests executed) exit non-zero: a no-op is not-verified, report it as changes_requested, never approved.",
        "Verdict 'approved' only when targeted checks and that gate ran green (never on a NoTargets/no-op run) and every criterion is met.",
    ] {
        assert!(old_stage.contains(kept), "3.1.0 held: {kept}");
        assert!(stage.contains(kept), "the stage lost: {kept}\n{stage}");
    }

    // The edit is two lines, the review line and the verify line:
    // everything else in the file is the 3.1.0 text.
    let old: Vec<&str> = PIPELINE_3_1_0.lines().collect();
    let new: Vec<&str> = SHIPPED.lines().collect();
    assert_eq!(old.len(), new.len());
    let changed: Vec<usize> = (0..old.len()).filter(|&i| old[i] != new[i]).collect();
    assert_eq!(changed.len(), 2, "two changed lines, got {changed:?}");
    assert!(old[changed[0]].trim_start().starts_with(OLD_REVIEW_START));
    assert!(new[changed[0]].trim_start().starts_with(NEW_REVIEW_START));
    assert!(old[changed[1]].trim_start().starts_with(OLD_START));
}

/// AC-1 over the whole default sequence: build, review and verify together
/// run the quick gate once, after the affected targeted checks. The review
/// stage runs it only when no verify stage follows.
#[test]
fn the_default_sequence_runs_quick_once_after_targeted_checks() {
    assert!(
        SHIPPED.contains("default: ['build', 'review', 'verify'],"),
        "the default preset is build, review, verify"
    );
    let review = review_stage_line(SHIPPED);
    let with_verify = between(&review, "PRESET.includes('verify') ? '", "' : '");
    let without_verify = between(&review, "' : '", "') + '");
    let tail = between(&review, "') + '; ", "'),");

    // The default preset holds verify, so review takes the first branch.
    let sequence = format!(
        "{}\nRun `codeflow validate`{with_verify}; {tail}\n{}",
        stage_block(SHIPPED, "build"),
        stage_block(SHIPPED, "verify"),
    );
    assert_eq!(
        sequence.matches(QUICK).count(),
        1,
        "the default sequence runs the quick gate once:\n{sequence}"
    );
    assert_eq!(
        sequence.matches("--mode quick").count(),
        1,
        "no other spelling of the quick gate:\n{sequence}"
    );
    let targeted = sequence
        .find("Run the affected targeted checks")
        .expect("the targeted checks");
    let quick = sequence.find(QUICK).expect("the quick gate");
    assert!(targeted < quick, "targeted checks come first:\n{sequence}");
    assert!(
        with_verify.contains("never run `codeflow test` here"),
        "review leaves the gate to verify: {with_verify}"
    );

    // A preset with review and no verify still runs targeted checks, then
    // quick once, in review.
    let alone = format!("Run `codeflow validate`{without_verify}; {tail}");
    assert_eq!(alone.matches(QUICK).count(), 1, "{alone}");
    assert!(
        alone.find("the affected targeted checks").expect("targeted") < alone.find(QUICK).unwrap(),
        "{alone}"
    );

    // AC-2: the review strictness text 3.1.0 shipped is kept word for word.
    let old_review = PIPELINE_3_1_0
        .lines()
        .map(str::trim_start)
        .find(|l| l.starts_with(OLD_REVIEW_START))
        .expect("the 3.1.0 review line");
    let kept = between(old_review, "--strict`; ", "'),");
    assert_eq!(tail, kept, "the review strictness text changed");
}

/// The shipped review stage line, without its indentation.
fn review_stage_line(source: &str) -> String {
    source
        .lines()
        .map(str::trim_start)
        .find(|l| l.starts_with(NEW_REVIEW_START))
        .expect("the shipped review line")
        .to_string()
}

/// AC-3: update never rewrites an unmodified 3.1.0 copy, and names the
/// exact edit: the review line, then the verify line.
#[test]
fn update_keeps_an_unmodified_3_1_0_copy_and_prints_the_edit() {
    let (_dir, root) = init_standard();
    as_installed_by_3_1_0(&root, PIPELINE_3_1_0);

    let printed = update(&root);

    assert_eq!(
        read(&root, PIPELINE),
        PIPELINE_3_1_0,
        "the copy was rewritten"
    );
    assert!(printed.contains(PIPELINE), "{printed}");
    assert!(
        printed.contains("user-owned: never mutated by update"),
        "{printed}"
    );
    assert!(
        printed.contains("still runs `codeflow test --mode essential --strict` on every change"),
        "{printed}"
    );
    assert_eq!(
        printed_lines(&printed),
        (review_stage_line(SHIPPED), verify_stage_line(SHIPPED)),
        "the printed edit is the shipped lines"
    );
}

/// AC-3: an edited copy that still runs essential keeps its bytes and gets
/// the same notice.
#[test]
fn update_keeps_an_edited_copy_and_prints_the_edit() {
    let (_dir, root) = init_standard();
    let edited = PIPELINE_3_1_0.replacen(
        "// Driver:",
        "// Our team runs this nightly.\n// Driver:",
        1,
    );
    assert_ne!(edited, PIPELINE_3_1_0);
    as_installed_by_3_1_0(&root, &edited);

    let printed = update(&root);

    assert_eq!(
        read(&root, PIPELINE),
        edited,
        "the edited copy was rewritten"
    );
    assert_eq!(
        printed_lines(&printed),
        (review_stage_line(SHIPPED), verify_stage_line(SHIPPED))
    );
}

/// A fresh copy runs the new stage, so update says nothing about it.
#[test]
fn update_says_nothing_for_a_copy_on_the_new_default() {
    let (_dir, root) = init_standard();
    let printed = update(&root);
    assert!(!printed.contains(EDIT_LEAD), "{printed}");
    assert!(!printed.contains(OLD_RUN), "{printed}");
}

/// The notice only advises: a copy that is not a regular file is said in
/// the output and never stops update, and doctor reports it.
#[test]
fn update_goes_on_when_the_copy_is_not_a_regular_file() {
    let (_dir, root) = init_standard();
    std::fs::remove_file(root.join(PIPELINE)).unwrap();
    std::fs::create_dir(root.join(PIPELINE)).unwrap();

    let printed = update(&root);

    assert!(root.join(PIPELINE).is_dir(), "update replaced the folder");
    assert!(
        printed.contains(
            "verify stage not checked, because the copy could not be read as a regular file"
        ),
        "{printed}"
    );
    assert!(!printed.contains(EDIT_LEAD), "{printed}");
    let checked = doctor(&root);
    assert!(
        checked.contains(PIPELINE),
        "doctor names the copy:\n{checked}"
    );
}

/// AC-4: doctor reports a copy that runs essential per change, naming the
/// edit, and nothing for one on targeted checks and quick.
#[test]
fn doctor_reports_the_old_stage_and_passes_the_new_one() {
    let (_dir, root) = init_standard();
    let fresh = doctor(&root);
    assert!(fresh.contains("pipeline"), "{fresh}");
    assert!(fresh.contains(DOCTOR_PASS), "a fresh copy passes:\n{fresh}");
    assert!(!fresh.contains(EDIT_LEAD), "{fresh}");

    as_installed_by_3_1_0(&root, PIPELINE_3_1_0);
    let old = doctor(&root);
    assert!(
        old.contains("still runs `codeflow test --mode essential --strict` on every change"),
        "{old}"
    );
    assert!(old.contains(PIPELINE), "{old}");
    assert_eq!(
        printed_lines(&old),
        (review_stage_line(SHIPPED), verify_stage_line(SHIPPED))
    );
}

/// Doctor's pass names only what it checked, the absence of the old
/// stage: an empty copy, a custom workflow with no verification and one
/// that runs a whole full gate each pass with that message, and none is
/// told it runs targeted checks and the quick gate.
#[test]
fn doctor_claims_only_the_absent_old_stage() {
    let (_dir, root) = init_standard();
    for copy in [
        "",
        "export const meta = { name: 'ours' };\nawait agent('build the change');\n",
        "export const meta = { name: 'full' };\nawait agent('run `codeflow test --mode full --strict`');\n",
    ] {
        write(&root, PIPELINE, copy);
        let checked = doctor(&root);
        assert!(checked.contains(DOCTOR_PASS), "{copy:?}:\n{checked}");
        assert!(!checked.contains(UNCHECKED_CLAIM), "{copy:?}:\n{checked}");
        assert!(!checked.contains(EDIT_LEAD), "{copy:?}:\n{checked}");
    }
}

/// AC-5: a fresh `init --standard` gets the new stage; a project updated
/// from 3.1.0 keeps its bytes, sees the edit in update and doctor, and
/// passes doctor once the printed edit is applied.
#[test]
fn journey_from_3_1_0_through_update_doctor_and_the_edit() {
    let (_fresh_dir, fresh) = init_standard();
    assert_eq!(
        read(&fresh, PIPELINE),
        SHIPPED,
        "a fresh install gets the new stage"
    );

    let (_dir, root) = init_standard();
    as_installed_by_3_1_0(&root, PIPELINE_3_1_0);

    let updated = update(&root);
    assert_eq!(read(&root, PIPELINE), PIPELINE_3_1_0);
    let (review, verify) = printed_lines(&updated);

    let before = doctor(&root);
    assert_eq!(
        printed_lines(&before),
        (review.clone(), verify.clone()),
        "update and doctor name one edit"
    );

    apply_printed_edit(&root, &review, &verify);
    assert_eq!(
        read(&root, PIPELINE),
        SHIPPED,
        "the edit gives the shipped file"
    );

    let after = doctor(&root);
    assert!(
        after.contains(DOCTOR_PASS),
        "doctor passes after the edit:\n{after}"
    );
    assert!(!after.contains(EDIT_LEAD), "{after}");

    // A later update still leaves the edited copy alone and says nothing.
    let again = update(&root);
    assert_eq!(read(&root, PIPELINE), SHIPPED);
    assert!(!again.contains(EDIT_LEAD), "{again}");
}

/// The shipped verify stage line, without its indentation.
fn verify_stage_line(source: &str) -> String {
    source
        .lines()
        .map(str::trim_start)
        .find(|l| l.starts_with("'Run the affected targeted checks"))
        .expect("the shipped verify line")
        .to_string()
}
