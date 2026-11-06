//! Adopters start and resume catalog work through the built command.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn repo(tier: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Bootstrap fixture"]);
    git(root, &["config", "user.email", "bootstrap@example.test"]);
    fs::create_dir_all(root.join(".codeflow")).unwrap();
    fs::write(root.join(".codeflow/project.toml"), format!(
        "schema_version = 1\ntier = '{tier}'\nscaffold_version = '4.0.0'\nstack = 'python'\nareas = ['engine']\npolicy_armed = false\ngit_hooks = 'unwired'\npermission_preset = 'default'\n"
    )).unwrap();
    fs::write(
        root.join(".gitignore"),
        include_str!("../../../assets/base/gitignore"),
    )
    .unwrap();
    fs::write(
        root.join("search.py"),
        "def search(items, term):\n    return [item for item in items if term in item]\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "chore: seed sample"]);
    dir
}

fn files(root: &Path, kind: &str) -> Vec<PathBuf> {
    let folder = root.join("project-management").join(kind);
    let mut files: Vec<_> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    files
}

fn bootstrap(root: &Path) -> String {
    success(cli(root, &["capability", "bootstrap"]))
}

fn validate(root: &Path) {
    let result = success(cli(root, &["validate", "--docs"]));
    assert!(result.contains("doc graph clean"), "{result}");
}

// These acceptance results are synthetic fixture state, never an independent
// review of an actual adopting project's catalog.
fn complete_stage(root: &Path, path: &Path) {
    git(root, &["add", "."]);
    git(
        root,
        &["commit", "-qm", "chore: record fixture stage outputs"],
    );
    let reviewed = git(root, &["rev-parse", "HEAD"]);
    let text = fs::read_to_string(path)
        .unwrap()
        .replace("status: todo", "status: complete");
    let acceptance = format!("\n```yaml\nacceptance:\n  reviewed: {}\n  review: https://example.test/bootstrap-fixture-review\n  criteria:\n    AC-1: verified | scripted stage output and validation\n  journey: verified | scripted capability bootstrap fixture\n  not_verified: live independent review\n  follow_ups: none: fixture only\n  verdict: approved\n```\n", reviewed.trim());
    fs::write(path, text + &acceptance).unwrap();
    validate(root);
}

#[test]
fn fresh_standard_enables_tracking_and_resumes_each_boundary() {
    let dir = repo("standard");
    let root = dir.path();
    let result = bootstrap(root);
    assert!(result.contains("next stage: inventory"), "{result}");
    assert!(result.contains("tracking enabled"), "{result}");
    assert!(result.contains("policy unchanged"), "{result}");
    assert!(codeflow_core::workgraph::durable_work_tracking_enabled(root).unwrap());
    assert!(!root.join("docs/capability-bootstrap.md").exists());
    validate(root);
    let epics = files(root, "epics");
    assert_eq!(epics.len(), 1);
    let epic = fs::read_to_string(&epics[0]).unwrap();
    assert!(epic.contains("inventory: complete"));
    assert!(epic.contains("recorded independent review"));
    let stages: Vec<_> = files(root, "tasks")
        .into_iter()
        .filter(|path| {
            fs::read_to_string(path)
                .unwrap()
                .contains("codeflow:capability-bootstrap v1")
        })
        .collect();
    assert_eq!(stages.len(), 4);
    for (index, path) in stages.iter().enumerate() {
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains(".codeflow/rules/capabilities.md"));
        assert!(text.contains("(serves EPC-001 AC-1)"));
        let expected = if index == 0 {
            "depends_on: []".into()
        } else {
            format!(
                "depends_on: [{}]",
                stages[index - 1].file_stem().unwrap().to_str().unwrap()
            )
        };
        assert!(text.contains(&expected), "{text}");
    }
    let names = ["inventory", "classification", "evidence", "review"];
    for (index, path) in stages.iter().enumerate() {
        // Partial progress within a stage must survive a rerun.
        let mut text = fs::read_to_string(path).unwrap();
        text.push_str("\nProgress: first surface examined.\n");
        fs::write(path, &text).unwrap();
        assert!(bootstrap(root).contains(&format!("next stage: {}", names[index])));
        assert_eq!(fs::read_to_string(path).unwrap(), text);
        complete_stage(root, path);
        let result = bootstrap(root);
        if index + 1 < names.len() {
            assert!(
                result.contains(&format!("next stage: {}", names[index + 1])),
                "{result}"
            );
        } else {
            assert!(
                result.contains("all stages complete; set `inventory: complete`"),
                "{result}"
            );
        }
        validate(root);
        assert_eq!(files(root, "epics"), epics);
        assert_eq!(files(root, "tasks").len(), 4);
    }
}

