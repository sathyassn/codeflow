use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use std::{
    thread,
    time::{Duration, Instant},
};

#[cfg(any(windows, test))]
use std::collections::BTreeSet;

#[cfg(all(unix, not(target_os = "macos")))]
use std::os::unix::fs::OpenOptionsExt as _;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::PresentError,
    state::{
        create_private_dir_all, parse_launch_recovery_name, write_json_atomic, SessionStore,
        LAUNCH_RECOVERY_PREFIX,
    },
    Result,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LaunchRecoveryRecord {
    schema_version: u32,
    session_id: Uuid,
    instance_id: Uuid,
    pid: Option<u32>,
    profile_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessIdentity {
    Owned,
    Absent,
    Reused,
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

/// The `file:` link to an owner-private bootstrap page, for an operator to
/// open when `codeflow` does not launch the browser itself (for example from an
/// agent sandbox). The page is single-use and expires with the bootstrap TTL.
pub fn handoff_link(store: &SessionStore, bootstrap_path: &Path) -> Result<String> {
    verify_runtime_descendant(store.runtime_root(), bootstrap_path)?;
    file_url(bootstrap_path)
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
    verify_profile_path(store.runtime_root(), profile_dir)?;
    let _launch_lease = store.acquire_browser_launch_lease(session_id)?;
    recover_launch_records_locked(store, session_id, profile_dir)?;
    if store.load(session_id)?.browser_instance.is_some() {
        return Err(PresentError::BrowserAlreadyOpen(session_id.to_string()));
    }
    create_private_dir_all(profile_dir)?;
    let executable = qualified_browser()?;

    let instance_id = Uuid::new_v4();
    let recovery_path = launch_recovery_path(profile_dir, instance_id)?;
    if recovery_path
        .try_exists()
        .map_err(|error| PresentError::io(&recovery_path, error))?
    {
        return Err(PresentError::CorruptState(format!(
            "browser launch recovery record already exists at {}",
            recovery_path.display()
        )));
    }
    write_launch_recovery(
        store,
        session_id,
        &recovery_path,
        instance_id,
        None,
        profile_dir,
    )?;
    let mut command =
        configured_browser_launch_command(&executable, app_url, profile_dir, instance_id);
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

fn launch_recovery_path(profile_dir: &Path, instance_id: Uuid) -> Result<PathBuf> {
    let session = profile_dir
        .parent()
        .ok_or_else(|| PresentError::UnsafePath(profile_dir.to_path_buf()))?;
    let control = session.join("control");
    create_private_dir_all(&control)?;
    Ok(control.join(format!(
        "{LAUNCH_RECOVERY_PREFIX}{}.json",
        instance_id.simple()
    )))
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
        session_id,
        instance_id,
        pid,
        profile_dir: profile_dir.to_path_buf(),
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

/// Reconcile any launch transaction that did not reach durable browser
/// registration. The session-scoped launch lease prevents a retry from
/// replacing the only evidence for an earlier attempt.
pub fn recover_incomplete_launch(
    store: &SessionStore,
    session_id: Uuid,
    profile_dir: &Path,
) -> Result<()> {
    verify_profile_path(store.runtime_root(), profile_dir)?;
    let _launch_lease = store.acquire_browser_launch_lease(session_id)?;
    recover_launch_records_locked(store, session_id, profile_dir)
}

fn recover_launch_records_locked(
    store: &SessionStore,
    session_id: Uuid,
    profile_dir: &Path,
) -> Result<()> {
    if let Some((path, record)) = launch_recovery_record(profile_dir, session_id)? {
        let session = store.load(session_id)?;
        if session.browser_instance == Some(record.instance_id)
            && session.browser_pid == record.pid
            && record.pid.is_some()
        {
            return remove_launch_recovery(store, session_id, &path);
        }

        terminate_recovery_record(&record)?;
        if record.profile_dir.exists() {
            fs::remove_dir_all(&record.profile_dir)
                .map_err(|error| PresentError::io(&record.profile_dir, error))?;
        }
        if session.browser_instance == Some(record.instance_id) {
            let _ = store.clear_browser(session_id, record.instance_id)?;
        }
        remove_launch_recovery(store, session_id, &path)?;
    }
    Ok(())
}

fn launch_recovery_record(
    profile_dir: &Path,
    session_id: Uuid,
) -> Result<Option<(PathBuf, LaunchRecoveryRecord)>> {
    let control = profile_dir
        .parent()
        .ok_or_else(|| PresentError::UnsafePath(profile_dir.to_path_buf()))?
        .join("control");
    let entries = match fs::read_dir(&control) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PresentError::io(&control, error)),
    };
    let mut found = None;
    for entry in entries {
        let entry = entry.map_err(|error| PresentError::io(&control, error))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(instance_id) = parse_launch_recovery_name(&name, &entry.path())? else {
            continue;
        };
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| PresentError::io(entry.path(), error))?;
        if !metadata.is_file()
            || crate::platform::is_link_like(&metadata)
            || metadata.len() > crate::limits::MAX_RUNTIME_CONTROL_BYTES
        {
            return Err(PresentError::UnsafePath(entry.path()));
        }
        let bytes =
            fs::read(entry.path()).map_err(|error| PresentError::io(entry.path(), error))?;
        let record: LaunchRecoveryRecord = serde_json::from_slice(&bytes)?;
        if record.schema_version != 1
            || record.session_id != session_id
            || record.instance_id != instance_id
            || record.profile_dir != profile_dir
        {
            return Err(PresentError::CorruptState(format!(
                "browser launch recovery record {} does not match its session, name, or profile",
                entry.path().display()
            )));
        }
        if found.replace((entry.path(), record)).is_some() {
            return Err(PresentError::CorruptState(
                "multiple browser launch recovery records exist for one serialized session"
                    .to_string(),
            ));
        }
    }
    Ok(found)
}

fn terminate_recovery_record(record: &LaunchRecoveryRecord) -> Result<()> {
    let mut candidates = owned_process_candidates(record.instance_id, &record.profile_dir)?;
    if let Some(pid) = record.pid {
        match process_identity(pid, record.instance_id, &record.profile_dir)? {
            ProcessIdentity::Owned if !candidates.contains(&pid) => candidates.push(pid),
            ProcessIdentity::Owned | ProcessIdentity::Absent | ProcessIdentity::Reused => {}
        }
    }
    candidates.sort_unstable();
    candidates.dedup();
    for pid in candidates {
        terminate_qualified_process(pid, record.instance_id, &record.profile_dir)?;
    }
    if !owned_process_candidates(record.instance_id, &record.profile_dir)?.is_empty()
        || !profile_resources_released(&record.profile_dir)?
    {
        return Err(PresentError::BrowserUnavailable(orphan_recovery_message(
            "the exact launch attempt still owns a process or profile resource",
        )));
    }
    Ok(())
}

fn browser_launch_command(executable: &Path) -> Command {
    crate::platform::restricted_command(executable)
}

fn configured_browser_launch_command(
    executable: &Path,
    app_url: &str,
    profile_dir: &Path,
    instance_id: Uuid,
) -> Command {
    let mut command = browser_launch_command(executable);
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
    command
}

pub fn is_isolated_running(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<bool> {
    match process_identity(pid, instance_id, profile_dir)? {
        ProcessIdentity::Owned => Ok(true),
        ProcessIdentity::Absent | ProcessIdentity::Reused => {
            if owned_process_candidates(instance_id, profile_dir)?.is_empty()
                && profile_resources_released(profile_dir)?
            {
                Ok(false)
            } else {
                Err(PresentError::BrowserUnavailable(orphan_recovery_message(
                    "the recorded browser PID is absent or reused, but exact owned resources remain",
                )))
            }
        }
    }
}

pub fn terminate_isolated(
    store: &SessionStore,
    session_id: Uuid,
    pid: u32,
    instance_id: Uuid,
    profile_dir: &Path,
) -> Result<()> {
    verify_profile_path(store.runtime_root(), profile_dir)?;
    let _launch_lease = store.acquire_browser_launch_lease(session_id)?;
    terminate_isolated_locked(store, session_id, pid, instance_id, profile_dir)
}

fn terminate_isolated_locked(
    store: &SessionStore,
    session_id: Uuid,
    pid: u32,
    instance_id: Uuid,
    profile_dir: &Path,
) -> Result<()> {
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

fn verify_profile_path(root: &Path, profile_dir: &Path) -> Result<()> {
    if profile_dir.file_name().and_then(|name| name.to_str()) != Some("browser-profile") {
        return Err(PresentError::UnsafePath(profile_dir.to_path_buf()));
    }
    let parent = profile_dir
        .parent()
        .ok_or_else(|| PresentError::UnsafePath(profile_dir.to_path_buf()))?;
    verify_owned_child(root, parent)?;
    if profile_dir.exists() {
        verify_owned_child(root, profile_dir)?;
    }
    Ok(())
}

#[cfg(unix)]
fn terminate_qualified_process(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<()> {
    match process_identity(pid, instance_id, profile_dir)? {
        ProcessIdentity::Owned => {}
        ProcessIdentity::Absent | ProcessIdentity::Reused => {
            if owned_process_candidates(instance_id, profile_dir)?.is_empty()
                && profile_resources_released(profile_dir)?
            {
                return Ok(());
            }
            return Err(PresentError::BrowserUnavailable(orphan_recovery_message(
                "the recorded browser PID is absent or reused, but exact owned resources remain",
            )));
        }
    }
    let pid = i32::try_from(pid).map_err(|_| {
        PresentError::BrowserUnavailable("browser PID is outside the platform range".to_string())
    })?;
    if unsafe { libc::kill(-pid, libc::SIGTERM) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(PresentError::io(
                "browser process group graceful signal",
                error,
            ));
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
    if process_identity(raw_pid, instance_id, profile_dir)? != ProcessIdentity::Owned {
        if owned_process_candidates(instance_id, profile_dir)?.is_empty() {
            return Ok(());
        }
        return Err(PresentError::BrowserUnavailable(orphan_recovery_message(
            "browser ownership changed before forced escalation",
        )));
    }
    if unsafe { libc::kill(-pid, libc::SIGKILL) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(PresentError::io(
                "browser process group forced signal",
                error,
            ));
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
fn process_identity(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<ProcessIdentity> {
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
        return Ok(ProcessIdentity::Absent);
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
        return Ok(ProcessIdentity::Reused);
    }
    Ok(ProcessIdentity::Owned)
}

#[cfg(any(target_os = "macos", test))]
fn macos_identity_command(pid: u32) -> Command {
    let mut command = crate::platform::restricted_command("/bin/ps");
    command.args(["-p", &pid.to_string(), "-o", "command="]);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn process_identity(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<ProcessIdentity> {
    process_identity_from_proc(Path::new("/proc"), pid, instance_id, profile_dir)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn process_identity_from_proc(
    proc_root: &Path,
    pid: u32,
    instance_id: Uuid,
    profile_dir: &Path,
) -> Result<ProcessIdentity> {
    let command_path = proc_root.join(pid.to_string()).join("cmdline");
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
            return Ok(ProcessIdentity::Absent);
        }
        Err(error) => return Err(PresentError::io(command_path, error)),
    };
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Err(PresentError::BrowserUnavailable(
            "browser identity output exceeded its bound".to_string(),
        ));
    }
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
    let arguments = linux_command_line_arguments(&bytes)?;
    if !arguments_prove_identity(&arguments, instance_id, profile_dir) {
        return Ok(ProcessIdentity::Reused);
    }
    Ok(ProcessIdentity::Owned)
}

#[cfg(unix)]
fn process_group_is_absent(pid: i32) -> Result<bool> {
    if unsafe { libc::kill(-pid, 0) } == 0 {
        return Ok(false);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(true)
    } else if error.raw_os_error() == Some(libc::EPERM) {
        Ok(false)
    } else {
        Err(PresentError::io("browser process group probe", error))
    }
}

#[cfg(target_os = "macos")]
fn owned_process_candidates(instance_id: Uuid, profile_dir: &Path) -> Result<Vec<u32>> {
    let mut command = crate::platform::restricted_command("/bin/ps");
    command.args(["-axww", "-o", "pid=", "-o", "command="]);
    let output = command
        .output()
        .map_err(|error| PresentError::io("/bin/ps", error))?;
    if !output.status.success() || output.stdout.len() > 4 * 1024 * 1024 {
        return Err(PresentError::BrowserUnavailable(
            "browser process inventory failed or exceeded its bound".to_string(),
        ));
    }
    macos_inventory_candidates(&output.stdout, instance_id, profile_dir)
}

/// The owned browser processes in a `ps -axww -o pid= -o command=` listing.
#[cfg(any(target_os = "macos", test))]
fn macos_inventory_candidates(
    output: &[u8],
    instance_id: Uuid,
    profile_dir: &Path,
) -> Result<Vec<u32>> {
    // `ps` lists every user's processes, and one caught in the middle of
    // `exec` can show bytes that are not UTF-8 (seen under a loaded full
    // gate, TSK-142 AC-6). Refusing the whole listing for it would fail
    // cleanup on any busy machine. Each invalid byte decodes to U+FFFD,
    // which is neither whitespace nor part of an identity argument, so an
    // owned line is still found and no other line can come to match.
    let output = String::from_utf8_lossy(output);
    let profile = format!("--user-data-dir={}", profile_dir.display());
    let instance = format!("--cf-present-instance={instance_id}");
    let mut candidates = Vec::new();
    for line in output.lines() {
        let line = line.trim_start();
        let Some(split) = line.find(char::is_whitespace) else {
            continue;
        };
        let (pid, command) = line.split_at(split);
        if rendered_command_line_contains_argument(command.trim_start(), &profile)
            && rendered_command_line_contains_argument(command.trim_start(), &instance)
        {
            candidates.push(pid.parse::<u32>().map_err(|_| {
                PresentError::BrowserUnavailable(
                    "browser process inventory contained an invalid PID".to_string(),
                )
            })?);
            if candidates.len() > 64 {
                return Err(PresentError::BrowserUnavailable(
                    "browser process candidate count exceeded its bound".to_string(),
                ));
            }
        }
    }
    Ok(candidates)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn owned_process_candidates(instance_id: Uuid, profile_dir: &Path) -> Result<Vec<u32>> {
    let current_uid = unsafe { libc::geteuid() };
    owned_process_candidates_from_proc(Path::new("/proc"), current_uid, instance_id, profile_dir)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn owned_process_candidates_from_proc(
    proc_root: &Path,
    current_uid: u32,
    instance_id: Uuid,
    profile_dir: &Path,
) -> Result<Vec<u32>> {
    use std::os::unix::fs::MetadataExt as _;

    let mut candidates = Vec::new();
    let mut examined = 0_usize;
    for entry in fs::read_dir(proc_root).map_err(|error| PresentError::io(proc_root, error))? {
        let entry = entry.map_err(|error| PresentError::io(proc_root, error))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        examined += 1;
        if examined > 65_536 {
            return Err(PresentError::BrowserUnavailable(
                "browser process inventory exceeded its bound".to_string(),
            ));
        }
        let process_dir = entry.path();
        let metadata = match fs::metadata(&process_dir) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(PresentError::io(&process_dir, error)),
        };
        if metadata.uid() != current_uid {
            continue;
        }
        let cmdline = process_dir.join("cmdline");
        let bytes = match fs::read(&cmdline) {
            Ok(bytes) if bytes.len() <= 64 * 1024 => bytes,
            Ok(_) => {
                return Err(PresentError::BrowserUnavailable(
                    "browser process identity exceeded its bound".to_string(),
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(PresentError::io(&cmdline, error)),
        };
        let arguments = linux_command_line_arguments(&bytes)?;
        if arguments_prove_identity(&arguments, instance_id, profile_dir) {
            candidates.push(pid);
            if candidates.len() > 64 {
                return Err(PresentError::BrowserUnavailable(
                    "browser process candidate count exceeded its bound".to_string(),
                ));
            }
        }
    }
    Ok(candidates)
}

#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn linux_command_line_arguments(bytes: &[u8]) -> Result<Vec<&str>> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(std::str::from_utf8)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| PresentError::BrowserUnavailable("browser identity is not UTF-8".to_string()))
}

#[cfg(unix)]
fn profile_resources_released(profile_dir: &Path) -> Result<bool> {
    match fs::symlink_metadata(profile_dir) {
        Ok(metadata) if metadata.is_dir() && !crate::platform::is_link_like(&metadata) => Ok(true),
        Ok(_) => Err(PresentError::UnsafePath(profile_dir.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(PresentError::io(profile_dir, error)),
    }
}

#[cfg(windows)]
fn process_identity(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<ProcessIdentity> {
    let script = format!(
        "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); $p=Get-CimInstance Win32_Process -Filter 'ProcessId={pid}' -ErrorAction Stop; if($null -eq $p){{exit 4}}; if($null -eq $p.CommandLine){{exit 5}}; [Console]::Out.Write($p.CommandLine)"
    );
    let powershell = crate::platform::trusted_system_path("WindowsPowerShell/v1.0/powershell.exe")?;
    let output = windows_identity_command(&powershell, &script)
        .output()
        .map_err(|error| PresentError::io(&powershell, error))?;
    if output.status.code() == Some(4) {
        return Ok(ProcessIdentity::Absent);
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
        return Ok(ProcessIdentity::Reused);
    }
    Ok(ProcessIdentity::Owned)
}

#[cfg(windows)]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsProcessRecord {
    process_id: u32,
    name: String,
    command_line: Option<String>,
}

#[cfg(windows)]
fn owned_process_candidates(instance_id: Uuid, profile_dir: &Path) -> Result<Vec<u32>> {
    let script = "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); $p=@(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {$_.Name -in @('chrome.exe','msedge.exe','chromium.exe')} | Select-Object ProcessId,Name,CommandLine); [Console]::Out.Write((ConvertTo-Json -Compress -InputObject $p))";
    let powershell = crate::platform::trusted_system_path("WindowsPowerShell/v1.0/powershell.exe")?;
    let output = windows_identity_command(&powershell, script)
        .output()
        .map_err(|error| PresentError::io(&powershell, error))?;
    if !output.status.success() || output.stdout.len() > 4 * 1024 * 1024 {
        return Err(PresentError::BrowserUnavailable(
            "Windows browser process inventory failed or exceeded its bound".to_string(),
        ));
    }
    let records: Vec<WindowsProcessRecord> =
        serde_json::from_slice(&output.stdout).map_err(|_| {
            PresentError::BrowserUnavailable(
                "Windows browser process inventory was not valid bounded JSON".to_string(),
            )
        })?;
    if records.len() > 4_096 {
        return Err(PresentError::BrowserUnavailable(
            "Windows browser process inventory exceeded its count bound".to_string(),
        ));
    }
    let mut candidates = Vec::new();
    for record in records {
        if !matches!(
            record.name.to_ascii_lowercase().as_str(),
            "chrome.exe" | "msedge.exe" | "chromium.exe"
        ) {
            return Err(PresentError::BrowserUnavailable(
                "Windows browser process inventory contained an unexpected executable".to_string(),
            ));
        }
        let command = record.command_line.ok_or_else(|| {
            PresentError::BrowserUnavailable(orphan_recovery_message(
                "a qualified Windows browser process has an unreadable command line",
            ))
        })?;
        let arguments = windows_command_line_arguments(&command)?;
        let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
        if arguments_prove_identity(&arguments, instance_id, profile_dir) {
            candidates.push(record.process_id);
            if candidates.len() > 64 {
                return Err(PresentError::BrowserUnavailable(
                    "Windows browser process candidate count exceeded its bound".to_string(),
                ));
            }
        }
    }
    Ok(candidates)
}

#[cfg(windows)]
fn profile_resources_released(profile_dir: &Path) -> Result<bool> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::{
        Foundation::{ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION},
        Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT},
    };

    let metadata = match fs::symlink_metadata(profile_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(PresentError::io(profile_dir, error)),
    };
    if !metadata.is_dir() || crate::platform::is_link_like(&metadata) {
        return Err(PresentError::UnsafePath(profile_dir.to_path_buf()));
    }

    let sharing = i32::try_from(ERROR_SHARING_VIOLATION).expect("error code fits i32");
    let locking = i32::try_from(ERROR_LOCK_VIOLATION).expect("error code fits i32");
    let mut pending = vec![profile_dir.to_path_buf()];
    let mut examined = 0_usize;
    while let Some(path) = pending.pop() {
        examined += 1;
        if examined > 65_536 {
            return Err(PresentError::BrowserUnavailable(
                "Windows browser profile resource proof exceeded its bound".to_string(),
            ));
        }
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| PresentError::io(&path, error))?;
        if crate::platform::is_link_like(&metadata) {
            return Err(PresentError::UnsafePath(path));
        }
        let mut options = fs::OpenOptions::new();
        options.read(true).share_mode(0).custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT
                | if metadata.is_dir() {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
        );
        match options.open(&path) {
            Ok(_) => {}
            Err(error) if matches!(error.raw_os_error(), Some(code) if code == sharing || code == locking) =>
            {
                return Ok(false);
            }
            Err(error) => return Err(PresentError::io(&path, error)),
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).map_err(|error| PresentError::io(&path, error))? {
                pending.push(
                    entry
                        .map_err(|error| PresentError::io(&path, error))?
                        .path(),
                );
            }
        } else if !metadata.is_file() {
            return Err(PresentError::UnsafePath(path));
        }
    }
    Ok(true)
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
    terminate_windows_processes_with(
        pid,
        |candidate| process_identity(candidate, instance_id, profile_dir),
        || owned_process_candidates(instance_id, profile_dir),
        |candidate| terminate_owned_windows_process(candidate, instance_id, profile_dir),
        || profile_resources_released(profile_dir),
    )
}

#[cfg(any(windows, test))]
fn terminate_windows_processes_with<Identity, Candidates, Terminate, Released>(
    pid: u32,
    mut identity: Identity,
    mut candidates: Candidates,
    mut terminate_owned: Terminate,
    mut resources_released: Released,
) -> Result<()>
where
    Identity: FnMut(u32) -> Result<ProcessIdentity>,
    Candidates: FnMut() -> Result<Vec<u32>>,
    Terminate: FnMut(u32) -> Result<()>,
    Released: FnMut() -> Result<bool>,
{
    const MAX_VISITED_PROCESSES: usize = 64;

    let mut pending = vec![pid];
    let mut scheduled = BTreeSet::from([pid]);
    let mut visited = BTreeSet::new();
    while let Some(candidate) = pending.pop() {
        if !visited.insert(candidate) {
            continue;
        }
        match identity(candidate)? {
            ProcessIdentity::Owned => terminate_owned(candidate)?,
            ProcessIdentity::Absent | ProcessIdentity::Reused => {
                for discovered in candidates()? {
                    if visited.contains(&discovered) || !scheduled.insert(discovered) {
                        continue;
                    }
                    if scheduled.len() > MAX_VISITED_PROCESSES {
                        return Err(PresentError::BrowserUnavailable(orphan_recovery_message(
                            "Windows browser recovery exceeded its bounded process worklist",
                        )));
                    }
                    pending.push(discovered);
                }
            }
        }
    }
    if !candidates()?.is_empty() || !resources_released()? {
        return Err(PresentError::BrowserUnavailable(orphan_recovery_message(
            "the owned Windows presentation browser tree still has an exact process or open profile resource",
        )));
    }
    Ok(())
}

#[cfg(windows)]
fn terminate_owned_windows_process(pid: u32, instance_id: Uuid, profile_dir: &Path) -> Result<()> {
    let taskkill = crate::platform::trusted_system_path("taskkill.exe")?;
    let output = windows_terminate_command(&taskkill, pid)
        .output()
        .map_err(|error| PresentError::io(&taskkill, error))?;
    if !output.status.success() || output.stdout.len() + output.stderr.len() > 64 * 1024 {
        return Err(PresentError::BrowserUnavailable(
            "the owned presentation browser process tree did not terminate".to_string(),
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        match process_identity(pid, instance_id, profile_dir)? {
            ProcessIdentity::Absent | ProcessIdentity::Reused => return Ok(()),
            ProcessIdentity::Owned => thread::sleep(Duration::from_millis(50)),
        }
    }
    Err(PresentError::BrowserUnavailable(orphan_recovery_message(
        "the exactly owned Windows presentation browser process did not terminate",
    )))
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

    #[cfg(target_os = "macos")]
    static PROCESS_GROUP_TEST_LEASE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(target_os = "macos")]
    fn acquire_process_group_test_file_lease() -> fs::File {
        use fs2::FileExt as _;
        use std::os::unix::fs::OpenOptionsExt as _;

        let path = std::env::temp_dir().join("codeflow-present-process-group-tests.lock");
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&path)
            .unwrap();
        file.lock_exclusive().unwrap();
        file
    }

    fn presentation_store() -> (tempfile::TempDir, SessionStore, Uuid, PathBuf) {
        use crate::document::{Block, ParsedDocument, PresentationDocument, Provenance};

        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore::at_root(temp.path().join("project"), "key".to_string()).unwrap();
        let session = store
            .create(ParsedDocument::Supported(PresentationDocument {
                summary: None,
                schema_version: 1,
                title: "Recovery".to_string(),
                language: None,
                provenance: Provenance::default(),
                blocks: vec![Block::Narrative {
                    id: "summary".to_string(),
                    markdown: "Recovery".to_string(),
                }],
            }))
            .unwrap();
        let profile = store
            .runtime_dir(session.id)
            .unwrap()
            .join("browser-profile");
        create_private_dir_all(&profile).unwrap();
        (temp, store, session.id, profile)
    }

    #[test]
    fn per_attempt_launch_recovery_is_consumed_without_overwrite() {
        let (_temp, store, session_id, profile) = presentation_store();
        let instance = Uuid::new_v4();
        let recovery = launch_recovery_path(&profile, instance).unwrap();
        write_launch_recovery(&store, session_id, &recovery, instance, None, &profile).unwrap();
        let second_instance = Uuid::new_v4();
        let second_recovery = launch_recovery_path(&profile, second_instance).unwrap();
        assert_ne!(recovery, second_recovery);

        recover_incomplete_launch(&store, session_id, &profile).unwrap();
        assert!(!recovery.exists());
        assert!(!profile.exists());

        create_private_dir_all(&profile).unwrap();
        write_launch_recovery(
            &store,
            session_id,
            &second_recovery,
            second_instance,
            None,
            &profile,
        )
        .unwrap();
        assert!(second_recovery.exists());
        recover_incomplete_launch(&store, session_id, &profile).unwrap();
        assert!(!second_recovery.exists());
        assert!(!profile.exists());
    }

    #[test]
    fn reused_recovery_pid_converges_without_signalling_the_reused_process() {
        let (_temp, store, session_id, profile) = presentation_store();
        let instance = Uuid::new_v4();
        let recovery = launch_recovery_path(&profile, instance).unwrap();
        write_launch_recovery(
            &store,
            session_id,
            &recovery,
            instance,
            Some(std::process::id()),
            &profile,
        )
        .unwrap();

        recover_incomplete_launch(&store, session_id, &profile).unwrap();
        assert!(!recovery.exists());
        assert!(!profile.exists());
    }

    #[test]
    fn committed_launch_record_is_finalized_without_stopping_registered_identity() {
        let (_temp, store, session_id, profile) = presentation_store();
        let instance = Uuid::new_v4();
        let pid = std::process::id();
        let recovery = launch_recovery_path(&profile, instance).unwrap();
        write_launch_recovery(&store, session_id, &recovery, instance, Some(pid), &profile)
            .unwrap();
        store.set_browser(session_id, pid, instance).unwrap();

        recover_incomplete_launch(&store, session_id, &profile).unwrap();
        assert!(!recovery.exists());
        assert!(profile.exists());
        assert_eq!(
            store.load(session_id).unwrap().browser_instance,
            Some(instance)
        );
        assert!(store.clear_browser(session_id, instance).unwrap());
    }

    #[test]
    fn browser_launch_lease_serializes_recovery() {
        let (_temp, store, session_id, profile) = presentation_store();
        let _lease = store.acquire_browser_launch_lease(session_id).unwrap();
        assert!(matches!(
            recover_incomplete_launch(&store, session_id, &profile),
            Err(PresentError::ServiceUnavailable(_))
        ));
    }

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

    /// TSK-142 AC-6: under load, `ps` can list a process of any user in
    /// the middle of `exec` with bytes that are not UTF-8. That line is not
    /// ours, so it never fails the inventory, and an owned line is found
    /// whatever else it or its neighbours hold.
    #[test]
    fn a_non_utf8_line_in_the_inventory_never_hides_or_fails_an_owned_process() {
        let instance = Uuid::new_v4();
        let profile = Path::new("/state/browser-profile");
        let owned =
            format!("--user-data-dir=/state/browser-profile --cf-present-instance={instance}");
        let mut listing = b"  12 /bin/other \xff\xfe arg\n".to_vec();
        listing.extend_from_slice(format!("  34 /bin/sh -c read value {owned}\n").as_bytes());
        listing.extend_from_slice(b"  56 chrome \xc3 ");
        listing.extend_from_slice(format!("{owned}\n").as_bytes());
        listing
            .extend_from_slice(b"  78 \xe2\x82 /bin/sh --user-data-dir=/state/browser-profile\n");
        assert_eq!(
            macos_inventory_candidates(&listing, instance, profile).unwrap(),
            [34, 56]
        );
        // A line whose identity is split by a stray byte does not match.
        let split = format!("  90 chrome --user-data-dir=/state/browser-profile\u{fffd} --cf-present-instance={instance}\n");
        assert!(
            macos_inventory_candidates(split.as_bytes(), instance, profile)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn linux_command_line_decoder_is_nul_exact_and_rejects_non_utf8() {
        assert_eq!(
            linux_command_line_arguments(b"browser\0\0--flag=value\0").unwrap(),
            ["browser", "--flag=value"]
        );
        assert!(linux_command_line_arguments(b"browser\0\xff\0")
            .unwrap_err()
            .to_string()
            .contains("not UTF-8"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_process_identity_proves_exact_markers_and_absence() {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("browser-profile");
        fs::create_dir(&profile).unwrap();
        let instance = Uuid::new_v4();
        let mut child = crate::platform::restricted_command("/bin/sh")
            .args([
                "-c",
                "read value || true",
                "cf-present-browser",
                &format!("--user-data-dir={}", profile.display()),
                &format!("--cf-present-instance={instance}"),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match process_identity(pid, instance, &profile) {
                Ok(ProcessIdentity::Owned) => break,
                result if Instant::now() < deadline => {
                    let _ = result;
                    thread::sleep(Duration::from_millis(10));
                }
                result => panic!("Linux process identity did not settle: {result:?}"),
            }
        }
        assert_eq!(
            process_identity(pid, Uuid::new_v4(), &profile).unwrap(),
            ProcessIdentity::Reused
        );
        assert!(owned_process_candidates(instance, &profile)
            .unwrap()
            .contains(&pid));

        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());
        assert_eq!(
            process_identity(pid, instance, &profile).unwrap(),
            ProcessIdentity::Absent
        );
        assert!(process_identity(u32::MAX, instance, &profile)
            .unwrap_err()
            .to_string()
            .contains("outside the platform range"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_proc_identity_rejects_unsafe_command_line_shapes() {
        let temp = tempfile::tempdir().unwrap();
        let instance = Uuid::new_v4();
        let profile = temp.path().join("browser-profile");

        fs::create_dir_all(temp.path().join("1/cmdline")).unwrap();
        assert!(
            process_identity_from_proc(temp.path(), 1, instance, &profile)
                .unwrap_err()
                .to_string()
                .contains("exceeded its bound")
        );

        fs::create_dir_all(temp.path().join("2")).unwrap();
        fs::write(temp.path().join("2/cmdline"), vec![b'x'; 64 * 1024 + 1]).unwrap();
        assert!(
            process_identity_from_proc(temp.path(), 2, instance, &profile)
                .unwrap_err()
                .to_string()
                .contains("exceeded its bound")
        );

        fs::create_dir_all(temp.path().join("3")).unwrap();
        fs::write(temp.path().join("3/cmdline"), b"browser\0\xff\0").unwrap();
        assert!(
            process_identity_from_proc(temp.path(), 3, instance, &profile)
                .unwrap_err()
                .to_string()
                .contains("not UTF-8")
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_proc_inventory_rejects_oversize_and_invalid_identity() {
        let instance = Uuid::new_v4();
        let profile = Path::new("/state/browser-profile");
        let current_uid = unsafe { libc::geteuid() };

        let oversize = tempfile::tempdir().unwrap();
        fs::create_dir(oversize.path().join("7")).unwrap();
        fs::write(oversize.path().join("7/cmdline"), vec![b'x'; 64 * 1024 + 1]).unwrap();
        assert!(owned_process_candidates_from_proc(
            oversize.path(),
            current_uid,
            instance,
            profile
        )
        .unwrap_err()
        .to_string()
        .contains("exceeded its bound"));

        let invalid = tempfile::tempdir().unwrap();
        fs::create_dir(invalid.path().join("8")).unwrap();
        fs::write(invalid.path().join("8/cmdline"), b"browser\0\xff\0").unwrap();
        assert!(
            owned_process_candidates_from_proc(invalid.path(), current_uid, instance, profile)
                .unwrap_err()
                .to_string()
                .contains("not UTF-8")
        );
    }

    #[test]
    fn windows_recovery_rejects_a_repeated_contradictory_candidate() {
        let identity_calls = std::cell::Cell::new(0_usize);
        let inventory_calls = std::cell::Cell::new(0_usize);
        let termination_calls = std::cell::Cell::new(0_usize);
        let error = terminate_windows_processes_with(
            7,
            |_| {
                identity_calls.set(identity_calls.get() + 1);
                Ok(ProcessIdentity::Reused)
            },
            || {
                inventory_calls.set(inventory_calls.get() + 1);
                Ok(vec![7])
            },
            |_| {
                termination_calls.set(termination_calls.get() + 1);
                Ok(())
            },
            || Ok(true),
        )
        .unwrap_err();

        assert!(error.to_string().contains("still has an exact process"));
        assert_eq!(identity_calls.get(), 1);
        assert_eq!(inventory_calls.get(), 2);
        assert_eq!(termination_calls.get(), 0);
    }

    #[test]
    fn windows_recovery_deduplicates_candidate_cycles() {
        use std::{cell::RefCell, collections::VecDeque};

        let identity_calls = RefCell::new(Vec::new());
        let inventories = RefCell::new(VecDeque::from([vec![1, 2, 2], vec![2, 1], Vec::new()]));
        terminate_windows_processes_with(
            1,
            |pid| {
                identity_calls.borrow_mut().push(pid);
                Ok(ProcessIdentity::Reused)
            },
            || Ok(inventories.borrow_mut().pop_front().unwrap()),
            |_| panic!("a reused candidate must never be terminated"),
            || Ok(true),
        )
        .unwrap();

        assert_eq!(*identity_calls.borrow(), vec![1, 2]);
        assert!(inventories.borrow().is_empty());
    }

    #[test]
    fn windows_recovery_terminates_only_a_proven_owned_candidate_once() {
        use std::{cell::RefCell, collections::VecDeque};

        let identity_calls = RefCell::new(Vec::new());
        let termination_calls = RefCell::new(Vec::new());
        let inventories = RefCell::new(VecDeque::from([vec![11, 11], Vec::new()]));
        terminate_windows_processes_with(
            7,
            |pid| {
                identity_calls.borrow_mut().push(pid);
                Ok(if pid == 11 {
                    ProcessIdentity::Owned
                } else {
                    ProcessIdentity::Reused
                })
            },
            || Ok(inventories.borrow_mut().pop_front().unwrap()),
            |pid| {
                termination_calls.borrow_mut().push(pid);
                Ok(())
            },
            || Ok(true),
        )
        .unwrap();

        assert_eq!(*identity_calls.borrow(), vec![7, 11]);
        assert_eq!(*termination_calls.borrow(), vec![11]);
        assert!(!termination_calls.borrow().contains(&7));
        assert!(inventories.borrow().is_empty());
    }

    #[test]
    fn windows_recovery_fails_closed_when_the_worklist_limit_is_exhausted() {
        let error = terminate_windows_processes_with(
            1,
            |_| Ok(ProcessIdentity::Reused),
            || Ok((2..=65).collect()),
            |_| panic!("a reused candidate must never be terminated"),
            || panic!("resource proof must not run after worklist exhaustion"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("bounded process worklist"));
    }

    #[cfg(unix)]
    #[test]
    fn orphan_process_group_retains_recovery_state_when_leader_is_absent() {
        use std::os::unix::process::CommandExt as _;

        #[cfg(target_os = "macos")]
        let _process_group_test_lease = PROCESS_GROUP_TEST_LEASE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(target_os = "macos")]
        let _process_group_test_file_lease = acquire_process_group_test_file_lease();

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
        use std::os::unix::process::ExitStatusExt as _;

        let _process_group_test_lease = PROCESS_GROUP_TEST_LEASE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _process_group_test_file_lease = acquire_process_group_test_file_lease();

        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("browser-profile");
        fs::create_dir(&profile).unwrap();
        let instance = Uuid::new_v4();
        // No TERM trap: macOS `/bin/sh` (bash 3.2) defers a trapped TERM
        // that lands as the `read` builtin starts until `read` returns, so
        // under load the fixture could miss the graceful signal and be
        // escalated to SIGKILL (TSK-142 AC-6). The default action ends it
        // on the signal itself, every time.
        let mut command = crate::platform::restricted_command("/bin/sh");
        command
            .args([
                "-c",
                "read value",
                "cf-present-browser",
                &format!("--user-data-dir={}", profile.display()),
                &format!("--cf-present-instance={instance}"),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let pid = child.id();
        let waiter = thread::spawn(move || child.wait().unwrap());

        let identity_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match process_identity(pid, instance, &profile) {
                Ok(ProcessIdentity::Owned) => break,
                result if Instant::now() < identity_deadline => {
                    let _ = result;
                    thread::sleep(Duration::from_millis(10));
                }
                result => panic!("owned process identity did not settle: {result:?}"),
            }
        }
        let wrong = Uuid::new_v4();
        assert_eq!(
            process_identity(pid, wrong, &profile).unwrap(),
            ProcessIdentity::Reused
        );
        terminate_qualified_process(pid, instance, &profile).unwrap();
        let status = waiter.join().unwrap();
        drop(stdin);
        // Ended by the graceful signal, never escalated to SIGKILL.
        assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
        assert!(owned_process_candidates(instance, &profile)
            .unwrap()
            .is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn process_group_cleanup_reverifies_before_forced_escalation() {
        use std::os::unix::process::CommandExt as _;

        let _process_group_test_lease = PROCESS_GROUP_TEST_LEASE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _process_group_test_file_lease = acquire_process_group_test_file_lease();

        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("browser-profile");
        fs::create_dir(&profile).unwrap();
        let instance = Uuid::new_v4();
        let mut command = crate::platform::restricted_command("/bin/sh");
        command
            .args([
                "-c",
                "trap '' TERM; read value",
                "cf-present-browser",
                &format!("--user-data-dir={}", profile.display()),
                &format!("--cf-present-instance={instance}"),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        let waiter = thread::spawn(move || child.wait().unwrap());
        let deadline = Instant::now() + Duration::from_secs(1);
        while process_identity(pid, instance, &profile).unwrap() != ProcessIdentity::Owned {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }

        terminate_qualified_process(pid, instance, &profile).unwrap();
        let status = waiter.join().unwrap();
        assert!(!status.success());
        assert!(owned_process_candidates(instance, &profile)
            .unwrap()
            .is_empty());
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
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "OPENAI_API_KEY",
                "AWS_SECRET_ACCESS_KEY",
                "AWS_SESSION_TOKEN",
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
