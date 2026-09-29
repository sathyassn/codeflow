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
    let poll_body = r#"{"cursor":"1:0:0"}"#;
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
    assert!(poll.contains("\"cursor\":\"2:3:0\""));

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
    // T114-1: the rest of the document renders as it always did.
    assert!(html.contains("The qualification path before the diagram block was retired."));
    assert!(html.contains("Message order"));
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
    // TSK-120: the delivered and acknowledged lines of the query ledger.
    for line in contract_fixture("ledger/queries.jsonl").lines() {
        let line: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(
            registry.errors(responses, &line),
            Vec::<String>::new(),
            "{line}"
        );
    }
    let mut delivered = line;
    delivered["event"] = "delivered".into();
    assert!(
        !registry.errors(responses, &delivered).is_empty(),
        "a delivered line carries no answer fields"
    );
    let state = serde_json::json!({ "event": "acknowledged", "sequence": 2, "at_unix": 0 });
    assert!(
        !registry.errors(responses, &state).is_empty(),
        "a state line names its target"
    );
}

/// The form and v2 decision examples in the authoring reference are blocks
/// the runtime accepts and the v2 schema describes.
#[test]
fn the_reference_form_examples_are_valid_v2_blocks() {
    let text = skill_file("references/document-authoring.md");
    let section = between(&text, "\n### Forms and decisions\n", "\n## ");
    let blocks: Vec<serde_json::Value> = section
        .split("```json\n")
        .skip(1)
        .map(|fence| serde_json::from_str(fence.split("```").next().unwrap()).unwrap())
        .collect();
    assert_eq!(
        blocks
            .iter()
            .map(|block| block["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["form", "decision"]
    );
    let document =
        serde_json::json!({ "schema_version": 2, "title": "Examples", "blocks": blocks });
    assert!(matches!(
        codeflow_present::document::parse_document(document.to_string().as_bytes()).unwrap(),
        codeflow_present::document::ParsedDocument::Supported(_)
    ));
    assert_eq!(
        schema_registry().errors("urn:codeflow:schema:present:document:2", &document),
        Vec::<String>::new()
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
    let mut legacy: Vec<&str> = Vec::new();
    for line in section.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = line.trim_matches('|').split(" | ").map(str::trim).collect();
        assert_eq!(cells.len(), 4, "{line}");
        for cell in &cells[1..] {
            assert!(
                cell.starts_with("yes") || cell.starts_with("no: "),
                "{line}: a cell is yes or no with a reason"
            );
        }
        // TSK-119: the schema_version 1 decision keeps its own row.
        match cells[0].strip_suffix(" (v1)") {
            Some(name) => legacy.push(name.trim_matches('`')),
            None => rows.push(cells[0].trim_matches('`')),
        }
    }
    rows.sort_unstable();
    assert_eq!(
        rows, variants,
        "the matrix has one row for every block type"
    );
    assert_eq!(
        legacy,
        ["decision"],
        "the matrix keeps one legacy row, the v1 decision"
    );
    let legacy_fixture: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../codeflow-present/tests/fixtures/annotation/legacy-decision.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(legacy_fixture["schema_version"], 1);
    assert!(
        legacy_fixture["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|block| block["type"] == "decision" && block["status"].is_string()),
        "the legacy fixture holds a v1 decision with a status"
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

fn post_review(port: u16, authority: &str, cookie: &str, review: &str) -> String {
    http(
        port,
        &format!(
            "POST /app/api/reviews HTTP/1.1\r\nHost: {authority}\r\nOrigin: http://{authority}\r\nCookie: {cookie}\r\nX-CF-Present: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{review}",
            review.len()
        ),
    )
}

/// The value of `attribute` on the element the page renders for `marker`.
fn attribute_after(page: &str, marker: &str, attribute: &str) -> String {
    let at = page
        .find(marker)
        .unwrap_or_else(|| panic!("no {marker} on the page"));
    between(&page[at..], &format!("{attribute}=\""), "\"").to_string()
}

/// The TSK-117 stream fixture (`streams/session.json`) on the real service:
/// its entity review on the framed document at revision 1, then its answer
/// on the forms document at revision 2. Returns the session and answer ids.
fn stream_session(fixture: &TestProject) -> (String, String) {
    let spec: serde_json::Value =
        serde_json::from_str(&contract_fixture("streams/session.json")).unwrap();
    let document = fixture.project.join("framed.json");
    fs::write(
        &document,
        contract_fixture(spec["document"].as_str().unwrap()),
    )
    .unwrap();
    let (session_id, opened) = open_no_launch(fixture, &document);
    let (port, authority, cookie) = bootstrap_cookie(&opened);
    let page = application_page(port, &authority, &cookie);
    let digest = attribute_after(
        &page,
        "data-cf-block-id=\"landing\"",
        "data-cf-block-digest",
    );
    let mut review: serde_json::Value = serde_json::from_str(
        &contract_fixture(spec["reviews"][0].as_str().unwrap())
            .replace("{{digest:landing}}", &digest),
    )
    .unwrap();
    review["session_id"] = session_id.clone().into();
    let label = attribute_after(&page, "data-cf-block-id=\"landing\"", "data-cf-block-label");
    for note in review["notes"].as_array_mut().unwrap() {
        let id = note.as_object_mut().unwrap().remove("id").unwrap();
        note["client_id"] = id;
        note["block_label"] = label.clone().into();
    }
    let posted = post_review(port, &authority, &cookie, &review.to_string());
    assert!(posted.starts_with("HTTP/1.1 201 "), "{posted}");

    let forms = fixture.project.join("forms.json");
    fs::write(
        &forms,
        contract_fixture(spec["answers_document"].as_str().unwrap()),
    )
    .unwrap();
    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "update", &session_id, forms.to_str().unwrap()],
    ));
    let page = application_page(port, &authority, &cookie);
    let form_digest = attribute_after(
        &page,
        "data-cf-form=\"store-choice\"",
        "data-cf-form-digest",
    );
    let mut answer: serde_json::Value = serde_json::from_str(
        &contract_fixture(spec["answers"][0].as_str().unwrap())
            .replace("{{form_digest:store-choice}}", &form_digest),
    )
    .unwrap();
    answer["session_id"] = session_id.clone().into();
    answer["revision"] = 2.into();
    let stored = post_answer(port, &authority, &cookie, &answer.to_string());
    assert!(stored.starts_with("HTTP/1.1 200 "), "{stored}");
    let answer_id = response_json(&stored)["answer_id"]
        .as_str()
        .unwrap()
        .to_string();
    (session_id, answer_id)
}

/// A stream with the ids and times that change on every run replaced by
/// placeholders, so it compares with a golden file.
fn normalized(text: &str, session_id: &str, answer_id: &str) -> String {
    const KEY: &str = "\"created_at_unix\":";
    let text = text
        .replace(session_id, "{{session_id}}")
        .replace(answer_id, "{{answer_id:1}}");
    let mut output = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find(KEY) {
        output.push_str(&rest[..at + KEY.len()]);
        rest = &rest[at + KEY.len()..];
        rest = rest.trim_start_matches(|character: char| character.is_ascii_digit());
        output.push('0');
    }
    output.push_str(rest);
    output
}

fn stream_golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codeflow-present/tests/fixtures/contract-v2/streams")
        .join(name)
}

/// TSK-120 AC-2, the architecture fitness check: on the TSK-117 stream
/// fixture, the v1 `feedback` stream is byte for byte what the build before
/// this task printed (`feedback-v1.jsonl`, captured at d1056b471), with the
/// pending answer named on stderr only; `--format v2` prints the review and
/// the answer as typed events in sequence order (`feedback-v2.jsonl`).
#[test]
fn the_v1_and_v2_feedback_streams_match_their_goldens() {
    let fixture = setup_project();
    let (session_id, answer_id) = stream_session(&fixture);
    let directory = session_dir(&fixture, &session_id);
    let events = fs::read(directory.join("events.jsonl")).unwrap();
    let responses = fs::read(directory.join("responses.jsonl")).unwrap();

    let v1 = codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "feedback", &session_id],
    );
    let v1_stdout = normalized(&require_success(&v1), &session_id, &answer_id);
    assert_eq!(
        v1_stdout,
        fs::read_to_string(stream_golden("feedback-v1.jsonl")).unwrap()
    );
    assert_eq!(
        String::from_utf8(v1.stderr).unwrap(),
        "present: 1 pending answer event is not on the v1 stream; read it with --format v2\n"
    );
    assert!(!v1_stdout.contains("entity_selector") && !v1_stdout.contains("untrusted"));

    // The same session as it was before the v1 read delivered its review.
    fs::write(directory.join("events.jsonl"), &events).unwrap();
    fs::write(directory.join("responses.jsonl"), &responses).unwrap();
    let v2 = codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "feedback", &session_id, "--format", "v2"],
    );
    let v2_stdout = normalized(&require_success(&v2), &session_id, &answer_id);
    assert!(
        v2.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&v2.stderr)
    );
    assert_eq!(
        v2_stdout,
        fs::read_to_string(stream_golden("feedback-v2.jsonl")).unwrap()
    );
    close_and_clear(&fixture, &session_id);
}

