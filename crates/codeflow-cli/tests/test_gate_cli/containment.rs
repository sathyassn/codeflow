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
