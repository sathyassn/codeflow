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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use codeflow_core::scaffold::{DirSource, ManifestEntry, Ownership, ScaffoldManifest, Tier};
use pulldown_cmark::{Event, Parser, Tag};

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

#[derive(Debug, PartialEq, Eq)]
struct CatalogMetadata {
    name: String,
    description: String,
}

fn catalog_metadata(document: &str) -> Result<CatalogMetadata, String> {
    let mut lines = document.lines();
    if lines.next() != Some("---") {
        return Err("missing opening YAML frontmatter delimiter".to_string());
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err("missing closing YAML frontmatter delimiter".to_string());
    }
    let value: serde_yaml::Value =
        serde_yaml::from_str(&yaml).map_err(|error| format!("malformed YAML: {error}"))?;
    let mapping = value
        .as_mapping()
        .ok_or_else(|| "frontmatter must be a YAML mapping".to_string())?;
    let required = |field: &str| {
        mapping
            .get(serde_yaml::Value::String(field.to_string()))
            .and_then(serde_yaml::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| format!("missing or empty string `{field}`"))
    };
    Ok(CatalogMetadata {
        name: required("name")?,
        description: required("description")?,
    })
}

fn catalog_entry(dest: &str) -> Option<(&'static str, String)> {
    for prefix in [".claude/skills/", ".agents/skills/"] {
        if let Some(name) = dest
            .strip_prefix(prefix)
            .and_then(|path| path.strip_suffix("/SKILL.md"))
            .filter(|path| !path.contains('/'))
        {
            return Some(("skill", name.to_string()));
        }
    }
    dest.strip_prefix(".claude/agents/")
        .and_then(|path| path.strip_suffix(".md"))
        .filter(|path| !path.contains('/'))
        .map(|name| ("agent", name.to_string()))
}

fn relative_markdown_references(document: &str) -> Vec<String> {
    Parser::new(document)
        .filter_map(|event| match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                let target = dest_url.trim();
                if target.starts_with('#')
                    || target.starts_with('/')
                    || target.contains("://")
                    || target.starts_with("mailto:")
                {
                    return None;
                }
                let end = target.find(['#', '?']).unwrap_or(target.len());
                let path = &target[..end];
                (!path.is_empty()).then(|| path.replace('\\', "/"))
            }
            _ => None,
        })
        .collect()
}