/// A `submit` answer to one form of `documents/v2-forms.json`, as the page
/// sends it; `amends` names an earlier answer for a correction.
fn forms_answer(
    session_id: &str,
    revision: u64,
    (form, digest): (&str, &str),
    request: u128,
    values: &str,
    amends: Option<&str>,
) -> String {
    let amends = amends.map_or(String::new(), |id| format!(r#","amends":"{id}""#));
    format!(
        r#"{{"request_id":"3f2a0c11-0000-4000-8000-{request:012x}","session_id":"{session_id}","revision":{revision},"form_id":"{form}","form_digest":"{digest}","outcome":"submit","values":{values},"rationales":{{}}{amends}}}"#
    )
}

/// A session on `documents/v2-forms.json` served by the real service.
struct FormsService {
    session_id: String,
    port: u16,
    authority: String,
    cookie: String,
}

impl FormsService {
    fn open(fixture: &TestProject) -> Self {
        let document = fixture.project.join("forms.json");
        fs::write(&document, contract_fixture("documents/v2-forms.json")).unwrap();
        let (session_id, opened) = open_no_launch(fixture, &document);
        let (port, authority, cookie) = bootstrap_cookie(&opened);
        Self {
            session_id,
            port,
            authority,
            cookie,
        }
    }

    /// Posts an answer to `form` on `revision`; returns its answer id.
    fn answer(
        &self,
        revision: u64,
        form: &str,
        request: u128,
        values: &str,
        amends: Option<&str>,
    ) -> String {
        let page = application_page(self.port, &self.authority, &self.cookie);
        let digest = attribute_after(
            &page,
            &format!("data-cf-form=\"{form}\""),
            "data-cf-form-digest",
        );
        let body = forms_answer(
            &self.session_id,
            revision,
            (form, &digest),
            request,
            values,
            amends,
        );
        let stored = post_answer(self.port, &self.authority, &self.cookie, &body);
        assert!(stored.starts_with("HTTP/1.1 200 "), "{stored}");
        response_json(&stored)["answer_id"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

/// The event ids of v2 lines, in order.
fn event_ids(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(|line| {
            let line: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(
                (line["format"].as_u64(), line["untrusted"].as_bool()),
                (Some(2), Some(true)),
                "{line}"
            );
            line["event_id"].as_str().unwrap().to_string()
        })
        .collect()
}

const V1_NOTICE: &str =
    "present: 1 pending answer event is not on the v1 stream; read it with --format v2\n";

/// TSK-120 AC-1, SPC-014 B8 and I5: `feedback --wait` exits 0 once it has
/// printed pending events, 6 when `--timeout` passes first and 7 when the
/// session closes with nothing pending; `--wait` with `--follow`, a timeout
/// out of range or without `--wait` are usage errors (2). The v1 wait does
/// not end on an answer and names it on stderr at the start and on exit.
#[test]
#[allow(clippy::too_many_lines)]
fn feedback_wait_exits_with_the_spc014_statuses() {
    let fixture = setup_project();
    let service = FormsService::open(&fixture);
    let session_id = service.session_id.clone();
    let feedback = |args: &[&str]| {
        let mut all = vec!["present", "feedback", session_id.as_str()];
        all.extend_from_slice(args);
        codeflow(&fixture.project, &fixture.home, &all)
    };

    for args in [
        &["--wait", "--follow"][..],
        &["--wait", "--timeout", "0"],
        &["--wait", "--timeout", "86401"],
        &["--timeout", "5"],
        &["--wait", "--format", "v3"],
    ] {
        let usage = feedback(args);
        assert_eq!(
            usage.status.code(),
            Some(2),
            "{args:?}: {}",
            String::from_utf8_lossy(&usage.stderr)
        );
        assert!(usage.stdout.is_empty(), "{args:?}");
    }

    let started = Instant::now();
    let timeout = feedback(&["--wait", "--timeout", "1", "--format", "v2"]);
    assert_eq!(
        timeout.status.code(),
        Some(6),
        "{}",
        String::from_utf8_lossy(&timeout.stderr)
    );
    assert!(timeout.stdout.is_empty());
    assert!(started.elapsed() >= Duration::from_secs(1));

    // A wait with no timeout blocks until an answer arrives, then prints it.
    let waiting = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args([
            "present",
            "feedback",
            &session_id,
            "--wait",
            "--format",
            "v2",
        ])
        .current_dir(&fixture.project)
        .env("HOME", &fixture.home)
        .env("XDG_STATE_HOME", fixture.home.join("state"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut waiting = waiting;
    thread::sleep(Duration::from_millis(800));
    assert!(
        waiting.try_wait().unwrap().is_none(),
        "the wait ended with nothing pending"
    );
    let first = service.answer(
        1,
        "store-choice",
        1,
        r#"{"home":"local","keep-days":30}"#,
        None,
    );
    let woken = waiting.wait_with_output().unwrap();
    assert_eq!(
        woken.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&woken.stderr)
    );
    let woken = String::from_utf8(woken.stdout).unwrap();
    assert_eq!(event_ids(&woken), std::slice::from_ref(&first));
    let line: serde_json::Value = serde_json::from_str(woken.trim()).unwrap();
    assert_eq!(
        (line["kind"].as_str(), line["status"].as_str()),
        (Some("answer"), Some("delivered"))
    );

    // The v1 stream carries no answer: its wait times out and names the
    // pending answer on stderr when it starts and again when it ends.
    let second = service.answer(1, "d-scope", 2, r#"{"choice":"a"}"#, None);
    let v1 = feedback(&["--wait", "--timeout", "1"]);
    assert_eq!(v1.status.code(), Some(6));
    assert!(v1.stdout.is_empty());
    assert_eq!(String::from_utf8(v1.stderr).unwrap(), V1_NOTICE.repeat(2));
    let plain = feedback(&[]);
    assert_eq!(plain.status.code(), Some(0));
    assert!(plain.stdout.is_empty());
    assert_eq!(String::from_utf8(plain.stderr).unwrap(), V1_NOTICE);
    let v2 = feedback(&["--wait", "--format", "v2"]);
    assert_eq!(v2.status.code(), Some(0));
    assert_eq!(event_ids(&String::from_utf8(v2.stdout).unwrap()), [second]);
    assert!(v2.stderr.is_empty());

    // Closed with an event pending, the wait still delivers it; closed with
    // nothing pending, it exits 7 at once, in either format. The form holds
    // its original answer, so the pending event is a correction.
    let third = service.answer(
        1,
        "store-choice",
        3,
        r#"{"home":"repo","keep-days":7}"#,
        Some(&first),
    );
    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "close", &session_id],
    ));
    let last = feedback(&["--wait", "--format", "v2"]);
    assert_eq!(last.status.code(), Some(0));
    assert_eq!(event_ids(&String::from_utf8(last.stdout).unwrap()), [third]);
    for format in ["v1", "v2"] {
        let started = Instant::now();
        let closed = feedback(&["--wait", "--format", format]);
        assert_eq!(closed.status.code(), Some(7), "{format}");
        assert!(
            closed.stdout.is_empty() && closed.stderr.is_empty(),
            "{format}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{format}: the closed wait hung"
        );
    }
    let unknown = codeflow(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "feedback",
            "019f9b53-a341-7fa7-84c2-5f198ceea099",
            "--wait",
        ],
    );
    assert_eq!(unknown.status.code(), Some(3));
    let cleared = require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "clear", &session_id, "--older-than", "0h"],
    ));
    assert!(
        cleared.contains(&format!("removed {session_id}")),
        "{cleared}"
    );
}

