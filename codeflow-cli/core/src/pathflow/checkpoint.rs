//! `PathFlow` phase checkpoint state management.
//!
//! Tracks task registration, completion, and skip status per phase.
//! Creates phase sentinels automatically when all expected tasks complete.
//! Uses file locking (`fs2`) for atomic read-modify-write.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::error::PathflowError;

/// Checkpoint state for a single phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseCheckpoint {
    pub expected: Vec<String>,
    #[serde(default)]
    pub conditions: HashMap<String, String>,
    #[serde(default)]
    pub registered: HashMap<String, String>,
    #[serde(default)]
    pub completed: HashMap<String, String>,
    #[serde(default)]
    pub skipped: HashMap<String, String>,
    #[serde(default)]
    pub sentinel_created: bool,
}

/// The full checkpoint file containing session context and per-phase state.
///
/// Serialized as a flat JSON object with `"context"` alongside `"PF1"`, `"PF2"`, etc.
#[derive(Debug, Clone, Default)]
pub struct CheckpointFile {
    pub context: HashMap<String, String>,
    pub phases: HashMap<String, PhaseCheckpoint>,
}

impl Serialize for CheckpointFile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.phases.len() + 1))?;
        if !self.context.is_empty() {
            map.serialize_entry("context", &self.context)?;
        }
        for (k, v) in &self.phases {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for CheckpointFile {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw: HashMap<String, serde_json::Value> = HashMap::deserialize(deserializer)?;
        let mut cf = CheckpointFile::default();

        for (k, v) in raw {
            if k == "context" {
                cf.context = serde_json::from_value(v).unwrap_or_default();
            } else if let Ok(pc) = serde_json::from_value::<PhaseCheckpoint>(v) {
                cf.phases.insert(k, pc);
            }
        }
        Ok(cf)
    }
}

/// Time source for checkpoint timestamps. Override in tests.
pub type NowFn = fn() -> String;

fn default_now() -> String {
    crate::util::now_rfc3339()
}

/// `PathFlow` checkpoint manager.
pub struct Checkpoint {
    now: NowFn,
}

impl Default for Checkpoint {
    fn default() -> Self {
        Self::new()
    }
}

impl Checkpoint {
    /// Create a checkpoint with the default time source.
    #[must_use]
    pub fn new() -> Self {
        Self { now: default_now }
    }

    /// Create a checkpoint with a custom time source (for testing).
    #[must_use]
    pub fn with_now(now: NowFn) -> Self {
        Self { now }
    }

    /// Initialize all phases from `pathflow-config.json`.
    ///
    /// Idempotent: existing phases are not overwritten.
    ///
    /// # Errors
    ///
    /// Returns `PathflowError` on I/O, parse, or lock errors.
    pub fn init_all_phases(
        &self,
        checkpoint_path: &Path,
        config_path: &Path,
    ) -> Result<(), PathflowError> {
        let config_data = fs::read_to_string(config_path).map_err(PathflowError::Io)?;

        let config: serde_json::Value = serde_json::from_str(&config_data)?;
        let phases = config
            .get("phases")
            .and_then(|v| v.as_object())
            .ok_or_else(|| PathflowError::Sentinel("no phases in config".to_string()))?;

        self.with_lock(checkpoint_path, |cf| {
            for (config_key, phase_val) in phases {
                // Extract phase prefix: PF1-INIT -> PF1
                let phase_id = config_key.split('-').next().unwrap_or(config_key);

                if cf.phases.contains_key(phase_id) {
                    continue; // Idempotent
                }

                let required_tasks: Vec<String> = phase_val
                    .get("required_tasks")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                let mut conditions = HashMap::new();
                if let Some(tasks) = phase_val.get("tasks").and_then(|v| v.as_array()) {
                    for task in tasks {
                        if let (Some(id), Some(cond)) = (
                            task.get("id").and_then(|v| v.as_str()),
                            task.get("condition").and_then(|v| v.as_str()),
                        ) {
                            if !cond.is_empty() {
                                conditions.insert(id.to_string(), cond.to_string());
                            }
                        }
                    }
                }

                cf.phases.insert(
                    phase_id.to_string(),
                    PhaseCheckpoint {
                        expected: required_tasks,
                        conditions,
                        registered: HashMap::new(),
                        completed: HashMap::new(),
                        skipped: HashMap::new(),
                        sentinel_created: false,
                    },
                );
            }
            Ok(())
        })
    }

