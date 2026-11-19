//! Report a Git too old to judge a merge, before a landing meets it.

use std::time::Instant;

use super::{CheckResult, Options, Status};
use crate::workgraph::history::{installed_git_version, merge_git_refusal};

pub(super) fn check(_opts: &Options) -> CheckResult {
    let start = Instant::now();
    let (status, message) = reading(installed_git_version());
    CheckResult {
        name: "git-version".into(),
        status,
        message,
        duration: start.elapsed(),
    }
}

fn reading(version: Result<String, String>) -> (Status, String) {
    match version {
        Ok(version) => merge_git_refusal(&version).map_or_else(
            || (Status::Pass, format!("{version} can judge merges")),
            |refusal| (Status::Fail, refusal),
        ),
        Err(reason) => (Status::Fail, reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsk1185_doctor_fails_a_git_too_old_to_judge_a_merge() {
        let (status, message) = reading(Ok("git version 2.34.1".into()));
        assert!(matches!(status, Status::Fail));
        assert!(message.contains("Git 2.34.1 is too old"), "{message}");
        assert!(message.contains("not a bad merge"), "{message}");

        let (status, _) = reading(Err("cannot run git: not found".into()));
        assert!(matches!(status, Status::Fail));

        let (status, message) = reading(Ok("git version 2.41.0".into()));
        assert!(matches!(status, Status::Pass));
        assert_eq!(message, "git version 2.41.0 can judge merges");
    }
}
