#![cfg(unix)]

use std::{
    fs,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[path = "support/json_schema.rs"]
mod json_schema;

fn codeflow(project: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .current_dir(project)
        .env("HOME", home)
        .env("XDG_STATE_HOME", home.join("state"))
        .output()
        .unwrap()
}

fn require_success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "command failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn http(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect_timeout(
        &SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into(),
        Duration::from_secs(2),
    )
    .unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    stream
        .take(2 * 1024 * 1024)
        .read_to_end(&mut response)
        .unwrap();
    String::from_utf8(response).unwrap()
}

fn between<'a>(value: &'a str, prefix: &str, suffix: &str) -> &'a str {
    value
        .split_once(prefix)
        .and_then(|(_, rest)| rest.split_once(suffix).map(|(match_, _)| match_))
        .unwrap()
}

struct TestProject {
    _temp: tempfile::TempDir,
    project: PathBuf,
    home: PathBuf,
    second: PathBuf,
}

struct RunningPresentation {
    session_id: String,
    bootstrap_path: PathBuf,
    ready_path: PathBuf,
    authority: String,
    port: u16,
    capability: String,
    cookie: String,
    application: String,
}

#[test]
fn present_cli_runs_the_local_service_revision_export_and_cleanup_flow() {
    let fixture = setup_project();
    let running = open_recover_and_bootstrap(&fixture);
    let (runtime_session, stop_writer, writer) = start_profile_writer(&fixture, &running);
    verify_runtime_boundaries(&fixture, &running);
    update_export_close_and_clear(&fixture, &running, &runtime_session);
    stop_writer.store(true, Ordering::Release);
    writer.join().unwrap();
}

#[test]
fn crashed_service_close_then_selected_clear_converges() {
    let fixture = setup_project();
    let opened = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "open",
            fixture.project.join("first.json").to_str().unwrap(),
            "--no-launch",
        ],
    ));
    let session_id = opened.split_whitespace().nth(1).unwrap().to_string();
    let listed: serde_json::Value = serde_json::from_str(&require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "list"],
    )))
    .unwrap();
    let pid = i32::try_from(listed[0]["service_pid"].as_u64().unwrap()).unwrap();
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0);

    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "close", &session_id],
    ));
    let cleared = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "clear", &session_id, "--older-than", "0h"],
    ));
    assert!(cleared.contains(&format!("removed {session_id}")));
}

fn start_profile_writer(
    fixture: &TestProject,
    running: &RunningPresentation,
) -> (PathBuf, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    #[cfg(target_os = "macos")]
    let runtime_projects = fixture
        .home
        .join("Library/Application Support/codeflow/present/runtime/projects");
    #[cfg(all(unix, not(target_os = "macos")))]
    let runtime_projects = fixture.home.join("state/codeflow/present/runtime/projects");
    let project = fs::read_dir(runtime_projects)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let runtime_session = project.join(&running.session_id);
    let profile = runtime_session.join("browser-profile");
    let cache = profile.join("live-cache.bin");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(cache)
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker = thread::spawn(move || {
        while !worker_stop.load(Ordering::Acquire) {
            file.write_all(b"browser-owned-cache\n").unwrap();
            thread::yield_now();
        }
    });
    (runtime_session, stop, worker)
}

fn setup_project() -> TestProject {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let home = temp.path().join("home");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&home).unwrap();
    assert!(Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&project)
        .status()
        .unwrap()
        .success());

    let first = project.join("first.json");
    let second = project.join("second.json");
    fs::write(
        &first,
        r#"{"schema_version":1,"title":"First review","provenance":{"task_id":"TSK-011","spec_id":"SPC-004","adr_id":"ADR-0050"},"blocks":[{"type":"narrative","id":"summary","markdown":"First revision"}]}"#,
    )
    .unwrap();
    fs::write(
        &second,
        r#"{"schema_version":1,"title":"Second review","provenance":{"task_id":"TSK-011","spec_id":"SPC-004","adr_id":"ADR-0050"},"blocks":[{"type":"narrative","id":"summary","markdown":"Second revision"}]}"#,
    )
    .unwrap();
    TestProject {
        _temp: temp,
        project,
        home,
        second,
    }
}

