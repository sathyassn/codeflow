//! TSK-1044 AC-2 through the real binary: a full gate writes its run record,
//! its target logs and its durable copy only inside the evidence folders,
//! whatever links, hard links or special files are planted there, by a
//! session before the run or by a target during it.
use super::{git, repo, run, stderr};
use std::path::{Path, PathBuf};

/// A repository with one check, `planter` (which runs `plant`, a shell line
/// with `$RUN` set to the run folder) before `victim`, committed so a full
/// run copies its evidence to the home.
fn repository(plant: &str) -> tempfile::TempDir {
    let planter = format!("RUN=target/codeflow-gate/$CODEFLOW_GATE_RUN_ID; {plant}");
    let config = serde_json::json!({"schema_version":"1.0","execution":{"parallel":false},"targets":[
        {"name":"planter","runner":"custom","modes":{"full":{"command":planter}}},
        {"name":"victim","runner":"custom","requires":["planter"],"modes":{"full":{"command":"echo victim-output"}}}
    ]});
    let dir = repo(&config.to_string());
    std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "test: planted evidence"]);
    dir
}

/// A folder outside every evidence folder holding the file `victim`.
fn outside() -> (tempfile::TempDir, PathBuf) {
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("victim");
    std::fs::write(&victim, "victim").unwrap();
    (outside, victim)
}

/// The outside folder still holds only the unchanged victim file.
fn unchanged(outside: &Path) {
    let names: Vec<_> = std::fs::read_dir(outside)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["victim"], "written outside the evidence folders");
    assert_eq!(std::fs::read(outside.join("victim")).unwrap(), b"victim");
}

fn refused(output: &std::process::Output) -> String {
    let text = stderr(output);
    assert!(
        !output.status.success(),
        "a planted entry was accepted: {text}"
    );
    assert!(text.contains("gate evidence refused at"), "{text}");
    text
}

/// The original finding: a link at the durable evidence folder or at the
/// repository's folder inside it redirected the copy outside.
#[test]
fn a_link_planted_in_the_durable_evidence_folder_is_refused() {
    let dir = repository("true");
    let home = tempfile::tempdir().unwrap();
    let first = run(dir.path(), home.path(), &["test", "--mode", "full"]);
    assert!(first.status.success(), "{}", stderr(&first));
    let artifact = PathBuf::from(
        stderr(&first)
            .lines()
            .find_map(|line| line.strip_prefix("[codeflow test] durable artifact: "))
            .unwrap(),
    );
    let repository_folder = artifact.parent().unwrap().parent().unwrap().to_owned();
    let evidence = home.path().join("gate-runs");
    for planted in [evidence.clone(), repository_folder] {
        let (outside, _) = outside();
        std::fs::remove_dir_all(&evidence).unwrap();
        std::fs::create_dir_all(planted.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(outside.path(), &planted).unwrap();
        let output = run(dir.path(), home.path(), &["test", "--mode", "full"]);
        let text = refused(&output);
        assert!(text.contains("is a symbolic link"), "{text}");
        unchanged(outside.path());
        assert!(std::fs::symlink_metadata(&planted).unwrap().is_symlink());
    }
}

/// A target planted a link at the run record's name, at another target's
/// log, or in the run folder: the gate refuses, and the victim is neither
/// changed nor copied into the evidence.
#[test]
fn links_a_target_plants_in_the_run_folder_are_refused() {
    let (outside, victim) = outside();
    let victim = victim.display().to_string();
    for plant in [
        format!("ln -s '{victim}' \"$RUN/run.json\""),
        format!("mkdir -p \"$RUN/victim\" && ln -s '{victim}' \"$RUN/victim/stdout.log\""),
        format!("ln -s '{victim}' \"$RUN/leak\""),
    ] {
        let dir = repository(&plant);
        let home = tempfile::tempdir().unwrap();
        let output = run(dir.path(), home.path(), &["test", "--mode", "full"]);
        let text = refused(&output);
        unchanged(outside.path());
        no_victim_copied(home.path(), &text);
    }
}

/// A pipe or a hard link a target plants in the run folder is refused and
/// never copied; the gate does not block on the pipe. (Not run against the
/// earlier copier, which blocks forever on the pipe.)
#[test]
fn a_pipe_or_hard_link_in_the_run_folder_is_refused() {
    for hard in [false, true] {
        let dir = repository("true");
        // The hard link must share the run folder's file system.
        let near = dir.path().join("untracked-victim");
        std::fs::write(&near, "victim").unwrap();
        let plant = if hard {
            format!("ln '{}' \"$RUN/leak\"", near.display())
        } else {
            "mkfifo \"$RUN/leak\"".to_string()
        };
        let path = dir.path().join(".codeflow/test-config.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        config["targets"][0]["modes"]["full"]["command"] = serde_json::json!(format!(
            "RUN=target/codeflow-gate/$CODEFLOW_GATE_RUN_ID; {plant}"
        ));
        std::fs::write(&path, config.to_string()).unwrap();
        let home = tempfile::tempdir().unwrap();
        let output = run(dir.path(), home.path(), &["test", "--mode", "full"]);
        let text = refused(&output);
        assert!(text.contains("leak"), "{text}");
        if hard {
            assert!(text.contains("hard link"), "{text}");
        }
        assert_eq!(std::fs::read(&near).unwrap(), b"victim");
        no_victim_copied(home.path(), &text);
    }
}

