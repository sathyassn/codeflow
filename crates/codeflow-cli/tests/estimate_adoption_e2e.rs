//! Real installed-method journeys; parser and allocation matrices live in the
//! estimator's unit/black-box tests, not duplicated here.
use codeflow_core::scaffold::sha256_hex;
use codeflow_core::validate::{validate_task, ValidateOptions};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const EXAMPLE: &str = ".agents/skills/cf-estimate/examples/forecast.json";

struct Project {
    _temp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl Project {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let home = temp.path().join("private-home");
        Self {
            _temp: temp,
            root,
            home,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
        let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        Command::new(exe)
            .args(args)
            .current_dir(&self.root)
            .env("CODEFLOW_HOME", &self.home)
            .env("PATH", path)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("CODEFLOW_INTEGRATE_TOKEN")
            .env_remove("CODEFLOW_HUMAN_OVERRIDE")
            .output()
            .unwrap()
    }

    fn success(&self, args: &[&str]) {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("CONFLICT"));
    }

    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let path = self.root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn read(&self, path: &str) -> Vec<u8> {
        std::fs::read(self.root.join(path)).unwrap()
    }

    fn manifest(&self) -> Value {
        serde_json::from_slice(&self.read(".codeflow/manifest.json")).unwrap()
    }

    fn check(&self, path: &str, exit: i32) -> Value {
        let before = self.read(path);
        let output = self.run(&["estimate", "check", path, "--json"]);
        assert_eq!(
            output.status.code(),
            Some(exit),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(self.read(path), before, "checker changed the forecast");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn assert_installed_method(&self) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/base/agents/skills/cf-estimate");
        let mut files = Vec::new();
        collect_files(&source, &source, &mut files);
        assert!(files.len() >= 8, "method inventory unexpectedly empty");
        let manifest = self.manifest();
        for file in files {
            let expected = std::fs::read(source.join(&file)).unwrap();
            for harness in [".agents", ".claude"] {
                let dest = format!("{harness}/skills/cf-estimate/{}", slash_path(&file));
                assert!(manifest["files"].get(&dest).is_some(), "unmanaged {dest}");
                assert_eq!(self.read(&dest), expected, "installed {dest}");
                assert_eq!(
                    self.read(&format!(".codeflow/.baseline/{dest}")),
                    expected,
                    "baseline {dest}"
                );
            }
        }
    }

    fn assert_example(&self) {
        for path in [EXAMPLE, ".claude/skills/cf-estimate/examples/forecast.json"] {
            let report = self.check(path, 0);
            assert_eq!(report["checked_package_count"], 1);
            assert_eq!(report["source_assurance"], "limited");
            assert_eq!(report["findings"], json!([]));
            let totals: Vec<_> = report["scenarios"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    assert_eq!(s["valid"], true);
                    s["elapsed_seconds"].as_u64().unwrap()
                })
                .collect();
            assert_eq!(totals, [5400, 9600, 15600]);
        }
    }
}

/// Manifest keys use `/` on every platform, so join components with it.
fn slash_path(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_str().unwrap())
        .collect::<Vec<_>>()
        .join("/")
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(root, &path, files);
        } else {
            files.push(path.strip_prefix(root).unwrap().to_owned());
        }
    }
}

#[test]
fn installed_tiers_expose_exact_runnable_method_without_adoption() {
    for tier in ["--full", "--standard", "--minimal"] {
        let project = Project::new();
        project.success(&["init", "--yes", tier]);
        assert!(!project.root.join(".codeflow/estimate.json").exists());
        assert!(!project.root.join("project-management/estimates").exists());
        if tier == "--minimal" {
            for harness in [".agents", ".claude"] {
                assert!(!project
                    .root
                    .join(format!("{harness}/skills/cf-estimate"))
                    .exists());
            }
            assert!(!project.manifest()["files"]
                .as_object()
                .unwrap()
                .keys()
                .any(|p| p.contains("/cf-estimate/")));
        } else {
            project.assert_installed_method();
            project.assert_example();
        }
        if tier != "--full" {
            assert!(!project.root.join("project-management").exists());
        }
    }
}

