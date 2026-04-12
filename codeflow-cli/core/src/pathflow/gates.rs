//! Declarative gate configuration for PathFlow enforcement.
//!
//! Replaces hardcoded sentinel name literals in hook code with config-driven
//! lookups. Adding a new gate or changing a requirement is a change to
//! `pathflow-config.json` — no Rust enforcement code modifications required.
//!
//! # Design
//!
//! Every gate has a `GateRequirement` describing what must be true for the
//! gate to pass. The two orthogonal dimensions are:
//!
//! - **Phase requirement** (`requires_phase`): the phase whose sentinel must
//!   exist. When `cumulative_phases=true`, every phase through that one must
//!   exist (pf-1 through pf-N).
//! - **Stage requirement** (`requires_all_pipeline_stages`): whether ALL
//!   stages in the current work type's pipeline must have sentinels.
//!
//! Gates without a `requires_phase` field (e.g., `work_type_infer`,
//! `pr_pushed`) describe semantic markers triggered by a phase completing,
//! not blocking conditions. They carry `on_phase_complete` instead.
//!
//! # Loading
//!
//! Load once at handler construction via [`GateConfig::load`] or
//! [`GateConfig::load_from_path`]. Both panic on invalid config — this is
//! intentional: a malformed gate config at startup is a fatal deploy error,
//! not a runtime recovery condition.
//!
//! # Sentinel name derivation
//!
//! Phase sentinels are derived deterministically from the `Phase` enum:
//! `format!("pf-{}", phase.index())`. Stage sentinels come from the pipeline
//! config via `stage_sentinel_name()` (lowercasing the pipeline entry).
//! No hardcoded name literals in this module.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::pathflow::sentinel;
use crate::types::Phase;

use super::super::hooks::pipeline;

/// A single gate's declarative requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRequirement {
    /// Name of the gate (e.g., "edit_write", "git_push").
    pub name: String,
    /// Phase whose sentinel must exist for this gate to pass.
    pub requires_phase: Option<Phase>,
    /// Phase whose completion triggers this semantic marker (non-blocking).
    pub on_phase_complete: Option<Phase>,
    /// When true, verify ALL phase sentinels up through `requires_phase`,
    /// not just the single target phase.
    pub cumulative_phases: bool,
    /// When true, verify ALL pipeline stage sentinels for the current
    /// work type.
    pub requires_all_pipeline_stages: bool,
    /// Human-readable description (for error messages and docs).
    pub description: String,
}

/// Raw gate entry from `pathflow-config.json`.
#[derive(Debug, Deserialize)]
struct RawGate {
    #[serde(default)]
    requires_phase: Option<String>,
    #[serde(default)]
    on_phase_complete: Option<String>,
    #[serde(default)]
    cumulative_phases: bool,
    #[serde(default)]
    requires_all_pipeline_stages: bool,
    #[serde(default)]
    description: String,
}

/// Parsed gate configuration — a map from gate name to its requirement.
#[derive(Debug, Clone)]
pub struct GateConfig {
    gates: HashMap<String, GateRequirement>,
}

impl GateConfig {
    /// Load gate config from `pathflow-config.json` under `config_dir`.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - the config file cannot be read
    /// - the JSON is invalid
    /// - the `gates` section is missing
    /// - any gate's `requires_phase` or `on_phase_complete` does not parse
    ///   to a valid `Phase` enum variant
    #[must_use]
    pub fn load(config_dir: &Path) -> Self {
        Self::load_from_path(&config_dir.join("pathflow-config.json"))
    }

    /// Load gate config from an explicit `pathflow-config.json` path.
    ///
    /// # Panics
    ///
    /// See [`GateConfig::load`].
    #[must_use]
    pub fn load_from_path(path: &Path) -> Self {
        let data = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("GateConfig::load: failed to read {}: {e}", path.display()));

