use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(unix)]
use std::{
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use uuid::Uuid;

use crate::{
    error::PresentError,
    state::{create_private_dir_all, write_json_atomic, SessionStore},
    Result,
};

#[derive(Serialize)]
struct LaunchRecoveryRecord<'a> {
    schema_version: u32,
    instance_id: Uuid,
    pid: Option<u32>,
    profile_dir: &'a Path,
}

/// Launch the one currently qualified native route without touching the
/// operator's browser profile or active window.
pub fn launch_isolated(
    store: &SessionStore,
    session_id: Uuid,
    bootstrap_path: &Path,
    profile_dir: &Path,
) -> Result<()> {
    verify_runtime_descendant(store.runtime_root(), bootstrap_path)?;
    let app_url = file_url(bootstrap_path)?;
    launch_url(store, session_id, &app_url, profile_dir)
}

pub fn launch_application(
    store: &SessionStore,
    session_id: Uuid,
    authority: &str,
    profile_dir: &Path,
) -> Result<()> {
    if !authority.starts_with("127.0.0.1:")
        || !authority
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b':'))
    {
        return Err(PresentError::BrowserUnavailable(
            "stored presentation authority is invalid".to_string(),
        ));
    }
    launch_url(
        store,
        session_id,
        &format!("http://{authority}/app/"),
        profile_dir,
    )
}

