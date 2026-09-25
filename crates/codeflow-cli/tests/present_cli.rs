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

fn canonical_theme_name_matches_the_alias(
    fixture: &TestProject,
    running: &RunningPresentation,
    alias_export: &str,
) {
    let path = fixture.project.join("review-instrument.html");
    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "export",
            &running.session_id,
            "--out",
            path.to_str().unwrap(),
            "--theme",
            "instrument",
            "--mode",
            "dark",
        ],
    ));
    let canonical = fs::read_to_string(&path).unwrap();
    assert!(canonical.contains("data-cf-theme=\"instrument\""));
    assert_eq!(
        canonical, alias_export,
        "technical and instrument export the same skin"
    );
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
    // `technical` is the documented alias of the instrument skin: it stays
    // accepted and resolves to the same skin the canonical name resolves to.
    assert!(exported.contains("data-cf-theme=\"instrument\""));
    assert!(exported.contains("data-cf-mode=\"dark\""));

    canonical_theme_name_matches_the_alias(fixture, running, &exported);
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
    assert!(html.contains("Former sequence diagram; convert it to a sequence figure."));
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
            "convert its state to a state figure",
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
          {"type":"figure","id":"flow","declaration":{"schema_version":1,"figure":{
            "id":"flow","family":"flow","binding":"authored",
            "title":"Qualification flow","caption":"Input moves through review to evidence."}}}
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
    assert!(page.contains("data-cf-figure-block=\"pending\""));
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
    for path in [
        "assets/review-document.example.json",
        "resources/present-document.example.json",
    ] {
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
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            registry.errors("urn:codeflow:schema:present:document:1", &value),
            Vec::<String>::new(),
            "{path}"
        );
        assert!(
            !text.contains("\"diagram\""),
            "{path} still carries a diagram"
        );
    }
    let review: serde_json::Value =
        serde_json::from_str(&skill_file("assets/review-document.example.json")).unwrap();
    assert_eq!(review["blocks"][0]["type"], "figure");
    assert_eq!(
        review["blocks"][0]["declaration"]["figure"]["family"],
        "flow"
    );

    // The narrowed schema refuses the removed block.
    let mut retired = review.clone();
    retired["blocks"][0] = serde_json::json!({
        "type": "diagram", "id": "flow", "kind": "flowchart", "source": "flowchart LR",
        "acc_title": "Flow", "acc_description": "A flow."
    });
    assert!(!registry
        .errors("urn:codeflow:schema:present:document:1", &retired)
        .is_empty());
}

/// The section the refusal names teaches every former kind with the same
/// replacement the runtime names, and points at a complete figure block and
/// the family specimens.
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
    for pointer in [
        "assets/review-document.example.json",
        "resources/figure-grammar-specimens.md",
        "tests/fixtures/figures/*.json",
    ] {
        assert!(
            section.contains(pointer),
            "the conversion section lost {pointer}"
        );
    }
    assert!(!text.contains("| diagram |") && !text.contains("\"type\": \"diagram\""));
}
