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
    service::{serve_session, HealthRecord, ReadyRecord},
    state::{SessionStatus, SessionStore},
    PresentError,
};
use uuid::Uuid;

#[derive(Debug, Args)]
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
        PresentCommand::Close { session_id } => {
            let id = parse_id(session_id)?;
            store.close(id)?;
            println!("closed {id}");
            Ok(())
        }
        PresentCommand::Export {
            session_id,
            out,
            theme,
            mode,
        } => {
            let theme = match theme.as_str() {
                "editorial" => ExportTheme::Editorial,
                "technical" => ExportTheme::Technical,
                _ => unreachable!("clap validates export themes"),
            };
            let mode = match mode.as_str() {
                "system" => ExportMode::System,
                "light" => ExportMode::Light,
                "dark" => ExportMode::Dark,
                _ => unreachable!("clap validates export modes"),
            };
            export_session(&store, parse_id(session_id)?, out, theme, mode)?;
            println!("exported {}", out.display());
            Ok(())
        }
        PresentCommand::Clear {
            session_id,
            older_than,
            dry_run,
        } => {
            let selected = session_id.as_deref().map(parse_id).transpose()?;
            let removed = store.clear(selected, parse_duration(older_than)?, *dry_run)?;
            for id in removed {
                println!("{} {id}", if *dry_run { "would remove" } else { "removed" });
            }
            Ok(())
        }
        PresentCommand::ServeInternal { session_id } => {
            let id = parse_id(session_id)?;
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
            runtime.block_on(serve_session(project, id))
        }
    }
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
        return Ok(());
    }
    let profile = store.runtime_dir(session.id)?.join("browser-profile");
    browser::launch_isolated(store, &ready.bootstrap_path, &profile)?;
    println!("opened {}", session.id);
    Ok(())
}

fn show(store: &SessionStore, id: Uuid, no_launch: bool) -> codeflow_present::Result<()> {
    let session = store.load(id)?;
    if session.status != SessionStatus::Active {
        println!("{}", serde_json::to_string_pretty(&session)?);
        return Ok(());
    }
    let port = session.service_port.ok_or_else(|| {
        PresentError::ServiceUnavailable("the session has no recorded service endpoint".to_string())
    })?;
    let instance_id = session.service_instance.ok_or_else(|| {
        PresentError::ServiceUnavailable("the session has no recorded service identity".to_string())
    })?;
    verify_service(id, port, instance_id)?;
    let authority = format!("127.0.0.1:{port}");
    let profile = store.runtime_dir(id)?.join("browser-profile");
    if no_launch {
        println!("http://{authority}/app/ profile={}", profile.display());
        return Ok(());
    }
    browser::launch_application(store, &authority, &profile)?;
    println!("opened {id}");
    Ok(())
}

fn start_service(store: &SessionStore, id: Uuid) -> codeflow_present::Result<ReadyRecord> {
    let executable = std::env::current_exe()
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    let child = Command::new(executable)
        .arg("present")
        .arg("serve-internal")
        .arg(id.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    let mut child = ChildGuard::new(child);
    let ready_path = store.runtime_dir(id)?.join("ready.json");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if ready_path.is_file() {
            let bytes =
                fs::read(&ready_path).map_err(|error| PresentError::io(&ready_path, error))?;
            let ready: ReadyRecord = serde_json::from_slice(&bytes)?;
            if ready.schema_version != 1 || ready.session_id != id {
                return Err(PresentError::CorruptState(
                    "presentation ready record is mismatched".to_string(),
                ));
            }
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
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    if metadata.len() > codeflow_present::limits::MAX_DOCUMENT_BYTES as u64 {
        return Err(PresentError::DocumentTooLarge {
            limit: codeflow_present::limits::MAX_DOCUMENT_BYTES,
        });
    }
    let file = fs::File::open(path).map_err(|error| PresentError::io(path, error))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((codeflow_present::limits::MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| PresentError::io(path, error))?;
    if bytes.len() > codeflow_present::limits::MAX_DOCUMENT_BYTES {
        return Err(PresentError::DocumentTooLarge {
            limit: codeflow_present::limits::MAX_DOCUMENT_BYTES,
        });
    }
    Ok(bytes)
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
        PresentError::SessionClosed(_) | PresentError::Io { .. } => 1,
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
}
