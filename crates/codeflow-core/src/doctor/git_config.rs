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
                        shown(&crate::git::display_path(&include.path)),
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
            format!(
                "{}; include-file exposure cannot be established",
                shown(&reason)
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

/// Text naming include files as doctor shows it: each URL's credentials
/// redacted, and in any path component or word that holds `@`, the text
/// before its last `@` replaced. An include value is read from a
/// configuration file that may not be trusted, and git reads a URL-shaped
/// value such as `https://user:token@host/config` as a relative path, so
/// the resolved path can carry `user:token@host` as one component, outside
/// any `scheme://` the URL scan finds (TSK-416 review round 4).
fn shown(text: &str) -> String {
    let urls = crate::git::remote_query::display_urls(text);
    let mut out = String::with_capacity(urls.len());
    for piece in urls.split_inclusive(|c| matches!(c, '/' | '\\' | ' ' | '\t' | '\n' | '\r')) {
        match piece.rfind('@') {
            Some(at) => {
                out.push_str("***");
                out.push_str(&piece[at..]);
            }
            None => out.push_str(piece),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::shown;

    /// TSK-416 review round 4: an include value holding a credential is
    /// shown redacted, as a URL and as the relative path git resolves it
    /// to; a path with no `@` is shown as it is. All credentials here are
    /// synthetic.
    #[test]
    fn an_include_path_is_shown_without_its_credential() {
        for (text, expected) in [
            (
                "https://user:OPAQUECANARY@host.invalid/config",
                "https://***@host.invalid/config",
            ),
            (
                "/repo/.git/https:/user:OPAQUECANARY@host.invalid/config",
                "/repo/.git/https:/***@host.invalid/config",
            ),
            (
                r"C:\repo\.git\https:\user:OPAQUECANARY@host.invalid\config",
                r"C:\repo\.git\https:\***@host.invalid\config",
            ),
            (
                "cannot read user:OPAQUECANARY@host.invalid/config: denied",
                "cannot read ***@host.invalid/config: denied",
            ),
            ("/home/me/.gitconfig.d/extra", "/home/me/.gitconfig.d/extra"),
        ] {
            assert_eq!(shown(text), expected, "{text}");
        }
        // The remedy asks for no configuration read that prints a value.
        let remedy = crate::remedy::DOCTOR_GIT_CONFIG_INCLUDES
            .remedy()
            .to_string();
        assert!(!remedy.contains("git config"), "{remedy}");
        assert!(remedy.contains("with an editor"), "{remedy}");
    }
}