fn resolve_installed_reference(parent: &Path, reference: &str) -> Option<String> {
    let parent = parent.to_string_lossy().replace('\\', "/");
    let reference = reference.replace('\\', "/");
    if reference.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = parent
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    for part in reference.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

#[test]
fn catalog_frontmatter_parser_supports_yaml_scalars_and_rejects_bad_metadata() {
    for (label, document, expected) in [
        (
            "plain",
            "---\nname: cf-plain\ndescription: Plain description\n---\n",
            ("cf-plain", "Plain description"),
        ),
        (
            "quoted",
            "---\nname: \"cf-quoted\"\ndescription: 'Quoted description'\n---\n",
            ("cf-quoted", "Quoted description"),
        ),
        (
            "literal block",
            "---\nname: cf-literal\ndescription: |\n  First line.\n  Second line.\n---\n",
            ("cf-literal", "First line.\nSecond line."),
        ),
        (
            "folded block",
            "---\nname: cf-folded\ndescription: >\n  First line.\n  Second line.\n---\n",
            ("cf-folded", "First line. Second line."),
        ),
    ] {
        let parsed = catalog_metadata(document).unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_eq!(parsed.name, expected.0, "{label}");
        assert_eq!(parsed.description, expected.1, "{label}");
    }

    for (label, document) in [
        ("no frontmatter", "# heading\n"),
        ("unclosed", "---\nname: cf-test\ndescription: test\n"),
        ("malformed", "---\nname: [\ndescription: test\n---\n"),
        ("missing name", "---\ndescription: test\n---\n"),
        (
            "empty description",
            "---\nname: cf-test\ndescription: ''\n---\n",
        ),
    ] {
        assert!(catalog_metadata(document).is_err(), "{label}");
    }
}

fn catalog_validation_problems(base: &Path, manifest: &ScaffoldManifest) -> (usize, Vec<String>) {
    let mut names: BTreeMap<(&str, String), String> = BTreeMap::new();
    let mut problems = Vec::new();
    let mut catalog_entries = 0usize;

    for entry in &manifest.entries {
        let Some((kind, expected_name)) = catalog_entry(&entry.dest) else {
            continue;
        };
        catalog_entries += 1;
        let source_path = base.join(&entry.src);
        let document = match std::fs::read_to_string(&source_path) {
            Ok(document) => document,
            Err(error) => {
                problems.push(format!(
                    "{}: cannot read catalog source: {error}",
                    entry.src
                ));
                continue;
            }
        };
        let metadata = match catalog_metadata(&document) {
            Ok(metadata) => metadata,
            Err(error) => {
                problems.push(format!("{}: {error}", entry.src));
                continue;
            }
        };
        if metadata.name != expected_name {
            problems.push(format!(
                "{}: metadata name {:?} does not match installed {kind} name {expected_name:?}",
                entry.src, metadata.name
            ));
        }
        let key = (kind, metadata.name.clone());
        if let Some(previous) = names.insert(key.clone(), entry.src.clone()) {
            if previous != entry.src {
                problems.push(format!(
                    "duplicate {} catalog name {:?}: {previous} and {}",
                    key.0, key.1, entry.src
                ));
            }
        }

        for reference in relative_markdown_references(&document) {
            let Some(dest_parent) = Path::new(&entry.dest).parent() else {
                problems.push(format!("{}: catalog destination has no parent", entry.dest));
                continue;
            };
            let Some(referenced_dest) = resolve_installed_reference(dest_parent, &reference) else {
                problems.push(format!(
                    "{}: linked reference {reference:?} escapes the installed repository root",
                    entry.src
                ));
                continue;
            };
            let shipped = manifest.entries.iter().find(|candidate| {
                candidate.dest == referenced_dest
                    && entry
                        .tiers
                        .iter()
                        .all(|tier| candidate.tiers.contains(tier))
            });
            let Some(shipped) = shipped else {
                problems.push(format!(
                    "{}: linked reference {reference:?} is not shipped beside {} for every tier",
                    entry.src, entry.dest
                ));
                continue;
            };
            if !base.join(&shipped.src).is_file() {
                problems.push(format!(
                    "{}: linked destination {referenced_dest} maps to missing source {}",
                    entry.src, shipped.src
                ));
            }
        }
    }
    problems.sort();
    problems.dedup();
    (catalog_entries, problems)
}

fn catalog_manifest_entry(src: &str, dest: &str, tiers: Vec<Tier>) -> ManifestEntry {
    ManifestEntry {
        src: src.to_string(),
        dest: dest.to_string(),
        ownership: Ownership::Managed,
        tiers,
        exec: false,
        template: false,
        preset: None,
        region: None,
    }
}

fn assert_catalog_problem(problems: &[String], expected: &str) {
    assert!(
        problems.iter().any(|problem| problem.contains(expected)),
        "missing {expected:?} in {problems:?}"
    );
}

#[test]
fn shipped_catalog_metadata_and_linked_references_are_valid_and_manifested() {
    let root = repo_root();
    let assets_dir = root.join("assets");
    let base = assets_dir.join("base");
    let manifest =
        ScaffoldManifest::load(&DirSource::new(&assets_dir)).expect("shipped manifest loads");
    let (catalog_entries, problems) = catalog_validation_problems(&base, &manifest);
    assert!(
        catalog_entries > 0,
        "expected shipped skill and agent catalogs"
    );
    assert!(
        problems.is_empty(),
        "shipped catalog metadata/reference errors:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn catalog_validation_reports_mutated_metadata_references_and_tier_coverage() {
    assert_eq!(
        Some(".claude/skills/cf-two/SKILL.md".to_string()),
        resolve_installed_reference(Path::new(".claude/skills/cf-one"), r"..\cf-two\SKILL.md")
    );
    assert_eq!(
        None,
        resolve_installed_reference(Path::new(".claude/skills/cf-one"), "../../../../outside.md")
    );

    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    let one = base.join("agents/skills/cf-one/SKILL.md");
    let two = base.join("agents/skills/cf-two/SKILL.md");
    std::fs::create_dir_all(one.parent().unwrap()).unwrap();
    std::fs::create_dir_all(two.parent().unwrap()).unwrap();
    let one_document = "---\nname: cf-one\ndescription: First skill\n---\n\
Read [the sibling skill](../cf-two/SKILL.md#usage).\n\
`[example only](missing-example.md)`\n";
    let two_document = "---\nname: cf-two\ndescription: Second skill\n---\n";
    std::fs::write(&one, one_document).unwrap();
    std::fs::write(&two, two_document).unwrap();
    let manifest = ScaffoldManifest {
        schema_version: 1,
        entries: vec![
            catalog_manifest_entry(
                "agents/skills/cf-one/SKILL.md",
                ".claude/skills/cf-one/SKILL.md",
                vec![Tier::Standard, Tier::Full],
            ),
            catalog_manifest_entry(
                "agents/skills/cf-two/SKILL.md",
                ".claude/skills/cf-two/SKILL.md",
                vec![Tier::Standard, Tier::Full],
            ),
        ],
    };
    assert_eq!(
        (2, Vec::new()),
        catalog_validation_problems(base, &manifest)
    );

    std::fs::write(
        &one,
        one_document.replace("name: cf-one", "name: wrong-name"),
    )
    .unwrap();
    let (_, problems) = catalog_validation_problems(base, &manifest);
    assert_catalog_problem(&problems, "does not match");

    std::fs::write(
        &one,
        one_document.replace("../cf-two/SKILL.md#usage", "missing.md"),
    )
    .unwrap();
    let (_, problems) = catalog_validation_problems(base, &manifest);
    assert_catalog_problem(&problems, "is not shipped");

    std::fs::write(&one, one_document).unwrap();
    let mut missing_tier = manifest.clone();
    missing_tier.entries[1].tiers = vec![Tier::Standard];
    let (_, problems) = catalog_validation_problems(base, &missing_tier);
    assert_catalog_problem(&problems, "for every tier");

    let duplicate_source = base.join("other/cf-two/SKILL.md");
    std::fs::create_dir_all(duplicate_source.parent().unwrap()).unwrap();
    std::fs::write(&duplicate_source, two_document).unwrap();
    let mut duplicate = manifest.clone();
    duplicate.entries.push(catalog_manifest_entry(
        "other/cf-two/SKILL.md",
        ".agents/skills/cf-two/SKILL.md",
        vec![Tier::Standard, Tier::Full],
    ));
    let (_, problems) = catalog_validation_problems(base, &duplicate);
    assert_catalog_problem(&problems, "duplicate skill catalog name");

    std::fs::write(
        &one,
        one_document.replace("../cf-two/SKILL.md#usage", "../../../../outside.md"),
    )
    .unwrap();
    let (_, problems) = catalog_validation_problems(base, &manifest);
    assert_catalog_problem(&problems, "escapes the installed repository root");
}

#[test]
fn every_manifest_catalog_artifact_matches_live_and_baseline_copies() {
    let root = repo_root();
    let assets_dir = root.join("assets");
    let base = assets_dir.join("base");
    let manifest =
        ScaffoldManifest::load(&DirSource::new(&assets_dir)).expect("shipped manifest loads");
    let mut compared = 0usize;
    let mut problems = Vec::new();
    for entry in &manifest.entries {
        if !entry.dest.starts_with(".claude/skills/")
            && !entry.dest.starts_with(".agents/skills/")
            && !entry.dest.starts_with(".claude/agents/")
        {
            continue;
        }
        compared += 1;
        let source = base.join(&entry.src);
        let expected = match std::fs::read(&source) {
            Ok(bytes) => bytes,
            Err(error) => {
                problems.push(format!("{}: cannot read source: {error}", entry.src));
                continue;
            }
        };
        for copy in [
            root.join(&entry.dest),
            root.join(".codeflow/.baseline").join(&entry.dest),
        ] {
            match std::fs::read(&copy) {
                Ok(actual) if actual == expected => {}
                Ok(_) => problems.push(format!(
                    "{} -> {}: byte drift",
                    entry.src,
                    rel(&root, &copy)
                )),
                Err(error) => problems.push(format!(
                    "{} -> {}: cannot read copy: {error}",
                    entry.src,
                    rel(&root, &copy)
                )),
            }
        }
    }
    assert!(compared > 0, "expected manifest catalog artifacts");
    problems.sort();
    assert!(
        problems.is_empty(),
        "manifest-derived catalog source/live/baseline drift:\n  {}",
        problems.join("\n  ")
    );
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

/// The ADR-0018 instruction-only clause (TSK-041 defect 8). ADR-0018 is an
/// accepted, append-only record ("never edited afterwards except to set
/// `superseded_by`"), so the clarifying clause cannot land there — its one
/// home is the AGENTS contract, which `codeflow update` regenerates.
const INSTRUCTION_ONLY_CLAUSE: &str =
    "that prohibition is instruction-only, and CodeFlow cannot technically prevent it";

/// Authored contract templates that could plausibly host the clause. Exactly
/// one of them may.
const CONTRACT_TEMPLATES: [&str; 4] = [
    "assets/base/AGENTS.md.tmpl",
    "assets/base/AGENTS.minimal.md.tmpl",
    "assets/base/CLAUDE.md.tmpl",
    "assets/base/CLAUDE.minimal.md.tmpl",
];

/// Normalize wrapped prose so a clause that spans a line break still matches.
fn unwrapped(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// DEFECT 8 (positive): the clause has exactly one authored home, and the
/// managed copies (`AGENTS.md`, the managed baseline) carry it because the
/// managed region is regenerated from that one template.
#[test]
fn instruction_only_clause_has_exactly_one_home() {
    let root = repo_root();
    let clause = unwrapped(INSTRUCTION_ONLY_CLAUSE);

    let homes: Vec<&str> = CONTRACT_TEMPLATES
        .into_iter()
        .filter(|template| {
            let text = std::fs::read_to_string(root.join(template))
                .unwrap_or_else(|error| panic!("read {template}: {error}"));
            unwrapped(&text).contains(&clause)
        })
        .collect();
    assert_eq!(
        homes,
        vec!["assets/base/AGENTS.md.tmpl"],
        "the ADR-0018 instruction-only clause must be stated once, in the \
         AGENTS template — found in: {homes:?}"
    );

    for mirror in ["AGENTS.md", ".codeflow/.baseline/AGENTS.md"] {
        let text = std::fs::read_to_string(root.join(mirror))
            .unwrap_or_else(|error| panic!("read {mirror}: {error}"));
        assert!(
            unwrapped(&text).contains(&clause),
            "{mirror}: managed region is out of step with the AGENTS template"
        );
    }
}

/// DEFECT 8 (negative): ADR-0018 is untouched. The append-only banner stands,
/// and neither the clause nor an appended clarifying note was written into it.
#[test]
fn adr_0018_is_not_amended_to_carry_the_clause() {
    let path =
        repo_root().join("docs/decisions/ADR-0018-interactive-only-cross-model-transport.md");
    let text = std::fs::read_to_string(&path).expect("ADR-0018 is readable");
    let flat = unwrapped(&text);

    assert!(
        flat.contains("ADRs are append-only"),
        "ADR-0018 lost its append-only banner"
    );
    assert!(
        !flat.contains(&unwrapped(INSTRUCTION_ONLY_CLAUSE)),
        "the clause was written into append-only ADR-0018; its home is the \
         AGENTS contract"
    );
    for appended in ["Clarifying note", "clarifying note", "## Note"] {
        assert!(
            !text.contains(appended),
            "ADR-0018 gained an appended note ({appended}); the record is \
             append-only and superseded, never amended"
        );
    }
}

/// The four scaffolded CI templates that put the `codeflow` binary on PATH.
/// `cargo-dist` installs with `install-path = "CARGO_HOME"`
/// (`dist-workspace.toml`), so a template that hardcodes `$HOME/.cargo/bin`
/// silently misses the binary whenever `CARGO_HOME` points elsewhere — the
/// gate then fails red for the wrong reason, or (worse) a later relaxation
/// makes a missing binary look like a pass.
const CI_PERIMETER_TEMPLATES: [&str; 4] = [
    "assets/base/ci/codeflow-ci.yml",
    "assets/base/ci/.gitlab-ci.yml",
    "assets/base/ci/bitbucket-pipelines.yml",
    "assets/base/ci/ci-generic.sh",
];

/// The one expanded form every CI template must use to reach cargo's bin dir.
const CARGO_BIN_EXPANDED: &str = "${CARGO_HOME:-$HOME/.cargo}/bin";

/// Every reference to a cargo bin directory in `text` goes through
/// [`CARGO_BIN_EXPANDED`]: no bare `$HOME/.cargo/bin`, and no stale
/// `.codeflow/bin` (no shipped installer writes there).
fn cargo_bin_is_resolved_through_cargo_home(text: &str) -> bool {
    let without_expanded = text.replace(CARGO_BIN_EXPANDED, "");
    !without_expanded.contains("$HOME/.cargo/bin")
        && !without_expanded.contains(".cargo/bin")
        && !text.contains(".codeflow/bin")
}

/// CI-PERIMETER CANARY (TSK-041 defect 1): the scaffolded CI templates must
/// resolve cargo binaries through `CARGO_HOME`, because that is where the
/// shipped installer actually puts them.
#[test]
fn ci_templates_resolve_cargo_bin_through_cargo_home() {
    let root = repo_root();
    for template in CI_PERIMETER_TEMPLATES {
        let text = std::fs::read_to_string(root.join(template))
            .unwrap_or_else(|error| panic!("read {template}: {error}"));
        assert!(
            text.contains(CARGO_BIN_EXPANDED),
            "{template}: must put {CARGO_BIN_EXPANDED} on PATH — cargo-dist \
             installs with install-path = \"CARGO_HOME\""
        );
        assert!(
            cargo_bin_is_resolved_through_cargo_home(&text),
            "{template}: a cargo bin path bypasses CARGO_HOME — use \
             {CARGO_BIN_EXPANDED} everywhere, including the install examples"
        );
    }

    // NEGATIVE: the canary is not vacuous — it rejects the bare forms it
    // exists to catch, in both the active and the commented-example position.
    for bad in [
        "export PATH=\"$HOME/.cargo/bin:$PATH\"",
        "echo \"$HOME/.cargo/bin\" >> \"$GITHUB_PATH\"",
        "#   export PATH=\"$HOME/.codeflow/bin:$PATH\"",
    ] {
        assert!(
            !cargo_bin_is_resolved_through_cargo_home(bad),
            "canary would not catch the bare form: {bad}"
        );
    }
    assert!(
        cargo_bin_is_resolved_through_cargo_home(&format!(
            "export PATH=\"{CARGO_BIN_EXPANDED}:$PATH\""
        )),
        "canary must accept the expanded form"
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
fn starter_portal_config_points_at_decisions_instead_of_publishing_them() {
    // ADR-0064: the guide has no per-record pages. The starter default keeps
    // the records switch off and names the decisions folder as a pointer, so
    // a freshly scaffolded portal never renders an ADR as a page.
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("assets/docs-portal/starter/portal.config.json"))
            .expect("starter portal config is readable"),
    )
    .expect("starter portal config is valid JSON");
    assert_eq!(config["records"]["enabled"], serde_json::Value::Bool(false));
    assert_eq!(config["records"]["layer"], "system");
    let folders: Vec<&str> = config["records"]["pointers"]
        .as_array()
        .expect("records pointers is an array")
        .iter()
        .map(|pointer| {
            pointer["folder"]
                .as_str()
                .expect("pointer folder is a string")
        })
        .collect();
    assert_eq!(folders, ["docs/decisions"]);
    for layer in config["layers"].as_array().expect("layers is an array") {
        for key in ["paths", "prefixes"] {
            for entry in layer[key].as_array().into_iter().flatten() {
                let entry = entry.as_str().expect("layer entry is a string");
                assert!(
                    !entry.starts_with("docs/decisions"),
                    "starter layer {} publishes the decisions folder through {entry}",
                    layer["id"]
                );
            }
        }
    }
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

/// The figure grammar (ADR-0068) is one doctrine in two skills: the copies are
/// byte-identical, both skills and the shared doctrine point at it, and it
/// keeps the nine families in one table, the twelve rules, the altitude
/// contract, the evidence board with its baselines as controls, and the named
/// anti-patterns. A smaller file that drops a family or a rule must fail here.
#[test]
fn figure_grammar_is_shared_and_keeps_its_families_rules_and_evidence() {
    let root = repo_root();
    let skills = root.join("assets/base/agents/skills");
    let normalized = |value: &str| value.split_whitespace().collect::<Vec<_>>().join(" ");
    let present = std::fs::read(skills.join("cf-present/resources/figure-grammar.md"))
        .expect("present figure grammar is readable");
    let portal = std::fs::read(skills.join("cf-docs-portal/resources/figure-grammar.md"))
        .expect("portal figure grammar is readable");
    assert_eq!(
        present, portal,
        "figure-grammar.md drifted between cf-present and cf-docs-portal"
    );
    let grammar = normalized(&String::from_utf8(present).expect("figure grammar is UTF-8"));

    let families_header =
        "| Family | Relationship it encodes | Geometry | Non-colour channels that carry state | Reader question it answers |";
    assert!(
        grammar.contains(families_header),
        "figure grammar lost the nine-families table header"
    );
    for (index, family) in [
        "flow",
        "structure",
        "layering",
        "sequence",
        "state",
        "coverage",
        "extent",
        "derivation",
        "graph",
    ]
    .iter()
    .enumerate()
    {
        assert!(
            grammar.contains(&format!("| {family} |")),
            "figure grammar lost the {family} family row"
        );
        assert!(
            grammar.contains(&format!("### 8.{} {family}", index + 1)),
            "figure grammar lost the {family} specimen heading"
        );
        assert!(
            grammar.contains(&format!("data-cf-figure=\"{family}\"")),
            "figure grammar lost the {family} specimen figure"
        );
    }
    for rule in 1..=12 {
        assert!(
            grammar.contains(&format!("| {rule} | ")),
            "figure grammar lost rule {rule}"
        );
    }
    for (duty, needle) in [
        ("masked-title test", "Masked-title test: with kicker, title, caption and legend masked"),
        ("inner mark floor", "Every drawn state mark 9 px or larger on its information-bearing dimension"),
        ("two-channel measurement", "Every pair of drawn states differs on at least two of"),
        ("overprint rule", "at a depth of 1 px or more fails; text keeps 8 px clear"),
        ("narrow variant rule", "Under a 646 px container the narrow composition shows"),
        ("fidelity rule", "the verifier re-derives the value and compares it exactly"),
        ("altitude contract", "| Altitude | Reader question | Families that answer it | Prose role |"),
        ("concept ownership", "Concept owns what the subject is, who it is for and what it is not"),
        ("how-to rule", "| How-to section | what do I do, in what order, and what tells me it worked | sequence, state or extent |"),
        ("prose rule", "no em or en dash"),
        ("evidence board", "`docs/verification/tsk-014-w5/` is the evidence board"),
        ("negative controls", "are the negative controls a figure must beat"),
        ("confusable pair", "| Confusable pair |"),
        ("inner mark anti-pattern", "| Inner mark floor miss |"),
        ("overprint anti-pattern", "| Overprint |"),
        ("elongation anti-pattern", "| Elongation by reflow |"),
        ("declaration schema", "ADR-0068 records the schema"),
        ("chat rule", "In chat the family choice is the same; the medium changes the marks"),
    ] {
        assert!(
            grammar.contains(&normalized(needle)),
            "figure grammar lost duty: {duty}"
        );
    }
    // A literal colour is `#` followed by exactly three or six hex digits and
    // then a non-alphanumeric boundary; anchors such as `SKILL.md#4` and ids
    // such as `#fg-cov-h` are not colours.
    let bytes = grammar.as_bytes();
    let literal_colour = bytes.iter().enumerate().any(|(index, byte)| {
        if *byte != b'#' {
            return false;
        }
        let run = bytes[index + 1..]
            .iter()
            .take_while(|b| b.is_ascii_hexdigit())
            .count();
        let boundary = bytes
            .get(index + 1 + run)
            .is_none_or(|b| !b.is_ascii_alphanumeric());
        (run == 3 || run == 6) && boundary
    });
    assert!(
        !literal_colour && !grammar.contains("rgb("),
        "figure grammar specimens must draw with --cf-fig-* tokens only, never a literal colour"
    );
}

/// Both presentation skills, their shared doctrine and their profile references
/// point at the figure grammar and at the evidence board with its baselines,
/// never at a bare design-exploration board.
#[test]
fn presentation_skills_point_at_the_figure_grammar_and_evidence_board() {
    let skills = repo_root().join("assets/base/agents/skills");
    for skill in ["cf-present", "cf-docs-portal"] {
        let skill_text = std::fs::read_to_string(skills.join(skill).join("SKILL.md"))
            .expect("skill is readable");
        assert!(
            skill_text.contains("resources/figure-grammar.md"),
            "{skill}/SKILL.md must name the figure grammar in its load order"
        );
        assert!(
            skill_text.contains("docs/verification/tsk-014-w5/"),
            "{skill}/SKILL.md must point at the evidence board, not a bare design-exploration board"
        );
        let doctrine = std::fs::read_to_string(
            skills
                .join(skill)
                .join("resources/utility-presentation-system.md"),
        )
        .expect("shared doctrine is readable");
        assert!(
            doctrine.contains("`figure-grammar.md`")
                && doctrine.contains("### Figure grammar")
                && !doctrine.contains("### Stage and diagram grammar"),
            "{skill} shared doctrine must point at the figure grammar as the default form"
        );
        assert!(
            doctrine.contains("docs/verification/tsk-014-w5/"),
            "{skill} shared doctrine must name the evidence board"
        );
    }
    for reference in [
        "cf-present/references/visual-craft.md",
        "cf-present/references/document-authoring.md",
        "cf-present/resources/how-presentation-works.md",
        "cf-docs-portal/references/visual-craft.md",
    ] {
        let text = std::fs::read_to_string(skills.join(reference)).expect("reference is readable");
        assert!(
            !text.contains("design-exploration board")
                && text.contains("docs/verification/tsk-014-w5/"),
            "{reference} must resolve the board reference to docs/verification/tsk-014-w5/"
        );
    }
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
/// `styles.css` is canonical; a divergence here is design-system drift. The
/// starter mirror read here must also equal the live `docs-portal` copy, so a
/// retune cannot land in one of the two portal files alone.
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
    let live_portal =
        std::fs::read_to_string(root.join("docs-portal/src/styles/utility-tokens.css"))
            .expect("live portal utility tokens are readable");
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

    // The settled TSK-045 token table. Equality between the sheets is not
    // enough on its own: a coordinated retune of all three would still pass it,
    // so every role of every skin and mode pair is pinned to its settled value
    // in each of the three sheets.
    let settled_roles: [&str; 14] = [
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
        "positive",
        "warning",
        "danger",
    ];
    let settled_table: [[&str; 14]; 6] = [
        [
            "#f2f3f4", "#ffffff", "#f7f8f9", "#e8eaec", "#15181b", "#4d555d", "#d5d9dd", "#6d767f",
            "#1f6fb2", "#185c95", "#e8f1f9", "#2e7d57", "#9a5f0f", "#9b1c1c",
        ],
        [
            "#0f1113", "#161a1e", "#1d2227", "#252b31", "#e8ebee", "#a8b0b8", "#2e353c", "#7a838c",
            "#6aaee8", "#8fc2f0", "#15283a", "#4fb183", "#d99a45", "#ff7b6e",
        ],
        [
            "#eef2f6", "#fafcfe", "#f2f5f9", "#e2e8ef", "#171c22", "#4a5563", "#cfd8e2", "#6b7a8c",
            "#2f5f8a", "#244a6d", "#e3edf6", "#246b4a", "#9a6b1a", "#a33a32",
        ],
        [
            "#0f141a", "#161c24", "#1d252f", "#26303b", "#e6ecf2", "#a4b0bd", "#2d3846", "#78889a",
            "#86b4d6", "#a5c8e4", "#182a3b", "#4fb183", "#d9a85a", "#ff8a80",
        ],
        [
            "#f6f3ee", "#fcfaf6", "#f4f0e9", "#eae4da", "#2a2116", "#5b5348", "#dcd4c8", "#857a6c",
            "#8a5636", "#6d4128", "#f3ece3", "#3d6b3a", "#8f5a12", "#9b1c1c",
        ],
        [
            "#1c1510", "#261d16", "#30261e", "#3b2f25", "#f3eadc", "#c6b6a3", "#45372b", "#927e69",
            "#dba672", "#e6bd8c", "#3e2c1c", "#7cbc74", "#d9a85a", "#ff8a80",
        ],
    ];
    for index in 0..pairs.len() {
        let (label, portal_marker, present_marker) = pairs[index];
        for (sheet, css, marker) in [
            ("starter portal", &portal, portal_marker),
            ("live portal", &live_portal, portal_marker),
            ("present", &present, present_marker),
        ] {
            let block = block_after(css, marker);
            for (role, settled) in settled_roles.iter().zip(settled_table[index].iter()) {
                match block.get(*role) {
                    Some(actual) if actual == settled => {}
                    actual => drift.push(format!(
                        "{label}: {sheet} --cf-{role}: {actual:?} != settled {settled:?}"
                    )),
                }
            }
        }
    }
    // Present's bare :root default carries the instrument light skin, so a
    // present document that has not yet had its theme attribute written paints
    // the settled instrument values rather than a stale earlier palette.
    let present_default = block_after(&present, ":root {");
    for (role, settled) in settled_roles.iter().zip(settled_table[0].iter()) {
        match present_default.get(*role) {
            Some(actual) if actual == settled => {}
            actual => drift.push(format!(
                "present :root default: --cf-{role}: {actual:?} != settled {settled:?}"
            )),
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
    // The starter mirror this test reads and the live portal copy must be the
    // same file: a retune applied to one and not the other would otherwise pass
    // every check above while the published portal still shipped the old skin.
    if portal != live_portal {
        for (label, portal_marker, _present_marker) in pairs {
            let starter_block = block_after(&portal, portal_marker);
            let live_block = block_after(&live_portal, portal_marker);
            for role in ROLES {
                match (starter_block.get(role), live_block.get(role)) {
                    (Some(a), Some(b)) if a == b => {}
                    (a, b) => drift.push(format!(
                        "{label}: --cf-{role}: starter {a:?} != live portal {b:?}"
                    )),
                }
            }
        }
        drift.push(
            "assets/docs-portal/starter/src/styles/utility-tokens.css and \
             docs-portal/src/styles/utility-tokens.css are not byte-identical"
                .to_string(),
        );
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