fn open_recover_and_bootstrap(fixture: &TestProject) -> RunningPresentation {
    let opened = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "open",
            fixture.project.join("first.json").to_str().unwrap(),
            "--no-launch",
        ],
    ));
    let session_id = opened.split_whitespace().nth(1).unwrap().to_string();
    let bootstrap_path = PathBuf::from(between(
        &opened,
        "owner-private bootstrap file ",
        " in a qualified",
    ));
    let ready_path = bootstrap_path.parent().unwrap().join("ready.json");
    let first_ready: serde_json::Value =
        serde_json::from_slice(&fs::read(&ready_path).unwrap()).unwrap();
    let first_instance = first_ready["instance_id"].as_str().unwrap().to_string();
    let listed: serde_json::Value = serde_json::from_str(&require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "list"],
    )))
    .unwrap();
    let first_pid = i32::try_from(listed[0]["service_pid"].as_u64().unwrap()).unwrap();
    assert_eq!(unsafe { libc::kill(first_pid, libc::SIGKILL) }, 0);
    let crash_deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(first_pid, 0) } == 0 && Instant::now() < crash_deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert_ne!(unsafe { libc::kill(first_pid, 0) }, 0);

    let recovered = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "show", &session_id, "--no-launch"],
    ));
    assert!(recovered.contains("recovered"));
    let second_ready: serde_json::Value =
        serde_json::from_slice(&fs::read(&ready_path).unwrap()).unwrap();
    assert_ne!(second_ready["instance_id"], first_instance);
    let bootstrap = fs::read_to_string(&bootstrap_path).unwrap();
    let authority = between(&bootstrap, "action=\"http://", "/bootstrap\"");
    let port = authority.split_once(':').unwrap().1.parse::<u16>().unwrap();
    let capability = between(&bootstrap, "name=\"capability\" value=\"", "\"");
    let form = format!("capability={capability}");
    let bootstrapped = http(
        port,
        &format!(
            "POST /bootstrap HTTP/1.1\r\nHost: {authority}\r\nOrigin: null\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{form}",
            form.len()
        ),
    );
    assert!(bootstrapped.starts_with("HTTP/1.1 200 "));
    assert!(bootstrapped.contains("location.replace('/app/')"));
    assert!(!bootstrapped.contains(capability));
    let cookie = bootstrapped
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("set-cookie")
                    .then(|| value.trim().split(';').next().unwrap().to_string())
            })
        })
        .unwrap();
    assert!(!bootstrap_path.exists());
    let application = http(
        port,
        &format!(
            "GET /app/ HTTP/1.1\r\nHost: {authority}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(application.starts_with("HTTP/1.1 200 "));
    assert!(application.contains("First revision"));
    assert!(application.contains("\"revision\":1"));
    RunningPresentation {
        session_id,
        bootstrap_path,
        ready_path,
        authority: authority.to_string(),
        port,
        capability: capability.to_string(),
        cookie,
        application,
    }
}

fn verify_runtime_boundaries(fixture: &TestProject, running: &RunningPresentation) {
    let reopened = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "show", &running.session_id, "--no-launch"],
    ));
    assert!(reopened.contains("owner-private bootstrap file"));
    let rotated_bootstrap = fs::read_to_string(&running.bootstrap_path).unwrap();
    let rotated_capability = between(&rotated_bootstrap, "name=\"capability\" value=\"", "\"");
    assert_ne!(rotated_capability, running.capability);
    let rotated_form = format!("capability={rotated_capability}");
    let rotated_response = http(
        running.port,
        &format!(
            "POST /bootstrap HTTP/1.1\r\nHost: {}\r\nOrigin: null\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{rotated_form}",
            running.authority,
            rotated_form.len()
        ),
    );
    assert!(rotated_response.starts_with("HTTP/1.1 200 "));
    assert!(rotated_response.contains("location.replace('/app/')"));
    assert!(!rotated_response.contains(rotated_capability));
    assert!(!running.bootstrap_path.exists());
    let wrong_host = http(
        running.port,
        &format!(
            "GET /app/ HTTP/1.1\r\nHost: localhost:{}\r\nCookie: {}\r\nConnection: close\r\n\r\n",
            running.port, running.cookie
        ),
    );
    assert!(wrong_host.starts_with("HTTP/1.1 421 "));
    let no_cookie = http(
        running.port,
        &format!(
            "GET /app/ HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            running.authority
        ),
    );
    assert!(no_cookie.starts_with("HTTP/1.1 401 "));
    let asset_path = format!(
        "/app/assets/{}",
        between(&running.application, "/app/assets/", "\"")
    );
    let missing_brotli = http(
        running.port,
        &format!(
            "GET {asset_path} HTTP/1.1\r\nHost: {}\r\nCookie: {}\r\nConnection: close\r\n\r\n",
            running.authority, running.cookie
        ),
    );
    assert!(missing_brotli.starts_with("HTTP/1.1 406 "));

    let event_id = "019f9b53-a341-7fa7-84c2-5f198ceea099";
    let note_id = "019f9b53-a341-7fa7-84c2-5f198ceea100";
    let review = format!(
        r#"{{"event_id":"{event_id}","session_id":"{}","revision":1,"verdict":"approve","notes":[{{"client_id":"{note_id}","block_id":"summary","block_label":"First revision","kind":"comment","body":"Keep the quote","excerpt":{{"text":"First revision"}}}}]}}"#,
        running.session_id
    );
    let submitted = http(
        running.port,
        &format!(
            "POST /app/api/reviews HTTP/1.1\r\nHost: {}\r\nOrigin: http://{}\r\nCookie: {}\r\nX-CF-Present: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{review}",
            running.authority,
            running.authority,
            running.cookie,
            review.len()
        ),
    );
    assert!(
        submitted.starts_with("HTTP/1.1 201 "),
        "review submit failed: {}",
        submitted.chars().take(800).collect::<String>()
    );
    let delivered = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "feedback", &running.session_id],
    ));
    assert!(delivered.contains(event_id));
    assert!(delivered.contains("First revision"));
    assert!(delivered.contains("\"excerpt\""));
    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "resolve",
            &running.session_id,
            event_id,
            "--event-version",
            "2",
            "--status",
            "addressed",
        ],
    ));
    let history = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "history", &running.session_id],
    ));
    assert!(history.contains("\"event\": \"addressed\""));
}

