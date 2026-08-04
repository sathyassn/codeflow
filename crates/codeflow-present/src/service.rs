use std::{
    collections::HashMap,
    fmt::Write as _,
    fs,
    io::Write as _,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Form, Path, State},
    http::{
        header::{self, HeaderName, HeaderValue},
        HeaderMap, Response, StatusCode,
    },
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use getrandom::fill as fill_random;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tokio::{net::TcpListener, sync::Notify};
use uuid::Uuid;

use crate::{
    config::UtilityTokens,
    document::Block,
    error::{PresentError, Result},
    limits,
    platform::is_link_like,
    render::{render_document, render_unsupported, sandbox_id, RenderIdentity, RenderOptions},
    state::{
        create_private_dir_all, write_json_atomic, ElementSelector, FeedbackEnvelope, FeedbackKind,
        FeedbackNote, FeedbackVerdict, RegionSelector, RevisionContent, SessionStatus,
        SessionStore, TextSelector,
    },
};

#[derive(RustEmbed)]
#[folder = "assets/"]
pub(crate) struct EmbeddedAssets;

const BOOTSTRAP_HANDOFF: &str = "<!doctype html><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\"><title>Opening presentation</title><script>location.replace('/app/')</script>";
const BOOTSTRAP_HANDOFF_CSP: &str = "default-src 'none'; script-src 'sha256-4MyoobivIq6Xw46Dc5S5dlGeU1Me98yo/zmVu3ed3zg='; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AssetManifest {
    pub(crate) schema_version: u32,
    service: ServiceManifest,
    pub(crate) export: ExportManifest,
}