        // Parse as a generic JSON Value so comment-shaped entries (scalar
        // strings under `_*` keys) can be filtered out before per-entry
        // deserialization. The config convention is to permit `_comment`
        // keys as free-form documentation with any value shape.
        #[derive(Deserialize)]
        struct Config {
            #[serde(default)]
            gates: Option<HashMap<String, serde_json::Value>>,
        }

        let parsed: Config = serde_json::from_str(&data).unwrap_or_else(|e| {
            panic!("GateConfig::load: invalid JSON in {}: {e}", path.display())
        });

        let raw_gates = parsed.gates.unwrap_or_else(|| {
            panic!(
                "GateConfig::load: missing 'gates' section in {}",
                path.display()
            )
        });

        let mut gates = HashMap::with_capacity(raw_gates.len());
        for (name, value) in raw_gates {
            // Skip comment keys (entries starting with "_") regardless of
            // value shape — string, object, array, null are all accepted.
            if name.starts_with('_') {
                continue;
            }
            let raw: RawGate = serde_json::from_value(value).unwrap_or_else(|e| {
                panic!(
                    "GateConfig::load: gate '{name}' has invalid shape in {}: {e}",
                    path.display()
                )
            });
            let requires_phase = raw.requires_phase.as_deref().map(|s| {
                s.parse::<Phase>().unwrap_or_else(|e| {
                    panic!(
                        "GateConfig::load: gate '{name}' requires_phase='{s}' is not a valid Phase: {e}"
                    )
                })
            });
            let on_phase_complete = raw.on_phase_complete.as_deref().map(|s| {
                s.parse::<Phase>().unwrap_or_else(|e| {
                    panic!(
                        "GateConfig::load: gate '{name}' on_phase_complete='{s}' is not a valid Phase: {e}"
                    )
                })
            });
            gates.insert(
                name.clone(),
                GateRequirement {
                    name,
                    requires_phase,
                    on_phase_complete,
                    cumulative_phases: raw.cumulative_phases,
                    requires_all_pipeline_stages: raw.requires_all_pipeline_stages,
                    description: raw.description,
                },
            );
        }

        Self { gates }
    }

    /// Look up a gate by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&GateRequirement> {
        self.gates.get(name)
    }

    /// Number of gates loaded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.gates.len()
    }

    /// Whether the config loaded no gates.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.gates.is_empty()
    }
}

/// Result of a gate satisfaction check.
///
/// On failure, carries the first missing sentinel name so callers can emit
/// actionable error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateCheckOutcome {
    /// Gate is satisfied — all prerequisites exist.
    Satisfied,
    /// Required phase sentinel is missing.
    MissingPhase(String),
    /// Required pipeline stage sentinel is missing.
    MissingStage(String),
    /// The gate requires a work_type but session_status reports none.
    MissingWorkType,
    /// The gate requires a pipeline definition for the current work_type
    /// but none is present in config.
    MissingPipeline(String),
}

impl GateRequirement {
    /// Check whether this gate is satisfied given the session state.
    ///
    /// # Arguments
    ///
    /// - `sentinel_dir`: directory containing `pathflow-*` sentinel files
    /// - `work_type`: current work type (empty if not yet classified)
    /// - `pipelines`: map of work_type → ordered stage names (from config)
    #[must_use]
    pub fn check(
        &self,
        sentinel_dir: &Path,
        work_type: &str,
        pipelines: &HashMap<String, Vec<String>>,
    ) -> GateCheckOutcome {
        // Phase check.
        if let Some(phase) = self.requires_phase {
            if self.cumulative_phases {
                let (ok, missing) = pipeline::verify_cumulative_phase_sentinels(
                    sentinel_dir,
                    phase.index() as usize,
                );
                if !ok {
                    return GateCheckOutcome::MissingPhase(missing);
                }
            } else {
                let name = format!("pf-{}", phase.index());
                if !sentinel::check_by_name(sentinel_dir, &name) {
                    return GateCheckOutcome::MissingPhase(name);
                }
            }
        }

        // Pipeline stage check.
        if self.requires_all_pipeline_stages {
            if work_type.is_empty() {
                return GateCheckOutcome::MissingWorkType;
            }
            let Some(pipeline_stages) = pipelines.get(work_type) else {
                return GateCheckOutcome::MissingPipeline(work_type.to_string());
            };
            let (ok, missing) = pipeline::verify_cumulative_stage_sentinels(
                sentinel_dir,
                pipeline_stages,
                pipeline_stages.len().saturating_sub(1),
            );
            if !ok {
                return GateCheckOutcome::MissingStage(missing);
            }
        }

        GateCheckOutcome::Satisfied
    }