fn update_export_close_and_clear(
    fixture: &TestProject,
    running: &RunningPresentation,
    runtime_session: &Path,
) {
    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "update",
            &running.session_id,
            fixture.second.to_str().unwrap(),
        ],
    ));
    let poll_body = r#"{"cursor":"1:0"}"#;
    let wrong_origin = http(
        running.port,
        &format!(
            "POST /app/api/events/poll HTTP/1.1\r\nHost: {}\r\nOrigin: https://example.com\r\nCookie: {}\r\nX-CF-Present: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{poll_body}",
            running.authority,
            running.cookie,
            poll_body.len()
        ),
    );
    assert!(wrong_origin.starts_with("HTTP/1.1 403 "));
    let poll = http(
        running.port,
        &format!(
            "POST /app/api/events/poll HTTP/1.1\r\nHost: {}\r\nOrigin: http://{}\r\nCookie: {}\r\nX-CF-Present: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{poll_body}",
            running.authority,
            running.authority,
            running.cookie,
            poll_body.len()
        ),
    );
    assert!(poll.starts_with("HTTP/1.1 200 "));
    assert!(poll.contains("\"kind\":\"revision\""));
    assert!(poll.contains("\"cursor\":\"2:3\""));

    let exported_path = fixture.project.join("review.html");
    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "export",
            &running.session_id,
            "--out",
            exported_path.to_str().unwrap(),
            "--theme",
            "technical",
            "--mode",
            "dark",
        ],
    ));
    let exported = fs::read_to_string(&exported_path).unwrap();
    assert!(exported.contains("Second revision"));
    assert!(exported.contains("data-cf-theme=\"technical\""));
    assert!(exported.contains("data-cf-mode=\"dark\""));
    for private in [
        running.capability.as_str(),
        running.cookie.as_str(),
        "/app/api/reviews",
        "browser-profile",
    ] {
        assert!(!exported.contains(private));
    }

    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "close", &running.session_id],
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    while running.ready_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    assert!(!running.ready_path.exists());

    let cleared = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "clear",
            &running.session_id,
            "--older-than",
            "0h",
        ],
    ));
    assert!(cleared.contains(&format!("removed {}", running.session_id)));
    assert!(!runtime_session.exists());
    let sessions = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "list"],
    ));
    assert_eq!(sessions.trim(), "[]");
}

/// Runs `codeflow` the way an agent sandbox does: only `HOME` locates state,
/// with no `XDG_STATE_HOME` override, so the platform default root is used.
fn codeflow_default_state(cwd: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env_remove("XDG_STATE_HOME")
        .output()
        .unwrap()
}

/// The platform's sandbox write root from the shipped settings preset, with
/// `~` expanded against the test home.
fn preset_present_state_root(home: &Path) -> PathBuf {
    let preset =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/settings/default.json");
    let preset: serde_json::Value = serde_json::from_slice(&fs::read(preset).unwrap()).unwrap();
    let suffix = if cfg!(target_os = "macos") {
        "Library/Application Support/codeflow/present"
    } else {
        ".local/state/codeflow/present"
    };
    let entry = preset["sandbox"]["filesystem"]["allowWrite"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .find(|entry| entry.strip_prefix("~/") == Some(suffix))
        .unwrap_or_else(|| panic!("the settings preset must allow writes to ~/{suffix}"));
    home.join(entry.strip_prefix("~/").unwrap())
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap();
            decoded.push(u8::from_str_radix(hex, 16).unwrap());
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).unwrap()
}

fn entries_below(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                pending.push(path.clone());
            }
            found.push(path);
        }
    }
    found
}