    /// Register a task in the checkpoint.
    ///
    /// Extracts the phase from the task ID (e.g., `PF1-TSK-01` -> `PF1`).
    /// Verifies the previous phase sentinel exists (cross-phase gate).
    /// Idempotent: registering an already-registered task is a no-op.
    ///
    /// # Errors
    ///
    /// Returns `PathflowError::CrossPhaseBlock` if the gate check fails.
    pub fn register_task(
        &self,
        checkpoint_path: &Path,
        sentinel_dir: &Path,
        task_id: &str,
    ) -> Result<(), PathflowError> {
        let phase_id = extract_phase(task_id)?;
        check_cross_phase_gate(sentinel_dir, &phase_id)?;

        self.with_lock(checkpoint_path, |cf| {
            let pc = cf
                .phases
                .get_mut(&phase_id)
                .ok_or_else(|| PathflowError::PhaseNotInitialized(phase_id.clone()))?;

            if pc.registered.contains_key(task_id) {
                return Ok(()); // Idempotent
            }

            pc.registered.insert(task_id.to_string(), (self.now)());
            Ok(())
        })
    }

    /// Mark a task as completed.
    ///
    /// If all expected tasks are done, creates the phase sentinel automatically.
    /// Idempotent: completing an already-completed task is a no-op.
    ///
    /// # Errors
    ///
    /// Returns `PathflowError` on I/O or lock errors.
    pub fn complete_task(
        &self,
        checkpoint_path: &Path,
        sentinel_dir: &Path,
        task_id: &str,
    ) -> Result<(), PathflowError> {
        let phase_id = extract_phase(task_id)?;

        self.with_lock(checkpoint_path, |cf| {
            let context = cf.context.clone();
            let pc = cf
                .phases
                .get_mut(&phase_id)
                .ok_or_else(|| PathflowError::PhaseNotInitialized(phase_id.clone()))?;

            if pc.sentinel_created {
                return Ok(());
            }

            if !pc.completed.contains_key(task_id) {
                pc.completed.insert(task_id.to_string(), (self.now)());
            }

            // PF4 stage task completion gate: verify cumulative stage sentinels.
            if phase_id == "PF4" {
                if let Some(err) = check_pf4_stage_sentinels(task_id, sentinel_dir, &context, pc) {
                    // Undo the completion -- stage sentinel missing.
                    pc.completed.remove(task_id);
                    return Err(err);
                }
            }

            if is_phase_complete(pc, &context, sentinel_dir, &phase_id) {
                let sentinel_name = phase_sentinel_name(&phase_id);
                super::sentinel::create_by_name(sentinel_dir, &sentinel_name)?;
                pc.sentinel_created = true;
            }

            Ok(())
        })
    }

    /// Mark a task as skipped.
    ///
    /// If all expected tasks are done/skipped, creates the phase sentinel.
    /// Idempotent.
    ///
    /// # Errors
    ///
    /// Returns `PathflowError` on I/O or lock errors.
    pub fn skip_task(
        &self,
        checkpoint_path: &Path,
        sentinel_dir: &Path,
        task_id: &str,
    ) -> Result<(), PathflowError> {
        let phase_id = extract_phase(task_id)?;

        self.with_lock(checkpoint_path, |cf| {
            let context = cf.context.clone();
            let pc = cf
                .phases
                .get_mut(&phase_id)
                .ok_or_else(|| PathflowError::PhaseNotInitialized(phase_id.clone()))?;

            if pc.sentinel_created {
                return Ok(());
            }

            if !pc.skipped.contains_key(task_id) {
                pc.skipped.insert(task_id.to_string(), (self.now)());
            }

            if is_phase_complete(pc, &context, sentinel_dir, &phase_id) {
                let sentinel_name = phase_sentinel_name(&phase_id);
                super::sentinel::create_by_name(sentinel_dir, &sentinel_name)?;
                pc.sentinel_created = true;
            }

            Ok(())
        })
    }

