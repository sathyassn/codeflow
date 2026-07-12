//! Self-consistency guards for the shipped scaffold artifacts.
//!
//! Two classes of mistake are easy to make and stay invisible until a consumer
//! runs `codeflow init`:
//!
//!   1. authoring a skill/agent asset into `assets/base` but forgetting its
//!      `scaffold-manifest.toml` entry — so the file never actually ships;
//!   2. letting the cross-harness skill mirrors (`.claude/skills` vs
//!      `.agents/skills`) drift out of lockstep.
//!
//! These tests run against the REAL repo tree — not the tempdir fixtures the
//! rest of the scaffold suite uses — and fail loudly, naming the offending
//! file. The repo is its own first consumer, so the shipped manifest and the
//! shipped mirrors must stay honest here first.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use codeflow_core::scaffold::{DirSource, ScaffoldManifest};

/// Repo root, resolved from this crate's manifest dir (`crates/codeflow-core`).
/// Same locator the manifest unit test uses (`env!("CARGO_MANIFEST_DIR")` up two
/// levels), so the two stay in step.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// Every regular file under `dir`, recursively, as absolute paths.
fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk_files(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}

/// Path of `path` relative to `base`, as a forward-slash string (matching the
/// `src`/`dest` spelling the manifest uses).
fn rel(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .expect("path under base")
        .to_string_lossy()
        .replace('\\', "/")
}

/// Immediate subdirectory names of `dir` (empty when `dir` is absent).
fn dir_names(dir: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                names.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    names
}

/// MANIFEST-COMPLETENESS: every shippable artifact authored into `assets/base`
/// must be referenced as a manifest `src`, or it silently never ships. This is
/// the check that would have caught `cf-security-reviewer.md` being authored
/// into the asset tree but omitted from the manifest.
///
/// Scoped by predicate to the three per-file artifact shapes — every
/// `**/skills/**/SKILL.md`, every `**/skills/**/resources/*`, and every
/// `claude/agents/*.md` — so embedded, not-shipped-per-file assets (e.g.
/// `assets/base/testing/*`) never false-positive.
#[test]
fn every_shipped_skill_and_agent_asset_is_in_the_manifest() {
    let assets_dir = repo_root().join("assets");
    let base = assets_dir.join("base");

    // Every `src` the manifest ships (relative to assets/base). A src may appear
    // under multiple dests (the cross-harness mirror); the set dedups them.
    let manifest =
        ScaffoldManifest::load(&DirSource::new(&assets_dir)).expect("shipped manifest loads");
    let srcs: BTreeSet<&str> = manifest.entries.iter().map(|e| e.src.as_str()).collect();

    let mut missing = Vec::new();
    for file in walk_files(&base) {
        let r = rel(&base, &file);
        let parts: Vec<&str> = r.split('/').collect();
        let is_skill_md = parts.contains(&"skills") && parts.last() == Some(&"SKILL.md");
        let is_skill_resource = parts.contains(&"skills") && parts.contains(&"resources");
        let is_claude_agent = parts.len() == 3
            && parts[0] == "claude"
            && parts[1] == "agents"
            && file.extension().and_then(|e| e.to_str()) == Some("md");

        if (is_skill_md || is_skill_resource || is_claude_agent) && !srcs.contains(r.as_str()) {
            missing.push(r);
        }
    }
    missing.sort();

    assert!(
        missing.is_empty(),
        "these shippable assets are NOT referenced as a manifest `src` (add a \
         scaffold-manifest.toml [[entry]] or they will never ship):\n  {}",
        missing.join("\n  ")
    );
}

/// REPO MIRROR-PARITY: the two cross-harness skill mirrors this repo ships to
/// itself must stay byte-identical. For every skill present in BOTH
/// `.claude/skills/<name>/` and `.agents/skills/<name>/`, the file sets must
/// match and every shared file (SKILL.md + any resources/*) must be
/// byte-identical — catching lockstep drift between the harness copies.
///
/// This compares only the repo's own `.claude`/`.agents` mirrors, never the
/// `assets/base` templates, which are allowed to differ (e.g. cf-method carries
/// harness-aware wording in its asset template).
#[test]
fn repo_skill_mirrors_are_byte_identical() {
    let root = repo_root();
    let claude = root.join(".claude/skills");
    let agents = root.join(".agents/skills");

    let shared: BTreeSet<String> = dir_names(&claude)
        .intersection(&dir_names(&agents))
        .cloned()
        .collect();
    assert!(
        !shared.is_empty(),
        "expected mirrored skills in both .claude/skills and .agents/skills"
    );

    let mut problems = Vec::new();
    for skill in &shared {
        let cdir = claude.join(skill);
        let adir = agents.join(skill);
        let cfiles: BTreeSet<String> = walk_files(&cdir).iter().map(|p| rel(&cdir, p)).collect();
        let afiles: BTreeSet<String> = walk_files(&adir).iter().map(|p| rel(&adir, p)).collect();

        for f in cfiles.symmetric_difference(&afiles) {
            problems.push(format!("{skill}/{f}: present in only one mirror"));
        }
        for f in cfiles.intersection(&afiles) {
            let cb = std::fs::read(cdir.join(f)).expect("read .claude copy");
            let ab = std::fs::read(adir.join(f)).expect("read .agents copy");
            if cb != ab {
                problems.push(format!(
                    "{skill}/{f}: .claude and .agents copies differ (byte drift)"
                ));
            }
        }
    }
    problems.sort();

    assert!(
        problems.is_empty(),
        "cross-harness skill mirrors drifted (keep .claude/skills and \
         .agents/skills byte-identical):\n  {}",
        problems.join("\n  ")
    );
}
