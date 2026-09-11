//! Self-consistency guards for the shipped scaffold artifacts.
//!
//! Three classes of mistake are easy to make and stay invisible until a
//! consumer runs `codeflow init`:
//!
//!   1. authoring an asset into `assets/base` but forgetting its
//!      `scaffold-manifest.toml` entry — so the file never actually ships;
//!   2. leaving a manifest `src` pointing at a renamed or deleted asset — the
//!      engine's missing-asset grace skips it with only a report line, so the
//!      artifact silently un-ships;
//!   3. letting the cross-harness skill mirrors (`.claude/skills` vs
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

#[test]
fn repository_runtime_state_never_enters_the_tracked_tree() {
    let root = repo_root();
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output()
        .expect("git is available for repository hygiene verification");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut forbidden: Vec<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).replace('\\', "/"))
        .filter(|path| {
            path.starts_with(".state/")
                || path.starts_with(".claude/memory/")
                || path.ends_with(".profraw")
        })
        .collect();
    forbidden.sort();
    assert!(
        forbidden.is_empty(),
        "repository-local runtime state is tracked:\n  {}",
        forbidden.join("\n  ")
    );

    let ignore = std::fs::read_to_string(root.join(".gitignore"))
        .expect("repository .gitignore is readable");
    let lines: BTreeSet<&str> = ignore.lines().map(str::trim).collect();
    for required in ["/.state/", "/.claude/memory/", "*.profraw"] {
        assert!(
            lines.contains(required),
            "repository .gitignore must retain root-scoped runtime guard {required}"
        );
    }
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

#[test]
fn ci_downloads_verify_pinned_checksums() {
    let workflow = std::fs::read_to_string(repo_root().join("assets/base/ci/codeflow-ci.yml"))
        .expect("shipped CI workflow is readable");

    for required in [
        "GITLEAKS_VERSION=8.30.1",
        "GITLEAKS_SHA256=551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb",
        "OSV_VERSION=2.4.0",
        "OSV_SHA256=15314940c10d26af9c6649f150b8a47c1262e8fc7e17b1d1029b0e479e8ed8a0",
    ] {
        assert!(
            workflow.contains(required),
            "shipped CI workflow is missing pinned tool evidence: {required}"
        );
    }
    assert_eq!(
        workflow.matches("sha256sum -c -").count(),
        2,
        "both downloaded security tools must be verified before execution"
    );
}

