//! YAML batch file parsing with validation and topological sorting.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::error::AutorunError;

/// Default maximum concurrent workers.
pub const DEFAULT_MAX_WORKERS: usize = 3;

/// Branches that cannot be auto-merged into.
const PROTECTED_BRANCHES: &[&str] = &["main", "master", "production"];

/// Top-level structure of a YAML batch file.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchFile {
    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub max_workers: usize,

    #[serde(default)]
    pub auto_merge: bool,

    #[serde(default)]
    pub target: String,

    #[serde(default)]
    pub tasks: Vec<TaskSpec>,
}

/// A single task specification in a batch file.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskSpec {
    pub id: String,

    #[serde(default)]
    pub depends_on: Vec<String>,

    /// File patterns this task claims for exclusive access.
    #[serde(default)]
    pub file_scope: Vec<String>,
}

/// Validated and resolved batch specification.
#[derive(Debug, Clone)]
pub struct ParsedBatch {
    pub name: String,
    pub file_path: String,
    pub max_workers: usize,
    pub auto_merge: bool,
    pub target: String,
    pub tasks: Vec<TaskSpec>,
    /// Topologically sorted task IDs (dependencies first).
    pub order: Vec<String>,
}

/// Parse and validate a YAML batch file from disk.
///
/// # Errors
///
/// Returns `AutorunError` on file read failure, YAML parse error,
/// or validation error (empty tasks, duplicates, missing deps, cycles,
/// protected branch merge).
pub fn parse_batch_file(path: &Path) -> Result<ParsedBatch, AutorunError> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| AutorunError::InvalidBatch(format!("reading {}: {e}", path.display())))?;
    parse_batch_data(&data, &path.display().to_string())
}

/// Parse and validate batch YAML data.
///
/// # Errors
///
/// Same as [`parse_batch_file`].
pub fn parse_batch_data(data: &str, file_path: &str) -> Result<ParsedBatch, AutorunError> {
    let bf: BatchFile =
        serde_yaml::from_str(data).map_err(|e| AutorunError::Yaml(e.to_string()))?;

    validate_batch(&bf)?;

    let order = topological_sort(&bf.tasks)?;

    let max_workers = if bf.max_workers == 0 {
        DEFAULT_MAX_WORKERS
    } else {
        bf.max_workers
    };

    let name = if bf.name.is_empty() {
        Path::new(file_path).file_stem().map_or_else(
            || "unnamed".to_string(),
            |s| s.to_string_lossy().to_string(),
        )
    } else {
        bf.name
    };

    Ok(ParsedBatch {
        name,
        file_path: file_path.to_string(),
        max_workers,
        auto_merge: bf.auto_merge,
        target: bf.target,
        tasks: bf.tasks,
        order,
    })
}

/// Validate a batch file for structural and semantic errors.
fn validate_batch(bf: &BatchFile) -> Result<(), AutorunError> {
    if bf.tasks.is_empty() {
        return Err(AutorunError::InvalidBatch("no tasks defined".into()));
    }

    let mut task_ids = HashSet::with_capacity(bf.tasks.len());
    for t in &bf.tasks {
        if t.id.is_empty() {
            return Err(AutorunError::InvalidBatch("task has empty id".into()));
        }
        if !task_ids.insert(&t.id) {
            return Err(AutorunError::InvalidBatch(format!(
                "duplicate task id {:?}",
                t.id
            )));
        }
    }

    // Validate dependency references.
    for t in &bf.tasks {
        for dep in &t.depends_on {
            if !task_ids.contains(dep) {
                return Err(AutorunError::MissingTask(format!(
                    "task {:?} depends on unknown task {:?}",
                    t.id, dep
                )));
            }
            if dep == &t.id {
                return Err(AutorunError::DependencyCycle(format!(
                    "task {:?} depends on itself",
                    t.id
                )));
            }
        }
    }

    // Validate auto_merge + protected branch constraint.
    if bf.auto_merge {
        let target = if bf.target.is_empty() {
            "main"
        } else {
            &bf.target
        };
        for &protected in PROTECTED_BRANCHES {
            if target == protected {
                return Err(AutorunError::ProtectedMerge(format!(
                    "cannot auto_merge into {target:?}"
                )));
            }
        }
        if target.starts_with("release/") {
            return Err(AutorunError::ProtectedMerge(format!(
                "cannot auto_merge into {target:?}"
            )));
        }
    }

    Ok(())
}

