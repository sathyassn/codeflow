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

/// The repository's own installed record must stay loadable and honest: a
/// merge that leaves conflict markers, or a stale hash for a changed managed
/// source, breaks `codeflow update` here without failing any shipped-asset
/// check. Managed hashes are the pristine shipped copy, so each must equal
/// both its source asset and its baseline copy.
#[test]
fn installed_manifest_loads_and_matches_managed_sources_and_baselines() {
    use codeflow_core::scaffold::sha256_hex;
    use codeflow_core::scaffold::state::InstalledManifest;

    let root = repo_root();
    assert!(
        InstalledManifest::path(&root).is_file(),
        "the repository keeps an installed manifest"
    );
    let installed = InstalledManifest::load_or_default(&root, "unused")
        .expect("installed .codeflow/manifest.json parses");
    let base = root.join("assets/base");
    // A templated source renders with project values, so only its baseline
    // (the rendered pristine copy) can match the recorded hash.
    let shipped = ScaffoldManifest::load(&DirSource::new(root.join("assets")))
        .expect("shipped manifest loads");
    let templated: std::collections::BTreeSet<&str> = shipped
        .entries
        .iter()
        .filter(|entry| entry.template)
        .map(|entry| entry.src.as_str())
        .collect();
    let mut compared = 0usize;
    let mut problems = Vec::new();
    for (dest, file) in &installed.files {
        if file.ownership != Ownership::Managed {
            continue;
        }
        compared += 1;
        let baseline = root.join(".codeflow/.baseline").join(dest);
        let copies = if templated.contains(file.src.as_str()) {
            vec![baseline]
        } else {
            vec![base.join(&file.src), baseline]
        };
        for copy in copies {
            match std::fs::read(&copy) {
                Ok(bytes) if sha256_hex(&bytes) == file.sha256 => {}
                Ok(_) => problems.push(format!("{dest}: {} differs", rel(&root, &copy))),
                Err(error) => problems.push(format!("{dest}: {}: {error}", rel(&root, &copy))),
            }
        }
    }
    assert!(
        compared > 0,
        "expected managed entries in the installed manifest"
    );
    problems.sort();
    assert!(
        problems.is_empty(),
        "installed manifest hashes drifted from source or baseline:\n  {}",
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
/// `superseded_by`"), so the clarifying clause cannot land there. Its one
/// home is the git rules reference, which `codeflow update` installs.
const INSTRUCTION_ONLY_CLAUSE: &str =
    "that prohibition is instruction-only, and CodeFlow cannot technically prevent it";

/// Authored instruction files that could plausibly host the clause. Exactly
/// one, the git rules reference the kernel points at, carries it; the
/// installed copy and its baseline are rendered from that one source.
const CONTRACT_TEMPLATES: [&str; 5] = [
    "assets/base/AGENTS.md.tmpl",
    "assets/base/AGENTS.minimal.md.tmpl",
    "assets/base/CLAUDE.md.tmpl",
    "assets/base/CLAUDE.minimal.md.tmpl",
    "assets/base/rules/git-rules.md",
];

fn unwrapped(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

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
        vec!["assets/base/rules/git-rules.md"],
        "the ADR-0018 instruction-only clause must be stated once, in the \
         git rules reference; found in: {homes:?}"
    );

    for mirror in [
        ".codeflow/rules/git-rules.md",
        ".codeflow/.baseline/.codeflow/rules/git-rules.md",
    ] {
        let text = std::fs::read_to_string(root.join(mirror))
            .unwrap_or_else(|error| panic!("read {mirror}: {error}"));
        assert!(
            unwrapped(&text).contains(&clause),
            "{mirror}: installed copy is out of step with the git rules reference"
        );
    }
}

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
         git rules reference"
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
const CI_PERIMETER_TEMPLATES: [&str; 4] = [
    "assets/base/ci/codeflow-ci.yml",
    "assets/base/ci/.gitlab-ci.yml",
    "assets/base/ci/bitbucket-pipelines.yml",
    "assets/base/ci/ci-generic.sh",
];

/// CI-PERIMETER CANARY (TSK-041 defect 1, as the 3.0.0 pinned install
/// changed it): the templates install the pinned, checksum-verified release
/// binary themselves, so none reaches a cargo bin directory, where a
/// hardcoded `$HOME/.cargo/bin` once missed the binary whenever `CARGO_HOME`
/// pointed elsewhere.
#[test]
fn ci_templates_never_resolve_codeflow_through_a_cargo_bin() {
    let root = repo_root();
    for template in CI_PERIMETER_TEMPLATES {
        let text = std::fs::read_to_string(root.join(template))
            .unwrap_or_else(|error| panic!("read {template}: {error}"));
        assert!(
            !text.contains(".cargo/bin"),
            "{template}: reaches a cargo bin directory; the pinned install \
             puts the verified binary on PATH itself"
        );
        assert!(
            text.contains("sha256.sum"),
            "{template}: installs codeflow without verifying sha256.sum"
        );
    }
}

/// Codex EPC-017 review, finding 5: the PR-body check (structure and the
/// ADR-0067 characters) must rerun when the body is edited after opening,
/// and the body keeps reaching `codeflow ci` through `env:`, never inline.
#[test]
fn shipped_ci_rechecks_an_edited_pr_body_through_env() {
    // The PR-body check runs in the enforcing workflow on
    // `pull_request_target` (TSK-107, SPC-013 R-113).
    let workflow = std::fs::read_to_string(repo_root().join("assets/base/ci/codeflow-policy.yml"))
        .expect("shipped policy workflow is readable");
    assert!(
        workflow.contains(
            "  pull_request_target:\n    types: [opened, synchronize, reopened, edited]\n"
        ),
        "the enforcing workflow must subscribe pull_request_target to edited"
    );
    assert!(workflow.contains("CODEFLOW_PR_BODY: ${{ github.event.pull_request.body }}"));
    let body_uses = workflow.matches("github.event.pull_request.body").count();
    assert_eq!(body_uses, 1, "the PR body is read only through env");
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
const UNSHIPPED_FILES: [&str; 6] = [
    "scaffold-manifest.toml",
    // The rule-map kernel (TSK-127) renders the root AGENTS.md and CLAUDE.md
    // templates; the binary embeds it, and only the renders ship per file.
    "rule-map.toml",
    "ci/README.md",
    "ci/.gitlab-ci.yml",
    "ci/bitbucket-pipelines.yml",
    "ci/ci-generic.sh",
];

/// Files an earlier version shipped and this one retires (TSK-184). They
/// are removed from the asset tree and the manifest, nothing more: on an
/// adopter, `codeflow update` reconciles each as an orphan, removing an
/// unmodified copy with its baseline and record, keeping a modified copy
/// unmanaged, and never re-adding it. Each stays out of both places.
/// The present review example stays: the visual guide line (EPC-016) made
/// it the figure-block example that document-authoring and the present tests
/// read, so the release line keeps it.
const RETIRED_FILES: [&str; 7] = [
    "agents/skills/cf-model-orchestrator/references/other-hosts.md",
    "agents/skills/cf-model-orchestrator/resources/routing/assignment.md",
    "agents/skills/cf-model-orchestrator/resources/routing/effort.md",
    "agents/skills/cf-model-orchestrator/resources/routing/hosts.md",
    "agents/skills/cf-model-orchestrator/resources/routing/review.md",
    "agents/skills/cf-model-orchestrator/resources/routing/roles.md",
    // Its registry check is a step of ci/codeflow-policy.yml now.
    "ci/codeflow-registry.yml",
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

    // A retired file is authored nowhere and shipped nowhere, so an update
    // prunes an adopter's copy as an orphan and never writes it again.
    for name in RETIRED_FILES {
        assert!(
            !base.join(name).exists(),
            "retired file {name} is back under assets/base"
        );
        assert!(
            !srcs.contains(name),
            "retired file {name} is a manifest src again"
        );
    }

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
        browser.contains(
            "serveBuiltSite({ directory: path.join(root, \"dist\"), base: config.base })"
        ) && browser.contains("env: hardenedChildEnvironment(),"),
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
/// while consumers receive stale runtime or tests. The check runs here and not
/// in the starter's own tests, because a consumer has no second copy to
/// compare against.
#[test]
fn portal_dogfood_runtime_matches_the_shipped_starter() {
    let root = repo_root();
    let starter_files = git_inventory(&root, "assets/docs-portal/starter");
    // The figure declarations the portal configuration binds are the
    // project's own content, like the configuration itself; the starter
    // ships none.
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("docs-portal/portal.config.json"))
            .expect("the dogfood portal config is readable"),
    )
    .expect("the dogfood portal config is valid JSON");
    let declarations: BTreeSet<String> = config["figures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|binding| binding["declaration"].as_str())
        .filter_map(|path| path.strip_prefix("docs-portal/"))
        .map(str::to_owned)
        .collect();
    let dogfood_files: Vec<String> = git_inventory(&root, "docs-portal")
        .into_iter()
        .filter(|relative| !declarations.contains(relative))
        .collect();
    assert!(!starter_files.is_empty(), "the starter lists no files");
    assert!(!dogfood_files.is_empty(), "docs-portal lists no files");
    let problems = portal_mirror_drift(
        &root.join("assets/docs-portal/starter"),
        &starter_files,
        &root.join("docs-portal"),
        &dogfood_files,
    );
    assert!(
        problems.is_empty(),
        "docs-portal diverged from the shipped reusable starter:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn portal_mirror_drift_names_each_perturbed_copy() {
    let root = tempfile::tempdir().expect("temporary directory");
    let repo = root.path();
    // No template, so the fixture never copies sample hooks.
    let init = std::process::Command::new("git")
        .args(["init", "-q", "--template="])
        .arg(repo)
        .output()
        .expect("git is available for the parity fixture");
    assert!(
        init.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    let starter = repo.join("starter");
    let dogfood = repo.join("dogfood");
    for side in [&starter, &dogfood] {
        std::fs::create_dir_all(side.join("scripts")).expect("create scripts");
        std::fs::write(side.join(".gitignore"), "node_modules/\ndist/\n").expect("write");
        std::fs::write(side.join("scripts/gate.mjs"), "export const gate = 1;\n").expect("write");
        std::fs::write(side.join("portal.config.json"), "{}\n").expect("write");
    }
    std::fs::write(dogfood.join("portal.config.json"), "{\"title\":\"own\"}\n").expect("write");
    // Ignored dependency and build output on the starter side only.
    std::fs::create_dir_all(starter.join("node_modules/pkg")).expect("create node_modules");
    std::fs::write(starter.join("node_modules/pkg/index.js"), "1\n").expect("write");
    std::fs::create_dir_all(starter.join("dist")).expect("create dist");
    std::fs::write(starter.join("dist/index.html"), "<html></html>\n").expect("write");
    let drift = || {
        portal_mirror_drift(
            &starter,
            &git_inventory(repo, "starter"),
            &dogfood,
            &git_inventory(repo, "dogfood"),
        )
    };
    // Project-owned configuration and ignored output are out of scope.
    assert_eq!(drift(), Vec::<String>::new());
    std::fs::write(dogfood.join("scripts/gate.mjs"), "export const gate = 2;\n").expect("write");
    assert_eq!(drift(), vec!["byte drift at scripts/gate.mjs".to_owned()]);
    std::fs::remove_file(dogfood.join("scripts/gate.mjs")).expect("remove");
    assert_eq!(
        drift(),
        vec!["missing dogfood file scripts/gate.mjs".to_owned()]
    );
    // A case-only rename resolves through either spelling on a
    // case-insensitive filesystem, so only the exact names reveal it.
    std::fs::write(dogfood.join("scripts/GATE.mjs"), "export const gate = 1;\n").expect("write");
    assert_eq!(
        drift(),
        vec![
            "missing dogfood file scripts/gate.mjs".to_owned(),
            "scripts/GATE.mjs is in docs-portal but not in the shipped starter".to_owned(),
        ]
    );
    std::fs::remove_file(dogfood.join("scripts/GATE.mjs")).expect("remove");
    std::fs::write(dogfood.join("scripts/gate.mjs"), "export const gate = 1;\n").expect("write");
    assert_eq!(drift(), Vec::<String>::new());
    std::fs::write(
        dogfood.join("scripts/extra.mjs"),
        "export const extra = 1;\n",
    )
    .expect("write");
    assert_eq!(
        drift(),
        vec!["scripts/extra.mjs is in docs-portal but not in the shipped starter".to_owned()]
    );
}

/// Files under `dir` (relative to the repository at `root`) that Git tracks or
/// would track: untracked files count before they are staged, and ignored
/// dependency or build output never does. Paths are relative to `dir`.
fn git_inventory(root: &Path, dir: &str) -> Vec<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            dir,
        ])
        .output()
        .expect("git is available for portal parity verification");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let prefix = format!("{dir}/");
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).replace('\\', "/"))
        .map(|path| {
            path.strip_prefix(&prefix)
                .unwrap_or_else(|| panic!("{path} is not under {dir}"))
                .to_owned()
        })
        .collect()
}