#[test]
fn sandboxed_open_keeps_state_in_the_allowed_root_and_prints_a_handoff_link() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = setup_project();
    let state_root = preset_present_state_root(&fixture.home);
    let opened = require_success(&codeflow_default_state(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "open",
            fixture.project.join("first.json").to_str().unwrap(),
            "--no-launch",
        ],
    ));
    let session_id = opened.split_whitespace().nth(1).unwrap().to_string();
    let bootstrap_path = PathBuf::from(between(
        &opened,
        "owner-private bootstrap file ",
        " in a qualified",
    ));

    // AC-2: an openable file link to the same single-use bootstrap page.
    let link_line = opened
        .lines()
        .find(|line| line.starts_with("handoff link (single use, open within 120 seconds): "))
        .unwrap_or_else(|| panic!("open printed no handoff link:\n{opened}"));
    let link = link_line.rsplit_once(": ").unwrap().1;
    assert!(link.starts_with("file:///"), "{link}");
    assert!(
        !link.contains(' '),
        "the link must be percent-encoded: {link}"
    );
    assert_eq!(
        PathBuf::from(percent_decode(link.strip_prefix("file://").unwrap())),
        bootstrap_path
    );
    let bootstrap = fs::read_to_string(&bootstrap_path).unwrap();
    assert!(bootstrap.contains("action=\"http://127.0.0.1:"));

    // AC-1 and AC-4: all state is under the one root the sandbox preset allows,
    // owner-private, and nothing else under HOME is written.
    assert!(
        bootstrap_path.starts_with(&state_root),
        "{}",
        bootstrap_path.display()
    );
    for path in entries_below(&fixture.home) {
        assert!(
            path.starts_with(&state_root) || state_root.starts_with(&path),
            "present wrote outside the allowed state root: {}",
            path.display()
        );
        if path.starts_with(&state_root) {
            let mode = fs::symlink_metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "{} is not owner-private", path.display());
        }
    }

    // Session identity is scoped to the project working tree: update works there
    // and fails with guidance from a directory outside any repository.
    require_success(&codeflow_default_state(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "update",
            &session_id,
            fixture.second.to_str().unwrap(),
        ],
    ));
    let outside = codeflow_default_state(
        &fixture.home,
        &fixture.home,
        &[
            "present",
            "update",
            &session_id,
            fixture.second.to_str().unwrap(),
        ],
    );
    assert!(!outside.status.success());
    assert!(String::from_utf8_lossy(&outside.stderr)
        .contains("run codeflow present from the project's working tree"));

    // Cleanup removes both the durable session and its derived runtime.
    require_success(&codeflow_default_state(
        &fixture.project,
        &fixture.home,
        &["present", "close", &session_id],
    ));
    let ready_path = bootstrap_path.parent().unwrap().join("ready.json");
    let deadline = Instant::now() + Duration::from_secs(5);
    while ready_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    let cleared = require_success(&codeflow_default_state(
        &fixture.project,
        &fixture.home,
        &["present", "clear", &session_id, "--older-than", "0h"],
    ));
    assert!(cleared.contains(&format!("removed {session_id}")));
    let remaining: Vec<PathBuf> = entries_below(&state_root)
        .into_iter()
        .filter(|path| path.to_string_lossy().contains(&session_id))
        .collect();
    assert!(remaining.is_empty(), "session state remains: {remaining:?}");
}

