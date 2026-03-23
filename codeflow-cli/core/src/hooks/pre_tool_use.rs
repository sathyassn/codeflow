//! `PreToolUse` hook handlers.
//!
//! Six handlers that run before tool invocations:
//! - `GateCheck`: `PathFlow` sentinel gate enforcement
//! - `TeamGuard`: Team dissolution protection
//! - `EditWriteGuard`: File scope enforcement
//! - `GhPrGuard`: Protected branch merge guard
//! - `ProtectionGuard`: Tiered resource protection
//! - `WebFetchGuard`: Domain validation for network access

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use super::{BlockCategory, HookEvent, HookHandler, HookInput, HookOutput};
use crate::error::HookError;
use crate::pathflow::sentinel;
use crate::types::SessionId;

// ---------------------------------------------------------------------------
// Exit code constants for claim enforcement
// ---------------------------------------------------------------------------

/// Advisory exit code: warn but do not block (Phase 1, Decision #5).
pub const EXIT_ADVISORY: i32 = 0;

/// Blocking exit code: hard-block the tool call (Phase 2, future).
pub const EXIT_BLOCKING: i32 = 2;

// ---------------------------------------------------------------------------
// Shared regex patterns (compiled once)
// ---------------------------------------------------------------------------

fn quoted_string_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#""(?:[^"\\]|\\.)*"|'[^']*'"#).expect("valid regex"))
}

fn git_push_pr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:^|\s|&&|\|)(?:git\s+push|gh\s+pr)(?:\s|$)").expect("valid regex")
    })
}

fn git_commit_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|\s|&&|\|)git\s+commit(?:\s|$)").expect("valid regex"))
}

fn gh_pr_merge_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|\s|&&|\|)gh\s+pr\s+merge(?:\s|$)").expect("valid regex"))
}

fn pr_number_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"gh\s+pr\s+merge\s+(\d+)").expect("valid regex"))
}

// ---------------------------------------------------------------------------
// Shared enforcement policy loading
// ---------------------------------------------------------------------------