fn no_victim_copied(home: &Path, text: &str) {
    let mut copied = Vec::new();
    files_below(&home.join("gate-runs"), &mut copied);
    for path in copied {
        assert_ne!(
            std::fs::read(&path).unwrap_or_default(),
            b"victim",
            "victim copied to {}: {text}",
            path.display()
        );
    }
}

fn files_below(path: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if std::fs::symlink_metadata(&path).unwrap().is_dir() {
            files_below(&path, files);
        } else {
            files.push(path);
        }
    }
}

/// TSK-1044 review 1: a link planted at the build-output folder (`target`,
/// or the last component of an absolute `CARGO_TARGET_DIR`) or at the
/// `CodeFlow` home itself is refused before any write; the folder it points
/// at stays unchanged.
#[test]
fn a_link_at_the_build_output_folder_or_the_home_is_refused() {
    for case in ["target", "absolute target", "home"] {
        let dir = repository("true");
        let (outside, _) = outside();
        let parent = tempfile::tempdir().unwrap();
        let mut home = parent.path().join("home");
        let mut command = super::command(dir.path(), &home, &["test", "--mode", "full"]);
        match case {
            "target" => {
                std::os::unix::fs::symlink(outside.path(), dir.path().join("target")).unwrap();
            }
            "absolute target" => {
                let link = parent.path().join("build");
                std::os::unix::fs::symlink(outside.path(), &link).unwrap();
                command.env("CARGO_TARGET_DIR", &link);
            }
            _ => {
                home = parent.path().join("linked-home");
                std::os::unix::fs::symlink(outside.path(), &home).unwrap();
                command.env("CODEFLOW_HOME", &home);
            }
        }
        let output = command.output().unwrap();
        let text = stderr(&output);
        assert!(!output.status.success(), "{case}: accepted: {text}");
        assert!(text.contains("is a symbolic link"), "{case}: {text}");
        unchanged(outside.path());
    }
}

/// TSK-1044 review 1: a report whose folder is a link is refused, so the
/// outside file of that name is never copied into the run folder.
#[test]
fn a_report_under_a_linked_folder_is_never_copied() {
    let (outside, _) = outside();
    std::fs::write(
        outside.path().join("junit.xml"),
        "<testsuites/>outside-secret-7f3",
    )
    .unwrap();
    let dir = repository("true");
    let path = dir.path().join(".codeflow/test-config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["targets"][1]["report"] =
        serde_json::json!({"format": "junit", "path": "reports/junit.xml"});
    std::fs::write(&path, config.to_string()).unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("reports")).unwrap();
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test", "--mode", "full"]);
    let text = stderr(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("report") && text.contains("refused"),
        "{text}"
    );
    let mut copied = Vec::new();
    files_below(&dir.path().join("target/codeflow-gate"), &mut copied);
    files_below(&home.path().join("gate-runs"), &mut copied);
    for path in copied {
        let bytes = std::fs::read(&path).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("outside-secret-7f3"),
            "outside report copied to {}",
            path.display()
        );
    }
}

/// Grok review 2: before a nextest target runs, the gate removes its stale
/// report. A link planted above that report, or at it, is refused, and the
/// outside file of the same name is never deleted.
#[test]
fn a_stale_nextest_report_under_a_link_is_never_deleted() {
    for leaf in [false, true] {
        let (outside, _) = outside();
        let report = outside.path().join("junit.xml");
        std::fs::write(&report, "outside-secret-7f3").unwrap();
        let dir = repository("true");
        let path = dir.path().join(".codeflow/test-config.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        config["targets"][1]["modes"]["full"]["command"] = serde_json::json!("echo nextest");
        config["targets"][1]["report"] =
            serde_json::json!({"format": "junit", "path": "reports/junit.xml"});
        std::fs::write(&path, config.to_string()).unwrap();
        let reports = dir.path().join("reports");
        if leaf {
            std::fs::create_dir(&reports).unwrap();
            std::os::unix::fs::symlink(&report, reports.join("junit.xml")).unwrap();
        } else {
            std::os::unix::fs::symlink(outside.path(), &reports).unwrap();
        }
        let home = tempfile::tempdir().unwrap();
        let output = run(dir.path(), home.path(), &["test", "--mode", "full"]);
        let text = stderr(&output);
        assert_eq!(
            std::fs::read(&report).ok().as_deref(),
            Some(b"outside-secret-7f3".as_slice()),
            "leaf {leaf}: the outside report was deleted"
        );
        assert!(!output.status.success(), "leaf {leaf}: {text}");
        assert!(text.contains("stale report"), "leaf {leaf}: {text}");
    }
}

/// Grok review 2: a producer output reached only through a link planted
/// above it is refused, never counted as present.
#[test]
fn a_producer_output_under_a_link_is_never_present() {
    let (outside, _) = outside();
    std::fs::write(outside.path().join("product"), "outside").unwrap();
    let dir = repository("true");
    let path = dir.path().join(".codeflow/test-config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["targets"][0]["outputs"] = serde_json::json!(["out/product"]);
    std::fs::write(&path, config.to_string()).unwrap();
    std::fs::write(dir.path().join(".gitignore"), "target/\nout\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "test: declared output"]);
    std::os::unix::fs::symlink(outside.path(), dir.path().join("out")).unwrap();
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test", "--mode", "full"]);
    let text = stderr(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("producer output refused:")
            && text.contains("out/product: out is a symbolic link"),
        "{text}"
    );
}
