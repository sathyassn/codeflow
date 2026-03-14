//! Pipeline configuration and cumulative sentinel verification.
//!
//! Mirrors Go `sentinel/pipeline.go`:
//! - Load pipeline definitions from `pathflow-config.json`
//! - Load phase ordering from config
//! - Verify cumulative stage/phase sentinels
//! - Infer work type from git branch name
//! - Derive project/session/config directories from sentinel directory

use std::collections::HashMap;
use std::path::Path;

use crate::pathflow::sentinel;

/// Represents the relevant parts of `pathflow-config.json`.
#[derive(Debug, serde::Deserialize)]
struct PathflowConfig {
    #[serde(default)]
    phases: HashMap<String, PhaseConfig>,
    #[serde(default)]
    pipelines: Option<HashMap<String, Vec<String>>>,
}

/// A single phase entry with its ordering.
#[derive(Debug, serde::Deserialize)]
struct PhaseConfig {
    #[serde(default)]
    phase_order: usize,
}

/// Load pipeline definitions from `pathflow-config.json`.
///
/// Returns `map[workType][]stageNames`, e.g.,
/// `{"FEAT": ["WS-DEV", "WS-REV", "WS-QA"]}`.
///
/// # Errors
///
/// Returns error if the config file cannot be read, parsed, or has no
/// `pipelines` section.
pub fn load_pipelines(config_dir: &Path) -> Result<HashMap<String, Vec<String>>, String> {
    let config_path = config_dir.join("pathflow-config.json");
    let data =
        std::fs::read_to_string(&config_path).map_err(|e| format!("load pipelines: {e}"))?;

    let config: PathflowConfig =
        serde_json::from_str(&data).map_err(|e| format!("parse pipelines: {e}"))?;

    config
        .pipelines
        .ok_or_else(|| "load pipelines: no pipelines section in config".to_string())
}

/// Load phase ordering from `pathflow-config.json`.
///
/// Returns phase keys in order: `["PF1", "PF2", ..., "PF7"]`.
///
/// # Errors
///
/// Returns error if the config file cannot be read, parsed, or has no
/// `phases` section.
pub fn load_phase_order(config_dir: &Path) -> Result<Vec<String>, String> {
    let config_path = config_dir.join("pathflow-config.json");
    let data =
        std::fs::read_to_string(&config_path).map_err(|e| format!("load phase order: {e}"))?;

    let config: PathflowConfig =
        serde_json::from_str(&data).map_err(|e| format!("parse phase order: {e}"))?;

    if config.phases.is_empty() {
        return Err("load phase order: no phases section in config".to_string());
    }

    // Build ordered slice using phase_order field.
    let mut ordered = vec![String::new(); config.phases.len()];
    for (key, phase) in &config.phases {
        // Extract "PF1" from "PF1-INIT".
        let phase_id = key.split('-').next().unwrap_or(key);
        let idx = phase.phase_order.wrapping_sub(1); // phase_order is 1-based
        if idx < ordered.len() {
            ordered[idx] = phase_id.to_string();
        }
    }

    // Filter out any empty slots.
    Ok(ordered.into_iter().filter(|p| !p.is_empty()).collect())
}

/// Verify cumulative stage sentinels: ALL `pipeline[0]` through
/// `pipeline[up_to_index]` must exist.
///
/// Returns `(all_present, first_missing_sentinel)`.
#[must_use]
pub fn verify_cumulative_stage_sentinels(
    sentinel_dir: &Path,
    pipeline: &[String],
    up_to_index: usize,
) -> (bool, String) {
    for (i, stage) in pipeline.iter().enumerate() {
        if i > up_to_index {
            break;
        }
        let name = stage_sentinel_name(stage);
        if !sentinel::check_by_name(sentinel_dir, &name) {
            return (false, name);
        }
    }
    (true, String::new())
}

/// Verify cumulative phase sentinels: ALL `pf-1` through `pf-{up_to_phase}`
/// must exist.
///
/// Returns `(all_present, first_missing_sentinel)`.
#[must_use]
pub fn verify_cumulative_phase_sentinels(
    sentinel_dir: &Path,
    up_to_phase: usize,
) -> (bool, String) {
    for i in 1..=up_to_phase {
        let name = format!("pf-{i}");
        if !sentinel::check_by_name(sentinel_dir, &name) {
            return (false, name);
        }
    }
    (true, String::new())
}