/// Parsed enforcement policy for hook handlers.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct EnforcementPolicy {
    #[serde(default)]
    pub edit_write: EditWriteConfig,
    #[serde(default)]
    pub protected_resources: ProtectedResources,
    #[serde(default)]
    pub merge_protection: MergeProtection,
    #[serde(default)]
    pub protected_branches: Vec<String>,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub managed_tmp: ManagedTmpConfig,
    #[serde(default)]
    pub network_operations: NetworkOperations,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct EditWriteConfig {
    #[serde(default)]
    pub blocked_directories: Vec<String>,
    #[serde(default)]
    pub allowed_tmp_prefixes: Vec<String>,
    #[serde(default)]
    pub dangerous_extensions: DangerousExtensions,
    #[serde(default)]
    pub warn_on_dangerous: bool,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct DangerousExtensions {
    #[serde(default)]
    pub binary: Vec<String>,
    #[serde(default)]
    pub credential: Vec<String>,
    #[serde(default)]
    pub archive: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ProtectedResources {
    #[serde(default)]
    pub critical: Vec<String>,
    #[serde(default)]
    pub high: Vec<String>,
    #[serde(default)]
    pub moderate: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct MergeProtection {
    #[serde(default)]
    pub protected_branches: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct NetworkConfig {
    #[serde(default)]
    pub always_block_domains: AlwaysBlockDomains,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct AlwaysBlockDomains {
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub private_ip_ranges: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ManagedTmpConfig {
    #[serde(default)]
    pub protected_folders: Vec<String>,
    #[serde(default)]
    pub state_folder: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct NetworkOperations {
    #[serde(default)]
    pub git_network: NetworkPatterns,
    #[serde(default)]
    pub github_cli: NetworkPatterns,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct NetworkPatterns {
    #[serde(default)]
    pub patterns: Vec<String>,
}

impl EnforcementPolicy {
    /// Load from project dir, returning defaults on failure.
    #[must_use]
    pub fn load(project_dir: &Path) -> Self {
        let path = project_dir
            .join(".codeflow")
            .join("config")
            .join("enforcement")
            .join("enforcement-policy.json");
        match std::fs::read_to_string(&path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Return all protected paths (critical + high + moderate).
    #[must_use]
    pub fn all_protected_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        paths.extend(self.protected_resources.critical.iter().cloned());
        paths.extend(self.protected_resources.high.iter().cloned());
        paths.extend(self.protected_resources.moderate.iter().cloned());
        paths
    }

    /// Return the protected branch list, falling back to defaults.
    #[must_use]
    pub fn protected_branch_list(&self) -> Vec<String> {
        if !self.protected_branches.is_empty() {
            return self.protected_branches.clone();
        }
        vec![
            "main".into(),
            "master".into(),
            "release/*".into(),
            "production".into(),
        ]
    }

    /// Return managed tmp folders with variable expansion.
    #[must_use]
    pub fn managed_tmp_folders(&self, project_root: &str) -> Vec<String> {
        if self.managed_tmp.protected_folders.is_empty() {
            return vec![
                format!("/tmp/claude/{project_root}/managed"),
                format!("/tmp/claude/{project_root}/managed/protected-edits"),
                format!("/tmp/claude/{project_root}/managed/state"),
            ];
        }
        self.managed_tmp
            .protected_folders
            .iter()
            .map(|f| f.replace("${CF_PROJECT_ROOT}", project_root))
            .collect()
    }

    /// Return the state folder path with variable expansion.
    #[must_use]
    pub fn state_folder_path(&self, project_root: &str) -> String {
        if self.managed_tmp.state_folder.is_empty() {
            return format!("/tmp/claude/{project_root}/managed/state");
        }
        self.managed_tmp
            .state_folder
            .replace("${CF_PROJECT_ROOT}", project_root)
    }
}

/// Default enforcement policy with hardcoded values.
impl EnforcementPolicy {
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            edit_write: EditWriteConfig {
                blocked_directories: vec![
                    ".git".into(),
                    "node_modules".into(),
                    "__pycache__".into(),
                    ".venv".into(),
                    "venv".into(),
                    ".tox".into(),
                    ".nox".into(),
                    "dist".into(),
                    "build".into(),
                    ".eggs".into(),
                ],
                allowed_tmp_prefixes: vec!["/tmp/claude/".into(), "/tmp/".into()],
                dangerous_extensions: DangerousExtensions {
                    binary: vec![
                        "exe".into(),
                        "dll".into(),
                        "so".into(),
                        "dylib".into(),
                        "bin".into(),
                        "o".into(),
                        "a".into(),
                    ],
                    credential: vec![
                        "pem".into(),
                        "key".into(),
                        "crt".into(),
                        "p12".into(),
                        "pfx".into(),
                        "keystore".into(),
                        "jks".into(),
                    ],
                    archive: vec![
                        "zip".into(),
                        "tar".into(),
                        "gz".into(),
                        "rar".into(),
                        "7z".into(),
                    ],
                },
                warn_on_dangerous: true,
            },
            protected_resources: ProtectedResources {
                critical: vec![
                    ".claude/settings.json".into(),
                    ".claude/settings.local.json".into(),
                    ".claude/CLAUDE.md".into(),
                ],
                high: vec![
                    ".claude/hooks/codeflow/**".into(),
                    ".claude/settings-templates/**".into(),
                    ".codeflow/config/**".into(),
                    ".codeflow/scripts/security/**".into(),
                    ".codeflow/scripts/git-hooks/**".into(),
                    ".codeflow/scripts/shell-lib/**".into(),
                    ".github/workflows/**".into(),
                    ".github/**".into(),
                    ".git/hooks/**".into(),
                    ".state/sentinels/**".into(),
                    ".state/session/**".into(),
                ],
                moderate: vec!["project/mission.md".into(), "project/tech-stack/**".into()],
            },
            merge_protection: MergeProtection {
                protected_branches: vec![
                    "main".into(),
                    "master".into(),
                    "release/*".into(),
                    "production".into(),
                ],
            },
            protected_branches: vec![
                "main".into(),
                "master".into(),
                "release/*".into(),
                "production".into(),
            ],
            network: NetworkConfig::default(),
            managed_tmp: ManagedTmpConfig::default(),
            network_operations: NetworkOperations::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// GateCheck handler
// ---------------------------------------------------------------------------

/// Gate type classification for `PathFlow` enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateType {
    EditWrite,
    GitCommit,
    GitPushPR,
    RoleTeammateSpawn,
    Ungated,
}

/// Role teammates that require pf-3 sentinel before spawning.
const ROLE_TEAMMATES: &[&str] = &[
    "cf-development",
    "cf-planning",
    "cf-documentation",
    "cf-review",
    "cf-quality-assurance",
];

/// Classify the gate type from tool name and tool input.
#[must_use]
pub fn classify_gate_type(tool_name: &str, tool_input: &serde_json::Value) -> GateType {
    match tool_name {
        "Edit" | "Write" => GateType::EditWrite,
        "Bash" => classify_bash_gate(tool_input),
        "Task" => classify_task_gate(tool_input),
        _ => GateType::Ungated,
    }
}

fn classify_bash_gate(tool_input: &serde_json::Value) -> GateType {
    let command = tool_input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if command.is_empty() {
        return GateType::Ungated;
    }

    // Strip quoted strings to prevent false positives.
    let stripped = quoted_string_re().replace_all(command, "");

    if git_push_pr_re().is_match(&stripped) {
        return GateType::GitPushPR;
    }
    if git_commit_re().is_match(&stripped) {
        return GateType::GitCommit;
    }
    GateType::Ungated
}

fn classify_task_gate(tool_input: &serde_json::Value) -> GateType {
    let prompt = tool_input
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let name = tool_input
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let description = tool_input
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let combined = format!("{prompt} {name} {description}");
    if combined.trim().is_empty() {
        return GateType::Ungated;
    }

    for role in ROLE_TEAMMATES {
        if combined.contains(role) {
            return GateType::RoleTeammateSpawn;
        }
    }
    GateType::Ungated
}

/// `PathFlow` gate-check handler.
///
/// Blocks Edit/Write before pf-3, git commit before pf-3,
/// git push/PR before pf-5 AND ws-rev (dual gate),
/// role teammate spawn before pf-3.
///
/// For Edit/Write tools, enforces `scope_policy` from `active-task.json`:
/// - `permissive`: no scope checking, no claim acquisition
/// - `hard`: blocks edits to files NOT in `file_scope` (exit 2, no claim)
/// - `soft` (default): in-scope files auto-claimed; out-of-scope attempts
///   CRDT claim — blocked on conflict, allowed with ScopeExpansion if unclaimed
pub struct GateCheck {
    sentinel_dir: PathBuf,
    /// Path to the Loro coordination state file (e.g., `.state/coordination/state.loro`).
    state_path: PathBuf,
    /// Session ID of the current session, used as claim owner.
    session_id: SessionId,
    /// Project root directory for reading active-task.json.
    project_dir: PathBuf,
}

impl GateCheck {
    #[must_use]
    pub fn new(
        sentinel_dir: PathBuf,
        state_path: PathBuf,
        session_id: SessionId,
        project_dir: PathBuf,
    ) -> Self {
        Self {
            sentinel_dir,
            state_path,
            session_id,
            project_dir,
        }
    }

    /// Read scope_policy and file_scope from active-task.json.
    /// Returns (scope_policy, file_scope) with defaults: ("soft", empty vec).
    fn read_scope_context(&self) -> (String, Vec<String>) {
        use crate::session::active_task::get_active_task_worktree_aware;

        match get_active_task_worktree_aware(&self.project_dir) {
            Ok(Some(task)) => {
                let policy = task.scope_policy.unwrap_or_else(|| "soft".to_string());
                let scope = task.file_scope.unwrap_or_default();
                (policy, scope)
            }
            _ => ("soft".to_string(), Vec::new()),
        }
    }

    /// Check if a file path is within the declared file_scope.
    fn is_in_scope(file_path: &str, file_scope: &[String]) -> bool {
        if file_scope.is_empty() {
            return true; // No scope declared = everything in scope.
        }
        file_scope.iter().any(|scope_entry| {
            file_path == scope_entry
                || file_path.starts_with(&format!("{scope_entry}/"))
                || scope_entry.ends_with('/') && file_path.starts_with(scope_entry.as_str())
        })
    }

    /// Enforce scope_policy for an Edit/Write operation on a file path.
    ///
    /// Dispatches based on scope_policy from active-task.json:
    /// - `permissive`: allow unconditionally, no claim acquisition
    /// - `hard`: block if file is NOT in file_scope (exit 2)
    /// - `soft` (default): in-scope → auto-acquire claim; out-of-scope →
    ///   attempt claim via Coordinator::acquire; block on conflict
    fn try_acquire_claim(&self, file_path: &str) -> HookOutput {
        let (scope_policy, file_scope) = self.read_scope_context();

        match scope_policy.as_str() {
            "permissive" => HookOutput::Allow,
            "hard" => {
                if Self::is_in_scope(file_path, &file_scope) {
                    self.acquire_claim(file_path)
                } else {
                    HookOutput::Block {
                        reason: format!(
                            "BLOCKED: scope_policy=hard — file '{file_path}' is NOT in file_scope.\n\
                             Declared scope: {file_scope:?}\n\
                             Hard mode does not attempt claim acquisition for out-of-scope files.\n",
                        ),
                        category: Some(BlockCategory::Gate),
                    }
                }
            }
            _ => {
                // "soft" (default)
                if Self::is_in_scope(file_path, &file_scope) {
                    self.acquire_claim(file_path)
                } else {
                    // Out-of-scope: attempt claim, block on conflict.
                    self.acquire_claim_or_block(file_path, &file_scope)
                }
            }
        }
    }

    /// Acquire a claim on a file path (in-scope, auto-acquire).
    /// On conflict, warns but allows (graceful degradation for in-scope files).
    fn acquire_claim(&self, file_path: &str) -> HookOutput {
        use crate::coordination::Coordinator;
        use crate::coordination::loro::LoroCoordinator;

        let mut conflict_warning: Option<String> = None;
        let fp = file_path.to_string();
        let sid = self.session_id.clone();

        let result = crate::file_lock::locked_binary_rmw(
            &self.state_path,
            LoroCoordinator::in_memory,
            |bytes| {
                LoroCoordinator::from_bytes(bytes, &self.state_path)
                    .map_err(|e| format!("loro load: {e}"))
            },
            |coord| coord.export_bytes().map_err(|e| format!("loro save: {e}")),
            |coord| {
                if let Some(existing) = coord.check(&fp) {
                    if existing.owner != sid {
                        let owner = &existing.owner;
                        let token = existing.token;
                        conflict_warning = Some(format!(
                            "CLAIM CONFLICT (in-scope): file '{fp}' is claimed by session {owner} \
                             with token {token}. Proceeding (in-scope auto-acquire).",
                        ));
                        return Ok(());
                    }
                }
                match coord.acquire(&fp, &sid) {
                    Ok(_token) => Ok(()),
                    Err(crate::error::CoordinationError::ClaimConflict { path, owner }) => {
                        conflict_warning = Some(format!(
                            "CLAIM CONFLICT (in-scope): file '{path}' is claimed by session {owner}."
                        ));
                        Ok(())
                    }
                    Err(e) => {
                        eprintln!("gate-check: claim acquisition error: {e}");
                        Ok(())
                    }
                }
            },
        );

        if let Err(e) = result {
            eprintln!("gate-check: claim coordinator unavailable: {e}");
            return HookOutput::Allow;
        }
        if let Some(message) = conflict_warning {
            return HookOutput::Warn { message };
        }
        HookOutput::Allow
    }

    /// Attempt to acquire a claim on an out-of-scope file (soft mode).
    /// If claim succeeds → allow + emit ScopeExpansion.
    /// If conflict → BLOCK (exit 2) + emit ClaimConflict.
    fn acquire_claim_or_block(&self, file_path: &str, file_scope: &[String]) -> HookOutput {
        use crate::coordination::Coordinator;
        use crate::coordination::loro::LoroCoordinator;

        #[derive(Debug)]
        enum ClaimResult {
            Acquired,
            Conflict { owner: String },
        }

        let mut claim_result: Option<ClaimResult> = None;
        let fp = file_path.to_string();
        let sid = self.session_id.clone();

        let result = crate::file_lock::locked_binary_rmw(
            &self.state_path,
            LoroCoordinator::in_memory,
            |bytes| {
                LoroCoordinator::from_bytes(bytes, &self.state_path)
                    .map_err(|e| format!("loro load: {e}"))
            },
            |coord| coord.export_bytes().map_err(|e| format!("loro save: {e}")),
            |coord| {
                // Check if another session already holds a claim.
                if let Some(existing) = coord.check(&fp) {
                    if existing.owner != sid {
                        claim_result = Some(ClaimResult::Conflict {
                            owner: existing.owner.as_str().to_string(),
                        });
                        return Ok(());
                    }
                }
                match coord.acquire(&fp, &sid) {
                    Ok(_token) => {
                        claim_result = Some(ClaimResult::Acquired);
                        Ok(())
                    }
                    Err(crate::error::CoordinationError::ClaimConflict { owner, .. }) => {
                        claim_result = Some(ClaimResult::Conflict {
                            owner: owner.as_str().to_string(),
                        });
                        Ok(())
                    }
                    Err(e) => {
                        eprintln!("gate-check: claim acquisition error: {e}");
                        Ok(())
                    }
                }
            },
        );

        if let Err(e) = result {
            eprintln!("gate-check: claim coordinator unavailable: {e}");
            return HookOutput::Allow;
        }

        match claim_result {
            Some(ClaimResult::Acquired) => {
                eprintln!(
                    "SCOPE EXPANSION: file '{file_path}' is outside file_scope {file_scope:?} but claim acquired. \
                     Edit allowed.",
                );
                HookOutput::Allow
            }
            Some(ClaimResult::Conflict { owner }) => HookOutput::Block {
                reason: format!(
                    "BLOCKED: scope_policy=soft — CLAIM CONFLICT on out-of-scope file.\n\
                     File: {file_path}\n\
                     Held by session: {owner}\n\
                     Your file_scope: {file_scope:?}\n\
                     The file is outside your declared scope AND held by another session.\n",
                ),
                category: Some(BlockCategory::Gate),
            },
            None => {
                // No result = coordinator issue, graceful degradation.
                HookOutput::Allow
            }
        }
    }

    /// Cumulative push/PR gate: verify ALL pf-1..pf-5 + ALL pipeline stage sentinels.
    /// Mirrors Go `checkCumulativePushPRGate()` in gate/gate.go.
    fn check_cumulative_push_pr_gate(&self) -> HookOutput {
        use super::pipeline;

        // Cumulative phase check: ALL pf-1 through pf-5.
        let (ok, missing) = pipeline::verify_cumulative_phase_sentinels(&self.sentinel_dir, 5);
        if !ok {
            return HookOutput::Block {
                reason: format!(
                    "BLOCKED: PathFlow gate - prerequisite not met\n\
                     Reason: git_push_pr requires all phases through PF5-VERIFY. \
                     Missing phase sentinel: pathflow-{missing}\n\
                     Gate: git_push_pr\n"
                ),
                category: Some(BlockCategory::Gate),
            };
        }

        // Cumulative stage check: ALL pipeline stages.
        let session_dir = pipeline::derive_session_dir(&self.sentinel_dir);
        let work_type = pipeline::read_work_type_from_session_status(&session_dir);

        // work_type must be set if pf-3+ exists (set by register_work_type
        // at checkpoint-complete for pf-3). If missing, the checkpoint system
        // was bypassed (e.g., manual sentinel creation). Block to prevent
        // pushing unreviewed/unverified work.
        if work_type.is_empty() {
            return HookOutput::Block {
                reason: "BLOCKED: PathFlow gate - prerequisite not met\n\
                     Reason: git_push_pr requires work_type in session status. \
                     work_type is not set (checkpoint system may have been bypassed).\n\
                     Gate: git_push_pr\n"
                    .to_string(),
                category: Some(BlockCategory::Gate),
            };
        }

        let config_dir = pipeline::derive_config_dir(&self.sentinel_dir);
        match pipeline::load_pipelines(&config_dir) {
            Ok(pipelines) => {
                if let Some(pipeline_stages) = pipelines.get(&work_type) {
                    let (all_ok, missing_stage) = pipeline::verify_cumulative_stage_sentinels(
                        &self.sentinel_dir,
                        pipeline_stages,
                        pipeline_stages.len().saturating_sub(1),
                    );
                    if !all_ok {
                        return HookOutput::Block {
                            reason: format!(
                                "BLOCKED: PathFlow gate - prerequisite not met\n\
                                 Reason: git_push_pr requires all pipeline stages. \
                                 Missing stage sentinel: pathflow-{missing_stage}\n\
                                 Pipeline for {work_type}: {pipeline_stages:?}\n\
                                 Gate: git_push_pr\n"
                            ),
                            category: Some(BlockCategory::Gate),
                        };
                    }
                }
            }
            Err(_) => {
                // Config load failure -- graceful degradation, allow through.
                eprintln!("gate: cannot load pipelines for push/PR gate");
            }
        }

        HookOutput::Allow
    }
}

impl HookHandler for GateCheck {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let tool_name = input.tool_name.as_deref().unwrap_or("");
        let tool_input = input.tool_input.clone().unwrap_or_default();

        let gate_type = classify_gate_type(tool_name, &tool_input);

        match gate_type {
            GateType::Ungated => Ok(HookOutput::Allow),
            GateType::EditWrite => {
                if !sentinel::check_by_name(&self.sentinel_dir, "pf-3") {
                    return Ok(HookOutput::Block {
                        reason: format!(
                            "BLOCKED: PathFlow gate - prerequisite not met\n\
                             Reason: {gate_type:?} requires PF3-CLASSIFY (branch creation). \
                             No pathflow-pf-3 sentinel found.\n\
                             Gate: {gate_type:?}\n"
                        ),
                        category: Some(BlockCategory::Gate),
                    });
                }
                // pf-3 passed — attempt claim acquisition on the file path.
                let file_path = tool_input
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if file_path.is_empty() {
                    return Ok(HookOutput::Allow);
                }
                Ok(self.try_acquire_claim(file_path))
            }
            GateType::GitCommit | GateType::RoleTeammateSpawn => {
                if sentinel::check_by_name(&self.sentinel_dir, "pf-3") {
                    Ok(HookOutput::Allow)
                } else {
                    Ok(HookOutput::Block {
                        reason: format!(
                            "BLOCKED: PathFlow gate - prerequisite not met\n\
                             Reason: {gate_type:?} requires PF3-CLASSIFY (branch creation). \
                             No pathflow-pf-3 sentinel found.\n\
                             Gate: {gate_type:?}\n"
                        ),
                        category: Some(BlockCategory::Gate),
                    })
                }
            }
            GateType::GitPushPR => Ok(self.check_cumulative_push_pr_gate()),
        }
    }

    fn name(&self) -> &'static str {
        "gate-check"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PreToolUse]
    }
}

// ---------------------------------------------------------------------------
// TeamGuard handler
// ---------------------------------------------------------------------------

/// Team guard: blocks `TeamDelete` while `PathFlow` is active and pf-6 is missing.
pub struct TeamGuard {
    session_dir: PathBuf,
    sentinel_dir: PathBuf,
}

impl TeamGuard {
    #[must_use]
    pub fn new(session_dir: PathBuf, sentinel_dir: PathBuf) -> Self {
        Self {
            session_dir,
            sentinel_dir,
        }
    }

    /// Check pipeline stage sentinels before allowing `TeamDelete`.
    /// Mirrors Go `checkPipelineStageSentinels()` in team/guard.go.
    fn check_pipeline_stage_sentinels(&self) -> Option<HookOutput> {
        use super::pipeline;

        let work_type = pipeline::read_work_type_from_session_status(&self.session_dir);
        if work_type.is_empty() {
            return None; // No work type -- allow through
        }

        let config_dir = pipeline::derive_config_dir(&self.sentinel_dir);
        let Ok(pipelines) = pipeline::load_pipelines(&config_dir) else {
            return None; // Config load failure -- graceful degradation
        };

        let pipeline_stages = pipelines.get(&work_type)?;

        let (ok, missing) = pipeline::verify_cumulative_stage_sentinels(
            &self.sentinel_dir,
            pipeline_stages,
            pipeline_stages.len().saturating_sub(1),
        );
        if !ok {
            return Some(HookOutput::Block {
                reason: format!(
                    "BLOCKED: TeamDelete blocked -- stage sentinel missing: {missing}\n\
                     All pipeline stages must complete before team deletion.\n\
                     Pipeline for {work_type}: {pipeline_stages:?}\n"
                ),
                category: Some(BlockCategory::TeamGuard),
            });
        }

        None
    }
}

impl HookHandler for TeamGuard {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let tool_name = input.tool_name.as_deref().unwrap_or("");

        let is_team_delete = tool_name == "TeamDelete";

        // For Teammate tool, only gate "cleanup" operations.
        if !is_team_delete && tool_name != "Teammate" {
            return Ok(HookOutput::Allow);
        }

        if !is_team_delete {
            let op = input
                .tool_input
                .as_ref()
                .and_then(|v| v.get("operation"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if op != "cleanup" {
                return Ok(HookOutput::Allow);
            }
        }

        // Check pathflow session status (status.json is sole authority).
        let status_path = self.session_dir.join("pathflow-session-status.json");
        let is_active = if let Ok(data) = std::fs::read_to_string(&status_path) {
            serde_json::from_str::<serde_json::Value>(&data)
                .ok()
                .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(String::from))
                .is_some_and(|s| !s.is_empty() && s != "pf-complete")
        } else {
            false
        };
        if !is_active {
            return Ok(HookOutput::Allow);
        }

        // PF7-END gate: if pf-6 sentinel exists AND all pipeline stages complete, allow TeamDelete.
        if is_team_delete && sentinel::check_by_name(&self.sentinel_dir, "pf-6") {
            // Also verify ALL pipeline stage sentinels exist.
            if let Some(block) = self.check_pipeline_stage_sentinels() {
                return Ok(block);
            }
            return Ok(HookOutput::Allow);
        }

        let blocked_op = if is_team_delete {
            "TeamDelete"
        } else {
            "cleanup"
        };

        Ok(HookOutput::Block {
            reason: format!(
                "BLOCKED: Team cleanup/deletion not allowed during active PathFlow\n\
                 Reason: PathFlow is active - team resources are still in use\n\
                 Operation: {blocked_op}\n\n\
                 Complete the PathFlow workflow (PF7-END) before cleaning up team resources.\n"
            ),
            category: Some(BlockCategory::TeamGuard),
        })
    }

    fn name(&self) -> &'static str {
        "team-guard"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PreToolUse]
    }
}

// ---------------------------------------------------------------------------
// EditWriteGuard handler
// ---------------------------------------------------------------------------

/// Edit/Write scope enforcement.
pub struct EditWriteGuard {
    project_dir: PathBuf,
    policy: EnforcementPolicy,
    current_branch: String,
}

impl EditWriteGuard {
    #[must_use]
    pub fn new(project_dir: PathBuf, policy: EnforcementPolicy, current_branch: String) -> Self {
        Self {
            project_dir,
            policy,
            current_branch,
        }
    }

    fn check_path(&self, tool_name: &str, file_path: &str) -> HookOutput {
        let config = &self.policy.edit_write;

        // 1. Check allowed tmp prefixes (bypass all checks).
        let prefixes = if config.allowed_tmp_prefixes.is_empty() {
            vec!["/tmp/claude/".to_string(), "/tmp/".to_string()]
        } else {
            config.allowed_tmp_prefixes.clone()
        };
        for prefix in &prefixes {
            if file_path.starts_with(prefix.as_str()) {
                return HookOutput::Allow;
            }
        }

        // 2. Check protected branch.
        if !self.current_branch.is_empty()
            && is_protected_branch(&self.current_branch, &self.policy.protected_branch_list())
        {
            return edit_write_block(
                tool_name,
                file_path,
                &format!(
                    "Cannot write files directly on protected branch '{}'. \
                     Create a feature branch first.",
                    self.current_branch
                ),
            );
        }

        // 3. Resolve to absolute path.
        let abs_path = if Path::new(file_path).is_absolute() {
            PathBuf::from(file_path)
        } else {
            self.project_dir.join(file_path)
        };
        let abs_str = abs_path.to_string_lossy();
        let proj_str = self.project_dir.to_string_lossy();

        // 4. Check project containment.
        if abs_str.as_ref() != proj_str.as_ref() && !abs_str.starts_with(&format!("{proj_str}/")) {
            return edit_write_block(
                tool_name,
                file_path,
                "Write operations must be within the project directory or allowed temp directories",
            );
        }

        // 5. Check blocked directories.
        let blocked_dirs = if config.blocked_directories.is_empty() {
            vec![
                ".git".into(),
                "node_modules".into(),
                "__pycache__".into(),
                ".venv".into(),
                "venv".into(),
            ]
        } else {
            config.blocked_directories.clone()
        };
        for dir in &blocked_dirs {
            if match_blocked_dir(file_path, dir) || match_blocked_dir(&abs_str, dir) {
                return edit_write_block(
                    tool_name,
                    file_path,
                    &format!("Cannot write to '{dir}' directory"),
                );
            }
        }

        // 6. Dangerous extension warnings.
        if config.warn_on_dangerous {
            if let Some(msg) =
                check_dangerous_extension(tool_name, file_path, &config.dangerous_extensions)
            {
                return HookOutput::Warn { message: msg };
            }
        }

        HookOutput::Allow
    }
}

fn match_blocked_dir(path: &str, blocked: &str) -> bool {
    path.starts_with(&format!("{blocked}/"))
        || path.contains(&format!("/{blocked}/"))
        || path.ends_with(&format!("/{blocked}"))
}

fn check_dangerous_extension(
    tool_name: &str,
    file_path: &str,
    exts: &DangerousExtensions,
) -> Option<String> {
    let ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    if ext.is_empty() {
        return None;
    }
    if exts.binary.iter().any(|e| e == &ext) {
        return Some(format!(
            "WARNING: {tool_name} targets binary file: {file_path}"
        ));
    }
    if exts.credential.iter().any(|e| e == &ext) {
        return Some(format!(
            "WARNING: {tool_name} targets credential file: {file_path}"
        ));
    }
    if exts.archive.iter().any(|e| e == &ext) {
        return Some(format!(
            "WARNING: {tool_name} targets archive file: {file_path}"
        ));
    }
    None
}

fn edit_write_block(tool_name: &str, file_path: &str, reason: &str) -> HookOutput {
    HookOutput::Block {
        reason: format!(
            "BLOCKED: {tool_name} operation not allowed\n\n\
             Path: {file_path}\n\
             Reason: {reason}\n\n\
             This is a safety restriction enforced by CodeFlow.\n"
        ),
        category: Some(BlockCategory::EditWriteScope),
    }
}

impl HookHandler for EditWriteGuard {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let tool_name = input.tool_name.as_deref().unwrap_or("");

        // Only check Edit and Write tools.
        if tool_name != "Edit" && tool_name != "Write" {
            return Ok(HookOutput::Allow);
        }

        let file_path = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("file_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if file_path.is_empty() {
            return Ok(HookOutput::Allow);
        }

        Ok(self.check_path(tool_name, file_path))
    }

    fn name(&self) -> &'static str {
        "edit-write-guard"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PreToolUse]
    }
}

// ---------------------------------------------------------------------------
// GhPrGuard handler
// ---------------------------------------------------------------------------

/// Trait for resolving PR target branches (DI for testability).
pub trait PRResolver: Send + Sync {
    /// Resolve the target (base) branch for a given PR number.
    /// Returns `None` if the PR cannot be resolved.
    fn resolve_target_branch(&self, pr_number: &str) -> Option<String>;
}

/// Production resolver that calls `gh pr view`.
pub struct OsPRResolver;

impl PRResolver for OsPRResolver {
    fn resolve_target_branch(&self, pr_number: &str) -> Option<String> {
        let output = std::process::Command::new("gh")
            .args([
                "pr",
                "view",
                pr_number,
                "--json",
                "baseRefName",
                "-q",
                ".baseRefName",
            ])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() {
            None
        } else {
            Some(branch)
        }
    }
}

/// GitHub PR merge guard: blocks `gh pr merge` on protected branches.
pub struct GhPrGuard<R: PRResolver = OsPRResolver> {
    protected_branches: Vec<String>,
    resolver: R,
}

impl GhPrGuard<OsPRResolver> {
    #[must_use]
    pub fn new(protected_branches: Vec<String>) -> Self {
        Self {
            protected_branches,
            resolver: OsPRResolver,
        }
    }
}

impl<R: PRResolver> GhPrGuard<R> {
    #[must_use]
    pub fn with_resolver(protected_branches: Vec<String>, resolver: R) -> Self {
        Self {
            protected_branches,
            resolver,
        }
    }
}

impl<R: PRResolver> HookHandler for GhPrGuard<R> {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let tool_name = input.tool_name.as_deref().unwrap_or("");

        // Only check Bash tool calls.
        if tool_name != "Bash" {
            return Ok(HookOutput::Allow);
        }

        let command = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if command.is_empty() || !gh_pr_merge_re().is_match(command) {
            return Ok(HookOutput::Allow);
        }

        // Extract PR number.
        let Some(caps) = pr_number_re().captures(command) else {
            return Ok(HookOutput::Allow); // No PR number, allow through.
        };
        let pr_number = &caps[1];

        // Resolve target branch.
        let Some(target_branch) = self.resolver.resolve_target_branch(pr_number) else {
            return Ok(HookOutput::Allow); // Cannot resolve, allow through.
        };

        // Check if target branch matches any protected pattern.
        for protected in &self.protected_branches {
            if match_branch_pattern(&target_branch, protected) {
                return Ok(HookOutput::Block {
                    reason: format!(
                        "BLOCKED: Cannot merge PR #{pr_number} into protected branch '{target_branch}'.\n\
                         Protected branches require manual merge via GitHub UI or admin override.\n\
                         Matched protection pattern: {protected}\n\n\
                         MUST: Do not attempt to merge into protected branches via CLI.\n"
                    ),
                    category: Some(BlockCategory::GhPrGuard),
                });
            }
        }

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "gh-pr-guard"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PreToolUse]
    }
}

