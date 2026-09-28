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
    let runtime_session = runtime_session(fixture, &running.session_id);
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

/// The session's runtime directory, which holds its browser profile.
fn runtime_session(fixture: &TestProject, session_id: &str) -> PathBuf {
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
    project.join(session_id)
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

fn export_theme_aliases_match_canonical_bytes(
    fixture: &TestProject,
    running: &RunningPresentation,
    graphite_export: &str,
) {
    let mut canonical = std::collections::BTreeMap::new();
    for skin in ["graphite", "slate", "sage"] {
        let path = fixture.project.join(format!("review-{skin}.html"));
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
                skin,
                "--mode",
                "dark",
            ],
        ));
        let html = fs::read_to_string(&path).unwrap();
        assert!(html.contains(&format!("data-cf-theme=\"{skin}\"")));
        assert!(
            !html
                .split("<html")
                .nth(1)
                .unwrap()
                .split('>')
                .next()
                .unwrap()
                .contains("data-cf-typeface"),
            "a skin must not select a typeface"
        );
        canonical.insert(skin, html);
    }
    assert_eq!(canonical["graphite"], graphite_export);
    for (alias, skin) in [
        ("instrument", "graphite"),
        ("technical", "graphite"),
        ("editorial", "slate"),
        ("ink", "sage"),
    ] {
        let path = fixture.project.join(format!("review-alias-{alias}.html"));
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
                alias,
                "--mode",
                "dark",
            ],
        ));
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            canonical[skin],
            "{alias} maps to {skin}"
        );
    }
    let path = fixture.project.join("review-default.html");
    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "export",
            &running.session_id,
            "--out",
            path.to_str().unwrap(),
            "--mode",
            "dark",
        ],
    ));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        canonical["slate"],
        "the retained default resolves to Slate"
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
    // The retained CLI alias resolves to the canonical skin.
    assert!(exported.contains("data-cf-theme=\"graphite\""));
    assert!(exported.contains("data-cf-mode=\"dark\""));

    export_theme_aliases_match_canonical_bytes(fixture, running, &exported);
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

/// SPC-014 C1 across the TSK-071 review-text changes: a review stored before
/// them (separator-migration fixture, with a diff quote that carries
/// "Added: +") reaches a 3.0.x agent on the v1 stream byte for byte as a
/// build before the change delivered it (`feedback-v1.jsonl`, captured at
/// ea0946594), and the history keeps the stored envelope unchanged.
#[test]
fn a_stored_review_reads_the_same_on_the_v1_stream() {
    const FIXTURES: &str = "../codeflow-present/tests/fixtures/separator-migration";
    const CAPTURED_SESSION: &str = "5e9a7c1e-0d2b-4f5a-9c3e-7b1d2f4a6c80";
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURES);
    let read = |name: &str| fs::read_to_string(fixtures.join(name)).unwrap();
    let fixture = setup_project();
    let document = fixture.project.join("migration.json");
    fs::write(&document, read("document.json")).unwrap();
    let (session_id, _) = open_no_launch(&fixture, &document);
    let events = read("events.jsonl").replace(CAPTURED_SESSION, &session_id);
    fs::write(
        session_dir(&fixture, &session_id).join("events.jsonl"),
        &events,
    )
    .unwrap();

    let delivered = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "feedback", &session_id],
    ));
    assert_eq!(
        delivered,
        read("feedback-v1.jsonl").replace(CAPTURED_SESSION, &session_id)
    );
    let history: serde_json::Value = serde_json::from_str(&require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "history", &session_id],
    )))
    .unwrap();
    assert_eq!(history["schema_version"], 1);
    let stored: serde_json::Value = serde_json::from_str(events.trim()).unwrap();
    assert_eq!(history["feedback_events"][0], stored);
    close_and_clear(&fixture, &session_id);
}