    /// Convenience: return `true` iff the gate is fully satisfied.
    #[must_use]
    pub fn is_satisfied(
        &self,
        sentinel_dir: &Path,
        work_type: &str,
        pipelines: &HashMap<String, Vec<String>>,
    ) -> bool {
        matches!(
            self.check(sentinel_dir, work_type, pipelines),
            GateCheckOutcome::Satisfied
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_config(dir: &Path, gates_json: &str) -> std::path::PathBuf {
        let path = dir.join("pathflow-config.json");
        let body = format!(
            r#"{{
              "phases": {{
                "PF1-INIT": {{"phase_order": 1}},
                "PF3-CLASSIFY": {{"phase_order": 3}},
                "PF5-VERIFY": {{"phase_order": 5}},
                "PF6-COMPLETE": {{"phase_order": 6}}
              }},
              "gates": {gates_json}
            }}"#
        );
        fs::write(&path, body).unwrap();
        path
    }

    fn create_sentinel(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(format!("pathflow-{name}")), b"").unwrap();
    }

    // -- Config loading --

    #[test]
    fn test_gate_config_load_valid() {
        let dir = TempDir::new().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
              "edit_write": {"requires_phase": "PF3-CLASSIFY", "description": "x"},
              "git_push": {
                "requires_phase": "PF5-VERIFY",
                "cumulative_phases": true,
                "requires_all_pipeline_stages": true,
                "description": "y"
              },
              "pr_pushed": {"on_phase_complete": "PF6-COMPLETE", "description": "z"}
            }"#,
        );

        let config = GateConfig::load_from_path(&path);
        assert_eq!(config.len(), 3);

        let ew = config.get("edit_write").unwrap();
        assert_eq!(ew.requires_phase, Some(Phase::Pf3Classify));
        assert!(!ew.cumulative_phases);
        assert!(!ew.requires_all_pipeline_stages);

        let gp = config.get("git_push").unwrap();
        assert_eq!(gp.requires_phase, Some(Phase::Pf5Verify));
        assert!(gp.cumulative_phases);
        assert!(gp.requires_all_pipeline_stages);

        let pr = config.get("pr_pushed").unwrap();
        assert_eq!(pr.on_phase_complete, Some(Phase::Pf6Complete));
        assert!(pr.requires_phase.is_none());
    }

    #[test]
    fn test_gate_config_load_skips_comment_keys() {
        let dir = TempDir::new().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
              "_comment": {"description": "ignored"},
              "edit_write": {"requires_phase": "PF3-CLASSIFY", "description": "x"}
            }"#,
        );

        let config = GateConfig::load_from_path(&path);
        assert_eq!(config.len(), 1);
        assert!(config.get("_comment").is_none());
        assert!(config.get("edit_write").is_some());
    }

    #[test]
    fn test_gate_config_load_string_comment_key() {
        // The on-disk pathflow-config.json uses a scalar string value for
        // its _comment key. Verify we accept that shape.
        let dir = TempDir::new().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
              "_comment": "This is a free-form documentation string, not a gate.",
              "edit_write": {"requires_phase": "PF3-CLASSIFY", "description": "x"}
            }"#,
        );

        let config = GateConfig::load_from_path(&path);
        assert_eq!(config.len(), 1);
        assert!(config.get("edit_write").is_some());
    }

    #[test]
    #[should_panic(expected = "missing 'gates' section")]
    fn test_gate_config_load_missing_gates() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pathflow-config.json");
        fs::write(&path, r#"{"phases": {}}"#).unwrap();
        let _ = GateConfig::load_from_path(&path);
    }

    #[test]
    #[should_panic(expected = "is not a valid Phase")]
    fn test_gate_config_load_invalid_phase() {
        let dir = TempDir::new().unwrap();
        let path = write_config(
            dir.path(),
            r#"{
              "bad_gate": {"requires_phase": "PF99-FAKE", "description": "bogus"}
            }"#,
        );
        let _ = GateConfig::load_from_path(&path);
    }

    #[test]
    #[should_panic(expected = "failed to read")]
    fn test_gate_config_load_missing_file() {
        let dir = TempDir::new().unwrap();
        let _ = GateConfig::load_from_path(&dir.path().join("nonexistent.json"));
    }

    #[test]
    #[should_panic(expected = "invalid JSON")]
    fn test_gate_config_load_invalid_json() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pathflow-config.json");
        fs::write(&path, "not json {{{").unwrap();
        let _ = GateConfig::load_from_path(&path);
    }

    // -- Gate satisfaction --

    fn make_req(requires_phase: Option<Phase>, cumulative: bool, stages: bool) -> GateRequirement {
        GateRequirement {
            name: "test".into(),
            requires_phase,
            on_phase_complete: None,
            cumulative_phases: cumulative,
            requires_all_pipeline_stages: stages,
            description: String::new(),
        }
    }

    #[test]
    fn test_gate_edit_write_satisfied() {
        let dir = TempDir::new().unwrap();
        create_sentinel(dir.path(), "pf-3");
        let req = make_req(Some(Phase::Pf3Classify), false, false);
        assert!(req.is_satisfied(dir.path(), "", &HashMap::new()));
    }

    #[test]
    fn test_gate_edit_write_not_satisfied() {
        let dir = TempDir::new().unwrap();
        // No pf-3 sentinel.
        let req = make_req(Some(Phase::Pf3Classify), false, false);
        let outcome = req.check(dir.path(), "", &HashMap::new());
        assert_eq!(outcome, GateCheckOutcome::MissingPhase("pf-3".into()));
    }

    #[test]
    fn test_gate_git_push_cumulative_all_present() {
        let dir = TempDir::new().unwrap();
        for i in 1..=5 {
            create_sentinel(dir.path(), &format!("pf-{i}"));
        }
        create_sentinel(dir.path(), "ws-dev");
        create_sentinel(dir.path(), "ws-rev");
        create_sentinel(dir.path(), "ws-qa");

        let mut pipelines = HashMap::new();
        pipelines.insert(
            "FIX".into(),
            vec!["WS-DEV".into(), "WS-REV".into(), "WS-QA".into()],
        );

        let req = make_req(Some(Phase::Pf5Verify), true, true);
        assert!(req.is_satisfied(dir.path(), "FIX", &pipelines));
    }

    #[test]
    fn test_gate_git_push_missing_one_phase() {
        let dir = TempDir::new().unwrap();
        // pf-4 missing.
        for i in [1, 2, 3, 5] {
            create_sentinel(dir.path(), &format!("pf-{i}"));
        }
        let req = make_req(Some(Phase::Pf5Verify), true, false);
        let outcome = req.check(dir.path(), "", &HashMap::new());
        assert_eq!(outcome, GateCheckOutcome::MissingPhase("pf-4".into()));
    }

    #[test]
    fn test_gate_git_push_missing_pipeline_stage() {
        let dir = TempDir::new().unwrap();
        for i in 1..=5 {
            create_sentinel(dir.path(), &format!("pf-{i}"));
        }
        create_sentinel(dir.path(), "ws-dev");
        // ws-rev missing.

        let mut pipelines = HashMap::new();
        pipelines.insert(
            "FIX".into(),
            vec!["WS-DEV".into(), "WS-REV".into(), "WS-QA".into()],
        );

        let req = make_req(Some(Phase::Pf5Verify), true, true);
        let outcome = req.check(dir.path(), "FIX", &pipelines);
        assert_eq!(outcome, GateCheckOutcome::MissingStage("ws-rev".into()));
    }

    #[test]
    fn test_gate_team_delete_satisfied() {
        let dir = TempDir::new().unwrap();
        create_sentinel(dir.path(), "pf-6");
        create_sentinel(dir.path(), "ws-dev");
        create_sentinel(dir.path(), "ws-rev");

        let mut pipelines = HashMap::new();
        pipelines.insert("DOCS".into(), vec!["WS-DOCS".into(), "WS-REV".into()]);
        // DOCS pipeline needs ws-docs + ws-rev.
        create_sentinel(dir.path(), "ws-docs");

        let req = make_req(Some(Phase::Pf6Complete), false, true);
        assert!(req.is_satisfied(dir.path(), "DOCS", &pipelines));
    }

    #[test]
    fn test_gate_pipeline_driven_docs() {
        // DOCS pipeline is [WS-DOCS, WS-REV] — ws-rev requires only ws-docs,
        // not the full FIX-style ws-dev/ws-sec chain.
        let dir = TempDir::new().unwrap();
        create_sentinel(dir.path(), "pf-5");
        create_sentinel(dir.path(), "ws-docs");
        create_sentinel(dir.path(), "ws-rev");

        let mut pipelines = HashMap::new();
        pipelines.insert("DOCS".into(), vec!["WS-DOCS".into(), "WS-REV".into()]);

        let req = make_req(Some(Phase::Pf5Verify), false, true);
        assert!(req.is_satisfied(dir.path(), "DOCS", &pipelines));
    }

    #[test]
    fn test_gate_missing_work_type() {
        let dir = TempDir::new().unwrap();
        create_sentinel(dir.path(), "pf-5");
        let req = make_req(Some(Phase::Pf5Verify), false, true);
        let outcome = req.check(dir.path(), "", &HashMap::new());
        assert_eq!(outcome, GateCheckOutcome::MissingWorkType);
    }

    #[test]
    fn test_gate_missing_pipeline() {
        let dir = TempDir::new().unwrap();
        create_sentinel(dir.path(), "pf-5");
        let req = make_req(Some(Phase::Pf5Verify), false, true);
        let outcome = req.check(dir.path(), "UNKNOWN", &HashMap::new());
        assert_eq!(outcome, GateCheckOutcome::MissingPipeline("UNKNOWN".into()));
    }

    #[test]
    fn test_gate_no_phase_requirement() {
        let dir = TempDir::new().unwrap();
        // Marker-only gate (on_phase_complete set, but we just check that
        // is_satisfied is true when nothing needs to be verified).
        let req = GateRequirement {
            name: "pr_pushed".into(),
            requires_phase: None,
            on_phase_complete: Some(Phase::Pf6Complete),
            cumulative_phases: false,
            requires_all_pipeline_stages: false,
            description: String::new(),
        };
        assert!(req.is_satisfied(dir.path(), "", &HashMap::new()));
    }

    #[test]
    fn test_gate_config_real_file() {
        // Integration smoke test: load the real project config and verify all
        // gate names resolve correctly.
        let real_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(".codeflow/config/pathflow/pathflow-config.json");
        if !real_path.exists() {
            // Skip in environments without the config (e.g., docs CI).
            return;
        }
        let config = GateConfig::load_from_path(&real_path);
        assert!(config.get("edit_write").is_some());
        assert!(config.get("git_commit").is_some());
        assert!(config.get("role_spawn").is_some());
        assert!(config.get("git_push").is_some());
        assert!(config.get("team_delete").is_some());
    }
}