// ---------------------------------------------------------------------------
// ProtectionGuard handler
// ---------------------------------------------------------------------------

/// Tiered resource protection: critical/high -> block, moderate -> warn.
pub struct ProtectionGuard {
    policy: EnforcementPolicy,
    project_dir: PathBuf,
    project_root: String,
    /// Worktree name extracted from `CODEFLOW_WORKTREE_PATH` (if set).
    /// Used to scope staging paths per worktree.
    worktree_name: Option<String>,
}

impl ProtectionGuard {
    #[must_use]
    pub fn new(policy: EnforcementPolicy, project_dir: PathBuf) -> Self {
        let project_root = std::env::var("CF_PROJECT_ROOT").unwrap_or_else(|_| {
            project_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("codeflow")
                .to_string()
        });
        let worktree_name = Self::read_worktree_name();
        Self {
            policy,
            project_dir,
            project_root,
            worktree_name,
        }
    }

    /// Testable constructor that accepts worktree_name directly.
    #[cfg(test)]
    fn new_with_worktree(
        policy: EnforcementPolicy,
        project_dir: PathBuf,
        worktree_name: Option<String>,
    ) -> Self {
        let project_root = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("codeflow")
            .to_string();
        Self {
            policy,
            project_dir,
            project_root,
            worktree_name,
        }
    }