#[test]
fn brownfield_init_and_repeated_update_preserve_adoption_history_and_customization() {
    for (tier, home) in [
        ("--full", "project-management/estimates"),
        ("--standard", "planning/estimates"),
    ] {
        for status in ["adopted", "declined"] {
            let project = Project::new();
            let mut owned = BTreeMap::new();
            owned.insert(".codeflow/estimate.json".to_owned(), serde_json::to_vec(&json!({
                "schema_version":1,"status":status,"method_version":"codeflow-agentic-1",
                "root":home,"rationale":"Existing owner decision", "re_offer_when":"Work authority changes"
            })).unwrap());
            for (suffix, text) in [
                (
                    "profile-v1.md",
                    "# Profile\nProject-specific reviewer calendar and acceptance.\n",
                ),
                (
                    "rubric-v1.md",
                    "# Rubric\nExisting versioned local adaptation.\n",
                ),
                ("forecasts/filter/v1.json", "{\"historical_forecast\":1}\n"),
                (
                    "forecasts/filter/v2.json",
                    "{\"historical_forecast\":2,\"predecessor\":\"v1\"}\n",
                ),
                (
                    "outcomes/filter.md",
                    "# Actuals\nStarted, reopened; release time remains unknown.\n",
                ),
            ] {
                owned.insert(format!("{home}/{suffix}"), text.as_bytes().to_vec());
            }
            owned.insert(
                "planning/AUTHORITY.md".into(),
                b"External tracker owns status, acceptance and dependencies.\n".to_vec(),
            );
            owned.insert(
                ".agents/skills/cf-estimate/LOCAL-NOTES.md".into(),
                b"User-owned skill notes.\n".to_vec(),
            );
            owned.insert(
                "unrelated.txt".into(),
                b"Preserve unrelated work.\n".to_vec(),
            );
            for (path, bytes) in &owned {
                project.write(path, bytes);
            }
            project.write(
                "AGENTS.md",
                "# Project policy\nOwner-specific release approval remains mandatory.\n",
            );
            project.success(&["init", "--yes", tier]);
            let agents = project.read("AGENTS.md");
            assert!(String::from_utf8_lossy(&agents)
                .contains("Owner-specific release approval remains mandatory."));
            for pass in 0..3 {
                if pass > 0 {
                    project.success(&["update"]);
                }
                for (path, bytes) in &owned {
                    assert_eq!(&project.read(path), bytes, "pass {pass}: {path}");
                    assert!(
                        project.manifest()["files"].get(path).is_none(),
                        "project record became managed: {path}"
                    );
                }
                assert_eq!(project.read("AGENTS.md"), agents);
                project.assert_installed_method();
                project.assert_example();
                if tier == "--standard" {
                    assert!(!project.root.join("project-management").exists());
                }
            }
        }
    }
}

#[test]
fn update_introduces_method_into_synthetic_pre_estimation_install() {
    let project = Project::new();
    project.success(&["init", "--yes", "--standard"]);
    // Synthetic previous-install shape, not a claim of running an older binary.
    let mut manifest = project.manifest();
    let files = manifest["files"].as_object_mut().unwrap();
    let removed: Vec<_> = files
        .keys()
        .filter(|p| p.contains("/skills/cf-estimate/"))
        .cloned()
        .collect();
    assert!(!removed.is_empty());
    for path in &removed {
        files.remove(path);
        std::fs::remove_file(project.root.join(path)).unwrap();
        std::fs::remove_file(project.root.join(".codeflow/.baseline").join(path)).unwrap();
    }
    project.write(
        ".codeflow/manifest.json",
        serde_json::to_vec_pretty(&manifest).unwrap(),
    );
    project.write(
        "planning/estimate-notes.md",
        "Existing project forecast notes.\n",
    );
    for _ in 0..2 {
        project.success(&["update"]);
        project.assert_installed_method();
        project.assert_example();
        assert_eq!(
            project.read("planning/estimate-notes.md"),
            b"Existing project forecast notes.\n"
        );
        assert!(!project.root.join(".codeflow/estimate.json").exists());
        assert!(!project.root.join("project-management").exists());
    }
}

