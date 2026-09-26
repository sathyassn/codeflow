use std::{
    fs,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use clap::{Args, Subcommand};
use codeflow_present::{
    browser,
    document::parse_document,
    export::{export_session, ExportMode, ExportTheme},
    limits,
    service::{serve_session, HealthRecord, ReadyRecord},
    state::{FeedbackResolution, SessionStatus, SessionStore},
    PresentError,
};
use uuid::Uuid;

#[derive(Debug, Args)]
#[command(
    about = "Review this session on the utility presentation surface",
    long_about = "Turn a validated catalog JSON document into one isolated review surface.\n\
Author this session's subject; the runtime owns chrome, themes, and Comment.\n\
Not a documentation portal, product UI, or a clone of the design-exploration board."
)]
pub struct PresentArgs {
    #[command(subcommand)]
    command: PresentCommand,
}

#[derive(Debug, Subcommand)]
enum PresentCommand {
    /// Open a validated presentation document in an isolated browser profile.
    Open {
        document: PathBuf,
        /// Start the service but do not launch a browser window.
        #[arg(long)]
        no_launch: bool,
    },
    /// List presentation sessions for this project.
    List,
    /// Reopen an active presentation session.
    Show {
        session_id: String,
        /// Print the session endpoint and profile without launching.
        #[arg(long)]
        no_launch: bool,
    },
    /// Append a validated immutable revision to an active session.
    Update {
        session_id: String,
        document: PathBuf,
    },
    /// Print the append-only feedback history as JSON.
    History { session_id: String },
    /// Deliver pending review envelopes as JSON lines.
    Feedback {
        session_id: String,
        /// Continue until the session closes.
        #[arg(long)]
        follow: bool,
    },
    /// Mark one delivered feedback event addressed or dismissed.
    Resolve {
        session_id: String,
        event_id: String,
        /// Current event version printed by the review surface/history.
        #[arg(long)]
        event_version: u64,
        #[arg(long, value_parser = ["addressed", "dismissed"])]
        status: String,
    },
    /// Close a presentation session. Repeating close is safe.
    Close { session_id: String },
    /// Export a deterministic self-contained read-only HTML artifact.
    Export {
        session_id: String,
        #[arg(long, value_name = "FILE")]
        out: PathBuf,
        #[arg(long, default_value = "editorial", value_parser = ["editorial", "technical"])]
        theme: String,
        #[arg(long, default_value = "system", value_parser = ["system", "light", "dark"])]
        mode: String,
    },
    /// Remove eligible closed session state.
    Clear {
        session_id: Option<String>,
        #[arg(long, default_value = "30d")]
        older_than: String,
        #[arg(long)]
        dry_run: bool,
    },
    #[command(hide = true)]
    ServeInternal { session_id: String },
}

pub fn run(args: &PresentArgs) -> i32 {
    match run_inner(&args.command) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("present: {error}");
            exit_code(&error)
        }
    }
}

fn run_inner(command: &PresentCommand) -> codeflow_present::Result<()> {
    let project = std::env::current_dir().map_err(|error| PresentError::io(".", error))?;
    let store = SessionStore::discover(&project)?;
    match command {
        PresentCommand::Open {
            document,
            no_launch,
        } => open(&store, document, *no_launch),
        PresentCommand::List => {
            println!("{}", serde_json::to_string_pretty(&store.list()?)?);
            Ok(())
        }
        PresentCommand::Show {
            session_id,
            no_launch,
        } => show(&store, parse_id(session_id)?, *no_launch),
        PresentCommand::Update {
            session_id,
            document,
        } => {
            let bytes = read_document(document)?;
            let revision = store.update_document(parse_id(session_id)?, parse_document(&bytes)?)?;
            println!("updated {session_id} to revision {revision}");
            Ok(())
        }
        PresentCommand::History { session_id } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&store.history(parse_id(session_id)?)?)?
            );
            Ok(())
        }
        PresentCommand::Feedback { session_id, follow } => {
            deliver_feedback(&store, parse_id(session_id)?, *follow)
        }
        PresentCommand::Resolve {
            session_id,
            event_id,
            event_version,
            status,
        } => resolve_feedback(&store, session_id, event_id, *event_version, status),
        PresentCommand::Close { session_id } => close(&store, session_id),
        PresentCommand::Export {
            session_id,
            out,
            theme,
            mode,
        } => export(&store, session_id, out, theme, mode),
        PresentCommand::Clear {
            session_id,
            older_than,
            dry_run,
        } => clear(&store, session_id.as_deref(), older_than, *dry_run),
        PresentCommand::ServeInternal { session_id } => serve(project, session_id),
    }
}