    /// Read worktree name from `CODEFLOW_WORKTREE_PATH` env var.
    fn read_worktree_name() -> Option<String> {
        std::env::var("CODEFLOW_WORKTREE_PATH").ok().and_then(|p| {
            Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        })
    }

    /// Build the staging directory path, scoped by worktree when available.
    fn staging_dir(&self) -> String {
        if let Some(ref wt_name) = self.worktree_name {
            format!(
                "/tmp/claude/{}/{}/managed/protected-edits",
                self.project_root, wt_name
            )
        } else {
            format!("/tmp/claude/{}/managed/protected-edits", self.project_root)
        }
    }

    fn normalize_path(&self, file_path: &str) -> String {
        let proj_prefix = format!("{}/", self.project_dir.to_string_lossy());
        if file_path.starts_with(&proj_prefix) {
            file_path
                .strip_prefix(&proj_prefix)
                .unwrap_or(file_path)
                .to_string()
        } else {
            file_path.to_string()
        }
    }

    fn is_staging_path(&self, path: &str) -> bool {
        let base = self.staging_dir();
        path.starts_with(&format!("{base}/"))
            || (path.starts_with("/tmp/claude/") && path.contains("/managed/protected-edits/"))
    }

    fn check_tier(&self, rel_path: &str) -> HookOutput {
        // Critical tier.
        for pattern in &self.policy.protected_resources.critical {
            if matches_glob_pattern(rel_path, pattern) {
                return self.block_verdict("critical", rel_path);
            }
        }

        // High tier.
        for pattern in &self.policy.protected_resources.high {
            if matches_glob_pattern(rel_path, pattern) {
                return self.block_verdict("high", rel_path);
            }
        }

        // Moderate tier -- warn.
        for pattern in &self.policy.protected_resources.moderate {
            if matches_glob_pattern(rel_path, pattern) {
                return HookOutput::Warn {
                    message: format!("Note: Editing moderately protected file: {rel_path}"),
                };
            }
        }

        HookOutput::Allow
    }

    fn block_verdict(&self, tier: &str, rel_path: &str) -> HookOutput {
        let staging_dir = self.staging_dir();
        let staged_path = format!("{staging_dir}/{rel_path}");

        HookOutput::Block {
            reason: format!(
                "BLOCKED: {} resource protection\n\n\
                 Path: {rel_path}\n\
                 Tier: {}\n\n\
                 AUTO-STAGING WORKFLOW:\n\
                 1. mkdir -p {}\n\
                 2. cp {rel_path} {staged_path}\n\
                 3. Edit the staged copy at: {staged_path}\n\
                 4. When ready, provide user: cp {staged_path} {rel_path}\n\
                 5. After user applies: rm {staged_path}\n",
                capitalize_first(tier),
                tier.to_uppercase(),
                staged_path.rsplit_once('/').map_or(".", |p| p.0),
            ),
            category: Some(BlockCategory::ProtectedResource),
        }
    }

    /// Check Bash commands for cp/mv operations targeting protected paths.
    ///
    /// Extracts the destination from `cp ... {dest}` or `mv ... {dest}` commands
    /// and applies the same tier check as Edit/Write operations.
    fn check_bash_file_ops(&self, input: &HookInput) -> HookOutput {
        let command = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if command.is_empty() {
            return HookOutput::Allow;
        }

        // Look for cp or mv commands.
        let tokens: Vec<&str> = command.split_whitespace().collect();
        let has_cp_mv = tokens.iter().any(|t| *t == "cp" || *t == "mv");
        if !has_cp_mv {
            return HookOutput::Allow;
        }

        // Extract destination: last non-flag token after cp/mv.
        // Skip flags (tokens starting with -) and the command itself.
        let mut found_cmd = false;
        let mut args: Vec<&str> = Vec::new();
        for token in &tokens {
            if !found_cmd {
                if *token == "cp" || *token == "mv" {
                    found_cmd = true;
                }
                continue;
            }
            if token.starts_with('-') {
                continue;
            }
            args.push(token);
        }

        // Destination is the last argument (cp src dest).
        let dest = match args.last() {
            Some(d) if args.len() >= 2 => *d,
            _ => return HookOutput::Allow,
        };

        // Skip if destination is in staging area.
        if self.is_staging_path(dest) {
            return HookOutput::Allow;
        }

        let rel_dest = self.normalize_path(dest);
        self.check_tier(&rel_dest)
    }
}

impl HookHandler for ProtectionGuard {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let tool_name = input.tool_name.as_deref().unwrap_or("");

        // Check Bash cp/mv commands targeting protected paths.
        if tool_name == "Bash" {
            return Ok(self.check_bash_file_ops(&input));
        }

        if tool_name != "Edit" && tool_name != "Write" {
            return Ok(HookOutput::Allow);
        }

        let file_path = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("file_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if file_path.is_empty() {
            return Ok(HookOutput::Allow);
        }

        let rel_path = self.normalize_path(file_path);

        if self.is_staging_path(file_path) {
            return Ok(HookOutput::Allow);
        }

        Ok(self.check_tier(&rel_path))
    }

    fn name(&self) -> &'static str {
        "protection-guard"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PreToolUse]
    }
}

// ---------------------------------------------------------------------------
// WebFetchGuard handler
// ---------------------------------------------------------------------------

/// `WebFetch` domain validation handler.
pub struct WebFetchGuard {
    trusted_domains: Vec<String>,
    blocked_patterns: Vec<String>,
    private_ip_ranges: Vec<String>,
}

impl WebFetchGuard {
    #[must_use]
    pub fn new(
        trusted_domains: Vec<String>,
        blocked_patterns: Vec<String>,
        private_ip_ranges: Vec<String>,
    ) -> Self {
        Self {
            trusted_domains,
            blocked_patterns,
            private_ip_ranges,
        }
    }

    /// Load from enforcement policy and trusted-domains list file.
    #[must_use]
    pub fn from_policy(policy: &EnforcementPolicy, trusted_domains_path: &Path) -> Self {
        let trusted = load_trusted_domains(trusted_domains_path);
        Self {
            trusted_domains: trusted,
            blocked_patterns: policy.network.always_block_domains.patterns.clone(),
            private_ip_ranges: policy
                .network
                .always_block_domains
                .private_ip_ranges
                .clone(),
        }
    }

    fn validate_url(&self, raw_url: &str) -> HookOutput {
        let lower = raw_url.to_lowercase();

        // Block file:// and data: URLs.
        if lower.starts_with("file://") {
            return HookOutput::Block {
                reason: format!("BLOCKED: file:// URLs are not allowed\nURL: {raw_url}\n"),
                category: Some(BlockCategory::WebFetch),
            };
        }
        if lower.starts_with("data:") {
            return HookOutput::Block {
                reason: format!("BLOCKED: data: URLs are not allowed\nURL: {raw_url}\n"),
                category: Some(BlockCategory::WebFetch),
            };
        }

        let Some(domain) = extract_domain(raw_url) else {
            return HookOutput::Allow;
        };

        // Trusted domains bypass block checks.
        if self.is_trusted_domain(&domain) {
            return HookOutput::Allow;
        }

        // Check always-blocked patterns.
        if self.is_blocked_domain(&domain) {
            return HookOutput::Block {
                reason: format!(
                    "BLOCKED: Internal/local network access forbidden\n\
                     URL: {raw_url}\nDomain: {domain}\n\
                     Access to localhost, internal networks, and private IPs is not allowed.\n"
                ),
                category: Some(BlockCategory::WebFetch),
            };
        }

        // Check private IP ranges.
        if self.is_private_ip(&domain) {
            return HookOutput::Block {
                reason: format!(
                    "BLOCKED: Internal/local network access forbidden\n\
                     URL: {raw_url}\nDomain: {domain}\n\
                     Access to private IP ranges is not allowed.\n"
                ),
                category: Some(BlockCategory::WebFetch),
            };
        }

        // Untrusted domain -- ask for approval (permission request).
        HookOutput::Warn {
            message: format!(
                "{{\"hookSpecificOutput\":{{\"permissionDecision\":\"ask\",\
                 \"domain\":\"{domain}\",\"url\":\"{raw_url}\"}}}}"
            ),
        }
    }

    fn is_trusted_domain(&self, domain: &str) -> bool {
        let d = domain.to_lowercase();
        for trusted in &self.trusted_domains {
            let t = trusted.to_lowercase();
            if d == t || d.ends_with(&format!(".{t}")) {
                return true;
            }
        }
        false
    }

    fn is_blocked_domain(&self, domain: &str) -> bool {
        for pattern in &self.blocked_patterns {
            if match_domain_pattern(domain, pattern) {
                return true;
            }
        }
        false
    }

    fn is_private_ip(&self, domain: &str) -> bool {
        for pattern in &self.private_ip_ranges {
            if match_ip_pattern(domain, pattern) {
                return true;
            }
        }
        // Also check standard private ranges via parsing.
        if let Ok(ip) = domain.parse::<std::net::IpAddr>() {
            return ip.is_loopback()
                || match ip {
                    std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
                    std::net::IpAddr::V6(v6) => v6.is_loopback(),
                };
        }
        false
    }
}

fn extract_domain(raw_url: &str) -> Option<String> {
    let parsed = url::Url::parse(raw_url).ok()?;
    let host = parsed.host_str()?;
    if host.is_empty() {
        return None;
    }
    Some(host.to_lowercase())
}

fn match_domain_pattern(domain: &str, pattern: &str) -> bool {
    let d = domain.to_lowercase();
    let p = pattern.to_lowercase();

    if d == p {
        return true;
    }
    // *.suffix pattern
    if let Some(suffix) = p.strip_prefix("*.") {
        if d.ends_with(&format!(".{suffix}")) || d == suffix {
            return true;
        }
    }
    // prefix.* pattern
    if let Some(prefix) = p.strip_suffix(".*") {
        if d.starts_with(&format!("{prefix}.")) || d == prefix {
            return true;
        }
    }
    false
}

fn match_ip_pattern(domain: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        domain.starts_with(prefix)
    } else {
        domain == pattern
    }
}

fn contains_network_tool(command: &str) -> bool {
    for tool in &[
        "curl", "wget", "fetch", "nc", "netcat", "ssh", "scp", "rsync", "ftp",
    ] {
        if command.contains(tool) {
            return true;
        }
    }
    false
}