    /// Return the checkpoint state for a specific phase (read-only).
    ///
    /// # Errors
    ///
    /// Returns `PathflowError` if the phase is not found or on I/O errors.
    pub fn get_status(
        &self,
        checkpoint_path: &Path,
        phase: &str,
    ) -> Result<PhaseCheckpoint, PathflowError> {
        let cf = read_checkpoint_file(checkpoint_path)?;
        cf.phases
            .get(phase)
            .cloned()
            .ok_or_else(|| PathflowError::PhaseNotInitialized(phase.to_string()))
    }

    /// Set a key-value pair in the checkpoint context.
    ///
    /// Used to store session metadata like `work_type` and `origin`.
    ///
    /// # Errors
    ///
    /// Returns `PathflowError` on I/O or lock errors.
    pub fn set_context(
        &self,
        checkpoint_path: &Path,
        key: &str,
        value: &str,
    ) -> Result<(), PathflowError> {
        self.with_lock(checkpoint_path, |cf| {
            cf.context.insert(key.to_string(), value.to_string());
            Ok(())
        })
    }

    /// Perform an atomic read-modify-write on the checkpoint file with
    /// exclusive file locking.
    #[allow(clippy::unused_self)]
    fn with_lock(
        &self,
        checkpoint_path: &Path,
        f: impl FnOnce(&mut CheckpointFile) -> Result<(), PathflowError>,
    ) -> Result<(), PathflowError> {
        if let Some(dir) = checkpoint_path.parent() {
            fs::create_dir_all(dir)?;
        }

        let lock_path = checkpoint_path.with_extension("lock");
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;

        lock_file
            .lock_exclusive()
            .map_err(|e| PathflowError::Lock(format!("acquiring lock: {e}")))?;

        let result = (|| {
            let mut cf = read_checkpoint_file(checkpoint_path)?;
            f(&mut cf)?;
            write_checkpoint_file(checkpoint_path, &cf)
        })();

        // Drop the lock file to release the lock (fs2 releases on drop).
        drop(lock_file);

        result
    }
}

/// Check if all expected tasks in a phase are completed, skipped, or auto-skipped.
///
/// An empty expected list means the phase is NOT complete (safety).
#[must_use]
#[allow(clippy::implicit_hasher)]
pub fn is_phase_complete(
    pc: &PhaseCheckpoint,
    ctx: &HashMap<String, String>,
    sentinel_dir: &Path,
    phase_id: &str,
) -> bool {
    if pc.expected.is_empty() {
        return false;
    }

    for task_id in &pc.expected {
        if pc.completed.contains_key(task_id) {
            continue;
        }
        if pc.skipped.contains_key(task_id) {
            continue;
        }
        if let Some(cond) = pc.conditions.get(task_id) {
            if is_condition_auto_skipped(cond, ctx) {
                continue;
            }
        }
        return false;
    }

    // For PF4: additionally require ALL pipeline stage sentinels.
    if phase_id == "PF4" {
        let work_type = ctx.get("work_type").cloned().unwrap_or_default();
        if !work_type.is_empty() {
            let config_dir = sentinel_dir
                .join("..")
                .join("..")
                .join("..")
                .join("..")
                .join(".codeflow")
                .join("config")
                .join("pathflow");
            if let Ok(pipelines) = crate::hooks::pipeline::load_pipelines(&config_dir) {
                if let Some(pipeline_stages) = pipelines.get(&work_type) {
                    let (ok, _) = crate::hooks::pipeline::verify_cumulative_stage_sentinels(
                        sentinel_dir,
                        pipeline_stages,
                        pipeline_stages.len().saturating_sub(1),
                    );
                    if !ok {
                        return false;
                    }
                }
            }
        }
    }

    true
}