fn launch_url(
    store: &SessionStore,
    session_id: Uuid,
    app_url: &str,
    profile_dir: &Path,
) -> Result<()> {
    verify_runtime_descendant(store.runtime_root(), profile_dir)?;
    let executable = qualified_browser()?;

    let instance_id = Uuid::new_v4();
    let recovery_path = launch_recovery_path(profile_dir)?;
    write_launch_recovery(
        store,
        session_id,
        &recovery_path,
        instance_id,
        None,
        profile_dir,
    )?;
    let mut command = browser_launch_command(&executable);
    command
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg(format!("--cf-present-instance={instance_id}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-sync")
        .arg("--disable-default-apps")
        .arg("--disable-extensions")
        .arg("--disk-cache-size=33554432")
        .arg("--media-cache-size=8388608")
        .arg(format!("--app={app_url}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = remove_launch_recovery(store, session_id, &recovery_path);
            return Err(PresentError::io(executable, error));
        }
    };
    if let Err(error) = write_launch_recovery(
        store,
        session_id,
        &recovery_path,
        instance_id,
        Some(child.id()),
        profile_dir,
    ) {
        return match terminate_qualified_process(child.id(), instance_id, profile_dir) {
            Ok(()) => {
                let _ = child.wait();
                let _ = remove_launch_recovery(store, session_id, &recovery_path);
                Err(error)
            }
            Err(cleanup) => Err(PresentError::BrowserUnavailable(format!(
                "browser launch recovery could not record pid {}: {error}; rollback was not proven: {cleanup}. Marker-bound recovery evidence remains at {}",
                child.id(),
                recovery_path.display()
            ))),
        };
    }
    if let Err(error) = store.set_browser(session_id, child.id(), instance_id) {
        match terminate_qualified_process(child.id(), instance_id, profile_dir) {
            Ok(()) => {
                let _ = child.wait();
                if profile_dir.exists() {
                    fs::remove_dir_all(profile_dir)
                        .map_err(|cleanup| PresentError::io(profile_dir, cleanup))?;
                }
                remove_launch_recovery(store, session_id, &recovery_path)?;
            }
            Err(cleanup) => {
                return Err(PresentError::BrowserUnavailable(format!(
                    "browser registration failed: {error}; launch rollback could not prove cleanup for pid {}, instance {instance_id}, profile {}: {cleanup}. Recovery evidence was retained at {}",
                    child.id(),
                    profile_dir.display(),
                    recovery_path.display()
                )));
            }
        }
        return Err(error);
    }
    remove_launch_recovery(store, session_id, &recovery_path)?;
    Ok(())
}

fn launch_recovery_path(profile_dir: &Path) -> Result<PathBuf> {
    let session = profile_dir
        .parent()
        .ok_or_else(|| PresentError::UnsafePath(profile_dir.to_path_buf()))?;
    let control = session.join("control");
    create_private_dir_all(&control)?;
    Ok(control.join("launch-recovery.json"))
}

fn write_launch_recovery(
    store: &SessionStore,
    session_id: Uuid,
    path: &Path,
    instance_id: Uuid,
    pid: Option<u32>,
    profile_dir: &Path,
) -> Result<()> {
    let record = LaunchRecoveryRecord {
        schema_version: 1,
        instance_id,
        pid,
        profile_dir,
    };
    let bytes = serde_json::to_vec_pretty(&record)?
        .len()
        .checked_add(1)
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or_else(|| PresentError::CorruptState("runtime recovery size overflow".to_string()))?;
    let _lease = store.prepare_runtime_control_mutation(session_id, bytes)?;
    write_json_atomic(path, &record)
}

fn remove_launch_recovery(store: &SessionStore, session_id: Uuid, path: &Path) -> Result<()> {
    let _lease = store.prepare_runtime_control_mutation(session_id, 0)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !crate::platform::is_link_like(&metadata) => {
            fs::remove_file(path).map_err(|error| PresentError::io(path, error))
        }
        Ok(_) => Err(PresentError::UnsafePath(path.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PresentError::io(path, error)),
    }
}

fn browser_launch_command(executable: &Path) -> Command {
    crate::platform::restricted_command(executable)
}

pub fn is_isolated_running(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<bool> {
    qualified_process_identity(pid, instance_id, profile_dir)
}

pub fn terminate_isolated(
    store: &SessionStore,
    session_id: Uuid,
    pid: u32,
    instance_id: Uuid,
    profile_dir: &Path,
) -> Result<()> {
    verify_runtime_descendant(store.runtime_root(), profile_dir)?;
    if profile_dir.file_name().and_then(|name| name.to_str()) != Some("browser-profile") {
        return Err(PresentError::UnsafePath(profile_dir.to_path_buf()));
    }
    terminate_qualified_process(pid, instance_id, profile_dir)?;
    if profile_dir.exists() {
        fs::remove_dir_all(profile_dir).map_err(|error| PresentError::io(profile_dir, error))?;
    }
    let _ = store.clear_browser(session_id, instance_id)?;
    Ok(())
}

fn verify_runtime_descendant(root: &Path, path: &Path) -> Result<()> {
    verify_owned_child(root, path)
}

#[cfg(unix)]
fn terminate_qualified_process(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<()> {
    if !qualified_process_identity(pid, instance_id, profile_dir)? {
        return Ok(());
    }
    let pid = i32::try_from(pid).map_err(|_| {
        PresentError::BrowserUnavailable("browser PID is outside the platform range".to_string())
    })?;
    if unsafe { libc::kill(-pid, libc::SIGTERM) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(PresentError::io("browser process group", error));
        }
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if unsafe { libc::kill(-pid, 0) } != 0 {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    let raw_pid = u32::try_from(pid).map_err(|_| {
        PresentError::BrowserUnavailable("browser PID is outside the platform range".to_string())
    })?;
    if !qualified_process_identity(raw_pid, instance_id, profile_dir)? {
        return Ok(());
    }
    if unsafe { libc::kill(-pid, libc::SIGKILL) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(PresentError::io("browser process group", error));
        }
        return Ok(());
    }
    let kill_deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < kill_deadline {
        if process_group_is_absent(pid)? {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(PresentError::BrowserUnavailable(
        "the reverified owned presentation browser process group did not exit after SIGKILL"
            .to_string(),
    ))
}

#[cfg(target_os = "macos")]
fn qualified_process_identity(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<bool> {
    let output = macos_identity_command(pid)
        .output()
        .map_err(|error| PresentError::io("/bin/ps", error))?;
    let pid_i32 = i32::try_from(pid).map_err(|_| {
        PresentError::BrowserUnavailable("browser PID is outside the platform range".to_string())
    })?;
    if !output.status.success() || output.stdout.is_empty() {
        if !process_group_is_absent(pid_i32)? {
            return Err(PresentError::BrowserUnavailable(orphan_recovery_message(
                "browser process group exists without its verifiable leader",
            )));
        }
        return Ok(false);
    }
    if output.stdout.len() > 64 * 1024 {
        return Err(PresentError::BrowserUnavailable(
            "browser identity output exceeded its bound".to_string(),
        ));
    }
    let command = std::str::from_utf8(&output.stdout).map_err(|_| {
        PresentError::BrowserUnavailable("browser identity is not UTF-8".to_string())
    })?;
    let profile = format!("--user-data-dir={}", profile_dir.display());
    let instance = format!("--cf-present-instance={instance_id}");
    if !rendered_command_line_contains_argument(command, &profile)
        || !rendered_command_line_contains_argument(command, &instance)
    {
        return Err(PresentError::BrowserUnavailable(
            "refusing to stop a PID that does not prove this presentation identity".to_string(),
        ));
    }
    Ok(true)
}

#[cfg(any(target_os = "macos", test))]
fn macos_identity_command(pid: u32) -> Command {
    let mut command = crate::platform::restricted_command("/bin/ps");
    command.args(["-p", &pid.to_string(), "-o", "command="]);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn qualified_process_identity(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<bool> {
    let command_path = PathBuf::from(format!("/proc/{pid}/cmdline"));
    let metadata = match fs::symlink_metadata(&command_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let pid = i32::try_from(pid).map_err(|_| {
                PresentError::BrowserUnavailable(
                    "browser PID is outside the platform range".to_string(),
                )
            })?;
            if !process_group_is_absent(pid)? {
                return Err(PresentError::BrowserUnavailable(orphan_recovery_message(
                    "browser process group exists without its verifiable leader",
                )));
            }
            return Ok(false);
        }
        Err(error) => return Err(PresentError::io(command_path, error)),
    };
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Err(PresentError::BrowserUnavailable(
            "browser identity output exceeded its bound".to_string(),
        ));
    }
    use std::os::unix::fs::OpenOptionsExt as _;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&command_path)
        .map_err(|error| PresentError::io(&command_path, error))?;
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len()).expect("64-KiB process command bound fits usize"),
    );
    let mut bounded = std::io::Read::take(file, 64 * 1024 + 1);
    std::io::Read::read_to_end(&mut bounded, &mut bytes)
        .map_err(|error| PresentError::io(&command_path, error))?;
    if bytes.len() > 64 * 1024 {
        return Err(PresentError::BrowserUnavailable(
            "browser identity output exceeded its bound".to_string(),
        ));
    }
    let arguments = bytes
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(std::str::from_utf8)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| {
            PresentError::BrowserUnavailable("browser identity is not UTF-8".to_string())
        })?;
    if !arguments_prove_identity(&arguments, instance_id, profile_dir) {
        return Err(PresentError::BrowserUnavailable(
            "refusing to stop a PID that does not prove this presentation identity".to_string(),
        ));
    }
    Ok(true)
}

#[cfg(unix)]
fn process_group_is_absent(pid: i32) -> Result<bool> {
    if unsafe { libc::kill(-pid, 0) } == 0 {
        return Ok(false);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(true)
    } else {
        Err(PresentError::io("browser process group", error))
    }
}

#[cfg(windows)]
fn qualified_process_identity(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<bool> {
    let marker = format!("--cf-present-instance={instance_id}");
    let profile = format!("--user-data-dir={}", profile_dir.display());
    let ps_marker = marker.replace('\'', "''");
    let ps_profile = profile.replace('\'', "''");
    let script = format!(
        "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); $p=Get-CimInstance Win32_Process -Filter 'ProcessId={pid}' -ErrorAction Stop; if($null -eq $p){{$all=@(Get-CimInstance Win32_Process -ErrorAction Stop); if(@($all | Where-Object {{($_.Name -in @('chrome.exe','msedge.exe','chromium.exe')) -and $null -eq $_.CommandLine}}).Count -gt 0){{exit 5}}; $owned=@($all | Where-Object {{$null -ne $_.CommandLine -and $_.CommandLine.Contains('{ps_marker}') -and $_.CommandLine.Contains('{ps_profile}')}}); if($owned.Count -gt 0){{exit 3}}; exit 4}}; [Console]::Out.Write($p.CommandLine)"
    );
    let powershell = crate::platform::trusted_system_path("WindowsPowerShell/v1.0/powershell.exe")?;
    let output = windows_identity_command(&powershell, &script)
        .output()
        .map_err(|error| PresentError::io(&powershell, error))?;
    if output.status.code() == Some(3) {
        return Err(PresentError::BrowserUnavailable(orphan_recovery_message(
            "browser leader is absent, so Windows process-tree cleanup cannot be proven",
        )));
    }
    if output.status.code() == Some(4) {
        if windows_profile_lock_released(profile_dir)? {
            return Ok(false);
        }
        return Err(PresentError::BrowserUnavailable(orphan_recovery_message(
            "browser leader and marker-matched processes are absent, but the profile remains locked",
        )));
    }
    if !output.status.success() || output.stdout.len() > 64 * 1024 {
        return Err(PresentError::BrowserUnavailable(
            "browser identity query failed or exceeded its bound".to_string(),
        ));
    }
    let command = String::from_utf8(output.stdout).map_err(|_| {
        PresentError::BrowserUnavailable("browser identity is not UTF-8".to_string())
    })?;
    let arguments = windows_command_line_arguments(&command)?;
    let argument_refs = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    if !arguments_prove_identity(&argument_refs, instance_id, profile_dir) {
        return Err(PresentError::BrowserUnavailable(
            "refusing to stop a PID that does not prove this presentation identity".to_string(),
        ));
    }
    Ok(true)
}

#[cfg(windows)]
fn windows_profile_lock_released(profile_dir: &Path) -> Result<bool> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::{
        Foundation::ERROR_SHARING_VIOLATION, Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
    };

    let lock = profile_dir.join("SingletonLock");
    let metadata = match fs::symlink_metadata(&lock) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(PresentError::io(&lock, error)),
    };
    if !metadata.is_file() || crate::platform::is_link_like(&metadata) {
        return Err(PresentError::UnsafePath(lock));
    }
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .write(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    match options.open(&lock) {
        Ok(_) => Ok(true),
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error()
                    == Some(
                        i32::try_from(ERROR_SHARING_VIOLATION).expect("error code fits i32"),
                    ) =>
        {
            Ok(false)
        }
        Err(error) => Err(PresentError::io(&lock, error)),
    }
}

fn orphan_recovery_message(reason: &str) -> String {
    format!(
        "{reason}; no unproven process will be signalled and recovery state was retained. Wait for the process group or tree to exit, or inspect and terminate it through an operator-controlled OS tool, then retry `codeflow present close`"
    )
}

#[cfg(any(windows, test))]
fn windows_identity_command(executable: &Path, script: &str) -> Command {
    let mut command = crate::platform::restricted_command(executable);
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        script,
    ]);
    command
}

