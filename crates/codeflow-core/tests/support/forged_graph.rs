//! Plant a real v1 graph, then make one cached parent skip a commit.
use std::path::Path;

pub fn plant(root: &Path, head: &str, parent: &str) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "commitGraph.generationVersion=1",
            "commit-graph",
            "write",
            "--reachable",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let enabled = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "--local", "core.commitGraph", "true"])
        .output()
        .unwrap();
    assert!(enabled.status.success());
    let file = root.join(".git/objects/info/commit-graph");
    let mut bytes = std::fs::read(&file).unwrap();
    let offset = |name: &[u8]| {
        bytes[8..]
            .chunks_exact(12)
            .take(usize::from(bytes[6]))
            .find(|entry| &entry[..4] == name)
            .map(|entry| {
                usize::try_from(u64::from_be_bytes(entry[4..12].try_into().unwrap())).unwrap()
            })
            .unwrap()
    };
    let lookup = offset(b"OIDL");
    let data = offset(b"CDAT");
    let position = |oid: &str| {
        let oid: Vec<u8> = (0..oid.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&oid[index..index + 2], 16).unwrap())
            .collect();
        bytes[lookup..data]
            .chunks_exact(20)
            .position(|entry| entry == oid)
            .unwrap()
    };
    let head_position = position(head);
    let parent_position = u32::try_from(position(parent)).unwrap();
    let parent_offset = data + head_position * 36 + 20;
    bytes[parent_offset..parent_offset + 4].copy_from_slice(&parent_position.to_be_bytes());
    std::fs::remove_file(&file).unwrap();
    std::fs::write(file, bytes).unwrap();
    let count = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.commitGraph=true",
            "rev-list",
            "--count",
            &format!("{parent}..HEAD"),
        ])
        .output()
        .unwrap();
    assert!(count.status.success(), "{count:?}");
    if String::from_utf8_lossy(&count.stdout).trim() != "2" {
        let verify = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["commit-graph", "verify"])
            .output()
            .unwrap();
        panic!("graph did not hide middle: count={count:?} verify={verify:?}, positions {head_position}/{parent_position}, data={data}");
    }
}