/// Evaluate whether a conditional task should be auto-skipped.
fn is_condition_auto_skipped(condition: &str, ctx: &HashMap<String, String>) -> bool {
    match condition {
        "adhoc_only" => ctx.get("origin").is_some_and(|v| v == "planned"),
        "if_pipeline_includes_qa" => ctx
            .get("work_type")
            .is_some_and(|wt| wt == "DOCS" || wt == "PLAN" || wt == "SPKE"),
        _ => false,
    }
}

/// Extract the phase ID from a task ID (e.g., `PF1-TSK-01` -> `PF1`).
fn extract_phase(task_id: &str) -> Result<String, PathflowError> {
    let re = regex::Regex::new(r"^(PF[0-9]+)-TSK-[0-9]+$").expect("valid regex");
    let captures = re.captures(task_id).ok_or_else(|| {
        PathflowError::InvalidTaskId(format!(
            "invalid task ID format: {task_id} (expected PF{{N}}-TSK-{{NN}})"
        ))
    })?;
    Ok(captures[1].to_string())
}

/// Convert a phase ID to its sentinel name (e.g., `PF1` -> `pf-1`).
fn phase_sentinel_name(phase_id: &str) -> String {
    let num = phase_id.strip_prefix("PF").unwrap_or(phase_id);
    format!("pf-{num}")
}

/// Verify ALL prior phase sentinels exist (cumulative cross-phase gate).
/// Mirrors Go `VerifyCumulativePhaseSentinels` -- checks pf-1 through pf-(N-1).
fn check_cross_phase_gate(sentinel_dir: &Path, phase_id: &str) -> Result<(), PathflowError> {
    let num_str = phase_id.strip_prefix("PF").unwrap_or(phase_id);
    let num: u32 = num_str
        .parse()
        .map_err(|_| PathflowError::InvalidTaskId(format!("invalid phase ID: {phase_id}")))?;

    if num <= 1 {
        return Ok(()); // PF1 has no predecessor
    }

    // Cumulative: check ALL prior phases, not just the immediate predecessor.
    for i in 1..num {
        let sentinel_name = format!("pf-{i}");
        if !super::sentinel::check_by_name(sentinel_dir, &sentinel_name) {
            return Err(PathflowError::CrossPhaseBlock(format!(
                "phase {phase_id} requires sentinel {sentinel_name} (phase PF{i} not complete). All prior phase sentinels (pf-1 through pf-{}) must exist.",
                num - 1
            )));
        }
    }
    Ok(())
}

/// Check PF4 stage sentinels for stage tasks (PF4-TSK-05 through PF4-TSK-07).
/// If the corresponding pipeline stage sentinel is missing, returns an error
/// to undo the completion. Mirrors Go `checkPF4StageSentinels()`.
fn check_pf4_stage_sentinels(
    task_id: &str,
    sentinel_dir: &Path,
    context: &HashMap<String, String>,
    _pc: &PhaseCheckpoint,
) -> Option<PathflowError> {
    // Extract task number from task_id (e.g., "PF4-TSK-06" -> 6)
    let re = regex::Regex::new(r"^PF\d+-TSK-(\d+)$").expect("valid regex");
    let task_num: usize = re
        .captures(task_id)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);

    if task_num < 5 {
        return None; // Only stage tasks (TSK-05+) need this check
    }

    let pipeline_index = task_num - 5;

    let work_type = context.get("work_type").cloned().unwrap_or_default();
    if work_type.is_empty() {
        return None;
    }

    let config_dir = sentinel_dir
        .join("..")
        .join("..")
        .join("..")
        .join("..")
        .join(".codeflow")
        .join("config")
        .join("pathflow");

    let Ok(pipelines) = crate::hooks::pipeline::load_pipelines(&config_dir) else {
        return None; // Config load failure -- allow through
    };

    let pipeline_stages = pipelines.get(&work_type)?;

    if pipeline_index >= pipeline_stages.len() {
        return None;
    }

    let (ok, missing) = crate::hooks::pipeline::verify_cumulative_stage_sentinels(
        sentinel_dir,
        pipeline_stages,
        pipeline_index,
    );

    if !ok {
        return Some(PathflowError::CrossPhaseBlock(format!(
            "Cannot complete {task_id}. Stage sentinel '{missing}' missing. All prior pipeline stages must complete. Pipeline for {work_type}: {pipeline_stages:?}"
        )));
    }

    None
}