/// Runs a scaffold command with a fresh `HOME` and isolated Git and registry
/// state, as an operator would outside the agent sandbox.
fn scaffold_command(project: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .current_dir(project)
        .env("HOME", home)
        .env("CODEFLOW_HOME", home.join(".codeflow"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("XDG_STATE_HOME")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap()
}

fn assert_owner_private_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::symlink_metadata(path)
        .unwrap_or_else(|error| panic!("{} was not created: {error}", path.display()));
    assert!(metadata.is_dir(), "{} is not a directory", path.display());
    assert_eq!(
        metadata.permissions().mode() & 0o077,
        0,
        "{} is not owner-private",
        path.display()
    );
}

#[test]
fn init_and_update_provision_the_present_state_root_in_a_fresh_home() {
    let fixture = setup_project();
    let state_root = preset_present_state_root(&fixture.home);
    assert!(!state_root.exists());

    require_success(&scaffold_command(
        &fixture.project,
        &fixture.home,
        &["init", "--minimal", "--yes"],
    ));
    assert_owner_private_dir(&state_root);

    // `update` recreates a root removed after `init`.
    let codeflow_dir = state_root.parent().unwrap();
    fs::remove_dir_all(codeflow_dir).unwrap();
    require_success(&scaffold_command(
        &fixture.project,
        &fixture.home,
        &["update"],
    ));
    assert_owner_private_dir(&state_root);
    assert_owner_private_dir(codeflow_dir);

    // A provisioned root is all a sandboxed session needs for first use.
    require_success(&codeflow_default_state(
        &fixture.project,
        &fixture.home,
        &["present", "list"],
    ));
}

#[test]
fn first_use_without_a_writable_state_parent_names_the_provisioning_command() {
    use std::os::unix::fs::PermissionsExt;

    if unsafe { libc::geteuid() } == 0 {
        // Root ignores directory permissions, so the denial cannot be staged.
        return;
    }
    let fixture = setup_project();
    let state_root = preset_present_state_root(&fixture.home);
    // Stage what the sandbox allowance leaves: the nearest existing ancestor
    // of the state root is not writable, so its missing parents cannot be made.
    let writable_limit = if cfg!(target_os = "macos") {
        fixture.home.join("Library/Application Support")
    } else {
        fixture.home.join(".local/state")
    };
    fs::create_dir_all(&writable_limit).unwrap();
    fs::set_permissions(&writable_limit, fs::Permissions::from_mode(0o555)).unwrap();

    let refused = codeflow_default_state(&fixture.project, &fixture.home, &["present", "list"]);
    fs::set_permissions(&writable_limit, fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(refused.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains(&format!(
            "the cf-present state directory {} is missing and could not be created",
            state_root.display()
        )),
        "{stderr}"
    );
    assert!(
        stderr.contains("run `codeflow update` once outside the agent sandbox to create it"),
        "{stderr}"
    );
    assert!(!state_root.exists());
}

const RETIRED_REVISION: &str =
    include_str!("../../codeflow-present/tests/fixtures/retired-diagram/revision.json");
const RETIRED_EVENTS: &str =
    include_str!("../../codeflow-present/tests/fixtures/retired-diagram/events.jsonl");
const RETIRED_CAPTURED_SESSION: &str = "c17874f5-9568-45f6-a657-180848fae57d";

fn schema_registry() -> json_schema::Registry {
    let schemas = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/present/schemas");
    json_schema::Registry::new(
        ["document-v1.schema.json", "session-history-v1.schema.json"].map(|name| {
            serde_json::from_slice::<serde_json::Value>(&fs::read(schemas.join(name)).unwrap())
                .unwrap()
        }),
    )
}

fn failure(output: &Output) -> String {
    assert!(
        !output.status.success(),
        "command succeeded\nstdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn open_no_launch(fixture: &TestProject, document: &Path) -> (String, String) {
    let opened = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "open", document.to_str().unwrap(), "--no-launch"],
    ));
    (
        opened.split_whitespace().nth(1).unwrap().to_string(),
        opened,
    )
}

fn close_and_clear(fixture: &TestProject, session_id: &str) {
    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "close", session_id],
    ));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let cleared = codeflow(
            &fixture.project,
            &fixture.home,
            &["present", "clear", session_id, "--older-than", "0h"],
        );
        if cleared.status.success() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "clear did not converge: {cleared:?}"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

fn session_dir(fixture: &TestProject, session_id: &str) -> PathBuf {
    fn find(root: &Path, session_id: &str) -> Option<PathBuf> {
        for entry in fs::read_dir(root).ok()?.flatten() {
            let path = entry.path();
            if !entry.file_type().ok()?.is_dir() {
                continue;
            }
            if path.file_name().is_some_and(|name| name == session_id)
                && path
                    .parent()
                    .is_some_and(|parent| parent.ends_with("sessions"))
            {
                return Some(path);
            }
            if let Some(found) = find(&path, session_id) {
                return Some(found);
            }
        }
        None
    }
    find(&fixture.home, session_id).expect("session state directory")
}

/// Put the captured pre-removal revision and its feedback log on disk under
/// an open session, as a pre-release build left them.
fn install_retired_revision(fixture: &TestProject, session_id: &str, revision: &str) -> PathBuf {
    let directory = session_dir(fixture, session_id);
    let path = directory.join("revisions/00000000000000000001.json");
    fs::write(&path, revision).unwrap();
    fs::write(
        directory.join("events.jsonl"),
        RETIRED_EVENTS.replace(RETIRED_CAPTURED_SESSION, session_id),
    )
    .unwrap();
    path
}

fn bootstrap_cookie(bootstrap_output: &str) -> (u16, String, String) {
    let bootstrap_path = PathBuf::from(between(
        bootstrap_output,
        "owner-private bootstrap file ",
        " in ",
    ));
    let bootstrap = fs::read_to_string(bootstrap_path).unwrap();
    let authority = between(&bootstrap, "action=\"http://", "/bootstrap\"").to_string();
    let port = authority.split_once(':').unwrap().1.parse::<u16>().unwrap();
    let form = format!(
        "capability={}",
        between(&bootstrap, "name=\"capability\" value=\"", "\"")
    );
    let response = http(
        port,
        &format!(
            "POST /bootstrap HTTP/1.1\r\nHost: {authority}\r\nOrigin: null\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{form}",
            form.len()
        ),
    );
    assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
    let cookie = response
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("set-cookie")
                    .then(|| value.trim().split(';').next().unwrap().to_string())
            })
        })
        .unwrap();
    (port, authority, cookie)
}

