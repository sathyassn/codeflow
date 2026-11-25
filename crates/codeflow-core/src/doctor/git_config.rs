//! Report configuration includes and uncertainty in their discovery.

use std::path::Path;
use std::time::Instant;

use super::{CheckResult, Options, Status};

pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let (status, message) = match crate::hooks::edit_guard::included_git_config_files(Path::new(
        &opts.project_dir,
    )) {
        Ok(includes) if includes.is_empty() => (
            Status::Pass,
            "Git configuration names no include files".into(),
        ),
        Ok(includes) => {
            // Each include is named by the configuration file that holds it
            // and its position there, never by its value or the path that
            // value resolves to: a value read from configuration that may not
            // be trusted can hold a credential (TSK-416 review round 5).
            let includes = includes
                .iter()
                .map(|include| {
                    format!(
                        "{} ({})",
                        include.entry,
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
                format!("Git configuration includes: {includes}. Present files may be writable; include-file writes are not guarded. Files behind inactive nested conditions and concurrent configuration changes are not verified"))
        }
        Err(reason) => (
            Status::Warn(crate::remedy::DOCTOR_GIT_CONFIG_INCLUDES.remedy()),
            format!(
                "{}; include-file exposure cannot be established",
                crate::git::display_text(&reason)
            ),
        ),
    };
    CheckResult {
        name: "git-config-includes".into(),
        status,
        message,
        duration: start.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    /// The remedy asks for no configuration read that prints a value: an
    /// include value can hold a credential (TSK-416 review round 4).
    #[test]
    fn the_include_remedy_prints_no_configuration_value() {
        let remedy = crate::remedy::DOCTOR_GIT_CONFIG_INCLUDES
            .remedy()
            .to_string();
        assert!(!remedy.contains("git config"), "{remedy}");
        assert!(remedy.contains("with an editor"), "{remedy}");
    }
}
