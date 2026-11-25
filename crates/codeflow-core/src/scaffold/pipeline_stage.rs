//! The verify stage of the shipped pipeline workflow (TSK-370).
//!
//! Through 3.1.x the user-owned `.claude/workflows/pipeline.workflow.js`
//! ended with a whole `codeflow test --mode essential --strict` run on every
//! change whenever a target defined an essential mode. The shipped default
//! is now the affected targeted checks plus `codeflow test --mode quick
//! --strict`, with one full gate on the landing candidate. The file stays
//! user-owned, so `codeflow update` never rewrites a copy; it and
//! `codeflow doctor --check pipeline` name the two-line edit instead: the
//! review line, so the quick gate runs once after the targeted checks, and
//! the verify line.

/// Where the pipeline workflow is installed.
pub const PIPELINE_DEST: &str = ".claude/workflows/pipeline.workflow.js";

/// The command only the old verify stage runs; a copy that still holds it
/// runs a whole essential gate on every change.
pub const OLD_STAGE_MARKER: &str = "`codeflow test --mode essential --strict`";

/// The verify stage line this release ships, as the file holds it after
/// its indentation.
macro_rules! new_stage_line {
    () => {
        r#"'Run the affected targeted checks (a check defined only in a heavier mode runs alone through `--only`, such as `codeflow test --mode essential --only <target>`), then `codeflow test --mode quick --strict`; never run a whole essential or full gate here, since the primary runs one full gate on the exact landing candidate. Report the exact exit code of each as a finding; any nonzero exit is changes_requested. `--strict` makes a NoTargets run (the loud "nothing to run" banner — zero tests executed) exit non-zero: a no-op is not-verified, report it as changes_requested, never approved.',"#
    };
}

/// The verify stage line this release ships, without its indentation.
pub const NEW_STAGE_LINE: &str = new_stage_line!();

/// The review stage line this release ships, as the file holds it after
/// its indentation. With a verify stage in the run, review leaves the
/// targeted checks and the quick gate to verify, so the quick gate runs
/// once, after the targeted checks; without one, review runs them itself.
macro_rules! new_review_line {
    () => {
        r#"'Run `codeflow validate`' + (PRESET.includes('verify') ? ' and read the diff and the tests; the verify stage that follows runs the affected targeted checks and then the quick gate once for this revision, so never run `codeflow test` here' : ' and the affected targeted checks (a check defined only in a heavier mode runs alone through `--only`), then `codeflow test --mode quick --strict`') + '; any nonzero exit is an automatic changes_requested. `--strict` makes a NoTargets run (the loud "nothing to run" banner — zero tests executed) exit non-zero: that is not-verified, treat it as changes_requested, never as a pass.'),"#
    };
}

/// The review stage line this release ships, without its indentation.
pub const NEW_REVIEW_LINE: &str = new_review_line!();

/// The note `codeflow update` prints for a copy that still runs essential
/// per change: the exact two-line edit, review line first.
pub const UPDATE_NOTICE: &str = concat!(
    "still runs `codeflow test --mode essential --strict` on every change; ",
    "the new default runs the affected targeted checks and then `codeflow test --mode quick --strict` once, ",
    "with one full gate on the landing candidate. To adopt it, replace two lines. First the review stage line that starts ",
    "'Run `codeflow validate` and `codeflow test --mode quick --strict`;'",
    " with this line, keeping its indentation: ",
    new_review_line!(),
    " Then the verify stage line that starts ",
    "'Run the affected targeted checks, then `codeflow test --mode essential --strict`'",
    " with this line, keeping its indentation: ",
    new_stage_line!(),
    " A project that wants an essential run on every change keeps its copy as it is",
);

/// The note `codeflow update` prints when it cannot read a copy as a
/// regular file to check its verify stage.
pub const UNREAD_NOTICE: &str = "verify stage not checked, because the copy could not be read as a regular file; `codeflow doctor --check pipeline` reports it";

/// Whether a pipeline copy still runs a whole essential gate on every
/// change. The bytes are searched as they are, so nothing is decoded.
#[must_use]
pub fn runs_essential_per_change(copy: &[u8]) -> bool {
    let marker = OLD_STAGE_MARKER.as_bytes();
    copy.windows(marker.len()).any(|window| window == marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_stage_is_found_and_the_new_line_is_not() {
        let old = "      'Run the affected targeted checks, then `codeflow test --mode essential --strict` when a target defines it.',\n";
        assert!(runs_essential_per_change(old.as_bytes()));
        assert!(!runs_essential_per_change(NEW_STAGE_LINE.as_bytes()));
        assert!(!runs_essential_per_change(b"\xff\xfe not text"));
    }

    #[test]
    fn the_new_line_keeps_the_strictness_text() {
        assert!(NEW_STAGE_LINE.contains("`codeflow test --mode quick --strict`"));
        assert!(NEW_STAGE_LINE.contains("any nonzero exit is changes_requested"));
        assert!(NEW_REVIEW_LINE.contains("any nonzero exit is an automatic changes_requested"));
        assert!(!runs_essential_per_change(NEW_REVIEW_LINE.as_bytes()));
        let review = UPDATE_NOTICE
            .find(NEW_REVIEW_LINE)
            .expect("the review line");
        let verify = UPDATE_NOTICE.find(NEW_STAGE_LINE).expect("the verify line");
        assert!(review < verify, "the review line comes first");
    }

    #[test]
    fn the_shipped_file_holds_both_lines() {
        let shipped = include_str!("../../../../assets/base/claude/workflows/pipeline.workflow.js");
        let lines: Vec<&str> = shipped.lines().map(str::trim_start).collect();
        assert!(lines.contains(&NEW_REVIEW_LINE));
        assert!(lines.contains(&NEW_STAGE_LINE));
    }
}