fn application_page(port: u16, authority: &str, cookie: &str) -> String {
    let page = http(
        port,
        &format!(
            "GET /app/ HTTP/1.1\r\nHost: {authority}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(page.starts_with("HTTP/1.1 200 "), "{page}");
    page
}

fn assert_retired_page(html: &str) {
    assert!(
        html.contains("This revision holds a diagram block, which was removed with Mermaid"),
        "{html}"
    );
    assert!(html
        .contains("<pre><code>flowchart LR\n  Input --&gt; Review --&gt; Evidence</code></pre>"));
    assert!(html.contains(
        "Former sequence diagram; convert it to an html block holding an inline SVG, or a table of the messages in order."
    ));
    for absent in ["data-cf-diagram", "<script", "type=\"module\"", "pending"] {
        assert!(!html.contains(absent), "retired page carries {absent}");
    }
}

#[test]
fn present_open_and_update_refuse_a_diagram_block_and_name_its_conversion() {
    let fixture = setup_project();
    let retired = fixture.project.join("retired.json");
    fs::write(
        &retired,
        r#"{"schema_version":1,"title":"Retired","blocks":[{"type":"tabs","id":"views","tabs":[{"label":"States","blocks":[
          {"type":"diagram","id":"lifecycle","kind":"state","source":"stateDiagram-v2\n  [*] --> Open","acc_title":"Lifecycle","acc_description":"A session opens."}
        ]}]}]}"#,
    )
    .unwrap();
    let assert_refusal = |output: &Output| {
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        let message = String::from_utf8(output.stderr.clone()).unwrap();
        for expected in [
            "block \"lifecycle\" is a diagram block, which was removed with Mermaid",
            "convert its state to an html block holding an inline SVG, or a table of the transitions",
            "\"Converting a diagram block\" section of cf-present/references/document-authoring.md",
        ] {
            assert!(message.contains(expected), "{message}");
        }
    };
    assert_refusal(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "open", retired.to_str().unwrap(), "--no-launch"],
    ));
    let listed = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "list"],
    ));
    assert_eq!(listed.trim(), "[]", "a refused open created a session");

    let (session_id, _) = open_no_launch(&fixture, &fixture.project.join("first.json"));
    assert_refusal(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "update", &session_id, retired.to_str().unwrap()],
    ));
    let history: serde_json::Value = serde_json::from_str(&require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "history", &session_id],
    )))
    .unwrap();
    assert_eq!(history["session"]["current_revision"], 1);
    close_and_clear(&fixture, &session_id);
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_stored_diagram_revision_stays_usable_through_every_present_command() {
    let fixture = setup_project();
    let registry = schema_registry();
    let history_schema = "urn:codeflow:schema:present:session-history:1";
    let (session_id, opened) = open_no_launch(&fixture, &fixture.project.join("first.json"));
    let revision_path = install_retired_revision(&fixture, &session_id, RETIRED_REVISION);
    let run = |args: &[&str]| require_success(&codeflow(&fixture.project, &fixture.home, args));

    let listed: serde_json::Value = serde_json::from_str(&run(&["present", "list"])).unwrap();
    assert_eq!(listed[0]["id"], session_id.as_str());

    // The served page and a new export show the notice and each source.
    let (port, authority, cookie) = bootstrap_cookie(&opened);
    let page = application_page(port, &authority, &cookie);
    assert_retired_page(&page);
    assert!(page.contains("style-src-elem 'self' 'unsafe-inline'; style-src-attr 'unsafe-inline'"));
    let shown = run(&["present", "show", &session_id, "--no-launch"]);
    assert!(shown.contains("owner-private bootstrap file"), "{shown}");
    let exported = fixture.project.join("retired.html");
    run(&[
        "present",
        "export",
        &session_id,
        "--out",
        exported.to_str().unwrap(),
    ]);
    assert_retired_page(&fs::read_to_string(&exported).unwrap());

    let history: serde_json::Value =
        serde_json::from_str(&run(&["present", "history", &session_id])).unwrap();
    assert_eq!(
        registry.errors(history_schema, &history),
        Vec::<String>::new()
    );
    let mut tampered = history.clone();
    tampered["revisions"][0]["content"]["diagram_ids"] = serde_json::json!([]);
    assert_eq!(
        registry.errors(history_schema, &tampered),
        ["$.revisions[0].content: 0 oneOf branches matched"],
        "the schema check must fail a retired kind without diagram ids"
    );
    assert_eq!(history["revisions"][0]["content"]["kind"], "retired");
    assert_eq!(
        history["revisions"][0]["content"]["diagram_ids"],
        serde_json::json!(["flow", "handshake"])
    );

    // The recorded note is delivered with its block, selector and state.
    let delivered: serde_json::Value =
        serde_json::from_str(run(&["present", "feedback", &session_id]).trim()).unwrap();
    let event_id = delivered["event_id"].as_str().unwrap().to_string();
    assert_eq!(delivered["notes"][0]["block_id"], "flow");
    assert_eq!(delivered["notes"][0]["selector"]["exact"], "Review");
    assert_eq!(delivered["notes"][0]["selector"]["start_utf16"], 25);
    let history: serde_json::Value =
        serde_json::from_str(&run(&["present", "history", &session_id])).unwrap();
    assert_eq!(history["feedback_events"][1]["event"], "delivered");
    run(&[
        "present",
        "resolve",
        &session_id,
        &event_id,
        "--event-version",
        "2",
        "--status",
        "addressed",
    ]);
    assert_eq!(
        fs::read_to_string(&revision_path).unwrap(),
        RETIRED_REVISION
    );

    // A converted update succeeds; the old note orphans on the removed block.
    let converted = fixture.project.join("converted.json");
    fs::write(
        &converted,
        r#"{"schema_version":1,"title":"Qualification review","blocks":[
          {"type":"narrative","id":"summary","markdown":"The qualification path, converted."},
          {"type":"html","id":"flow","title":"Qualification flow","html":"<figure role='img' aria-label='Input moves through review to evidence'><svg viewBox='0 0 300 40'><text x='0' y='24'>Input, Review, Evidence</text></svg></figure>"}
        ]}"#,
    )
    .unwrap();
    run(&[
        "present",
        "update",
        &session_id,
        converted.to_str().unwrap(),
    ]);
    let page = application_page(port, &authority, &cookie);
    assert!(
        page.contains("<text x=\"0\" y=\"24\">Input, Review, Evidence</text>"),
        "{page}"
    );
    assert!(page.contains(
        "the diagram block flow was removed with Mermaid; convert it to reanchor this note"
    ));
    let history: serde_json::Value =
        serde_json::from_str(&run(&["present", "history", &session_id])).unwrap();
    assert_eq!(
        registry.errors(history_schema, &history),
        Vec::<String>::new()
    );
    assert_eq!(history["revisions"][1]["content"]["kind"], "supported");
    assert_eq!(
        fs::read_to_string(&revision_path).unwrap(),
        RETIRED_REVISION
    );

    close_and_clear(&fixture, &session_id);
    assert_eq!(run(&["present", "list"]).trim(), "[]");
}