/// TSK-071 AC-4: `present open` launches the qualified browser itself, with
/// its profile under this test's scratch state. `present close` leaves no
/// process of that launch, found by the pid and instance `CodeFlow` recorded
/// and by the profile path its helpers carry, and no profile; `clear` leaves
/// nothing that names the session. It opens a real browser window on the
/// desktop, so it runs only on request:
/// `cargo test -p codeflow-cli --test present_cli -- --ignored`.
#[test]
#[ignore = "opens a visible browser window; run with --ignored"]
fn close_stops_the_browser_open_launched() {
    let fixture = setup_project();
    let document = fixture.project.join("first.json");
    let opened = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "open", document.to_str().unwrap()],
    ));
    let session_id = opened.split_whitespace().nth(1).unwrap().to_string();
    let listed: serde_json::Value = serde_json::from_str(&require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "list"],
    )))
    .unwrap();
    let pid = u32::try_from(
        listed[0]["browser_pid"]
            .as_u64()
            .expect("recorded browser pid"),
    )
    .unwrap();
    let instance = listed[0]["browser_instance"]
        .as_str()
        .expect("recorded browser instance")
        .to_string();
    let profile = runtime_session(&fixture, &session_id).join("browser-profile");
    assert!(profile.is_dir(), "no profile at {}", profile.display());

    // The recorded pid carries the recorded instance and profile; wait for
    // the browser to start its helpers, which carry the profile path.
    let deadline = Instant::now() + Duration::from_secs(30);
    let launched = loop {
        let owned = launch_processes(&instance, &profile);
        if owned.len() > 1 || Instant::now() > deadline {
            break owned;
        }
        thread::sleep(Duration::from_millis(250));
    };
    let leader = launched.iter().find(|(owned, _)| *owned == pid);
    assert!(
        leader.is_some_and(
            |(_, command)| command.contains(&format!("--cf-present-instance={instance}"))
        ),
        "the recorded pid {pid} is not the launch {instance}: {launched:?}"
    );
    assert!(
        launched.len() > 1,
        "the browser started no helpers: {launched:?}"
    );

    eprintln!(
        "launch {instance}: recorded pid {pid}, {} processes of the launch before close",
        launched.len()
    );
    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "close", &session_id],
    ));
    let remaining = launch_processes(&instance, &profile);
    assert!(
        remaining.is_empty(),
        "close left processes of the launch: {remaining:?}"
    );
    assert_ne!(
        unsafe { libc::kill(i32::try_from(pid).unwrap(), 0) },
        0,
        "the recorded pid {pid} still runs"
    );
    assert!(
        !profile.exists(),
        "close left the profile at {}",
        profile.display()
    );

    let cleared = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "clear", &session_id, "--older-than", "0h"],
    ));
    assert!(
        cleared.contains(&format!("removed {session_id}")),
        "{cleared}"
    );
    let named = paths_naming(&fixture.home, &session_id);
    assert!(named.is_empty(), "clear left state: {named:?}");
}

/// Every running process whose command line carries the launch's instance
/// marker or its profile path, with that command line.
fn launch_processes(instance: &str, profile: &Path) -> Vec<(u32, String)> {
    let listing = Command::new("/bin/ps")
        .args(["-axww", "-o", "pid=,command="])
        .output()
        .unwrap();
    assert!(listing.status.success(), "ps failed");
    let marker = format!("--cf-present-instance={instance}");
    let profile = profile.display().to_string();
    String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim_start().split_once(' ')?;
            if !(command.contains(&marker) || command.contains(&profile)) {
                return None;
            }
            Some((pid.parse().ok()?, command.to_string()))
        })
        .collect()
}

/// Every file or directory under `root` whose name contains `needle`.
fn paths_naming(root: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains(needle))
        {
            found.push(path.clone());
        }
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            found.extend(paths_naming(&path, needle));
        }
    }
    found
}

const RETIRED_REVISION: &str =
    include_str!("../../codeflow-present/tests/fixtures/retired-diagram/revision.json");
const RETIRED_EVENTS: &str =
    include_str!("../../codeflow-present/tests/fixtures/retired-diagram/events.jsonl");
const RETIRED_CAPTURED_SESSION: &str = "c17874f5-9568-45f6-a657-180848fae57d";

fn schema_registry() -> json_schema::Registry {
    let schemas = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/present/schemas");
    json_schema::Registry::new(
        [
            "document-v1.schema.json",
            "document-v2.schema.json",
            "session-history-v1.schema.json",
            "session-history-v2.schema.json",
            "session-responses-v1.schema.json",
        ]
        .map(|name| {
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
    // close returns once the service has exited, so clear needs no retry.
    let cleared = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "clear", session_id, "--older-than", "0h"],
    ));
    assert!(
        cleared.contains(&format!("removed {session_id}")),
        "{cleared}"
    );
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

fn contract_fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../codeflow-present/tests/fixtures/contract-v2")
            .join(name),
    )
    .unwrap()
}