#[test]
fn codeql_remains_repository_owned_not_a_scaffolded_workflow() {
    let root = repo_root();
    let mut violations = Vec::new();
    for directory in [root.join(".github/workflows"), root.join("assets/base/ci")] {
        for file in walk_files(&directory) {
            let content = std::fs::read_to_string(&file)
                .unwrap_or_else(|error| panic!("read {}: {error}", file.display()));
            if content.to_ascii_lowercase().contains("codeql") {
                violations.push(rel(&root, &file));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "CodeQL is enabled as CodeFlow repository default setup after the repo \
         becomes public; it must not become a portable scaffold/CI dependency: {}",
        violations.join(", ")
    );
}

/// Files under `assets/base` that deliberately ship WITHOUT a manifest entry.
/// Exact and honest: `every_authored_asset_is_in_the_manifest` fails when an
/// entry here goes stale (file deleted, or wired into the manifest after all).
///
/// - the manifest itself is the shipping instruction, not a shipped artifact;
/// - the alternate CI wrappers are copy-in-by-hand only — the platform picker
///   and their manifest wiring are an explicitly deferred follow-up
///   (`assets/base/ci/README.md`, ADR-0017).
const UNSHIPPED_FILES: [&str; 5] = [
    "scaffold-manifest.toml",
    "ci/README.md",
    "ci/.gitlab-ci.yml",
    "ci/bitbucket-pipelines.yml",
    "ci/ci-generic.sh",
];

/// Directory prefixes under `assets/base` whose files are embedded and
/// consumed by the binary at runtime, never scaffolded per-file:
/// `codeflow test setup` reads the testing templates and schema directly.
const UNSHIPPED_PREFIXES: [&str; 1] = ["testing/"];

/// MANIFEST-COMPLETENESS: every artifact authored into `assets/base` must be
/// referenced as a manifest `src`, or it silently never ships. This is the
/// check that would have caught `cf-security-reviewer.md` being authored into
/// the asset tree but omitted from the manifest.
///
/// Inverted with an explicit allowlist rather than scoped by artifact-shape
/// predicate, so every shipped-per-file class is covered — skills, agents,
/// resources, workflows, CI, codex, settings presets, git-hook shims, pm and
/// docs templates, and the root contract files alike. Deliberately-unshipped
/// files live in [`UNSHIPPED_FILES`] / [`UNSHIPPED_PREFIXES`], which this test
/// also keeps honest.
#[test]
fn every_authored_asset_is_in_the_manifest() {
    let assets_dir = repo_root().join("assets");
    let base = assets_dir.join("base");

    // Every `src` the manifest ships (relative to assets/base). A src may appear
    // under multiple dests (the cross-harness mirror); the set dedups them.
    let manifest =
        ScaffoldManifest::load(&DirSource::new(&assets_dir)).expect("shipped manifest loads");
    let srcs: BTreeSet<&str> = manifest.entries.iter().map(|e| e.src.as_str()).collect();

    let allowlisted = |r: &str| {
        UNSHIPPED_FILES.contains(&r) || UNSHIPPED_PREFIXES.iter().any(|p| r.starts_with(p))
    };

    let mut missing = Vec::new();
    for file in walk_files(&base) {
        let r = rel(&base, &file);
        // Finder writes .DS_Store under assets/base on macOS; it is not a
        // scaffold artifact. Do not allowlist it — the file is absent on CI.
        if r.rsplit('/').next() == Some(".DS_Store") {
            continue;
        }
        if !allowlisted(&r) && !srcs.contains(r.as_str()) {
            missing.push(r);
        }
    }
    missing.sort();
    assert!(
        missing.is_empty(),
        "these authored assets are NOT referenced as a manifest `src` (add a \
         scaffold-manifest.toml [[entry]], or allowlist a deliberately-unshipped \
         file here with its rationale):\n  {}",
        missing.join("\n  ")
    );

    // Keep the allowlist honest: an entry that no longer exists, or that got
    // wired into the manifest after all, must be removed from it.
    for name in UNSHIPPED_FILES {
        assert!(
            base.join(name).is_file(),
            "UNSHIPPED_FILES entry {name} no longer exists under assets/base — remove it"
        );
        assert!(
            !srcs.contains(name),
            "UNSHIPPED_FILES entry {name} is now a manifest src — remove it from the allowlist"
        );
    }
}

/// MANIFEST-RESOLUTION (the reverse direction): every manifest `src` must
/// resolve to an existing file under `assets/base`. The engine tolerates a
/// missing src by design (skip + warning — parallel authoring grace at a
/// consumer), which is exactly why the shipped tree needs this repo-level
/// guard: a renamed or typo'd src otherwise un-ships the artifact with
/// nothing red in CI.
#[test]
fn every_manifest_src_resolves_to_a_shipped_asset() {
    let assets_dir = repo_root().join("assets");
    let base = assets_dir.join("base");

    let manifest =
        ScaffoldManifest::load(&DirSource::new(&assets_dir)).expect("shipped manifest loads");

    let mut dangling: Vec<String> = manifest
        .entries
        .iter()
        .filter(|e| !base.join(&e.src).is_file())
        .map(|e| format!("{} (dest {})", e.src, e.dest))
        .collect();
    dangling.sort();
    dangling.dedup();

    assert!(
        dangling.is_empty(),
        "these manifest `src`s do not exist under assets/base (renamed or \
         typo'd — the artifact would silently never ship):\n  {}",
        dangling.join("\n  ")
    );
}

#[test]
fn portal_bundle_is_single_complete_and_bounded() {
    let root = repo_root();
    let assets = root.join("assets");
    let bundle = assets.join("docs-portal/starter");
    let manifest_path = assets.join("docs-portal/manifest.json");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&manifest_path).expect("portal manifest is readable"),
    )
    .expect("portal manifest is JSON");
    assert_eq!(manifest["schema_version"], 1);
    let files = manifest["files"].as_array().expect("portal files array");
    let declared: BTreeSet<String> = files
        .iter()
        .map(|entry| {
            entry["path"]
                .as_str()
                .expect("portal path string")
                .to_string()
        })
        .collect();
    assert_eq!(declared.len(), files.len(), "portal paths must be unique");
    let bundle_files: Vec<PathBuf> = walk_files(&bundle)
        .into_iter()
        .filter(|path| {
            !path
                .strip_prefix(&bundle)
                .expect("portal file is under bundle")
                .components()
                .any(|component| component.as_os_str() == "node_modules")
        })
        .collect();
    let authored: BTreeSet<String> = bundle_files.iter().map(|path| rel(&bundle, path)).collect();
    assert_eq!(
        authored, declared,
        "portal manifest must cover the starter exactly"
    );
    let unpacked: u64 = bundle_files
        .iter()
        .map(|path| std::fs::metadata(path).expect("portal file metadata").len())
        .sum();
    assert!(
        unpacked <= 2 * 1024 * 1024,
        "portal starter is {unpacked} bytes"
    );
    let package_lock: serde_json::Value = serde_json::from_slice(
        &std::fs::read(bundle.join("package-lock.json")).expect("portal lockfile is readable"),
    )
    .expect("portal lockfile is JSON");
    assert_eq!(
        package_lock["packages"]["node_modules/astro"]["bin"]["astro"], "bin/astro.mjs",
        "the pinned Astro executable changed; update the workflow deliberately"
    );
    let workflow = std::fs::read_to_string(bundle.join("scripts/workflow.mjs"))
        .expect("portal workflow is readable");
    assert!(
        workflow.contains("path.join(\"node_modules\", \"astro\", \"bin\", \"astro.mjs\")"),
        "the portal workflow must execute the pinned Astro package entrypoint"
    );
    assert!(
        workflow.contains("env: hardenedChildEnvironment()"),
        "portal workflow children must receive the shared allowlisted environment"
    );
    let browser = std::fs::read_to_string(bundle.join("scripts/browser-verify.mjs"))
        .expect("portal browser verifier is readable");
    assert!(
        browser.contains("env: hardenedChildEnvironment(process.env, { BROWSER: \"none\" })")
            && browser.contains("env: hardenedChildEnvironment(),"),
        "portal preview and browser children must receive the shared allowlisted environment"
    );
    assert!(
        !browser.contains("env: { ...process.env"),
        "portal browser verification must not forward the ambient environment"
    );
    let package: serde_json::Value = serde_json::from_slice(
        &std::fs::read(bundle.join("package.json")).expect("portal package is readable"),
    )
    .expect("portal package is JSON");
    assert_eq!(
        package["scripts"]["deps:install"], "node scripts/install-dependencies.mjs",
        "the portal must expose its hardened locked-install entrypoint"
    );
    let installer = std::fs::read_to_string(bundle.join("scripts/install-dependencies.mjs"))
        .expect("portal dependency installer is readable");
    assert!(
        installer.contains("spawn(process.execPath, [npmCli, \"ci\", \"--ignore-scripts\", \"--no-audit\", \"--no-fund\"]")
            && installer.contains("env: hardenedChildEnvironment()"),
        "the portal dependency installer must use npm without a shell and disable lifecycle scripts under the allowlisted environment"
    );
    for forbidden in [
        root.join("assets/base/agents/skills/cf-docs-portal/package-lock.json"),
        root.join(".agents/skills/cf-docs-portal/package-lock.json"),
        root.join(".claude/skills/cf-docs-portal/package-lock.json"),
    ] {
        assert!(
            !forbidden.exists(),
            "lockfile escaped the opt-in bundle: {}",
            forbidden.display()
        );
    }
}