/// Map git branch prefix to work type.
///
/// `feat/` -> `FEAT`, `fix/` -> `FIX`, `refactor/` -> `RFCT`, etc.
/// Returns empty string if the branch prefix is not recognized.
#[must_use]
pub fn infer_work_type_from_branch(branch_name: &str) -> &'static str {
    const PREFIX_MAP: &[(&str, &str)] = &[
        ("feat/", "FEAT"),
        ("fix/", "FIX"),
        ("refactor/", "RFCT"),
        ("docs/", "DOCS"),
        ("test/", "TEST"),
        ("chore/", "CHOR"),
        ("cicd/", "CICD"),
        ("plan/", "PLAN"),
        ("spike/", "SPKE"),
        ("hotfix/", "HTFX"),
    ];

    for &(prefix, work_type) in PREFIX_MAP {
        if branch_name.starts_with(prefix) {
            return work_type;
        }
    }
    ""
}

/// Convert a pipeline entry to its sentinel name: `"WS-DEV"` -> `"ws-dev"`.
#[must_use]
pub fn stage_sentinel_name(pipeline_entry: &str) -> String {
    pipeline_entry.to_lowercase()
}

/// Read `work_type` from `pathflow-session-status.json` in the given session
/// pathflow directory.
#[must_use]
pub fn read_work_type_from_session_status(session_dir: &Path) -> String {
    let status_path = session_dir.join("pathflow-session-status.json");
    #[derive(serde::Deserialize)]
    struct Status {
        #[serde(default)]
        work_type: String,
    }

    let Ok(data) = std::fs::read_to_string(&status_path) else {
        return String::new();
    };
    serde_json::from_str::<Status>(&data)
        .map(|s| s.work_type)
        .unwrap_or_default()
}

/// Derive the project root from a sentinel directory path.
///
/// `sentinel_dir` is `{project_dir}/.state/sentinels/pathflow/{SID}/`,
/// so project dir is 4 levels up.
#[must_use]
pub fn derive_project_dir(sentinel_dir: &Path) -> std::path::PathBuf {
    sentinel_dir.join("..").join("..").join("..").join("..")
}

/// Derive the session pathflow directory from a sentinel directory path.
///
/// `sentinel_dir` is `{project_dir}/.state/sentinels/pathflow/{SID}/`,
/// `session_dir` is `{project_dir}/.state/session/{SID}/pathflow/`.
#[must_use]
pub fn derive_session_dir(sentinel_dir: &Path) -> std::path::PathBuf {
    let project_dir = derive_project_dir(sentinel_dir);
    let session_id = sentinel_dir
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    project_dir
        .join(".state")
        .join("session")
        .join(session_id)
        .join("pathflow")
}

