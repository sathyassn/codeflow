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
    assert!(bootstrapped.starts_with("HTTP/1.1 303 "));
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
    assert!(rotated_response.starts_with("HTTP/1.1 303 "));
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
    let review = format!(
        r#"{{"event_id":"{event_id}","session_id":"{}","revision":1,"verdict":"approve","notes":[]}}"#,
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
    assert!(submitted.starts_with("HTTP/1.1 201 "));
    let delivered = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "feedback", &running.session_id],
    ));
    assert!(delivered.contains(event_id));
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