#[cfg(windows)]
fn windows_command_line_arguments(command: &str) -> Result<Vec<String>> {
    use windows_sys::Win32::{Foundation::LocalFree, UI::Shell::CommandLineToArgvW};

    if command.encode_utf16().count() > 32_767 || command.contains('\0') {
        return Err(PresentError::BrowserUnavailable(
            "browser command line is invalid or exceeded the Windows bound".to_string(),
        ));
    }
    let wide = command
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut count = 0_i32;
    let argv = unsafe { CommandLineToArgvW(wide.as_ptr(), &raw mut count) };
    if argv.is_null() || !(1..=1_024).contains(&count) {
        if !argv.is_null() {
            unsafe { LocalFree(argv.cast()) };
        }
        return Err(PresentError::BrowserUnavailable(
            "browser command line could not be parsed safely".to_string(),
        ));
    }

    let parsed = (|| {
        let count = usize::try_from(count).expect("positive bounded Windows argument count");
        let pointers = unsafe { std::slice::from_raw_parts(argv, count) };
        let mut arguments = Vec::with_capacity(pointers.len());
        for pointer in pointers {
            if pointer.is_null() {
                return Err(PresentError::BrowserUnavailable(
                    "browser command line contained an invalid argument".to_string(),
                ));
            }
            let mut length = 0_usize;
            while length <= 32_767 && unsafe { *pointer.add(length) } != 0 {
                length += 1;
            }
            if length > 32_767 {
                return Err(PresentError::BrowserUnavailable(
                    "browser command-line argument exceeded the Windows bound".to_string(),
                ));
            }
            arguments.push(
                String::from_utf16(unsafe { std::slice::from_raw_parts(*pointer, length) })
                    .map_err(|_| {
                        PresentError::BrowserUnavailable(
                            "browser command line contained invalid UTF-16".to_string(),
                        )
                    })?,
            );
        }
        Ok(arguments)
    })();
    unsafe { LocalFree(argv.cast()) };
    parsed
}

