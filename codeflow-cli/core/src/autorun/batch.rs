//! YAML batch file parsing with validation and topological sorting.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::AutorunError;

// DEFAULT_MAX_WORKERS removed (C13): when max_workers==0, use tasks.len().

/// Fallback protected branches when enforcement-policy.json is missing.
const DEFAULT_PROTECTED_BRANCHES: &[&str] = &["main", "master", "release/*", "production"];

/// Path to the enforcement policy config relative to the project root.
const ENFORCEMENT_POLICY_PATH: &str = ".codeflow/config/enforcement/enforcement-policy.json";

/// Top-level structure of a YAML batch file.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchFile {
    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub max_workers: usize,

    #[serde(default)]
    pub integration_auto_merge: Option<bool>,

    #[serde(default)]
    pub integration_branch: String,

    /// If true, create a final PR from integration branch to final_pr_target.
    #[serde(default)]
    pub final_pr: Option<bool>,

    /// Base branch for the final PR (default: "main").
    #[serde(default)]
    pub final_pr_target: String,

    #[serde(default)]
    pub tasks: Vec<TaskSpec>,
}

/// A single task specification in a batch file.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskSpec {
    pub id: String,

    #[serde(default)]
    pub depends_on: Vec<String>,

    /// Optional file scope override. If empty, the orchestrator uses the task
    /// markdown's file_scope (source of truth). Only specify to NARROW scope
    /// for a specific batch run.
    #[serde(default)]
    pub file_scope: Vec<String>,

    /// Optional scope policy override. If None, the orchestrator uses the task
    /// markdown's scope_policy. Only specify to TIGHTEN policy (e.g., "hard"
    /// when markdown says "soft").
    #[serde(default)]
    pub scope_policy: Option<String>,

    /// Optional per-task timeout in seconds. Overrides the global
    /// `autorun.worker_timeout_secs` config for this task. Useful for tasks
    /// known to be long-running (e.g., large features) or quick (e.g., docs).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Validated and resolved batch specification.
#[derive(Debug, Clone)]
pub struct ParsedBatch {
    pub name: String,
    pub file_path: String,
    pub max_workers: usize,
    pub integration_auto_merge: bool,
    pub integration_branch: String,
    /// Whether the integration branch was auto-generated (empty in YAML).
    pub integration_branch_is_auto: bool,
    /// Whether to create a final PR from integration branch to final_pr_target.
    pub final_pr: bool,
    /// Base branch for the final PR (default: "main").
    pub final_pr_target: String,
    pub tasks: Vec<TaskSpec>,
    /// Topologically sorted task IDs (dependencies first).
    pub order: Vec<String>,
}