/// The repository guide is the first consumer of the shipped starter. Keep
/// project-owned configuration independent, but require every reusable starter
/// file to exist and remain byte-identical so a dogfood-only fix cannot pass
/// while consumers receive stale runtime or tests.
#[test]
fn portal_dogfood_runtime_matches_the_shipped_starter() {
    let root = repo_root();
    let starter = root.join("assets/docs-portal/starter");
    let dogfood = root.join("docs-portal");
    let mut problems = Vec::new();
    // Match the exact dependency-tree exclusion in EmbeddedAssets without
    // excluding any authored starter file or weakening the parity inventory.
    let sources: Vec<PathBuf> = std::fs::read_dir(&starter)
        .expect("starter directory is readable")
        .map(|entry| entry.expect("starter entry is readable").path())
        .filter(|path| path.file_name().is_none_or(|name| name != "node_modules"))
        .flat_map(|path| {
            if path.is_dir() {
                walk_files(&path)
            } else {
                vec![path]
            }
        })
        .collect();
    for source in sources {
        let relative = rel(&starter, &source);
        if relative == "portal.config.json" {
            continue;
        }
        let deployed = dogfood.join(&relative);
        if !deployed.is_file() {
            problems.push(format!("missing dogfood file {relative}"));
            continue;
        }
        if std::fs::read(&source).expect("read starter portal file")
            != std::fs::read(&deployed).expect("read dogfood portal file")
        {
            problems.push(format!("byte drift at {relative}"));
        }
    }
    assert!(
        problems.is_empty(),
        "docs-portal diverged from the shipped reusable starter:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn portal_skill_preserves_explicit_ownership_and_integrity_contracts() {
    let skill = std::fs::read_to_string(
        repo_root().join("assets/base/agents/skills/cf-docs-portal/SKILL.md"),
    )
    .expect("canonical portal skill is readable");
    let operations = std::fs::read_to_string(
        repo_root().join("assets/base/agents/skills/cf-docs-portal/references/operations.md"),
    )
    .expect("portal operations are readable");
    for marker in [
        "there is no source merge",
        "confirm whole-runtime ownership",
        "Never transfer merely",
    ] {
        assert!(skill.contains(marker), "portal skill lost duty: {marker}");
    }
    for marker in [
        "journal before full adoption-state parsing",
        "stops the new migration or transfer before publication",
        "restore exact reviewed bytes",
        "Empty reserved directories are removed without",
        "Chromium, Firefox and WebKit",
    ] {
        assert!(
            operations.contains(marker),
            "portal operations lost duty: {marker}"
        );
    }
    assert!(
        !skill.contains("<path>.codeflow-<hash>.new"),
        "portal skill must not prescribe retired sidecars"
    );
}

/// MIRROR-SHIPPING: every skill the manifest ships must ship to BOTH harness
/// mirrors — a `.claude/skills/…` dest and an `.agents/skills/…` dest per
/// src. A skill added with only one dest reaches consumers half-mirrored.
#[test]
fn every_skill_src_ships_to_both_harness_mirrors() {
    let assets_dir = repo_root().join("assets");
    let manifest =
        ScaffoldManifest::load(&DirSource::new(&assets_dir)).expect("shipped manifest loads");

    let mut dests_by_src: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();
    for entry in &manifest.entries {
        if entry.src.split('/').any(|part| part == "skills") {
            dests_by_src
                .entry(entry.src.as_str())
                .or_default()
                .push(entry.dest.as_str());
        }
    }
    assert!(
        !dests_by_src.is_empty(),
        "expected skill entries in the shipped manifest"
    );

    let mut problems = Vec::new();
    for (src, dests) in &dests_by_src {
        for mirror in [".claude/skills/", ".agents/skills/"] {
            if !dests.iter().any(|d| d.starts_with(mirror)) {
                problems.push(format!("{src}: no {mirror}… dest — ships half-mirrored"));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "every skill src needs both a .claude/skills and an .agents/skills \
         dest in scaffold-manifest.toml:\n  {}",
        problems.join("\n  ")
    );
}

/// REPO MIRROR-PARITY: the two cross-harness skill mirrors this repo ships to
/// itself must stay byte-identical. The skill-directory sets themselves must
/// match (a skill present in only one mirror is drift, not a smaller
/// intersection), and within every shared skill the file sets must match and
/// every shared file (SKILL.md + any resources/*) must be byte-identical.
///
/// This compares only the repo's own `.claude`/`.agents` mirrors; the
/// `assets/base` sources are pinned to the deployed copies by the CLI crate's
/// `skill_mirrors.rs` dogfood test.
#[test]
fn repo_skill_mirrors_are_byte_identical() {
    let root = repo_root();
    let claude = root.join(".claude/skills");
    let agents = root.join(".agents/skills");

    let claude_names = dir_names(&claude);
    let agents_names = dir_names(&agents);
    let shared: BTreeSet<String> = claude_names.intersection(&agents_names).cloned().collect();
    assert!(
        !shared.is_empty(),
        "expected mirrored skills in both .claude/skills and .agents/skills"
    );

    let mut problems = Vec::new();
    for name in claude_names.symmetric_difference(&agents_names) {
        problems.push(format!(
            "{name}: skill directory present in only one mirror"
        ));
    }
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

fn assert_skill_source_live_and_baseline_copies(skill: &str) {
    let root = repo_root();
    let canonical = root.join("assets/base/agents/skills").join(skill);
    let copies = [
        root.join(".agents/skills").join(skill),
        root.join(".claude/skills").join(skill),
        root.join(".codeflow/.baseline/.agents/skills").join(skill),
        root.join(".codeflow/.baseline/.claude/skills").join(skill),
    ];
    let canonical_files: BTreeSet<String> = walk_files(&canonical)
        .iter()
        .map(|path| rel(&canonical, path))
        .collect();
    assert!(!canonical_files.is_empty(), "{skill} source is empty");

    let mut problems = Vec::new();
    for copy in copies {
        let copy_files: BTreeSet<String> = walk_files(&copy)
            .iter()
            .map(|path| rel(&copy, path))
            .collect();
        for file in canonical_files.symmetric_difference(&copy_files) {
            problems.push(format!("{}: file-set drift at {file}", copy.display()));
        }
        for file in canonical_files.intersection(&copy_files) {
            let expected = std::fs::read(canonical.join(file)).expect("read canonical file");
            let actual = std::fs::read(copy.join(file)).expect("read mirrored file");
            if expected != actual {
                problems.push(format!("{}: byte drift at {file}", copy.display()));
            }
        }
    }
    problems.sort();
    assert!(
        problems.is_empty(),
        "{skill} source/live/baseline copies drifted:\n  {}",
        problems.join("\n  ")
    );
}

/// The evaluator is stored in five source/live/baseline locations. Baseline
/// drift is especially dangerous because it corrupts future three-way updates
/// without changing the two active harness mirrors.
#[test]
fn model_eval_source_live_and_baseline_copies_are_byte_identical() {
    assert_skill_source_live_and_baseline_copies("cf-evaluate-model");
}

/// Model routing is consumed directly by both harnesses. Keep its managed
/// catalog and policy copies identical through dogfood updates.
#[test]
fn model_orchestrator_source_live_and_baseline_copies_are_byte_identical() {
    assert_skill_source_live_and_baseline_copies("cf-model-orchestrator");
}

/// The contextual editorial skill is managed in the same five source/live/
/// baseline locations. Pin the baseline too: active mirror parity alone cannot
/// detect a stale three-way merge base.
#[test]
fn editorial_source_live_and_baseline_copies_are_byte_identical() {
    assert_skill_source_live_and_baseline_copies("cf-editorial-review");
}

/// Design direction is also a managed cross-harness skill. Pin its source,
/// active mirrors, and update baselines as one contract.
#[test]
fn design_source_live_and_baseline_copies_are_byte_identical() {
    assert_skill_source_live_and_baseline_copies("cf-design");
}

/// Interactive presentation authoring is also a managed cross-harness skill.
/// Its examples and conditional reference are part of the contract, so pin the
/// complete directory rather than only SKILL.md.
#[test]
fn present_source_live_and_baseline_copies_are_byte_identical() {
    assert_skill_source_live_and_baseline_copies("cf-present");
}

/// Documentation-portal references include lifecycle and installer safety
/// duties, so pin the complete managed directory like the other rich skills.
#[test]
fn docs_portal_source_live_and_baseline_copies_are_byte_identical() {
    assert_skill_source_live_and_baseline_copies("cf-docs-portal");
}

/// Presentation JSON contracts are public consumer inputs and exported state.
/// Pin the authored copies to the deployed and three-way-merge baseline files,
/// and reject an accidentally open or malformed root contract.
#[test]
fn present_schema_source_live_and_baseline_copies_are_identical_and_closed() {
    let root = repo_root();
    let source = root.join("assets/base/present/schemas");
    let live = root.join(".codeflow/schemas/present");
    let baseline = root.join(".codeflow/.baseline/.codeflow/schemas/present");
    let expected: BTreeSet<String> = walk_files(&source)
        .iter()
        .map(|path| rel(&source, path))
        .collect();
    assert!(!expected.is_empty(), "presentation schema source is empty");

    let mut problems = Vec::new();
    for copy in [&live, &baseline] {
        let actual: BTreeSet<String> = walk_files(copy)
            .iter()
            .map(|path| rel(copy, path))
            .collect();
        for file in expected.symmetric_difference(&actual) {
            problems.push(format!("{}: file-set drift at {file}", copy.display()));
        }
        for file in expected.intersection(&actual) {
            let authored = std::fs::read(source.join(file)).expect("read authored schema");
            let deployed = std::fs::read(copy.join(file)).expect("read deployed schema");
            if authored != deployed {
                problems.push(format!("{}: byte drift at {file}", copy.display()));
            }
        }
    }

    for file in &expected {
        let bytes = std::fs::read(source.join(file)).expect("read presentation schema");
        let schema: serde_json::Value =
            serde_json::from_slice(&bytes).expect("presentation schema is valid JSON");
        assert_eq!(
            schema.get("$schema").and_then(serde_json::Value::as_str),
            Some("https://json-schema.org/draft/2020-12/schema"),
            "{file}: wrong or missing JSON Schema dialect"
        );
        assert!(
            schema
                .get("$id")
                .and_then(serde_json::Value::as_str)
                .is_some(),
            "{file}: public schema needs a stable $id"
        );
        assert_eq!(
            schema.get("additionalProperties"),
            Some(&serde_json::Value::Bool(false)),
            "{file}: public root contract must reject unknown fields"
        );
    }

    problems.sort();
    assert!(
        problems.is_empty(),
        "presentation schema source/live/baseline copies drifted:\n  {}",
        problems.join("\n  ")
    );
}

/// The portal's utility token layer must stay value-identical to the settled
/// present skins (ADR-0053 shared craft): portal `signal` mirrors the present
/// `instrument` skin and portal `folio` mirrors `ink`, in both modes, plus the
/// instrument/plex typeface stacks and the shared mono stack. Present's
/// `styles.css` is canonical; a divergence here is design-system drift.
#[test]
fn portal_utility_tokens_match_present_skins() {
    fn block_after(css: &str, marker: &str) -> std::collections::BTreeMap<String, String> {
        let start = css
            .find(marker)
            .unwrap_or_else(|| panic!("selector marker not found: {marker}"));
        let open = css[start..].find('{').expect("selector block opens") + start + 1;
        let close = css[open..].find('}').expect("selector block closes") + open;
        css[open..close]
            .lines()
            .filter_map(|line| {
                let line = line.split("/*").next().unwrap_or("").trim();
                let (name, value) = line.strip_prefix("--cf-")?.split_once(':')?;
                Some((
                    name.trim().to_string(),
                    value.trim().trim_end_matches(';').trim().to_lowercase(),
                ))
            })
            .collect()
    }
    const ROLES: [&str; 16] = [
        "canvas",
        "surface",
        "surface-raised",
        "surface-subtle",
        "text",
        "text-muted",
        "border",
        "border-strong",
        "accent",
        "accent-strong",
        "accent-soft",
        "focus",
        "positive",
        "warning",
        "danger",
        "diagram-line",
    ];
    let root = repo_root();
    let present = std::fs::read_to_string(root.join("crates/codeflow-present/web/src/styles.css"))
        .expect("present styles are readable");
    let portal = std::fs::read_to_string(
        root.join("assets/docs-portal/starter/src/styles/utility-tokens.css"),
    )
    .expect("portal utility tokens are readable");
    let pairs = [
        (
            "portal instrument light vs present instrument light",
            ":root {",
            "[data-cf-theme=\"instrument\"][data-cf-mode-resolved=\"light\"]",
        ),
        (
            "portal instrument dark vs present instrument dark",
            ":root[data-theme=\"dark\"] {",
            "[data-cf-theme=\"instrument\"][data-cf-mode-resolved=\"dark\"]",
        ),
        (
            "portal editorial light vs present editorial light",
            ":root[data-cfp-skin=\"editorial\"] {",
            "[data-cf-theme=\"editorial\"][data-cf-mode-resolved=\"light\"]",
        ),
        (
            "portal editorial dark vs present editorial dark",
            ":root[data-theme=\"dark\"][data-cfp-skin=\"editorial\"] {",
            "[data-cf-theme=\"editorial\"][data-cf-mode-resolved=\"dark\"]",
        ),
        (
            "portal ink light vs present ink light",
            ":root[data-cfp-skin=\"ink\"] {",
            "[data-cf-theme=\"ink\"][data-cf-mode-resolved=\"light\"]",
        ),
        (
            "portal ink dark vs present ink dark",
            ":root[data-theme=\"dark\"][data-cfp-skin=\"ink\"] {",
            "[data-cf-theme=\"ink\"][data-cf-mode-resolved=\"dark\"]",
        ),
    ];
    let mut drift = Vec::new();
    for (label, portal_marker, present_marker) in pairs {
        let ours = block_after(&portal, portal_marker);
        let theirs = block_after(&present, present_marker);
        for role in ROLES {
            match (ours.get(role), theirs.get(role)) {
                (Some(a), Some(b)) if a == b => {}
                (a, b) => drift.push(format!(
                    "{label}: --cf-{role}: portal {a:?} != present {b:?}"
                )),
            }
        }
    }

    let typefaces = [
        (
            "portal instrument sans vs present instrument typeface",
            ":root[data-cfp-typeface=\"instrument\"]",
            "[data-cf-typeface=\"instrument\"]",
            "font-sans",
        ),
        (
            "portal editorial sans vs present editorial typeface",
            ":root[data-cfp-typeface=\"editorial\"]",
            "[data-cf-typeface=\"editorial\"]",
            "font-sans",
        ),
        (
            "portal plex sans vs present plex typeface",
            ":root[data-cfp-typeface=\"plex\"]",
            "[data-cf-typeface=\"plex\"]",
            "font-sans",
        ),
    ];
    for (label, portal_marker, present_marker, role) in typefaces {
        let ours = block_after(&portal, portal_marker);
        let theirs = block_after(&present, present_marker);
        match (ours.get(role), theirs.get(role)) {
            (Some(a), Some(b)) if a == b => {}
            (a, b) => drift.push(format!(
                "{label}: --cf-{role}: portal {a:?} != present {b:?}"
            )),
        }
    }
    let portal_mono = block_after(&portal, ":root {");
    let present_mono = block_after(&present, ":root {");
    match (portal_mono.get("font-mono"), present_mono.get("font-mono")) {
        (Some(a), Some(b)) if a == b => {}
        (a, b) => drift.push(format!("shared mono stack: portal {a:?} != present {b:?}")),
    }

    assert!(
        drift.is_empty(),
        "portal utility tokens drifted from the canonical present skins \
         (crates/codeflow-present/web/src/styles.css):\n  {}",
        drift.join("\n  ")
    );
}
