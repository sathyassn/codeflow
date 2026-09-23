//! Transport-neutral state machine for interactive delegate turns.
//!
//! The host remains responsible for launching a harness and delivering text.
//! This module only creates durable, owner-only protocol records and waits for
//! their state transitions.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u8 = 2;
/// Maximum exact prompt size accepted by the lifecycle protocol.
pub const MAX_PROMPT_BYTES: usize = 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const RUN_LOCK_TIMEOUT: Duration = Duration::from_secs(1);

/// State requested by `delegate wait`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitUntil {
    /// The delegated harness reported a startup event.
    Ready,
    /// The armed prompt was accepted by the harness.
    Accepted,
    /// The accepted turn completed or failed.
    Terminal,
}

/// A successfully observed wait state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitResult {
    /// Canonical JSON record to print for the caller.
    pub json: String,
    /// Whether the terminal record represents failure.
    pub failed: bool,
}

/// Protocol failure category used by the CLI's stable exit-code contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    /// Invalid input, conflicting state, or a hook commit failure.
    Invalid,
    /// The run is poisoned, insecure, malformed, or mis-correlated.
    Unsafe,
    /// The requested state did not arrive before the deadline.
    Timeout,
    /// The waiter was interrupted by its caller.
    Interrupted,
}

/// A legible protocol error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelegateError {
    /// Stable error category.
    pub kind: ErrorKind,
    /// Reader-facing explanation.
    pub message: String,
}

impl std::fmt::Display for DelegateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DelegateError {}