#[test]
fn broken_stored_revisions_fail_with_their_own_error() {
    let fixture = setup_project();
    let stored: serde_json::Value = serde_json::from_str(RETIRED_REVISION).unwrap();
    let variant = |change: &dyn Fn(&mut serde_json::Value)| {
        let mut record = stored.clone();
        change(&mut record);
        serde_json::to_string_pretty(&record).unwrap()
    };
    // Duplicate keys beside a valid legacy diagram, in the envelope, the
    // document and the diagram block.
    let duplicate = |old: &str, new: &str| {
        assert_eq!(RETIRED_REVISION.matches(old).count(), 1, "{old}");
        RETIRED_REVISION.replacen(old, new, 1)
    };
    for (name, revision, expected) in [
        (
            "duplicate revision",
            duplicate(
                "\n  \"revision\": 1,",
                "\n  \"revision\": 2,\n  \"revision\": 1,",
            ),
            "duplicate field `revision`",
        ),
        (
            "duplicate title",
            duplicate(
                "\"title\": \"Qualification review\",",
                "\"title\": \"Forged\", \"title\": \"Qualification review\",",
            ),
            "duplicate field `title`",
        ),
        (
            "duplicate source",
            duplicate(
                "\"source\": \"flowchart LR",
                "\"source\": \"graph TD\", \"source\": \"flowchart LR",
            ),
            "duplicate field `source`",
        ),
        (
            "truncated",
            RETIRED_REVISION[..RETIRED_REVISION.len() / 2].to_string(),
            "EOF while parsing",
        ),
        (
            "unknown block",
            variant(&|record| {
                record["content"]["document"]["blocks"][0]["type"] = "sketch".into();
            }),
            "unknown variant `sketch`",
        ),
        (
            "malformed diagram",
            variant(&|record| {
                record["content"]["document"]["blocks"][1]["kind"] = "gantt".into();
            }),
            "unknown variant `diagram`",
        ),
    ] {
        let (session_id, _) = open_no_launch(&fixture, &fixture.project.join("first.json"));
        install_retired_revision(&fixture, &session_id, &revision);
        let message = failure(&codeflow(
            &fixture.project,
            &fixture.home,
            &["present", "history", &session_id],
        ));
        assert!(message.contains(expected), "{name}: {message}");
        let exported = fixture.project.join(format!("{name}.html"));
        let message = failure(&codeflow(
            &fixture.project,
            &fixture.home,
            &[
                "present",
                "export",
                &session_id,
                "--out",
                exported.to_str().unwrap(),
            ],
        ));
        assert!(message.contains(expected), "{name}: {message}");
        assert!(!exported.exists(), "{name}");
        close_and_clear(&fixture, &session_id);
    }
}

