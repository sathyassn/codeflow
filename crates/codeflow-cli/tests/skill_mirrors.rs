//! Dogfood parity for every shipped skill: this repo deploys its own
//! scaffold, so `.claude/skills/` and `.agents/skills/` must stay
//! byte-identical to the shipped sources — both the Claude-authored ones
//! (`assets/base/claude/skills/`) and the harness-agnostic ones
//! (`assets/base/agents/skills/`), including any `resources/*` files — or
//! the deployed copies silently diverge from what consumers get. The
//! failure mode this pins: `cf-method`'s deployed SKILL.md drifted behind
//! its `assets/base` source until a manual resync (its asset once carried
//! deliberately divergent harness-aware wording; since the merge it is
//! pinned byte-identical like every other skill).
//!
//! The cross-mirror check (`.claude` vs `.agents`) lives in
//! `codeflow-core/tests/manifest_consistency.rs`; this test pins both
//! deployed copies to the SOURCE, so coordinated drift of the two mirrors
//! away from `assets/base` cannot pass.

use std::path::{Path, PathBuf};

/// The two shipped skill source roots, relative to the repo root.
const SHIPPED_SKILL_ROOTS: [&str; 2] = ["assets/base/claude/skills", "assets/base/agents/skills"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every skill directory under `root_rel`, as `(name, absolute path)`.
fn skill_dirs(root_rel: &str) -> Vec<(String, PathBuf)> {
    let dir = repo_root().join(root_rel);
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries {
        let entry = entry.expect("dir entry");
        if !entry.file_type().expect("file type").is_dir() {
            continue;
        }
        let name = entry.file_name().into_string().expect("utf-8 dir name");
        out.push((name, entry.path()));
    }
    out.sort();
    out
}

/// Every regular file under `dir`, recursively, relative forward-slash paths.
fn walk_rel(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .expect("path under base")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn dogfood_skills_match_shipped_scaffold() {
    let root = repo_root();
    let mut seen: Vec<String> = Vec::new();

    for shipped_root in SHIPPED_SKILL_ROOTS {
        let skills = skill_dirs(shipped_root);
        assert!(
            !skills.is_empty(),
            "no skill directories found under {shipped_root}"
        );

        for (name, shipped_dir) in skills {
            assert!(
                !seen.contains(&name),
                "skill {name} is authored under both shipped roots — ambiguous source"
            );
            seen.push(name.clone());

            let shipped_files = walk_rel(&shipped_dir);
            assert!(
                shipped_files.iter().any(|f| f == "SKILL.md"),
                "{shipped_root}/{name} has no SKILL.md"
            );

            for deployed_root in [".claude/skills", ".agents/skills"] {
                let deployed_dir = root.join(deployed_root).join(&name);
                assert_eq!(
                    shipped_files,
                    walk_rel(&deployed_dir),
                    "{deployed_root}/{name}: file set differs from {shipped_root}/{name}"
                );
                for file in &shipped_files {
                    assert!(
                        read(&shipped_dir.join(file)) == read(&deployed_dir.join(file)),
                        "{deployed_root}/{name}/{file} drifted from {shipped_root}/{name}/{file}"
                    );
                }
            }
        }
    }
}