/// TSK-120 AC-3 and AC-4 on the real binary: the session of the TSK-117
/// query fixture (`ledger/queries.json`) built through the service, with
/// answers across two revisions, two forms and every status. Every query
/// lists exactly its ids, alone and combined, however often it runs, and
/// changes nothing; the wait after them still returns every pending event.
/// `ack` refuses an undelivered or unknown event (2), records a delivered
/// one, and a second ack changes nothing.
#[test]
#[allow(clippy::too_many_lines)]
fn responses_list_filters_never_consume_and_ack_is_recorded_once() {
    let fixture = setup_project();
    let service = FormsService::open(&fixture);
    let session_id = service.session_id.clone();
    let run = |args: &[&str]| codeflow(&fixture.project, &fixture.home, args);
    let a1 = service.answer(
        1,
        "store-choice",
        1,
        r#"{"home":"local","keep-days":30}"#,
        None,
    );
    let a2 = service.answer(1, "d-scope", 2, r#"{"choice":"a"}"#, None);

    let undelivered = run(&["present", "ack", &session_id, &a1]);
    assert_eq!(undelivered.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&undelivered.stderr).contains("not delivered"));
    let delivered = require_success(&run(&[
        "present",
        "feedback",
        &session_id,
        "--format",
        "v2",
    ]));
    assert_eq!(event_ids(&delivered), [a1.clone(), a2.clone()]);
    assert_eq!(
        require_success(&run(&["present", "ack", &session_id, &a1])).trim(),
        format!("acknowledged {a1}")
    );
    let ledger_path = session_dir(&fixture, &session_id).join("responses.jsonl");
    let acknowledged = fs::read(&ledger_path).unwrap();
    assert_eq!(
        require_success(&run(&["present", "ack", &session_id, &a1])).trim(),
        format!("{a1} was already acknowledged")
    );
    assert_eq!(
        fs::read(&ledger_path).unwrap(),
        acknowledged,
        "a second ack wrote"
    );
    for unknown in ["019f9b53-a341-7fa7-84c2-5f198ceea099", "not-an-event"] {
        let refused = run(&["present", "ack", &session_id, unknown]);
        assert_eq!(refused.status.code(), Some(2), "{unknown}");
    }

    // Revision 2 retitles both forms: a changed question takes a new
    // original answer, as one form digest holds one (SPC-014 B6).
    let mut second: serde_json::Value =
        serde_json::from_str(&contract_fixture("documents/v2-forms.json")).unwrap();
    for block in second["blocks"].as_array_mut().unwrap() {
        if block["type"] == "form" || block["type"] == "decision" {
            let title = format!("{}, revised", block["title"].as_str().unwrap());
            block["title"] = serde_json::json!(title);
        }
    }
    let document = fixture.project.join("forms-2.json");
    fs::write(&document, serde_json::to_vec_pretty(&second).unwrap()).unwrap();
    require_success(&run(&[
        "present",
        "update",
        &session_id,
        document.to_str().unwrap(),
    ]));
    let a3 = service.answer(
        2,
        "store-choice",
        3,
        r#"{"home":"repo","keep-days":7}"#,
        None,
    );
    let a4 = service.answer(
        2,
        "store-choice",
        4,
        r#"{"home":"local","keep-days":7}"#,
        Some(&a3),
    );
    let a5 = service.answer(2, "d-scope", 5, r#"{"choice":"b"}"#, None);
    let ours = [&a1, &a2, &a3, &a4, &a5];
    let mapped = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|id| {
                let id = id.as_str().unwrap();
                let index = usize::from_str_radix(&id[id.len() - 2..], 16).unwrap() - 1;
                ours[index].clone()
            })
            .collect()
    };

    let before = fs::read(&ledger_path).unwrap();
    let queries: serde_json::Value =
        serde_json::from_str(&contract_fixture("ledger/queries.json")).unwrap();
    let mut then = None;
    for case in queries["cases"].as_array().unwrap() {
        let Some(args) = case["args"].as_array() else {
            then = Some(case);
            continue;
        };
        let mut command = vec!["present", "responses", "list", session_id.as_str()];
        command.extend(args.iter().map(|arg| arg.as_str().unwrap()));
        for _ in 0..2 {
            let listed = require_success(&run(&command));
            assert_eq!(
                event_ids(&listed),
                mapped(&case["expect_event_ids"]),
                "{args:?}"
            );
        }
    }
    assert_eq!(
        fs::read(&ledger_path).unwrap(),
        before,
        "listing changed the ledger"
    );
    let then = then.unwrap();
    let waited = run(&[
        "present",
        "feedback",
        &session_id,
        "--wait",
        "--timeout",
        "5",
        "--format",
        "v2",
    ]);
    assert_eq!(waited.status.code(), Some(0));
    assert_eq!(
        event_ids(&String::from_utf8(waited.stdout).unwrap()),
        mapped(&then["expect_event_ids"]),
        "{}",
        then["why"]
    );
    let pending = require_success(&run(&[
        "present",
        "responses",
        "list",
        &session_id,
        "--status",
        "pending",
    ]));
    assert!(pending.is_empty(), "{pending}");
    let unknown = run(&[
        "present",
        "responses",
        "list",
        "019f9b53-a341-7fa7-84c2-5f198ceea099",
    ]);
    assert_eq!(unknown.status.code(), Some(3));

    let registry = schema_registry();
    let lines = fs::read_to_string(&ledger_path).unwrap();
    let kinds = lines
        .lines()
        .map(|line| {
            let line: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(
                registry.errors("urn:codeflow:schema:present:session-responses:1", &line),
                Vec::<String>::new(),
                "{line}"
            );
            line["event"].as_str().unwrap().to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            "answer",
            "answer",
            "delivered",
            "delivered",
            "acknowledged",
            "answer",
            "amendment",
            "answer",
            "delivered",
            "delivered",
            "delivered"
        ]
    );
    // The schema root is closed, and a state line carries no answer field.
    let state = lines
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|line| line["event"] == "delivered")
        .unwrap();
    for (field, value) in [
        ("note", serde_json::json!("x")),
        ("answer_id", state["target"].clone()),
    ] {
        let mut invalid = state.clone();
        invalid[field] = value;
        assert!(
            !registry
                .errors("urn:codeflow:schema:present:session-responses:1", &invalid)
                .is_empty(),
            "{invalid}"
        );
    }
    close_and_clear(&fixture, &session_id);
}