#[test]
fn schema_v2_fixtures_match_the_v2_schemas_and_v1_stays_on_v1() {
    let registry = schema_registry();
    let document_v2 = "urn:codeflow:schema:present:document:2";
    for valid in [
        "documents/v2-framed.json",
        "documents/v2-figure-mark-without-id.json",
    ] {
        // The schema checks shape; mark ids and references are runtime rules.
        let value: serde_json::Value = serde_json::from_str(&contract_fixture(valid)).unwrap();
        assert_eq!(
            registry.errors(document_v2, &value),
            Vec::<String>::new(),
            "{valid}"
        );
    }
    for invalid in [
        "documents/v2-html-missing-caption.json",
        "documents/v2-table-missing-title.json",
    ] {
        let value: serde_json::Value = serde_json::from_str(&contract_fixture(invalid)).unwrap();
        assert!(
            !registry.errors(document_v2, &value).is_empty(),
            "{invalid}"
        );
    }
    let v1: serde_json::Value =
        serde_json::from_str(&contract_fixture("documents/v1-html-title.json")).unwrap();
    assert_eq!(
        registry.errors("urn:codeflow:schema:present:document:1", &v1),
        Vec::<String>::new()
    );
    assert!(!registry.errors(document_v2, &v1).is_empty());
}

/// TSK-119: the form and v2 decision fixtures match the v2 document schema,
/// a v2 decision with a status and a field with a default do not, and an
/// answer line of the ledger fixture matches the responses schema. Required
/// ids and a single recommended option are runtime rules.
#[test]
fn form_fixtures_and_answer_lines_match_their_schemas() {
    let registry = schema_registry();
    let document_v2 = "urn:codeflow:schema:present:document:2";
    for valid in [
        "documents/v2-forms.json",
        "documents/delivery-v2.json",
        "documents/v2-form-required-unknown.json",
        "documents/v2-form-two-recommended.json",
    ] {
        let value: serde_json::Value = serde_json::from_str(&contract_fixture(valid)).unwrap();
        assert_eq!(
            registry.errors(document_v2, &value),
            Vec::<String>::new(),
            "{valid}"
        );
    }
    let every_block: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../codeflow-present/tests/fixtures/annotation/every-block.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        registry.errors(document_v2, &every_block),
        Vec::<String>::new()
    );
    for invalid in [
        "documents/v2-decision-with-status.json",
        "documents/v2-form-default-value.json",
    ] {
        let value: serde_json::Value = serde_json::from_str(&contract_fixture(invalid)).unwrap();
        assert!(
            !registry.errors(document_v2, &value).is_empty(),
            "{invalid}"
        );
    }
    let responses = "urn:codeflow:schema:present:session-responses:1";
    let ledger = contract_fixture("ledger/torn-tail.jsonl");
    let line: serde_json::Value = serde_json::from_str(ledger.lines().next().unwrap()).unwrap();
    assert_eq!(registry.errors(responses, &line), Vec::<String>::new());
    let mut amendment = line.clone();
    amendment["event"] = "amendment".into();
    assert!(
        !registry.errors(responses, &amendment).is_empty(),
        "amends is required"
    );
    amendment["amends"] = line["answer_id"].clone();
    assert_eq!(registry.errors(responses, &amendment), Vec::<String>::new());
    let mut delivered = line;
    delivered["event"] = "delivered".into();
    assert!(
        !registry.errors(responses, &delivered).is_empty(),
        "responses v1 describes answer and amendment lines only"
    );
}

fn post_answer(port: u16, authority: &str, cookie: &str, answer: &str) -> String {
    http(
        port,
        &format!(
            "POST /app/api/answers HTTP/1.1\r\nHost: {authority}\r\nOrigin: http://{authority}\r\nCookie: {cookie}\r\nX-CF-Present: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
            answer.len()
        ),
    )
}

