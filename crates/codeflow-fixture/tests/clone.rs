use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn child_started(trace: &Path, name: &str) -> bool {
    let events: Vec<serde_json::Value> = std::fs::read_to_string(trace)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(events.iter().any(|event| event["event"] == "start"));
    events.iter().any(|event| {
        event["event"] == "child_start"
            && event["argv"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| arg.as_str().unwrap().contains(name))
    })
}

#[cfg(unix)]
fn object_inodes(root: &Path) -> std::collections::BTreeSet<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    fn collect(root: &Path, found: &mut std::collections::BTreeSet<(u64, u64)>) {
        for entry in std::fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                collect(&entry.path(), found);
            } else {
                found.insert((metadata.dev(), metadata.ino()));
            }
        }
    }
    let mut found = std::collections::BTreeSet::new();
    collect(&root.join(".git/objects"), &mut found);
    assert!(!found.is_empty());
    found
}

#[test]
fn the_helper_uses_transport_and_owns_its_objects() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let source = root.join("source");
    std::fs::create_dir(&source).unwrap();
    git(&source, &["init", "-q"]);
    std::fs::write(source.join("tracked"), "fixture content").unwrap();
    git(&source, &["add", "."]);
    git(
        &source,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    let trace = root.join("transport.jsonl");
    let destination = root.join("transport");
    codeflow_fixture::clone(root, &source, &destination)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TRACE2_EVENT", &trace)
        .run();
    assert!(child_started(&trace, "upload-pack"));
    assert!(child_started(&trace, "pack-objects"));
    #[cfg(unix)]
    assert!(object_inodes(&source).is_disjoint(&object_inodes(&destination)));

    // The ordinary local clone is the discriminating control, on the same drive.
    let control = root.join("local");
    let trace = root.join("local.jsonl");
    let output = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .arg("clone")
        .arg(&source)
        .arg(&control)
        .env("GIT_TRACE2_EVENT", &trace)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Git 2.53 also uses upload-pack for local reference discovery. Only the
    // transport path transfers a pack; the local path shares object inodes.
    assert!(!child_started(&trace, "pack-objects"));
    #[cfg(unix)]
    assert!(!object_inodes(&source).is_disjoint(&object_inodes(&control)));
}