fn resolve_feedback(
    store: &SessionStore,
    session_id: &str,
    event_id: &str,
    event_version: u64,
    status: &str,
) -> codeflow_present::Result<()> {
    let resolution = match status {
        "addressed" => FeedbackResolution::Addressed,
        "dismissed" => FeedbackResolution::Dismissed,
        _ => unreachable!("clap validates feedback resolution"),
    };
    let sequence = store.resolve_feedback(
        parse_id(session_id)?,
        parse_id(event_id)?,
        event_version,
        resolution,
    )?;
    println!("resolved {event_id} as {status} at version {sequence}");
    Ok(())
}

fn close(store: &SessionStore, session_id: &str) -> codeflow_present::Result<()> {
    let id = parse_id(session_id)?;
    store.close(id)?;
    let profile = store.runtime_dir(id)?.join("browser-profile");
    browser::recover_incomplete_launch(store, id, &profile)?;
    let session = store.load(id)?;
    if let (Some(pid), Some(instance_id)) = (session.browser_pid, session.browser_instance) {
        browser::terminate_isolated(store, id, pid, instance_id, &profile)?;
    }
    store.enforce_retention()?;
    println!("closed {id}");
    Ok(())
}

fn export(
    store: &SessionStore,
    session_id: &str,
    out: &Path,
    theme: &str,
    mode: &str,
) -> codeflow_present::Result<()> {
    let theme = match theme {
        "editorial" => ExportTheme::Editorial,
        "technical" => ExportTheme::Technical,
        _ => unreachable!("clap validates export themes"),
    };
    let mode = match mode {
        "system" => ExportMode::System,
        "light" => ExportMode::Light,
        "dark" => ExportMode::Dark,
        _ => unreachable!("clap validates export modes"),
    };
    export_session(store, parse_id(session_id)?, out, theme, mode)?;
    println!("exported {}", out.display());
    Ok(())
}

fn clear(
    store: &SessionStore,
    session_id: Option<&str>,
    older_than: &str,
    dry_run: bool,
) -> codeflow_present::Result<()> {
    let selected = session_id.map(parse_id).transpose()?;
    let removed = store.clear(selected, parse_duration(older_than)?, dry_run)?;
    for id in removed {
        println!("{} {id}", if dry_run { "would remove" } else { "removed" });
    }
    Ok(())
}

fn serve(project: PathBuf, session_id: &str) -> codeflow_present::Result<()> {
    let id = parse_id(session_id)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    runtime.block_on(serve_session(project, id))
}

fn open(store: &SessionStore, document: &Path, no_launch: bool) -> codeflow_present::Result<()> {
    let bytes = read_document(document)?;
    let parsed = parse_document(&bytes)?;
    let session = store.create(parsed)?;
    let ready = start_service(store, session.id)?;
    if no_launch {
        println!(
            "session {} ready; open the owner-private bootstrap file {} in a qualified isolated browser profile",
            session.id,
            ready.bootstrap_path.display()
        );
        print_handoff_link(store, &ready.bootstrap_path)?;
        return Ok(());
    }
    let profile = store.runtime_dir(session.id)?.join("browser-profile");
    browser::launch_isolated(store, session.id, &ready.bootstrap_path, &profile)?;
    println!("opened {}", session.id);
    Ok(())
}

