use std::{fs, path::Path};

#[cfg(windows)]
use crate::PresentError;
use crate::Result;

#[cfg(unix)]
pub(crate) fn is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
pub(crate) fn is_link_like(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
pub(crate) fn harden_private_path(path: &Path, directory: bool) -> Result<()> {
    use std::{process::Command, sync::OnceLock};

    static SID: OnceLock<std::result::Result<String, String>> = OnceLock::new();
    let sid = SID
        .get_or_init(|| {
            let whoami = trusted_system_path("whoami.exe").map_err(|error| error.to_string())?;
            let output = Command::new(whoami)
                .args(["/user", "/fo", "csv", "/nh"])
                .output()
                .map_err(|error| error.to_string())?;
            if !output.status.success() || output.stdout.len() > 4_096 {
                return Err("whoami did not return a bounded user SID".to_string());
            }
            let text = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
            parse_whoami_sid(&text)
                .ok_or_else(|| "whoami output did not contain a user SID".to_string())
        })
        .as_ref()
        .map_err(|message| PresentError::ServiceUnavailable(message.clone()))?;
    let grant = if directory {
        format!("*{sid}:(OI)(CI)F")
    } else {
        format!("*{sid}:F")
    };
    let output = Command::new(trusted_system_path("icacls.exe")?)
        .arg(path)
        .args(["/inheritance:r", "/grant:r", &grant])
        .output()
        .map_err(|error| PresentError::io(path, error))?;
    if !output.status.success() || output.stdout.len() + output.stderr.len() > 64 * 1024 {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn trusted_system_path(relative: &str) -> Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt as _;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    let relative = std::path::Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(PresentError::UnsafePath(relative.to_path_buf()));
    }
    let mut buffer = vec![0_u16; 32_768];
    let length = unsafe {
        GetSystemDirectoryW(
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).expect("bounded Windows path buffer"),
        )
    };
    if length == 0 || usize::try_from(length).map_or(true, |length| length >= buffer.len()) {
        return Err(PresentError::ServiceUnavailable(
            "Windows system directory is unavailable".to_string(),
        ));
    }
    let directory = std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &buffer[..usize::try_from(length).expect("Windows path length fits usize")],
    ));
    let path = directory.join(relative);
    let mut cursor = directory;
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            continue;
        };
        cursor.push(component);
        let metadata =
            fs::symlink_metadata(&cursor).map_err(|error| PresentError::io(&cursor, error))?;
        if is_link_like(&metadata) {
            return Err(PresentError::UnsafePath(cursor));
        }
    }
    let metadata = fs::symlink_metadata(&path).map_err(|error| PresentError::io(&path, error))?;
    if !metadata.is_file() || is_link_like(&metadata) {
        return Err(PresentError::UnsafePath(path));
    }
    Ok(path)
}

#[cfg(windows)]
pub(crate) fn trusted_descendant_file(root: &Path, relative: &str) -> Result<std::path::PathBuf> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(PresentError::UnsafePath(relative.to_path_buf()));
    }
    let mut path = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            unreachable!("relative path was validated above")
        };
        path.push(component);
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| PresentError::io(&path, error))?;
        if is_link_like(&metadata) {
            return Err(PresentError::UnsafePath(path));
        }
    }
    let metadata = fs::symlink_metadata(&path).map_err(|error| PresentError::io(&path, error))?;
    if !metadata.is_file() {
        return Err(PresentError::UnsafePath(path));
    }
    Ok(path)
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn trusted_system_path_rejects_traversal_and_accepts_known_binary() {
        assert!(trusted_system_path("../taskkill.exe").is_err());
        assert!(trusted_system_path("taskkill.exe").is_ok());
    }
}

#[cfg(windows)]
pub(crate) fn known_folder(id: &windows_sys::core::GUID) -> Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt as _;
    use windows_sys::Win32::{System::Com::CoTaskMemFree, UI::Shell::SHGetKnownFolderPath};

    let mut raw = std::ptr::null_mut();
    let status = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut raw) };
    if status < 0 || raw.is_null() {
        return Err(PresentError::ServiceUnavailable(
            "Windows known-folder lookup failed".to_string(),
        ));
    }
    let mut length = 0_usize;
    while length < 32_768 && unsafe { *raw.add(length) } != 0 {
        length += 1;
    }
    if length == 32_768 {
        unsafe { CoTaskMemFree(raw.cast()) };
        return Err(PresentError::UnsafePath("Windows known folder".into()));
    }
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    unsafe { CoTaskMemFree(raw.cast()) };
    if !path.is_absolute() {
        return Err(PresentError::UnsafePath(path));
    }
    Ok(path)
}

#[cfg(any(windows, test))]
fn parse_whoami_sid(text: &str) -> Option<String> {
    text.split(',')
        .map(|field| field.trim().trim_matches('"'))
        .find(|field| {
            field.starts_with("S-1-")
                && field.len() <= 184
                && field
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'-' || byte == b'S')
        })
        .map(str::to_owned)
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn harden_private_path(_path: &Path, _directory: bool) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_sid_parser_accepts_only_a_bounded_sid_field() {
        assert_eq!(
            parse_whoami_sid(r#""desktop\\user","S-1-5-21-123-456-789-1001""#).as_deref(),
            Some("S-1-5-21-123-456-789-1001")
        );
        assert!(parse_whoami_sid(r#""user","S-1-5-21 & whoami""#).is_none());
    }
}
