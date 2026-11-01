//! Every shipped GitHub Actions job bounds setup and gate execution.

use std::path::Path;

use codeflow_core::scaffold::{DirSource, ScaffoldManifest};

#[test]
fn shipped_and_repository_workflow_jobs_declare_timeouts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let assets = DirSource::new(root.join("assets"));
    let manifest = ScaffoldManifest::load(&assets).unwrap();
    let mut workflows: Vec<_> = manifest
        .entries
        .iter()
        .filter(|entry| entry.dest.starts_with(".github/workflows/"))
        .map(|entry| root.join("assets/base").join(&entry.src))
        .collect();
    assert!(!workflows.is_empty(), "shipped workflows must be checked");
    workflows.push(root.join(".github/workflows/codeflow-ci.yml"));
    let mut missing = Vec::new();
    for path in workflows {
        let workflow: serde_yaml::Value =
            serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let jobs = workflow["jobs"].as_mapping().expect("workflow has jobs");
        assert!(!jobs.is_empty(), "{} has no jobs", path.display());
        for (name, job) in jobs {
            if job["timeout-minutes"]
                .as_u64()
                .is_none_or(|minutes| minutes == 0)
            {
                missing.push(format!(
                    "{}: jobs.{} needs a positive timeout-minutes",
                    path.display(),
                    name.as_str().unwrap()
                ));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}