fn read_checkpoint_file(path: &Path) -> Result<CheckpointFile, PathflowError> {
    match fs::read_to_string(path) {
        Ok(data) => {
            let cf: CheckpointFile = serde_json::from_str(&data)?;
            Ok(cf)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CheckpointFile::default()),
        Err(e) => Err(PathflowError::Io(e)),
    }
}

fn write_checkpoint_file(path: &Path, cf: &CheckpointFile) -> Result<(), PathflowError> {
    let data = serde_json::to_string_pretty(cf)?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, format!("{data}\n"))?;
    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        PathflowError::Io(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixed_now() -> String {
        "2026-03-07T00:00:00Z".to_string()
    }

    fn create_test_config(dir: &Path) -> std::path::PathBuf {
        let config_path = dir.join("pathflow-config.json");
        let config = serde_json::json!({
            "phases": {
                "PF1-INIT": {
                    "required_tasks": ["PF1-TSK-01", "PF1-TSK-02"],
                    "tasks": [
                        {"id": "PF1-TSK-01"},
                        {"id": "PF1-TSK-02"}
                    ]
                },
                "PF2-CONTEXT": {
                    "required_tasks": ["PF2-TSK-01"],
                    "tasks": [
                        {"id": "PF2-TSK-01"}
                    ]
                },
                "PF4-EXECUTE": {
                    "required_tasks": ["PF4-TSK-01", "PF4-TSK-02"],
                    "tasks": [
                        {"id": "PF4-TSK-01", "condition": "adhoc_only"},
                        {"id": "PF4-TSK-02"}
                    ]
                }
            }
        });
        let mut f = fs::File::create(&config_path).unwrap();
        f.write_all(serde_json::to_string(&config).unwrap().as_bytes())
            .unwrap();
        config_path
    }

    #[test]
    fn test_init_all_phases() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let config_path = create_test_config(dir.path());

        let cp = Checkpoint::with_now(fixed_now);
        cp.init_all_phases(&checkpoint_path, &config_path).unwrap();

        let status = cp.get_status(&checkpoint_path, "PF1").unwrap();
        assert_eq!(status.expected.len(), 2);
        assert!(status.registered.is_empty());
    }

    #[test]
    fn test_init_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let config_path = create_test_config(dir.path());

        let cp = Checkpoint::with_now(fixed_now);
        cp.init_all_phases(&checkpoint_path, &config_path).unwrap();
        cp.init_all_phases(&checkpoint_path, &config_path).unwrap();

        let status = cp.get_status(&checkpoint_path, "PF1").unwrap();
        assert_eq!(status.expected.len(), 2);
    }

    #[test]
    fn test_register_task() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let sentinel_dir = dir.path().join("sentinels");
        let config_path = create_test_config(dir.path());

        let cp = Checkpoint::with_now(fixed_now);
        cp.init_all_phases(&checkpoint_path, &config_path).unwrap();

        // PF1 has no predecessor, so no gate check needed
        cp.register_task(&checkpoint_path, &sentinel_dir, "PF1-TSK-01")
            .unwrap();

        let status = cp.get_status(&checkpoint_path, "PF1").unwrap();
        assert!(status.registered.contains_key("PF1-TSK-01"));
        assert_eq!(status.registered["PF1-TSK-01"], "2026-03-07T00:00:00Z");
    }

    #[test]
    fn test_register_task_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let sentinel_dir = dir.path().join("sentinels");
        let config_path = create_test_config(dir.path());

        let cp = Checkpoint::with_now(fixed_now);
        cp.init_all_phases(&checkpoint_path, &config_path).unwrap();

        cp.register_task(&checkpoint_path, &sentinel_dir, "PF1-TSK-01")
            .unwrap();
        cp.register_task(&checkpoint_path, &sentinel_dir, "PF1-TSK-01")
            .unwrap();

        let status = cp.get_status(&checkpoint_path, "PF1").unwrap();
        assert_eq!(status.registered.len(), 1);
    }

    #[test]
    fn test_complete_task_creates_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let sentinel_dir = dir.path().join("sentinels");
        let config_path = create_test_config(dir.path());

        let cp = Checkpoint::with_now(fixed_now);
        cp.init_all_phases(&checkpoint_path, &config_path).unwrap();

        cp.complete_task(&checkpoint_path, &sentinel_dir, "PF1-TSK-01")
            .unwrap();
        assert!(!super::super::sentinel::check_by_name(
            &sentinel_dir,
            "pf-1"
        ));

        cp.complete_task(&checkpoint_path, &sentinel_dir, "PF1-TSK-02")
            .unwrap();
        assert!(super::super::sentinel::check_by_name(&sentinel_dir, "pf-1"));

        let status = cp.get_status(&checkpoint_path, "PF1").unwrap();
        assert!(status.sentinel_created);
    }

    #[test]
    fn test_skip_task() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let sentinel_dir = dir.path().join("sentinels");
        let config_path = create_test_config(dir.path());

        let cp = Checkpoint::with_now(fixed_now);
        cp.init_all_phases(&checkpoint_path, &config_path).unwrap();

        cp.skip_task(&checkpoint_path, &sentinel_dir, "PF1-TSK-01")
            .unwrap();
        cp.complete_task(&checkpoint_path, &sentinel_dir, "PF1-TSK-02")
            .unwrap();

        assert!(super::super::sentinel::check_by_name(&sentinel_dir, "pf-1"));
    }

    #[test]
    fn test_cross_phase_gate_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let sentinel_dir = dir.path().join("sentinels");
        let config_path = create_test_config(dir.path());

        let cp = Checkpoint::with_now(fixed_now);
        cp.init_all_phases(&checkpoint_path, &config_path).unwrap();

        // PF2 requires PF1 sentinel, which doesn't exist
        let result = cp.register_task(&checkpoint_path, &sentinel_dir, "PF2-TSK-01");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cross-phase"));
    }

    #[test]
    fn test_cross_phase_gate_passes() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let sentinel_dir = dir.path().join("sentinels");
        let config_path = create_test_config(dir.path());

        let cp = Checkpoint::with_now(fixed_now);
        cp.init_all_phases(&checkpoint_path, &config_path).unwrap();

        // Create PF1 sentinel manually
        super::super::sentinel::create_by_name(&sentinel_dir, "pf-1").unwrap();

        // Now PF2 registration should succeed
        cp.register_task(&checkpoint_path, &sentinel_dir, "PF2-TSK-01")
            .unwrap();
    }

    #[test]
    fn test_is_phase_complete_empty_expected() {
        let pc = PhaseCheckpoint {
            expected: vec![],
            conditions: HashMap::new(),
            registered: HashMap::new(),
            completed: HashMap::new(),
            skipped: HashMap::new(),
            sentinel_created: false,
        };
        assert!(!is_phase_complete(
            &pc,
            &HashMap::new(),
            std::path::Path::new("/tmp"),
            "PF1"
        ));
    }

    #[test]
    fn test_condition_auto_skip_adhoc_only() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let sentinel_dir = dir.path().join("sentinels");
        let config_path = create_test_config(dir.path());

        let cp = Checkpoint::with_now(fixed_now);
        cp.init_all_phases(&checkpoint_path, &config_path).unwrap();

        // Create PF1, PF2, PF3 sentinels for gate
        for n in 1..=3 {
            super::super::sentinel::create_by_name(&sentinel_dir, &format!("pf-{n}")).unwrap();
        }

        // Set context so adhoc_only auto-skips
        cp.with_lock(&checkpoint_path, |cf| {
            cf.context
                .insert("origin".to_string(), "planned".to_string());
            Ok(())
        })
        .unwrap();

        // Complete only PF4-TSK-02; PF4-TSK-01 should auto-skip via adhoc_only
        cp.complete_task(&checkpoint_path, &sentinel_dir, "PF4-TSK-02")
            .unwrap();

        assert!(super::super::sentinel::check_by_name(&sentinel_dir, "pf-4"));
    }

    #[test]
    fn test_extract_phase_valid() {
        assert_eq!(extract_phase("PF1-TSK-01").unwrap(), "PF1");
        assert_eq!(extract_phase("PF7-TSK-03").unwrap(), "PF7");
    }

    #[test]
    fn test_extract_phase_invalid() {
        assert!(extract_phase("invalid").is_err());
        assert!(extract_phase("").is_err());
        assert!(extract_phase("TSK-01").is_err());
    }

    #[test]
    fn test_phase_sentinel_name() {
        assert_eq!(phase_sentinel_name("PF1"), "pf-1");
        assert_eq!(phase_sentinel_name("PF7"), "pf-7");
    }

    #[test]
    fn test_get_status_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");

        let cp = Checkpoint::with_now(fixed_now);
        let result = cp.get_status(&checkpoint_path, "PF1");
        assert!(result.is_err());
    }
    // -- PF4 stage sentinel gate tests --

    fn create_pf4_config(dir: &Path) -> std::path::PathBuf {
        let config_dir = dir.join(".codeflow").join("config").join("pathflow");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("pathflow-config.json");
        let config = serde_json::json!({
            "phases": {
                "PF1-INIT": {"phase_order": 1, "required_tasks": ["PF1-TSK-01"], "tasks": [{"id": "PF1-TSK-01"}]},
                "PF4-EXECUTE": {"phase_order": 4, "required_tasks": ["PF4-TSK-05", "PF4-TSK-06", "PF4-TSK-07"], "tasks": [
                    {"id": "PF4-TSK-05"},
                    {"id": "PF4-TSK-06"},
                    {"id": "PF4-TSK-07"}
                ]}
            },
            "pipelines": {
                "FEAT": ["WS-DEV", "WS-REV", "WS-QA"],
                "DOCS": ["WS-DOCS", "WS-REV"],
                "FIX": ["WS-DEV", "WS-REV", "WS-QA"]
            }
        });
        let mut f = fs::File::create(&config_path).unwrap();
        f.write_all(serde_json::to_string(&config).unwrap().as_bytes())
            .unwrap();
        config_path
    }

    fn create_sentinel(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(format!("pathflow-{name}")), b"").unwrap();
    }

    #[test]
    fn test_pf4_tsk06_blocked_when_ws_dev_missing() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path();
        let sentinel_dir = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join("ses-test");
        fs::create_dir_all(&sentinel_dir).unwrap();
        create_pf4_config(project_dir);

        create_sentinel(&sentinel_dir, "ws-rev");

        let mut ctx = HashMap::new();
        ctx.insert("work_type".to_string(), "FEAT".to_string());
        let pc = PhaseCheckpoint {
            expected: vec![
                "PF4-TSK-05".into(),
                "PF4-TSK-06".into(),
                "PF4-TSK-07".into(),
            ],
            conditions: HashMap::new(),
            registered: HashMap::new(),
            completed: HashMap::new(),
            skipped: HashMap::new(),
            sentinel_created: false,
        };

        let result = check_pf4_stage_sentinels("PF4-TSK-06", &sentinel_dir, &ctx, &pc);
        assert!(
            result.is_some(),
            "PF4-TSK-06 should be blocked when ws-dev missing"
        );
        let err = result.unwrap();
        assert!(
            err.to_string().contains("ws-dev"),
            "error should mention ws-dev: {err}"
        );
    }

    #[test]
    fn test_pf4_tsk07_blocked_cumulative() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path();
        let sentinel_dir = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join("ses-test");
        fs::create_dir_all(&sentinel_dir).unwrap();
        create_pf4_config(project_dir);

        create_sentinel(&sentinel_dir, "ws-rev");
        create_sentinel(&sentinel_dir, "ws-qa");

        let mut ctx = HashMap::new();
        ctx.insert("work_type".to_string(), "FEAT".to_string());
        let pc = PhaseCheckpoint {
            expected: vec![],
            conditions: HashMap::new(),
            registered: HashMap::new(),
            completed: HashMap::new(),
            skipped: HashMap::new(),
            sentinel_created: false,
        };

        let result = check_pf4_stage_sentinels("PF4-TSK-07", &sentinel_dir, &ctx, &pc);
        assert!(
            result.is_some(),
            "PF4-TSK-07 should be blocked when ws-dev missing"
        );
        let err = result.unwrap();
        assert!(
            err.to_string().contains("ws-dev"),
            "error should mention first missing: {err}"
        );
    }

    #[test]
    fn test_pf4_tsk05_allowed_when_ws_dev_exists() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path();
        let sentinel_dir = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join("ses-test");
        fs::create_dir_all(&sentinel_dir).unwrap();
        create_pf4_config(project_dir);

        create_sentinel(&sentinel_dir, "ws-dev");

        let mut ctx = HashMap::new();
        ctx.insert("work_type".to_string(), "FEAT".to_string());
        let pc = PhaseCheckpoint {
            expected: vec![],
            conditions: HashMap::new(),
            registered: HashMap::new(),
            completed: HashMap::new(),
            skipped: HashMap::new(),
            sentinel_created: false,
        };

        let result = check_pf4_stage_sentinels("PF4-TSK-05", &sentinel_dir, &ctx, &pc);
        assert!(
            result.is_none(),
            "PF4-TSK-05 should be allowed when ws-dev exists"
        );
    }

    #[test]
    fn test_pf4_tsk07_skipped_for_docs_pipeline() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path();
        let sentinel_dir = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join("ses-test");
        fs::create_dir_all(&sentinel_dir).unwrap();
        create_pf4_config(project_dir);

        let mut ctx = HashMap::new();
        ctx.insert("work_type".to_string(), "DOCS".to_string());
        let pc = PhaseCheckpoint {
            expected: vec![],
            conditions: HashMap::new(),
            registered: HashMap::new(),
            completed: HashMap::new(),
            skipped: HashMap::new(),
            sentinel_created: false,
        };

        let result = check_pf4_stage_sentinels("PF4-TSK-07", &sentinel_dir, &ctx, &pc);
        assert!(
            result.is_none(),
            "PF4-TSK-07 should be skipped for DOCS pipeline"
        );
    }

    #[test]
    fn test_pf4_tsk04_not_checked() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = HashMap::new();
        let pc = PhaseCheckpoint {
            expected: vec![],
            conditions: HashMap::new(),
            registered: HashMap::new(),
            completed: HashMap::new(),
            skipped: HashMap::new(),
            sentinel_created: false,
        };

        let result = check_pf4_stage_sentinels("PF4-TSK-04", dir.path(), &ctx, &pc);
        assert!(result.is_none(), "PF4-TSK-04 should not be checked");
    }

    #[test]
    fn test_pf4_no_work_type_allows_through() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = HashMap::new();
        let pc = PhaseCheckpoint {
            expected: vec![],
            conditions: HashMap::new(),
            registered: HashMap::new(),
            completed: HashMap::new(),
            skipped: HashMap::new(),
            sentinel_created: false,
        };

        let result = check_pf4_stage_sentinels("PF4-TSK-06", dir.path(), &ctx, &pc);
        assert!(result.is_none(), "empty work_type should allow through");
    }
}