#[cfg(windows)]
fn terminate_qualified_process(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<()> {
    if !qualified_process_identity(pid, instance_id, profile_dir)? {
        return Ok(());
    }
    let taskkill = crate::platform::trusted_system_path("taskkill.exe")?;
    let output = windows_terminate_command(&taskkill, pid)
        .output()
        .map_err(|error| PresentError::io(&taskkill, error))?;
    if !output.status.success() || output.stdout.len() + output.stderr.len() > 64 * 1024 {
        return Err(PresentError::BrowserUnavailable(
            "the owned presentation browser process tree did not terminate".to_string(),
        ));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn windows_terminate_command(executable: &Path, pid: u32) -> Command {
    let mut command = crate::platform::restricted_command(executable);
    command.args(["/PID", &pid.to_string(), "/T", "/F"]);
    command
}

fn qualified_browser() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let chrome = PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
        if chrome.is_file() {
            return Ok(chrome);
        }
        Err(PresentError::BrowserUnavailable(
            "the qualified macOS Google Chrome route is unavailable; use --no-launch".to_string(),
        ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        qualified_browser_non_macos()
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn qualified_browser_non_macos() -> Result<PathBuf> {
    for path in [
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/snap/bin/chromium",
    ] {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(PresentError::BrowserUnavailable(
        "no qualified Linux/WSL Chrome or Chromium executable was found; use --no-launch"
            .to_string(),
    ))
}

#[cfg(windows)]
fn qualified_browser_non_macos() -> Result<PathBuf> {
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_LocalAppData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86,
    };

    let roots = [
        crate::platform::known_folder(&FOLDERID_ProgramFiles),
        crate::platform::known_folder(&FOLDERID_ProgramFilesX86),
        crate::platform::known_folder(&FOLDERID_LocalAppData),
    ];
    for root in roots.into_iter().flatten() {
        for relative in [
            "Google/Chrome/Application/chrome.exe",
            "Microsoft/Edge/Application/msedge.exe",
        ] {
            if let Ok(candidate) = crate::platform::trusted_descendant_file(&root, relative) {
                return Ok(candidate);
            }
        }
    }
    Err(PresentError::BrowserUnavailable(
        "no qualified native-Windows Chrome or Edge executable was found; use --no-launch"
            .to_string(),
    ))
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
    Ok(encode_file_path(value, cfg!(windows)))
}

fn encode_file_path(value: &str, windows: bool) -> String {
    let mut encoded = String::from("file://");
    if !value.starts_with('/') {
        encoded.push('/');
    }
    for byte in value.as_bytes() {
        if windows && *byte == b'\\' {
            encoded.push('/');
        } else if byte.is_ascii_alphanumeric()
            || matches!(*byte, b'/' | b'-' | b'_' | b'.' | b'~' | b':')
        {
            encoded.push(char::from(*byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    encoded
}

#[cfg(any(all(unix, not(target_os = "macos")), windows, test))]
fn arguments_prove_identity(arguments: &[&str], instance_id: Uuid, profile_dir: &Path) -> bool {
    let profile = format!("--user-data-dir={}", profile_dir.display());
    let instance = format!("--cf-present-instance={instance_id}");
    arguments.iter().any(|argument| *argument == profile)
        && arguments.iter().any(|argument| *argument == instance)
}

#[cfg(any(target_os = "macos", test))]
fn rendered_command_line_contains_argument(command: &str, expected: &str) -> bool {
    command.match_indices(expected).any(|(start, value)| {
        let before = command[..start].chars().next_back();
        let after = command[start + value.len()..].chars().next();
        before.is_none_or(char::is_whitespace) && after.is_none_or(char::is_whitespace)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_url_encodes_spaces_and_unicode_bytes() {
        let url = file_url(std::path::Path::new("/tmp/review ü.html")).unwrap();
        assert_eq!(url, "file:///tmp/review%20%C3%BC.html");
        assert_eq!(
            encode_file_path(r"C:\Users\Jürgen\review.html", true),
            "file:///C:/Users/J%C3%BCrgen/review.html"
        );
    }

    #[test]
    fn platform_command_line_identity_requires_exact_profile_and_instance_arguments() {
        let instance = Uuid::new_v4();
        let profile = Path::new("/state/browser-profile");
        let profile_argument = format!("--user-data-dir={}", profile.display());
        let instance_argument = format!("--cf-present-instance={instance}");
        assert!(arguments_prove_identity(
            &["chrome", &profile_argument, &instance_argument],
            instance,
            profile
        ));
        assert!(!arguments_prove_identity(
            &[
                "chrome",
                &format!("{profile_argument}-other"),
                &instance_argument
            ],
            instance,
            profile
        ));

        let unicode_profile = Path::new("C:/Users/Jürgen Name/レビュー/browser-profile");
        let unicode_argument = format!("--user-data-dir={}", unicode_profile.display());
        assert!(arguments_prove_identity(
            &["chrome", &unicode_argument, &instance_argument],
            instance,
            unicode_profile
        ));
        assert!(!arguments_prove_identity(
            &[
                "chrome",
                &format!("x{unicode_argument}"),
                &instance_argument
            ],
            instance,
            unicode_profile
        ));
    }

    #[test]
    fn rendered_command_line_identity_rejects_prefix_and_suffix_confusion() {
        assert!(rendered_command_line_contains_argument(
            "chrome --user-data-dir=/tmp/profile --flag",
            "--user-data-dir=/tmp/profile"
        ));
        assert!(!rendered_command_line_contains_argument(
            "chrome --user-data-dir=/tmp/profile-other --flag",
            "--user-data-dir=/tmp/profile"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn orphan_process_group_retains_recovery_state_when_leader_is_absent() {
        use std::os::unix::process::CommandExt as _;

        let mut command = crate::platform::restricted_command("/bin/sh");
        command
            .args(["-c", "sleep 10 &"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut leader = command.spawn().unwrap();
        let pid = leader.id();
        leader.wait().unwrap();
        let pid_i32 = i32::try_from(pid).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while process_group_is_absent(pid_i32).unwrap() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let termination =
            terminate_qualified_process(pid, Uuid::new_v4(), Path::new("/state/browser-profile"));
        let group_remained = !process_group_is_absent(pid_i32).unwrap();
        unsafe { libc::kill(-pid_i32, libc::SIGKILL) };
        assert!(termination.is_err());
        assert!(group_remained);
    }

    #[cfg(windows)]
    #[test]
    fn windows_command_line_identity_parses_quoted_unicode_arguments_exactly() {
        let instance = Uuid::new_v4();
        let profile = Path::new(r"C:\Users\Jürgen Name\レビュー\browser-profile");
        let command = format!(
            r#""C:\Program Files\Google\Chrome\Application\chrome.exe" "--user-data-dir={}" "--cf-present-instance={instance}""#,
            profile.display()
        );
        let arguments = windows_command_line_arguments(&command).unwrap();
        let argument_refs = arguments.iter().map(String::as_str).collect::<Vec<_>>();
        assert!(arguments_prove_identity(&argument_refs, instance, profile));

        let suffix = command.replace("browser-profile\"", "browser-profile-other\"");
        let suffix_arguments = windows_command_line_arguments(&suffix).unwrap();
        let suffix_refs = suffix_arguments
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert!(!arguments_prove_identity(&suffix_refs, instance, profile));

        let prefix = command.replace("--cf-present-instance=", "x--cf-present-instance=");
        let prefix_arguments = windows_command_line_arguments(&prefix).unwrap();
        let prefix_refs = prefix_arguments
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert!(!arguments_prove_identity(&prefix_refs, instance, profile));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn process_group_cleanup_requires_and_terminates_the_exact_owned_identity() {
        use std::os::unix::process::CommandExt as _;

        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("browser-profile");
        fs::create_dir(&profile).unwrap();
        let instance = Uuid::new_v4();
        let mut command = crate::platform::restricted_command("/bin/sh");
        command
            .args([
                "-c",
                "trap 'exit 0' TERM; while :; do sleep 1; done",
                "cf-present-browser",
                &format!("--user-data-dir={}", profile.display()),
                &format!("--cf-present-instance={instance}"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        let waiter = thread::spawn(move || child.wait().unwrap());

        let identity_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match qualified_process_identity(pid, instance, &profile) {
                Ok(true) => break,
                result if Instant::now() < identity_deadline => {
                    let _ = result;
                    thread::sleep(Duration::from_millis(10));
                }
                result => panic!("owned process identity did not settle: {result:?}"),
            }
        }
        let wrong = Uuid::new_v4();
        assert!(qualified_process_identity(pid, wrong, &profile).is_err());
        terminate_qualified_process(pid, instance, &profile).unwrap();
        let _status = waiter.join().unwrap();
        assert!(!qualified_process_identity(pid, instance, &profile).unwrap());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn process_group_cleanup_reverifies_before_forced_escalation() {
        use std::os::unix::process::CommandExt as _;

        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("browser-profile");
        fs::create_dir(&profile).unwrap();
        let instance = Uuid::new_v4();
        let mut command = crate::platform::restricted_command("/bin/sh");
        command
            .args([
                "-c",
                "trap '' TERM; while :; do sleep 1; done",
                "cf-present-browser",
                &format!("--user-data-dir={}", profile.display()),
                &format!("--cf-present-instance={instance}"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !qualified_process_identity(pid, instance, &profile).unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }

        terminate_qualified_process(pid, instance, &profile).unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert!(process_group_is_absent(i32::try_from(pid).unwrap()).unwrap());
    }

    #[test]
    fn every_external_command_route_uses_the_restricted_environment() {
        let commands = [
            browser_launch_command(Path::new("browser")),
            macos_identity_command(42),
            windows_identity_command(
                Path::new("powershell.exe"),
                "[Console]::Out.Write('identity')",
            ),
            windows_terminate_command(Path::new("taskkill.exe"), 42),
        ];
        for command in &commands {
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
            assert!(environment.iter().any(|(name, value)| matches!(
                name.to_str(),
                Some("HOME" | "SYSTEMROOT")
            ) && value.is_some()));
        }
    }
}