/// Differences between the shipped starter and the dogfood portal. The exact
/// relative names are compared as strings in both directions, so a case-only
/// rename is reported even where the filesystem resolves either spelling;
/// bytes are compared only for names present on both sides.
/// `portal.config.json` is project-owned, so only its presence is compared.
fn portal_mirror_drift(
    starter: &Path,
    starter_files: &[String],
    dogfood: &Path,
    dogfood_files: &[String],
) -> Vec<String> {
    let starter_names: BTreeSet<&str> = starter_files.iter().map(String::as_str).collect();
    let dogfood_names: BTreeSet<&str> = dogfood_files.iter().map(String::as_str).collect();
    let mut problems: Vec<String> =
        starter_names
            .difference(&dogfood_names)
            .map(|relative| format!("missing dogfood file {relative}"))
            .chain(dogfood_names.difference(&starter_names).map(|relative| {
                format!("{relative} is in docs-portal but not in the shipped starter")
            }))
            .collect();
    for relative in starter_names.intersection(&dogfood_names) {
        if *relative == "portal.config.json" {
            continue;
        }
        if std::fs::read(starter.join(relative)).expect("read starter portal file")
            != std::fs::read(dogfood.join(relative)).expect("read dogfood portal file")
        {
            problems.push(format!("byte drift at {relative}"));
        }
    }
    problems.sort();
    problems
}

/// `.codeflow/docs-portal.json` records what `codeflow update` last installed
/// from the shipped starter: its version and, for a managed file, the hash of
/// the starter bytes. `codeflow update` rewrites a version or managed hash
/// that no longer matches the starter, so a starter edit that leaves the state
/// stale dirties the next update on `main`, and no check of the portal bytes
/// (`portal_dogfood_runtime_matches_the_shipped_starter`) sees it. A
/// user-owned hash is frozen provenance that update keeps while the starter
/// changes, so only its form is checked, and ownership equal to the starter
/// manifest's is an invariant of this repository that update does not repair.
#[test]
fn portal_state_matches_the_shipped_starter() {
    let root = repo_root();
    let state: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(".codeflow/docs-portal.json"))
            .expect("the repository keeps its portal state"),
    )
    .expect("the portal state is valid JSON");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("assets/docs-portal/manifest.json"))
            .expect("the portal manifest is readable"),
    )
    .expect("the portal manifest is valid JSON");
    let problems = portal_pin_drift(&state, &manifest, &root.join("assets/docs-portal/starter"));
    assert!(
        problems.is_empty(),
        "portal state drifted from the shipped starter; run `codeflow update` and commit \
         .codeflow/docs-portal.json:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn portal_pin_drift_names_each_stale_missing_and_extra_pin() {
    use codeflow_core::scaffold::sha256_hex;

    let starter = tempfile::tempdir().expect("temporary directory");
    let write = |relative: &str, bytes: &str| {
        std::fs::write(starter.path().join(relative), bytes).expect("write starter file");
    };
    write("a.mjs", "a\n");
    write("b.mjs", "b\n");
    write("own.json", "own v1\n");
    let manifest = serde_json::json!({
        "version": "2.0.0",
        "files": [
            {"path": "a.mjs", "ownership": "managed"},
            {"path": "b.mjs", "ownership": "managed"},
            {"path": "own.json", "ownership": "user-owned"},
        ]
    });
    let pin = |ownership: &str, bytes: &str| serde_json::json!({"ownership": ownership, "pristine_sha256": sha256_hex(bytes.as_bytes())});
    let clean = serde_json::json!({
        "starter_version": "2.0.0",
        "generator": {"version": "2.0.0"},
        "files": {
            "a.mjs": pin("managed", "a\n"),
            "b.mjs": pin("managed", "b\n"),
            "own.json": pin("user-owned", "own v1\n"),
        }
    });
    let drift = |state: &serde_json::Value| portal_pin_drift(state, &manifest, starter.path());
    assert_eq!(drift(&clean), Vec::<String>::new());
    // A managed starter file edited without re-pinning.
    write("b.mjs", "b changed\n");
    assert_eq!(drift(&clean), vec!["stale pin for b.mjs".to_owned()]);
    write("b.mjs", "b\n");
    // A user-owned starter default changes: update keeps the frozen hash.
    write("own.json", "own v2\n");
    assert_eq!(drift(&clean), Vec::<String>::new());
    // A missing pin, a managed pin for a file the starter no longer ships,
    // and a retired user-owned pin that update keeps.
    let mut state = clean.clone();
    state["files"]["gone.mjs"] = pin("managed", "gone\n");
    state["files"]["retired.json"] = pin("user-owned", "retired\n");
    state["files"]
        .as_object_mut()
        .expect("files object")
        .remove("a.mjs");
    assert_eq!(
        drift(&state),
        vec![
            "missing pin for a.mjs".to_owned(),
            "pin for gone.mjs is not in the shipped starter manifest".to_owned(),
        ]
    );
    // Changed ownership, invalid frozen hashes, and version metadata.
    let mut state = clean.clone();
    state["files"]["retired.json"] = pin("user-owned", "retired\n");
    state["files"]["retired.json"]["pristine_sha256"] = "short".into();
    state["files"]["a.mjs"]["ownership"] = "user-owned".into();
    state["files"]["own.json"]["pristine_sha256"] = "not-a-hash".into();
    state["starter_version"] = "1.9.9".into();
    state["generator"]["version"] = "1.9.9".into();
    assert_eq!(
        drift(&state),
        vec![
            "generator version 1.9.9 is not the shipped starter version 2.0.0".to_owned(),
            "ownership of a.mjs is user-owned, the starter manifest says managed".to_owned(),
            "starter_version 1.9.9 is not the shipped starter version 2.0.0".to_owned(),
            "user-owned pin for own.json is not a sha256".to_owned(),
            "user-owned pin for retired.json is not a sha256".to_owned(),
        ]
    );
}

