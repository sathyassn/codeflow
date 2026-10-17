//! Report configuration includes and uncertainty in their discovery.

use std::path::Path;
use std::time::Instant;

use super::{CheckResult, Options, Status};

pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let (status, message) = match crate::hooks::edit_guard::included_git_config_files(Path::new(
        &opts.project_dir,
    )) {
        Ok(paths) if paths.is_empty() => (
            Status::Pass,
            "Git configuration names no include files".into(),
        ),
        Ok(paths) => {
            let paths = paths
                .iter()
                .map(|include| {
                    format!(
                        "{} ({})",
                        include.path.display(),
                        if include.missing {
                            "missing; contributes no configuration"
                        } else {
                            "present; writability not verified"
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            (Status::Warn(crate::remedy::DOCTOR_GIT_CONFIG_INCLUDES.remedy()),
                format!("Git configuration includes: {paths}. Present files may be writable; include-file writes are not guarded. Files behind inactive nested conditions and concurrent configuration changes are not verified"))
        }
        Err(reason) => (
            Status::Warn(crate::remedy::DOCTOR_GIT_CONFIG_INCLUDES.remedy()),
            format!("{reason}; include-file exposure cannot be established"),
        ),
    };
    CheckResult {
        name: "git-config-includes".into(),
        status,
        message,
        duration: start.elapsed(),
    }
}
