//! Dogfood parity for the Claude-authored skills (`assets/base/claude/skills/`):
//! this repo deploys its own scaffold, so `.claude/skills/` and
//! `.agents/skills/` must stay byte-identical to the shipped source or the two
//! silently diverge (the failure mode this test pins: `cf-method`'s SKILL.md
//! drifted behind its `assets/base` source until a manual resync).

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every skill directory name under `assets/base/claude/skills/` (the
/// Claude-authored skills; `assets/base/agents/skills/` covers the
/// harness-agnostic ones separately).
fn shipped_claude_skill_names() -> Vec<String> {
    let dir = repo_root().join("assets/base/claude/skills");
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.expect("dir entry");
        let is_dir = entry.file_type().expect("file type").is_dir();
        if !is_dir {
            continue;
        }
        let name = entry.file_name().into_string().expect("utf-8 dir name");
        names.push(name);
    }
    assert!(!names.is_empty(), "no skill directories found under {}", dir.display());
    names.sort();
    names
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn dogfood_claude_skills_match_shipped_scaffold() {
    let root = repo_root();
    for name in shipped_claude_skill_names() {
        let shipped = root.join(format!("assets/base/claude/skills/{name}/SKILL.md"));
        let shipped_text = read(&shipped);

        for deployed_dir in [".claude/skills", ".agents/skills"] {
            let deployed = root.join(format!("{deployed_dir}/{name}/SKILL.md"));
            let deployed_text = read(&deployed);
            assert_eq!(
                shipped_text, deployed_text,
                "{deployed_dir}/{name}/SKILL.md drifted from assets/base/claude/skills/{name}/SKILL.md"
            );
        }
    }
}