fn response_json(response: &str) -> serde_json::Value {
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

/// TSK-119 end to end: the page the service renders carries each form's
/// digest; answers posted to the real service are stored in the session's
/// `responses.jsonl`, whose lines match the responses schema, and the v1
/// `events.jsonl` and `feedback` stream stay as they were.
#[test]
fn answers_posted_to_the_service_are_stored_as_schema_lines() {
    let fixture = setup_project();
    let document = fixture.project.join("forms.json");
    fs::write(&document, contract_fixture("documents/v2-forms.json")).unwrap();
    let (session_id, opened) = open_no_launch(&fixture, &document);
    let (port, authority, cookie) = bootstrap_cookie(&opened);
    let page = application_page(port, &authority, &cookie);
    let digest = |form: &str| {
        let at = page
            .find(&format!("data-cf-form=\"{form}\""))
            .unwrap_or_else(|| panic!("no form {form} on the page"));
        between(&page[at..], "data-cf-form-digest=\"", "\"").to_string()
    };
    let events_before = fs::read(session_dir(&fixture, &session_id).join("events.jsonl")).unwrap();
    let answer = format!(
        r#"{{"request_id":"3f2a0c11-0000-4000-8000-0000000000c1","session_id":"{session_id}","revision":1,"form_id":"store-choice","form_digest":"{}","outcome":"submit","values":{{"home":"local","keep-days":30,"channels":["rail"],"share":false}},"rationales":{{"home":"Answers can hold private text."}}}}"#,
        digest("store-choice")
    );
    let stored = post_answer(port, &authority, &cookie, &answer);
    assert!(stored.starts_with("HTTP/1.1 200 "), "{stored}");
    let receipt = response_json(&stored);
    assert_eq!(receipt["state"], "stored");
    let amendment = format!(
        r#"{{"request_id":"3f2a0c11-0000-4000-8000-0000000000c2","session_id":"{session_id}","revision":1,"form_id":"store-choice","form_digest":"{}","outcome":"submit","values":{{"home":"repo","keep-days":7}},"rationales":{{}},"amends":"{}"}}"#,
        digest("store-choice"),
        receipt["answer_id"].as_str().unwrap()
    );
    assert!(post_answer(port, &authority, &cookie, &amendment).starts_with("HTTP/1.1 200 "));
    let decision = format!(
        r#"{{"request_id":"3f2a0c11-0000-4000-8000-0000000000c3","session_id":"{session_id}","revision":1,"form_id":"d-scope","form_digest":"{}","outcome":"decline","values":{{}},"rationales":{{}},"reason":"Not my call."}}"#,
        digest("d-scope")
    );
    assert!(post_answer(port, &authority, &cookie, &decision).starts_with("HTTP/1.1 200 "));
    let refused = post_answer(
        port,
        &authority,
        &cookie,
        &answer.replace("\"keep-days\":30", "\"keep-days\":0"),
    );
    assert!(
        refused.starts_with("HTTP/1.1 409 "),
        "the reused request id: {refused}"
    );

    let directory = session_dir(&fixture, &session_id);
    let ledger = fs::read_to_string(directory.join("responses.jsonl")).unwrap();
    let registry = schema_registry();
    let lines: Vec<serde_json::Value> = ledger
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    for line in &lines {
        assert_eq!(
            registry.errors("urn:codeflow:schema:present:session-responses:1", line),
            Vec::<String>::new(),
            "{line}"
        );
    }
    assert_eq!(
        lines
            .iter()
            .map(|line| line["event"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["answer", "amendment", "answer"]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(directory.join("responses.jsonl"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert_eq!(
        fs::read(directory.join("events.jsonl")).unwrap(),
        events_before
    );
    let feedback = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "feedback", &session_id],
    ));
    assert!(
        feedback.trim().is_empty(),
        "answers are not v1 feedback: {feedback}"
    );
    close_and_clear(&fixture, &session_id);
}

#[test]
fn a_v2_session_opens_renders_framing_and_prints_history_v2() {
    let fixture = setup_project();
    let document = fixture.project.join("framed.json");
    fs::write(&document, contract_fixture("documents/v2-framed.json")).unwrap();
    let (session_id, opened) = open_no_launch(&fixture, &document);
    let (port, authority, cookie) = bootstrap_cookie(&opened);
    let page = application_page(port, &authority, &cookie);
    assert!(
        page.contains("<span class=\"cf-frame-number\">Figure 2</span>"),
        "{page}"
    );
    assert!(page.contains("data-cf-entity=\"submit-edge\""));
    let history: serde_json::Value = serde_json::from_str(&require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "history", &session_id],
    )))
    .unwrap();
    assert_eq!(history["schema_version"], 2);
    assert_eq!(
        schema_registry().errors("urn:codeflow:schema:present:session-history:2", &history),
        Vec::<String>::new()
    );
    close_and_clear(&fixture, &session_id);
}

