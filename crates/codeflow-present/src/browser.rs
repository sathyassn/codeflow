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

use uuid::Uuid;

use crate::{error::PresentError, state::SessionStore, Result};

/// Launch the one currently qualified native route without touching the
/// operator's browser profile or active window.
pub fn launch_isolated(
    store: &SessionStore,
    session_id: Uuid,
    bootstrap_path: &Path,
    profile_dir: &Path,
) -> Result<()> {
    verify_owned_child(store.root(), bootstrap_path)?;
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
    verify_owned_child(store.root(), profile_dir)?;
    let executable = qualified_browser()?;

    let instance_id = Uuid::new_v4();
    let mut command = Command::new(&executable);
    apply_minimal_environment(&mut command);
    command
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg(format!("--cf-present-instance={instance_id}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-sync")
        .arg("--disable-default-apps")
        .arg("--disable-extensions")
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
    let mut child = command
        .spawn()
        .map_err(|error| PresentError::io(executable, error))?;
    if let Err(error) = store.set_browser(session_id, child.id(), instance_id) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok(())
}

fn apply_minimal_environment(command: &mut Command) {
    const ALLOWED: &[&str] = &[
        "HOME",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "SYSTEMROOT",
        "WINDIR",
    ];
    command.env_clear();
    for name in ALLOWED {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
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
    verify_owned_child(store.root(), profile_dir)?;
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
    Err(PresentError::BrowserUnavailable(
        "the owned presentation browser process group did not exit after SIGTERM".to_string(),
    ))
}

#[cfg(target_os = "macos")]
fn qualified_process_identity(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<bool> {
    let output = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .map_err(|error| PresentError::io("/bin/ps", error))?;
    let pid_i32 = i32::try_from(pid).map_err(|_| {
        PresentError::BrowserUnavailable("browser PID is outside the platform range".to_string())
    })?;
    if !output.status.success() || output.stdout.is_empty() {
        if unsafe { libc::kill(-pid_i32, 0) } == 0 {
            return Err(PresentError::BrowserUnavailable(
                "browser process group exists without its verifiable leader".to_string(),
            ));
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

#[cfg(all(unix, not(target_os = "macos")))]
fn qualified_process_identity(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<bool> {
    let command_path = PathBuf::from(format!("/proc/{pid}/cmdline"));
    let metadata = match fs::symlink_metadata(&command_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
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

#[cfg(windows)]
fn qualified_process_identity(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<bool> {
    let script = format!(
        "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); $p=Get-CimInstance Win32_Process -Filter 'ProcessId={pid}' -ErrorAction SilentlyContinue; if($null -eq $p){{exit 3}}; [Console]::Out.Write($p.CommandLine)"
    );
    let powershell = crate::platform::trusted_system_path("WindowsPowerShell/v1.0/powershell.exe")?;
    let output = Command::new(&powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ])
        .output()
        .map_err(|error| PresentError::io(&powershell, error))?;
    if output.status.code() == Some(3) {
        return Ok(false);
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
        let pointers = unsafe { std::slice::from_raw_parts(argv, count as usize) };
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
    let output = Command::new(&taskkill)
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output()
        .map_err(|error| PresentError::io(&taskkill, error))?;
    if !output.status.success() || output.stdout.len() + output.stderr.len() > 64 * 1024 {
        return Err(PresentError::BrowserUnavailable(
            "the owned presentation browser process tree did not terminate".to_string(),
        ));
    }
    Ok(())
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
        let mut command = Command::new("/bin/sh");
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

    #[test]
    fn browser_environment_excludes_provider_secrets() {
        let mut command = Command::new("browser");
        command.env("OPENAI_API_KEY", "sentinel");
        apply_minimal_environment(&mut command);
        let environment = command.get_envs().collect::<Vec<_>>();
        assert!(!environment.iter().any(|(name, value)| {
            *name == "OPENAI_API_KEY" && value.and_then(|value| value.to_str()) == Some("sentinel")
        }));
    }
}