/// Differences between the installed portal state and the shipped starter,
/// limited to what `codeflow update` rewrites: the version metadata, a
/// managed file's hash and the pin set. Update preserves an existing ownership
/// designation, so ownership equal to the manifest's is a repository
/// invariant this check holds, not something update repairs. A user-owned pin
/// is frozen provenance and only has to be a sha256, a retired user-owned pin
/// is kept by update, and any other extra pin is not.
fn portal_pin_drift(
    state: &serde_json::Value,
    manifest: &serde_json::Value,
    starter: &Path,
) -> Vec<String> {
    use codeflow_core::scaffold::sha256_hex;

    let well_formed = |hash: &str| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    };
    let version = manifest["version"].as_str().expect("manifest version");
    let mut problems = Vec::new();
    for (label, found) in [
        ("starter_version", &state["starter_version"]),
        ("generator version", &state["generator"]["version"]),
    ] {
        if found.as_str() != Some(version) {
            problems.push(format!(
                "{label} {} is not the shipped starter version {version}",
                found.as_str().unwrap_or("(missing)")
            ));
        }
    }
    let pins = state["files"].as_object().expect("state files object");
    let entries = manifest["files"].as_array().expect("manifest files array");
    let shipped: BTreeSet<&str> = entries
        .iter()
        .map(|entry| entry["path"].as_str().expect("manifest path string"))
        .collect();
    for entry in entries {
        let relative = entry["path"].as_str().expect("manifest path string");
        let ownership = entry["ownership"].as_str().expect("manifest ownership");
        let Some(pin) = pins.get(relative) else {
            problems.push(format!("missing pin for {relative}"));
            continue;
        };
        if pin["ownership"].as_str() != Some(ownership) {
            problems.push(format!(
                "ownership of {relative} is {}, the starter manifest says {ownership}",
                pin["ownership"].as_str().unwrap_or("(missing)")
            ));
            continue;
        }
        let hash = pin["pristine_sha256"].as_str().unwrap_or_default();
        if ownership == "user-owned" {
            if !well_formed(hash) {
                problems.push(format!("user-owned pin for {relative} is not a sha256"));
            }
        } else {
            let bytes = std::fs::read(starter.join(relative)).expect("read starter file");
            if hash != sha256_hex(&bytes) {
                problems.push(format!("stale pin for {relative}"));
            }
        }
    }
    for (relative, pin) in pins {
        if shipped.contains(relative.as_str()) {
            continue;
        }
        if pin["ownership"].as_str() != Some("user-owned") {
            problems.push(format!(
                "pin for {relative} is not in the shipped starter manifest"
            ));
        } else if !well_formed(pin["pristine_sha256"].as_str().unwrap_or_default()) {
            problems.push(format!("user-owned pin for {relative} is not a sha256"));
        }
    }
    problems.sort();
    problems
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
        "exact served URL from the tool output",
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

/// Collapse whitespace so a needle survives Markdown reflow.
fn normalized_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Read one shared presentation resource (the explanation method, the figure
/// grammar or its specimens), assert its cf-present and cf-docs-portal copies
/// are byte-identical, and return its whitespace-normalized text.
fn shared_presentation_resource(file: &str) -> String {
    let skills = repo_root().join("assets/base/agents/skills");
    let present = std::fs::read(skills.join("cf-present/resources").join(file))
        .unwrap_or_else(|_| panic!("present {file} is readable"));
    let portal = std::fs::read(skills.join("cf-docs-portal/resources").join(file))
        .unwrap_or_else(|_| panic!("portal {file} is readable"));
    assert_eq!(
        present, portal,
        "{file} drifted between cf-present and cf-docs-portal"
    );
    normalized_whitespace(&String::from_utf8(present).expect("shared resource is UTF-8"))
}

const FIGURE_FAMILIES: [&str; 9] = [
    "flow",
    "structure",
    "layering",
    "sequence",
    "state",
    "coverage",
    "extent",
    "derivation",
    "graph",
];

/// The figure grammar (ADR-0068) is one doctrine in two skills, with its
/// specimens in a companion file: each file's copies are byte-identical, the
/// doctrine names the companion, and neither file draws with a literal colour.
#[test]
fn figure_grammar_is_shared_names_its_specimens_and_draws_with_tokens_only() {
    let grammar = shared_presentation_resource("figure-grammar.md");
    let specimens = shared_presentation_resource("figure-grammar-specimens.md");
    assert!(
        grammar.contains(&normalized_whitespace(
            "`figure-grammar-specimens.md` beside this file, also byte-identical in both skills. Load it when authoring a figure"
        )),
        "figure grammar must name its specimens file and when to load it"
    );
    // A literal colour is `#` followed by exactly three or six hex digits and
    // then a non-alphanumeric boundary; anchors such as `SKILL.md#4` and ids
    // such as `#fg-cov-h` are not colours.
    for (file, text) in [
        ("figure-grammar.md", &grammar),
        ("figure-grammar-specimens.md", &specimens),
    ] {
        let bytes = text.as_bytes();
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
            !literal_colour && !text.contains("rgb("),
            "{file} must draw with --cf-fig-* tokens only, never a literal colour"
        );
    }
}

/// The doctrine keeps the nine families in one table, the twelve rules, the
/// altitude contract, the evidence board with its baselines as controls, and
/// the named anti-patterns. A smaller file that drops a family or a rule must
/// fail here.
#[test]
fn figure_grammar_keeps_its_families_rules_altitude_and_evidence() {
    let grammar = shared_presentation_resource("figure-grammar.md");
    let families_header =
        "| Family | Relationship it encodes | Geometry | Non-colour channels that carry state | Reader question it answers |";
    assert!(
        grammar.contains(families_header),
        "figure grammar lost the nine-families table header"
    );
    for family in FIGURE_FAMILIES {
        assert!(
            grammar.contains(&format!("| {family} |")),
            "figure grammar lost the {family} family row"
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
            grammar.contains(&normalized_whitespace(needle)),
            "figure grammar lost duty: {duty}"
        );
    }
}

/// The specimens companion keeps one numbered specimen per family, each with
/// its declared figure.
#[test]
fn figure_grammar_specimens_keep_one_specimen_per_family() {
    let specimens = shared_presentation_resource("figure-grammar-specimens.md");
    for (index, family) in FIGURE_FAMILIES.iter().enumerate() {
        assert!(
            specimens.contains(&format!("## {}. {family}", index + 1)),
            "figure grammar specimens lost the {family} specimen heading"
        );
        assert!(
            specimens.contains(&format!("data-cf-figure=\"{family}\"")),
            "figure grammar specimens lost the {family} specimen figure"
        );
    }
}

/// Both presentation skills name the figure grammar in their load list; the
/// shared doctrine names the evidence board and states once, in section 0,
/// that it is never cloned; the grammar names the board and points at that
/// rule; no reference names a bare design-exploration board (TSK-072).
#[test]
fn presentation_skills_point_at_the_figure_grammar_and_evidence_board() {
    let skills = repo_root().join("assets/base/agents/skills");
    let grammar = shared_presentation_resource("figure-grammar.md");
    assert!(
        grammar.contains("`docs/verification/tsk-014-w5/` is the evidence board")
            && grammar.contains("How the board may be used is the shared doctrine's section 0"),
        "figure grammar must name the evidence board and point at the doctrine's rule"
    );
    for skill in ["cf-present", "cf-docs-portal"] {
        let skill_text = std::fs::read_to_string(skills.join(skill).join("SKILL.md"))
            .expect("skill is readable");
        assert!(
            skill_text.contains("resources/figure-grammar.md"),
            "{skill}/SKILL.md must name the figure grammar in its load order"
        );
        let doctrine = normalized_whitespace(
            &std::fs::read_to_string(
                skills
                    .join(skill)
                    .join("resources/utility-presentation-system.md"),
            )
            .expect("shared doctrine is readable"),
        );
        assert!(
            doctrine.contains("`figure-grammar.md`")
                && doctrine.contains("### Figure grammar")
                && !doctrine.contains("### Stage and diagram grammar"),
            "{skill} shared doctrine must point at the figure grammar as the default form"
        );
        let section_zero = doctrine
            .split("## 1. Separation of planes")
            .next()
            .expect("doctrine has a section 0");
        assert!(
            section_zero.contains("docs/verification/tsk-014-w5/")
                && section_zero.contains("It is **evidence only**. Do not clone it"),
            "{skill} shared doctrine section 0 must name the evidence board and its rule"
        );
    }
    for (path, text) in presentation_skill_markdown("cf-present")
        .into_iter()
        .map(|(path, text)| (format!("cf-present/{path}"), text))
        .chain(
            presentation_skill_markdown("cf-docs-portal")
                .into_iter()
                .map(|(path, text)| (format!("cf-docs-portal/{path}"), text)),
        )
    {
        assert!(
            !text.contains("design-exploration board"),
            "{path} must not name a bare design-exploration board"
        );
    }
}

/// Every Markdown file of one presentation skill in the canonical source,
/// keyed by its path relative to the skill.
fn presentation_skill_markdown(skill: &str) -> BTreeMap<String, String> {
    let dir = repo_root().join("assets/base/agents/skills").join(skill);
    walk_files(&dir)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));
            (rel(&dir, &path), text)
        })
        .collect()
}

/// The number of paragraphs in `text` that state the evidence board rule: a
/// paragraph that names the board and carries a prohibition verb. A
/// paraphrase that avoids both the board's names and these verbs is not
/// detected; review owns that case.
fn board_rule_paragraphs(text: &str) -> usize {
    text.split("\n\n")
        .map(|paragraph| normalized_whitespace(paragraph).to_lowercase())
        .filter(|paragraph| {
            ["evidence board", "tsk-014-w5", "the board"]
                .iter()
                .any(|name| paragraph.contains(name))
                && ["clone", "re-render", "reproduce"]
                    .iter()
                    .any(|verb| paragraph.contains(verb))
        })
        .count()
}

/// The board-rule predicate ignores an unrelated clone command and counts a
/// second statement of the rule, however it is worded among its verbs.
#[test]
fn board_rule_predicate_ignores_clone_commands_and_counts_duplicates() {
    assert_eq!(
        board_rule_paragraphs("Run `git clone <url>` to fetch the repository."),
        0
    );
    assert_eq!(
        board_rule_paragraphs("How the board may be used is the shared doctrine's section 0."),
        0
    );
    let doctrine = std::fs::read_to_string(
        repo_root()
            .join("assets/base/agents/skills/cf-present/resources/utility-presentation-system.md"),
    )
    .expect("shared doctrine is readable");
    assert_eq!(board_rule_paragraphs(&doctrine), 1);
    let duplicated =
        format!("{doctrine}\n\nThe evidence board is evidence only; never reproduce its cases.\n");
    assert_eq!(board_rule_paragraphs(&duplicated), 2);
}