#[test]
fn installed_forecast_rejects_stale_scope_and_resource_overcommit_without_rewriting_history() {
    let project = Project::new();
    project.success(&["init", "--yes", "--standard"]);
    project.assert_example();
    let brief_path = ".agents/skills/cf-estimate/examples/brief.md";
    let original = project.read(brief_path);
    let mut changed = original.clone();
    changed.extend_from_slice(b"\nAdditional acceptance: include archived records.\n");
    project.write(brief_path, changed);
    let stale = project.check(EXAMPLE, 1);
    assert!(stale["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["code"] == "stale_pin"));
    assert!(stale["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["valid"] == false && s["elapsed_seconds"].is_null()));
    project.write(brief_path, original);
    let mut forecast: Value = serde_json::from_slice(&project.read(EXAMPLE)).unwrap();
    for scenario in forecast["scenarios"].as_array_mut().unwrap() {
        scenario["activities"][0]["demands"][0]["units"] = json!(2);
    }
    project.write(
        "planning/impossible.json",
        serde_json::to_vec_pretty(&forecast).unwrap(),
    );
    let impossible = project.check("planning/impossible.json", 1);
    assert!(impossible["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["code"] == "resource_capacity"));
    assert!(impossible["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["valid"] == false && s["elapsed_seconds"].is_null()));
    project.assert_example();
}

#[test]
fn installed_evaluation_fixtures_preserve_green_and_stale_only_premises() {
    let registry: Value = serde_json::from_slice(include_bytes!(
        "../../../assets/base/agents/skills/cf-evaluate-model/resources/fixtures.json"
    ))
    .unwrap();
    for (id, forecast_path, exit) in [
        (
            "estimate-checker-green-is-not-delivery-evidence",
            "planning/proposed.json",
            0,
        ),
        (
            "estimate-stale-pins-do-not-rewrite-history",
            "planning/forecast-v1.json",
            1,
        ),
    ] {
        let fixture = registry["fixtures"]
            .as_array()
            .unwrap()
            .iter()
            .find(|fixture| fixture["id"] == id)
            .unwrap();
        let overlays = fixture["files"].as_object().unwrap();
        let forecast: Value =
            serde_json::from_str(overlays[forecast_path].as_str().unwrap()).unwrap();
        let project = Project::new();
        project.success(&["init", "--yes", "--standard"]);
        let source = &forecast["packages"][0]["source"]["pin"];
        let pins = [source, &forecast["profile"], &forecast["rubric"]];
        // Even the stale fixture must start from current shipped source pins;
        // its declared brief overlay, not unrelated upstream drift, causes red.
        for pin in pins {
            assert_eq!(
                pin["sha256"],
                sha256_hex(&project.read(pin["path"].as_str().unwrap())),
                "{id}: fixture pin is not current before overlay: {}",
                pin["path"]
            );
        }
        for (path, bytes) in overlays {
            project.write(path, bytes.as_str().unwrap());
        }
        let report = project.check(forecast_path, exit);
        assert_eq!(report["checked_package_count"], 1);
        assert_eq!(report["scenarios"].as_array().unwrap().len(), 3);
        for pin in [&forecast["profile"], &forecast["rubric"]] {
            assert_eq!(
                pin["sha256"],
                sha256_hex(&project.read(pin["path"].as_str().unwrap())),
                "{id}: profile/rubric premise changed"
            );
        }
        if exit == 0 {
            assert_eq!(report["source_assurance"], "limited");
            assert_eq!(report["findings"], json!([]));
            let exclusions = report["exclusions"].as_array().unwrap();
            assert_eq!(exclusions.len(), 3);
            for stage in ["review", "verify", "rework"] {
                assert!(exclusions.iter().any(|exclusion| {
                    exclusion["stage"] == stage
                        && exclusion["package_id"] == forecast["packages"][0]["id"]
                }));
            }
            assert!(report["scenarios"]
                .as_array()
                .unwrap()
                .iter()
                .all(|scenario| {
                    scenario["valid"] == true && scenario["elapsed_seconds"].is_u64()
                }));
        } else {
            assert_eq!(report["source_assurance"], "unverified");
            let findings = report["findings"].as_array().unwrap();
            assert_eq!(findings.len(), 1, "{id}: {findings:?}");
            assert_eq!(findings[0]["code"], "stale_pin");
            assert_ne!(
                source["sha256"],
                sha256_hex(&project.read(source["path"].as_str().unwrap()))
            );
            assert!(report["scenarios"]
                .as_array()
                .unwrap()
                .iter()
                .all(|scenario| {
                    scenario["valid"] == false && scenario["elapsed_seconds"].is_null()
                }));
        }
    }
}

#[test]
fn canonical_installed_forecast_keeps_legacy_estimate_metadata_independent() {
    let project = Project::new();
    project.success(&["init", "--yes", "--full"]);
    for legacy in ["XS", "S", "M", "L", "XL"] {
        let task = format!("---\nid: TSK-001\nepic_id: null\nstandalone_reason: Independent read-contract change\nintegration_target: main\ntitle: Add read contract\nstatus: todo\nwork_type: feat\nestimate: {legacy}\nspecs: []\ndepends_on: []\n---\n## Description\nOwn one read-only contract; existing storage and authorization are consumed.\n\n## Acceptance Criteria\nReturn only authorized records in stable order; preserve rejection of unknown filters.\n");
        let path = "project-management/tasks/TSK-001.md";
        project.write(path, &task);
        let (errors, _) =
            validate_task(&project.root.join(path), &ValidateOptions::default()).unwrap();
        assert!(errors.is_empty(), "legacy {legacy}: {errors:?}");
        let mut forecast: Value = serde_json::from_slice(&project.read(EXAMPLE)).unwrap();
        forecast["packages"][0]["source"] = json!({"kind":"codeflow_task","task_id":"TSK-001","sha256":sha256_hex(task.as_bytes())});
        project.write(
            "project-management/estimates/current.json",
            serde_json::to_vec_pretty(&forecast).unwrap(),
        );
        let report = project.check("project-management/estimates/current.json", 0);
        assert_eq!(report["source_assurance"], "canonical_local");
        assert_eq!(report["checked_package_count"], 1);
        assert_eq!(forecast["packages"][0]["grade"], "easy");
        assert_eq!(project.read(path), task.as_bytes());
    }
}
