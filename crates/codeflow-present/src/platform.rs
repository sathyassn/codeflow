use std::{ffi::OsStr, fs, path::Path, process::Command};

#[cfg(windows)]
use crate::PresentError;
use crate::Result;

const ALLOWED_CHILD_ENVIRONMENT: &[&str] = &[
    "HOME",
    "TMPDIR",
    "TMP",
    "TEMP",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "SYSTEMROOT",
    "WINDIR",
];

/// Build every external child from a small explicit environment. Callers use
/// qualified absolute paths; inheriting `PATH`, provider keys, or the rest of
/// the host session is neither required nor permitted.
pub(crate) fn restricted_command(executable: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(executable);
    apply_restricted_environment(&mut command);
    command
}

#[cfg(unix)]
pub(crate) fn harden_private_file(path: &Path, file: &fs::File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| crate::PresentError::io(path, error))
}

#[cfg(windows)]
pub(crate) fn open_private_create_new(path: &Path) -> Result<fs::File> {
    use std::os::windows::{ffi::OsStrExt as _, io::FromRawHandle as _};
    use windows_sys::Win32::{
        Foundation::{LocalFree, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
        Storage::FileSystem::{
            CreateFileW, CREATE_NEW, DELETE, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL,
        },
    };

    let current = current_user_sid()?;
    let sddl = format!("O:{sid}G:{sid}D:P(A;;GA;;;{sid})", sid = current.text);
    let sddl = sddl
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &raw mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
        || descriptor.is_null()
    {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    let security = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>())
            .expect("security attributes size fits u32"),
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE | DELETE | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &raw const security,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    unsafe { LocalFree(descriptor.cast()) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(PresentError::io(path, std::io::Error::last_os_error()));
    }
    let file = unsafe { fs::File::from_raw_handle(handle.cast()) };
    if let Err(error) = verify_private_file(path, &file) {
        return Err(error);
    }
    Ok(file)
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn harden_private_file(path: &Path, _file: &fs::File) -> Result<()> {
    harden_private_path(path, false)
}

fn apply_restricted_environment(command: &mut Command) {
    command.env_clear();
    for name in ALLOWED_CHILD_ENVIRONMENT {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

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
    let sid = &current_user_sid()?.text;
    let grant = if directory {
        format!("*{sid}:(OI)(CI)F")
    } else {
        format!("*{sid}:F")
    };
    let icacls = trusted_system_path("icacls.exe")?;
    let output = icacls_command(&icacls, path, &grant)
        .output()
        .map_err(|error| PresentError::io(path, error))?;
    if !output.status.success() || output.stdout.len() + output.stderr.len() > 64 * 1024 {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn icacls_command(executable: &Path, path: &Path, grant: &str) -> Command {
    let mut command = restricted_command(executable);
    command
        .arg(path)
        .args(["/inheritance:r", "/grant:r", grant]);
    command
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

#[cfg(windows)]
struct CurrentUserSid {
    storage: Box<[usize]>,
    text: String,
}

#[cfg(windows)]
impl CurrentUserSid {
    fn as_ptr(&self) -> windows_sys::Win32::Security::PSID {
        self.storage.as_ptr().cast_mut().cast()
    }
}

#[cfg(windows)]
fn current_user_sid() -> Result<&'static CurrentUserSid> {
    use std::sync::OnceLock;

    static SID: OnceLock<std::result::Result<CurrentUserSid, String>> = OnceLock::new();
    SID.get_or_init(query_current_user_sid)
        .as_ref()
        .map_err(|message| PresentError::ServiceUnavailable(message.clone()))
}

#[cfg(windows)]
#[allow(clippy::too_many_lines)] // Linear Win32 token ownership keeps FFI lifetimes auditable.
fn query_current_user_sid() -> std::result::Result<CurrentUserSid, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree, HANDLE},
        Security::{
            Authorization::ConvertSidToStringSidW, CopySid, GetLengthSid, GetTokenInformation,
            IsValidSid, TokenUser, TOKEN_QUERY, TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    let mut raw_token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut raw_token) } == 0 {
        return Err(format!(
            "current Windows process token is unavailable: {}",
            std::io::Error::last_os_error()
        ));
    }
    let token = Token(raw_token);
    let mut required = 0_u32;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            std::ptr::null_mut(),
            0,
            &raw mut required,
        )
    };
    if required == 0 || required > 64 * 1024 {
        return Err("current Windows user token exceeded its bounded shape".to_string());
    }
    let word = std::mem::size_of::<usize>();
    let words = usize::try_from(required)
        .ok()
        .and_then(|bytes| bytes.checked_add(word - 1))
        .map(|bytes| bytes / word)
        .ok_or_else(|| "current Windows user token is not addressable".to_string())?;
    let mut token_storage = vec![0_usize; words];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            token_storage.as_mut_ptr().cast(),
            required,
            &raw mut required,
        )
    } == 0
    {
        return Err(format!(
            "current Windows user token could not be read: {}",
            std::io::Error::last_os_error()
        ));
    }
    let token_user = unsafe { &*token_storage.as_ptr().cast::<TOKEN_USER>() };
    if token_user.User.Sid.is_null() || unsafe { IsValidSid(token_user.User.Sid) } == 0 {
        return Err("current Windows user SID is invalid".to_string());
    }
    let sid_bytes = unsafe { GetLengthSid(token_user.User.Sid) };
    if sid_bytes == 0 || sid_bytes > 256 {
        return Err("current Windows user SID exceeded its bound".to_string());
    }
    let sid_words = usize::try_from(sid_bytes)
        .ok()
        .and_then(|bytes| bytes.checked_add(word - 1))
        .map(|bytes| bytes / word)
        .ok_or_else(|| "current Windows user SID is not addressable".to_string())?;
    let mut sid_storage = vec![0_usize; sid_words].into_boxed_slice();
    if unsafe {
        CopySid(
            sid_bytes,
            sid_storage.as_mut_ptr().cast(),
            token_user.User.Sid,
        )
    } == 0
    {
        return Err(format!(
            "current Windows user SID could not be copied: {}",
            std::io::Error::last_os_error()
        ));
    }

    let mut raw_text = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid_storage.as_ptr().cast_mut().cast(), &raw mut raw_text) }
        == 0
        || raw_text.is_null()
    {
        return Err(format!(
            "current Windows user SID could not be rendered: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut text_len = 0_usize;
    while text_len <= 184 && unsafe { *raw_text.add(text_len) } != 0 {
        text_len += 1;
    }
    if text_len > 184 {
        unsafe { LocalFree(raw_text.cast()) };
        return Err("current Windows user SID text exceeded its bound".to_string());
    }
    let text = String::from_utf16(unsafe { std::slice::from_raw_parts(raw_text, text_len) })
        .map_err(|_| "current Windows user SID text was invalid UTF-16".to_string());
    unsafe { LocalFree(raw_text.cast()) };
    let text = text?;
    if !valid_sid_text(&text) {
        return Err("current Windows user SID text was not canonical".to_string());
    }
    Ok(CurrentUserSid {
        storage: sid_storage,
        text,
    })
}

#[cfg(any(windows, test))]
fn valid_sid_text(text: &str) -> bool {
    let Some(authorities) = text.strip_prefix("S-1-") else {
        return false;
    };
    text.len() <= 184
        && !authorities.is_empty()
        && authorities.split('-').all(|authority| {
            !authority.is_empty() && authority.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(windows)]
pub(crate) fn verify_private_file(path: &Path, file: &fs::File) -> Result<()> {
    use std::os::windows::io::AsRawHandle as _;

    verify_private_handle(path, file.as_raw_handle().cast())
}

#[cfg(windows)]
pub(crate) fn verify_private_directory(path: &Path) -> Result<()> {
    use std::os::windows::{fs::OpenOptionsExt as _, io::AsRawHandle as _};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    let directory = options
        .open(path)
        .map_err(|error| PresentError::io(path, error))?;
    let metadata = directory
        .metadata()
        .map_err(|error| PresentError::io(path, error))?;
    if !metadata.is_dir() || is_link_like(&metadata) {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    verify_private_handle(path, directory.as_raw_handle().cast())
}

#[cfg(windows)]
#[allow(clippy::too_many_lines)] // One linear descriptor walk keeps every FFI pointer under its owner.
fn verify_private_handle(
    path: &Path,
    handle: windows_sys::Win32::Foundation::HANDLE,
) -> Result<()> {
    use windows_sys::Win32::{
        Foundation::{LocalFree, GENERIC_ALL},
        Security::{
            AclSizeInformation,
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            EqualSid, GetAce, GetAclInformation, GetLengthSid, GetSecurityDescriptorControl,
            IsValidSid, ACCESS_ALLOWED_ACE, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION,
            INHERITED_ACE, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
        System::SystemServices::ACCESS_ALLOWED_ACE_TYPE,
    };

    struct Descriptor(PSECURITY_DESCRIPTOR);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            unsafe { LocalFree(self.0) };
        }
    }

    let mut owner = std::ptr::null_mut();
    let mut dacl = std::ptr::null_mut();
    let mut raw_descriptor = std::ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &raw mut owner,
            std::ptr::null_mut(),
            &raw mut dacl,
            std::ptr::null_mut(),
            &raw mut raw_descriptor,
        )
    };
    if status != 0 || raw_descriptor.is_null() {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    let descriptor = Descriptor(raw_descriptor);
    let current = current_user_sid()?;
    let owner_is_current = !owner.is_null()
        && unsafe { IsValidSid(owner) } != 0
        && unsafe { EqualSid(owner, current.as_ptr()) } != 0;
    if dacl.is_null() {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    let mut control = 0_u16;
    let mut revision = 0_u32;
    if unsafe { GetSecurityDescriptorControl(descriptor.0, &raw mut control, &raw mut revision) }
        == 0
    {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    let dacl_is_protected = control & SE_DACL_PROTECTED != 0;
    let mut information = ACL_SIZE_INFORMATION::default();
    if unsafe {
        GetAclInformation(
            dacl,
            (&raw mut information).cast(),
            u32::try_from(std::mem::size_of::<ACL_SIZE_INFORMATION>())
                .expect("ACL information size fits u32"),
            AclSizeInformation,
        )
    } == 0
        || information.AceCount == 0
        || information.AceCount > 64
        || information.AclBytesInUse > 64 * 1024
    {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }

    let mut evidence = Vec::with_capacity(information.AceCount as usize);
    for index in 0..information.AceCount {
        let mut raw_ace = std::ptr::null_mut();
        if unsafe { GetAce(dacl, index, &raw mut raw_ace) } == 0 || raw_ace.is_null() {
            return Err(PresentError::UnsafePath(path.to_path_buf()));
        }
        let ace = unsafe { &*raw_ace.cast::<ACCESS_ALLOWED_ACE>() };
        if u32::from(ace.Header.AceType) != ACCESS_ALLOWED_ACE_TYPE
            || usize::from(ace.Header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
        {
            return Err(PresentError::UnsafePath(path.to_path_buf()));
        }
        let trustee = std::ptr::addr_of!(ace.SidStart).cast_mut().cast();
        if unsafe { IsValidSid(trustee) } == 0 {
            return Err(PresentError::UnsafePath(path.to_path_buf()));
        }
        let trustee_length = unsafe { GetLengthSid(trustee) };
        let sid_offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        if trustee_length == 0
            || sid_offset
                .checked_add(
                    usize::try_from(trustee_length)
                        .map_err(|_| PresentError::UnsafePath(path.to_path_buf()))?,
                )
                .is_none_or(|end| end > usize::from(ace.Header.AceSize))
        {
            return Err(PresentError::UnsafePath(path.to_path_buf()));
        }
        evidence.push(PrivateAceEvidence {
            allowed: true,
            inherited: ace.Header.AceFlags & u8::try_from(INHERITED_ACE).expect("ACE flag fits u8")
                != 0,
            current_user: unsafe { EqualSid(trustee, current.as_ptr()) } != 0,
            mask: ace.Mask,
        });
    }
    if !private_acl_evidence_is_acceptable(
        owner_is_current,
        dacl_is_protected,
        &evidence,
        FILE_ALL_ACCESS,
        GENERIC_ALL,
    ) {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy)]
struct PrivateAceEvidence {
    allowed: bool,
    inherited: bool,
    current_user: bool,
    mask: u32,
}

#[cfg(any(windows, test))]
fn private_acl_evidence_is_acceptable(
    owner_is_current: bool,
    dacl_is_protected: bool,
    aces: &[PrivateAceEvidence],
    full_access: u32,
    generic_all: u32,
) -> bool {
    if !owner_is_current || !dacl_is_protected || aces.is_empty() {
        return false;
    }
    let mut allowed_mask = 0_u32;
    for ace in aces {
        if !ace.allowed || ace.inherited || !ace.current_user {
            return false;
        }
        allowed_mask |= ace.mask;
    }
    allowed_mask & full_access == full_access || allowed_mask & generic_all != 0
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
    let status = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &raw mut raw) };
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

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn harden_private_path(_path: &Path, _directory: bool) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_sid_text_is_account_name_independent_and_strict() {
        assert!(valid_sid_text("S-1-5-21-123-456-789-1001"));
        assert!(!valid_sid_text("Jürgen レビュー,S-1-5-21-123"));
        assert!(!valid_sid_text("S-1-5-21 & whoami"));
        assert!(!valid_sid_text("S-1-S-21"));
        assert!(!valid_sid_text("S-1-5--21"));
    }

    #[test]
    fn restricted_system_tool_command_drops_provider_canaries() {
        let mut command = Command::new("icacls.exe");
        command
            .env("AWS_SECRET_ACCESS_KEY", "aws-canary")
            .env("OPENAI_API_KEY", "openai-canary")
            .env("ANTHROPIC_API_KEY", "anthropic-canary");
        apply_restricted_environment(&mut command);
        assert_restricted_environment(&command);

        let command = icacls_command(
            Path::new("C:/state"),
            Path::new("C:/state/session"),
            "*S-1-5-21-1:F",
        );
        assert_restricted_environment(&command);
    }

    #[test]
    fn restricted_child_execution_drops_provider_canaries() {
        const STAGE: &str = "CF_PRESENT_SECRET_CANARY_STAGE";
        const TEST: &str = "platform::tests::restricted_child_execution_drops_provider_canaries";
        const CANARIES: &[(&str, &str)] = &[
            ("ANTHROPIC_API_KEY", "anthropic-provider-canary"),
            ("ANTHROPIC_AUTH_TOKEN", "anthropic-auth-provider-canary"),
            ("OPENAI_API_KEY", "openai-provider-canary"),
            ("AWS_SECRET_ACCESS_KEY", "aws-provider-canary"),
            ("AWS_SESSION_TOKEN", "aws-session-provider-canary"),
        ];

        match std::env::var(STAGE).as_deref() {
            Ok("inner") => {
                for (name, value) in CANARIES {
                    assert_ne!(std::env::var(name).as_deref(), Ok(*value));
                }
                println!("restricted child received no provider canary");
            }
            Ok("outer") => {
                let executable = std::env::current_exe().expect("current test executable");
                let temporary = tempfile::tempdir().expect("restricted child temporary directory");
                let temporary_root = temporary.path().to_path_buf();
                let profile = temporary.path().join("restricted-%p.profraw");
                let mut command = restricted_command(executable);
                let output = command
                    .current_dir(temporary.path())
                    // Test instrumentation still needs a task-owned sink. This
                    // is deliberately added after the production environment
                    // restriction and is never part of that allowlist.
                    .env("LLVM_PROFILE_FILE", &profile)
                    .args(["--exact", TEST, "--nocapture"])
                    .env(STAGE, "inner")
                    .output()
                    .expect("execute restricted child canary");
                assert!(
                    output.status.success(),
                    "restricted child failed\nstdout: {}\nstderr: {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(String::from_utf8_lossy(&output.stdout)
                    .contains("restricted child received no provider canary"));
                drop(temporary);
                assert!(
                    !temporary_root.exists(),
                    "restricted child temporary directory remained"
                );
            }
            _ => {
                let executable = std::env::current_exe().expect("current test executable");
                let mut command = Command::new(executable);
                command.args(["--exact", TEST, "--nocapture"]);
                command.env(STAGE, "outer");
                for (name, value) in CANARIES {
                    command.env(name, value);
                }
                let output = command.output().expect("execute outer provider canary");
                assert!(
                    output.status.success(),
                    "outer canary failed\nstdout: {}\nstderr: {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }

    #[test]
    fn presentation_children_have_one_restricted_command_constructor() {
        // Defense-in-depth source tripwire: executable child canaries remain
        // the authority for the environment actually crossing this boundary.
        let platform = compact_rust(
            include_str!("platform.rs")
                .split("#[cfg(test)]")
                .next()
                .expect("production platform source"),
        );
        assert_eq!(platform.matches("Command::new(").count(), 1);
        assert_eq!(
            compact_rust("std::process::Command \n :: new(tool)")
                .matches("Command::new(")
                .count(),
            1,
            "whitespace must not bypass the source tripwire"
        );
        for (name, source) in [
            ("browser", include_str!("browser.rs")),
            ("service", include_str!("service.rs")),
            ("state", include_str!("state.rs")),
        ] {
            let production = compact_rust(
                source
                    .split("#[cfg(test)]")
                    .next()
                    .expect("production presentation source"),
            );
            assert!(
                !production.contains("Command::new("),
                "{name} added a raw external child route"
            );
        }
    }

    fn compact_rust(source: &str) -> String {
        source
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect()
    }

    #[test]
    fn private_acl_policy_rejects_weak_owners_inheritance_and_other_trustees() {
        const FULL: u32 = 0x001f_01ff;
        const GENERIC_ALL: u32 = 0x1000_0000;
        let private = PrivateAceEvidence {
            allowed: true,
            inherited: false,
            current_user: true,
            mask: FULL,
        };
        assert!(private_acl_evidence_is_acceptable(
            true,
            true,
            &[private],
            FULL,
            GENERIC_ALL
        ));
        assert!(!private_acl_evidence_is_acceptable(
            false,
            true,
            &[private],
            FULL,
            GENERIC_ALL
        ));
        assert!(!private_acl_evidence_is_acceptable(
            true,
            false,
            &[private],
            FULL,
            GENERIC_ALL
        ));
        assert!(!private_acl_evidence_is_acceptable(
            true,
            true,
            &[PrivateAceEvidence {
                allowed: false,
                ..private
            }],
            FULL,
            GENERIC_ALL
        ));
        assert!(!private_acl_evidence_is_acceptable(
            true,
            true,
            &[PrivateAceEvidence {
                inherited: true,
                ..private
            }],
            FULL,
            GENERIC_ALL
        ));
        assert!(!private_acl_evidence_is_acceptable(
            true,
            true,
            &[PrivateAceEvidence {
                current_user: false,
                ..private
            }],
            FULL,
            GENERIC_ALL
        ));
        assert!(!private_acl_evidence_is_acceptable(
            true,
            true,
            &[PrivateAceEvidence {
                mask: 0x0002_0000,
                ..private
            }],
            FULL,
            GENERIC_ALL
        ));
    }

    fn assert_restricted_environment(command: &Command) {
        let environment = command.get_envs().collect::<Vec<_>>();
        assert!(
            !environment.is_empty(),
            "test host has no allowed runtime environment"
        );
        for secret in [
            "AWS_SECRET_ACCESS_KEY",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
        ] {
            assert!(environment.iter().all(|(name, _)| *name != secret));
        }
        assert!(environment.iter().all(|(name, _)| {
            ALLOWED_CHILD_ENVIRONMENT
                .iter()
                .any(|allowed| *name == OsStr::new(allowed))
        }));
    }
}