/// TSK-072 dedupe: in each presentation skill the altitude contract table
/// header is stated once, in the figure grammar; the evidence board path
/// appears only in the doctrine and the grammar; and exactly one paragraph,
/// in the doctrine, states the rule not to clone the board. The rule count
/// uses `board_rule_paragraphs`, so it proves no second statement names the
/// board with a prohibition verb; it does not prove the absence of every
/// paraphrase.
#[test]
fn presentation_skills_state_the_altitude_table_and_board_rule_once() {
    let mut problems = Vec::new();
    for skill in ["cf-present", "cf-docs-portal"] {
        let files = presentation_skill_markdown(skill);
        let count_where = |predicate: &dyn Fn(&str) -> usize| -> BTreeMap<String, usize> {
            files
                .iter()
                .map(|(path, text)| (path.clone(), predicate(text)))
                .filter(|(_, count)| *count > 0)
                .collect()
        };
        let altitude_headers = count_where(&|text| {
            text.lines()
                .filter(|line| line.trim_start().starts_with("| Altitude |"))
                .count()
        });
        let expected_altitude = BTreeMap::from([("resources/figure-grammar.md".to_string(), 1)]);
        if altitude_headers != expected_altitude {
            problems.push(format!(
                "{skill}: altitude table headers {altitude_headers:?}, want {expected_altitude:?}"
            ));
        }
        let board: BTreeSet<String> =
            count_where(&|text| text.matches("docs/verification/tsk-014-w5/").count())
                .into_keys()
                .collect();
        let expected_board = BTreeSet::from([
            "resources/figure-grammar.md".to_string(),
            "resources/utility-presentation-system.md".to_string(),
        ]);
        if board != expected_board {
            problems.push(format!(
                "{skill}: evidence board path in {board:?}, want {expected_board:?}"
            ));
        }
        let board_rule = count_where(&board_rule_paragraphs);
        let expected_rule =
            BTreeMap::from([("resources/utility-presentation-system.md".to_string(), 1)]);
        if board_rule != expected_rule {
            problems.push(format!(
                "{skill}: the evidence board rule is stated in {board_rule:?}, want {expected_rule:?}"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "presentation doctrine is restated:\n  {}",
        problems.join("\n  ")
    );
}

/// The explanation method (TSK-072) is one shared resource in both
/// presentation skills. It keeps its five stages, a reader and question per
/// altitude, a carrier row per family plus screenshot, table and the
/// no-family boundary, the README and smallest-carrier rules, three worked
/// decisions and the sentence on sources it leaves alone.
#[test]
fn explanation_method_is_shared_and_carries_its_stages_readers_and_carriers() {
    let method = shared_presentation_resource("explanation-method.md");
    let mut missing = Vec::new();
    for stage in [
        "## 1. Reader and question",
        "## 2. Altitude",
        "## 3. Carrier",
        "## 4. Draft",
        "## 5. Check",
    ] {
        if !method.contains(stage) {
            missing.push(stage.to_string());
        }
    }
    for reader in [
        "| Concept | someone deciding whether the subject is for them |",
        "| Architecture | an engineer who will change or integrate it |",
        "| Technical | an operator or a reviewer |",
        "| How-to section | someone doing the task now |",
        "| Reply | the operator who asked |",
        "A pull request body takes the reply row's reader, an ADR takes the Architecture row's, and a README takes the Concept row's",
    ] {
        if !method.contains(reader) {
            missing.push(reader.to_string());
        }
    }
    for family in FIGURE_FAMILIES {
        let row = format!("| {family} | ");
        if !method.contains(&row) {
            missing.push(row);
        }
    }
    for marker in [
        "| a surface as it is | screenshot |",
        "| facts to look up, with no relationship | table |",
        "| a distribution, or a series over time | no family draws it today |",
        "In a chat reply, the surface rule in `cf-method/references/workflow-lifecycle.md` picks the form; the fenced ASCII chat form is its plain-text form.",
        "In a README, a figure is the fenced ASCII chat form",
        "An SVG file and a Mermaid fence are not README figures",
        "GitHub shows a `cf-stage` fence as code",
        "The smallest carrier that keeps the depth wins",
        "Facts with no relationship between them take bullets or a table and no figure",
        "nothing asks a page or a document to record the stages",
        "The method never rewrites a source it must leave alone",
        "declared `illustrated` with a companion figure bound in configuration or `pass-through`",
        "`governance` for governance text, and `no-relationship`, with the design primary's note",
        "lists for the altitude; when it is not, the relationship belongs at another altitude, so go back to stage 2",
        "with Architecture only when a second structural view is needed",
        "The second draws the supported architecture of a consuming repository whose remote protection has been verified active",
        "**Substitute:** draw your own repository's verified enforcement state",
        "Derivation is the rival and loses here",
        "rules 1 and 7 partial on the gate surfaces and covered in the review column",
        "A figure also passes the grammar's self-check, `figure-grammar.md` section 8",
    ] {
        if !method.contains(&normalized_whitespace(marker)) {
            missing.push(marker.to_string());
        }
    }
    if method.contains("marked not claimed") {
        missing.push("rules decided by review are not marked not claimed".to_string());
    }
    let decisions: Vec<&str> = method.split("### ").skip(1).collect();
    if decisions.len() != 3 {
        missing.push(format!("three worked decisions, found {}", decisions.len()));
    }
    for (decision, (altitude, family)) in decisions.iter().zip([
        ("Concept", "structure"),
        ("Architecture", "layering"),
        ("Technical", "coverage"),
    ]) {
        for field in [
            format!("**Altitude:** {altitude}."),
            "**Reader:**".to_string(),
            "**Question:**".to_string(),
            format!("**Family:** {family}"),
        ] {
            if !decision.contains(&field) {
                missing.push(format!("worked decision {altitude}: {field}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "explanation method lost:\n  {}",
        missing.join("\n  ")
    );
}

/// The rows of the first Markdown table under `heading` in `text`, each a
/// list of trimmed cells, header and rule rows dropped.
fn table_rows_under(text: &str, heading: &str) -> Vec<Vec<String>> {
    let section = text
        .split(heading)
        .nth(1)
        .unwrap_or_else(|| panic!("no section {heading}"));
    let section = section.split("\n## ").next().unwrap_or(section);
    section
        .lines()
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            line.trim_matches('|')
                .split(" | ")
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}

/// Codex R1 controls: stages 4 and 5 apply by carrier, so an answer with no
/// figure and a table lookup draft only the universal parts and pass on the
/// universal checks, while every figure check still holds for a figure.
#[test]
fn explanation_method_lets_a_figureless_answer_and_a_table_lookup_pass() {
    let method = std::fs::read_to_string(
        repo_root().join("assets/base/agents/skills/cf-present/resources/explanation-method.md"),
    )
    .expect("explanation method is readable");
    let parts = table_rows_under(&method, "## 4. Draft");
    let checks = table_rows_under(&method, "## 5. Check");
    let named = |rows: &[Vec<String>], scope: &str| -> BTreeSet<String> {
        rows.iter()
            .filter(|row| row.get(1).map(String::as_str) == Some(scope))
            .map(|row| row[0].clone())
            .collect()
    };
    let universal_parts = named(&parts, "every answer");
    let universal_checks = named(&checks, "every answer");
    // An answer with no figure and a table lookup take only the universal rows.
    // TSK-073: the lead is drafted only for an answer with a carrier, so a
    // short answer, its own summary under the copy guide, takes no lead.
    assert!(
        parts.iter().any(|row| row[0] == "Lead"
            && row[1] == "an answer with a carrier"
            && row[2] == "one sentence saying what the reader is looking at, above the carrier"),
        "the lead is drafted only for an answer with a carrier"
    );
    for carrier in ["an answer with no figure", "a table lookup"] {
        assert_eq!(
            universal_parts,
            BTreeSet::from(["Acting text".to_string()]),
            "{carrier} takes only the acting text as a universal part"
        );
        assert_eq!(
            universal_checks,
            BTreeSet::from([
                "Sources".to_string(),
                "Copy".to_string(),
                "Policy characters".to_string()
            ]),
            "{carrier} must pass on the universal checks alone"
        );
    }
    assert_eq!(
        named(&checks, "a figure"),
        BTreeSet::from([
            "Masked title".to_string(),
            "Removal".to_string(),
            "State channels".to_string()
        ]),
        "every figure check still holds for a figure"
    );
    for part in ["Declaration", "Twin"] {
        assert!(
            parts
                .iter()
                .any(|row| row[0] == part && row[1] == "a figure on the portal or in present"),
            "{part} is drafted only for a figure on the portal or in present"
        );
    }
    let normalized = normalized_whitespace(&method);
    for marker in [
        "An answer with no figure, a table lookup included, passes on the universal checks alone",
        "An answer with no carrier drafts its summary, when it leads into a list, and the acting text; a short answer drafts only the answer.",
        "a chat form: every pair of marks differs in glyph and each is keyed in the legend line",
        "A chat form has no declaration file or twin",
    ] {
        assert!(
            normalized.contains(marker),
            "explanation method lost: {marker}"
        );
    }
}

/// The paragraphs of `text` that tie a chat figure to the ASCII form without
/// deferring to the lifecycle's surface rule: the paragraph names ASCII and a
/// chat medium, and neither scopes it to a plain-text surface nor points at
/// `workflow-lifecycle.md`. A paraphrase that avoids these words is not
/// detected; review owns that case.
fn unconditional_ascii_chat_paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n")
        .map(normalized_whitespace)
        .filter(|paragraph| {
            let lower = paragraph.to_lowercase();
            lower.contains("ascii")
                && ["in chat", "for chat", "chat reply"]
                    .iter()
                    .any(|medium| lower.contains(medium))
                && !lower.contains("plain-text")
                && !lower.contains("workflow-lifecycle.md")
                && !lower.contains("writing.md")
        })
        .collect()
}

/// The lifecycle owns the reply surface rule: an inline HTML figure where
/// the harness renders one, a `cf-present` page for a full page, and fenced
/// ASCII only on a plain-text or unknown surface. While it does, no Markdown
/// file of either presentation skill (the explanation method, the figure
/// grammar and the portal content contract included) may state that a chat
/// figure is ASCII without deferring to that rule.
#[test]
fn presentation_chat_figures_defer_to_the_lifecycle_surface_rule() {
    // TSK-184 moved the surface rule from the lifecycle to the writing
    // reference; the lifecycle points there.
    let lifecycle = normalized_whitespace(
        &std::fs::read_to_string(repo_root().join("assets/base/rules/writing.md"))
            .expect("writing reference is readable"),
    );
    for rule in [
        "Where the current surface renders one, an inline figure is the default, such as an inline HTML figure in a desktop harness.",
        "Use fenced ASCII in other Markdown files (READMEs, docs, records, PR bodies), in terminal output and on any other plain-text surface, or when unsure what the surface renders.",
    ] {
        assert!(
            lifecycle.contains(rule),
            "writing reference lost the reply surface rule: {rule}"
        );
    }
    assert_eq!(
        unconditional_ascii_chat_paragraphs(
            "In chat and in a README, a figure is the fenced ASCII chat form."
        )
        .len(),
        1,
        "the predicate must flag an unconditional ASCII chat rule"
    );
    let mut problems = Vec::new();
    for skill in ["cf-present", "cf-docs-portal"] {
        for (path, text) in presentation_skill_markdown(skill) {
            for paragraph in unconditional_ascii_chat_paragraphs(&text) {
                problems.push(format!("{skill}/{path}: {paragraph}"));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "chat figure rules must defer to the lifecycle surface rule:\n  {}",
        problems.join("\n  ")
    );
}

/// Each presentation skill carries its only load list in `SKILL.md`; the
/// references that used to restate it carry one pointer line instead.
#[test]
fn presentation_references_point_at_the_one_load_list() {
    const POINTER: &str = "**Load order:** the one list in [SKILL.md](../SKILL.md).";
    let mut problems = Vec::new();
    for skill in ["cf-present", "cf-docs-portal"] {
        let files = presentation_skill_markdown(skill);
        let lists: Vec<&String> = files
            .iter()
            .filter(|(_, text)| text.contains("load in order"))
            .map(|(path, _)| path)
            .collect();
        if lists != ["SKILL.md"] {
            problems.push(format!(
                "{skill}: load lists in {lists:?}, want SKILL.md only"
            ));
        }
        for (path, text) in &files {
            for retired in ["Required load order", "Required first", "Thinking first"] {
                if text.contains(retired) {
                    problems.push(format!("{skill}/{path}: still says {retired}"));
                }
            }
        }
    }
    for path in [
        "cf-present/references/visual-craft.md",
        "cf-present/references/document-authoring.md",
        "cf-present/resources/how-presentation-works.md",
        "cf-docs-portal/references/visual-craft.md",
    ] {
        let text =
            std::fs::read_to_string(repo_root().join("assets/base/agents/skills").join(path))
                .expect("reference is readable");
        let pointers = text.lines().filter(|line| *line == POINTER).count();
        let head: Vec<&str> = text
            .lines()
            .filter(|line| !line.is_empty())
            .take(2)
            .collect();
        if pointers != 1 || head.get(1) != Some(&POINTER) {
            problems.push(format!(
                "{path}: the load-order preamble must be the one pointer line under the title"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "load order is restated:\n  {}",
        problems.join("\n  ")
    );
}

/// `how-presentation-works.md` keeps only present content: what the human
/// sees, block order as attention order, the mental models and the worked
/// contrast. The general steps and the self-check belong to the method.
#[test]
fn how_presentation_works_keeps_only_present_content() {
    let text = std::fs::read_to_string(
        repo_root()
            .join("assets/base/agents/skills/cf-present/resources/how-presentation-works.md"),
    )
    .expect("how-presentation-works is readable");
    for step in ["A", "B", "C", "D", "E", "F"] {
        assert!(
            !text.contains(&format!("### Step {step}")),
            "how-presentation-works still carries Step {step}"
        );
    }
    assert!(
        !text.contains("Self-check"),
        "how-presentation-works still carries the self-check the method owns"
    );
    for kept in [
        "## 1. What the human actually encounters",
        "**order of `blocks[]` = order of attention.**",
        "## 2. Why structure is the presentation",
        "## 4. Mental models for present blocks",
        "## 5. Worked contrast (same facts, different thinking)",
        "Work through `explanation-method.md` first",
    ] {
        assert!(
            text.contains(kept),
            "how-presentation-works lost present content: {kept}"
        );
    }
}

/// Both presentation skills load the explanation method as item 1, before
/// the shared doctrine, and `cf-design` routes utility surfaces to them.
#[test]
fn presentation_skills_load_the_explanation_method_first() {
    let skills = repo_root().join("assets/base/agents/skills");
    for skill in ["cf-present", "cf-docs-portal"] {
        let text = std::fs::read_to_string(skills.join(skill).join("SKILL.md"))
            .expect("skill is readable");
        let list = text
            .split("load in order:**")
            .nth(1)
            .unwrap_or_else(|| panic!("{skill}/SKILL.md has no load list"));
        let first = list
            .lines()
            .find(|line| line.starts_with("1. "))
            .unwrap_or_else(|| panic!("{skill}/SKILL.md load list has no item 1"));
        assert!(
            first.contains("(resources/explanation-method.md)"),
            "{skill}/SKILL.md must load resources/explanation-method.md as item 1, found {first}"
        );
        let method = list.find("resources/explanation-method.md");
        let doctrine = list.find("resources/utility-presentation-system.md");
        assert!(
            method.is_some() && doctrine.is_some() && method < doctrine,
            "{skill}/SKILL.md must load the method before the shared doctrine"
        );
    }
    let design = normalized_whitespace(
        &std::fs::read_to_string(skills.join("cf-design/SKILL.md")).expect("cf-design is readable"),
    );
    let section_one = design
        .split("## 1. Select the process weight")
        .nth(1)
        .and_then(|rest| rest.split("## 2.").next())
        .expect("cf-design has section 1");
    assert!(
        section_one.contains(
            "Utility surfaces (portal, present) load `cf-docs-portal` or `cf-present` instead."
        ),
        "cf-design section 1 must route utility surfaces to their skills"
    );
}

/// One measured count per family, read off the SVG specimen's facts: stop
/// bars, copies, act markers, calls, states, covered cells, compares and
/// critical edges.
fn chat_form_measures(family: &str) -> &'static [(&'static str, usize)] {
    match family {
        "flow" => &[("|", 2), ("(H)", 1)],
        "structure" => &[("[ .", 4)],
        "layering" => &[("*", 9)],
        "sequence" => &[("->|", 3), ("<==", 2)],
        "state" => &[("[", 7)],
        "coverage" => &[("#", 12), ("X", 1)],
        "derivation" => &[("<~>", 2)],
        "graph" => &[("====", 5), ("<-", 11)],
        _ => &[],
    }
}

/// Measured counts in one chat form against its specimen's facts.
fn chat_form_measure_problems(family: &str, lines: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    let drawing: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| !line.starts_with("Legend: ") && !line.starts_with("Caption: "))
        .collect();
    let measured = |needle: &str| {
        drawing
            .iter()
            .map(|line| line.matches(needle).count())
            .sum::<usize>()
    };
    for (needle, want) in chat_form_measures(family) {
        let got = measured(needle);
        if got != *want {
            problems.push(format!(
                "{family}: {got} of {needle:?} drawn, the specimen has {want}"
            ));
        }
    }
    if family == "extent" {
        // Two characters a column, odd lengths rounded up: the used runs of
        // the subject, the subject line and the three bullets.
        let runs: Vec<usize> = drawing
            .iter()
            .filter_map(|line| {
                let start = line.find('#')?;
                Some(line[start..].chars().take_while(|c| *c == '#').count())
            })
            .collect();
        let want: Vec<usize> = [31, 31, 46, 60, 38]
            .iter()
            .map(|n: &usize| n.div_ceil(2))
            .collect();
        if runs != want {
            problems.push(format!("extent: used runs {runs:?}, want {want:?}"));
        }
    }
    problems
}

/// Every specimen family carries a chat form (TSK-072): a fenced text block
/// with one legend line and one caption line, every line printable ASCII and
/// under 78 columns, so it renders unwrapped in an 80-column terminal.
#[test]
fn figure_grammar_specimens_carry_a_chat_form_per_family() {
    let skills = repo_root().join("assets/base/agents/skills");
    let specimens =
        std::fs::read_to_string(skills.join("cf-present/resources/figure-grammar-specimens.md"))
            .expect("specimens are readable");
    let sections: Vec<&str> = specimens.split("\n## ").skip(1).collect();
    assert_eq!(
        sections.len(),
        FIGURE_FAMILIES.len(),
        "one section per family"
    );
    let mut problems = Vec::new();
    for (section, family) in sections.iter().zip(FIGURE_FAMILIES) {
        let Some(chat) = section.split("\n### Chat form\n").nth(1) else {
            problems.push(format!("{family}: no Chat form section"));
            continue;
        };
        if section.matches("\n### Chat form\n").count() != 1 {
            problems.push(format!("{family}: more than one Chat form section"));
        }
        let Some(block) = chat
            .split("```text\n")
            .nth(1)
            .and_then(|rest| rest.split("\n```").next())
        else {
            problems.push(format!(
                "{family}: the chat form is not a fenced text block"
            ));
            continue;
        };
        let lines: Vec<&str> = block.lines().collect();
        let legends = lines
            .iter()
            .filter(|line| line.starts_with("Legend: "))
            .count();
        let captions = lines
            .iter()
            .filter(|line| line.starts_with("Caption: "))
            .count();
        if legends != 1 || captions != 1 {
            problems.push(format!(
                "{family}: {legends} legend lines and {captions} caption lines, want one each"
            ));
        }
        if lines
            .last()
            .is_none_or(|line| !line.starts_with("Caption: "))
        {
            problems.push(format!("{family}: the caption is not the last line"));
        }
        for line in &lines {
            if line.chars().count() >= 78 {
                problems.push(format!(
                    "{family}: {} columns: {line}",
                    line.chars().count()
                ));
            }
            if !line.chars().all(|c| (' '..='~').contains(&c)) {
                problems.push(format!("{family}: not printable ASCII: {line}"));
            }
            if line.contains('\u{2013}') || line.contains('\u{2014}') {
                problems.push(format!("{family}: en or em dash: {line}"));
            }
        }
        problems.extend(chat_form_measure_problems(family, &lines));
    }
    assert!(
        problems.is_empty(),
        "chat forms are incomplete:\n  {}",
        problems.join("\n  ")
    );
}

/// The shared doctrine states the screenshot and raster image rules beside
/// its media carrier, the portal content contract tells authors what a figure
/// is in each medium, and the portal visual craft walks one page through the
/// method with a reader, a question and a family per panel.
#[test]
fn presentation_doctrine_carries_screenshot_rules_figure_media_and_the_page_walk() {
    let skills = repo_root().join("assets/base/agents/skills");
    let read = |path: &str| {
        normalized_whitespace(
            &std::fs::read_to_string(skills.join(path))
                .unwrap_or_else(|error| panic!("{path} is readable: {error}")),
        )
    };
    let mut missing = Vec::new();
    for skill in ["cf-present", "cf-docs-portal"] {
        let doctrine = read(&format!("{skill}/resources/utility-presentation-system.md"));
        for rule in [
            "### Screenshots and raster images",
            "| Media (a screenshot or photograph) |",
            "A screenshot shows a surface as it is and never a relationship",
            "Capture CodeFlow utility chrome (a portal or present screen) in the Graphite skin in light at 2x, unless the subject is a skin or a mode",
            "A screenshot of a consuming project's own product keeps that product's default appearance",
            "Crop to the surface plus a margin of 16 CSS px on every side (32 image pixels at 2x)",
            "Annotate only with numbered markers keyed in the caption; never draw arrows",
            "Write alt text that names the surface and its state",
            "Save chrome as PNG and photographs as WebP, inside the adapter's media limits",
            "8 MiB per file (`MAX_MEDIA_BYTES`), 64 MiB in all (`MAX_TOTAL_MEDIA_BYTES`)",
            "Commit it beside its source, in a folder named for the page",
            "Refresh it when the surface changes",
            "An imported raster diagram is never a carrier: redraw it in a family",
        ] {
            if !doctrine.contains(&normalized_whitespace(rule)) {
                missing.push(format!("{skill} doctrine: {rule}"));
            }
        }
    }
    let contract = read("cf-docs-portal/references/content-contract.md");
    for marker in [
        "SVG/PDF copies, traversal, unsupported schemes, and broken targets fail closed",
        "For authors: on the portal and in present a figure is inline SVG through the figure block, in a README it is the ASCII chat form, and in a chat reply the surface rule in `cf-method/references/workflow-lifecycle.md` picks the form.",
    ] {
        if !contract.contains(marker) {
            missing.push(format!("content contract: {marker}"));
        }
    }
    let craft = read("cf-docs-portal/references/visual-craft.md");
    for marker in [
        "## 1. How a portal page thinks",
        "| Panel | Reader | Question | Family and what it draws |",
        "| Concept | someone deciding",
        "| Architecture | an engineer",
        "| Technical | a reviewer",
        "| How-to: land a change | someone landing a change now",
        "the human merge as the one decision no plane makes",
        "the git discipline page of a consuming repository whose remote protection has been verified active",
        "Substitute your repository's verified enforcement state before drawing these panels",
        "| structure:",
        "| layering:",
        "| coverage:",
        "| sequence:",
    ] {
        if !craft.contains(marker) {
            missing.push(format!("portal visual craft: {marker}"));
        }
    }
    assert!(
        missing.is_empty(),
        "presentation doctrine lost:\n  {}",
        missing.join("\n  ")
    );
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

/// Both products use the design kit's 27 colour roles in all six skin/mode
/// pairs, with independent font preferences. The starter and live portal
/// remain byte-identical; the kit JSON is the shared value authority.
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
    let root = repo_root();
    let present = std::fs::read_to_string(root.join("crates/codeflow-present/web/src/styles.css"))
        .expect("present styles");
    let portal = std::fs::read_to_string(
        root.join("assets/docs-portal/starter/src/styles/utility-tokens.css"),
    )
    .expect("starter tokens");
    let live = std::fs::read_to_string(root.join("docs-portal/src/styles/utility-tokens.css"))
        .expect("live tokens");
    assert_eq!(
        portal, live,
        "live portal and starter must remain identical"
    );
    let kit: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            root.join("assets/base/agents/skills/cf-present/resources/design-system/tokens.json"),
        )
        .expect("kit tokens"),
    )
    .expect("kit JSON");
    for skin in ["graphite", "slate", "sage"] {
        for mode in ["light", "dark"] {
            let pair = format!("{skin}-{mode}");
            let roles = kit[&pair].as_object().expect("kit pair");
            assert_eq!(roles.len(), 27, "complete kit role set");
            for (sheet, css, marker) in [
                (
                    "portal",
                    &portal,
                    format!("[data-cfp-skin=\"{skin}\"][data-theme=\"{mode}\"]"),
                ),
                (
                    "present",
                    &present,
                    format!("[data-cf-theme=\"{skin}\"][data-cf-mode-resolved=\"{mode}\"]"),
                ),
            ] {
                let actual = block_after(css, &marker);
                for (role, value) in roles {
                    assert_eq!(
                        actual.get(role).map(String::as_str),
                        value.as_str(),
                        "{sheet} {pair} {role}"
                    );
                }
            }
        }
    }
    let default = block_after(&present, ":root {");
    for (role, value) in kit["graphite-light"].as_object().expect("default pair") {
        assert_eq!(
            default.get(role).map(String::as_str),
            value.as_str(),
            "present prepaint default {role}"
        );
    }
    for face in ["archivo", "inter", "plex"] {
        let portal = block_after(&portal, &format!("[data-cfp-typeface=\"{face}\"]"));
        let present = block_after(&present, &format!("[data-cf-typeface=\"{face}\"]"));
        assert_eq!(
            portal.get("font-sans"),
            present.get("font-sans"),
            "font {face}"
        );
        assert!(portal.contains_key("font-sans"));
    }
    let portal_default = block_after(&portal, ":root {");
    for role in ["font-mono", "font-sans"] {
        assert_eq!(
            portal_default.get(role),
            default.get(role),
            "default {role}"
        );
        assert!(default.contains_key(role));
    }
}

/// The copy guide (TSK-073): one section per string type, the home of the
/// writing rules that other skill files point at.
const COPY_GUIDE: &str = "assets/base/agents/skills/cf-editorial-review/references/copy-guide.md";

/// The thirteen sections of the copy guide, in order (TSK-073 AC-2).
const COPY_GUIDE_SECTIONS: [&str; 13] = [
    "Voice",
    "Sentences",
    "Words",
    "Titles and headings",
    "Leads",
    "Captions",
    "Legend keys and descriptions",
    "Summaries",
    "Bullets and tables",
    "Microcopy",
    "Replies",
    "Skill prose",
    "ADR and PR shapes",
];

fn repo_text(relative: &str) -> String {
    std::fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("{relative} is readable: {error}"))
}

/// The `## ` sections of a Markdown file, each as (heading, body).
fn level_two_sections(text: &str) -> Vec<(String, String)> {
    let mut sections: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            sections.push((heading.trim().to_string(), String::new()));
        } else if let Some((_, body)) = sections.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    sections
}

/// The examples in one guide section. Each is a line
/// ``Example, <label> (source: `<path>`):`` (the label is optional) followed
/// by one or more `> ` lines that hold the exact string. Returns the
/// (source path, whitespace-folded example text) pairs and the problems: a
/// line that starts with "Example" but does not parse, a heading with no
/// quotation, and a quotation with no parsed heading above it.
fn copy_guide_examples(body: &str) -> (Vec<(String, String)>, Vec<String>) {
    let heading = regex::Regex::new(r"^Example(?:, .+?)? \(source: `([^`]+)`\):$")
        .expect("valid example heading pattern");
    let mut examples = Vec::new();
    let mut problems = Vec::new();
    let mut lines = body.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with('>') {
            problems.push(format!("quotation with no example heading: {trimmed}"));
            continue;
        }
        if !trimmed.starts_with("Example") {
            continue;
        }
        let Some(source) = heading.captures(trimmed).map(|found| found[1].to_string()) else {
            problems.push(format!("example heading that does not parse: {trimmed}"));
            continue;
        };
        while lines.peek().is_some_and(|next| next.trim().is_empty()) {
            lines.next();
        }
        let mut quoted = Vec::new();
        while let Some(next) = lines.peek() {
            let Some(text) = next.trim_start().strip_prefix('>') else {
                break;
            };
            quoted.push(text.trim().to_string());
            lines.next();
        }
        if quoted.is_empty() {
            problems.push(format!("example heading with no quotation: {trimmed}"));
        }
        examples.push((source, normalized_whitespace(&quoted.join(" "))));
    }
    (examples, problems)
}

/// Where a named example source lives: a path starting with `cf-` is inside
/// the shipped skill trees, anything else is relative to the repository root.
fn copy_guide_source(path: &str) -> PathBuf {
    if path.starts_with("cf-") {
        for tree in ["assets/base/agents/skills", "assets/base/claude/skills"] {
            let candidate = repo_root().join(tree).join(path);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    repo_root().join(path)
}

/// The visible text of an HTML source: tags dropped, the common entities
/// unescaped, whitespace normalized.
fn html_visible_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    let text = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&rsquo;", "\u{2019}")
        .replace("&amp;", "&");
    normalized_whitespace(&text)
}

/// Whether `example` appears verbatim in the source at `path`.
fn example_resolves(path: &str, example: &str) -> Result<bool, String> {
    let file = copy_guide_source(path);
    let source = std::fs::read_to_string(&file)
        .map_err(|error| format!("{path}: source is not readable: {error}"))?;
    let is_html = file
        .extension()
        .is_some_and(|extension| extension == "html");
    Ok(normalized_whitespace(&source).contains(example)
        || (is_html && html_visible_text(&source).contains(example)))
}

/// TSK-073 AC-2. The guide carries its thirteen sections in order.
#[test]
fn copy_guide_carries_its_thirteen_sections_in_order() {
    let headings: Vec<String> = level_two_sections(&repo_text(COPY_GUIDE))
        .into_iter()
        .map(|(heading, _)| heading)
        .collect();
    assert_eq!(
        headings, COPY_GUIDE_SECTIONS,
        "copy guide sections drifted from the thirteen in TSK-073"
    );
}

/// TSK-107 AC-4 (SPC-013 R-37): every shipped record template and the PR
/// template are free of em and en dashes, so a record or PR body an adopter
/// starts from never trips the policy-character rule.
#[test]
fn shipped_record_and_pr_templates_carry_no_dash() {
    let root = repo_root();
    let mut found = Vec::new();
    for rel in [
        "assets/base/pm/epic.md.tmpl",
        "assets/base/pm/spec.md.tmpl",
        "assets/base/pm/task.md.tmpl",
        "assets/base/ci/pull_request_template.md",
    ] {
        let text = std::fs::read_to_string(root.join(rel)).expect("template is readable");
        for (i, line) in text.lines().enumerate() {
            if line.contains('\u{2014}') || line.contains('\u{2013}') {
                found.push(format!("{rel}:{}", i + 1));
            }
        }
    }
    assert!(found.is_empty(), "dashes in shipped templates: {found:?}");
}

/// TSK-073 AC-3. Every section carries at least one example, and every
/// example is found verbatim in the source it names. A string that is not
/// in its source fails the check.
#[test]
fn copy_guide_examples_resolve_verbatim_in_their_named_sources() {
    let mut problems = Vec::new();
    let mut first = None;
    for (heading, body) in level_two_sections(&repo_text(COPY_GUIDE)) {
        let (examples, malformed) = copy_guide_examples(&body);
        problems.extend(
            malformed
                .into_iter()
                .map(|problem| format!("{heading}: {problem}")),
        );
        if examples.is_empty() {
            problems.push(format!("{heading}: no example"));
        }
        for (source, example) in examples {
            if example.is_empty() {
                problems.push(format!("{heading}: empty example from {source}"));
                continue;
            }
            match example_resolves(&source, &example) {
                Ok(true) => {
                    first.get_or_insert((source, example));
                }
                Ok(false) => problems.push(format!("{heading}: not in {source}: {example}")),
                Err(error) => problems.push(format!("{heading}: {error}")),
            }
        }
    }
    assert!(
        problems.is_empty(),
        "copy guide examples must resolve verbatim:\n  {}",
        problems.join("\n  ")
    );
    // The check fails on a string its source does not hold.
    let (source, example) = first.expect("the guide carries at least one example");
    assert_eq!(
        example_resolves(&source, &format!("{example} TSK-073 absent")),
        Ok(false),
        "the resolution check must fail on a missing string"
    );
    // Negative controls (Codex review R1, finding 3): an example heading the
    // parser cannot read and a quotation with no heading both fail, even
    // beside a well-formed example; a labelled heading parses.
    let (parsed, malformed) = copy_guide_examples(
        "Example, a label (source: `cf-present/SKILL.md`):\n> text\n\n\
         Example from `cf-present/SKILL.md`:\n> stale text\n\n> orphan quotation\n",
    );
    assert_eq!(
        parsed,
        vec![("cf-present/SKILL.md".to_string(), "text".to_string())]
    );
    assert_eq!(
        malformed,
        vec![
            "example heading that does not parse: Example from `cf-present/SKILL.md`:".to_string(),
            "quotation with no example heading: > stale text".to_string(),
            "quotation with no example heading: > orphan quotation".to_string(),
        ]
    );
}

/// The words of the lifecycle reply rule that make a short answer its own
/// summary: no lead, no heading, no recap.
const SHORT_ANSWER_EXCEPTION: [&str; 2] = [
    "A simple answer stays simple",
    "a one-line answer stays one line",
];

/// TSK-073 AC-4. The Summaries and Replies sections carry the short-answer
/// exception in the words of the lifecycle reply rule.
#[test]
fn copy_guide_keeps_the_short_answer_exception_in_summaries_and_replies() {
    // TSK-184 moved the reply rule into the writing reference.
    let lifecycle = normalized_whitespace(&repo_text("assets/base/rules/writing.md"));
    let sections: BTreeMap<String, String> = level_two_sections(&repo_text(COPY_GUIDE))
        .into_iter()
        .map(|(heading, body)| (heading, normalized_whitespace(&body)))
        .collect();
    for marker in SHORT_ANSWER_EXCEPTION {
        assert!(
            lifecycle.contains(marker),
            "the writing reference's reply rule lost: {marker}"
        );
        for section in ["Summaries", "Replies"] {
            let body = sections
                .get(section)
                .unwrap_or_else(|| panic!("copy guide lost its {section} section"));
            assert!(
                body.contains(marker),
                "copy guide {section} lost the short-answer exception: {marker}"
            );
        }
    }
}

/// The writing bullets of the design-system kit README, whitespace
/// normalized, from its `## Writing rules` section.
fn kit_readme_writing_bullets(readme: &str) -> Vec<String> {
    let section = readme
        .split("## Writing rules")
        .nth(1)
        .expect("kit README keeps its Writing rules section");
    let section = section.split("\n## ").next().unwrap_or(section);
    let mut bullets: Vec<String> = Vec::new();
    for line in section.lines() {
        if let Some(bullet) = line.strip_prefix("- ") {
            bullets.push(bullet.to_string());
        } else if line.starts_with("  ") && !line.trim().is_empty() {
            if let Some(last) = bullets.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        }
    }
    bullets
        .iter()
        .map(|bullet| normalized_whitespace(bullet))
        .collect()
}

/// TSK-073 AC-5. The kit README keeps its six writing bullets and names the
/// guide, and the guide carries the six bullets verbatim; the README copies
/// stay byte-identical across the two skills.
#[test]
fn kit_readme_writing_bullets_are_carried_verbatim_by_the_copy_guide() {
    let present =
        repo_text("assets/base/agents/skills/cf-present/resources/design-system/README.md");
    let portal =
        repo_text("assets/base/agents/skills/cf-docs-portal/resources/design-system/README.md");
    assert_eq!(present, portal, "kit README drifted between the skills");
    let bullets = kit_readme_writing_bullets(&present);
    assert_eq!(bullets.len(), 6, "kit README keeps six writing bullets");
    assert!(
        normalized_whitespace(&present).contains("`cf-editorial-review/references/copy-guide.md`"),
        "kit README must name the copy guide"
    );
    let guide = normalized_whitespace(&repo_text(COPY_GUIDE));
    for bullet in &bullets {
        assert!(
            guide.contains(bullet.as_str()),
            "copy guide lost the kit README bullet: {bullet}"
        );
    }
}

/// Sentences that restated the writing rules outside their homes before
/// TSK-073, and the six kit README bullets. A paraphrase that avoids these
/// words is not detected; review owns that case.
const WRITING_RULE_RESTATEMENTS: [&str; 16] = [
    "short plain sentences, bullets or a table where they carry facts better than a sentence",
    "The lead sentence says what the reader is looking at; it does not restate the caption",
    "one sentence saying what the reader takes from it; it never repeats the title",
    "the prose around the carrier is short and plain",
    "Keep language plain, direct, calm",
    "Prefer plain language, descriptive titles",
    "otherwise use calm, direct, third-person documentation language",
    "Avoid cryptic headings, invented personality",
    "Keep titles literal and findable",
    "Utility language remains neutral when no project voice is established",
    "Visuals first: a lead sentence above each figure, the acting sentences below.",
    "Short plain sentences; bullets or a table where they carry facts better than prose.",
    "No em or en dash; use a comma, colon, full stop or hyphen.",
    "Titles name the subject in words, never a bare identifier.",
    "Sentence case, except the uppercase mono kicker.",
    "No slogans, no \"not X but Y\" turns, no rhetorical triplets.",
];

/// The three homes that state the writing rules in full.
fn is_writing_rule_home(path: &str) -> bool {
    path.ends_with("cf-editorial-review/references/copy-guide.md")
        || path.ends_with("cf-editorial-review/references/editorial-smells.md")
        || path.ends_with("resources/design-system/README.md")
}

/// The skill files TSK-073 turns into pointers, relative to the shipped
/// skill trees under `assets/base`.
const COPY_GUIDE_POINTER_FILES: [&str; 12] = [
    "agents/skills/cf-present/resources/explanation-method.md",
    "agents/skills/cf-docs-portal/resources/explanation-method.md",
    "agents/skills/cf-present/resources/utility-presentation-system.md",
    "agents/skills/cf-docs-portal/resources/utility-presentation-system.md",
    "agents/skills/cf-present/resources/figure-grammar.md",
    "agents/skills/cf-docs-portal/resources/figure-grammar.md",
    "agents/skills/cf-present/SKILL.md",
    "agents/skills/cf-docs-portal/SKILL.md",
    "agents/skills/cf-present/references/document-authoring.md",
    "agents/skills/cf-editorial-review/SKILL.md",
    // TSK-184 made the writing reference the one home of the reply rule.
    "rules/writing.md",
    "agents/skills/cf-ship/SKILL.md",
];

/// TSK-073 AC-6. The writing rules are stated in full only in the copy
/// guide, the editorial smells and the kit README; every other skill file
/// named in the task points at the guide instead.
#[test]
fn writing_rules_are_stated_in_full_only_in_their_three_homes() {
    let base = repo_root().join("assets/base");
    let mut restated = Vec::new();
    for tree in ["agents/skills", "claude/skills"] {
        for path in walk_files(&base.join(tree)) {
            if path.extension().is_none_or(|extension| extension != "md") {
                continue;
            }
            let relative = rel(&base, &path);
            if is_writing_rule_home(&relative) {
                continue;
            }
            let text = normalized_whitespace(
                &std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("{relative} is readable: {error}")),
            );
            for rule in WRITING_RULE_RESTATEMENTS {
                if text.contains(&normalized_whitespace(rule)) {
                    restated.push(format!("{relative}: {rule}"));
                }
            }
        }
    }
    assert!(
        restated.is_empty(),
        "writing rules restated outside their homes; point at the copy guide instead:\n  {}",
        restated.join("\n  ")
    );
    let mut unpointed = Vec::new();
    for file in COPY_GUIDE_POINTER_FILES {
        let text = normalized_whitespace(&repo_text(&format!("assets/base/{file}")));
        if !text.contains("copy-guide.md") && !text.contains("copy guide") {
            unpointed.push(file);
        }
    }
    assert!(
        unpointed.is_empty(),
        "skill files must point at the copy guide: {unpointed:?}"
    );
}

/// TSK-073 AC-7. The writing reference, which TSK-184 made the one home of
/// the reply and figure rules, keeps the nine families, the reply rule, the
/// link rule and the pointer to the method and the guide; the editorial
/// skill names the guide and points its figure line at the nine families;
/// cf-ship step 5 and the ADR and epic templates name the guide.
#[test]
fn copy_guide_pointers_and_shape_deliverables_markers_stay_pinned() {
    let writing = normalized_whitespace(&repo_text("assets/base/rules/writing.md"));
    let mut missing = Vec::new();
    for marker in [
        "(flow, structure, layering, sequence, state, coverage, extent, derivation, graph)",
        "When a relationship carries the point, the reply or document carries a figure.",
        "Match the form to the surface",
        "Never use Mermaid.",
        "A simple answer stays simple: no figure, no headings, no recap, and a one-line answer stays one line.",
        "give the exact link a tool printed or one you verified",
        "to explain, follow the explanation method (`.agents/skills/cf-present/resources/explanation-method.md`); to write each string, follow the copy guide (`.agents/skills/cf-editorial-review/references/copy-guide.md`).",
    ] {
        if !writing.contains(marker) {
            missing.push(format!("rules/writing.md: {marker}"));
        }
    }
    for (file, markers) in [
        (
            "assets/base/agents/skills/cf-editorial-review/SKILL.md",
            &[
                "This skill, its copy guide and its contextual-smells reference are the canonical CodeFlow home",
                "Load [references/copy-guide.md](references/copy-guide.md) when writing and [references/editorial-smells.md](references/editorial-smells.md) when reviewing.",
                "in one of the nine families of the explanation method (`cf-present/resources/explanation-method.md`)",
            ][..],
        ),
        (
            "assets/base/agents/skills/cf-ship/SKILL.md",
            &["5. Apply `cf-editorial-review` and its copy guide"][..],
        ),
        (
            "assets/base/docs/decisions/template.md",
            &["The copy guide (`cf-editorial-review/references/copy-guide.md`) has the ADR shape."][..],
        ),
        (
            "docs/decisions/template.md",
            &["The copy guide (`cf-editorial-review/references/copy-guide.md`) has the ADR shape."][..],
        ),
        (
            "assets/base/pm/epic.md.tmpl",
            &["Write it by the copy guide (`cf-editorial-review/references/copy-guide.md`)."][..],
        ),
        (
            "assets/base/agents/skills/cf-present/resources/figure-grammar.md",
            &["Prose around a figure follows the written content policy (ADR-0067); the copy guide (`cf-editorial-review/references/copy-guide.md`)"][..],
        ),
    ] {
        let text = normalized_whitespace(&repo_text(file));
        for marker in markers {
            if !text.contains(marker) {
                missing.push(format!("{file}: {marker}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "copy guide pointers lost:\n  {}",
        missing.join("\n  ")
    );
}

/// The Summary section of a pull request template: from `## Summary` up to
/// the next `## ` heading.
fn pr_summary_block(text: &str) -> String {
    let start = text
        .find("## Summary\n")
        .expect("template keeps ## Summary");
    let rest = &text[start..];
    let end = rest[1..]
        .find("\n## ")
        .map_or(rest.len(), |offset| offset + 1);
    rest[..end].to_string()
}

/// TSK-073 AC-8. The Summary comment is the same block in the shipped
/// template, this repository's live template and its baseline; the live
/// template keeps its changelog impact lines and every Release impact field
/// and instruction inside that section, and the shipped template and the
/// baseline stay equal.
#[test]
fn pr_template_summary_block_is_identical_across_its_three_copies() {
    let shipped = repo_text("assets/base/ci/pull_request_template.md");
    let live = repo_text(".github/pull_request_template.md");
    let baseline = repo_text(".codeflow/.baseline/.github/pull_request_template.md");
    let summary = pr_summary_block(&shipped);
    assert!(
        summary.contains("copy guide"),
        "the Summary comment names the copy guide"
    );
    assert_eq!(
        summary,
        pr_summary_block(&live),
        "live Summary block drifted"
    );
    assert_eq!(
        summary,
        pr_summary_block(&baseline),
        "baseline Summary block drifted"
    );
    assert_eq!(shipped, baseline, "shipped template and baseline differ");
    let problems = release_impact_problems(&live);
    assert!(
        problems.is_empty(),
        "live template lost its Release impact contract:\n  {}",
        problems.join("\n  ")
    );
    // Negative control (Codex review R1, finding 1): a section gutted to its
    // heading, the Unit line and a comment that still names the release
    // script and the changelog marker fails.
    let start = live
        .find("## Release impact\n")
        .expect("live template keeps ## Release impact");
    let end = live[start..]
        .find("<!-- Conditional sections")
        .map_or(live.len(), |offset| start + offset);
    let gutted = format!(
        "{}## Release impact\n\n- Unit: `codeflow`\n\n<!-- Read by `scripts/release.py`. Put a \
         codeflow:release-impact patch|minor|major HTML marker before each entry. -->\n\n{}",
        &live[..start],
        &live[end..]
    );
    let gutted_problems = release_impact_problems(&gutted);
    for lost in [
        "- Impact:",
        "- Breaking:",
        "- Migration:",
        "add a Withdrawal field",
    ] {
        assert!(
            gutted_problems.iter().any(|problem| problem.contains(lost)),
            "a gutted Release impact section must fail on {lost}: {gutted_problems:?}"
        );
    }
    // Negative control (Codex confirm, finding 1): a sibling heading right
    // under the Release impact heading moves every field out of the section
    // the release script reads, so the fields count as missing.
    let split = live.replacen("## Release impact\n", "## Release impact\n\n## Other\n", 1);
    let split_problems = release_impact_problems(&split);
    assert!(
        split_problems
            .iter()
            .any(|problem| problem.contains("- Impact:")),
        "a heading inside Release impact must hide its fields: {split_problems:?}"
    );
}

/// The Release impact contract of this repository's live PR template:
/// every field line `scripts/release.py` reads and the instructions around
/// them, each inside the Release impact section (from its heading to the
/// next level-two heading or the conditional-sections comment, whichever
/// comes first), not anywhere in the file.
fn release_impact_problems(template: &str) -> Vec<String> {
    let Some(start) = template.find("## Release impact\n") else {
        return vec!["no ## Release impact section".to_string()];
    };
    let rest = &template[start..];
    // The section ends at whichever comes first, as `scripts/release.py`
    // stops at the next level-two heading.
    let end = [
        rest.find("<!-- Conditional sections"),
        rest[1..].find("\n## ").map(|offset| offset + 1),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(rest.len());
    let section = &rest[..end];
    let lines: BTreeSet<&str> = section.lines().map(str::trim_end).collect();
    let prose = normalized_whitespace(section);
    let mut problems = Vec::new();
    for field in [
        "- Impact: `none | patch | minor | major`",
        "- Breaking: `yes | no`",
        "- Rationale:",
        "- Migration: `none`, steps, or \"see Breaking change\"",
        "- Unit: `codeflow`",
        "- Evidence:",
    ] {
        if !lines.contains(field) {
            problems.push(format!("field line missing from the section: {field}"));
        }
    }
    for instruction in [
        "Impact is the change level a consumer sees",
        "Breaking states compatibility",
        "Choose each value; never leave the alternatives",
        "Migration is normally `none` for nonbreaking work",
        "Read by `scripts/release.py` with the fields above",
        "Breaking is yes if and only if Impact is major",
        "put a codeflow:release-impact patch|minor|major HTML marker directly before each pending changelog entry",
        "Impact must equal the highest one this PR adds or edits",
        "Declared impact cannot be below conventional commit markers",
        "add a Withdrawal field that says what was removed and why the remaining net contract permits it",
        "do not add Withdrawal to ordinary PRs",
    ] {
        if !prose.contains(instruction) {
            problems.push(format!("instruction missing from the section: {instruction}"));
        }
    }
    problems
}
