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

            if is_phase_complete(pc, &context) {
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

            if is_phase_complete(pc, &context) {
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
pub fn is_phase_complete(pc: &PhaseCheckpoint, ctx: &HashMap<String, String>) -> bool {
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

/// Verify the previous phase sentinel exists (cross-phase gate).
fn check_cross_phase_gate(sentinel_dir: &Path, phase_id: &str) -> Result<(), PathflowError> {
    let num_str = phase_id.strip_prefix("PF").unwrap_or(phase_id);
    let num: u32 = num_str
        .parse()
        .map_err(|_| PathflowError::InvalidTaskId(format!("invalid phase ID: {phase_id}")))?;

    if num <= 1 {
        return Ok(()); // PF1 has no predecessor
    }

    let prev_phase = format!("PF{}", num - 1);
    let prev_sentinel = phase_sentinel_name(&prev_phase);

    if !super::sentinel::check_by_name(sentinel_dir, &prev_sentinel) {
        return Err(PathflowError::CrossPhaseBlock(format!(
            "phase {phase_id} requires sentinel {prev_sentinel} (phase {prev_phase} not complete)"
        )));
    }
    Ok(())
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
        assert!(!is_phase_complete(&pc, &HashMap::new()));
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
}
