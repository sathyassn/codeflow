//! The `pipeline` check (TSK-370): an installed pipeline workflow whose
//! verify stage still runs a whole essential gate on every change.
//!
//! The pipeline is user-owned, so `codeflow update` never moves a copy to
//! the new default of targeted checks plus the quick gate. This check names
//! the one-line edit. The old stage only warns, since a project may keep an
//! essential run on every change on purpose. A copy that exists but cannot
//! be read fails the check.

use std::path::PathBuf;
use std::time::Instant;

use super::{input_failure, CheckResult, Options, Status};
use crate::remedy;
use crate::scaffold::pipeline_stage::{runs_essential_per_change, NEW_STAGE_LINE, PIPELINE_DEST};

const NAME: &str = "pipeline";

pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let path = PathBuf::from(&opts.project_dir).join(PIPELINE_DEST);
    let result = |status: Status, message: &str| CheckResult {
        name: NAME.into(),
        status,
        message: message.into(),
        duration: start.elapsed(),
    };
    match crate::absence::proven_absent(&path) {
        Ok(true) => {
            return result(
                Status::Pass,
                "pipeline workflow not installed (check not applicable)",
            )
        }
        Ok(false) => {}
        Err(error) => return input_failure(NAME, start, &path, error),
    }
    let bytes = match crate::bounded_file::read_regular(&path) {
        Ok(bytes) => bytes,
        Err(error) => return input_failure(NAME, start, &path, error),
    };
    if runs_essential_per_change(&bytes) {
        result(
            Status::Warn(remedy::DOCTOR_PIPELINE_STAGE.with(&[("line", NEW_STAGE_LINE)])),
            "the pipeline's verify stage still runs `codeflow test --mode essential --strict` on every change; the shipped default runs the affected targeted checks and `codeflow test --mode quick --strict`, with one full gate on the landing candidate",
        )
    } else {
        result(
            Status::Pass,
            "the pipeline's verify stage runs targeted checks and the quick gate",
        )
    }
}
