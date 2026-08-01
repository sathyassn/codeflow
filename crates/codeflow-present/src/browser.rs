use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use crate::{error::PresentError, state::SessionStore, Result};

/// Launch the one currently qualified native route without touching the
/// operator's browser profile or active window.
pub fn launch_isolated(
    store: &SessionStore,
    bootstrap_path: &Path,
    profile_dir: &Path,
) -> Result<Child> {
    verify_owned_child(store.root(), bootstrap_path)?;
    let app_url = file_url(bootstrap_path)?;
    launch_url(store, &app_url, profile_dir)
}

pub fn launch_application(
    store: &SessionStore,
    authority: &str,
    profile_dir: &Path,
) -> Result<Child> {
    if !authority.starts_with("127.0.0.1:")
        || !authority
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b':'))
    {
        return Err(PresentError::BrowserUnavailable(
            "stored presentation authority is invalid".to_string(),
        ));
    }
    launch_url(store, &format!("http://{authority}/app/"), profile_dir)
}

fn launch_url(store: &SessionStore, app_url: &str, profile_dir: &Path) -> Result<Child> {
    verify_owned_child(store.root(), profile_dir)?;
    let executable = qualified_browser()?;

    Command::new(&executable)
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-sync")
        .arg("--disable-default-apps")
        .arg("--disable-extensions")
        .arg(format!("--app={app_url}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| PresentError::io(executable, error))
}

fn qualified_browser() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let chrome = PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
        if chrome.is_file() {
            return Ok(chrome);
        }
        return Err(PresentError::BrowserUnavailable(
            "the qualified macOS Google Chrome route is unavailable; use --no-launch".to_string(),
        ));
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(PresentError::BrowserUnavailable(
            "this native browser route is pending TSK-007 qualification; use --no-launch"
                .to_string(),
        ))
    }
}

fn verify_owned_child(root: &Path, path: &Path) -> Result<()> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| PresentError::io(root, error))?;
    let canonical = path
        .canonicalize()
        .map_err(|error| PresentError::io(path, error))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

fn file_url(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .ok_or_else(|| PresentError::UnsafePath(path.to_path_buf()))?;
    let mut encoded = String::from("file://");
    if !value.starts_with('/') {
        encoded.push('/');
    }
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'/' | b'-' | b'_' | b'.' | b'~' | b':')
        {
            encoded.push(char::from(*byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::file_url;

    #[test]
    fn file_url_encodes_spaces_and_unicode_bytes() {
        let url = file_url(std::path::Path::new("/tmp/review ü.html")).unwrap();
        assert_eq!(url, "file:///tmp/review%20%C3%BC.html");
    }
}