fn show(store: &SessionStore, id: Uuid, no_launch: bool) -> codeflow_present::Result<()> {
    let session = store.load(id)?;
    if session.status != SessionStatus::Active {
        println!("{}", serde_json::to_string_pretty(&session)?);
        return Ok(());
    }
    let healthy = match (session.service_port, session.service_instance) {
        (Some(port), Some(instance_id)) => verify_service(id, port, instance_id).is_ok(),
        (None, None) => false,
        _ => unreachable!("validated sessions cannot contain partial service identity"),
    };
    if !healthy {
        if let Some(instance_id) = session.service_instance {
            let lease = store.acquire_service_lease(id)?;
            drop(lease);
            if !store.clear_service(id, instance_id)? {
                return Err(PresentError::ServiceUnavailable(
                    "the stale presentation service identity changed during recovery".to_string(),
                ));
            }
        }
        let ready = start_service(store, id)?;
        let profile = store.runtime_dir(id)?.join("browser-profile");
        if no_launch {
            println!(
                "session {id} recovered; open the owner-private bootstrap file {} in the isolated profile {}",
                ready.bootstrap_path.display(),
                profile.display()
            );
            print_handoff_link(store, &ready.bootstrap_path)?;
            return Ok(());
        }
        launch_or_focus_guard(store, id, &ready.bootstrap_path, None, &profile)?;
        println!("recovered and opened {id}");
        return Ok(());
    }
    let port = session
        .service_port
        .expect("healthy service has a recorded port");
    let authority = format!("127.0.0.1:{port}");
    let profile = store.runtime_dir(id)?.join("browser-profile");
    browser::recover_incomplete_launch(store, id, &profile)?;
    if let (Some(pid), Some(instance_id)) = (session.browser_pid, session.browser_instance) {
        if browser::is_isolated_running(pid, instance_id, &profile)? {
            if no_launch {
                println!("http://{authority}/app/ profile={}", profile.display());
                return Ok(());
            }
            return Err(PresentError::BrowserUnavailable(
                "this presentation already has an owned browser window".to_string(),
            ));
        }
        let _ = store.clear_browser(id, instance_id)?;
    }
    let ready_path = store.runtime_dir(id)?.join("control").join("ready.json");
    let ready = read_ready_record(&ready_path, id)?;
    request_rebootstrap(&ready)?;
    if no_launch {
        println!(
            "session {id} ready; open the owner-private bootstrap file {} in the isolated profile {}",
            ready.bootstrap_path.display(),
            profile.display()
        );
        print_handoff_link(store, &ready.bootstrap_path)?;
    } else {
        browser::launch_isolated(store, id, &ready.bootstrap_path, &profile)?;
        println!("opened {id}");
    }
    Ok(())
}

/// Print the openable link an agent hands to the operator when `codeflow` does
/// not launch the browser itself, such as from an agent sandbox.
fn print_handoff_link(store: &SessionStore, bootstrap_path: &Path) -> codeflow_present::Result<()> {
    println!(
        "handoff link (single use, open within {} seconds): {}",
        limits::BOOTSTRAP_TTL_SECONDS,
        browser::handoff_link(store, bootstrap_path)?
    );
    Ok(())
}

fn launch_or_focus_guard(
    store: &SessionStore,
    id: Uuid,
    bootstrap_path: &Path,
    authority: Option<&str>,
    profile: &Path,
) -> codeflow_present::Result<()> {
    browser::recover_incomplete_launch(store, id, profile)?;
    let session = store.load(id)?;
    if let (Some(pid), Some(instance_id)) = (session.browser_pid, session.browser_instance) {
        if browser::is_isolated_running(pid, instance_id, profile)? {
            return Err(PresentError::BrowserUnavailable(
                "this presentation already has an owned browser window".to_string(),
            ));
        }
        let _ = store.clear_browser(id, instance_id)?;
    }
    match authority {
        Some(authority) => browser::launch_application(store, id, authority, profile),
        None => browser::launch_isolated(store, id, bootstrap_path, profile),
    }
}