/// Topological sort via Kahn's algorithm with deterministic alphabetical ordering.
///
/// # Errors
///
/// Returns `AutorunError::DependencyCycle` if a cycle is detected.
fn topological_sort(tasks: &[TaskSpec]) -> Result<Vec<String>, AutorunError> {
    let mut in_degree: HashMap<&str, usize> = HashMap::with_capacity(tasks.len());
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::with_capacity(tasks.len());

    for t in tasks {
        in_degree.entry(&t.id).or_insert(0);
        for dep in &t.depends_on {
            dependents.entry(dep.as_str()).or_default().push(&t.id);
            *in_degree.entry(&t.id).or_insert(0) += 1;
        }
    }

    // Seed the queue with zero in-degree nodes, sorted alphabetically.
    let mut queue: Vec<&str> = tasks
        .iter()
        .filter(|t| *in_degree.get(t.id.as_str()).unwrap_or(&0) == 0)
        .map(|t| t.id.as_str())
        .collect();
    queue.sort_unstable();

    let mut order = Vec::with_capacity(tasks.len());
    while let Some(node) = queue.first().copied() {
        queue.remove(0);
        order.push(node.to_string());

        if let Some(deps) = dependents.get(node) {
            let mut ready = Vec::new();
            for &dep in deps {
                if let Some(deg) = in_degree.get_mut(dep) {
                    *deg -= 1;
                    if *deg == 0 {
                        ready.push(dep);
                    }
                }
            }
            ready.sort_unstable();
            queue.extend(ready);
        }
    }

    if order.len() != tasks.len() {
        let mut cycled: Vec<&str> = in_degree
            .iter()
            .filter(|(_, deg)| **deg > 0)
            .map(|(id, _)| *id)
            .collect();
        cycled.sort_unstable();
        return Err(AutorunError::DependencyCycle(format!(
            "tasks involved: {}",
            cycled.join(", ")
        )));
    }

    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_batch() {
        let yaml = r#"
name: test-batch
max_workers: 2
tasks:
  - id: task-a
  - id: task-b
    depends_on: [task-a]
"#;
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert_eq!(batch.name, "test-batch");
        assert_eq!(batch.max_workers, 2);
        assert_eq!(batch.order, vec!["task-a", "task-b"]);
    }

    #[test]
    fn test_parse_default_max_workers() {
        let yaml = "tasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert_eq!(batch.max_workers, DEFAULT_MAX_WORKERS);
    }

    #[test]
    fn test_parse_default_name_from_filename() {
        let yaml = "tasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "my-batch.yaml").unwrap();
        assert_eq!(batch.name, "my-batch");
    }

    #[test]
    fn test_validate_empty_tasks() {
        let yaml = "tasks: []\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no tasks defined"));
    }

    #[test]
    fn test_validate_empty_task_id() {
        let yaml = "tasks:\n  - id: \"\"\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty id"));
    }

    #[test]
    fn test_validate_duplicate_task_id() {
        let yaml = "tasks:\n  - id: task-a\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("duplicate"));
    }

    #[test]
    fn test_validate_missing_dependency() {
        let yaml = "tasks:\n  - id: task-a\n    depends_on: [task-z]\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown task"));
    }

    #[test]
    fn test_validate_self_dependency() {
        let yaml = "tasks:\n  - id: task-a\n    depends_on: [task-a]\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("depends on itself")
        );
    }

    #[test]
    fn test_validate_protected_branch_main() {
        let yaml = "auto_merge: true\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("main"));
    }

    #[test]
    fn test_validate_protected_branch_explicit() {
        let yaml = "auto_merge: true\ntarget: master\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("master"));
    }

    #[test]
    fn test_validate_protected_branch_release() {
        let yaml = "auto_merge: true\ntarget: release/v1.0\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("release/v1.0"));
    }

    #[test]
    fn test_validate_auto_merge_non_protected() {
        let yaml = "auto_merge: true\ntarget: develop\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_ok());
    }

    #[test]
    fn test_topological_sort_linear() {
        let tasks = vec![
            TaskSpec {
                id: "c".into(),
                depends_on: vec!["b".into()],
                file_scope: vec![],
            },
            TaskSpec {
                id: "a".into(),
                depends_on: vec![],
                file_scope: vec![],
            },
            TaskSpec {
                id: "b".into(),
                depends_on: vec!["a".into()],
                file_scope: vec![],
            },
        ];
        let order = topological_sort(&tasks).unwrap();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_topological_sort_parallel() {
        let tasks = vec![
            TaskSpec {
                id: "b".into(),
                depends_on: vec![],
                file_scope: vec![],
            },
            TaskSpec {
                id: "a".into(),
                depends_on: vec![],
                file_scope: vec![],
            },
            TaskSpec {
                id: "c".into(),
                depends_on: vec![],
                file_scope: vec![],
            },
        ];
        let order = topological_sort(&tasks).unwrap();
        // Alphabetical ordering for determinism.
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_topological_sort_diamond() {
        let tasks = vec![
            TaskSpec {
                id: "a".into(),
                depends_on: vec![],
                file_scope: vec![],
            },
            TaskSpec {
                id: "b".into(),
                depends_on: vec!["a".into()],
                file_scope: vec![],
            },
            TaskSpec {
                id: "c".into(),
                depends_on: vec!["a".into()],
                file_scope: vec![],
            },
            TaskSpec {
                id: "d".into(),
                depends_on: vec!["b".into(), "c".into()],
                file_scope: vec![],
            },
        ];
        let order = topological_sort(&tasks).unwrap();
        assert_eq!(order[0], "a");
        assert_eq!(order[3], "d");
        // b and c can be in either order, but sorted alphabetically.
        assert_eq!(order[1], "b");
        assert_eq!(order[2], "c");
    }

    #[test]
    fn test_topological_sort_cycle() {
        let tasks = vec![
            TaskSpec {
                id: "a".into(),
                depends_on: vec!["b".into()],
                file_scope: vec![],
            },
            TaskSpec {
                id: "b".into(),
                depends_on: vec!["a".into()],
                file_scope: vec![],
            },
        ];
        let result = topological_sort(&tasks);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("a"));
        assert!(err.contains("b"));
    }

    #[test]
    fn test_parse_batch_file_nonexistent() {
        let result = parse_batch_file(Path::new("/nonexistent/batch.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_batch_file_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        std::fs::write(
            &batch_path,
            "name: disk-test\ntasks:\n  - id: t1\n  - id: t2\n    depends_on: [t1]\n",
        )
        .unwrap();
        let batch = parse_batch_file(&batch_path).unwrap();
        assert_eq!(batch.name, "disk-test");
        assert_eq!(batch.order, vec!["t1", "t2"]);
    }

    #[test]
    fn test_parse_invalid_yaml() {
        let result = parse_batch_data("{{invalid yaml", "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("yaml"));
    }

    #[test]
    fn test_validate_max_workers_accepted() {
        let yaml = "max_workers: 5\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().max_workers, 5);
    }

    #[test]
    fn test_validate_max_workers_at_default() {
        let yaml = "max_workers: 3\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().max_workers, 3);
    }

    #[test]
    fn test_validate_max_workers_zero_defaults() {
        let yaml = "max_workers: 0\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().max_workers,
            DEFAULT_MAX_WORKERS,
            "max_workers=0 should default to DEFAULT_MAX_WORKERS"
        );
    }

    #[test]
    fn test_task_spec_file_scope_default() {
        let yaml = "tasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert!(
            batch.tasks[0].file_scope.is_empty(),
            "file_scope should default to empty"
        );
    }

    #[test]
    fn test_task_spec_file_scope_parsed() {
        let yaml = "tasks:\n  - id: task-a\n    file_scope:\n      - \"src/**/*.rs\"\n      - \"tests/\"\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert_eq!(batch.tasks[0].file_scope, vec!["src/**/*.rs", "tests/"]);
    }

    #[test]
    fn test_validate_max_workers_one_below_limit() {
        let yaml = "max_workers: 2\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().max_workers, 2);
    }

    #[test]
    fn test_validate_max_workers_above_default() {
        let yaml = "max_workers: 4\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().max_workers, 4);
    }

    #[test]
    fn test_validate_max_workers_large_value() {
        let yaml = "max_workers: 10\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().max_workers, 10);
    }

    #[test]
    fn test_validate_max_workers_very_large_value() {
        let yaml = "max_workers: 20\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().max_workers, 20);
    }
}