fn skill_file(path: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/base/agents/skills/cf-present")
            .join(path),
    )
    .unwrap()
}

#[test]
fn shipped_example_documents_parse_and_match_the_document_schema() {
    let registry = schema_registry();
    let path = "resources/present-document.example.json";
    let text = skill_file(path);
    let parsed = codeflow_present::document::parse_document(text.as_bytes())
        .unwrap_or_else(|error| panic!("{path}: {error}"));
    assert!(
        matches!(
            parsed,
            codeflow_present::document::ParsedDocument::Supported(_)
        ),
        "{path}"
    );
    let example: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        registry.errors("urn:codeflow:schema:present:document:1", &example),
        Vec::<String>::new(),
        "{path}"
    );
    assert!(
        !text.contains("\"diagram\""),
        "{path} still carries a diagram"
    );
    // The example leads with the conversion the refusal names first: an
    // html block holding an inline SVG.
    assert_eq!(example["blocks"][0]["type"], "html");
    assert!(example["blocks"][0]["html"]
        .as_str()
        .is_some_and(|html| html.contains("<svg")));

    // The narrowed schema refuses the removed block.
    let mut retired = example.clone();
    retired["blocks"][0] = serde_json::json!({
        "type": "diagram", "id": "flow", "kind": "flowchart", "source": "flowchart LR",
        "acc_title": "Flow", "acc_description": "A flow."
    });
    assert!(!registry
        .errors("urn:codeflow:schema:present:document:1", &retired)
        .is_empty());
}

/// Every block a refusal names is one this source accepts: each named
/// replacement parses as a document on its own (TSK-114 AC-3).
#[test]
fn each_conversion_the_refusal_names_is_a_block_this_source_accepts() {
    use codeflow_present::retired::{replacement, CONVERSIONS};

    let samples = [
        (
            "an html block holding an inline SVG",
            r#"{"type":"html","id":"converted","title":"Flow","html":"<figure role='img' aria-label='A flow'><svg viewBox='0 0 10 10'><rect width='4' height='4'></rect></svg></figure>"}"#,
        ),
        (
            "a table",
            r#"{"type":"table","id":"converted","columns":["From","To"],"rows":[["Open","Closed"]]}"#,
        ),
        (
            "a tree block",
            r#"{"type":"tree","id":"converted","label":"Topics","nodes":[{"label":"Root"}]}"#,
        ),
    ];
    let named: Vec<&str> = CONVERSIONS
        .iter()
        .map(|(_, named)| *named)
        .chain([replacement(None)])
        .collect();
    let mut used = std::collections::BTreeSet::new();
    for named in named {
        let matched: Vec<_> = samples
            .iter()
            .filter(|(block, _)| named.contains(block))
            .collect();
        assert!(
            !matched.is_empty(),
            "{named} names no block this source has"
        );
        used.extend(matched.iter().map(|(block, _)| *block));
    }
    assert_eq!(used.len(), samples.len(), "a sample no conversion names");
    for (block, sample) in samples {
        let document = format!(r#"{{"schema_version":1,"title":"Converted","blocks":[{sample}]}}"#);
        let parsed = codeflow_present::document::parse_document(document.as_bytes())
            .unwrap_or_else(|error| panic!("{block}: {error}"));
        assert!(
            matches!(
                parsed,
                codeflow_present::document::ParsedDocument::Supported(_)
            ),
            "{block}"
        );
    }
}

/// The section the refusal names teaches every former kind with the same
/// replacement the runtime names, and points at a complete converted block.
#[test]
fn the_conversion_section_matches_the_runtime_refusal() {
    use codeflow_present::retired::{CONVERSIONS, CONVERSION_GUIDE};

    let text = skill_file("references/document-authoring.md");
    let heading = between(CONVERSION_GUIDE, "the \"", "\" section");
    assert!(CONVERSION_GUIDE.ends_with("cf-present/references/document-authoring.md"));
    let section = text
        .split(&format!("\n## {heading}\n"))
        .nth(1)
        .unwrap_or_else(|| panic!("document-authoring.md has no {heading} section"))
        .split("\n## ")
        .next()
        .unwrap()
        .replace('`', "");
    for (kind, replacement) in CONVERSIONS {
        assert!(
            section.contains(&format!("| {kind} | {replacement} |")),
            "the conversion section lost {kind}: {replacement}"
        );
    }
    assert!(
        section.contains("resources/present-document.example.json"),
        "the conversion section lost its complete example"
    );
    assert!(!text.contains("| diagram |") && !text.contains("\"type\": \"diagram\""));
}