fn extract_urls(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .filter_map(|w| {
            let trimmed = w.trim_matches(|c: char| c == '"' || c == '\'');
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn load_trusted_domains(path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(data) => data
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect(),
        Err(_) => Vec::new(),
    }
}

impl HookHandler for WebFetchGuard {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let tool_name = input.tool_name.as_deref().unwrap_or("");

        match tool_name {
            "WebFetch" | "WebSearch" => {
                let url = input
                    .tool_input
                    .as_ref()
                    .and_then(|v| v.get("url"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if url.is_empty() {
                    return Ok(HookOutput::Allow);
                }
                Ok(self.validate_url(url))
            }
            "Bash" => {
                let command = input
                    .tool_input
                    .as_ref()
                    .and_then(|v| v.get("command"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if command.is_empty() || !contains_network_tool(command) {
                    return Ok(HookOutput::Allow);
                }
                let urls = extract_urls(command);
                for url in &urls {
                    let verdict = self.validate_url(url);
                    if matches!(verdict, HookOutput::Block { .. }) {
                        return Ok(verdict);
                    }
                }
                Ok(HookOutput::Allow)
            }
            _ => Ok(HookOutput::Allow),
        }
    }

    fn name(&self) -> &'static str {
        "webfetch-guard"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PreToolUse]
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Check if a branch matches a protected branch pattern (supports globs).
#[must_use]
pub fn is_protected_branch(branch: &str, protected: &[String]) -> bool {
    for pat in protected {
        if match_branch_pattern(branch, pat) {
            return true;
        }
    }
    false
}

/// Check if a branch name matches a single pattern (exact or glob).
#[must_use]
pub fn match_branch_pattern(branch: &str, pattern: &str) -> bool {
    if branch == pattern {
        return true;
    }
    if pattern.contains('*') {
        // Convert glob to regex.
        let regex_str = format!("^{}$", regex::escape(pattern).replace(r"\*", ".*"));
        if let Ok(re) = Regex::new(&regex_str) {
            return re.is_match(branch);
        }
    }
    false
}

/// Match a file path against a glob pattern supporting `**`.
#[must_use]
pub fn matches_glob_pattern(path: &str, pattern: &str) -> bool {
    if path == pattern {
        return true;
    }
    // Handle ** recursive glob -- prefix match.
    if pattern.contains("**") {
        let prefix = pattern.split("**").next().unwrap_or("");
        if path.starts_with(prefix) {
            return true;
        }
    }
    // Handle single * glob (within one directory level).
    if pattern.contains('*') && !pattern.contains("**") {
        // Use std::path::Path's built-in matching isn't available, so use simple conversion.
        let regex_str = format!("^{}$", regex::escape(pattern).replace(r"\*", "[^/]*"));
        if let Ok(re) = Regex::new(&regex_str) {
            return re.is_match(path);
        }
    }
    false
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a `GateCheck` with a sentinel dir and in-memory coordinator.
    fn make_gate_check(sentinel_dir: PathBuf) -> GateCheck {
        // Use /dev/null as state_path — LoroCoordinator::new will create a fresh doc
        // since /dev/null is empty. For tests that need real claim state, construct
        // GateCheck directly with a proper state_path.
        GateCheck::new(
            sentinel_dir.clone(),
            PathBuf::from("/dev/null"),
            SessionId::new_unchecked("ses-gate-test"),
            sentinel_dir,
        )
    }

    // -- GateCheck tests --

    #[test]
    fn test_gate_classify_edit_write() {
        assert_eq!(
            classify_gate_type("Edit", &serde_json::json!({})),
            GateType::EditWrite
        );
        assert_eq!(
            classify_gate_type("Write", &serde_json::json!({})),
            GateType::EditWrite
        );
    }

    #[test]
    fn test_gate_classify_bash_git_push() {
        let input = serde_json::json!({"command": "git push origin main"});
        assert_eq!(classify_gate_type("Bash", &input), GateType::GitPushPR);
    }

    #[test]
    fn test_gate_classify_bash_gh_pr() {
        let input = serde_json::json!({"command": "gh pr create --title test"});
        assert_eq!(classify_gate_type("Bash", &input), GateType::GitPushPR);
    }

    #[test]
    fn test_gate_classify_bash_git_commit() {
        let input = serde_json::json!({"command": "git commit -m 'test'"});
        assert_eq!(classify_gate_type("Bash", &input), GateType::GitCommit);
    }

    #[test]
    fn test_gate_classify_bash_quoted_git_push_ignored() {
        // git push inside quotes should be stripped and not match.
        let input = serde_json::json!({"command": "echo \"git push\" | cat"});
        assert_eq!(classify_gate_type("Bash", &input), GateType::Ungated);
    }

    #[test]
    fn test_gate_classify_task_role_teammate() {
        let input = serde_json::json!({"name": "cf-development", "prompt": "implement feature"});
        assert_eq!(
            classify_gate_type("Task", &input),
            GateType::RoleTeammateSpawn
        );
    }

    #[test]
    fn test_gate_classify_task_function_teammate() {
        let input = serde_json::json!({"name": "cf-security", "prompt": "check posture"});
        assert_eq!(classify_gate_type("Task", &input), GateType::Ungated);
    }

    #[test]
    fn test_gate_classify_ungated_tool() {
        assert_eq!(
            classify_gate_type("Read", &serde_json::json!({})),
            GateType::Ungated
        );
    }

    #[test]
    fn test_gate_check_blocks_edit_without_pf3() {
        let dir = tempfile::tempdir().unwrap();
        let handler = make_gate_check(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "test.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_gate_check_allows_edit_with_pf3() {
        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();
        let handler = make_gate_check(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "test.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_gate_check_dual_gate_push_blocks_missing_pf5() {
        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "ws-rev").unwrap();
        let handler = make_gate_check(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "git push origin feat/test"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_gate_check_dual_gate_push_blocks_missing_ws_rev() {
        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-5").unwrap();
        let handler = make_gate_check(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "git push origin feat/test"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_gate_check_dual_gate_push_allows_with_all_sentinels() {
        // To pass the push/PR gate, we need:
        // 1. All phase sentinels pf-1..pf-5
        // 2. work_type set in session status
        // 3. All pipeline stage sentinels for that work_type
        //
        // This test uses a sentinel dir that doesn't have a real session
        // directory tree, so derive_session_dir returns a non-existent path,
        // read_work_type returns "", and the gate blocks on missing work_type.
        // To make it pass, we need to set up the full session structure.
        let dir = tempfile::tempdir().unwrap();

        // Sentinel dir: {dir}/sentinels/pathflow/{SID}/
        let sid = "ses-testpushgate1234567890";
        let sentinel_dir = dir
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();

        // Create cumulative phase sentinels pf-1..pf-5.
        for i in 1..=5 {
            sentinel::create_by_name(&sentinel_dir, &format!("pf-{i}")).unwrap();
        }
        // Create stage sentinels for HTFX pipeline: ws-dev, ws-rev, ws-qa.
        sentinel::create_by_name(&sentinel_dir, "ws-dev").unwrap();
        sentinel::create_by_name(&sentinel_dir, "ws-rev").unwrap();
        sentinel::create_by_name(&sentinel_dir, "ws-qa").unwrap();

        // Create session status with work_type.
        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","work_type":"HTFX"}"#,
        )
        .unwrap();

        let handler = make_gate_check(sentinel_dir);
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "git push origin feat/test"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(
            result.exit_code(),
            0,
            "push should be allowed with all sentinels + work_type"
        );
    }

    #[test]
    fn test_gate_check_push_blocks_missing_work_type() {
        // All phase sentinels present but work_type empty → BLOCK.
        let dir = tempfile::tempdir().unwrap();
        for i in 1..=5 {
            sentinel::create_by_name(dir.path(), &format!("pf-{i}")).unwrap();
        }
        let handler = make_gate_check(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "git push origin feat/test"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(
            result.exit_code(),
            2,
            "push should be blocked when work_type is missing"
        );
    }

    // -- GateCheck claim tests (AC #9) --

    #[test]
    fn test_gate_check_edit_acquires_claim_successfully() {
        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();
        let state_path = dir.path().join("state.loro");
        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            state_path.clone(),
            SessionId::new_unchecked("ses-claim-test-001"),
            dir.path().to_path_buf(),
        );
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "src/main.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-claim-test-001".into()),
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(
            result.exit_code(),
            EXIT_ADVISORY,
            "successful acquire should allow (exit 0)"
        );
        assert!(matches!(result, HookOutput::Allow));

        // Verify claim was persisted.
        use crate::coordination::Coordinator;
        use crate::coordination::loro::LoroCoordinator;
        let coord = LoroCoordinator::new(&state_path).unwrap();
        let claim = coord.check("src/main.rs");
        assert!(claim.is_some(), "claim should be persisted after acquire");
        assert_eq!(claim.unwrap().owner.as_str(), "ses-claim-test-001");
    }

    #[test]
    fn test_gate_check_edit_conflict_produces_advisory_warning() {
        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();
        let state_path = dir.path().join("state.loro");

        // Pre-populate a claim by session A.
        {
            use crate::coordination::Coordinator;
            use crate::coordination::loro::LoroCoordinator;
            let mut coord = LoroCoordinator::new(&state_path).unwrap();
            let sid_a = SessionId::new_unchecked("ses-session-a");
            coord.acquire("src/main.rs", &sid_a).unwrap();
            coord.persist().unwrap();
        }

        // Session B tries to edit the same file — fencing token validation
        // detects the stale claim before acquire() is reached.
        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            state_path,
            SessionId::new_unchecked("ses-session-b"),
            dir.path().to_path_buf(),
        );
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "src/main.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-session-b".into()),
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        // Advisory mode: exit 0 (Warn), not exit 2 (Block).
        assert_eq!(
            result.exit_code(),
            EXIT_ADVISORY,
            "conflict should warn, not block (in-scope auto-acquire)"
        );
        match &result {
            HookOutput::Warn { message } => {
                assert!(
                    message.contains("CLAIM CONFLICT"),
                    "warning should mention CLAIM CONFLICT, got: {message}"
                );
                assert!(
                    message.contains("ses-session-a"),
                    "warning should identify the owner"
                );
            }
            other => panic!("expected HookOutput::Warn, got {other:?}"),
        }
    }

    #[test]
    fn test_gate_check_release_then_reacquire() {
        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();
        let state_path = dir.path().join("state.loro");

        // Session A acquires a claim.
        {
            use crate::coordination::Coordinator;
            use crate::coordination::loro::LoroCoordinator;
            let mut coord = LoroCoordinator::new(&state_path).unwrap();
            let sid_a = SessionId::new_unchecked("ses-session-a");
            coord.acquire("src/main.rs", &sid_a).unwrap();
            coord.persist().unwrap();
        }

        // Release session A's claims.
        {
            use crate::coordination::Coordinator;
            use crate::coordination::claims::release_all;
            use crate::coordination::loro::LoroCoordinator;
            let mut coord = LoroCoordinator::new(&state_path).unwrap();
            let sid_a = SessionId::new_unchecked("ses-session-a");
            let released = release_all(&mut coord, &sid_a).unwrap();
            assert_eq!(released, 1, "should release one claim");
            coord.persist().unwrap();
        }

        // Session B should now be able to acquire without conflict.
        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            state_path,
            SessionId::new_unchecked("ses-session-b"),
            dir.path().to_path_buf(),
        );
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "src/main.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-session-b".into()),
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(
            result.exit_code(),
            EXIT_ADVISORY,
            "should allow after release"
        );
        assert!(matches!(result, HookOutput::Allow));
    }

    // -- GateCheck claim integration test (AC #10) --

    #[test]
    fn test_gate_check_integration_two_coordinators_shared_state() {
        use crate::coordination::Coordinator;
        use crate::coordination::loro::LoroCoordinator;

        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.loro");

        // Session A acquires and persists.
        let sid_a = SessionId::new_unchecked("ses-integ-a");
        {
            let mut coord_a = LoroCoordinator::new(&state_path).unwrap();
            coord_a.acquire("src/contested.rs", &sid_a).unwrap();
            coord_a.persist().unwrap();
        }

        // Session B loads the same state and attempts acquire — should get conflict.
        let sid_b = SessionId::new_unchecked("ses-integ-b");
        {
            let mut coord_b = LoroCoordinator::new(&state_path).unwrap();
            let result = coord_b.acquire("src/contested.rs", &sid_b);
            assert!(result.is_err(), "session B should get a conflict");
            let err = result.unwrap_err();
            assert!(
                err.to_string().contains("claim conflict"),
                "error should be a claim conflict: {err}"
            );
        }
    }

    // -- Fencing token validation tests (INF-TSK-023-005) --

    #[test]
    fn test_gate_check_same_session_reacquire_allowed() {
        // Same session re-acquiring the same file should succeed (token increments).
        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();
        let state_path = dir.path().join("state.loro");

        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            state_path.clone(),
            SessionId::new_unchecked("ses-reacquire"),
            dir.path().to_path_buf(),
        );

        // First acquire.
        let input1 = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "src/main.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-reacquire".into()),
            ..Default::default()
        };
        let r1 = handler.handle(input1).unwrap();
        assert!(
            matches!(r1, HookOutput::Allow),
            "first acquire should allow"
        );

        // Second acquire — same session should still be allowed with incremented token.
        let input2 = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "src/main.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-reacquire".into()),
            ..Default::default()
        };
        let r2 = handler.handle(input2).unwrap();
        assert!(
            matches!(r2, HookOutput::Allow),
            "same session reacquire should allow"
        );

        // Verify token incremented.
        use crate::coordination::Coordinator;
        use crate::coordination::loro::LoroCoordinator;
        let coord = LoroCoordinator::new(&state_path).unwrap();
        let claim = coord.check("src/main.rs").unwrap();
        assert!(
            claim.token.value() >= 2,
            "token should have incremented, got {}",
            claim.token.value()
        );
    }

    #[test]
    fn test_gate_check_stale_claim_warns_advisory() {
        // Session A claims a file, then session B supersedes it (via expired TTL
        // simulation). When session A tries again, validation detects the stale
        // claim and warns.
        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();
        let state_path = dir.path().join("state.loro");

        // Session A acquires with TTL=0 so its stored claim immediately expires.
        {
            use crate::coordination::Coordinator;
            use crate::coordination::loro::LoroCoordinator;
            let mut coord = LoroCoordinator::new(&state_path).unwrap();
            coord.set_ttl_secs(0); // A's claim stored with TTL=0 (immediately expired).
            let sid_a = SessionId::new_unchecked("ses-stale-a");
            coord.acquire("src/main.rs", &sid_a).unwrap();
            coord.persist().unwrap();
        }

        // Session B supersedes: A's claim is expired (TTL=0), so B acquires
        // successfully with default TTL (non-expired, visible to later checks).
        {
            use crate::coordination::Coordinator;
            use crate::coordination::loro::LoroCoordinator;
            let mut coord = LoroCoordinator::new(&state_path).unwrap();
            let sid_b = SessionId::new_unchecked("ses-stale-b");
            coord.acquire("src/main.rs", &sid_b).unwrap();
            coord.persist().unwrap();
        }

        // Session A tries to edit again — should detect stale claim.
        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            state_path,
            SessionId::new_unchecked("ses-stale-a"),
            dir.path().to_path_buf(),
        );
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "src/main.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-stale-a".into()),
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(
            result.exit_code(),
            EXIT_ADVISORY,
            "stale claim should warn (in-scope), not block"
        );
        match &result {
            HookOutput::Warn { message } => {
                assert!(
                    message.contains("CLAIM CONFLICT"),
                    "warning should mention CLAIM CONFLICT, got: {message}"
                );
                assert!(
                    message.contains("ses-stale-b"),
                    "warning should identify the superseding session, got: {message}"
                );
            }
            other => panic!("expected HookOutput::Warn for stale claim, got {other:?}"),
        }
    }

    #[test]
    fn test_gate_check_fencing_token_concurrency() {
        // Two sessions race on the same state.loro file via persist/load.
        // Session A acquires, persists. Session B loads, acquires (superseding
        // via expired TTL). Session A reloads and detects the stale claim.
        use crate::coordination::Coordinator;
        use crate::coordination::loro::LoroCoordinator;

        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.loro");

        let sid_a = SessionId::new_unchecked("ses-race-a");
        let sid_b = SessionId::new_unchecked("ses-race-b");

        // Session A acquires with TTL=0 (immediately expired) and persists.
        let token_a = {
            let mut coord = LoroCoordinator::new(&state_path).unwrap();
            coord.set_ttl_secs(0); // A's claim stored with TTL=0 (immediately expired).
            let t = coord.acquire("src/contested.rs", &sid_a).unwrap();
            coord.persist().unwrap();
            t
        };

        // Session B loads same state — A's claim is expired (stored TTL=0),
        // so B acquires successfully with default TTL (visible to later checks).
        let token_b = {
            let mut coord = LoroCoordinator::new(&state_path).unwrap();
            let t = coord.acquire("src/contested.rs", &sid_b).unwrap();
            coord.persist().unwrap();
            t
        };

        // Token B must be higher than token A (monotonically increasing).
        assert!(
            token_b.value() > token_a.value(),
            "token_b ({}) should be greater than token_a ({})",
            token_b.value(),
            token_a.value()
        );

        // Session A reloads and checks — should detect mismatch.
        {
            let coord = LoroCoordinator::new(&state_path).unwrap();
            let claim = coord.check("src/contested.rs").unwrap();
            assert_eq!(
                claim.owner.as_str(),
                "ses-race-b",
                "session B should now own the claim"
            );
            assert_ne!(
                claim.token, token_a,
                "stored token should differ from session A's original"
            );

            // Validate using the claims module.
            use crate::coordination::claims::validate_token;
            let result = validate_token(&coord, "src/contested.rs", &sid_a, token_a);
            assert!(
                result.is_err(),
                "session A's stale token should fail validation"
            );
        }
    }

    // -- EXIT_ADVISORY / EXIT_BLOCKING constant tests (AC #4) --

    #[test]
    fn test_exit_advisory_constant() {
        assert_eq!(EXIT_ADVISORY, 0, "advisory should be exit 0 (Warn)");
    }

    #[test]
    fn test_exit_blocking_constant() {
        assert_eq!(EXIT_BLOCKING, 2, "blocking should be exit 2 (Block)");
    }

    // -- TeamGuard tests --

    #[test]
    fn test_team_guard_allows_non_team_delete() {
        let dir = tempfile::tempdir().unwrap();
        let handler = TeamGuard::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "ls"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_team_guard_blocks_team_delete_active_no_pf6() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("session");
        let sentinel_dir = dir.path().join("sentinels");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","team_name":"test-team"}"#,
        )
        .unwrap();

        let handler = TeamGuard::new(session_dir, sentinel_dir);
        let input = HookInput {
            tool_name: Some("TeamDelete".into()),
            tool_input: Some(serde_json::json!({})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_team_guard_allows_team_delete_with_pf6() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("session");
        let sentinel_dir = dir.path().join("sentinels");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","team_name":"test-team"}"#,
        )
        .unwrap();
        sentinel::create_by_name(&sentinel_dir, "pf-6").unwrap();

        let handler = TeamGuard::new(session_dir, sentinel_dir);
        let input = HookInput {
            tool_name: Some("TeamDelete".into()),
            tool_input: Some(serde_json::json!({})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_team_guard_allows_when_no_active_session() {
        let dir = tempfile::tempdir().unwrap();
        let handler = TeamGuard::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("TeamDelete".into()),
            tool_input: Some(serde_json::json!({})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    // -- EditWriteGuard tests --

    #[test]
    fn test_protection_guard_blocks_bash_cp_to_protected() {
        let dir = tempfile::tempdir().unwrap();
        // Create enforcement policy with .claude/CLAUDE.md as critical.
        let config_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("enforcement-policy.json"),
            r#"{"protected_resources":{"critical":[".claude/CLAUDE.md",".claude/settings.json"],"high":[".claude/hooks/**"],"moderate":[]}}"#,
        ).unwrap();
        let policy = EnforcementPolicy::load(dir.path());
        let handler = ProtectionGuard::new_with_worktree(
            policy,
            dir.path().to_path_buf(),
            None,
        );
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({
                "command": "cp /tmp/claude/codeflow/managed/protected-edits/CLAUDE.md .claude/CLAUDE.md"
            })),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test123".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "cp to .claude/CLAUDE.md should be blocked"
        );
    }

    #[test]
    fn test_protection_guard_allows_bash_cp_to_normal_file() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::load(dir.path());
        let handler = ProtectionGuard::new_with_worktree(
            policy,
            dir.path().to_path_buf(),
            None,
        );
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({
                "command": "cp /tmp/source.rs codeflow-cli/core/src/some_file.rs"
            })),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test123".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Allow),
            "cp to normal file should be allowed"
        );
    }

    #[test]
    fn test_protection_guard_allows_bash_non_cp() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::load(dir.path());
        let handler = ProtectionGuard::new_with_worktree(
            policy,
            dir.path().to_path_buf(),
            None,
        );
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({
                "command": "ls -la .claude/"
            })),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test123".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Allow),
            "non-cp bash command should be allowed"
        );
    }

    #[test]
    fn test_protection_guard_allows_bash_cp_to_staging() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::load(dir.path());
        let handler = ProtectionGuard::new_with_worktree(
            policy,
            dir.path().to_path_buf(),
            None,
        );
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({
                "command": "cp .claude/CLAUDE.md /tmp/claude/codeflow/managed/protected-edits/CLAUDE.md"
            })),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test123".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Allow),
            "cp TO staging area should be allowed"
        );
    }

        #[test]
    fn test_edit_write_guard_allows_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = EditWriteGuard::new(dir.path().to_path_buf(), policy, "feat/test".into());
        let input = HookInput {
            tool_name: Some("Write".into()),
            tool_input: Some(serde_json::json!({"file_path": "/tmp/claude/test.txt"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_edit_write_guard_blocks_protected_branch() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = EditWriteGuard::new(dir.path().to_path_buf(), policy, "main".into());
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "src/lib.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_edit_write_guard_blocks_blocked_directory() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = EditWriteGuard::new(dir.path().to_path_buf(), policy, "feat/test".into());
        let input = HookInput {
            tool_name: Some("Write".into()),
            tool_input: Some(serde_json::json!({"file_path": ".git/config"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_edit_write_guard_warns_dangerous_ext() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = EditWriteGuard::new(dir.path().to_path_buf(), policy, "feat/test".into());
        // Use a relative path so it joins with project_dir, avoiding macOS
        // symlink canonicalization mismatches (/var vs /private/var).
        let input = HookInput {
            tool_name: Some("Write".into()),
            tool_input: Some(serde_json::json!({"file_path": "test.pem"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Warn { .. }));
        assert_eq!(result.exit_code(), 0);
    }

    // -- GhPrGuard tests --

    struct MockResolver {
        branch: Option<String>,
    }

    impl PRResolver for MockResolver {
        fn resolve_target_branch(&self, _pr_number: &str) -> Option<String> {
            self.branch.clone()
        }
    }

    #[test]
    fn test_gh_pr_guard_blocks_protected_branch() {
        let resolver = MockResolver {
            branch: Some("main".into()),
        };
        let handler = GhPrGuard::with_resolver(vec!["main".into(), "master".into()], resolver);
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "gh pr merge 123"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_gh_pr_guard_allows_non_protected_branch() {
        let resolver = MockResolver {
            branch: Some("feat/test".into()),
        };
        let handler = GhPrGuard::with_resolver(vec!["main".into(), "master".into()], resolver);
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "gh pr merge 123"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_gh_pr_guard_allows_non_bash() {
        let resolver = MockResolver {
            branch: Some("main".into()),
        };
        let handler = GhPrGuard::with_resolver(vec!["main".into()], resolver);
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_gh_pr_guard_glob_protected_branch() {
        let resolver = MockResolver {
            branch: Some("release/v1.0".into()),
        };
        let handler = GhPrGuard::with_resolver(vec!["release/*".into()], resolver);
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "gh pr merge 42"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    // -- ProtectionGuard tests --

    #[test]
    fn test_protection_guard_blocks_critical() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = ProtectionGuard::new(policy, dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": ".claude/settings.json"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_protection_guard_blocks_high_glob() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = ProtectionGuard::new(policy, dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Write".into()),
            tool_input: Some(
                serde_json::json!({"file_path": ".codeflow/config/enforcement/test.json"}),
            ),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_protection_guard_warns_moderate() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = ProtectionGuard::new(policy, dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "project/mission.md"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Warn { .. }));
    }

    #[test]
    fn test_protection_guard_allows_unprotected() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = ProtectionGuard::new(policy, dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "src/main.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_protection_guard_allows_staging_path() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = ProtectionGuard::new(policy, dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Write".into()),
            tool_input: Some(
                serde_json::json!({"file_path": "/tmp/claude/codeflow/managed/protected-edits/.claude/settings.json"}),
            ),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    // -- WebFetchGuard tests --

    #[test]
    fn test_webfetch_blocks_file_url() {
        let handler = WebFetchGuard::new(vec![], vec![], vec![]);
        let input = HookInput {
            tool_name: Some("WebFetch".into()),
            tool_input: Some(serde_json::json!({"url": "file:///etc/passwd"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_webfetch_blocks_data_url() {
        let handler = WebFetchGuard::new(vec![], vec![], vec![]);
        let input = HookInput {
            tool_name: Some("WebFetch".into()),
            tool_input: Some(serde_json::json!({"url": "data:text/html,<h1>test</h1>"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_webfetch_allows_trusted_domain() {
        let handler =
            WebFetchGuard::new(vec!["github.com".into()], vec!["localhost".into()], vec![]);
        let input = HookInput {
            tool_name: Some("WebFetch".into()),
            tool_input: Some(serde_json::json!({"url": "https://api.github.com/repos"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_webfetch_blocks_localhost() {
        let handler = WebFetchGuard::new(vec![], vec!["localhost".into()], vec![]);
        let input = HookInput {
            tool_name: Some("WebFetch".into()),
            tool_input: Some(serde_json::json!({"url": "http://localhost:8080/api"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_webfetch_blocks_private_ip() {
        let handler = WebFetchGuard::new(vec![], vec![], vec!["192.168.*".into()]);
        let input = HookInput {
            tool_name: Some("WebFetch".into()),
            tool_input: Some(serde_json::json!({"url": "http://192.168.1.1/admin"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_webfetch_untrusted_domain_warns() {
        let handler = WebFetchGuard::new(vec!["github.com".into()], vec![], vec![]);
        let input = HookInput {
            tool_name: Some("WebFetch".into()),
            tool_input: Some(serde_json::json!({"url": "https://example.com/page"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Warn { .. }));
    }

    #[test]
    fn test_webfetch_allows_non_network_tool() {
        let handler = WebFetchGuard::new(vec![], vec![], vec![]);
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    // -- Shared helper tests --

    #[test]
    fn test_is_protected_branch_exact() {
        let branches = vec!["main".into(), "master".into()];
        assert!(is_protected_branch("main", &branches));
        assert!(!is_protected_branch("feat/test", &branches));
    }

    #[test]
    fn test_is_protected_branch_glob() {
        let branches = vec!["release/*".into()];
        assert!(is_protected_branch("release/v1.0", &branches));
        assert!(!is_protected_branch("feat/release", &branches));
    }

    #[test]
    fn test_matches_glob_pattern_exact() {
        assert!(matches_glob_pattern(
            ".claude/settings.json",
            ".claude/settings.json"
        ));
    }

    #[test]
    fn test_matches_glob_pattern_recursive() {
        assert!(matches_glob_pattern(
            ".codeflow/config/enforcement/test.json",
            ".codeflow/config/**"
        ));
    }

    #[test]
    fn test_match_domain_pattern_exact() {
        assert!(match_domain_pattern("localhost", "localhost"));
    }

    #[test]
    fn test_match_domain_pattern_wildcard_suffix() {
        assert!(match_domain_pattern("test.local", "*.local"));
    }

    #[test]
    fn test_match_domain_pattern_wildcard_prefix() {
        assert!(match_domain_pattern("internal.corp.com", "internal.*"));
    }

    // -- EnforcementPolicy tests --

    #[test]
    fn test_enforcement_policy_defaults_has_protected_paths() {
        let policy = EnforcementPolicy::defaults();
        let paths = policy.all_protected_paths();
        assert!(!paths.is_empty());
        assert!(paths.iter().any(|p| p.contains("settings.json")));
    }

    #[test]
    fn test_enforcement_policy_protected_branch_list_defaults() {
        let mut policy = EnforcementPolicy::defaults();
        policy.protected_branches = vec![];
        let branches = policy.protected_branch_list();
        assert!(branches.contains(&"main".to_string()));
        assert!(branches.contains(&"master".to_string()));
        assert!(branches.contains(&"production".to_string()));
    }

    #[test]
    fn test_enforcement_policy_protected_branch_list_custom() {
        let mut policy = EnforcementPolicy::defaults();
        policy.protected_branches = vec!["develop".into()];
        let branches = policy.protected_branch_list();
        assert_eq!(branches, vec!["develop".to_string()]);
    }

    #[test]
    fn test_enforcement_policy_managed_tmp_folders_defaults() {
        let policy = EnforcementPolicy::defaults();
        let folders = policy.managed_tmp_folders("myproject");
        assert!(!folders.is_empty());
        assert!(folders.iter().any(|f| f.contains("myproject")));
    }

    #[test]
    fn test_enforcement_policy_managed_tmp_folders_custom() {
        let mut policy = EnforcementPolicy::defaults();
        policy.managed_tmp.protected_folders = vec!["/tmp/claude/${CF_PROJECT_ROOT}/custom".into()];
        let folders = policy.managed_tmp_folders("testproj");
        assert_eq!(folders, vec!["/tmp/claude/testproj/custom"]);
    }

    #[test]
    fn test_enforcement_policy_state_folder_defaults() {
        let policy = EnforcementPolicy::defaults();
        let path = policy.state_folder_path("myproject");
        assert!(path.contains("myproject"));
    }

    #[test]
    fn test_enforcement_policy_state_folder_custom() {
        let mut policy = EnforcementPolicy::defaults();
        policy.managed_tmp.state_folder = "/tmp/claude/${CF_PROJECT_ROOT}/state".into();
        let path = policy.state_folder_path("testproj");
        assert_eq!(path, "/tmp/claude/testproj/state");
    }

    #[test]
    fn test_enforcement_policy_load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("enforcement-policy.json"),
            r#"{"protected_branches": ["custom-branch"]}"#,
        )
        .unwrap();
        let policy = EnforcementPolicy::load(dir.path());
        assert_eq!(
            policy.protected_branch_list(),
            vec!["custom-branch".to_string()]
        );
    }

    #[test]
    fn test_enforcement_policy_load_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::load(dir.path());
        // Should return default.
        assert!(policy.protected_branch_list().contains(&"main".to_string()));
    }

    // -- TeamGuard: Teammate cleanup operation --

    #[test]
    fn test_team_guard_blocks_teammate_cleanup_active() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("session");
        let sentinel_dir = dir.path().join("sentinels");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","team_name":"test-team"}"#,
        )
        .unwrap();

        let handler = TeamGuard::new(session_dir, sentinel_dir);
        let input = HookInput {
            tool_name: Some("Teammate".into()),
            tool_input: Some(serde_json::json!({"operation": "cleanup"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_team_guard_allows_teammate_non_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("session");
        let sentinel_dir = dir.path().join("sentinels");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","team_name":"test-team"}"#,
        )
        .unwrap();

        let handler = TeamGuard::new(session_dir, sentinel_dir);
        let input = HookInput {
            tool_name: Some("Teammate".into()),
            tool_input: Some(serde_json::json!({"operation": "spawn"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    // -- EditWriteGuard: additional paths --

    #[test]
    fn test_edit_write_guard_allows_non_edit_write() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = EditWriteGuard::new(dir.path().to_path_buf(), policy, "feat/test".into());
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "ls"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_edit_write_guard_allows_empty_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = EditWriteGuard::new(dir.path().to_path_buf(), policy, "feat/test".into());
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": ""})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_edit_write_guard_warns_binary_ext() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = EditWriteGuard::new(dir.path().to_path_buf(), policy, "feat/test".into());
        let input = HookInput {
            tool_name: Some("Write".into()),
            tool_input: Some(serde_json::json!({"file_path": "output.exe"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Warn { .. }));
    }

    #[test]
    fn test_edit_write_guard_warns_archive_ext() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = EditWriteGuard::new(dir.path().to_path_buf(), policy, "feat/test".into());
        let input = HookInput {
            tool_name: Some("Write".into()),
            tool_input: Some(serde_json::json!({"file_path": "package.tar.gz"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Warn { .. }));
    }

    #[test]
    fn test_edit_write_guard_blocks_outside_project() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = EditWriteGuard::new(dir.path().to_path_buf(), policy, "feat/test".into());
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "/etc/hosts"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_edit_write_guard_name_and_events() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = EditWriteGuard::new(dir.path().to_path_buf(), policy, "feat/test".into());
        assert_eq!(handler.name(), "edit-write-guard");
        assert_eq!(handler.events(), &[HookEvent::PreToolUse]);
    }

    // -- GhPrGuard: additional edge cases --

    #[test]
    fn test_gh_pr_guard_resolver_returns_none() {
        let resolver = MockResolver { branch: None };
        let handler = GhPrGuard::with_resolver(vec!["main".into()], resolver);
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "gh pr merge 999"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        // Can't resolve → allow through.
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_gh_pr_guard_non_merge_command() {
        let resolver = MockResolver {
            branch: Some("main".into()),
        };
        let handler = GhPrGuard::with_resolver(vec!["main".into()], resolver);
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "gh pr list"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_gh_pr_guard_name_and_events() {
        let handler = GhPrGuard::new(vec!["main".into()]);
        assert_eq!(handler.name(), "gh-pr-guard");
        assert_eq!(handler.events(), &[HookEvent::PreToolUse]);
    }

    // -- GateCheck: name and events --

    #[test]
    fn test_gate_check_name_and_events() {
        let dir = tempfile::tempdir().unwrap();
        let handler = make_gate_check(dir.path().to_path_buf());
        assert_eq!(handler.name(), "gate-check");
        assert_eq!(handler.events(), &[HookEvent::PreToolUse]);
    }

    // -- ProtectionGuard: name, events, edge cases --

    #[test]
    fn test_protection_guard_name_and_events() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = ProtectionGuard::new(policy, dir.path().to_path_buf());
        assert_eq!(handler.name(), "protection-guard");
        assert_eq!(handler.events(), &[HookEvent::PreToolUse]);
    }

    #[test]
    fn test_protection_guard_ignores_non_edit_write() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = ProtectionGuard::new(policy, dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "ls"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_protection_guard_empty_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = ProtectionGuard::new(policy, dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": ""})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    // -- ProtectionGuard worktree-scoped staging path tests --

    #[test]
    fn test_block_verdict_staging_path_includes_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = ProtectionGuard::new_with_worktree(
            policy,
            dir.path().to_path_buf(),
            Some("worktree-ses-abc".to_string()),
        );

        let staging = handler.staging_dir();
        let dir_name = dir.path().file_name().unwrap().to_string_lossy();
        assert_eq!(
            staging,
            format!("/tmp/claude/{dir_name}/worktree-ses-abc/managed/protected-edits")
        );
    }

    #[test]
    fn test_block_verdict_staging_path_no_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = ProtectionGuard::new_with_worktree(policy, dir.path().to_path_buf(), None);

        let staging = handler.staging_dir();
        let dir_name = dir.path().file_name().unwrap().to_string_lossy();
        assert_eq!(
            staging,
            format!("/tmp/claude/{dir_name}/managed/protected-edits")
        );
    }

    #[test]
    fn test_is_staging_path_worktree_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = ProtectionGuard::new_with_worktree(
            policy,
            dir.path().to_path_buf(),
            Some("worktree-ses-xyz".to_string()),
        );

        let dir_name = dir.path().file_name().unwrap().to_string_lossy();

        // Exact match with worktree-scoped path.
        assert!(handler.is_staging_path(&format!(
            "/tmp/claude/{dir_name}/worktree-ses-xyz/managed/protected-edits/some/file.rs"
        )));

        // Generic fallback (contains pattern) — still allowed.
        assert!(handler.is_staging_path(
            "/tmp/claude/other-project/worktree-ses-other/managed/protected-edits/file.rs"
        ));

        // Non-staging path — rejected.
        assert!(!handler.is_staging_path("/tmp/claude/other/file.rs"));
    }

    // -- WebFetchGuard: Bash tool path, from_policy, name/events --

    #[test]
    fn test_webfetch_bash_curl_blocks_private_ip() {
        let handler = WebFetchGuard::new(vec![], vec![], vec!["10.*".into()]);
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "curl http://10.0.0.1/api/admin"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_webfetch_bash_no_network_tool_allows() {
        let handler = WebFetchGuard::new(vec![], vec!["evil.com".into()], vec![]);
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "echo hello"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_webfetch_bash_safe_url_allows() {
        let handler = WebFetchGuard::new(vec!["safe.com".into()], vec![], vec![]);
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "curl https://safe.com/api"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_webfetch_websearch_blocks_file_url() {
        let handler = WebFetchGuard::new(vec![], vec![], vec![]);
        let input = HookInput {
            tool_name: Some("WebSearch".into()),
            tool_input: Some(serde_json::json!({"url": "file:///etc/shadow"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_webfetch_empty_url_allows() {
        let handler = WebFetchGuard::new(vec![], vec![], vec![]);
        let input = HookInput {
            tool_name: Some("WebFetch".into()),
            tool_input: Some(serde_json::json!({"url": ""})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_webfetch_name_and_events() {
        let handler = WebFetchGuard::new(vec![], vec![], vec![]);
        assert_eq!(handler.name(), "webfetch-guard");
        assert_eq!(handler.events(), &[HookEvent::PreToolUse]);
    }

    #[test]
    fn test_webfetch_from_policy() {
        let dir = tempfile::tempdir().unwrap();
        let domains_path = dir.path().join("trusted-domains.txt");
        std::fs::write(&domains_path, "github.com\n# comment\nexample.com\n").unwrap();
        let policy = EnforcementPolicy::defaults();
        let handler = WebFetchGuard::from_policy(&policy, &domains_path);
        assert!(handler.trusted_domains.contains(&"github.com".to_string()));
        assert!(handler.trusted_domains.contains(&"example.com".to_string()));
        assert!(!handler.trusted_domains.iter().any(|d| d.starts_with('#')));
    }

    #[test]
    fn test_webfetch_from_policy_missing_file() {
        let policy = EnforcementPolicy::defaults();
        let handler = WebFetchGuard::from_policy(&policy, Path::new("/tmp/nonexistent-file.txt"));
        assert!(handler.trusted_domains.is_empty());
    }

    // -- Private IP detection --

    #[test]
    fn test_webfetch_blocks_loopback_ip() {
        let handler = WebFetchGuard::new(vec![], vec![], vec![]);
        let input = HookInput {
            tool_name: Some("WebFetch".into()),
            tool_input: Some(serde_json::json!({"url": "http://127.0.0.1:3000/api"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert_eq!(result.exit_code(), 2);
    }

    // -- Helper function tests --

    #[test]
    fn test_extract_urls() {
        let urls = extract_urls("curl https://api.example.com/data -o output.json");
        assert_eq!(urls, vec!["https://api.example.com/data"]);
    }

    #[test]
    fn test_extract_urls_multiple() {
        let urls = extract_urls("wget http://a.com/1 http://b.com/2");
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn test_extract_urls_quoted() {
        let urls = extract_urls(r#"curl "https://api.example.com/data""#);
        assert_eq!(urls, vec!["https://api.example.com/data"]);
    }

    #[test]
    fn test_extract_urls_none() {
        let urls = extract_urls("echo hello world");
        assert!(urls.is_empty());
    }

    #[test]
    fn test_contains_network_tool_true() {
        assert!(contains_network_tool("curl https://example.com"));
        assert!(contains_network_tool("wget http://file.tar.gz"));
        assert!(contains_network_tool("ssh user@host"));
        assert!(contains_network_tool("nc -l 8080"));
    }

    #[test]
    fn test_contains_network_tool_false() {
        assert!(!contains_network_tool("echo hello"));
        assert!(!contains_network_tool("ls -la"));
    }

    #[test]
    fn test_load_trusted_domains() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("domains.txt");
        std::fs::write(&path, "github.com\n\n# comment\nexample.com\n  \n").unwrap();
        let domains = load_trusted_domains(&path);
        assert_eq!(domains, vec!["github.com", "example.com"]);
    }

    #[test]
    fn test_load_trusted_domains_missing() {
        let domains = load_trusted_domains(Path::new("/tmp/nonexistent"));
        assert!(domains.is_empty());
    }

    #[test]
    fn test_match_ip_pattern_exact() {
        assert!(match_ip_pattern("192.168.1.1", "192.168.1.1"));
        assert!(!match_ip_pattern("192.168.1.2", "192.168.1.1"));
    }

    #[test]
    fn test_match_ip_pattern_wildcard() {
        assert!(match_ip_pattern("10.0.0.1", "10.*"));
        assert!(!match_ip_pattern("192.168.1.1", "10.*"));
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            extract_domain("https://api.github.com/repos"),
            Some("api.github.com".into())
        );
        assert_eq!(extract_domain("not-a-url"), None);
    }

    #[test]
    fn test_capitalize_first() {
        assert_eq!(capitalize_first("hello"), "Hello");
        assert_eq!(capitalize_first(""), "");
        assert_eq!(capitalize_first("a"), "A");
    }

    #[test]
    fn test_matches_glob_pattern_single_star() {
        assert!(matches_glob_pattern("src/main.rs", "src/*.rs"));
        assert!(!matches_glob_pattern("src/sub/main.rs", "src/*.rs"));
    }

    #[test]
    fn test_match_domain_pattern_no_match() {
        assert!(!match_domain_pattern("example.com", "other.com"));
        assert!(!match_domain_pattern("example.com", "*.other"));
    }

    #[test]
    fn test_match_blocked_dir() {
        assert!(match_blocked_dir(".git/config", ".git"));
        assert!(match_blocked_dir("path/to/.git/hooks", ".git"));
        assert!(match_blocked_dir("path/to/node_modules", "node_modules"));
        assert!(!match_blocked_dir("gitconfig", ".git"));
    }

    // -- scope_policy tests (AC #1-5, #17) --

    #[test]
    fn test_is_in_scope_exact_match() {
        let scope = vec!["src/main.rs".to_string()];
        assert!(GateCheck::is_in_scope("src/main.rs", &scope));
        assert!(!GateCheck::is_in_scope("src/lib.rs", &scope));
    }

    #[test]
    fn test_is_in_scope_directory_prefix() {
        let scope = vec!["codeflow-cli/core/src".to_string()];
        assert!(GateCheck::is_in_scope(
            "codeflow-cli/core/src/hooks/pre_tool_use.rs",
            &scope
        ));
        assert!(!GateCheck::is_in_scope(
            "codeflow-cli/cli/src/main.rs",
            &scope
        ));
    }

    #[test]
    fn test_is_in_scope_empty_scope_allows_all() {
        assert!(GateCheck::is_in_scope("anything.rs", &[]));
    }

    #[test]
    fn test_scope_policy_permissive_allows_all() {
        use crate::session::active_task::{ActiveTask, set_active_task};
        use crate::types::TaskId;

        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();

        let runtime_dir = dir.path().join(".state").join("runtime");
        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-perm"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: None,
            status: None,
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
            scope_policy: Some("permissive".to_string()),
            file_scope: Some(vec!["src/main.rs".to_string()]),
        };
        set_active_task(&runtime_dir, &task).unwrap();

        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            dir.path().join("state.loro"),
            SessionId::new_unchecked("ses-perm-test"),
            dir.path().to_path_buf(),
        );
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "out/of/scope.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-perm-test".into()),
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Allow),
            "permissive should allow out-of-scope edits"
        );
    }

    #[test]
    fn test_scope_policy_hard_blocks_out_of_scope() {
        use crate::session::active_task::{ActiveTask, set_active_task};
        use crate::types::TaskId;

        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();

        let runtime_dir = dir.path().join(".state").join("runtime");
        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-hard"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: None,
            status: None,
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
            scope_policy: Some("hard".to_string()),
            file_scope: Some(vec!["src/main.rs".to_string()]),
        };
        set_active_task(&runtime_dir, &task).unwrap();

        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            dir.path().join("state.loro"),
            SessionId::new_unchecked("ses-hard-test"),
            dir.path().to_path_buf(),
        );
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "out/of/scope.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-hard-test".into()),
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "hard mode should block out-of-scope edits"
        );
    }

    #[test]
    fn test_scope_policy_hard_allows_in_scope() {
        use crate::session::active_task::{ActiveTask, set_active_task};
        use crate::types::TaskId;

        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();

        let runtime_dir = dir.path().join(".state").join("runtime");
        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-hard-in"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: None,
            status: None,
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
            scope_policy: Some("hard".to_string()),
            file_scope: Some(vec!["src/main.rs".to_string()]),
        };
        set_active_task(&runtime_dir, &task).unwrap();

        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            dir.path().join("state.loro"),
            SessionId::new_unchecked("ses-hard-in-test"),
            dir.path().to_path_buf(),
        );
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "src/main.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-hard-in-test".into()),
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Allow),
            "hard mode should allow in-scope edits"
        );
    }

    #[test]
    fn test_scope_policy_soft_allows_in_scope() {
        use crate::session::active_task::{ActiveTask, set_active_task};
        use crate::types::TaskId;

        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();

        let runtime_dir = dir.path().join(".state").join("runtime");
        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-soft-in"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: None,
            status: None,
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
            scope_policy: Some("soft".to_string()),
            file_scope: Some(vec!["src/main.rs".to_string()]),
        };
        set_active_task(&runtime_dir, &task).unwrap();

        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            dir.path().join("state.loro"),
            SessionId::new_unchecked("ses-soft-in-test"),
            dir.path().to_path_buf(),
        );
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "src/main.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-soft-in-test".into()),
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Allow),
            "soft mode should allow in-scope edits"
        );
    }

    #[test]
    fn test_scope_policy_soft_out_of_scope_unclaimed_allows() {
        use crate::session::active_task::{ActiveTask, set_active_task};
        use crate::types::TaskId;

        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();

        let runtime_dir = dir.path().join(".state").join("runtime");
        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-soft-expand"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: None,
            status: None,
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
            scope_policy: Some("soft".to_string()),
            file_scope: Some(vec!["src/main.rs".to_string()]),
        };
        set_active_task(&runtime_dir, &task).unwrap();

        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            dir.path().join("state.loro"),
            SessionId::new_unchecked("ses-soft-expand"),
            dir.path().to_path_buf(),
        );
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "out/of/scope.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-soft-expand".into()),
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Allow),
            "soft mode should allow out-of-scope edits when unclaimed (scope expansion)"
        );
    }

    #[test]
    fn test_scope_policy_soft_out_of_scope_conflict_blocks() {
        use crate::coordination::Coordinator;
        use crate::coordination::loro::LoroCoordinator;
        use crate::session::active_task::{ActiveTask, set_active_task};
        use crate::types::TaskId;

        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();

        let runtime_dir = dir.path().join(".state").join("runtime");
        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-soft-conflict"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: None,
            status: None,
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
            scope_policy: Some("soft".to_string()),
            file_scope: Some(vec!["src/main.rs".to_string()]),
        };
        set_active_task(&runtime_dir, &task).unwrap();

        // Pre-claim the file by another session.
        let state_path = dir.path().join("state.loro");
        {
            let mut coord = LoroCoordinator::in_memory();
            let other = SessionId::new_unchecked("ses-other-holder");
            coord.acquire("out/of/scope.rs", &other).unwrap();
            let bytes = coord.export_bytes().unwrap();
            std::fs::write(&state_path, bytes).unwrap();
        }

        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            state_path,
            SessionId::new_unchecked("ses-soft-conflict"),
            dir.path().to_path_buf(),
        );
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "out/of/scope.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-soft-conflict".into()),
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "soft mode should block out-of-scope edits when claimed by another session"
        );
    }

    #[test]
    fn test_scope_policy_default_is_soft() {
        use crate::session::active_task::{ActiveTask, set_active_task};
        use crate::types::TaskId;

        let dir = tempfile::tempdir().unwrap();
        sentinel::create_by_name(dir.path(), "pf-3").unwrap();

        let runtime_dir = dir.path().join(".state").join("runtime");
        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-default"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: None,
            status: None,
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
            scope_policy: None, // No policy specified = default to soft.
            file_scope: Some(vec!["src/main.rs".to_string()]),
        };
        set_active_task(&runtime_dir, &task).unwrap();

        let handler = GateCheck::new(
            dir.path().to_path_buf(),
            dir.path().join("state.loro"),
            SessionId::new_unchecked("ses-default"),
            dir.path().to_path_buf(),
        );
        // Edit in-scope file: should allow (soft default behavior).
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "src/main.rs"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-default".into()),
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Allow),
            "default scope_policy (soft) should allow in-scope edits"
        );
    }
}