#[derive(Debug, Clone, Deserialize)]
struct ServiceManifest {
    entrypoints: HashMap<String, String>,
    assets: Vec<ServiceAsset>,
    inline: HashMap<String, InlineAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ExportManifest {
    #[serde(rename = "present.export")]
    pub(crate) renderer: ExportAsset,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ExportAsset {
    pub(crate) stored_path: String,
    pub(crate) media_type: String,
    pub(crate) content_encoding: String,
    pub(crate) raw_bytes: u64,
    pub(crate) encoded_bytes: u64,
    pub(crate) sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ServiceAsset {
    request_path: String,
    stored_path: String,
    media_type: String,
    content_encoding: String,
    encoded_bytes: u64,
    sha256: String,
    etag: String,
}

#[derive(Debug, Clone, Deserialize)]
struct InlineAsset {
    source: String,
    csp_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadyRecord {
    pub schema_version: u32,
    pub session_id: Uuid,
    pub port: u16,
    pub instance_id: Uuid,
    pub bootstrap_path: PathBuf,
    pub recovery_capability: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthRecord {
    pub schema_version: u32,
    pub session_id: Uuid,
    pub instance_id: Uuid,
}

#[derive(Clone)]
struct AppState {
    store: SessionStore,
    session_id: Uuid,
    instance_id: Uuid,
    authority: String,
    bootstrap: Arc<Mutex<BootstrapState>>,
    recovery_capability: Arc<String>,
    cookie_name: Arc<String>,
    cookie_value: Arc<String>,
    bootstrap_path: Arc<PathBuf>,
    last_activity: Arc<AtomicU64>,
    shutdown: Arc<Notify>,
    assets: Arc<AssetManifest>,
    tokens: Arc<Option<UtilityTokens>>,
}

struct BootstrapState {
    capability: String,
    used: bool,
    issued_at: u64,
}

#[derive(Debug, Deserialize)]
struct BootstrapForm {
    capability: String,
}

#[derive(Debug, Deserialize)]
struct RebootstrapForm {
    recovery_capability: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PollRequest {
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct SessionEvent {
    cursor: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewRequest {
    event_id: String,
    session_id: String,
    revision: u64,
    verdict: FeedbackVerdict,
    #[serde(default)]
    instruction: Option<String>,
    #[serde(default)]
    notes: Vec<ReviewNote>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewNote {
    client_id: String,
    block_id: String,
    block_label: String,
    kind: FeedbackKind,
    body: String,
    #[serde(default)]
    selector: Option<TextSelector>,
    #[serde(default)]
    element_selector: Option<ElementSelector>,
    #[serde(default)]
    region_selector: Option<RegionSelector>,
}

#[derive(Debug, Serialize)]
struct ReviewResponse {
    event_id: Uuid,
    state: &'static str,
}

/// Run one session-scoped service until close or idle expiry.
pub async fn serve_session(project: PathBuf, session_id: Uuid) -> Result<()> {
    let store = SessionStore::discover(&project)?;
    let _service_lease = store.acquire_service_lease(session_id)?;
    let session = store.load(session_id)?;
    if session.status != SessionStatus::Active {
        return Err(PresentError::SessionClosed(session_id.to_string()));
    }
    let assets = Arc::new(load_manifest()?);
    let tokens = Arc::new(store.utility_tokens()?);
    let (bootstrap_path, ready_path) = prepare_runtime_paths(&store, session_id)?;
    let _runtime_cleanup = RuntimeCleanup::new(
        store.clone(),
        session_id,
        bootstrap_path.clone(),
        ready_path.clone(),
    );

    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()))?
        .port();
    let authority = format!("127.0.0.1:{port}");
    let instance_id = Uuid::new_v4();
    let capability = random_hex(32)?;
    let recovery_capability = random_hex(32)?;
    let cookie_value = random_hex(32)?;
    let cookie_name = format!("cf_present_{}", session_id.simple());
    let bootstrap_html = render_bootstrap(&authority, &capability);
    {
        let bootstrap_bytes = u64::try_from(bootstrap_html.len())
            .map_err(|_| PresentError::ServiceUnavailable("bootstrap size overflow".to_string()))?;
        let _runtime_lease = store.prepare_runtime_control_mutation(session_id, bootstrap_bytes)?;
        write_bootstrap(&bootstrap_path, &bootstrap_html)?;
    }
    store.set_service(session_id, port, std::process::id(), instance_id)?;
    let _service_registration = ServiceRegistration::new(store.clone(), session_id, instance_id);
    let ready = ReadyRecord {
        schema_version: 1,
        session_id,
        port,
        instance_id,
        bootstrap_path: bootstrap_path.clone(),
        recovery_capability: recovery_capability.clone(),
    };
    write_ready_record(&store, session_id, &ready_path, &ready)?;

    let now = now_unix();
    let state = AppState {
        store,
        session_id,
        instance_id,
        authority,
        bootstrap: Arc::new(Mutex::new(BootstrapState {
            capability,
            used: false,
            issued_at: now,
        })),
        recovery_capability: Arc::new(recovery_capability),
        cookie_name: Arc::new(cookie_name),
        cookie_value: Arc::new(cookie_value),
        bootstrap_path: Arc::new(bootstrap_path.clone()),
        last_activity: Arc::new(AtomicU64::new(now)),
        shutdown: Arc::new(Notify::new()),
        assets,
        tokens,
    };

    let monitor_state = state.clone();
    let monitor = tokio::spawn(async move { monitor_session(monitor_state).await });
    let shutdown = state.shutdown.clone();
    let app = Router::new()
        .route("/bootstrap", post(bootstrap))
        .route("/_cf-present/rebootstrap/{instance_id}", post(rebootstrap))
        .route("/_cf-present/health/{instance_id}", get(health))
        .route("/app/", get(application))
        .route("/app/assets/{*path}", get(asset))
        .route("/app/api/reviews", post(submit_review))
        .route("/app/api/events/poll", post(poll_events))
        .route("/sandbox/{revision}/{id}", get(sandbox))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(limits::MAX_FEEDBACK_BYTES))
        .with_state(state);

    let serve_result = axum::serve(listener, app)
        .with_graceful_shutdown(async move { shutdown.notified().await })
        .await
        .map_err(|error| PresentError::ServiceUnavailable(error.to_string()));
    monitor.abort();
    serve_result
}

fn prepare_runtime_paths(store: &SessionStore, session_id: Uuid) -> Result<(PathBuf, PathBuf)> {
    let runtime_dir = store.runtime_dir(session_id)?;
    let profile_dir = runtime_dir.join("browser-profile");
    create_private_dir_all(&profile_dir)?;
    let control_dir = runtime_dir.join("control");
    create_private_dir_all(&control_dir)?;
    let bootstrap_path = control_dir.join("bootstrap.html");
    let ready_path = control_dir.join("ready.json");
    let _runtime_lease = store.prepare_runtime_control_mutation(session_id, 0)?;
    remove_if_regular(&bootstrap_path)?;
    remove_if_regular(&ready_path)?;
    Ok((bootstrap_path, ready_path))
}

fn write_ready_record(
    store: &SessionStore,
    session_id: Uuid,
    ready_path: &std::path::Path,
    ready: &ReadyRecord,
) -> Result<()> {
    let bytes = u64::try_from(serde_json::to_vec_pretty(ready)?.len() + 1)
        .map_err(|_| PresentError::ServiceUnavailable("ready record size overflow".to_string()))?;
    let _runtime_lease = store.prepare_runtime_control_mutation(session_id, bytes)?;
    write_json_atomic(ready_path, ready)
}

async fn health(
    State(state): State<AppState>,
    Path(instance_id): Path<String>,
    headers: HeaderMap,
) -> Response<Body> {
    if let Err(response) = require_host(&state, &headers) {
        return response;
    }
    if Uuid::parse_str(&instance_id).ok() != Some(state.instance_id) {
        return plain(StatusCode::NOT_FOUND, "service instance not found");
    }
    let body = match serde_json::to_vec(&HealthRecord {
        schema_version: 1,
        session_id: state.session_id,
        instance_id: state.instance_id,
    }) {
        Ok(body) => body,
        Err(error) => return plain(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };
    response_with_headers(
        StatusCode::OK,
        Body::from(body),
        &[
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
    )
}

async fn bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<BootstrapForm>,
) -> Response<Body> {
    if let Err(response) = require_host(&state, &headers) {
        return response;
    }
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if !matches!(origin, None | Some("null")) {
        return plain(StatusCode::FORBIDDEN, "bootstrap Origin is not allowed");
    }
    let Ok(mut bootstrap_state) = state.bootstrap.lock() else {
        return plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            "bootstrap state is unavailable",
        );
    };
    if now_unix().saturating_sub(bootstrap_state.issued_at) > limits::BOOTSTRAP_TTL_SECONDS {
        drop(bootstrap_state);
        expire_unbootstrapped_session(&state);
        return plain(
            StatusCode::GONE,
            "bootstrap expired; reopen the presentation",
        );
    }
    if bootstrap_state.used
        || !constant_time_equal(
            form.capability.as_bytes(),
            bootstrap_state.capability.as_bytes(),
        )
    {
        return plain(
            StatusCode::UNAUTHORIZED,
            "invalid or consumed bootstrap capability",
        );
    }
    bootstrap_state.used = true;
    drop(bootstrap_state);
    let _ = remove_runtime_control(&state.store, state.session_id, &state.bootstrap_path);
    state.last_activity.store(now_unix(), Ordering::Release);
    let cookie = format!(
        "{}={}; Path=/app; HttpOnly; SameSite=Strict",
        state.cookie_name, state.cookie_value
    );
    // A cross-site `file:` form submission cannot carry a SameSite=Strict
    // cookie through an HTTP redirect chain. Finish the POST at the loopback
    // origin, set the cookie, then replace from that same-origin document.
    // The bootstrap response contains no capability or session value.
    response_with_headers(
        StatusCode::OK,
        Body::from(BOOTSTRAP_HANDOFF),
        &[
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::SET_COOKIE, &cookie),
            (header::CACHE_CONTROL, "no-store"),
            (header::CONTENT_SECURITY_POLICY, BOOTSTRAP_HANDOFF_CSP),
            (header::REFERRER_POLICY, "no-referrer"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
    )
}

async fn rebootstrap(
    State(state): State<AppState>,
    Path(instance_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<RebootstrapForm>,
) -> Response<Body> {
    if let Err(response) = require_host(&state, &headers) {
        return response;
    }
    if Uuid::parse_str(&instance_id).ok() != Some(state.instance_id)
        || !constant_time_equal(
            form.recovery_capability.as_bytes(),
            state.recovery_capability.as_bytes(),
        )
    {
        return plain(StatusCode::UNAUTHORIZED, "invalid recovery capability");
    }
    let Ok(capability) = random_hex(32) else {
        return plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            "bootstrap rotation failed",
        );
    };
    let bootstrap_html = render_bootstrap(&state.authority, &capability);
    let bootstrap_bytes = u64::try_from(bootstrap_html.len()).map_err(|_| ());
    let rotated = bootstrap_bytes
        .map_err(|()| PresentError::ServiceUnavailable("bootstrap size overflow".to_string()))
        .and_then(|bytes| {
            state
                .store
                .prepare_runtime_control_mutation(state.session_id, bytes)
        })
        .and_then(|_lease| {
            remove_if_regular(&state.bootstrap_path)?;
            write_bootstrap(&state.bootstrap_path, &bootstrap_html)
        });
    if rotated.is_err() {
        return plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            "bootstrap rotation failed",
        );
    }
    let Ok(mut bootstrap) = state.bootstrap.lock() else {
        return plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            "bootstrap state is unavailable",
        );
    };
    *bootstrap = BootstrapState {
        capability,
        used: false,
        issued_at: now_unix(),
    };
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .expect("static recovery response is valid")
}

async fn application(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    if let Err(response) = require_application_request(&state, &headers, false) {
        return response;
    }
    state.last_activity.store(now_unix(), Ordering::Release);
    let revision = match state.store.current_revision(state.session_id) {
        Ok(revision) => revision,
        Err(error) => return plain(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };
    let event_sequence = match state.store.latest_event_sequence(state.session_id) {
        Ok(sequence) => sequence,
        Err(error) => return plain(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };
    let manifest = &state.assets.service;
    let style = manifest
        .entrypoints
        .get("present.style")
        .map(String::as_str);
    let script = manifest.entrypoints.get("present.app").map(String::as_str);
    let prepaint = manifest.inline.get("present.prepaint");
    let utility_style = state.tokens.as_ref().as_ref().map(UtilityTokens::css);
    let identity_src = state
        .tokens
        .as_ref()
        .as_ref()
        .and_then(UtilityTokens::identity_data_url);
    let identity = state.tokens.as_ref().as_ref().and_then(|tokens| {
        Some(RenderIdentity {
            src: identity_src.as_deref()?,
            alt: &tokens.identity.as_ref()?.alt,
        })
    });
    let feedback = match state.store.feedback_snapshot(state.session_id) {
        Ok(feedback) => feedback,
        Err(error) => return plain(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };
    let body = match revision.content {
        RevisionContent::Supported { document } => render_document(
            &document,
            &RenderOptions {
                session_id: &state.session_id.to_string(),
                revision: revision.revision,
                event_sequence,
                script_path: script,
                style_path: style,
                prepaint_source: prepaint.map(|asset| asset.source.as_str()),
                utility_style: utility_style.as_deref(),
                identity,
                feedback: Some(&feedback),
                read_only_warning: None,
                interactive: true,
            },
        ),
        RevisionContent::Unsupported {
            schema_version,
            raw,
        } => render_unsupported(&raw, schema_version),
    };
    let prepaint_hash = prepaint
        .map(|asset| format!("'{}'", asset.csp_sha256))
        .unwrap_or_default();
    // Mermaid necessarily creates transient inline SVG styles while rendering.
    // The document renderer drops raw HTML, isolates explicit HTML blocks, and
    // the final SVG sanitizer rejects active attributes and external CSS URLs.
    // Split directives keep scripts strict while allowing only local/inline CSS.
    let csp = format!(
        "default-src 'none'; script-src 'self' {prepaint_hash}; style-src 'self'; style-src-elem 'self' 'unsafe-inline'; style-src-attr 'unsafe-inline'; img-src data: blob:; media-src data:; connect-src 'self'; frame-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    );
    secure_html(StatusCode::OK, body, &csp)
}

fn csp_source_hash(source: &str) -> String {
    format!(
        "'sha256-{}'",
        STANDARD.encode(sha2::Sha256::digest(source.as_bytes()))
    )
}

async fn asset(
    State(state): State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Response<Body> {
    if let Err(response) = require_application_request(&state, &headers, false) {
        return response;
    }
    if !accepts_brotli(&headers) {
        return plain(
            StatusCode::NOT_ACCEPTABLE,
            "This browser route is not qualified because it does not advertise Brotli. Close it and run `codeflow present show <session-id> --no-launch` with a qualified browser.",
        );
    }
    let request_path = format!("/app/assets/{path}");
    let Some(asset) = state
        .assets
        .service
        .assets
        .iter()
        .find(|asset| asset.request_path == request_path)
    else {
        return plain(StatusCode::NOT_FOUND, "asset not found");
    };
    if asset.content_encoding != "br" {
        return plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            "asset manifest encoding is invalid",
        );
    }
    let Some(bytes) = EmbeddedAssets::get(&asset.stored_path) else {
        return plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            "embedded asset is missing",
        );
    };
    if u64::try_from(bytes.data.len()).ok() != Some(asset.encoded_bytes) {
        return plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            "embedded asset size does not match its manifest",
        );
    }
    response_with_headers(
        StatusCode::OK,
        Body::from(bytes.data.into_owned()),
        &[
            (header::CONTENT_TYPE, &asset.media_type),
            (header::CONTENT_ENCODING, "br"),
            (header::VARY, "Accept-Encoding"),
            (header::CACHE_CONTROL, "public,max-age=31536000,immutable"),
            (header::ETAG, &asset.etag),
            (HeaderName::from_static("x-content-type-options"), "nosniff"),
        ],
    )
}

async fn submit_review(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ReviewRequest>,
) -> Response<Body> {
    if let Err(response) = require_application_request(&state, &headers, true) {
        return response;
    }
    let Ok(session_id) = Uuid::parse_str(&request.session_id) else {
        return plain(StatusCode::BAD_REQUEST, "review session id is invalid");
    };
    let mut notes = Vec::with_capacity(request.notes.len());
    for note in request.notes {
        let Ok(id) = Uuid::parse_str(&note.client_id) else {
            return plain(StatusCode::BAD_REQUEST, "review note id is invalid");
        };
        notes.push(FeedbackNote {
            id,
            block_id: note.block_id,
            block_label: note.block_label,
            kind: note.kind,
            body: note.body,
            selector: note.selector,
            element_selector: note.element_selector,
            region_selector: note.region_selector,
        });
    }
    let Ok(event_id) = Uuid::parse_str(&request.event_id) else {
        return plain(StatusCode::BAD_REQUEST, "review event id is invalid");
    };
    let envelope = FeedbackEnvelope {
        event_id,
        session_id,
        revision: request.revision,
        actor: "operator".to_string(),
        verdict: request.verdict,
        instruction: request.instruction,
        notes,
        created_at_unix: 0,
    };
    match state.store.append_feedback(envelope) {
        Ok(outcome) => {
            state.last_activity.store(now_unix(), Ordering::Release);
            let body = serde_json::to_vec(&ReviewResponse {
                event_id,
                state: if outcome.created {
                    "received"
                } else {
                    "duplicate"
                },
            })
            .unwrap_or_else(|_| b"{}".to_vec());
            response_with_headers(
                if outcome.created {
                    StatusCode::CREATED
                } else {
                    StatusCode::OK
                },
                Body::from(body),
                &[
                    (header::CONTENT_TYPE, "application/json"),
                    (header::CACHE_CONTROL, "no-store"),
                ],
            )
        }
        Err(PresentError::SessionClosed(_)) => plain(StatusCode::GONE, "session is closed"),
        Err(error) => plain(StatusCode::BAD_REQUEST, &error.to_string()),
    }
}

async fn poll_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PollRequest>,
) -> Response<Body> {
    if let Err(response) = require_application_request(&state, &headers, true) {
        return response;
    }
    let initial_session = match state.store.load(state.session_id) {
        Ok(session) => session,
        Err(error) => return plain(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };
    let initial_sequence = match state.store.latest_event_sequence(state.session_id) {
        Ok(sequence) => sequence,
        Err(error) => return plain(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };
    let (after_revision, after_sequence) = match request.cursor.as_deref() {
        None => (initial_session.current_revision, initial_sequence),
        Some(cursor) => match parse_event_cursor(cursor) {
            Some(cursor) => cursor,
            None => return plain(StatusCode::BAD_REQUEST, "event cursor is invalid"),
        },
    };
    state.last_activity.store(now_unix(), Ordering::Release);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(limits::EVENT_POLL_SECONDS);
    loop {
        let session = match state.store.load(state.session_id) {
            Ok(session) => session,
            Err(error) => return plain(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        };
        let latest = match state.store.latest_event_sequence(state.session_id) {
            Ok(latest) => latest,
            Err(error) => return plain(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        };
        let terminal = session.status == SessionStatus::Closed;
        let revised = session.current_revision > after_revision;
        if revised || latest > after_sequence || terminal || tokio::time::Instant::now() >= deadline
        {
            let response = SessionEvent {
                cursor: format!("{}:{latest}", session.current_revision),
                kind: if terminal {
                    "session_closed"
                } else if revised {
                    "revision"
                } else {
                    "feedback_state"
                },
                message: if revised {
                    Some(format!(
                        "Revision {} is available.",
                        session.current_revision
                    ))
                } else {
                    (latest > after_sequence).then(|| "Review state changed.".to_string())
                },
            };
            let body = match serde_json::to_vec(&response) {
                Ok(body) if body.len() <= limits::MAX_EVENT_RESPONSE_BYTES => body,
                Ok(_) => {
                    return plain(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "event response exceeded its bound",
                    )
                }
                Err(error) => return plain(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
            };
            return response_with_headers(
                StatusCode::OK,
                Body::from(body),
                &[
                    (header::CONTENT_TYPE, "application/json"),
                    (header::CACHE_CONTROL, "no-store"),
                ],
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn parse_event_cursor(cursor: &str) -> Option<(u64, u64)> {
    let (revision, sequence) = cursor.split_once(':')?;
    Some((revision.parse().ok()?, sequence.parse().ok()?))
}

async fn sandbox(
    State(state): State<AppState>,
    Path((revision, id)): Path<(u64, String)>,
    headers: HeaderMap,
) -> Response<Body> {
    if let Err(response) = require_host(&state, &headers) {
        return response;
    }
    let Ok(revision) = state.store.revision(state.session_id, revision) else {
        return plain(StatusCode::NOT_FOUND, "sandbox revision not found");
    };
    let sandbox = sandbox_blocks(state.session_id, &revision.content);
    let Some(html) = sandbox.get(&id) else {
        return plain(StatusCode::NOT_FOUND, "sandbox block not found");
    };
    let csp = format!(
        "default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:; media-src data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors http://{}",
        state.authority
    );
    response_with_headers(
        StatusCode::OK,
        Body::from(html.clone()),
        &[
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (HeaderName::from_static("content-security-policy"), &csp),
            (HeaderName::from_static("x-content-type-options"), "nosniff"),
            (HeaderName::from_static("referrer-policy"), "no-referrer"),
            (HeaderName::from_static("x-frame-options"), "SAMEORIGIN"),
        ],
    )
}

async fn not_found() -> Response<Body> {
    plain(StatusCode::NOT_FOUND, "not found")
}

#[allow(
    clippy::result_large_err,
    reason = "Axum responses preserve exact request-rejection headers at this private boundary"
)]
fn require_application_request(
    state: &AppState,
    headers: &HeaderMap,
    mutation: bool,
) -> std::result::Result<(), Response<Body>> {
    require_host(state, headers)?;
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let authenticated = cookie.split(';').map(str::trim).any(|part| {
        part.split_once('=').is_some_and(|(name, value)| {
            name == state.cookie_name.as_str()
                && constant_time_equal(value.as_bytes(), state.cookie_value.as_bytes())
        })
    });
    if !authenticated {
        return Err(plain(
            StatusCode::UNAUTHORIZED,
            "missing presentation session cookie",
        ));
    }
    if mutation {
        let expected = format!("http://{}", state.authority);
        if headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            != Some(expected.as_str())
        {
            return Err(plain(
                StatusCode::FORBIDDEN,
                "Origin does not match this session",
            ));
        }
        if headers
            .get(HeaderName::from_static("x-cf-present"))
            .and_then(|value| value.to_str().ok())
            != Some("1")
        {
            return Err(plain(
                StatusCode::FORBIDDEN,
                "missing presentation request marker",
            ));
        }
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !content_type.eq_ignore_ascii_case("application/json") {
            return Err(plain(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "expected application/json",
            ));
        }
    }
    Ok(())
}

#[allow(
    clippy::result_large_err,
    reason = "Axum responses preserve exact request-rejection headers at this private boundary"
)]
fn require_host(state: &AppState, headers: &HeaderMap) -> std::result::Result<(), Response<Body>> {
    if headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        != Some(state.authority.as_str())
    {
        return Err(plain(
            StatusCode::MISDIRECTED_REQUEST,
            "Host does not match this session",
        ));
    }
    Ok(())
}

fn accepts_brotli(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').any(|encoding| {
                encoding
                    .split(';')
                    .next()
                    .is_some_and(|name| name.trim().eq_ignore_ascii_case("br"))
            })
        })
}

fn secure_html(status: StatusCode, body: String, csp: &str) -> Response<Body> {
    response_with_headers(
        status,
        Body::from(body),
        &[
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (HeaderName::from_static("content-security-policy"), csp),
            (HeaderName::from_static("x-content-type-options"), "nosniff"),
            (HeaderName::from_static("referrer-policy"), "no-referrer"),
            (HeaderName::from_static("x-frame-options"), "DENY"),
        ],
    )
}

fn plain(status: StatusCode, message: &str) -> Response<Body> {
    response_with_headers(
        status,
        Body::from(message.to_string()),
        &[
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (HeaderName::from_static("x-content-type-options"), "nosniff"),
        ],
    )
}

fn response_with_headers(
    status: StatusCode,
    body: Body,
    headers: &[(HeaderName, &str)],
) -> Response<Body> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    for (name, value) in headers {
        if let Ok(value) = HeaderValue::from_str(value) {
            response.headers_mut().insert(name.clone(), value);
        } else {
            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
    response
}

pub(crate) fn load_manifest() -> Result<AssetManifest> {
    static MANIFEST: OnceLock<AssetManifest> = OnceLock::new();
    if let Some(manifest) = MANIFEST.get() {
        return Ok(manifest.clone());
    }
    let bytes = EmbeddedAssets::get("manifest.json").ok_or_else(|| {
        PresentError::CorruptState("embedded renderer manifest is missing".to_string())
    })?;
    let manifest: AssetManifest = serde_json::from_slice(&bytes.data)?;
    validate_manifest(&manifest)?;
    let _ = MANIFEST.set(manifest.clone());
    Ok(manifest)
}

fn validate_manifest(manifest: &AssetManifest) -> Result<()> {
    if manifest.schema_version != 1 {
        return Err(PresentError::CorruptState(
            "embedded renderer manifest version is unsupported".to_string(),
        ));
    }
    for required in ["present.app", "present.style"] {
        if !manifest.service.entrypoints.contains_key(required) {
            return Err(PresentError::CorruptState(format!(
                "renderer entrypoint {required} is missing"
            )));
        }
    }
    let Some(prepaint) = manifest.service.inline.get("present.prepaint") else {
        return Err(PresentError::CorruptState(
            "renderer prepaint source is missing".to_string(),
        ));
    };
    if prepaint.source.len() > 64 * 1024
        || prepaint.csp_sha256 != csp_source_hash(&prepaint.source).trim_matches('\'')
    {
        return Err(PresentError::CorruptState(
            "renderer prepaint source violates its CSP integrity contract".to_string(),
        ));
    }
    let mut total = 0_u64;
    for asset in &manifest.service.assets {
        if !asset.request_path.starts_with("/app/assets/")
            || asset.request_path.contains("..")
            || !asset.stored_path.starts_with("service/")
            || asset.stored_path.contains("..")
            || asset.content_encoding != "br"
            || asset.encoded_bytes > limits::MAX_BROTLI_CHUNK_BYTES
            || asset.sha256.len() != 64
        {
            return Err(PresentError::CorruptState(format!(
                "renderer asset {} violates the manifest contract",
                asset.request_path
            )));
        }
        total = total.checked_add(asset.encoded_bytes).ok_or_else(|| {
            PresentError::CorruptState("renderer asset byte total overflow".to_string())
        })?;
        let bytes = EmbeddedAssets::get(&asset.stored_path).ok_or_else(|| {
            PresentError::CorruptState(format!("renderer asset {} is missing", asset.stored_path))
        })?;
        let digest = sha2::Sha256::digest(&bytes.data);
        let mut actual = String::with_capacity(digest.len() * 2);
        for byte in digest {
            write!(actual, "{byte:02x}").expect("writing to a String cannot fail");
        }
        if actual != asset.sha256 {
            return Err(PresentError::CorruptState(format!(
                "renderer asset {} does not match its SHA-256",
                asset.stored_path
            )));
        }
    }
    if total > limits::MAX_BROTLI_ASSET_BYTES {
        return Err(PresentError::CorruptState(format!(
            "renderer Brotli corpus exceeds {} bytes",
            limits::MAX_BROTLI_ASSET_BYTES
        )));
    }
    Ok(())
}

fn sandbox_blocks(session_id: Uuid, content: &RevisionContent) -> HashMap<String, String> {
    let mut output = HashMap::new();
    if let RevisionContent::Supported { document } = content {
        collect_sandbox(session_id, &document.blocks, &mut output);
    }
    output
}

fn collect_sandbox(session_id: Uuid, blocks: &[Block], output: &mut HashMap<String, String>) {
    for block in blocks {
        match block {
            Block::Html { id, html, .. } => {
                output.insert(sandbox_id(&session_id.to_string(), id), html.clone());
            }
            Block::Disclosure { blocks, .. } => collect_sandbox(session_id, blocks, output),
            Block::Tabs { tabs, .. } => {
                for tab in tabs {
                    collect_sandbox(session_id, &tab.blocks, output);
                }
            }
            _ => {}
        }
    }
}

fn render_bootstrap(authority: &str, capability: &str) -> String {
    let endpoint = format!("http://{authority}/bootstrap");
    format!(
        "<!doctype html><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\"><title>Opening presentation</title><form id=\"bootstrap\" method=\"post\" action=\"{endpoint}\"><input type=\"hidden\" name=\"capability\" value=\"{capability}\"><noscript><button type=\"submit\">Open presentation</button></noscript></form><script>document.getElementById('bootstrap').submit()</script>"
    )
}

fn write_bootstrap(path: &std::path::Path, html: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| PresentError::UnsafePath(path.to_path_buf()))?;
    create_private_dir_all(parent)?;
    let mut file = crate::state::open_private_create_new(path)?;
    file.write_all(html.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| PresentError::io(path, error))
}

async fn monitor_session(state: AppState) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let now = now_unix();
        let bootstrap_expired = state.bootstrap.lock().is_ok_and(|bootstrap| {
            !bootstrap.used
                && now.saturating_sub(bootstrap.issued_at) > limits::BOOTSTRAP_TTL_SECONDS
        });
        if bootstrap_expired {
            expire_unbootstrapped_session(&state);
            return;
        }
        if now.saturating_sub(state.last_activity.load(Ordering::Acquire))
            > limits::SESSION_IDLE_SECONDS
        {
            let _ = state.store.close(state.session_id);
            cleanup_owned_browser(&state);
            state.shutdown.notify_waiters();
            return;
        }
        match state.store.load(state.session_id) {
            Ok(session) if session.status == SessionStatus::Closed => {
                cleanup_owned_browser(&state);
                state.shutdown.notify_waiters();
                return;
            }
            Ok(_) => {}
            Err(_) => {
                state.shutdown.notify_waiters();
                return;
            }
        }
    }
}

fn expire_unbootstrapped_session(state: &AppState) {
    let _ = remove_runtime_control(&state.store, state.session_id, &state.bootstrap_path);
    let _ = state.store.close(state.session_id);
    cleanup_owned_browser(state);
    state.shutdown.notify_waiters();
}

fn cleanup_owned_browser(state: &AppState) {
    let Ok(session) = state.store.load(state.session_id) else {
        return;
    };
    let (Some(pid), Some(instance_id)) = (session.browser_pid, session.browser_instance) else {
        return;
    };
    let Ok(profile) = state
        .store
        .runtime_dir(state.session_id)
        .map(|runtime| runtime.join("browser-profile"))
    else {
        return;
    };
    let _ = crate::browser::terminate_isolated(
        &state.store,
        state.session_id,
        pid,
        instance_id,
        &profile,
    );
}

struct RuntimeCleanup {
    store: SessionStore,
    session_id: Uuid,
    bootstrap_path: PathBuf,
    ready_path: PathBuf,
}

struct ServiceRegistration {
    store: SessionStore,
    session_id: Uuid,
    instance_id: Uuid,
}

impl ServiceRegistration {
    fn new(store: SessionStore, session_id: Uuid, instance_id: Uuid) -> Self {
        Self {
            store,
            session_id,
            instance_id,
        }
    }
}

impl Drop for ServiceRegistration {
    fn drop(&mut self) {
        if self
            .store
            .clear_service(self.session_id, self.instance_id)
            .unwrap_or(false)
        {
            let _ = self.store.enforce_retention();
        }
    }
}

impl RuntimeCleanup {
    fn new(
        store: SessionStore,
        session_id: Uuid,
        bootstrap_path: PathBuf,
        ready_path: PathBuf,
    ) -> Self {
        Self {
            store,
            session_id,
            bootstrap_path,
            ready_path,
        }
    }
}

impl Drop for RuntimeCleanup {
    fn drop(&mut self) {
        let _ = remove_runtime_control(&self.store, self.session_id, &self.bootstrap_path);
        let _ = remove_runtime_control(&self.store, self.session_id, &self.ready_path);
    }
}

fn remove_runtime_control(
    store: &SessionStore,
    session_id: Uuid,
    path: &std::path::Path,
) -> Result<()> {
    let _runtime_lease = store.prepare_runtime_control_mutation(session_id, 0)?;
    remove_if_regular(path)
}

fn remove_if_regular(path: &std::path::Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_link_like(&metadata) => {
            fs::remove_file(path).map_err(|error| PresentError::io(path, error))
        }
        Ok(_) => Err(PresentError::UnsafePath(path.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PresentError::io(path, error)),
    }
}

fn random_hex(bytes: usize) -> Result<String> {
    let mut value = vec![0_u8; bytes];
    fill_random(&mut value).map_err(|error| {
        PresentError::ServiceUnavailable(format!("random source failed: {error}"))
    })?;
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(output)
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Block, ParsedDocument, PresentationDocument, Provenance};

    fn parsed_with(block: Block) -> ParsedDocument {
        ParsedDocument::Supported(PresentationDocument {
            schema_version: 1,
            title: "Review".to_string(),
            language: None,
            provenance: Provenance::default(),
            blocks: vec![block],
        })
    }

    fn app_state() -> (tempfile::TempDir, AppState) {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore::at_root(temp.path().join("project"), "key".to_string()).unwrap();
        let session = store
            .create(ParsedDocument::Supported(PresentationDocument {
                schema_version: 1,
                title: "Review".to_string(),
                language: None,
                provenance: Provenance::default(),
                blocks: vec![Block::Narrative {
                    id: "intro".to_string(),
                    markdown: "Hello".to_string(),
                }],
            }))
            .unwrap();
        let runtime = store.runtime_dir(session.id).unwrap();
        let control = runtime.join("control");
        crate::state::create_private_dir_all(&control).unwrap();
        let bootstrap_path = control.join("bootstrap.html");
        write_bootstrap(&bootstrap_path, "bootstrap").unwrap();
        let now = now_unix();
        let state = AppState {
            store,
            session_id: session.id,
            instance_id: Uuid::new_v4(),
            authority: "127.0.0.1:43210".to_string(),
            bootstrap: Arc::new(Mutex::new(BootstrapState {
                capability: "capability".to_string(),
                used: false,
                issued_at: now,
            })),
            recovery_capability: Arc::new("recovery".to_string()),
            cookie_name: Arc::new("cf_present_test".to_string()),
            cookie_value: Arc::new("cookie".to_string()),
            bootstrap_path: Arc::new(bootstrap_path),
            last_activity: Arc::new(AtomicU64::new(now)),
            shutdown: Arc::new(Notify::new()),
            assets: Arc::new(load_manifest().unwrap()),
            tokens: Arc::new(None),
        };
        (temp, state)
    }

    fn application_headers(state: &AppState, mutation: bool) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::HOST,
            HeaderValue::from_str(&state.authority).unwrap(),
        );
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{}={}", state.cookie_name, state.cookie_value))
                .unwrap(),
        );
        if mutation {
            headers.insert(
                header::ORIGIN,
                HeaderValue::from_str(&format!("http://{}", state.authority)).unwrap(),
            );
            headers.insert(
                HeaderName::from_static("x-cf-present"),
                HeaderValue::from_static("1"),
            );
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
        }
        headers
    }

    #[test]
    fn expired_bootstrap_closes_session_and_removes_capability_file() {
        let (_temp, state) = app_state();
        assert!(state.bootstrap_path.exists());

        expire_unbootstrapped_session(&state);

        assert!(!state.bootstrap_path.exists());
        assert_eq!(
            state.store.load(state.session_id).unwrap().status,
            SessionStatus::Closed
        );
    }

    #[test]
    fn tokens_compare_without_prefix_acceptance() {
        assert!(constant_time_equal(b"same", b"same"));
        assert!(!constant_time_equal(b"same", b"same-more"));
        assert!(!constant_time_equal(b"same", b"diff"));
    }

    #[test]
    fn accept_encoding_requires_brotli_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("gzip, br;q=1"),
        );
        assert!(accepts_brotli(&headers));
        headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
        assert!(!accepts_brotli(&headers));
    }

    #[test]
    fn request_matrix_requires_exact_host_cookie_and_mutation_proofs() {
        let (_temp, state) = app_state();
        let read = application_headers(&state, false);
        assert!(require_application_request(&state, &read, false).is_ok());

        let mut wrong_host = read.clone();
        wrong_host.insert(header::HOST, HeaderValue::from_static("localhost:43210"));
        assert_eq!(
            require_application_request(&state, &wrong_host, false)
                .unwrap_err()
                .status(),
            StatusCode::MISDIRECTED_REQUEST
        );

        let mut no_cookie = read;
        no_cookie.remove(header::COOKIE);
        assert_eq!(
            require_application_request(&state, &no_cookie, false)
                .unwrap_err()
                .status(),
            StatusCode::UNAUTHORIZED
        );

        let mutation = application_headers(&state, true);
        assert!(require_application_request(&state, &mutation, true).is_ok());
        for header_name in [header::ORIGIN, HeaderName::from_static("x-cf-present")] {
            let mut missing = mutation.clone();
            missing.remove(header_name);
            assert_eq!(
                require_application_request(&state, &missing, true)
                    .unwrap_err()
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
        let mut wrong_type = mutation;
        wrong_type.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        assert_eq!(
            require_application_request(&state, &wrong_type, true)
                .unwrap_err()
                .status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
    }

    #[tokio::test]
    async fn bootstrap_capability_is_single_use_and_origin_bounded() {
        let (_temp, state) = app_state();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::HOST,
            HeaderValue::from_str(&state.authority).unwrap(),
        );
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://example.com"),
        );
        let rejected = bootstrap(
            State(state.clone()),
            headers.clone(),
            Form(BootstrapForm {
                capability: "capability".to_string(),
            }),
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

        headers.insert(header::ORIGIN, HeaderValue::from_static("null"));
        let accepted = bootstrap(
            State(state.clone()),
            headers.clone(),
            Form(BootstrapForm {
                capability: "capability".to_string(),
            }),
        )
        .await;
        assert_eq!(accepted.status(), StatusCode::OK);
        assert!(accepted.headers().get(header::SET_COOKIE).is_some());
        assert!(!state.bootstrap_path.exists());

        let replay = bootstrap(
            State(state),
            headers,
            Form(BootstrapForm {
                capability: "capability".to_string(),
            }),
        )
        .await;
        assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn recovery_capability_rotates_a_consumed_bootstrap() {
        let (_temp, state) = app_state();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::HOST,
            HeaderValue::from_str(&state.authority).unwrap(),
        );
        let consumed = bootstrap(
            State(state.clone()),
            headers.clone(),
            Form(BootstrapForm {
                capability: "capability".to_string(),
            }),
        )
        .await;
        assert_eq!(consumed.status(), StatusCode::OK);

        let rejected = rebootstrap(
            State(state.clone()),
            Path(state.instance_id.to_string()),
            headers.clone(),
            Form(RebootstrapForm {
                recovery_capability: "wrong".to_string(),
            }),
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);

        let rotated = rebootstrap(
            State(state.clone()),
            Path(state.instance_id.to_string()),
            headers.clone(),
            Form(RebootstrapForm {
                recovery_capability: "recovery".to_string(),
            }),
        )
        .await;
        assert_eq!(rotated.status(), StatusCode::NO_CONTENT);
        let html = fs::read_to_string(&*state.bootstrap_path).unwrap();
        assert!(!html.contains("capability\" value=\"capability"));
        let capability = html
            .split_once("name=\"capability\" value=\"")
            .and_then(|(_, tail)| tail.split_once('\"').map(|(value, _)| value))
            .unwrap();
        let accepted = bootstrap(
            State(state),
            headers,
            Form(BootstrapForm {
                capability: capability.to_string(),
            }),
        )
        .await;
        assert_eq!(accepted.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn first_poll_after_render_observes_a_revision_created_in_between() {
        let (_temp, state) = app_state();
        let rendered = application(State(state.clone()), application_headers(&state, false)).await;
        assert_eq!(rendered.status(), StatusCode::OK);
        let rendered = axum::body::to_bytes(rendered.into_body(), limits::MAX_DOCUMENT_BYTES * 2)
            .await
            .unwrap();
        let rendered = std::str::from_utf8(&rendered).unwrap();
        assert!(
            rendered.contains("&quot;event_sequence&quot;:0")
                || rendered.contains("\"event_sequence\":0")
        );
        assert!(rendered.contains("&quot;revision&quot;:1") || rendered.contains("\"revision\":1"));

        state
            .store
            .update_document(
                state.session_id,
                parsed_with(Block::Narrative {
                    id: "intro".to_string(),
                    markdown: "Updated".to_string(),
                }),
            )
            .unwrap();
        let response = poll_events(
            State(state.clone()),
            application_headers(&state, true),
            Json(PollRequest {
                cursor: Some("1:0".to_string()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), limits::MAX_EVENT_RESPONSE_BYTES)
            .await
            .unwrap();
        let event: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(event["kind"], "revision");
        assert_eq!(event["cursor"], "2:0");
    }

    #[tokio::test]
    async fn sandbox_urls_are_bound_to_the_requested_immutable_revision() {
        let (_temp, state) = app_state();
        let opaque = sandbox_id(&state.session_id.to_string(), "sample");
        let revision_two = state
            .store
            .update_document(
                state.session_id,
                parsed_with(Block::Html {
                    id: "sample".to_string(),
                    html: "<p>revision two</p>".to_string(),
                    title: Some("Sample".to_string()),
                }),
            )
            .unwrap();
        let revision_three = state
            .store
            .update_document(
                state.session_id,
                parsed_with(Block::Html {
                    id: "sample".to_string(),
                    html: "<p>revision three</p>".to_string(),
                    title: Some("Sample".to_string()),
                }),
            )
            .unwrap();
        let revision_four = state
            .store
            .update_document(
                state.session_id,
                parsed_with(Block::Narrative {
                    id: "replacement".to_string(),
                    markdown: "No sandbox here.".to_string(),
                }),
            )
            .unwrap();

        for (revision, expected) in [
            (revision_two, "revision two"),
            (revision_three, "revision three"),
        ] {
            let response = sandbox(
                State(state.clone()),
                Path((revision, opaque.clone())),
                application_headers(&state, false),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert!(response
                .headers()
                .get("content-security-policy")
                .unwrap()
                .to_str()
                .unwrap()
                .contains("script-src 'none'"));
            let body = axum::body::to_bytes(response.into_body(), limits::MAX_HTML_BYTES)
                .await
                .unwrap();
            assert!(std::str::from_utf8(&body).unwrap().contains(expected));
        }

        let missing = sandbox(
            State(state.clone()),
            Path((revision_four, opaque)),
            application_headers(&state, false),
        )
        .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn runtime_cleanup_removes_regular_capability_files() {
        let (_temp, state) = app_state();
        let bootstrap = (*state.bootstrap_path).clone();
        let ready = bootstrap.with_file_name("ready.json");
        write_bootstrap(&ready, "identity").unwrap();
        drop(RuntimeCleanup::new(
            state.store,
            state.session_id,
            bootstrap.clone(),
            ready.clone(),
        ));
        assert!(!bootstrap.exists());
        assert!(!ready.exists());
    }
}
