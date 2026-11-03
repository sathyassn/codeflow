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

#[test]
fn standard_with_tracking_creates_ordered_work_and_resumes_each_boundary() {
    let dir = repo("standard");
    let root = dir.path();
    // Standard projects activate durable tracking when they carry a task.
    success(cli(
        root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "Establish sample work tracking",
            "Seed sample tracking",
        ],
    ));
    let result = bootstrap(root);
    assert!(result.contains("next stage: inventory"), "{result}");
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
        fs::write(path, text.replace("status: todo", "status: complete")).unwrap();
        let next = names[(index + 1).min(3)];
        assert!(bootstrap(root).contains(&format!("next stage: {next}")));
        assert_eq!(files(root, "epics"), epics);
        assert_eq!(files(root, "tasks").len(), 5);
    }
}

#[test]
fn fresh_standard_and_explicit_minimal_get_a_resumable_checklist() {
    for tier in ["standard", "minimal"] {
        let dir = repo(tier);
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
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(files(root, "epics").len(), 1);
    assert_eq!(files(root, "tasks").len(), 4);
}

#[test]
fn non_rust_sample_reaches_its_first_valid_catalog_through_four_stages() {
    let dir = repo("standard");
    let root = dir.path();
    println!("bootstrap: {}", bootstrap(root));
    let checklist = root.join("docs/capability-bootstrap.md");
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
    for stage in ["inventory", "classification", "evidence"] {
        let text = fs::read_to_string(&checklist).unwrap();
        fs::write(
            &checklist,
            text.replace(&format!("- [ ] {stage}"), &format!("- [x] {stage}")),
        )
        .unwrap();
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
    let text = fs::read_to_string(&checklist).unwrap();
    fs::write(&checklist, text.replace("- [ ] review", "- [x] review")).unwrap();
    let output = bootstrap(root);
    println!("review: {output}");
    assert!(output.contains("inventory is complete"));
    assert!(!root.join("project-management/tasks").exists());
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
