use std::{
    collections::HashMap,
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, OnceLock,
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
use getrandom::fill as fill_random;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tokio::{net::TcpListener, sync::Notify};
use uuid::Uuid;

use crate::{
    document::Block,
    error::{PresentError, Result},
    limits,
    render::{render_document, render_unsupported, sandbox_id, RenderOptions},
    state::{
        create_private_dir_all, write_json_atomic, FeedbackEnvelope, FeedbackKind, FeedbackNote,
        FeedbackVerdict, RevisionContent, SessionStatus, SessionStore, TextSelector,
    },
};

#[derive(RustEmbed)]
#[folder = "assets/"]
pub(crate) struct EmbeddedAssets;

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
    capability: Arc<String>,
    cookie_name: Arc<String>,
    cookie_value: Arc<String>,
    bootstrap_path: Arc<PathBuf>,
    bootstrap_used: Arc<AtomicBool>,
    started_at: u64,
    last_activity: Arc<AtomicU64>,
    shutdown: Arc<Notify>,
    assets: Arc<AssetManifest>,
    sandbox: Arc<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct BootstrapForm {
    capability: String,
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
    let revision = store.current_revision(session_id)?;
    let assets = Arc::new(load_manifest()?);
    let runtime_dir = store.runtime_dir(session_id)?;
    let profile_dir = runtime_dir.join("browser-profile");
    create_private_dir_all(&profile_dir)?;
    let bootstrap_path = runtime_dir.join("bootstrap.html");
    let ready_path = runtime_dir.join("ready.json");
    remove_if_regular(&bootstrap_path)?;
    remove_if_regular(&ready_path)?;
    let _runtime_cleanup = RuntimeCleanup::new(bootstrap_path.clone(), ready_path.clone());

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
    let cookie_value = random_hex(32)?;
    let cookie_name = format!("cf_present_{}", session_id.simple());
    write_bootstrap(&bootstrap_path, &authority, &capability)?;
    store.set_service(session_id, port, std::process::id(), instance_id)?;
    write_json_atomic(
        &ready_path,
        &ReadyRecord {
            schema_version: 1,
            session_id,
            port,
            instance_id,
            bootstrap_path: bootstrap_path.clone(),
        },
    )?;

    let sandbox_map = Arc::new(sandbox_blocks(session_id, &revision.content));
    let now = now_unix();
    let state = AppState {
        store,
        session_id,
        instance_id,
        authority,
        capability: Arc::new(capability),
        cookie_name: Arc::new(cookie_name),
        cookie_value: Arc::new(cookie_value),
        bootstrap_path: Arc::new(bootstrap_path.clone()),
        bootstrap_used: Arc::new(AtomicBool::new(false)),
        started_at: now,
        last_activity: Arc::new(AtomicU64::new(now)),
        shutdown: Arc::new(Notify::new()),
        assets,
        sandbox: sandbox_map,
    };

    let monitor_state = state.clone();
    let monitor = tokio::spawn(async move { monitor_session(monitor_state).await });
    let shutdown = state.shutdown.clone();
    let app = Router::new()
        .route("/bootstrap", post(bootstrap))
        .route("/_cf-present/health/{instance_id}", get(health))
        .route("/app/", get(application))
        .route("/app/assets/{*path}", get(asset))
        .route("/app/api/reviews", post(submit_review))
        .route("/app/api/events/poll", post(poll_events))
        .route("/sandbox/{id}", get(sandbox))
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
    if now_unix().saturating_sub(state.started_at) > limits::BOOTSTRAP_TTL_SECONDS {
        return plain(
            StatusCode::GONE,
            "bootstrap expired; reopen the presentation",
        );
    }
    if state.bootstrap_used.load(Ordering::Acquire)
        || !constant_time_equal(form.capability.as_bytes(), state.capability.as_bytes())
    {
        return plain(
            StatusCode::UNAUTHORIZED,
            "invalid or consumed bootstrap capability",
        );
    }
    if state
        .bootstrap_used
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return plain(
            StatusCode::UNAUTHORIZED,
            "bootstrap capability already consumed",
        );
    }
    let _ = remove_if_regular(&state.bootstrap_path);
    state.last_activity.store(now_unix(), Ordering::Release);
    let cookie = format!(
        "{}={}; Path=/app; HttpOnly; SameSite=Strict",
        state.cookie_name, state.cookie_value
    );
    response_with_headers(
        StatusCode::SEE_OTHER,
        Body::empty(),
        &[
            (header::LOCATION, "/app/"),
            (header::SET_COOKIE, &cookie),
            (header::CACHE_CONTROL, "no-store"),
        ],
    )
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
    let manifest = &state.assets.service;
    let style = manifest
        .entrypoints
        .get("present.style")
        .map(String::as_str);
    let script = manifest.entrypoints.get("present.app").map(String::as_str);
    let prepaint = manifest.inline.get("present.prepaint");
    let body = match revision.content {
        RevisionContent::Supported { document } => render_document(
            &document,
            &RenderOptions {
                session_id: &state.session_id.to_string(),
                revision: revision.revision,
                script_path: script,
                style_path: style,
                prepaint_source: prepaint.map(|asset| asset.source.as_str()),
                read_only_warning: None,
                interactive: true,
            },
        ),
        RevisionContent::Unsupported {
            schema_version,
            raw,
        } => render_unsupported(&raw, schema_version),
    };
    let csp = format!(
        "default-src 'none'; script-src 'self' {}; style-src 'self'; img-src data: blob:; media-src data:; connect-src 'self'; frame-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        prepaint.map_or("", |asset| asset.csp_sha256.as_str())
    );
    secure_html(StatusCode::OK, body, &csp)
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
    let session_id = match Uuid::parse_str(&request.session_id) {
        Ok(session_id) => session_id,
        Err(_) => return plain(StatusCode::BAD_REQUEST, "review session id is invalid"),
    };
    let mut notes = Vec::with_capacity(request.notes.len());
    for note in request.notes {
        let id = match Uuid::parse_str(&note.client_id) {
            Ok(id) => id,
            Err(_) => return plain(StatusCode::BAD_REQUEST, "review note id is invalid"),
        };
        notes.push(FeedbackNote {
            id,
            block_id: note.block_id,
            block_label: note.block_label,
            kind: note.kind,
            body: note.body,
            selector: note.selector,
        });
    }
    let event_id = match Uuid::parse_str(&request.event_id) {
        Ok(event_id) => event_id,
        Err(_) => return plain(StatusCode::BAD_REQUEST, "review event id is invalid"),
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
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response<Body> {
    if let Err(response) = require_host(&state, &headers) {
        return response;
    }
    let Some(html) = state.sandbox.get(&id) else {
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
    if !manifest.service.inline.contains_key("present.prepaint") {
        return Err(PresentError::CorruptState(
            "renderer prepaint source is missing".to_string(),
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
        let actual: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
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

fn write_bootstrap(path: &std::path::Path, authority: &str, capability: &str) -> Result<()> {
    let endpoint = format!("http://{authority}/bootstrap");
    let html = format!(
        "<!doctype html><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\"><title>Opening presentation</title><form id=\"bootstrap\" method=\"post\" action=\"{endpoint}\"><input type=\"hidden\" name=\"capability\" value=\"{capability}\"><noscript><button type=\"submit\">Open presentation</button></noscript></form><script>document.getElementById('bootstrap').submit()</script>"
    );
    let parent = path
        .parent()
        .ok_or_else(|| PresentError::UnsafePath(path.to_path_buf()))?;
    create_private_dir_all(parent)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| PresentError::io(path, error))?;
    use std::io::Write as _;
    file.write_all(html.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| PresentError::io(path, error))
}

async fn monitor_session(state: AppState) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let now = now_unix();
        if !state.bootstrap_used.load(Ordering::Acquire)
            && now.saturating_sub(state.started_at) > limits::BOOTSTRAP_TTL_SECONDS
        {
            let _ = remove_if_regular(&state.bootstrap_path);
            state.shutdown.notify_waiters();
            return;
        }
        if now.saturating_sub(state.last_activity.load(Ordering::Acquire))
            > limits::SESSION_IDLE_SECONDS
        {
            let _ = state.store.close(state.session_id);
            state.shutdown.notify_waiters();
            return;
        }
        match state.store.load(state.session_id) {
            Ok(session) if session.status == SessionStatus::Closed => {
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

struct RuntimeCleanup {
    bootstrap_path: PathBuf,
    ready_path: PathBuf,
}

impl RuntimeCleanup {
    fn new(bootstrap_path: PathBuf, ready_path: PathBuf) -> Self {
        Self {
            bootstrap_path,
            ready_path,
        }
    }
}

impl Drop for RuntimeCleanup {
    fn drop(&mut self) {
        let _ = remove_if_regular(&self.bootstrap_path);
        let _ = remove_if_regular(&self.ready_path);
    }
}

fn remove_if_regular(path: &std::path::Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
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
    Ok(value.iter().map(|byte| format!("{byte:02x}")).collect())
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
        let bootstrap_path = temp.path().join("bootstrap.html");
        fs::write(&bootstrap_path, b"bootstrap").unwrap();
        let now = now_unix();
        let state = AppState {
            store,
            session_id: session.id,
            instance_id: Uuid::new_v4(),
            authority: "127.0.0.1:43210".to_string(),
            capability: Arc::new("capability".to_string()),
            cookie_name: Arc::new("cf_present_test".to_string()),
            cookie_value: Arc::new("cookie".to_string()),
            bootstrap_path: Arc::new(bootstrap_path),
            bootstrap_used: Arc::new(AtomicBool::new(false)),
            started_at: now,
            last_activity: Arc::new(AtomicU64::new(now)),
            shutdown: Arc::new(Notify::new()),
            assets: Arc::new(load_manifest().unwrap()),
            sandbox: Arc::new(HashMap::new()),
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
        assert_eq!(accepted.status(), StatusCode::SEE_OTHER);
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

    #[test]
    fn runtime_cleanup_removes_regular_capability_files() {
        let temp = tempfile::tempdir().unwrap();
        let bootstrap = temp.path().join("bootstrap.html");
        let ready = temp.path().join("ready.json");
        fs::write(&bootstrap, b"secret").unwrap();
        fs::write(&ready, b"identity").unwrap();
        drop(RuntimeCleanup::new(bootstrap.clone(), ready.clone()));
        assert!(!bootstrap.exists());
        assert!(!ready.exists());
    }
}