/// Derive the pathflow config directory from a sentinel directory path.
#[must_use]
pub fn derive_config_dir(sentinel_dir: &Path) -> std::path::PathBuf {
    let project_dir = derive_project_dir(sentinel_dir);
    project_dir.join(".codeflow").join("config").join("pathflow")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn write_test_config(config_dir: &Path, pipelines: &HashMap<String, Vec<String>>) {
        let config = serde_json::json!({
            "phases": {
                "PF1-INIT": {"phase_order": 1},
                "PF2-CONTEXT": {"phase_order": 2},
                "PF3-CLASSIFY": {"phase_order": 3},
                "PF4-EXECUTE": {"phase_order": 4},
                "PF5-VERIFY": {"phase_order": 5},
                "PF6-COMPLETE": {"phase_order": 6},
                "PF7-END": {"phase_order": 7},
            },
            "pipelines": pipelines,
        });
        fs::create_dir_all(config_dir).unwrap();
        fs::write(
            config_dir.join("pathflow-config.json"),
            serde_json::to_string_pretty(&config).unwrap(),
        )
        .unwrap();
    }

    fn create_test_sentinel(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(format!("pathflow-{name}")), b"").unwrap();
    }

    // -- LoadPipelines tests --

    #[test]
    fn test_load_pipelines_all() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let mut pipelines = HashMap::new();
        pipelines.insert(
            "FEAT".into(),
            vec!["WS-DEV".into(), "WS-REV".into(), "WS-QA".into()],
        );
        pipelines.insert("DOCS".into(), vec!["WS-DOCS".into(), "WS-REV".into()]);
        pipelines.insert("PLAN".into(), vec!["WS-PLAN".into(), "WS-REV".into()]);
        write_test_config(&config_dir, &pipelines);

        let result = load_pipelines(&config_dir).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result["FEAT"], vec!["WS-DEV", "WS-REV", "WS-QA"]);
        assert_eq!(result["DOCS"], vec!["WS-DOCS", "WS-REV"]);
    }

    #[test]
    fn test_load_pipelines_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_pipelines(&dir.path().join("nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_pipelines_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pathflow-config.json"), "not json").unwrap();
        let result = load_pipelines(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_load_pipelines_missing_section() {
        let dir = tempfile::tempdir().unwrap();
        let config = serde_json::json!({"phases": {}});
        fs::write(
            dir.path().join("pathflow-config.json"),
            serde_json::to_string(&config).unwrap(),
        )
        .unwrap();
        let result = load_pipelines(dir.path());
        assert!(result.is_err());
    }

    // -- LoadPhaseOrder tests --

    #[test]
    fn test_load_phase_order() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        write_test_config(&config_dir, &HashMap::new());

        let phases = load_phase_order(&config_dir).unwrap();
        assert_eq!(
            phases,
            vec!["PF1", "PF2", "PF3", "PF4", "PF5", "PF6", "PF7"]
        );
    }

    #[test]
    fn test_load_phase_order_missing_config() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_phase_order(&dir.path().join("nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_phase_order_missing_phases() {
        let dir = tempfile::tempdir().unwrap();
        let config = serde_json::json!({"pipelines": {}});
        fs::write(
            dir.path().join("pathflow-config.json"),
            serde_json::to_string(&config).unwrap(),
        )
        .unwrap();
        let result = load_phase_order(dir.path());
        assert!(result.is_err());
    }

    // -- VerifyCumulativeStageSentinels tests --

    #[test]
    fn test_stage_sentinels_all_present() {
        let dir = tempfile::tempdir().unwrap();
        let sdir = dir.path();
        create_test_sentinel(sdir, "ws-dev");
        create_test_sentinel(sdir, "ws-rev");
        create_test_sentinel(sdir, "ws-qa");

        let pipeline: Vec<String> = vec!["WS-DEV".into(), "WS-REV".into(), "WS-QA".into()];
        let (ok, missing) = verify_cumulative_stage_sentinels(sdir, &pipeline, 2);
        assert!(ok);
        assert!(missing.is_empty());
    }

    #[test]
    fn test_stage_sentinels_first_missing() {
        let dir = tempfile::tempdir().unwrap();
        let sdir = dir.path();
        create_test_sentinel(sdir, "ws-rev");

        let pipeline: Vec<String> = vec!["WS-DEV".into(), "WS-REV".into()];
        let (ok, missing) = verify_cumulative_stage_sentinels(sdir, &pipeline, 1);
        assert!(!ok);
        assert_eq!(missing, "ws-dev");
    }

    #[test]
    fn test_stage_sentinels_middle_missing() {
        let dir = tempfile::tempdir().unwrap();
        let sdir = dir.path();
        create_test_sentinel(sdir, "ws-dev");
        create_test_sentinel(sdir, "ws-qa");

        let pipeline: Vec<String> = vec!["WS-DEV".into(), "WS-REV".into(), "WS-QA".into()];
        let (ok, missing) = verify_cumulative_stage_sentinels(sdir, &pipeline, 2);
        assert!(!ok);
        assert_eq!(missing, "ws-rev");
    }

    #[test]
    fn test_stage_sentinels_index_zero() {
        let dir = tempfile::tempdir().unwrap();
        let sdir = dir.path();
        create_test_sentinel(sdir, "ws-dev");

        let pipeline: Vec<String> = vec!["WS-DEV".into(), "WS-REV".into(), "WS-QA".into()];
        let (ok, _) = verify_cumulative_stage_sentinels(sdir, &pipeline, 0);
        assert!(ok);
    }

    #[test]
    fn test_stage_sentinels_empty_pipeline() {
        let dir = tempfile::tempdir().unwrap();
        let (ok, _) = verify_cumulative_stage_sentinels(dir.path(), &[], 0);
        assert!(ok);
    }

    // -- VerifyCumulativePhaseSentinels tests --

    #[test]
    fn test_phase_sentinels_all_present() {
        let dir = tempfile::tempdir().unwrap();
        let sdir = dir.path();
        for i in 1..=5 {
            create_test_sentinel(sdir, &format!("pf-{i}"));
        }
        let (ok, missing) = verify_cumulative_phase_sentinels(sdir, 5);
        assert!(ok);
        assert!(missing.is_empty());
    }

    #[test]
    fn test_phase_sentinels_gap_in_middle() {
        let dir = tempfile::tempdir().unwrap();
        let sdir = dir.path();
        create_test_sentinel(sdir, "pf-1");
        // pf-2 missing
        create_test_sentinel(sdir, "pf-3");
        create_test_sentinel(sdir, "pf-4");

        let (ok, missing) = verify_cumulative_phase_sentinels(sdir, 4);
        assert!(!ok);
        assert_eq!(missing, "pf-2");
    }

    #[test]
    fn test_phase_sentinels_first_missing() {
        let dir = tempfile::tempdir().unwrap();
        let sdir = dir.path();
        for i in 2..=5 {
            create_test_sentinel(sdir, &format!("pf-{i}"));
        }
        let (ok, missing) = verify_cumulative_phase_sentinels(sdir, 5);
        assert!(!ok);
        assert_eq!(missing, "pf-1");
    }

    #[test]
    fn test_phase_sentinels_zero_passes() {
        let dir = tempfile::tempdir().unwrap();
        let (ok, _) = verify_cumulative_phase_sentinels(dir.path(), 0);
        assert!(ok);
    }

    // -- InferWorkTypeFromBranch tests --

    #[test]
    fn test_infer_work_type_all_prefixes() {
        assert_eq!(infer_work_type_from_branch("feat/add-login"), "FEAT");
        assert_eq!(infer_work_type_from_branch("fix/auth-bug"), "FIX");
        assert_eq!(infer_work_type_from_branch("refactor/cleanup"), "RFCT");
        assert_eq!(infer_work_type_from_branch("docs/readme"), "DOCS");
        assert_eq!(infer_work_type_from_branch("test/coverage"), "TEST");
        assert_eq!(infer_work_type_from_branch("chore/deps"), "CHOR");
        assert_eq!(infer_work_type_from_branch("cicd/pipeline"), "CICD");
        assert_eq!(infer_work_type_from_branch("plan/design"), "PLAN");
        assert_eq!(infer_work_type_from_branch("spike/prototype"), "SPKE");
        assert_eq!(infer_work_type_from_branch("hotfix/urgent"), "HTFX");
    }

    #[test]
    fn test_infer_work_type_unrecognized() {
        assert_eq!(infer_work_type_from_branch("main"), "");
        assert_eq!(infer_work_type_from_branch("unknown/branch"), "");
        assert_eq!(infer_work_type_from_branch(""), "");
    }

    // -- StageSentinelName tests --

    #[test]
    fn test_stage_sentinel_name_conversion() {
        assert_eq!(stage_sentinel_name("WS-DEV"), "ws-dev");
        assert_eq!(stage_sentinel_name("WS-REV"), "ws-rev");
        assert_eq!(stage_sentinel_name("WS-QA"), "ws-qa");
        assert_eq!(stage_sentinel_name("WS-DOCS"), "ws-docs");
        assert_eq!(stage_sentinel_name("WS-PLAN"), "ws-plan");
        assert_eq!(stage_sentinel_name("WS-TEST"), "ws-test");
    }

    // -- ReadWorkTypeFromSessionStatus tests --

    #[test]
    fn test_read_work_type_from_status() {
        let dir = tempfile::tempdir().unwrap();
        let status = serde_json::json!({"work_type": "FEAT", "status": "pf-in-progress"});
        fs::write(
            dir.path().join("pathflow-session-status.json"),
            serde_json::to_string(&status).unwrap(),
        )
        .unwrap();

        assert_eq!(read_work_type_from_session_status(dir.path()), "FEAT");
    }

    #[test]
    fn test_read_work_type_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_work_type_from_session_status(dir.path()), "");
    }

    #[test]
    fn test_read_work_type_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("pathflow-session-status.json"),
            "not json",
        )
        .unwrap();
        assert_eq!(read_work_type_from_session_status(dir.path()), "");
    }

    // -- Derive directory tests --

    #[test]
    fn test_derive_project_dir() {
        let sdir = PathBuf::from("/project/.state/sentinels/pathflow/ses-123");
        let got = derive_project_dir(&sdir);
        let got_str = got.to_string_lossy();
        assert!(got_str.contains("project"), "got: {got_str}");
    }

    #[test]
    fn test_derive_session_dir() {
        let sdir = PathBuf::from("/project/.state/sentinels/pathflow/ses-123");
        let got = derive_session_dir(&sdir);
        let got_str = got.to_string_lossy();
        assert!(got_str.contains("session"));
        assert!(got_str.contains("ses-123"));
        assert!(got_str.contains("pathflow"));
    }

    #[test]
    fn test_derive_config_dir() {
        let sdir = PathBuf::from("/project/.state/sentinels/pathflow/ses-123");
        let got = derive_config_dir(&sdir);
        let got_str = got.to_string_lossy();
        assert!(got_str.contains(".codeflow"));
        assert!(got_str.contains("config"));
        assert!(got_str.contains("pathflow"));
    }
}