#[test]
fn explicit_minimal_gets_a_resumable_checklist() {
    let dir = repo("minimal");
    let root = dir.path();
    assert!(!root.join("docs/capability-bootstrap.md").exists());
    let result = bootstrap(root);
    assert!(result.contains("tracking is not enabled"), "{result}");
    assert!(!root.join("project-management/tasks").exists());
    let path = root.join("docs/capability-bootstrap.md");
    let original = fs::read_to_string(&path).unwrap();
    for name in ["inventory", "classification", "evidence", "review"] {
        assert!(original.contains(&format!("- [ ] {name}")));
    }
    let progress = original.replace("- [ ] inventory", "- [x] inventory");
    fs::write(&path, &progress).unwrap();
    assert!(bootstrap(root).contains("next stage: classification"));
    assert_eq!(fs::read_to_string(&path).unwrap(), progress);
}

#[test]
fn incomplete_existing_catalog_seeds_inventory_without_recreating_records() {
    let dir = repo("full");
    let root = dir.path();
    success(cli(
        root,
        &[
            "capability",
            "new",
            "search",
            "--summary",
            "Readers search the sample catalog",
            "--area",
            "engine",
            "--audience",
            "external",
        ],
    ));
    let path = root.join("docs/capabilities/CAP-001-search.md");
    let before = fs::read(&path).unwrap();
    assert!(bootstrap(root).contains("next stage: inventory"));
    validate(root);
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(files(root, "epics").len(), 1);
    assert_eq!(files(root, "tasks").len(), 4);
}

#[test]
fn non_rust_sample_reaches_its_first_valid_catalog_through_four_tasks() {
    let dir = repo("full");
    let root = dir.path();
    println!("bootstrap: {}", bootstrap(root));
    validate(root);
    let tasks = files(root, "tasks");
    assert_eq!(files(root, "epics").len(), 1);
    assert_eq!(tasks.len(), 4);
    fs::create_dir_all(root.join("docs")).unwrap();
    // Inventory is a scripted fixture's work, not an independently reviewed adopter catalog.
    fs::write(root.join("docs/capability-inventory.md"),
        "# Sample inventory\n\nsearch.py search: CAP-001. No build, deployment or quality promises beyond this sample.\n").unwrap();
    let created = success(cli(
        root,
        &[
            "capability",
            "new",
            "search",
            "--summary",
            "Readers search the sample catalog",
            "--area",
            "engine",
            "--audience",
            "external",
        ],
    ));
    println!("inventory: {created}");
    for (index, stage) in ["inventory", "classification", "evidence"]
        .into_iter()
        .enumerate()
    {
        let text = fs::read_to_string(&tasks[index]).unwrap();
        assert!(text.contains(&format!("codeflow:capability-bootstrap v1 {stage}")));
        complete_stage(root, &tasks[index]);
        println!("{stage}: {}", bootstrap(root));
    }
    // A planned capability makes no shipped-evidence or maturity assertion.
    assert_eq!(
        codeflow_core::capability::load_capability_catalog(root)
            .unwrap()
            .records
            .len(),
        1
    );
    fs::write(root.join("docs/capability-review.md"),
        "# Sample review\n\nFixture review of search.py and CAP-001: one planned functional surface. No shipped behavior or live reviewer claimed.\n").unwrap();
    fs::write(
        root.join("docs/capabilities/vocabulary.yaml"),
        "inventory: complete\n",
    )
    .unwrap();
    let text = fs::read_to_string(&tasks[3]).unwrap();
    assert!(text.contains("codeflow:capability-bootstrap v1 review"));
    complete_stage(root, &tasks[3]);
    let output = bootstrap(root);
    println!("review: {output}");
    assert!(output.contains("inventory is complete"));
    assert_eq!(files(root, "tasks"), tasks);
    validate(root);
    assert!(codeflow_core::capability::load_capability_catalog(root).is_ok());
}

fn assert_offer(command: &[&str]) {
    let dir = repo("minimal");
    let root = dir.path();
    if command[0] == "update" {
        success(cli(root, &["init", "--minimal", "--yes"]));
    }
    let result = success(cli(root, command));
    assert!(result.contains("codeflow capability bootstrap"), "{result}");
    assert!(!root.join("docs/capability-bootstrap.md").exists());
    assert!(!root.join("project-management/tasks").exists());
}

#[test]
fn init_offers_bootstrap_without_creating_work() {
    assert_offer(&["init", "--minimal", "--yes"]);
}