impl DelegateError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Invalid,
            message: message.into(),
        }
    }

    fn unsafe_state(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Unsafe,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReadyRecord {
    schema_version: u8,
    run_id: String,
    event: String,
    source: String,
    session_id: String,
    cwd: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PoisonRecord {
    schema_version: u8,
    run_id: String,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RequestRecord {
    schema_version: u8,
    run_id: String,
    turn_id: String,
    prompt_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AcceptedRecord {
    schema_version: u8,
    run_id: String,
    turn_id: String,
    event: String,
    session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_id: Option<String>,
    prompt_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<String>,
    /// How the submitted prompt matched: `exact` or `paste_envelope`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delivery: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ResultRecord {
    schema_version: u8,
    run_id: String,
    turn_id: String,
    event: String,
    status: String,
    session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_assistant_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_details: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct HookPayload {
    hook_event_name: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    prompt_id: Option<String>,
    #[serde(default)]
    last_assistant_message: Option<String>,
    #[serde(default)]
    error: Option<serde_json::Value>,
    #[serde(default)]
    error_details: Option<serde_json::Value>,
}

/// Create a schema-v2 run directory and its task-scoped Claude settings.
///
/// Exact retries are idempotent. Existing conflicting or insecure content is
/// rejected without being replaced.
///
/// # Errors
///
/// Returns an error for invalid identifiers or paths, insecure ownership or
/// modes, conflicting records, and filesystem durability failures.
pub fn init(run_id: &str, state_dir: &Path) -> Result<PathBuf, DelegateError> {
    validate_id("run", run_id)?;
    validate_absolute(state_dir)?;
    validate_outside_git_worktree(state_dir)?;
    create_private_dir(state_dir)?;
    let turns = state_dir.join("turns");
    create_private_dir(&turns)?;
    validate_state_dir(state_dir)?;
    let _lock = acquire_run_lock(state_dir)?;

    let settings_path = state_dir.join("settings.json");
    let settings = task_settings(run_id, state_dir);
    install_json(&settings_path, &settings)?;
    Ok(settings_path)
}

/// Arm exactly one prompt for a run.
///
/// # Errors
///
/// Returns an error when the run is invalid or poisoned, another turn is
/// outstanding, the compatibility session has already been used, the prompt
/// is not canonical UTF-8/LF text, is too large, or the durable request cannot
/// be committed safely.
pub fn arm(
    run_id: &str,
    state_dir: &Path,
    turn_id: &str,
    prompt: &[u8],
) -> Result<(), DelegateError> {
    validate_id("run", run_id)?;
    validate_id("turn", turn_id)?;
    validate_state_dir(state_dir)?;
    validate_outside_git_worktree(state_dir)?;
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err(DelegateError::invalid(format!(
            "prompt exceeds the {MAX_PROMPT_BYTES}-byte limit"
        )));
    }
    validate_prompt_bytes(prompt)?;
    let _lock = acquire_run_lock(state_dir)?;
    validate_run_binding(run_id, state_dir)?;
    ensure_not_poisoned(run_id, state_dir)?;

    let turns_dir = state_dir.join("turns");
    validate_private_dir(&turns_dir)?;
    let states = inspect_turns(run_id, state_dir)?;
    if states
        .iter()
        .any(|state| state.turn_id == turn_id && state.result.is_some())
    {
        return Err(DelegateError::invalid(
            "delegate turn id is already terminal; use a new turn id",
        ));
    }
    if states
        .iter()
        .any(|state| state.request.is_some() && state.result.is_none() && state.turn_id != turn_id)
    {
        return Err(DelegateError::invalid(
            "another delegate turn is still outstanding",
        ));
    }
    if states.iter().any(|state| {
        state
            .accepted
            .as_ref()
            .is_some_and(|a| a.prompt_id.is_none())
    }) && !states.iter().any(|state| state.turn_id == turn_id)
    {
        return Err(DelegateError::invalid(
            "this pre-2.1.196 session has no prompt_id and is limited to one turn",
        ));
    }

    let current_turn_dir = turns_dir.join(turn_id);
    create_private_dir(&current_turn_dir)?;
    validate_private_dir(&current_turn_dir)?;
    let record = RequestRecord {
        schema_version: SCHEMA_VERSION,
        run_id: run_id.to_string(),
        turn_id: turn_id.to_string(),
        prompt_sha256: hex_sha256(prompt),
    };
    install_json(&current_turn_dir.join("request.json"), &record)
}

fn validate_prompt_bytes(prompt: &[u8]) -> Result<(), DelegateError> {
    const MESSAGE: &str = "delegate prompt must be non-empty canonical UTF-8 text with internal LF line endings, no terminal line break, and no other control characters; normalize it before arm";
    let text = std::str::from_utf8(prompt).map_err(|_| DelegateError::invalid(MESSAGE))?;
    if text.is_empty()
        || text.ends_with('\n')
        || text
            .chars()
            .any(|character| character.is_control() && character != '\n')
    {
        return Err(DelegateError::invalid(MESSAGE));
    }
    Ok(())
}

/// Process one Claude hook event in schema-v2 state mode.
///
/// # Errors
///
/// Returns an error when the payload or state is invalid, insecure,
/// conflicting, poisoned, or cannot be committed durably.
pub fn handle_hook(run_id: &str, state_dir: &Path, input: &str) -> Result<(), DelegateError> {
    validate_id("run", run_id)?;
    validate_state_dir(state_dir)?;
    validate_outside_git_worktree(state_dir)?;
    let payload: HookPayload = serde_json::from_str(input)
        .map_err(|error| DelegateError::invalid(format!("invalid Claude hook payload: {error}")))?;
    let _lock = acquire_run_lock(state_dir)?;
    validate_run_binding(run_id, state_dir)?;
    match payload.hook_event_name.as_str() {
        "SessionStart" => session_start(run_id, state_dir, payload),
        "UserPromptSubmit" => accept_prompt(run_id, state_dir, payload),
        "Stop" | "StopFailure" => record_terminal(run_id, state_dir, payload),
        other => Err(DelegateError::invalid(format!(
            "unsupported hook_event_name {other:?}"
        ))),
    }
}

/// Return whether a syntactically valid hook payload names `UserPromptSubmit`.
#[must_use]
pub fn is_prompt_submission(input: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(input)
        .ok()
        .and_then(|value| {
            value
                .get("hook_event_name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref()
        == Some("UserPromptSubmit")
}

/// Poll for a protocol state, checking poison before every possible success.
///
/// # Errors
///
/// Returns an error for invalid or insecure state, poison, timeout, or caller
/// interruption.
pub fn wait(
    run_id: &str,
    state_dir: &Path,
    turn_id: Option<&str>,
    until: WaitUntil,
    timeout: Duration,
    interrupted: impl Fn() -> bool,
) -> Result<WaitResult, DelegateError> {
    validate_id("run", run_id)?;
    if let Some(id) = turn_id {
        validate_id("turn", id)?;
    }
    validate_absolute(state_dir)?;
    validate_outside_git_worktree(state_dir)?;
    let required_turn = if until == WaitUntil::Ready {
        None
    } else {
        Some(turn_id.ok_or_else(|| {
            DelegateError::invalid("--turn-id is required when waiting for accepted or terminal")
        })?)
    };
    let started = Instant::now();
    loop {
        validate_state_dir(state_dir)?;
        validate_run_binding(run_id, state_dir)?;
        ensure_not_poisoned(run_id, state_dir)?;
        if interrupted() {
            let _lock = acquire_run_lock(state_dir)?;
            ensure_not_poisoned(run_id, state_dir)?;
            if has_accepted_without_result(run_id, state_dir)? {
                poison(
                    run_id,
                    state_dir,
                    "wait interrupted after prompt acceptance",
                    None,
                    None,
                    required_turn,
                )?;
            }
            return Err(DelegateError {
                kind: ErrorKind::Interrupted,
                message: "delegate wait interrupted".to_string(),
            });
        }

        if let Some(result) = observe_wait(run_id, state_dir, required_turn, until)? {
            ensure_not_poisoned(run_id, state_dir)?;
            return Ok(result);
        }
        if started.elapsed() >= timeout {
            return Err(DelegateError {
                kind: ErrorKind::Timeout,
                message: format!("delegate wait timed out after {}s", timeout.as_secs()),
            });
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn session_start(
    run_id: &str,
    state_dir: &Path,
    payload: HookPayload,
) -> Result<(), DelegateError> {
    let source = required_nonempty(payload.source, "SessionStart source")?;
    ensure_not_poisoned(run_id, state_dir)?;
    if source != "startup" {
        poison(
            run_id,
            state_dir,
            "delegate session restarted; a new run is required",
            Some("SessionStart"),
            Some(&source),
            None,
        )?;
        return Err(DelegateError::unsafe_state(format!(
            "SessionStart source {source:?} poisoned the delegate run"
        )));
    }
    let record = ReadyRecord {
        schema_version: SCHEMA_VERSION,
        run_id: run_id.to_string(),
        event: "SessionStart".to_string(),
        source,
        session_id: required_nonempty(payload.session_id, "SessionStart session_id")?,
        cwd: required_nonempty(payload.cwd, "SessionStart cwd")?,
    };
    let path = state_dir.join("ready.json");
    match install_json(&path, &record) {
        Ok(()) => Ok(()),
        Err(error) if error.kind == ErrorKind::Invalid => {
            poison(
                run_id,
                state_dir,
                "conflicting startup session observed; a new run is required",
                Some("SessionStart"),
                Some("startup"),
                None,
            )?;
            Err(DelegateError::unsafe_state(format!(
                "conflicting startup session poisoned the delegate run: {error}"
            )))
        }
        Err(error) => Err(error),
    }
}

fn accept_prompt(
    run_id: &str,
    state_dir: &Path,
    payload: HookPayload,
) -> Result<(), DelegateError> {
    ensure_not_poisoned(run_id, state_dir)?;
    let prompt = payload
        .prompt
        .ok_or_else(|| DelegateError::invalid("UserPromptSubmit payload omitted prompt"))?;
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err(DelegateError::invalid(
            "submitted prompt exceeds the size limit",
        ));
    }
    validate_prompt_id(payload.prompt_id.as_deref())?;
    let session_id = required_nonempty(payload.session_id, "UserPromptSubmit session_id")?;
    let ready: ReadyRecord = read_json(&state_dir.join("ready.json"))?;
    validate_ready_record(&ready, run_id)?;
    if ready.session_id != session_id {
        return Err(DelegateError::invalid(
            "UserPromptSubmit session_id does not match ready session",
        ));
    }

    let states = inspect_turns(run_id, state_dir)?;
    let outstanding: Vec<_> = states
        .iter()
        .filter(|state| state.request.is_some() && state.result.is_none())
        .collect();
    let [state] = outstanding.as_slice() else {
        return Err(DelegateError::invalid(
            "UserPromptSubmit requires exactly one armed outstanding turn",
        ));
    };
    let request = state
        .request
        .as_ref()
        .ok_or_else(|| DelegateError::unsafe_state("armed request disappeared"))?;
    let delivery = submitted_delivery(&prompt, &request.prompt_sha256).ok_or_else(|| {
        DelegateError::invalid("submitted prompt does not match the armed prompt digest")
    })?;
    let compatibility = payload
        .prompt_id
        .is_none()
        .then(|| "pre-2.1.196-single-turn".to_string());
    let accepted = AcceptedRecord {
        schema_version: SCHEMA_VERSION,
        run_id: run_id.to_string(),
        turn_id: state.turn_id.clone(),
        event: "UserPromptSubmit".to_string(),
        session_id,
        prompt_id: payload.prompt_id,
        prompt_sha256: request.prompt_sha256.clone(),
        compatibility,
        delivery: Some(delivery.to_string()),
    };
    install_json(
        &turn_path(state_dir, &state.turn_id, "accepted.json"),
        &accepted,
    )
}

/// Classify how a submitted prompt matches the armed digest.
///
/// The prompt matches `exact` when its bytes are the armed bytes. Claude Code
/// wraps a long or multi-line paste as one envelope, so it also matches
/// `paste_envelope` when removing exactly one outer envelope leaves the armed
/// bytes. The accepted grammar is `<pasted_content id="N">` LF, the inner
/// bytes, LF, `</pasted_content id="N">`, then at most one LF, where both N are
/// the same non-empty ASCII digit string. Anything else does not match.
fn submitted_delivery(prompt: &str, armed_sha256: &str) -> Option<&'static str> {
    if hex_sha256(prompt.as_bytes()) == armed_sha256 {
        return Some("exact");
    }
    let inner = strip_paste_envelope(prompt)?;
    (hex_sha256(inner.as_bytes()) == armed_sha256).then_some("paste_envelope")
}

fn strip_paste_envelope(prompt: &str) -> Option<&str> {
    let rest = prompt.strip_prefix("<pasted_content id=\"")?;
    let id_len = rest.find(|character: char| !character.is_ascii_digit())?;
    if id_len == 0 {
        return None;
    }
    let (id, rest) = rest.split_at(id_len);
    let body = rest.strip_prefix("\">\n")?;
    let body = body.strip_suffix('\n').unwrap_or(body);
    body.strip_suffix(&format!("\n</pasted_content id=\"{id}\">"))
}

fn record_terminal(
    run_id: &str,
    state_dir: &Path,
    payload: HookPayload,
) -> Result<(), DelegateError> {
    ensure_not_poisoned(run_id, state_dir)?;
    let evidence = TerminalEvidence::from_payload(payload)?;
    let turn_states = inspect_turns(run_id, state_dir)?;
    let unresolved: Vec<_> = turn_states
        .iter()
        .filter(|state| state.accepted.is_some() && state.result.is_none())
        .collect();
    if exact_terminal_retry(run_id, state_dir, &turn_states, &unresolved, &evidence)? {
        return Ok(());
    }
    let [turn] = unresolved.as_slice() else {
        poison(
            run_id,
            state_dir,
            "terminal event has no unique accepted turn",
            Some(&evidence.event),
            None,
            None,
        )?;
        return Err(DelegateError::unsafe_state(
            "terminal event could not be correlated; delegate run poisoned",
        ));
    };
    validate_terminal_binding(run_id, state_dir, turn, &evidence)?;
    let result = evidence.result_for(run_id, &turn.turn_id);
    install_json(&turn_path(state_dir, &turn.turn_id, "result.json"), &result)
}

#[derive(Debug)]
struct TerminalEvidence {
    event: String,
    status: &'static str,
    session_id: String,
    prompt_id: Option<String>,
    message: Option<String>,
    error: Option<serde_json::Value>,
    error_details: Option<serde_json::Value>,
}

impl TerminalEvidence {
    fn from_payload(payload: HookPayload) -> Result<Self, DelegateError> {
        validate_prompt_id(payload.prompt_id.as_deref())?;
        let session_id = required_nonempty(payload.session_id, "terminal session_id")?;
        if payload.hook_event_name == "Stop" {
            let message = payload.last_assistant_message.ok_or_else(|| {
                DelegateError::invalid("Stop payload omitted last_assistant_message")
            })?;
            if message.len() > MAX_MESSAGE_BYTES {
                return Err(DelegateError::invalid(
                    "last_assistant_message exceeds the size limit",
                ));
            }
            return Ok(Self {
                event: payload.hook_event_name,
                status: "completed",
                session_id,
                prompt_id: payload.prompt_id,
                message: Some(message),
                error: None,
                error_details: None,
            });
        }
        ensure_json_bound("error", payload.error.as_ref(), MAX_ERROR_BYTES)?;
        ensure_json_bound(
            "error_details",
            payload.error_details.as_ref(),
            MAX_ERROR_BYTES,
        )?;
        Ok(Self {
            event: payload.hook_event_name,
            status: "failed",
            session_id,
            prompt_id: payload.prompt_id,
            message: None,
            error: payload.error,
            error_details: payload.error_details,
        })
    }

    fn result_for(&self, run_id: &str, turn_id: &str) -> ResultRecord {
        ResultRecord {
            schema_version: SCHEMA_VERSION,
            run_id: run_id.to_string(),
            turn_id: turn_id.to_string(),
            event: self.event.clone(),
            status: self.status.to_string(),
            session_id: self.session_id.clone(),
            prompt_id: self.prompt_id.clone(),
            last_assistant_message: self.message.clone(),
            error: self.error.clone(),
            error_details: self.error_details.clone(),
        }
    }
}

fn exact_terminal_retry(
    run_id: &str,
    state_dir: &Path,
    states: &[TurnState],
    unresolved: &[&TurnState],
    evidence: &TerminalEvidence,
) -> Result<bool, DelegateError> {
    for state in states {
        let Some(existing) = &state.result else {
            continue;
        };
        let candidate = evidence.result_for(run_id, &state.turn_id);
        if existing != &candidate {
            continue;
        }
        if let Some(other) = unresolved.first() {
            let accepted_prompt = other
                .accepted
                .as_ref()
                .and_then(|accepted| accepted.prompt_id.as_ref());
            let proven_old_retry = candidate.prompt_id.is_some()
                && candidate.prompt_id.as_ref() == existing.prompt_id.as_ref()
                && candidate.prompt_id.as_ref() != accepted_prompt;
            if !proven_old_retry {
                poison(
                    run_id,
                    state_dir,
                    "ambiguous prior result retry while another turn is accepted",
                    Some(&evidence.event),
                    None,
                    Some(&other.turn_id),
                )?;
                return Err(DelegateError::unsafe_state(
                    "ambiguous terminal retry poisoned the delegate run",
                ));
            }
        }
        return Ok(true);
    }
    Ok(false)
}

fn validate_terminal_binding(
    run_id: &str,
    state_dir: &Path,
    state: &TurnState,
    evidence: &TerminalEvidence,
) -> Result<(), DelegateError> {
    let accepted = state
        .accepted
        .as_ref()
        .ok_or_else(|| DelegateError::unsafe_state("accepted record disappeared"))?;
    let mismatch = if accepted.session_id != evidence.session_id {
        Some((
            "terminal session_id does not match accepted turn",
            "terminal session mismatch poisoned the delegate run",
        ))
    } else if accepted.prompt_id != evidence.prompt_id {
        Some((
            "terminal prompt_id does not match accepted turn",
            "terminal prompt mismatch poisoned the delegate run",
        ))
    } else {
        None
    };
    if let Some((reason, message)) = mismatch {
        poison(
            run_id,
            state_dir,
            reason,
            Some(&evidence.event),
            None,
            Some(&state.turn_id),
        )?;
        return Err(DelegateError::unsafe_state(message));
    }
    Ok(())
}

#[derive(Debug)]
struct TurnState {
    turn_id: String,
    request: Option<RequestRecord>,
    accepted: Option<AcceptedRecord>,
    result: Option<ResultRecord>,
}

fn inspect_turns(run_id: &str, state_dir: &Path) -> Result<Vec<TurnState>, DelegateError> {
    let turns_dir = state_dir.join("turns");
    validate_private_dir(&turns_dir)?;
    let mut entries = std::fs::read_dir(&turns_dir)
        .map_err(|error| DelegateError::unsafe_state(format!("cannot read turns: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| DelegateError::unsafe_state(format!("cannot read turns: {error}")))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let mut states = Vec::with_capacity(entries.len());
    for entry in entries {
        let turn_id = entry
            .file_name()
            .into_string()
            .map_err(|_| DelegateError::unsafe_state("turn directory name is not UTF-8"))?;
        validate_id("turn", &turn_id)
            .map_err(|error| DelegateError::unsafe_state(error.message))?;
        let path = entry.path();
        validate_private_dir(&path)?;
        let request = read_optional_json::<RequestRecord>(&path.join("request.json"))?;
        let accepted = read_optional_json::<AcceptedRecord>(&path.join("accepted.json"))?;
        let result = read_optional_json::<ResultRecord>(&path.join("result.json"))?;
        if let Some(record) = &request {
            validate_header(record.schema_version, &record.run_id, run_id, "request")?;
            if record.turn_id != turn_id || !valid_digest(&record.prompt_sha256) {
                return Err(DelegateError::unsafe_state(
                    "request record is malformed or mis-correlated",
                ));
            }
        }
        if let Some(record) = &accepted {
            validate_header(record.schema_version, &record.run_id, run_id, "accepted")?;
            let Some(request) = &request else {
                return Err(DelegateError::unsafe_state(
                    "accepted record lacks a matching request",
                ));
            };
            if record.turn_id != turn_id
                || record.event != "UserPromptSubmit"
                || record.prompt_sha256 != request.prompt_sha256
                || record.session_id.is_empty()
                || record.session_id.len() > 4096
                || (record.prompt_id.is_none()
                    && record.compatibility.as_deref() != Some("pre-2.1.196-single-turn"))
                || (record.prompt_id.is_some() && record.compatibility.is_some())
                || !matches!(
                    record.delivery.as_deref(),
                    None | Some("exact" | "paste_envelope")
                )
            {
                return Err(DelegateError::unsafe_state(
                    "accepted record is mis-correlated with its request",
                ));
            }
            validate_prompt_id(record.prompt_id.as_deref())
                .map_err(|error| DelegateError::unsafe_state(error.message))?;
        }
        if let Some(record) = &result {
            validate_header(record.schema_version, &record.run_id, run_id, "result")?;
            let Some(accepted) = &accepted else {
                return Err(DelegateError::unsafe_state(
                    "result record lacks a matching acceptance",
                ));
            };
            let valid_terminal = (record.event == "Stop" && record.status == "completed")
                || (record.event == "StopFailure" && record.status == "failed");
            let valid_content = if record.event == "Stop" {
                record.last_assistant_message.is_some()
                    && record.error.is_none()
                    && record.error_details.is_none()
            } else {
                record.last_assistant_message.is_none()
            };
            if record.turn_id != turn_id
                || !valid_terminal
                || !valid_content
                || record.session_id != accepted.session_id
                || record.prompt_id != accepted.prompt_id
            {
                return Err(DelegateError::unsafe_state(
                    "result record is mis-correlated with its acceptance",
                ));
            }
        }
        states.push(TurnState {
            turn_id,
            request,
            accepted,
            result,
        });
    }
    if states
        .iter()
        .filter(|state| state.request.is_some() && state.result.is_none())
        .count()
        > 1
    {
        return Err(DelegateError::unsafe_state(
            "delegate run contains multiple outstanding turns",
        ));
    }
    Ok(states)
}

fn observe_wait(
    run_id: &str,
    state_dir: &Path,
    turn_id: Option<&str>,
    until: WaitUntil,
) -> Result<Option<WaitResult>, DelegateError> {
    match until {
        WaitUntil::Ready => {
            let path = state_dir.join("ready.json");
            if !path_exists_safely(&path)? {
                return Ok(None);
            }
            let record: ReadyRecord = read_json(&path)?;
            validate_ready_record(&record, run_id)?;
            serialize_wait(&record, false).map(Some)
        }
        WaitUntil::Accepted | WaitUntil::Terminal => {
            let turn_id = turn_id.ok_or_else(|| DelegateError::invalid("turn id unavailable"))?;
            let states = inspect_turns(run_id, state_dir)?;
            let Some(state) = states.iter().find(|state| state.turn_id == turn_id) else {
                return Ok(None);
            };
            if until == WaitUntil::Accepted {
                return state
                    .accepted
                    .as_ref()
                    .map(|record| serialize_wait(record, false))
                    .transpose();
            }
            state
                .result
                .as_ref()
                .map(|record| serialize_wait(record, record.status == "failed"))
                .transpose()
        }
    }
}

fn serialize_wait(record: &impl Serialize, failed: bool) -> Result<WaitResult, DelegateError> {
    let json = serde_json::to_string_pretty(record)
        .map_err(|error| DelegateError::unsafe_state(error.to_string()))?;
    Ok(WaitResult {
        json: format!("{json}\n"),
        failed,
    })
}

fn has_accepted_without_result(run_id: &str, state_dir: &Path) -> Result<bool, DelegateError> {
    Ok(inspect_turns(run_id, state_dir)?
        .iter()
        .any(|state| state.accepted.is_some() && state.result.is_none()))
}

fn ensure_not_poisoned(run_id: &str, state_dir: &Path) -> Result<(), DelegateError> {
    let path = state_dir.join("poison.json");
    if path_exists_safely(&path)? {
        let record: PoisonRecord = read_json(&path)?;
        validate_header(record.schema_version, &record.run_id, run_id, "poison")?;
        return Err(DelegateError::unsafe_state(format!(
            "delegate run is poisoned: {}",
            record.reason
        )));
    }
    Ok(())
}

fn poison(
    run_id: &str,
    state_dir: &Path,
    reason: &str,
    event: Option<&str>,
    source: Option<&str>,
    turn_id: Option<&str>,
) -> Result<(), DelegateError> {
    let record = PoisonRecord {
        schema_version: SCHEMA_VERSION,
        run_id: run_id.to_string(),
        reason: reason.to_string(),
        event: event.map(str::to_string),
        source: source.map(str::to_string),
        turn_id: turn_id.map(str::to_string),
    };
    install_json(&state_dir.join("poison.json"), &record)
}

fn acquire_run_lock(state_dir: &Path) -> Result<File, DelegateError> {
    let path = state_dir.join(".protocol.lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path).map_err(|error| {
        DelegateError::unsafe_state(format!("cannot open delegate run lock: {error}"))
    })?;
    validate_open_file(&file, &path)?;

    let started = Instant::now();
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => break,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    && started.elapsed() < RUN_LOCK_TIMEOUT =>
            {
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(DelegateError::unsafe_state(
                    "delegate run lock remained busy past the bounded deadline",
                ));
            }
            Err(error) => {
                return Err(DelegateError::unsafe_state(format!(
                    "cannot acquire delegate run lock: {error}"
                )));
            }
        }
    }
    validate_open_file(&file, &path)?;
    validate_state_dir(state_dir)?;
    Ok(file)
}

fn validate_open_file(file: &File, path: &Path) -> Result<(), DelegateError> {
    let opened = file.metadata().map_err(|error| {
        DelegateError::unsafe_state(format!("cannot inspect open file: {error}"))
    })?;
    validate_owner_mode(&opened, 0o600, path)?;
    if !opened.is_file() {
        return Err(DelegateError::unsafe_state(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let current = std::fs::symlink_metadata(path).map_err(|error| {
        DelegateError::unsafe_state(format!("cannot revalidate {}: {error}", path.display()))
    })?;
    if current.file_type().is_symlink() || !same_file(&opened, &current) {
        return Err(DelegateError::unsafe_state(format!(
            "{} changed while it was opened",
            path.display()
        )));
    }
    Ok(())
}

fn install_json<T: Serialize + DeserializeOwned + PartialEq>(
    path: &Path,
    record: &T,
) -> Result<(), DelegateError> {
    let mut bytes = serde_json::to_vec_pretty(record)
        .map_err(|error| DelegateError::invalid(format!("cannot encode record: {error}")))?;
    bytes.push(b'\n');
    let parent = path
        .parent()
        .ok_or_else(|| DelegateError::invalid("record path has no parent"))?;
    validate_private_dir(parent)?;
    let temporary = parent.join(format!(".codeflow-{}.tmp", ulid::Ulid::new()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|error| {
        DelegateError::unsafe_state(format!("cannot create temporary record: {error}"))
    })?;
    let persist = file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| DelegateError::unsafe_state(format!("cannot persist record: {error}")));
    drop(file);
    if let Err(error) = persist {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    match std::fs::hard_link(&temporary, path) {
        Ok(()) => {
            std::fs::remove_file(&temporary).map_err(|error| {
                DelegateError::unsafe_state(format!("cannot remove temporary record: {error}"))
            })?;
            sync_dir(parent)?;
            validate_private_file(path)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(&temporary);
            let existing: T = read_json(path)?;
            if existing == *record {
                Ok(())
            } else {
                Err(DelegateError::invalid(format!(
                    "conflicting record already exists at {}",
                    path.display()
                )))
            }
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            Err(DelegateError::unsafe_state(format!(
                "cannot atomically install record: {error}"
            )))
        }
    }
}

fn read_optional_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, DelegateError> {
    if path_exists_safely(path)? {
        read_json(path).map(Some)
    } else {
        Ok(None)
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, DelegateError> {
    let before = private_file_metadata(path)?;
    let mut file = File::open(path)
        .map_err(|error| DelegateError::unsafe_state(format!("cannot open record: {error}")))?;
    let opened = file
        .metadata()
        .map_err(|error| DelegateError::unsafe_state(format!("cannot inspect record: {error}")))?;
    validate_owner_mode(&opened, 0o600, path)?;
    if !same_file(&before, &opened) {
        return Err(DelegateError::unsafe_state(format!(
            "{} changed while it was opened",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| DelegateError::unsafe_state(format!("cannot read record: {error}")))?;
    let after = std::fs::symlink_metadata(path).map_err(|error| {
        DelegateError::unsafe_state(format!("cannot revalidate {}: {error}", path.display()))
    })?;
    if after.file_type().is_symlink() || !same_file(&opened, &after) {
        return Err(DelegateError::unsafe_state(format!(
            "{} changed while it was read",
            path.display()
        )));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        DelegateError::unsafe_state(format!("malformed record {}: {error}", path.display()))
    })
}

fn path_exists_safely(path: &Path) -> Result<bool, DelegateError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            validate_private_file(path)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(DelegateError::unsafe_state(format!(
            "cannot inspect {}: {error}",
            path.display()
        ))),
    }
}

fn create_private_dir(path: &Path) -> Result<(), DelegateError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(DelegateError::invalid(format!(
                    "cannot create private directory {}: {error}",
                    path.display()
                )));
            }
        }
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(path).map_err(|error| {
            DelegateError::invalid(format!(
                "cannot create private directory {}: {error}",
                path.display()
            ))
        })?;
    }
    validate_private_dir(path)
}

fn validate_state_dir(path: &Path) -> Result<(), DelegateError> {
    validate_absolute(path)?;
    validate_private_dir(path)?;
    validate_private_dir(&path.join("turns"))
}

fn validate_absolute(path: &Path) -> Result<(), DelegateError> {
    if !path.is_absolute() {
        return Err(DelegateError::invalid(
            "delegate state directory must be absolute",
        ));
    }
    if path.to_str().is_none() {
        return Err(DelegateError::invalid(
            "delegate state directory must be valid UTF-8",
        ));
    }
    #[cfg(not(unix))]
    return Err(DelegateError::invalid(
        "native Windows is unsupported for delegate state; use WSL2",
    ));
    #[cfg(unix)]
    Ok(())
}

fn validate_outside_git_worktree(path: &Path) -> Result<(), DelegateError> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor.join(".git")) {
            Ok(_) => {
                return Err(DelegateError::invalid(
                    "delegate state directory must be outside a Git worktree",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DelegateError::unsafe_state(format!(
                    "cannot verify delegate state is outside a Git worktree: {error}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_private_dir(path: &Path) -> Result<(), DelegateError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        DelegateError::unsafe_state(format!(
            "cannot inspect directory {}: {error}",
            path.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DelegateError::unsafe_state(format!(
            "{} is not a regular directory",
            path.display()
        )));
    }
    validate_owner_mode(&metadata, 0o700, path)
}

fn validate_private_file(path: &Path) -> Result<(), DelegateError> {
    let _ = private_file_metadata(path)?;
    Ok(())
}

fn private_file_metadata(path: &Path) -> Result<std::fs::Metadata, DelegateError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        DelegateError::unsafe_state(format!("cannot inspect file {}: {error}", path.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(DelegateError::unsafe_state(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    validate_owner_mode(&metadata, 0o600, path)?;
    Ok(metadata)
}

#[cfg(unix)]
fn validate_owner_mode(
    metadata: &std::fs::Metadata,
    expected: u32,
    path: &Path,
) -> Result<(), DelegateError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if metadata.permissions().mode() & 0o777 != expected {
        return Err(DelegateError::unsafe_state(format!(
            "{} must have mode {expected:04o}",
            path.display(),
        )));
    }
    if metadata.uid() != effective_uid() {
        return Err(DelegateError::unsafe_state(format!(
            "{} is not owned by the current user",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino() && left.uid() == right.uid()
}

#[cfg(not(unix))]
fn same_file(_left: &std::fs::Metadata, _right: &std::fs::Metadata) -> bool {
    false
}

#[cfg(not(unix))]
fn validate_owner_mode(
    _metadata: &std::fs::Metadata,
    _expected: u32,
    _path: &Path,
) -> Result<(), DelegateError> {
    Err(DelegateError::unsafe_state(
        "native Windows is unsupported for delegate state; use WSL2",
    ))
}

#[cfg(unix)]
fn effective_uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: geteuid takes no arguments and has no failure mode on supported
    // Unix targets.
    unsafe { geteuid() }
}

fn sync_dir(path: &Path) -> Result<(), DelegateError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            DelegateError::unsafe_state(format!("cannot sync {}: {error}", path.display()))
        })
}

fn validate_header(
    schema_version: u8,
    record_run_id: &str,
    expected_run_id: &str,
    kind: &str,
) -> Result<(), DelegateError> {
    if schema_version != SCHEMA_VERSION || record_run_id != expected_run_id {
        return Err(DelegateError::unsafe_state(format!(
            "{kind} record has the wrong schema or run_id"
        )));
    }
    Ok(())
}

fn validate_ready_record(record: &ReadyRecord, run_id: &str) -> Result<(), DelegateError> {
    validate_header(record.schema_version, &record.run_id, run_id, "ready")?;
    if record.event != "SessionStart"
        || record.source != "startup"
        || record.session_id.is_empty()
        || record.session_id.len() > 4096
        || record.cwd.is_empty()
        || record.cwd.len() > 4096
    {
        return Err(DelegateError::unsafe_state(
            "ready record is malformed or mis-correlated",
        ));
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn validate_id(kind: &str, value: &str) -> Result<(), DelegateError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(DelegateError::invalid(format!(
            "{kind} id must be 1-64 ASCII letters, digits, '.', '_' or '-' and not '.' or '..'"
        )))
    }
}

fn validate_prompt_id(prompt_id: Option<&str>) -> Result<(), DelegateError> {
    let Some(value) = prompt_id else {
        return Ok(());
    };
    let bytes = value.as_bytes();
    let valid = bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit());
    if valid {
        Ok(())
    } else {
        Err(DelegateError::invalid(
            "prompt_id must be a canonical 36-character UUID",
        ))
    }
}

fn required_nonempty(value: Option<String>, field: &str) -> Result<String, DelegateError> {
    match value {
        Some(value) if !value.is_empty() && value.len() <= 4096 => Ok(value),
        _ => Err(DelegateError::invalid(format!(
            "{field} is required and must be bounded"
        ))),
    }
}

fn ensure_json_bound(
    name: &str,
    value: Option<&serde_json::Value>,
    max: usize,
) -> Result<(), DelegateError> {
    if value.is_some_and(|value| serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() > max)) {
        return Err(DelegateError::invalid(format!(
            "{name} exceeds the {max}-byte limit"
        )));
    }
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn turn_path(state_dir: &Path, turn_id: &str, file: &str) -> PathBuf {
    state_dir.join("turns").join(turn_id).join(file)
}

fn task_settings(run_id: &str, state_dir: &Path) -> serde_json::Value {
    let command = hook_command(run_id, state_dir);
    serde_json::json!({
        "hooks": {
            "SessionStart": [{"hooks": [{"type": "command", "command": command}]}],
            "UserPromptSubmit": [{"hooks": [{"type": "command", "command": command}]}],
            "Stop": [{"hooks": [{"type": "command", "command": command}]}],
            "StopFailure": [{"hooks": [{"type": "command", "command": command}]}]
        }
    })
}

fn validate_run_binding(run_id: &str, state_dir: &Path) -> Result<(), DelegateError> {
    let settings: serde_json::Value = read_json(&state_dir.join("settings.json"))?;
    if settings != task_settings(run_id, state_dir) {
        return Err(DelegateError::unsafe_state(
            "delegate settings do not match this run_id and state directory",
        ));
    }
    Ok(())
}

fn hook_command(run_id: &str, state_dir: &Path) -> String {
    let state_dir = state_dir
        .to_str()
        .expect("delegate state paths are validated as UTF-8");
    format!(
        "codeflow hook delegate-turn --run-id {} --state-dir {}",
        shell_quote(run_id),
        shell_quote(state_dir)
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const PROMPT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

    fn state() -> (TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state");
        init("run-1", &path).unwrap();
        (temp, path)
    }

    fn ready(path: &Path) {
        handle_hook(
            "run-1",
            path,
            r#"{"hook_event_name":"SessionStart","source":"startup","session_id":"s1","cwd":"/tmp"}"#,
        )
        .unwrap();
    }

    fn accept(path: &Path, prompt_id: Option<&str>) {
        let mut value = serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "s1",
            "prompt": "hello"
        });
        if let Some(id) = prompt_id {
            value["prompt_id"] = serde_json::Value::String(id.to_string());
        }
        handle_hook("run-1", path, &value.to_string()).unwrap();
    }

    fn stop(path: &Path, prompt_id: &str, message: &str) -> Result<(), DelegateError> {
        handle_hook(
            "run-1",
            path,
            &serde_json::json!({
                "hook_event_name": "Stop",
                "session_id": "s1",
                "prompt_id": prompt_id,
                "last_assistant_message": message
            })
            .to_string(),
        )
    }

    fn rewrite_json(path: &Path, mutate: impl FnOnce(&mut serde_json::Value)) {
        let mut value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        mutate(&mut value);
        std::fs::remove_file(path).unwrap();
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path).unwrap();
        serde_json::to_writer_pretty(&mut file, &value).unwrap();
        file.write_all(b"\n").unwrap();
        file.sync_all().unwrap();
    }

    #[test]
    fn complete_lifecycle_and_exact_retries() {
        let (_temp, path) = state();
        ready(&path);
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, Some(PROMPT_ID));
        accept(&path, Some(PROMPT_ID));
        let stop = format!(
            r#"{{"hook_event_name":"Stop","session_id":"s1","prompt_id":"{PROMPT_ID}","last_assistant_message":"done"}}"#
        );
        handle_hook("run-1", &path, &stop).unwrap();
        handle_hook("run-1", &path, &stop).unwrap();
        let result = wait(
            "run-1",
            &path,
            Some("turn-1"),
            WaitUntil::Terminal,
            Duration::ZERO,
            || false,
        )
        .unwrap();
        assert!(!result.failed);
        assert!(result.json.contains("\"done\""));
    }

    #[test]
    fn later_different_stop_poisons_without_replacing_first_result() {
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, Some(PROMPT_ID));
        stop(&path, PROMPT_ID, "waiting for the peer").unwrap();
        let result_path = path.join("turns/turn-1/result.json");
        let original = std::fs::read(&result_path).unwrap();

        let error = stop(&path, PROMPT_ID, "peer returned; final answer").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsafe);
        assert!(path.join("poison.json").exists());
        assert_eq!(std::fs::read(&result_path).unwrap(), original);
        assert_eq!(
            wait(
                "run-1",
                &path,
                Some("turn-1"),
                WaitUntil::Terminal,
                Duration::ZERO,
                || false,
            )
            .unwrap_err()
            .kind,
            ErrorKind::Unsafe
        );
    }

    #[test]
    fn later_stop_cannot_bind_to_armed_but_unaccepted_next_turn() {
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, Some(PROMPT_ID));
        stop(&path, PROMPT_ID, "waiting for the peer").unwrap();
        let result_path = path.join("turns/turn-1/result.json");
        let original = std::fs::read(&result_path).unwrap();
        arm("run-1", &path, "turn-2", b"again").unwrap();

        let error = stop(&path, PROMPT_ID, "late peer continuation").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsafe);
        assert_eq!(std::fs::read(&result_path).unwrap(), original);
        assert!(!path.join("turns/turn-2/result.json").exists());
        assert!(path.join("poison.json").exists());
    }

    #[test]
    fn terminal_turn_ids_cannot_be_rearmed() {
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, Some(PROMPT_ID));
        stop(&path, PROMPT_ID, "done").unwrap();
        let error = arm("run-1", &path, "turn-1", b"hello").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Invalid);
        assert!(error.message.contains("already terminal"));
    }

    #[test]
    fn non_startup_session_poison_precedes_ready() {
        for source in ["resume", "clear", "compact", "fork"] {
            let (_temp, path) = state();
            ready(&path);
            let payload = serde_json::json!({
                "hook_event_name": "SessionStart",
                "source": source,
                "session_id": "s2",
                "cwd": "/tmp"
            });
            let error = handle_hook("run-1", &path, &payload.to_string()).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Unsafe);
            let poison: PoisonRecord = read_json(&path.join("poison.json")).unwrap();
            assert_eq!(poison.source.as_deref(), Some(source));
            let error = wait(
                "run-1",
                &path,
                None,
                WaitUntil::Ready,
                Duration::ZERO,
                || false,
            )
            .unwrap_err();
            assert_eq!(error.kind, ErrorKind::Unsafe);
            assert_eq!(
                arm("run-1", &path, "turn-1", b"hello").unwrap_err().kind,
                ErrorKind::Unsafe
            );
        }
    }

    #[test]
    fn conflicting_startup_session_poisons_the_run() {
        let (_temp, path) = state();
        ready(&path);
        let error = handle_hook(
            "run-1",
            &path,
            r#"{"hook_event_name":"SessionStart","source":"startup","session_id":"s2","cwd":"/tmp"}"#,
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsafe);
        assert!(path.join("poison.json").exists());
        assert_eq!(
            wait(
                "run-1",
                &path,
                None,
                WaitUntil::Ready,
                Duration::ZERO,
                || false,
            )
            .unwrap_err()
            .kind,
            ErrorKind::Unsafe
        );
    }

    #[test]
    fn rejects_unarmed_missing_and_mismatched_prompts() {
        let (_temp, path) = state();
        ready(&path);
        let missing = r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1"}"#;
        assert_eq!(
            handle_hook("run-1", &path, missing).unwrap_err().kind,
            ErrorKind::Invalid
        );
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        let mismatch =
            r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt":"other"}"#;
        assert_eq!(
            handle_hook("run-1", &path, mismatch).unwrap_err().kind,
            ErrorKind::Invalid
        );
    }

    const ARMED: &str = "Run the pipeline.\n\nThen stop.";

    fn submit(path: &Path, prompt: &str) -> Result<(), DelegateError> {
        handle_hook(
            "run-1",
            path,
            &serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": "s1",
                "prompt_id": PROMPT_ID,
                "prompt": prompt
            })
            .to_string(),
        )
    }

    fn accepted_delivery(path: &Path) -> String {
        let text = std::fs::read_to_string(path.join("turns/turn-1/accepted.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["prompt_sha256"], hex_sha256(ARMED.as_bytes()));
        value["delivery"].as_str().unwrap().to_string()
    }

    #[test]
    fn accepts_exact_prompt_or_one_matching_paste_envelope() {
        for (submitted, delivery) in [
            (ARMED.to_string(), "exact"),
            (
                format!("<pasted_content id=\"7914\">\n{ARMED}\n</pasted_content id=\"7914\">"),
                "paste_envelope",
            ),
            (
                format!("<pasted_content id=\"7914\">\n{ARMED}\n</pasted_content id=\"7914\">\n"),
                "paste_envelope",
            ),
        ] {
            let (_temp, path) = state();
            ready(&path);
            arm("run-1", &path, "turn-1", ARMED.as_bytes()).unwrap();
            submit(&path, &submitted).unwrap();
            assert_eq!(accepted_delivery(&path), delivery, "{submitted:?}");
            // The exact retry stays idempotent for either delivery form.
            submit(&path, &submitted).unwrap();
        }
    }

    #[test]
    fn rejects_every_other_paste_envelope_shape() {
        let open = "<pasted_content id=\"7914\">";
        let close = "</pasted_content id=\"7914\">";
        let mut differs = ARMED.to_string();
        differs.replace_range(0..1, "r");
        for submitted in [
            format!("<pasted_content id=\"7914\">\n{ARMED}\n</pasted_content id=\"7915\">"),
            format!("<pasted_content id=\"\">\n{ARMED}\n</pasted_content id=\"\">"),
            format!("<pasted_content id=\"a1\">\n{ARMED}\n</pasted_content id=\"a1\">"),
            format!("x{open}\n{ARMED}\n{close}"),
            format!("\n{open}\n{ARMED}\n{close}"),
            format!("{open}\n{ARMED}\n{close}x"),
            format!("{open}\n{ARMED}\n{close}\n\n"),
            format!("{open}{ARMED}{close}"),
            format!("{open}\n{ARMED}\n\n{close}"),
            format!("{open}\n{ARMED}\n{close}\n{open}\n{ARMED}\n{close}"),
            format!(
                "{open}\n{ARMED}\n{close}\n<pasted_content id=\"2\">\n{ARMED}\n</pasted_content id=\"2\">"
            ),
            format!(
                "{open}\n<pasted_content id=\"2\">\n{ARMED}\n</pasted_content id=\"2\">\n{close}"
            ),
            format!("{open}\n{differs}\n{close}"),
            format!("{open}\n{ARMED} \n{close}"),
            format!("{open}\nsomething else entirely\n{close}"),
        ] {
            let (_temp, path) = state();
            ready(&path);
            arm("run-1", &path, "turn-1", ARMED.as_bytes()).unwrap();
            let error = submit(&path, &submitted).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Invalid, "{submitted:?}");
            assert!(
                error.message.contains("does not match the armed prompt digest"),
                "{submitted:?}"
            );
            assert!(!path.join("turns/turn-1/accepted.json").exists());
        }
    }

    #[test]
    fn accepted_delivery_must_be_a_known_form() {
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", ARMED.as_bytes()).unwrap();
        submit(&path, ARMED).unwrap();
        rewrite_json(&path.join("turns/turn-1/accepted.json"), |value| {
            value["delivery"] = serde_json::Value::String("normalized".into());
        });
        assert_eq!(
            inspect_turns("run-1", &path).unwrap_err().kind,
            ErrorKind::Unsafe
        );
    }

    #[test]
    fn prompt_id_mismatch_poisons_terminal() {
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, Some(PROMPT_ID));
        let error = handle_hook(
            "run-1",
            &path,
            r#"{"hook_event_name":"Stop","session_id":"s1","prompt_id":"223e4567-e89b-12d3-a456-426614174000","last_assistant_message":"done"}"#,
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsafe);
        assert!(path.join("poison.json").exists());
    }

    #[test]
    fn compat_without_prompt_id_is_single_turn() {
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, None);
        handle_hook(
            "run-1",
            &path,
            r#"{"hook_event_name":"Stop","session_id":"s1","last_assistant_message":"done"}"#,
        )
        .unwrap();
        let error = arm("run-1", &path, "turn-2", b"again").unwrap_err();
        assert!(error.message.contains("limited to one turn"));
    }

    #[test]
    fn interruption_after_acceptance_poisons() {
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, Some(PROMPT_ID));
        let error = wait(
            "run-1",
            &path,
            Some("turn-1"),
            WaitUntil::Terminal,
            Duration::from_secs(1),
            || true,
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Interrupted);
        assert!(path.join("poison.json").exists());
    }

    #[test]
    fn insecure_mode_fails_closed() {
        let (_temp, path) = state();
        #[cfg(not(unix))]
        let _ = path;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(
                arm("run-1", &path, "turn-1", b"hello").unwrap_err().kind,
                ErrorKind::Unsafe
            );
        }
    }

    #[test]
    fn init_is_idempotent_and_settings_are_task_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state with ' quote");
        let settings = init("run-1", &path).unwrap();
        assert_eq!(init("run-1", &path).unwrap(), settings);
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
        let command = value["hooks"]["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(command.contains("--state-dir"));
        assert!(command.contains("'\"'\"'"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(path.join("settings.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn rejects_invalid_inputs_and_unsupported_events() {
        let (_temp, path) = state();
        assert_eq!(
            arm("run-2", &path, "turn-1", b"x").unwrap_err().kind,
            ErrorKind::Unsafe
        );
        assert_eq!(
            wait(
                "run-2",
                &path,
                None,
                WaitUntil::Ready,
                Duration::ZERO,
                || false,
            )
            .unwrap_err()
            .kind,
            ErrorKind::Unsafe
        );
        assert_eq!(
            arm("bad/id", &path, "turn-1", b"x").unwrap_err().kind,
            ErrorKind::Invalid
        );
        assert_eq!(
            arm("run-1", &path, "bad/id", b"x").unwrap_err().kind,
            ErrorKind::Invalid
        );
        for turn_id in [".", ".."] {
            assert_eq!(
                arm("run-1", &path, turn_id, b"x").unwrap_err().kind,
                ErrorKind::Invalid
            );
        }
        assert!(!path.join("request.json").exists());
        assert!(!path.join("turns/request.json").exists());
        assert_eq!(
            init("run", Path::new("relative")).unwrap_err().kind,
            ErrorKind::Invalid
        );
        assert_eq!(
            handle_hook("run-1", &path, "not-json").unwrap_err().kind,
            ErrorKind::Invalid
        );
        assert_eq!(
            handle_hook("run-1", &path, r#"{"hook_event_name":"Notification"}"#)
                .unwrap_err()
                .kind,
            ErrorKind::Invalid
        );
        assert!(is_prompt_submission(
            r#"{"hook_event_name":"UserPromptSubmit"}"#
        ));
        assert!(!is_prompt_submission("not-json"));
    }

    #[test]
    fn init_rejects_state_inside_a_git_worktree() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".git")).unwrap();
        let path = temp.path().join("private/delegate");
        let error = init("run-1", &path).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Invalid);
        assert!(error.message.contains("outside a Git worktree"));
        assert!(!path.exists());
    }

    #[test]
    fn arm_enforces_size_and_one_outstanding_turn() {
        let (_temp, path) = state();
        let large = vec![b'x'; MAX_PROMPT_BYTES + 1];
        assert!(arm("run-1", &path, "turn-1", &large)
            .unwrap_err()
            .message
            .contains("byte limit"));
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        let error = arm("run-1", &path, "turn-2", b"other").unwrap_err();
        assert!(error.message.contains("still outstanding"));
    }

    #[test]
    fn arm_rejects_prompt_bytes_the_interactive_tui_cannot_preserve() {
        for prompt in [
            b"".as_slice(),
            b"first\r\nsecond".as_slice(),
            b"terminal line break\n".as_slice(),
            b"tab\tbecomes spaces".as_slice(),
            b"escape\x1bsequence".as_slice(),
            b"delete\x7fcharacter".as_slice(),
            b"embedded\0nul".as_slice(),
            b"\xffinvalid".as_slice(),
        ] {
            let (_temp, path) = state();
            let error = arm("run-1", &path, "turn-1", prompt).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Invalid);
            assert!(error.message.contains("canonical UTF-8 text"));
            assert!(inspect_turns("run-1", &path).unwrap().is_empty());
        }
    }

    #[test]
    fn waits_require_turn_ids_and_time_out_legibly() {
        let (_temp, path) = state();
        let missing = wait(
            "run-1",
            &path,
            None,
            WaitUntil::Accepted,
            Duration::ZERO,
            || false,
        )
        .unwrap_err();
        assert_eq!(missing.kind, ErrorKind::Invalid);
        let timeout = wait(
            "run-1",
            &path,
            None,
            WaitUntil::Ready,
            Duration::ZERO,
            || false,
        )
        .unwrap_err();
        assert_eq!(timeout.kind, ErrorKind::Timeout);
    }

    #[test]
    fn session_and_submission_fields_are_strict() {
        let (_temp, path) = state();
        for payload in [
            r#"{"hook_event_name":"SessionStart","session_id":"s1","cwd":"/tmp"}"#,
            r#"{"hook_event_name":"SessionStart","source":"startup","cwd":"/tmp"}"#,
            r#"{"hook_event_name":"SessionStart","source":"startup","session_id":"s1"}"#,
        ] {
            assert_eq!(
                handle_hook("run-1", &path, payload).unwrap_err().kind,
                ErrorKind::Invalid
            );
        }
        let oversized_source = serde_json::json!({
            "hook_event_name": "SessionStart",
            "source": "x".repeat(4097),
            "session_id": "s1",
            "cwd": "/tmp"
        });
        assert_eq!(
            handle_hook("run-1", &path, &oversized_source.to_string())
                .unwrap_err()
                .kind,
            ErrorKind::Invalid
        );
        assert!(!path.join("poison.json").exists());
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        for payload in [
            r#"{"hook_event_name":"UserPromptSubmit","session_id":"other","prompt":"hello"}"#,
            r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"invalid","prompt":"hello"}"#,
        ] {
            assert_eq!(
                handle_hook("run-1", &path, payload).unwrap_err().kind,
                ErrorKind::Invalid
            );
        }
    }

    #[test]
    fn terminal_requires_acceptance_message_and_matching_session() {
        let (_temp, path) = state();
        ready(&path);
        assert_eq!(
            stop(&path, PROMPT_ID, "early").unwrap_err().kind,
            ErrorKind::Unsafe
        );

        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, Some(PROMPT_ID));
        let missing =
            format!(r#"{{"hook_event_name":"Stop","session_id":"s1","prompt_id":"{PROMPT_ID}"}}"#);
        assert_eq!(
            handle_hook("run-1", &path, &missing).unwrap_err().kind,
            ErrorKind::Invalid
        );
        let mismatch = format!(
            r#"{{"hook_event_name":"Stop","session_id":"other","prompt_id":"{PROMPT_ID}","last_assistant_message":"done"}}"#
        );
        assert_eq!(
            handle_hook("run-1", &path, &mismatch).unwrap_err().kind,
            ErrorKind::Unsafe
        );
    }

    #[test]
    fn stop_failure_is_structured_and_wait_reports_failure() {
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, Some(PROMPT_ID));
        let failure = format!(
            r#"{{"hook_event_name":"StopFailure","session_id":"s1","prompt_id":"{PROMPT_ID}","error":{{"type":"tool_error"}},"error_details":{{"retryable":false}}}}"#
        );
        handle_hook("run-1", &path, &failure).unwrap();
        let result = wait(
            "run-1",
            &path,
            Some("turn-1"),
            WaitUntil::Terminal,
            Duration::ZERO,
            || false,
        )
        .unwrap();
        assert!(result.failed);
        assert!(result.json.contains("tool_error"));
    }

    #[test]
    fn oversized_terminal_content_is_rejected() {
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, Some(PROMPT_ID));
        let message = "x".repeat(MAX_MESSAGE_BYTES + 1);
        assert_eq!(
            stop(&path, PROMPT_ID, &message).unwrap_err().kind,
            ErrorKind::Invalid
        );
        let error = "x".repeat(MAX_ERROR_BYTES + 1);
        let failure = serde_json::json!({
            "hook_event_name": "StopFailure",
            "session_id": "s1",
            "prompt_id": PROMPT_ID,
            "error": {"message": error}
        });
        assert_eq!(
            handle_hook("run-1", &path, &failure.to_string())
                .unwrap_err()
                .kind,
            ErrorKind::Invalid
        );
    }

    #[test]
    fn delayed_retry_does_not_bind_to_the_next_turn() {
        const SECOND_ID: &str = "223e4567-e89b-12d3-a456-426614174000";
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, Some(PROMPT_ID));
        stop(&path, PROMPT_ID, "first").unwrap();
        arm("run-1", &path, "turn-2", b"hello").unwrap();
        accept(&path, Some(SECOND_ID));
        stop(&path, PROMPT_ID, "first").unwrap();
        assert!(!path.join("poison.json").exists());
        assert!(!path.join("turns/turn-2/result.json").exists());
        stop(&path, SECOND_ID, "second").unwrap();
    }

    #[test]
    fn ambiguous_compat_retry_with_another_acceptance_poisons() {
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, None);
        handle_hook(
            "run-1",
            &path,
            r#"{"hook_event_name":"Stop","session_id":"s1","last_assistant_message":"done"}"#,
        )
        .unwrap();

        let turn = path.join("turns/turn-2");
        create_private_dir(&turn).unwrap();
        install_json(
            &turn.join("request.json"),
            &RequestRecord {
                schema_version: SCHEMA_VERSION,
                run_id: "run-1".into(),
                turn_id: "turn-2".into(),
                prompt_sha256: hex_sha256(b"other"),
            },
        )
        .unwrap();
        install_json(
            &turn.join("accepted.json"),
            &AcceptedRecord {
                schema_version: SCHEMA_VERSION,
                run_id: "run-1".into(),
                turn_id: "turn-2".into(),
                event: "UserPromptSubmit".into(),
                session_id: "s1".into(),
                prompt_id: None,
                prompt_sha256: hex_sha256(b"other"),
                compatibility: Some("pre-2.1.196-single-turn".into()),
                delivery: Some("exact".into()),
            },
        )
        .unwrap();
        let retry =
            r#"{"hook_event_name":"Stop","session_id":"s1","last_assistant_message":"done"}"#;
        assert_eq!(
            handle_hook("run-1", &path, retry).unwrap_err().kind,
            ErrorKind::Unsafe
        );
        assert!(path.join("poison.json").exists());
    }

    #[test]
    fn malformed_and_miscorrelated_records_fail_closed() {
        let (_temp, path) = state();
        ready(&path);
        arm("run-1", &path, "turn-1", b"hello").unwrap();
        accept(&path, Some(PROMPT_ID));
        let accepted = path.join("turns/turn-1/accepted.json");
        rewrite_json(&accepted, |value| {
            value["prompt_sha256"] = serde_json::Value::String("bad".into());
        });
        assert_eq!(
            wait(
                "run-1",
                &path,
                Some("turn-1"),
                WaitUntil::Accepted,
                Duration::ZERO,
                || false,
            )
            .unwrap_err()
            .kind,
            ErrorKind::Unsafe
        );

        rewrite_json(&accepted, |value| {
            *value = serde_json::Value::String("partial".into());
        });
        assert_eq!(
            wait(
                "run-1",
                &path,
                Some("turn-1"),
                WaitUntil::Accepted,
                Duration::ZERO,
                || false,
            )
            .unwrap_err()
            .kind,
            ErrorKind::Unsafe
        );
    }

    #[test]
    fn symlinked_records_and_directories_fail_closed() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            let (_temp, path) = state();
            let target = path.join("target.json");
            std::fs::write(&target, "{}").unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
            symlink(&target, path.join("ready.json")).unwrap();
            assert_eq!(
                wait(
                    "run-1",
                    &path,
                    None,
                    WaitUntil::Ready,
                    Duration::ZERO,
                    || false,
                )
                .unwrap_err()
                .kind,
                ErrorKind::Unsafe
            );
        }
    }

    #[test]
    fn concurrent_exact_arm_is_idempotent() {
        let (_temp, path) = state();
        let first = {
            let path = path.clone();
            std::thread::spawn(move || arm("run-1", &path, "turn-1", b"hello"))
        };
        let second = {
            let path = path.clone();
            std::thread::spawn(move || arm("run-1", &path, "turn-1", b"hello"))
        };
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        let request: RequestRecord = read_json(&path.join("turns/turn-1/request.json")).unwrap();
        assert_eq!(request.prompt_sha256, hex_sha256(b"hello"));
    }

    #[test]
    fn concurrent_different_arms_preserve_one_outstanding_turn() {
        let (_temp, path) = state();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let spawn = |turn: &'static str, barrier: std::sync::Arc<std::sync::Barrier>| {
            let path = path.clone();
            std::thread::spawn(move || {
                barrier.wait();
                arm("run-1", &path, turn, turn.as_bytes())
            })
        };
        let first = spawn("turn-1", barrier.clone());
        let second = spawn("turn-2", barrier.clone());
        barrier.wait();
        let outcomes = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| outcome
                    .as_ref()
                    .is_err_and(|error| error.kind == ErrorKind::Invalid))
                .count(),
            1
        );
        let states = inspect_turns("run-1", &path).unwrap();
        assert_eq!(
            states
                .iter()
                .filter(|state| state.request.is_some() && state.result.is_none())
                .count(),
            1
        );
    }

    #[test]
    fn multiple_outstanding_records_fail_closed() {
        let (_temp, path) = state();
        for turn_id in ["turn-1", "turn-2"] {
            let turn = path.join("turns").join(turn_id);
            create_private_dir(&turn).unwrap();
            install_json(
                &turn.join("request.json"),
                &RequestRecord {
                    schema_version: SCHEMA_VERSION,
                    run_id: "run-1".into(),
                    turn_id: turn_id.into(),
                    prompt_sha256: hex_sha256(turn_id.as_bytes()),
                },
            )
            .unwrap();
        }
        let error = inspect_turns("run-1", &path).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsafe);
        assert!(error.message.contains("multiple outstanding"));
    }

    #[test]
    fn run_lock_contention_is_bounded_and_fails_closed() {
        let (_temp, path) = state();
        let _lock = acquire_run_lock(&path).unwrap();
        let started = Instant::now();
        let error = arm("run-1", &path, "turn-1", b"hello").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsafe);
        assert!(error.message.contains("lock remained busy"));
        assert!(started.elapsed() >= RUN_LOCK_TIMEOUT);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!path.join("turns/turn-1/request.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn run_lock_requires_a_private_regular_file() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let (_temp, path) = state();
        let lock = path.join(".protocol.lock");
        std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(
            arm("run-1", &path, "turn-1", b"hello").unwrap_err().kind,
            ErrorKind::Unsafe
        );

        std::fs::remove_file(&lock).unwrap();
        let target = path.join("foreign-lock");
        std::fs::write(&target, b"").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&target, &lock).unwrap();
        assert_eq!(
            arm("run-1", &path, "turn-1", b"hello").unwrap_err().kind,
            ErrorKind::Unsafe
        );
        assert!(!path.join("turns/turn-1/request.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_state_dir_is_rejected() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(OsString::from_vec(vec![b's', b't', 0xff]));
        assert_eq!(init("run-1", &path).unwrap_err().kind, ErrorKind::Invalid);
    }

    #[test]
    fn delayed_ready_wait_polls_until_committed() {
        let (_temp, path) = state();
        let producer = {
            let path = path.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(60));
                ready(&path);
            })
        };
        let observed = wait(
            "run-1",
            &path,
            None,
            WaitUntil::Ready,
            Duration::from_secs(1),
            || false,
        )
        .unwrap();
        producer.join().unwrap();
        assert!(observed.json.contains("\"startup\""));
    }

    #[test]
    fn conflicting_init_preserves_first_settings() {
        let (_temp, path) = state();
        let settings = path.join("settings.json");
        let original = std::fs::read_to_string(&settings).unwrap();
        rewrite_json(&settings, |value| {
            value["foreign"] = serde_json::Value::Bool(true);
        });
        let error = init("run-1", &path).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Invalid);
        assert_ne!(std::fs::read_to_string(settings).unwrap(), original);
    }

    #[test]
    fn malformed_ready_and_record_graphs_fail_closed() {
        let (_temp, path) = state();
        ready(&path);
        rewrite_json(&path.join("ready.json"), |value| {
            value["source"] = serde_json::Value::String("resume".into());
        });
        assert_eq!(
            wait(
                "run-1",
                &path,
                None,
                WaitUntil::Ready,
                Duration::ZERO,
                || false,
            )
            .unwrap_err()
            .kind,
            ErrorKind::Unsafe
        );

        let (_temp, path) = state();
        let turn = path.join("turns/turn-1");
        create_private_dir(&turn).unwrap();
        install_json(
            &turn.join("accepted.json"),
            &AcceptedRecord {
                schema_version: SCHEMA_VERSION,
                run_id: "run-1".into(),
                turn_id: "turn-1".into(),
                event: "UserPromptSubmit".into(),
                session_id: "s1".into(),
                prompt_id: Some(PROMPT_ID.into()),
                prompt_sha256: hex_sha256(b"hello"),
                compatibility: None,
                delivery: None,
            },
        )
        .unwrap();
        assert_eq!(
            inspect_turns("run-1", &path).unwrap_err().kind,
            ErrorKind::Unsafe
        );
    }

    #[test]
    fn insecure_record_mode_and_wrong_headers_fail_closed() {
        let (_temp, path) = state();
        ready(&path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                path.join("ready.json"),
                std::fs::Permissions::from_mode(0o640),
            )
            .unwrap();
            assert_eq!(
                wait(
                    "run-1",
                    &path,
                    None,
                    WaitUntil::Ready,
                    Duration::ZERO,
                    || false,
                )
                .unwrap_err()
                .kind,
                ErrorKind::Unsafe
            );
        }

        assert_eq!(
            validate_header(1, "other", "run-1", "test")
                .unwrap_err()
                .kind,
            ErrorKind::Unsafe
        );
        assert_eq!(
            required_nonempty(Some(String::new()), "field")
                .unwrap_err()
                .kind,
            ErrorKind::Invalid
        );
        assert_eq!(
            validate_prompt_id(Some("not-a-uuid")).unwrap_err().kind,
            ErrorKind::Invalid
        );
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn init_fails_closed_before_creating_native_windows_state() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        let error = init("run-1", &state).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Invalid);
        assert_eq!(
            error.message,
            "native Windows is unsupported for delegate state; use WSL2"
        );
        assert!(!state.exists());
    }
}