/// The annotation matrix in the authoring reference names every block type
/// of the closed enum, with a text, element and area cell, each "yes" or
/// "no: <reason>" (TSK-071). The block types come from the enum itself (its
/// deserializer names every variant it accepts), with no fixed count, so a
/// new variant fails here until the fixture and the matrix both carry it.
#[test]
fn the_annotation_matrix_names_every_block_type() {
    use codeflow_present::document::Block;

    let refusal = serde_json::from_value::<Block>(serde_json::json!({"type": "not-a-block"}))
        .unwrap_err()
        .to_string();
    let listed = refusal
        .split_once("expected one of ")
        .map_or("", |(_, rest)| rest);
    let mut variants: Vec<&str> = listed
        .split(", ")
        .map(|name| name.trim().trim_matches('`'))
        .collect();
    variants.sort_unstable();
    assert!(
        variants.len() > 1 && variants.iter().all(|name| !name.is_empty()),
        "could not read the block types from {refusal:?}"
    );

    let fixture: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../codeflow-present/tests/fixtures/annotation/every-block.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let blocks: Vec<Block> = serde_json::from_value(fixture["blocks"].clone()).unwrap();
    let mut types: Vec<String> = blocks
        .iter()
        .map(|block| {
            serde_json::to_value(block).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    types.sort_unstable();
    types.dedup();
    assert_eq!(
        types, variants,
        "the fixture carries one top-level block of every type"
    );

    let text = skill_file("references/document-authoring.md");
    let section = between(
        &text,
        "\n### What a reviewer can mark on each block\n",
        "\n## ",
    );
    let mut rows: Vec<&str> = Vec::new();
    for line in section.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = line.trim_matches('|').split(" | ").map(str::trim).collect();
        assert_eq!(cells.len(), 4, "{line}");
        for cell in &cells[1..] {
            assert!(
                cell.starts_with("yes") || cell.starts_with("no: "),
                "{line}: a cell is yes or no with a reason"
            );
        }
        rows.push(cells[0].trim_matches('`'));
    }
    rows.sort_unstable();
    assert_eq!(
        rows, variants,
        "the matrix has one row for every block type"
    );
}

/// QA defect 10: a command's own bad input is named as a bad request, not a
/// bad document, and a dry run with nothing eligible says so.
#[test]
fn cli_input_errors_and_empty_clears_are_named() {
    let fixture = setup_project();
    let document = fixture.project.join("review.json");
    fs::write(&document, contract_fixture("documents/v2-framed.json")).unwrap();
    let (session_id, _) = open_no_launch(&fixture, &document);
    let resolve = failure(&codeflow(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "resolve",
            &session_id,
            "019f9b53-a341-7fa7-84c2-5f198ceea099",
            "--event-version",
            "1",
            "--status",
            "addressed",
        ],
    ));
    assert!(
        resolve.contains("invalid request: feedback event"),
        "{resolve}"
    );
    assert!(!resolve.contains("presentation document"), "{resolve}");
    let duration = failure(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "clear", "--older-than", "3m"],
    ));
    assert!(duration.contains("invalid request: duration"), "{duration}");
    let empty = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "clear", "--dry-run"],
    ));
    assert!(empty.contains("nothing to clear"), "{empty}");
    close_and_clear(&fixture, &session_id);
}

/// TSK-119, SPC-014 I5: `present update --expected-revision N` applies only
/// while N is current; otherwise it exits 8 with the exact conflict line on
/// stderr and writes no revision. Without the flag, update is unchanged.
#[test]
fn update_with_an_expected_revision_refuses_a_stale_base() {
    let fixture = setup_project();
    let document = fixture.project.join("forms.json");
    fs::write(&document, contract_fixture("documents/v2-forms.json")).unwrap();
    let (session_id, _) = open_no_launch(&fixture, &document);
    let revisions = || {
        fs::read_dir(session_dir(&fixture, &session_id).join("revisions"))
            .unwrap()
            .count()
    };
    let update = |expected: Option<&str>| {
        let mut args = vec!["present", "update", &session_id, document.to_str().unwrap()];
        if let Some(expected) = expected {
            args.extend(["--expected-revision", expected]);
        }
        codeflow(&fixture.project, &fixture.home, &args)
    };

    let applied = update(Some("1"));
    assert_eq!(
        require_success(&applied).trim(),
        format!("updated {session_id} to revision 2")
    );
    assert_eq!(revisions(), 2);

    for stale in ["1", "3", "0"] {
        let refused = update(Some(stale));
        assert_eq!(
            refused.status.code(),
            Some(8),
            "--expected-revision {stale}"
        );
        assert_eq!(
            String::from_utf8(refused.stderr).unwrap(),
            format!("{{\"error\":\"revision_conflict\",\"expected\":{stale},\"current\":2}}\n")
        );
        assert!(refused.stdout.is_empty());
        assert_eq!(revisions(), 2, "no revision is written");
    }

    let not_a_number = update(Some("two"));
    assert_eq!(not_a_number.status.code(), Some(2), "usage error");
    assert_eq!(revisions(), 2);

    require_success(&update(None));
    assert_eq!(revisions(), 3);
    close_and_clear(&fixture, &session_id);
}