fn start_service(store: &SessionStore, id: Uuid) -> codeflow_present::Result<ReadyRecord> {
    let _startup = store.acquire_startup_lease(id)?;
    let executable = std::env::current_exe()
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    let runtime = store.runtime_dir(id)?;
    let control = runtime.join("control");
    let ready_path = control.join("ready.json");
    let bootstrap_path = control.join("bootstrap.html");
    remove_regular_if_present(&ready_path)?;
    remove_regular_if_present(&bootstrap_path)?;
    let mut command = Command::new(executable);
    apply_minimal_service_environment(&mut command);
    let child = command
        .arg("present")
        .arg("serve-internal")
        .arg(id.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    let mut child = ChildGuard::new(child);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if ready_path.is_file() {
            let ready = read_ready_record(&ready_path, id)?;
            let session = store.load(id)?;
            if session.service_port != Some(ready.port)
                || session.service_instance != Some(ready.instance_id)
            {
                return Err(PresentError::CorruptState(
                    "presentation ready record does not match persisted service identity"
                        .to_string(),
                ));
            }
            child.disarm();
            return Ok(ready);
        }
        if let Some(status) = child
            .child_mut()
            .try_wait()
            .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?
        {
            return Err(PresentError::ServiceUnavailable(format!(
                "service exited before readiness with {status}"
            )));
        }
        if Instant::now() >= deadline {
            return Err(PresentError::ServiceUnavailable(
                "timed out waiting for the service readiness record".to_string(),
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn read_ready_record(path: &Path, id: Uuid) -> codeflow_present::Result<ReadyRecord> {
    let bytes = read_bounded_regular(path, 16 * 1024)?;
    let ready: ReadyRecord = serde_json::from_slice(&bytes)?;
    if ready.schema_version != 1
        || ready.session_id != id
        || ready.recovery_capability.len() != 64
        || !ready
            .recovery_capability
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(PresentError::CorruptState(
            "presentation ready record is mismatched".to_string(),
        ));
    }
    Ok(ready)
}

fn request_rebootstrap(ready: &ReadyRecord) -> codeflow_present::Result<()> {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), ready.port);
    let timeout = Duration::from_secs(2);
    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    let form = format!("recovery_capability={}", ready.recovery_capability);
    let request = format!(
        "POST /_cf-present/rebootstrap/{} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        ready.instance_id,
        ready.port,
        form.len(),
        form
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    let mut response = Vec::new();
    stream
        .take(16 * 1024)
        .read_to_end(&mut response)
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    if !response.starts_with(b"HTTP/1.1 204 ") {
        return Err(PresentError::ServiceUnavailable(
            "presentation service refused bootstrap recovery".to_string(),
        ));
    }
    Ok(())
}

fn apply_minimal_service_environment(command: &mut Command) {
    const ALLOWED: &[&str] = &[
        "HOME",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
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

fn remove_regular_if_present(path: &Path) -> codeflow_present::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_link_like(&metadata) => {
            fs::remove_file(path).map_err(|error| PresentError::io(path, error))
        }
        Ok(_) => Err(PresentError::UnsafePath(path.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PresentError::io(path, error)),
    }
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn child_mut(&mut self) -> &mut Child {
        self.0.as_mut().expect("child guard is armed")
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn verify_service(id: Uuid, port: u16, instance_id: Uuid) -> codeflow_present::Result<()> {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let timeout = Duration::from_secs(2);
    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    let request = format!(
        "GET /_cf-present/health/{instance_id} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept: application/json\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    let mut response = Vec::new();
    stream
        .take(16 * 1024)
        .read_to_end(&mut response)
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    let separator = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            PresentError::ServiceUnavailable("invalid service health response".to_string())
        })?;
    if !response.starts_with(b"HTTP/1.1 200 ") {
        return Err(PresentError::ServiceUnavailable(
            "the recorded port is not the expected presentation service".to_string(),
        ));
    }
    let health: HealthRecord = serde_json::from_slice(&response[separator + 4..])?;
    if health.schema_version != 1 || health.session_id != id || health.instance_id != instance_id {
        return Err(PresentError::ServiceUnavailable(
            "the recorded port did not prove the expected presentation service identity"
                .to_string(),
        ));
    }
    Ok(())
}

fn deliver_feedback(store: &SessionStore, id: Uuid, follow: bool) -> codeflow_present::Result<()> {
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    loop {
        let pending = store.pending_feedback(id)?;
        for envelope in &pending {
            serde_json::to_writer(&mut output, envelope)?;
            output
                .write_all(b"\n")
                .and_then(|()| output.flush())
                .map_err(|error| PresentError::io("stdout", error))?;
            store.mark_delivered(id, &[envelope.event_id])?;
        }
        if !follow || store.load(id)?.status == SessionStatus::Closed {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn read_document(path: &Path) -> codeflow_present::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|error| PresentError::io(path, error))?;
    if !metadata.is_file() || is_link_like(&metadata) {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    if metadata.len() > codeflow_present::limits::MAX_DOCUMENT_BYTES as u64 {
        return Err(PresentError::DocumentTooLarge {
            limit: codeflow_present::limits::MAX_DOCUMENT_BYTES,
        });
    }
    let bytes = read_bounded_regular(path, codeflow_present::limits::MAX_DOCUMENT_BYTES)?;
    if bytes.len() > codeflow_present::limits::MAX_DOCUMENT_BYTES {
        return Err(PresentError::DocumentTooLarge {
            limit: codeflow_present::limits::MAX_DOCUMENT_BYTES,
        });
    }
    Ok(bytes)
}

fn read_bounded_regular(path: &Path, max_bytes: usize) -> codeflow_present::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|error| PresentError::io(path, error))?;
    if !metadata.is_file() || is_link_like(&metadata) {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|error| PresentError::io(path, error))?;
    let opened = file
        .metadata()
        .map_err(|error| PresentError::io(path, error))?;
    if !opened.is_file() || opened.len() > max_bytes as u64 {
        return Err(PresentError::CorruptState(format!(
            "{} exceeds its bounded input size",
            path.display()
        )));
    }
    let capacity = usize::try_from(opened.len()).map_err(|_| {
        PresentError::CorruptState(format!("{} is not addressable", path.display()))
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| PresentError::io(path, error))?;
    if bytes.len() > max_bytes {
        return Err(PresentError::CorruptState(format!(
            "{} grew beyond its bounded input size",
            path.display()
        )));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_link_like(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn parse_id(value: &str) -> codeflow_present::Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| PresentError::InvalidSessionId(value.to_string()))
}

fn parse_duration(value: &str) -> codeflow_present::Result<Duration> {
    let (number, unit) = value.split_at(value.len().saturating_sub(1));
    let amount = number.parse::<u64>().map_err(|_| {
        PresentError::InvalidDocument("duration must look like 24h, 30d, or 2w".to_string())
    })?;
    let seconds = match unit {
        "h" => amount.checked_mul(60 * 60),
        "d" => amount.checked_mul(24 * 60 * 60),
        "w" => amount.checked_mul(7 * 24 * 60 * 60),
        _ => None,
    }
    .ok_or_else(|| PresentError::InvalidDocument("duration is invalid or too large".to_string()))?;
    Ok(Duration::from_secs(seconds))
}

fn exit_code(error: &PresentError) -> i32 {
    match error {
        PresentError::DocumentTooLarge { .. }
        | PresentError::InvalidDocument(_)
        | PresentError::UnsupportedSchema { .. }
        | PresentError::InvalidSessionId(_)
        | PresentError::Json(_) => 2,
        PresentError::SessionNotFound(_) => 3,
        PresentError::BrowserUnavailable(_) | PresentError::ServiceUnavailable(_) => 4,
        PresentError::UnsafePath(_) | PresentError::CorruptState(_) => 5,
        PresentError::SessionClosed(_)
        | PresentError::PartialCleanup { .. }
        | PresentError::Io { .. } => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_parser_is_bounded_and_explicit() {
        assert_eq!(parse_duration("24h").unwrap(), Duration::from_secs(86_400));
        assert_eq!(
            parse_duration("2w").unwrap(),
            Duration::from_secs(1_209_600)
        );
        assert!(parse_duration("30").is_err());
        assert!(parse_duration("-1d").is_err());
    }

    #[test]
    fn service_environment_excludes_provider_secrets() {
        let mut command = Command::new("service");
        for secret in [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "OPENAI_API_KEY",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
        ] {
            command.env(secret, "sentinel");
        }
        apply_minimal_service_environment(&mut command);
        let environment = command.get_envs().collect::<Vec<_>>();
        for secret in [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "OPENAI_API_KEY",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
        ] {
            assert!(!environment.iter().any(|(name, value)| {
                *name == secret && value.and_then(|value| value.to_str()) == Some("sentinel")
            }));
        }
    }

    #[test]
    fn service_child_execution_drops_provider_canaries() {
        const STAGE: &str = "CF_PRESENT_SERVICE_CANARY_STAGE";
        const TEST: &str = "cmd::present::tests::service_child_execution_drops_provider_canaries";
        const CANARIES: &[(&str, &str)] = &[
            ("ANTHROPIC_API_KEY", "anthropic-service-canary"),
            ("ANTHROPIC_AUTH_TOKEN", "anthropic-auth-service-canary"),
            ("OPENAI_API_KEY", "openai-service-canary"),
            ("AWS_SECRET_ACCESS_KEY", "aws-service-canary"),
            ("AWS_SESSION_TOKEN", "aws-session-service-canary"),
        ];

        match std::env::var(STAGE).as_deref() {
            Ok("inner") => {
                for (name, value) in CANARIES {
                    assert_ne!(std::env::var(name).as_deref(), Ok(*value));
                }
                println!("service child received no provider canary");
            }
            Ok("outer") => {
                let executable = std::env::current_exe().expect("current test executable");
                let temporary = tempfile::tempdir().expect("service child temporary directory");
                let temporary_root = temporary.path().to_path_buf();
                let profile = temporary.path().join("service-%p.profraw");
                let mut command = Command::new(executable);
                apply_minimal_service_environment(&mut command);
                let output = command
                    .current_dir(temporary.path())
                    // Test instrumentation still needs a task-owned sink. This
                    // is deliberately added after the production environment
                    // restriction and is never part of that allowlist.
                    .env("LLVM_PROFILE_FILE", &profile)
                    .args(["--exact", TEST, "--nocapture"])
                    .env(STAGE, "inner")
                    .output()
                    .expect("execute restricted service child canary");
                assert!(
                    output.status.success(),
                    "service child failed\nstdout: {}\nstderr: {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(String::from_utf8_lossy(&output.stdout)
                    .contains("service child received no provider canary"));
                drop(temporary);
                assert!(
                    !temporary_root.exists(),
                    "service child temporary directory remained"
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
                let output = command.output().expect("execute outer service canary");
                assert!(
                    output.status.success(),
                    "outer service canary failed\nstdout: {}\nstderr: {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }

    #[test]
    fn service_launch_has_one_minimal_command_constructor() {
        // Defense-in-depth source tripwire: executable child canaries remain
        // the authority for the environment actually crossing this boundary.
        let production = compact_rust(
            include_str!("present.rs")
                .split("#[cfg(test)]")
                .next()
                .expect("production present command source"),
        );
        assert_eq!(production.matches("Command::new(").count(), 1);
        assert!(production.contains(
            "letmutcommand=Command::new(executable);apply_minimal_service_environment(&mutcommand);"
        ));
        assert_eq!(
            compact_rust("std::process::Command \n :: new(tool)")
                .matches("Command::new(")
                .count(),
            1,
            "whitespace must not bypass the source tripwire"
        );
    }

    fn compact_rust(source: &str) -> String {
        source
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect()
    }
}