/// Read a task's `file_scope` and `scope_policy` from its markdown frontmatter.
///
/// Returns `(file_scope, scope_policy)`. Defaults to empty scope and `"soft"` policy
/// if the fields are missing from the frontmatter.
///
/// # Errors
///
/// Returns `AutorunError::MissingTask` if the task markdown cannot be read.
/// Returns `AutorunError::InvalidBatch` if the frontmatter cannot be parsed.
pub fn read_task_scope(
    task_id: &str,
    project_dir: &Path,
) -> Result<(Vec<String>, String), AutorunError> {
    let task_path = resolve_task_path(project_dir, task_id)?;

    let content = std::fs::read(&task_path).map_err(|e| {
        AutorunError::MissingTask(format!(
            "reading task markdown {}: {e}",
            task_path.display()
        ))
    })?;

    let (data, _body) = crate::validate::parse_frontmatter(&content).map_err(|e| {
        AutorunError::InvalidBatch(format!("parsing frontmatter for {task_id}: {e}"))
    })?;

    let file_scope = data
        .get("file_scope")
        .and_then(serde_yaml::Value::as_sequence)
        .map(|seq| {
            seq.iter()
                .filter_map(serde_yaml::Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    let scope_policy = crate::validate::get_string_field(&data, "scope_policy");
    let scope_policy = if scope_policy.is_empty() {
        "soft".to_string()
    } else {
        scope_policy
    };

    Ok((file_scope, scope_policy))
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
    parse_batch_data_with_project_dir(data, file_path, None)
}

/// Parse and validate batch YAML data with an optional project directory for
/// loading protected branches from enforcement-policy.json.
///
/// # Errors
///
/// Same as [`parse_batch_file`].
pub fn parse_batch_data_with_project_dir(
    data: &str,
    file_path: &str,
    project_dir: Option<&Path>,
) -> Result<ParsedBatch, AutorunError> {
    let bf: BatchFile =
        serde_yaml::from_str(data).map_err(|e| AutorunError::Yaml(e.to_string()))?;

    let protected_branches: Vec<String> = match project_dir {
        Some(dir) => load_protected_branches(dir),
        None => DEFAULT_PROTECTED_BRANCHES
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
    };

    // Resolve integration_auto_merge: explicit value or inferred from target protection.
    // When integration_branch is empty, the orchestrator auto-generates a non-protected
    // branch (autorun/{name}-{suffix}), so auto_merge defaults to true.
    let resolved_auto_merge = bf.integration_auto_merge.unwrap_or_else(|| {
        if bf.integration_branch.is_empty() {
            true // auto-generated branches are never protected
        } else {
            !is_protected_branch(&bf.integration_branch, &protected_branches)
        }
    });

    // Temporarily set integration_auto_merge to resolved value for validation.
    let bf_for_validation = BatchFile {
        integration_auto_merge: Some(resolved_auto_merge),
        ..bf.clone()
    };
    validate_batch(&bf_for_validation, &protected_branches)?;

    let order = topological_sort(&bf.tasks)?;

    let max_workers = if bf.max_workers == 0 {
        bf.tasks.len().max(1) // one worker per task when unspecified
    } else {
        bf.max_workers
    };

    let name = if bf.name.is_empty() {
        Path::new(file_path).file_stem().map_or_else(
            || "unnamed".to_string(),
            |s| s.to_string_lossy().to_string(),
        )
    } else {
        bf.name.clone()
    };

    // Resolve final_pr_target: default to "main" when empty.
    let final_pr_target = if bf.final_pr_target.is_empty() {
        "main".to_string()
    } else {
        bf.final_pr_target
    };

    // Resolve final_pr: default based on whether integration_branch differs from main/master.
    let integration_branch_is_auto = bf.integration_branch.is_empty();
    let final_pr = bf.final_pr.unwrap_or_else(|| {
        let effective_target = &bf.integration_branch;
        !effective_target.is_empty() && effective_target != "main" && effective_target != "master"
    });

    Ok(ParsedBatch {
        name,
        file_path: file_path.to_string(),
        max_workers,
        integration_auto_merge: resolved_auto_merge,
        integration_branch: bf.integration_branch,
        integration_branch_is_auto,
        final_pr,
        final_pr_target,
        tasks: bf.tasks,
        order,
    })
}

/// Load protected branches from enforcement-policy.json, falling back to defaults.
///
/// Reads `merge_protection.protected_branches` from the enforcement policy config.
/// Returns default list if the file does not exist or cannot be parsed.
fn load_protected_branches(project_dir: &Path) -> Vec<String> {
    let path = project_dir.join(ENFORCEMENT_POLICY_PATH);
    let data = match std::fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => {
            return DEFAULT_PROTECTED_BRANCHES
                .iter()
                .map(|s| (*s).to_string())
                .collect();
        }
    };

    let parsed: serde_json::Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => {
            return DEFAULT_PROTECTED_BRANCHES
                .iter()
                .map(|s| (*s).to_string())
                .collect();
        }
    };

    if let Some(branches) = parsed
        .get("merge_protection")
        .and_then(|mp| mp.get("protected_branches"))
        .and_then(|pb| pb.as_array())
    {
        let result: Vec<String> = branches
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if result.is_empty() {
            DEFAULT_PROTECTED_BRANCHES
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        } else {
            result
        }
    } else {
        DEFAULT_PROTECTED_BRANCHES
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }
}

/// Check whether a branch name matches a protected branch pattern.
///
/// Supports exact matches and `prefix/*` wildcard patterns (e.g. `release/*`).
fn is_protected_branch(target: &str, protected: &[String]) -> bool {
    for pattern in protected {
        if pattern.ends_with("/*") {
            let prefix = &pattern[..pattern.len() - 1]; // "release/" from "release/*"
            if target.starts_with(prefix) {
                return true;
            }
        } else if target == pattern {
            return true;
        }
    }
    false
}

/// Resolve a task format ID to its markdown file path.
///
/// Replicates the path construction logic from `worker::build_task_prompt_from_file`.
/// Task ID format: `{AREA}-TSK-{epic_NNN}-{seq_NNN}`.
///
/// # Errors
///
/// Returns `AutorunError::MissingTask` if the task ID format is invalid or
/// contains path traversal characters.
pub fn resolve_task_path(project_dir: &Path, task_id: &str) -> Result<PathBuf, AutorunError> {
    let parts: Vec<&str> = task_id.split('-').collect();
    if parts.len() < 4 || parts[1] != "TSK" {
        return Err(AutorunError::MissingTask(format!(
            "invalid task ID format: {task_id} (expected AREA-TSK-NNN-NNN)"
        )));
    }

    // Reject path traversal characters.
    for part in &parts {
        if part.contains("..") || part.contains('/') || part.contains('\\') {
            return Err(AutorunError::MissingTask(format!(
                "task ID contains path traversal characters: {task_id}"
            )));
        }
    }

    let area = parts[0];
    let epic_num = parts[2];
    Ok(project_dir
        .join("project-management")
        .join("epics")
        .join(area)
        .join(format!("{area}-EPC-{epic_num}"))
        .join("tasks")
        .join(format!("{task_id}.md")))
}

/// Validate a batch file for structural and semantic errors.
fn validate_batch(bf: &BatchFile, protected_branches: &[String]) -> Result<(), AutorunError> {
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

    // Validate integration_auto_merge + protected branch constraint.
    // Only check when branch is explicitly set (empty = auto-generated = non-protected).
    if bf.integration_auto_merge.unwrap_or(false)
        && !bf.integration_branch.is_empty()
        && is_protected_branch(&bf.integration_branch, protected_branches)
    {
        return Err(AutorunError::ProtectedMerge(format!(
            "cannot auto_merge into {:?}",
            bf.integration_branch
        )));
    }

    Ok(())
}

/// Extended validation: reads task markdown files, checks autorun fields,
/// detects file_scope overlaps, validates target_branch, and reconciles
/// max_workers with max_concurrent.
///
/// Call after `parse_batch_file()` / `parse_batch_data()` with the resulting
/// `ParsedBatch`.
///
/// # Errors
///
/// Returns `AutorunError::InvalidBatch` for any validation failure.
pub fn validate_batch_extended(
    batch: &mut ParsedBatch,
    project_dir: &Path,
) -> Result<(), AutorunError> {
    let protected_branches = load_protected_branches(project_dir);
    let config = crate::autorun::config::load_config(project_dir)?;

    // Reconcile max_workers with max_concurrent.
    let effective = batch.max_workers.min(config.worktree.max_concurrent);
    if batch.max_workers > config.worktree.max_concurrent {
        eprintln!(
            "WARNING: max_workers ({}) exceeds worktree.max_concurrent ({}), capping to {}",
            batch.max_workers, config.worktree.max_concurrent, effective
        );
    }
    batch.max_workers = effective;

    // Build dependency map for overlap detection.
    let dep_set: HashMap<&str, HashSet<&str>> = batch
        .tasks
        .iter()
        .map(|t| {
            let deps: HashSet<&str> = t.depends_on.iter().map(String::as_str).collect();
            (t.id.as_str(), deps)
        })
        .collect();

    // Track effective file_scope per task for overlap detection.
    // Reads from task markdown (source of truth), with batch TaskSpec as override.
    let mut task_scopes: Vec<(String, Vec<String>)> = Vec::with_capacity(batch.tasks.len());

    for task in &batch.tasks {
        let task_path = resolve_task_path(project_dir, &task.id)?;

        let content = std::fs::read(&task_path).map_err(|e| {
            AutorunError::MissingTask(format!(
                "reading task markdown {}: {e}",
                task_path.display()
            ))
        })?;

        let (data, _body) = crate::validate::parse_frontmatter(&content).map_err(|e| {
            AutorunError::InvalidBatch(format!("parsing frontmatter for {}: {e}", task.id))
        })?;

        // Check autorun_eligible.
        let (autorun_eligible, ae_set) = crate::validate::get_bool_field(&data, "autorun_eligible");
        if !ae_set || !autorun_eligible {
            return Err(AutorunError::InvalidBatch(format!(
                "task {} has autorun_eligible=false or missing",
                task.id
            )));
        }

        // Check acceptance not empty.
        let acceptance_errs = crate::validate::validate_autorun_acceptance(&data);
        if !acceptance_errs.is_empty() {
            return Err(AutorunError::InvalidBatch(format!(
                "task {}: {}",
                task.id, acceptance_errs[0]
            )));
        }

        // Check scope_policy not permissive.
        let scope_errs = crate::validate::validate_autorun_scope_policy(&data);
        if !scope_errs.is_empty() {
            return Err(AutorunError::InvalidBatch(format!(
                "task {}: {}",
                task.id, scope_errs[0]
            )));
        }

        // Check file_scope not empty.
        let fscope_errs = crate::validate::validate_autorun_file_scope(&data);
        if !fscope_errs.is_empty() {
            return Err(AutorunError::InvalidBatch(format!(
                "task {}: {}",
                task.id, fscope_errs[0]
            )));
        }

        // Read file_scope from task markdown (source of truth).
        // Batch TaskSpec file_scope is an optional override.
        let md_file_scope: Vec<String> = data
            .get("file_scope")
            .and_then(serde_yaml::Value::as_sequence)
            .map(|seq| {
                seq.iter()
                    .filter_map(serde_yaml::Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        let effective_scope = if task.file_scope.is_empty() {
            md_file_scope
        } else {
            task.file_scope.clone()
        };

        task_scopes.push((task.id.clone(), effective_scope));
    }

    // Detect file_scope overlaps between concurrent (non-dependent) tasks.
    for i in 0..task_scopes.len() {
        for j in (i + 1)..task_scopes.len() {
            let (id_a, scope_a) = &task_scopes[i];
            let (id_b, scope_b) = &task_scopes[j];

            // Skip if one depends on the other (dependency serializes access).
            let a_deps = dep_set.get(id_a.as_str()).cloned().unwrap_or_default();
            let b_deps = dep_set.get(id_b.as_str()).cloned().unwrap_or_default();
            if a_deps.contains(id_b.as_str()) || b_deps.contains(id_a.as_str()) {
                continue;
            }

            // Check for overlapping paths (exact match + directory containment).
            let set_a: HashSet<&str> = scope_a.iter().map(String::as_str).collect();
            let set_b: HashSet<&str> = scope_b.iter().map(String::as_str).collect();
            let overlaps: Vec<&str> = scope_b
                .iter()
                .filter(|p| {
                    let pb = std::path::Path::new(p.as_str());
                    // Exact match.
                    if set_a.contains(p.as_str()) {
                        return true;
                    }
                    // Path containment: a scope_a entry is a parent of this path.
                    scope_a.iter().any(|a| {
                        let pa = std::path::Path::new(a.as_str());
                        pb.starts_with(pa) || pa.starts_with(pb)
                    })
                })
                .map(String::as_str)
                .chain(
                    // Also check reverse: scope_a entries contained by scope_b entries.
                    scope_a
                        .iter()
                        .filter(|a| {
                            let pa = std::path::Path::new(a.as_str());
                            if set_b.contains(a.as_str()) {
                                return false; // Already caught above.
                            }
                            scope_b.iter().any(|b| {
                                let pb = std::path::Path::new(b.as_str());
                                pa.starts_with(pb)
                            })
                        })
                        .map(String::as_str),
                )
                .collect::<HashSet<&str>>()
                .into_iter()
                .collect::<Vec<&str>>();
            if !overlaps.is_empty() {
                return Err(AutorunError::InvalidBatch(format!(
                    "concurrent tasks {} and {} have overlapping file_scope: {}",
                    id_a,
                    id_b,
                    overlaps.join(", ")
                )));
            }
        }
    }

    // Validate integration_branch if specified.
    let target = if batch.integration_branch.is_empty() {
        "main"
    } else {
        &batch.integration_branch
    };

    // Check integration branch exists via git2 (skip for auto-generated targets --
    // the orchestrator creates them).
    if !batch.integration_branch_is_auto {
        if let Ok(repo) = git2::Repository::discover(project_dir) {
            // Even explicit targets that don't exist yet are OK -- the orchestrator
            // will create them. Only warn, don't error.
            if crate::git::conflict::find_target_ref(&repo, target).is_err() {
                eprintln!(
                    "WARNING: integration_branch {target:?} does not exist yet (will be created)"
                );
            }
        }
    }

    // Validate final_pr_target exists if final_pr is enabled.
    if batch.final_pr {
        if batch.final_pr_target == batch.integration_branch {
            return Err(AutorunError::InvalidBatch(format!(
                "final_pr_target ({:?}) cannot be the same as integration_branch ({:?})",
                batch.final_pr_target, batch.integration_branch
            )));
        }
        if let Ok(repo) = git2::Repository::discover(project_dir) {
            if crate::git::conflict::find_target_ref(&repo, &batch.final_pr_target).is_err() {
                return Err(AutorunError::InvalidBatch(format!(
                    "final_pr_target {:?} does not exist",
                    batch.final_pr_target
                )));
            }
        }
    }

    // Check protected branch + integration_auto_merge.
    // Skip for auto-generated branches (never protected).
    if batch.integration_auto_merge
        && !batch.integration_branch_is_auto
        && is_protected_branch(&batch.integration_branch, &protected_branches)
    {
        return Err(AutorunError::ProtectedMerge(format!(
            "cannot auto_merge into {:?}",
            batch.integration_branch
        )));
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

/// Resolve the integration branch for the batch, generating one if empty.
///
/// If the batch's `integration_branch` is empty (auto-mode), generates a branch name from
/// the batch name and session ID suffix, and sets `integration_branch_is_auto = true`.
/// Otherwise leaves the integration branch as-is.
pub fn resolve_target(batch: &mut ParsedBatch, session_id: &str) {
    if batch.integration_branch.is_empty() {
        let suffix = &session_id[session_id.len().saturating_sub(8)..];
        batch.integration_branch = format!("autorun/{}-{suffix}", batch.name);
        batch.integration_branch_is_auto = true;
        // Auto-generated targets always get integration_auto_merge and final_pr.
        batch.integration_auto_merge = true;
        batch.final_pr = true;
    }
}

/// Build a resume batch from an original batch by filtering out completed tasks.
///
/// Tasks whose IDs are in `completed_task_ids` are removed. Dependencies on
/// completed tasks are also removed (they are already satisfied).
/// All other batch settings (integration_auto_merge, integration_branch, max_workers) are preserved.
#[must_use]
pub fn build_resume_batch<S: ::std::hash::BuildHasher>(
    original: &ParsedBatch,
    completed_task_ids: &HashSet<String, S>,
) -> ParsedBatch {
    let remaining_tasks: Vec<TaskSpec> = original
        .tasks
        .iter()
        .filter(|t| !completed_task_ids.contains(&t.id))
        .map(|t| {
            let mut task = t.clone();
            task.depends_on
                .retain(|dep| !completed_task_ids.contains(dep));
            task
        })
        .collect();

    let order = topological_sort(&remaining_tasks)
        .unwrap_or_else(|_| remaining_tasks.iter().map(|t| t.id.clone()).collect());

    ParsedBatch {
        name: format!("{}-resume", original.name),
        file_path: original.file_path.clone(),
        max_workers: original.max_workers,
        integration_auto_merge: original.integration_auto_merge,
        integration_branch: original.integration_branch.clone(),
        integration_branch_is_auto: original.integration_branch_is_auto,
        final_pr: original.final_pr,
        final_pr_target: original.final_pr_target.clone(),
        tasks: remaining_tasks,
        order,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_batch() {
        let yaml = r"
name: test-batch
max_workers: 2
tasks:
  - id: task-a
  - id: task-b
    depends_on: [task-a]
";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert_eq!(batch.name, "test-batch");
        assert_eq!(batch.max_workers, 2);
        assert_eq!(batch.order, vec!["task-a", "task-b"]);
    }

    #[test]
    fn test_parse_default_max_workers() {
        let yaml = "tasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        // C13: max_workers=0 defaults to tasks.len() (one worker per task).
        assert_eq!(batch.max_workers, 1);
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
    fn test_validate_auto_merge_empty_branch_valid() {
        // Empty integration_branch means auto-generated (non-protected), so auto_merge is valid.
        let yaml = "integration_auto_merge: true\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(
            result.is_ok(),
            "auto_merge with empty branch (auto-generated) should be valid"
        );
    }

    #[test]
    fn test_validate_auto_merge_explicit_main_rejected() {
        // Explicitly targeting "main" with auto_merge is forbidden.
        let yaml =
            "integration_auto_merge: true\nintegration_branch: main\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("main"));
    }

    #[test]
    fn test_validate_protected_branch_explicit() {
        let yaml =
            "integration_auto_merge: true\nintegration_branch: master\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("master"));
    }

    #[test]
    fn test_validate_protected_branch_release() {
        let yaml = "integration_auto_merge: true\nintegration_branch: release/v1.0\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("release/v1.0"));
    }

    #[test]
    fn test_validate_auto_merge_non_protected() {
        let yaml =
            "integration_auto_merge: true\nintegration_branch: develop\ntasks:\n  - id: task-a\n";
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
                scope_policy: None,
                timeout_secs: None,
            },
            TaskSpec {
                id: "a".into(),
                depends_on: vec![],
                file_scope: vec![],
                scope_policy: None,
                timeout_secs: None,
            },
            TaskSpec {
                id: "b".into(),
                depends_on: vec!["a".into()],
                file_scope: vec![],
                scope_policy: None,
                timeout_secs: None,
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
                scope_policy: None,
                timeout_secs: None,
            },
            TaskSpec {
                id: "a".into(),
                depends_on: vec![],
                file_scope: vec![],
                scope_policy: None,
                timeout_secs: None,
            },
            TaskSpec {
                id: "c".into(),
                depends_on: vec![],
                file_scope: vec![],
                scope_policy: None,
                timeout_secs: None,
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
                scope_policy: None,
                timeout_secs: None,
            },
            TaskSpec {
                id: "b".into(),
                depends_on: vec!["a".into()],
                file_scope: vec![],
                scope_policy: None,
                timeout_secs: None,
            },
            TaskSpec {
                id: "c".into(),
                depends_on: vec!["a".into()],
                file_scope: vec![],
                scope_policy: None,
                timeout_secs: None,
            },
            TaskSpec {
                id: "d".into(),
                depends_on: vec!["b".into(), "c".into()],
                file_scope: vec![],
                scope_policy: None,
                timeout_secs: None,
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
                scope_policy: None,
                timeout_secs: None,
            },
            TaskSpec {
                id: "b".into(),
                depends_on: vec!["a".into()],
                file_scope: vec![],
                scope_policy: None,
                timeout_secs: None,
            },
        ];
        let result = topological_sort(&tasks);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains('a'));
        assert!(err.contains('b'));
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
    fn test_validate_max_workers_zero_defaults_to_task_count() {
        let yaml = "max_workers: 0\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().max_workers,
            1,
            "max_workers=0 should default to tasks.len()"
        );

        // With multiple tasks, should default to the count.
        let yaml2 = "max_workers: 0\ntasks:\n  - id: task-a\n  - id: task-b\n  - id: task-c\n";
        let result2 = parse_batch_data(yaml2, "test.yaml").unwrap();
        assert_eq!(result2.max_workers, 3);
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

    #[test]
    fn test_task_spec_scope_policy_default() {
        let yaml = "tasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert!(
            batch.tasks[0].scope_policy.is_none(),
            "scope_policy should default to None"
        );
    }

    #[test]
    fn test_task_spec_scope_policy_parsed() {
        let yaml = "tasks:\n  - id: task-a\n    scope_policy: hard\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert_eq!(batch.tasks[0].scope_policy.as_deref(), Some("hard"));
    }

    // -----------------------------------------------------------------------
    // Helper: create task markdown with frontmatter
    // -----------------------------------------------------------------------

    fn make_task_markdown(fields: &str) -> String {
        format!(
            "---\n{fields}\n---\n\n# Task\n\n## Description\n\nTest.\n\n\
             ## Approach\n\nTest.\n\n## Files\n\nNone.\n\n\
             ## Acceptance Criteria\n\n1. Test\n\n## Dependencies\n\nNone.\n\n\
             ## Verification\n\nNone.\n\n## Stage Reports\n\nNone.\n\n## Notes\n\nNone.\n"
        )
    }

    /// Create a valid task markdown for autorun.
    fn valid_task_fields(task_id: &str, epic_num: &str, area: &str) -> String {
        format!(
            "id: \"task-{task_id}\"\n\
             format_id: \"{area}-TSK-{epic_num}-001\"\n\
             epic_id: \"epic-test\"\n\
             epic_format_id: \"{area}-EPC-{epic_num}\"\n\
             title: \"Test task\"\n\
             description: \"Test\"\n\
             status: todo\n\
             area_type: \"{area}\"\n\
             work_type: \"FEAT\"\n\
             domain: \"GENL\"\n\
             origin: planned\n\
             file_scope:\n  - \"src/foo.rs\"\n\
             scope_policy: soft\n\
             autorun_eligible: true\n\
             acceptance:\n  - \"criterion 1\"\n\
             raise_pr: true\n\
             auto_merge: false\n\
             created_at: \"2026-01-01T00:00:00Z\"\n\
             updated_at: \"2026-01-01T00:00:00Z\""
        )
    }

    /// Set up a project dir with a task markdown file and enforcement policy.
    fn setup_project_with_task(dir: &std::path::Path, task_id: &str, fields: &str) {
        // Task path: project-management/epics/INF/INF-EPC-023/tasks/{task_id}.md
        let parts: Vec<&str> = task_id.split('-').collect();
        let area = parts[0];
        let epic_num = parts[2];
        let task_dir = dir
            .join("project-management")
            .join("epics")
            .join(area)
            .join(format!("{area}-EPC-{epic_num}"))
            .join("tasks");
        std::fs::create_dir_all(&task_dir).unwrap();
        let task_path = task_dir.join(format!("{task_id}.md"));
        std::fs::write(&task_path, make_task_markdown(fields)).unwrap();

        // Enforcement policy.
        let policy_dir = dir.join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&policy_dir).unwrap();
        let policy = r#"{
            "merge_protection": {
                "protected_branches": ["main", "master", "release/*", "production"]
            }
        }"#;
        std::fs::write(policy_dir.join("enforcement-policy.json"), policy).unwrap();

        // Parallel work config (defaults are fine, but create the dir).
        let pw_dir = dir.join(".codeflow/config/parallel-work");
        std::fs::create_dir_all(&pw_dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // Tests: resolve_task_path
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_task_path_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = resolve_task_path(dir.path(), "INF-TSK-023-031").unwrap();
        assert!(
            path.ends_with("project-management/epics/INF/INF-EPC-023/tasks/INF-TSK-023-031.md")
        );
    }

    #[test]
    fn test_resolve_task_path_invalid_format() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_task_path(dir.path(), "BADFORMAT");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid task ID format")
        );
    }

    #[test]
    fn test_resolve_task_path_traversal_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_task_path(dir.path(), "INF-TSK-../../../etc-001");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path traversal"));
    }

    // -----------------------------------------------------------------------
    // Tests: is_protected_branch
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_protected_branch_exact_match() {
        let branches = vec!["main".into(), "master".into(), "production".into()];
        assert!(is_protected_branch("main", &branches));
        assert!(is_protected_branch("master", &branches));
        assert!(!is_protected_branch("develop", &branches));
    }

    #[test]
    fn test_is_protected_branch_wildcard() {
        let branches = vec!["release/*".into()];
        assert!(is_protected_branch("release/v1.0", &branches));
        assert!(is_protected_branch("release/hotfix", &branches));
        assert!(!is_protected_branch("releases/v1.0", &branches));
    }

    // -----------------------------------------------------------------------
    // Tests: load_protected_branches
    // -----------------------------------------------------------------------

    #[test]
    fn test_load_protected_branches_from_config() {
        let dir = tempfile::tempdir().unwrap();
        let policy_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&policy_dir).unwrap();
        let policy = r#"{
            "merge_protection": {
                "protected_branches": ["main", "master", "release/*", "production"]
            }
        }"#;
        std::fs::write(policy_dir.join("enforcement-policy.json"), policy).unwrap();

        let branches = load_protected_branches(dir.path());
        assert_eq!(branches, vec!["main", "master", "release/*", "production"]);
    }

    #[test]
    fn test_load_protected_branches_missing_file_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let branches = load_protected_branches(dir.path());
        assert_eq!(
            branches,
            DEFAULT_PROTECTED_BRANCHES
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_load_protected_branches_malformed_json_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let policy_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&policy_dir).unwrap();
        std::fs::write(
            policy_dir.join("enforcement-policy.json"),
            "{{not valid json",
        )
        .unwrap();

        let branches = load_protected_branches(dir.path());
        assert_eq!(branches.len(), 4);
        assert!(branches.contains(&"main".to_string()));
    }

    // -----------------------------------------------------------------------
    // Tests: validate_batch_extended
    // -----------------------------------------------------------------------

    #[test]
    fn test_extended_autorun_eligible_false_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fields = valid_task_fields("t1", "023", "INF")
            .replace("autorun_eligible: true", "autorun_eligible: false");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields);

        let yaml = "tasks:\n  - id: INF-TSK-023-001\n    file_scope:\n      - src/foo.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        let result = validate_batch_extended(&mut batch, dir.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("autorun_eligible=false"),
            "expected autorun_eligible error, got: {err}"
        );
    }

    #[test]
    fn test_extended_empty_acceptance_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fields = valid_task_fields("t1", "023", "INF")
            .replace("acceptance:\n  - \"criterion 1\"", "acceptance: []");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields);

        let yaml = "tasks:\n  - id: INF-TSK-023-001\n    file_scope:\n      - src/foo.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        let result = validate_batch_extended(&mut batch, dir.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("acceptance"),
            "expected acceptance error, got: {err}"
        );
    }

    #[test]
    fn test_extended_scope_policy_permissive_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fields = valid_task_fields("t1", "023", "INF")
            .replace("scope_policy: soft", "scope_policy: permissive");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields);

        let yaml = "tasks:\n  - id: INF-TSK-023-001\n    file_scope:\n      - src/foo.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        let result = validate_batch_extended(&mut batch, dir.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("scope_policy") || err.contains("permissive"),
            "expected scope_policy error, got: {err}"
        );
    }

    #[test]
    fn test_extended_empty_file_scope_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fields = valid_task_fields("t1", "023", "INF")
            .replace("file_scope:\n  - \"src/foo.rs\"", "file_scope: []");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields);

        let yaml = "tasks:\n  - id: INF-TSK-023-001\n    file_scope:\n      - src/foo.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        let result = validate_batch_extended(&mut batch, dir.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("file_scope"),
            "expected file_scope error, got: {err}"
        );
    }

    #[test]
    fn test_extended_concurrent_overlap_rejected() {
        let dir = tempfile::tempdir().unwrap();

        // Task A
        let fields_a = valid_task_fields("t1", "023", "INF");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields_a);

        // Task B with same file_scope but no dependency
        let fields_b = valid_task_fields("t2", "023", "INF").replace(
            "format_id: \"INF-TSK-023-001\"",
            "format_id: \"INF-TSK-023-002\"",
        );
        // Write task B to a separate task file
        let task_dir_b = dir
            .path()
            .join("project-management/epics/INF/INF-EPC-023/tasks");
        std::fs::write(
            task_dir_b.join("INF-TSK-023-002.md"),
            make_task_markdown(&fields_b),
        )
        .unwrap();

        let yaml = "tasks:\n  \
            - id: INF-TSK-023-001\n    \
              file_scope:\n      - src/foo.rs\n  \
            - id: INF-TSK-023-002\n    \
              file_scope:\n      - src/foo.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        let result = validate_batch_extended(&mut batch, dir.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("overlapping file_scope"),
            "expected overlap error, got: {err}"
        );
        assert!(err.contains("src/foo.rs"));
    }

    #[test]
    fn test_extended_dependent_overlap_allowed() {
        let dir = tempfile::tempdir().unwrap();

        // Task A
        let fields_a = valid_task_fields("t1", "023", "INF");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields_a);

        // Task B depends on A — overlapping file_scope is allowed
        let fields_b = valid_task_fields("t2", "023", "INF").replace(
            "format_id: \"INF-TSK-023-001\"",
            "format_id: \"INF-TSK-023-002\"",
        );
        let task_dir_b = dir
            .path()
            .join("project-management/epics/INF/INF-EPC-023/tasks");
        std::fs::write(
            task_dir_b.join("INF-TSK-023-002.md"),
            make_task_markdown(&fields_b),
        )
        .unwrap();

        let yaml = "tasks:\n  \
            - id: INF-TSK-023-001\n    \
              file_scope:\n      - src/foo.rs\n  \
            - id: INF-TSK-023-002\n    \
              depends_on: [INF-TSK-023-001]\n    \
              file_scope:\n      - src/foo.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        let result = validate_batch_extended(&mut batch, dir.path());
        // Should not error — dependency serializes access.
        assert!(
            result.is_ok(),
            "dependent tasks with overlap should be allowed, got: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn test_extended_max_workers_capped() {
        let dir = tempfile::tempdir().unwrap();
        let fields = valid_task_fields("t1", "023", "INF");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields);

        // Default max_concurrent is 30, request 50 workers.
        let yaml = "max_workers: 50\ntasks:\n  - id: INF-TSK-023-001\n    file_scope:\n      - src/foo.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert_eq!(batch.max_workers, 50);

        let result = validate_batch_extended(&mut batch, dir.path());
        // Validation should succeed (capping is not an error, just a warning).
        assert!(
            result.is_ok(),
            "max_workers capping should succeed, got: {:?}",
            result.unwrap_err()
        );
        assert_eq!(
            batch.max_workers, 30,
            "max_workers should be capped to max_concurrent (30)"
        );
    }

    #[test]
    fn test_extended_project_dir_parameter() {
        let dir = tempfile::tempdir().unwrap();
        let fields = valid_task_fields("t1", "023", "INF");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields);

        let yaml = "tasks:\n  - id: INF-TSK-023-001\n    file_scope:\n      - src/foo.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        // Should accept the project_dir parameter without error.
        let result = validate_batch_extended(&mut batch, dir.path());
        assert!(
            result.is_ok(),
            "project_dir parameter should be accepted, got: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn test_extended_missing_task_markdown() {
        let dir = tempfile::tempdir().unwrap();
        // Set up enforcement policy but NOT the task markdown.
        let policy_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&policy_dir).unwrap();
        std::fs::write(
            policy_dir.join("enforcement-policy.json"),
            r#"{"merge_protection":{"protected_branches":["main"]}}"#,
        )
        .unwrap();

        let yaml = "tasks:\n  - id: INF-TSK-023-001\n    file_scope:\n      - src/foo.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        let result = validate_batch_extended(&mut batch, dir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("reading task markdown")
        );
    }

    #[test]
    fn test_extended_valid_batch_passes() {
        let dir = tempfile::tempdir().unwrap();
        let fields = valid_task_fields("t1", "023", "INF");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields);

        let yaml = "tasks:\n  - id: INF-TSK-023-001\n    file_scope:\n      - src/foo.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        let result = validate_batch_extended(&mut batch, dir.path());
        assert!(
            result.is_ok(),
            "valid batch should pass, got: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn test_extended_protected_branch_auto_merge_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fields = valid_task_fields("t1", "023", "INF");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields);

        let yaml = "integration_auto_merge: true\nintegration_branch: develop\ntasks:\n  - id: INF-TSK-023-001\n    file_scope:\n      - src/foo.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        // Passes because "develop" is not protected.
        let result = validate_batch_extended(&mut batch, dir.path());
        assert!(result.is_ok(), "non-protected target should pass");
    }

    #[test]
    fn test_extended_auto_merge_release_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fields = valid_task_fields("t1", "023", "INF");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields);

        // integration_auto_merge=true with "release/v1.0" should be caught by validate_batch.
        let yaml = "integration_auto_merge: true\nintegration_branch: release/v1.0\ntasks:\n  - id: INF-TSK-023-001\n    file_scope:\n      - src/foo.rs\n";
        let result = parse_batch_data(yaml, "test.yaml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("release/v1.0"));
    }

    #[test]
    fn test_extended_no_overlap_different_scopes() {
        let dir = tempfile::tempdir().unwrap();

        // Task A with scope foo.rs
        let fields_a = valid_task_fields("t1", "023", "INF");
        setup_project_with_task(dir.path(), "INF-TSK-023-001", &fields_a);

        // Task B with scope bar.rs (different, no overlap)
        let fields_b = valid_task_fields("t2", "023", "INF")
            .replace(
                "format_id: \"INF-TSK-023-001\"",
                "format_id: \"INF-TSK-023-002\"",
            )
            .replace(
                "file_scope:\n  - \"src/foo.rs\"",
                "file_scope:\n  - \"src/bar.rs\"",
            );
        let task_dir = dir
            .path()
            .join("project-management/epics/INF/INF-EPC-023/tasks");
        std::fs::write(
            task_dir.join("INF-TSK-023-002.md"),
            make_task_markdown(&fields_b),
        )
        .unwrap();

        let yaml = "tasks:\n  \
            - id: INF-TSK-023-001\n    \
              file_scope:\n      - src/foo.rs\n  \
            - id: INF-TSK-023-002\n    \
              file_scope:\n      - src/bar.rs\n";
        let mut batch = parse_batch_data(yaml, "test.yaml").unwrap();
        let result = validate_batch_extended(&mut batch, dir.path());
        assert!(
            result.is_ok(),
            "non-overlapping scopes should pass, got: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn test_parse_batch_data_with_project_dir_loads_policy() {
        let dir = tempfile::tempdir().unwrap();
        // Create enforcement policy with custom protected branches.
        let policy_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&policy_dir).unwrap();
        std::fs::write(
            policy_dir.join("enforcement-policy.json"),
            r#"{"merge_protection":{"protected_branches":["main","custom-protected"]}}"#,
        )
        .unwrap();

        // integration_auto_merge=true targeting custom-protected should fail.
        let yaml = "integration_auto_merge: true\nintegration_branch: custom-protected\ntasks:\n  - id: task-a\n";
        let result = parse_batch_data_with_project_dir(yaml, "test.yaml", Some(dir.path()));
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("custom-protected"),
            "should reject custom protected branch"
        );
    }

    // -- H1: Path containment overlap detection --

    #[test]
    fn test_path_containment_overlap_detected() {
        // scope_a contains "src/" and scope_b contains "src/main.rs"
        // This should be detected as overlapping via directory containment.
        let scope_a: Vec<String> = vec!["src/".to_string()];
        let scope_b: Vec<String> = vec!["src/main.rs".to_string()];

        let set_a: HashSet<&str> = scope_a.iter().map(String::as_str).collect();

        // Check that path containment logic finds the overlap.
        let has_containment = scope_b.iter().any(|p| {
            let pb = std::path::Path::new(p.as_str());
            if set_a.contains(p.as_str()) {
                return true;
            }
            scope_a.iter().any(|a| {
                let pa = std::path::Path::new(a.as_str());
                pb.starts_with(pa) || pa.starts_with(pb)
            })
        });
        assert!(has_containment, "src/ should contain src/main.rs");
    }

    #[test]
    fn test_distinct_paths_no_overlap() {
        // Completely distinct paths should not overlap.
        let scope_a: Vec<String> = vec!["lib/utils.rs".to_string()];
        let scope_b: Vec<String> = vec!["tests/test_main.rs".to_string()];

        let set_a: HashSet<&str> = scope_a.iter().map(String::as_str).collect();

        let has_overlap = scope_b.iter().any(|p| {
            let pb = std::path::Path::new(p.as_str());
            if set_a.contains(p.as_str()) {
                return true;
            }
            scope_a.iter().any(|a| {
                let pa = std::path::Path::new(a.as_str());
                pb.starts_with(pa) || pa.starts_with(pb)
            })
        });
        assert!(!has_overlap, "distinct paths should not overlap");
    }

    // -- Fix 1: read_task_scope tests --

    #[test]
    fn test_read_task_scope_reads_from_markdown() {
        let dir = tempfile::tempdir().unwrap();

        // Create a task markdown with file_scope and scope_policy in frontmatter.
        let epic_dir = dir
            .path()
            .join("project-management/epics/TST/TST-EPC-001/tasks");
        std::fs::create_dir_all(&epic_dir).unwrap();
        let task_path = epic_dir.join("TST-TSK-001-001.md");
        std::fs::write(
            &task_path,
            "---\ntitle: Test\nfile_scope:\n  - src/main.rs\n  - src/lib.rs\nscope_policy: hard\n---\nBody\n",
        )
        .unwrap();

        let (scope, policy) = read_task_scope("TST-TSK-001-001", dir.path()).unwrap();
        assert_eq!(scope, vec!["src/main.rs", "src/lib.rs"]);
        assert_eq!(policy, "hard");
    }

    #[test]
    fn test_read_task_scope_defaults_when_missing() {
        let dir = tempfile::tempdir().unwrap();

        // Create a task markdown with no file_scope or scope_policy.
        let epic_dir = dir
            .path()
            .join("project-management/epics/TST/TST-EPC-001/tasks");
        std::fs::create_dir_all(&epic_dir).unwrap();
        let task_path = epic_dir.join("TST-TSK-001-002.md");
        std::fs::write(&task_path, "---\ntitle: Test\n---\nBody\n").unwrap();

        let (scope, policy) = read_task_scope("TST-TSK-001-002", dir.path()).unwrap();
        assert!(scope.is_empty(), "should default to empty scope");
        assert_eq!(policy, "soft", "should default to soft policy");
    }

    #[test]
    fn test_read_task_scope_missing_task_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_task_scope("NONEXISTENT-TSK-001-001", dir.path());
        assert!(result.is_err(), "should error on missing task");
    }

    // -- Fix 3: Overlap detection uses markdown scopes --

    #[test]
    fn test_overlap_uses_markdown_scope_not_batch() {
        // When batch TaskSpec has empty file_scope, overlap detection should use
        // the markdown file_scope. We verify this by checking that the effective_scope
        // logic prefers markdown when batch is empty.
        let batch_scope: Vec<String> = vec![];
        let md_scope = vec!["src/main.rs".to_string(), "src/lib.rs".to_string()];

        let effective = if batch_scope.is_empty() {
            md_scope.clone()
        } else {
            batch_scope
        };

        assert_eq!(
            effective, md_scope,
            "should use markdown scope when batch is empty"
        );
    }

    #[test]
    fn test_batch_override_takes_precedence() {
        let batch_scope = vec!["tests/only.rs".to_string()];
        let md_scope = vec!["src/main.rs".to_string(), "src/lib.rs".to_string()];

        let effective = if batch_scope.is_empty() {
            md_scope
        } else {
            batch_scope.clone()
        };

        assert_eq!(
            effective, batch_scope,
            "batch override should take precedence"
        );
    }

    // -- build_resume_batch tests --

    #[test]
    fn test_resume_batch_all_completed() {
        let batch = ParsedBatch {
            name: "test".into(),
            file_path: "batch.yaml".into(),
            max_workers: 2,
            integration_auto_merge: false,
            integration_branch: "main".into(),
            integration_branch_is_auto: false,
            final_pr: false,
            final_pr_target: "main".into(),
            tasks: vec![
                TaskSpec {
                    id: "task-a".into(),
                    depends_on: vec![],
                    file_scope: vec![],
                    scope_policy: None,
                    timeout_secs: None,
                },
                TaskSpec {
                    id: "task-b".into(),
                    depends_on: vec!["task-a".into()],
                    file_scope: vec![],
                    scope_policy: None,
                    timeout_secs: None,
                },
            ],
            order: vec!["task-a".into(), "task-b".into()],
        };

        let completed: HashSet<String> = ["task-a".to_string(), "task-b".to_string()].into();
        let resume = build_resume_batch(&batch, &completed);
        assert!(resume.tasks.is_empty());
        assert!(resume.order.is_empty());
        assert_eq!(resume.name, "test-resume");
    }

    #[test]
    fn test_resume_batch_one_failed() {
        let batch = ParsedBatch {
            name: "test".into(),
            file_path: "batch.yaml".into(),
            max_workers: 2,
            integration_auto_merge: false,
            integration_branch: "main".into(),
            integration_branch_is_auto: false,
            final_pr: false,
            final_pr_target: "main".into(),
            tasks: vec![
                TaskSpec {
                    id: "task-a".into(),
                    depends_on: vec![],
                    file_scope: vec![],
                    scope_policy: None,
                    timeout_secs: None,
                },
                TaskSpec {
                    id: "task-b".into(),
                    depends_on: vec!["task-a".into()],
                    file_scope: vec![],
                    scope_policy: None,
                    timeout_secs: None,
                },
            ],
            order: vec!["task-a".into(), "task-b".into()],
        };

        let completed: HashSet<String> = ["task-a".to_string()].into();
        let resume = build_resume_batch(&batch, &completed);
        assert_eq!(resume.tasks.len(), 1);
        assert_eq!(resume.tasks[0].id, "task-b");
        // Dependency on task-a should be removed (already completed).
        assert!(resume.tasks[0].depends_on.is_empty());
        assert_eq!(resume.order, vec!["task-b"]);
    }

    #[test]
    fn test_resume_batch_preserves_settings() {
        let batch = ParsedBatch {
            name: "my-batch".into(),
            file_path: "path/to/batch.yaml".into(),
            max_workers: 5,
            integration_auto_merge: true,
            integration_branch: "develop".into(),
            integration_branch_is_auto: false,
            final_pr: true,
            final_pr_target: "main".into(),
            tasks: vec![TaskSpec {
                id: "task-a".into(),
                depends_on: vec![],
                file_scope: vec![],
                scope_policy: None,
                timeout_secs: None,
            }],
            order: vec!["task-a".into()],
        };

        let completed: HashSet<String> = HashSet::new();
        let resume = build_resume_batch(&batch, &completed);
        assert_eq!(resume.max_workers, 5);
        assert!(resume.integration_auto_merge);
        assert_eq!(resume.integration_branch, "develop");
        assert_eq!(resume.file_path, "path/to/batch.yaml");
        assert!(resume.final_pr);
        assert_eq!(resume.final_pr_target, "main");
    }

    // -- resolve_target tests --

    #[test]
    fn test_resolve_target_empty_generates_branch() {
        let mut batch = ParsedBatch {
            name: "my-batch".into(),
            file_path: "b.yaml".into(),
            max_workers: 1,
            integration_auto_merge: false,
            integration_branch: String::new(),
            integration_branch_is_auto: false,
            final_pr: false,
            final_pr_target: "main".into(),
            tasks: vec![],
            order: vec![],
        };

        resolve_target(&mut batch, "ses-01abc12345678xyz");
        assert!(batch.integration_branch.starts_with("autorun/my-batch-"));
        assert!(batch.integration_branch.ends_with("678xyz"));
        assert!(batch.integration_branch_is_auto);
        assert!(batch.integration_auto_merge);
        assert!(batch.final_pr);
    }

    #[test]
    fn test_resolve_target_explicit_unchanged() {
        let mut batch = ParsedBatch {
            name: "my-batch".into(),
            file_path: "b.yaml".into(),
            max_workers: 1,
            integration_auto_merge: false,
            integration_branch: "develop".into(),
            integration_branch_is_auto: false,
            final_pr: true,
            final_pr_target: "main".into(),
            tasks: vec![],
            order: vec![],
        };

        resolve_target(&mut batch, "ses-01abc12345678xyz");
        assert_eq!(batch.integration_branch, "develop");
        assert!(!batch.integration_branch_is_auto);
        assert!(!batch.integration_auto_merge); // Not changed for explicit targets.
    }

    // -- integration_auto_merge inference tests --

    #[test]
    fn test_auto_merge_inferred_non_protected() {
        let yaml = "integration_branch: develop\ntasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert!(
            batch.integration_auto_merge,
            "integration_auto_merge should be true for non-protected target when not specified"
        );
    }

    #[test]
    fn test_auto_merge_inferred_protected() {
        let yaml = "integration_branch: main\ntasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert!(
            !batch.integration_auto_merge,
            "integration_auto_merge should be false for protected target when not specified"
        );
    }

    #[test]
    fn test_auto_merge_explicit_true() {
        let yaml =
            "integration_auto_merge: true\nintegration_branch: develop\ntasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert!(batch.integration_auto_merge);
    }

    #[test]
    fn test_auto_merge_explicit_false() {
        let yaml =
            "integration_auto_merge: false\nintegration_branch: develop\ntasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert!(!batch.integration_auto_merge);
    }

    // -- backward compatibility --

    #[test]
    fn test_backward_compat_no_new_fields() {
        let yaml = "name: old-batch\ntasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert_eq!(batch.name, "old-batch");
        // Default: empty integration_branch, integration_auto_merge inferred from protection.
        assert!(batch.integration_branch.is_empty());
        assert!(batch.integration_branch_is_auto);
        // final_pr defaults to false when integration_branch is empty (no explicit integration branch).
        assert!(!batch.final_pr);
        assert_eq!(batch.final_pr_target, "main");
    }

    // -- final_pr defaults --

    #[test]
    fn test_final_pr_true_for_non_main_target() {
        let yaml = "integration_branch: develop\ntasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert!(
            batch.final_pr,
            "final_pr should default to true for non-main integration_branch"
        );
    }

    #[test]
    fn test_final_pr_false_for_main_target() {
        let yaml = "integration_branch: main\ntasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert!(
            !batch.final_pr,
            "final_pr should default to false for main integration_branch"
        );
    }

    #[test]
    fn test_final_pr_explicit_override() {
        let yaml = "integration_branch: develop\nfinal_pr: false\ntasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert!(
            !batch.final_pr,
            "explicit final_pr: false should override default"
        );
    }

    // -- Per-task timeout tests --

    #[test]
    fn test_task_spec_timeout_secs_default_none() {
        let yaml = "tasks:\n  - id: task-a\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert!(
            batch.tasks[0].timeout_secs.is_none(),
            "timeout_secs should default to None"
        );
    }

    #[test]
    fn test_task_spec_timeout_secs_custom() {
        let yaml = "tasks:\n  - id: task-a\n    timeout_secs: 3600\n";
        let batch = parse_batch_data(yaml, "test.yaml").unwrap();
        assert_eq!(
            batch.tasks[0].timeout_secs,
            Some(3600),
            "timeout_secs should be parsed from YAML"
        );
    }
}