/// TSK-120 AC-6 on the real binary: with no listener an answer waits in the
/// store as pending; after the service is killed and `show` restarts it,
/// the next wait delivers that answer and one stored through the new
/// service, each once, and a later wait finds nothing.
#[test]
fn a_pending_answer_survives_a_service_restart_and_is_delivered_once() {
    let fixture = setup_project();
    let service = FormsService::open(&fixture);
    let session_id = service.session_id.clone();
    let run = |args: &[&str]| codeflow(&fixture.project, &fixture.home, args);
    let first = service.answer(
        1,
        "store-choice",
        1,
        r#"{"home":"local","keep-days":30}"#,
        None,
    );
    let pending = require_success(&run(&[
        "present",
        "responses",
        "list",
        &session_id,
        "--status",
        "pending",
    ]));
    assert_eq!(event_ids(&pending), std::slice::from_ref(&first));

    let listed: serde_json::Value =
        serde_json::from_str(&require_success(&run(&["present", "list"]))).unwrap();
    let pid = i32::try_from(listed[0]["service_pid"].as_u64().unwrap()).unwrap();
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0);
    let recovered = require_success(&run(&["present", "show", &session_id, "--no-launch"]));
    assert!(recovered.contains("recovered"), "{recovered}");
    let (port, authority, cookie) = bootstrap_cookie(&recovered);
    let restarted = FormsService {
        session_id: session_id.clone(),
        port,
        authority,
        cookie,
    };
    let second = restarted.answer(1, "d-scope", 2, r#"{"choice":"a"}"#, None);

    let waited = run(&[
        "present",
        "feedback",
        &session_id,
        "--wait",
        "--timeout",
        "5",
        "--format",
        "v2",
    ]);
    assert_eq!(waited.status.code(), Some(0));
    assert_eq!(
        event_ids(&String::from_utf8(waited.stdout).unwrap()),
        [first, second]
    );
    let again = run(&[
        "present",
        "feedback",
        &session_id,
        "--wait",
        "--timeout",
        "1",
        "--format",
        "v2",
    ]);
    assert_eq!(again.status.code(), Some(6));
    assert!(again.stdout.is_empty());
    close_and_clear(&fixture, &session_id);
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

/// Write `document` as a JSON file under the project and return its path.
fn write_document(fixture: &TestProject, name: &str, document: &serde_json::Value) -> PathBuf {
    let path = fixture.project.join(name);
    fs::write(&path, serde_json::to_vec(document).unwrap()).unwrap();
    path
}

fn export_html(fixture: &TestProject, session_id: &str, name: &str) -> String {
    let out = fixture.project.join(name);
    require_success(&codeflow(
        &fixture.project,
        &fixture.home,
        &[
            "present",
            "export",
            session_id,
            "--out",
            out.to_str().unwrap(),
        ],
    ));
    fs::read_to_string(out).unwrap()
}

/// T114-1: a pre-release session too large for `present history` still
/// shows its whole current document. Five revisions of about 6.75 MiB put
/// the session over the one-shot history limit; the export of the retired
/// current revision carries every narrative block and the diagram's source.
#[test]
fn a_large_retired_session_exports_its_whole_document() {
    let fixture = setup_project();
    let padding = "p".repeat(460 * 1024);
    let narratives = |marker: &str| -> Vec<serde_json::Value> {
        (0..15)
            .map(|index| {
                serde_json::json!({
                    "type": "narrative",
                    "id": format!("part-{index}"),
                    "markdown": format!("{marker} part {index}.\n\n{padding}")
                })
            })
            .collect()
    };
    let document = |marker: &str| serde_json::json!({"schema_version": 1, "title": "Large review", "blocks": narratives(marker)});
    let first = write_document(&fixture, "large-1.json", &document("Revision 1"));
    let (session_id, _) = open_no_launch(&fixture, &first);
    for revision in 2..=5 {
        let path = write_document(
            &fixture,
            &format!("large-{revision}.json"),
            &document(&format!("Revision {revision}")),
        );
        require_success(&codeflow(
            &fixture.project,
            &fixture.home,
            &["present", "update", &session_id, path.to_str().unwrap()],
        ));
    }

    // A pre-release build stored the current revision with a diagram.
    let current = session_dir(&fixture, &session_id).join("revisions/00000000000000000005.json");
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(&current).unwrap()).unwrap();
    let mut blocks = narratives("Retired narrative");
    blocks.insert(
        7,
        serde_json::json!({
            "type": "diagram", "id": "flow", "kind": "flowchart",
            "source": "flowchart LR\n  Input --> Review",
            "acc_title": "Review flow", "acc_description": "Input reaches review."
        }),
    );
    record["content"]["document"]["blocks"] = blocks.into();
    fs::write(&current, serde_json::to_vec(&record).unwrap()).unwrap();

    let history = codeflow(
        &fixture.project,
        &fixture.home,
        &["present", "history", &session_id],
    );
    assert_eq!(history.status.code(), Some(4), "{history:?}");
    assert!(
        String::from_utf8_lossy(&history.stderr).contains("one-shot history output is limited"),
        "{history:?}"
    );

    let html = export_html(&fixture, &session_id, "large.html");
    for index in 0..15 {
        assert!(
            html.contains(&format!("Retired narrative part {index}.")),
            "the export lost narrative {index}"
        );
    }
    assert!(
        html.contains("flowchart LR\n  Input --&gt; Review"),
        "diagram source"
    );
    assert!(
        html.contains("Former flowchart diagram; convert it to a flow figure."),
        "conversion"
    );
    assert!(
        !html.contains("Revision 5 part"),
        "an older document leaked"
    );
    close_and_clear(&fixture, &session_id);
}