#[test]
fn update_offers_bootstrap_without_creating_work() {
    assert_offer(&["update"]);
}

#[test]
fn doctor_offers_bootstrap_without_creating_work() {
    assert_offer(&["doctor", "--check", "capabilities"]);
}

#[test]
fn complete_inventory_suppresses_the_offer_and_creates_no_bootstrap() {
    let dir = repo("full");
    let root = dir.path();
    success(cli(
        root,
        &[
            "capability",
            "new",
            "search",
            "--summary",
            "Readers search the sample catalog",
            "--area",
            "engine",
            "--audience",
            "external",
        ],
    ));
    fs::write(
        root.join("docs/capabilities/vocabulary.yaml"),
        "inventory: complete\n",
    )
    .unwrap();
    let doctor = success(cli(root, &["doctor", "--check", "capabilities"]));
    assert!(
        !doctor.contains("codeflow capability bootstrap"),
        "{doctor}"
    );
    assert!(bootstrap(root).contains("inventory is complete"));
    assert!(!root.join("project-management/tasks").exists());
}

#[test]
fn a_legacy_standard_checklist_is_not_forgotten_when_tracking_appears() {
    let dir = repo("standard");
    let root = dir.path();
    fs::create_dir_all(root.join("docs")).unwrap();
    let checklist = "# Initial catalog\n\n<!-- codeflow:capability-bootstrap v1 checklist -->\n\n- [x] inventory\n- [ ] classification\n- [ ] evidence\n- [ ] review\n";
    fs::write(root.join("docs/capability-bootstrap.md"), checklist).unwrap();
    success(cli(
        root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "Track sample work",
            "Sample work",
        ],
    ));
    let result = cli(root, &["capability", "bootstrap"]);
    assert!(!result.status.success());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("docs/capability-bootstrap.md"), "{error}");
    assert!(
        error.contains("finish or reconcile that checklist by hand"),
        "{error}"
    );
    assert!(!error.contains("resume that checklist"), "{error}");
    assert!(error.contains("no epic or task was created"), "{error}");
    assert_eq!(
        fs::read_to_string(root.join("docs/capability-bootstrap.md")).unwrap(),
        checklist
    );
    assert_eq!(files(root, "tasks").len(), 1);
    assert!(!root.join("project-management/epics/EPC-001.md").exists());
}

#[test]
fn installed_stage_templates_control_the_generated_records() {
    let dir = repo("standard");
    let root = dir.path();
    success(cli(root, &["init", "--standard", "--yes"]));
    let template = root.join(".codeflow/templates/capability-bootstrap/review.md");
    let text = fs::read_to_string(&template).unwrap();
    fs::write(
        template,
        text.replace(
            "Review the catalog independently",
            "Review the catalog independently under the sample reviewer convention",
        ),
    )
    .unwrap();
    bootstrap(root);
    let review = fs::read_to_string(root.join("project-management/tasks/TSK-004.md")).unwrap();
    assert!(review.contains("sample reviewer convention"));
    validate(root);
    assert_eq!(
        git(
            root,
            &["check-ignore", ".codeflow/capability-bootstrap.lock"]
        )
        .trim(),
        ".codeflow/capability-bootstrap.lock"
    );
}

#[test]
fn complete_checklist_names_the_remaining_inventory_confirmation() {
    let dir = repo("minimal");
    let root = dir.path();
    bootstrap(root);
    let path = root.join("docs/capability-bootstrap.md");
    let text = fs::read_to_string(&path).unwrap().replace("- [ ]", "- [x]");
    fs::write(path, text).unwrap();
    let result = bootstrap(root);
    assert!(
        result.contains("all stages complete; set `inventory: complete`"),
        "{result}"
    );
    assert!(!result.contains("next stage: review"), "{result}");
    assert!(!result.contains("inventory is complete"), "{result}");
}

#[test]
fn tracked_bootstrap_preserves_an_unrelated_file_at_the_checklist_path() {
    let dir = repo("full");
    let root = dir.path();
    fs::create_dir_all(root.join("docs")).unwrap();
    let path = root.join("docs/capability-bootstrap.md");
    let notes =
        "# Project bootstrap notes\n\nThese notes are unrelated to catalog stage tracking.\n";
    fs::write(&path, notes).unwrap();
    let result = bootstrap(root);
    assert!(result.contains("next stage: inventory"), "{result}");
    assert_eq!(fs::read_to_string(path).unwrap(), notes);
    assert_eq!(files(root, "epics").len(), 1);
    assert_eq!(files(root, "tasks").len(), 4);
    validate(root);
}
