//! Read-only verification of documentation-portal evidence claims.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde::de::{DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use unicode_normalization::UnicodeNormalization;

use crate::capability::parse_capabilities;
use crate::scaffold::sha256_hex;

const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PAGES: usize = 10_000;
const MAX_ARTIFACTS: usize = 100_000;
const MAX_CLAIMED_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct PortalValidationReport {
    pub checked_pages: usize,
    pub issues: Vec<String>,
}
impl PortalValidationReport {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    schema_version: u32,
    generator: Generator,
    repository: Repository,
    config_sha256: String,
    primitive_tokens: Option<PrimitiveTokens>,
    media: Vec<Media>,
    pages: Vec<Page>,
    llms: Artifact,
    artifacts: Vec<Artifact>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdoptionState {
    schema_version: u32,
    root: String,
    starter_version: String,
    #[serde(deserialize_with = "deserialize_adoption_files")]
    files: BTreeMap<String, AdoptionFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdoptionFile {
    ownership: String,
    pristine_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Generator {
    name: String,
    version: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Repository {
    root: String,
    commit: String,
    release_version: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrimitiveTokens {
    source_path: String,
    source_sha256: String,
    output_path: String,
    output_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Media {
    source_path: String,
    source_sha256: String,
    output_path: String,
    output_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: String,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snippet {
    start_line: usize,
    end_line: usize,
    sha256: String,
}
#[derive(Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct Relationship {
    #[serde(rename = "type")]
    kind: String,
    target: String,
    #[serde(default)]
    source_id: Option<String>,
}
#[derive(Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct Backlink {
    #[serde(rename = "type")]
    kind: String,
    source_route: String,
    target: String,
    #[serde(default)]
    source_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    source_path: String,
    source_sha256: String,
    built_from_commit: String,
    route: String,
    title: String,
    status: Option<String>,
    output_markdown: String,
    output_markdown_sha256: String,
    markdown_twin: String,
    markdown_twin_sha256: String,
    stale: bool,
    searchable: bool,
    ids: Vec<String>,
    relationships: Vec<Relationship>,
    #[serde(default)]
    backlinks: Vec<Backlink>,
    snippets: Vec<Snippet>,
    #[serde(default)]
    last_good_commit: Option<String>,
    #[serde(default)]
    last_good_source_sha256: Option<String>,
}

fn deserialize_adoption_files<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, AdoptionFile>, D::Error>
where
    D: Deserializer<'de>,
{
    struct FilesVisitor;
    impl<'de> Visitor<'de> for FilesVisitor {
        type Value = BTreeMap<String, AdoptionFile>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a bounded portal adoption file map")
        }
        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut files = BTreeMap::new();
            while let Some((path, state)) = map.next_entry()? {
                if files.len() >= 256 {
                    return Err(serde::de::Error::custom(
                        "portal adoption state contains too many files",
                    ));
                }
                if files.insert(path, state).is_some() {
                    return Err(serde::de::Error::custom("duplicate portal adoption file"));
                }
            }
            Ok(files)
        }
    }
    deserializer.deserialize_map(FilesVisitor)
}

/// Verify evidence without executing project code or writing output.
#[must_use]
pub fn validate_portal(repo_root: &Path, portal_root: &Path) -> PortalValidationReport {
    let mut report = PortalValidationReport::default();
    let Some(normalized_portal_root) = normalized_relative(portal_root) else {
        report.issues.push(format!(
            "portal root is not a safe relative path: {}",
            portal_root.display()
        ));
        return report;
    };
    let adoption_path = repo_root.join(".codeflow/docs-portal.json");
    let adoption_bytes = match read_bounded_regular(&adoption_path, 64 * 1024) {
        Ok(bytes) => bytes,
        Err(error) => {
            report
                .issues
                .push(format!("portal adoption state is unreadable: {error}"));
            return report;
        }
    };
    let adoption: AdoptionState = match parse_strict_json(&adoption_bytes) {
        Ok(value) => value,
        Err(error) => {
            report
                .issues
                .push(format!("portal adoption state is invalid: {error}"));
            return report;
        }
    };
    if adoption.schema_version != 1 {
        report.issues.push(format!(
            "unsupported portal adoption schema {}",
            adoption.schema_version
        ));
    }
    if adoption.starter_version.trim().is_empty() || adoption.files.is_empty() {
        report
            .issues
            .push("portal adoption state has no pinned starter files".into());
    }
    for (file, state) in &adoption.files {
        if !safe_path_text(file)
            || !matches!(state.ownership.as_str(), "managed" | "user-owned")
            || !valid_sha256(&state.pristine_sha256)
        {
            report
                .issues
                .push(format!("portal adoption file state is invalid: {file:?}"));
        }
    }
    let requested = normalized_portal_root.to_string_lossy().replace('\\', "/");
    if !paths_equal(&adoption.root, &requested) {
        report.issues.push(format!(
            "requested portal root {requested:?} does not match adopted root {:?}",
            adoption.root
        ));
        return report;
    }
    let Some(portal) = safe_join(
        repo_root,
        &normalized_portal_root,
        "portal root",
        &mut report,
    ) else {
        return report;
    };
    let evidence_path = portal.join(".portal/generated/evidence.json");
    let bytes = match read_bounded_regular(&evidence_path, MAX_MANIFEST_BYTES) {
        Ok(bytes) => bytes,
        Err(error) => {
            report
                .issues
                .push(format!("{}: {error}", evidence_path.display()));
            return report;
        }
    };
    let evidence: Evidence = match parse_strict_json(&bytes) {
        Ok(value) => value,
        Err(error) => {
            report
                .issues
                .push(format!("evidence manifest is invalid JSON: {error}"));
            return report;
        }
    };
    if evidence.schema_version != 1 {
        report.issues.push(format!(
            "unsupported evidence schema {}",
            evidence.schema_version
        ));
    }
    if evidence.generator.name != "@codeflow/docs-portal"
        || evidence.generator.version.trim().is_empty()
    {
        report
            .issues
            .push("generator name/version is not pinned".into());
    }
    if evidence.generator.version != adoption.starter_version {
        report.issues.push(format!(
            "evidence generator version {:?} does not match adopted starter {:?}",
            evidence.generator.version, adoption.starter_version
        ));
    }
    if !valid_commit(&evidence.repository.commit) {
        report
            .issues
            .push("repository commit is not a full Git object ID".into());
    }
    match git_text_bounded(repo_root, &["rev-parse", "--verify", "HEAD^{commit}"], 1024) {
        Ok(head) if head.trim() == evidence.repository.commit => {}
        Ok(_) => report
            .issues
            .push("evidence repository commit does not match HEAD".into()),
        Err(error) => report
            .issues
            .push(format!("repository HEAD cannot be verified: {error}")),
    }
    if evidence
        .repository
        .release_version
        .as_ref()
        .is_some_and(|version| {
            version.is_empty()
                || version.len() > 128
                || !version.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
                })
        })
    {
        report
            .issues
            .push("repository release version is invalid".into());
    }
    if evidence.pages.len() > MAX_PAGES {
        report
            .issues
            .push(format!("page count exceeds {MAX_PAGES}"));
        return report;
    }
    if evidence.artifacts.len() > MAX_ARTIFACTS {
        report
            .issues
            .push(format!("artifact count exceeds {MAX_ARTIFACTS}"));
        return report;
    }
    verify_file(
        &portal,
        "portal.config.json",
        &evidence.config_sha256,
        "configuration",
        &mut report,
    );
    verify_config_contract(&portal, &evidence, &mut report);
    let portal_depth = normalized_portal_root
        .components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count();
    let expected_repository_root = std::iter::repeat_n("..", portal_depth)
        .collect::<Vec<_>>()
        .join("/");
    if evidence.repository.root != expected_repository_root {
        report.issues.push(format!("repository root claim {:?} does not resolve to the owning repository ({expected_repository_root:?})", evidence.repository.root));
        return report;
    }
    let repository = repo_root.to_path_buf();
    let authority = collect_ids(&repository, &evidence.pages, &evidence.repository.commit);
    report.issues.extend(authority.issues);
    let ids: BTreeSet<String> = authority.owners.keys().cloned().collect();
    let mut routes = BTreeSet::new();
    let mut claimed_paths = BTreeSet::new();
    let mut id_owner = BTreeMap::new();
    for page in &evidence.pages {
        report.checked_pages += 1;
        if !safe_path_text(&page.route) || !routes.insert(portable_key(&page.route)) {
            report
                .issues
                .push(format!("duplicate or unsafe route {:?}", page.route));
        }
        if page.built_from_commit != evidence.repository.commit {
            report.issues.push(format!(
                "{} was not built from the evidenced repository commit",
                page.route
            ));
        }
        if page.title.trim().is_empty() || page.title.len() > 256 {
            report
                .issues
                .push(format!("{} has an invalid title", page.route));
        }
        if page
            .status
            .as_ref()
            .is_some_and(|status| status.is_empty() || status.len() > 128)
        {
            report
                .issues
                .push(format!("{} has an invalid status", page.route));
        }
        if page.output_markdown != format!("src/content/docs/{}.md", page.route)
            || page.markdown_twin != format!("public/markdown/{}.md", page.route)
        {
            report.issues.push(format!(
                "{} output and Markdown twin do not match their canonical route",
                page.route
            ));
        }
        if page.stale {
            if !page.last_good_commit.as_deref().is_some_and(valid_commit)
                || !page
                    .last_good_source_sha256
                    .as_deref()
                    .is_some_and(valid_sha256)
            {
                report.issues.push(format!(
                    "{} stale page lacks valid last-good source provenance",
                    page.route
                ));
            } else {
                verify_last_good_source(
                    &repository,
                    &evidence.repository.commit,
                    page,
                    &mut report,
                );
            }
        } else if page.last_good_commit.is_some() || page.last_good_source_sha256.is_some() {
            report.issues.push(format!(
                "{} active page unexpectedly claims last-good source provenance",
                page.route
            ));
        }
        for claimed in [
            &page.source_path,
            &page.output_markdown,
            &page.markdown_twin,
        ] {
            if !claimed_paths.insert(portable_key(claimed)) {
                report
                    .issues
                    .push(format!("path is claimed more than once: {claimed}"));
            }
        }
        for (label, hash) in [
            ("source", &page.source_sha256),
            ("output Markdown", &page.output_markdown_sha256),
            ("Markdown twin", &page.markdown_twin_sha256),
        ] {
            if !valid_sha256(hash) {
                report
                    .issues
                    .push(format!("{} has an invalid {label} hash", page.route));
            }
        }
        verify_file(
            &repository,
            &page.source_path,
            &page.source_sha256,
            "source",
            &mut report,
        );
        verify_file(
            &portal,
            &page.output_markdown,
            &page.output_markdown_sha256,
            "output markdown",
            &mut report,
        );
        verify_file(
            &portal,
            &page.markdown_twin,
            &page.markdown_twin_sha256,
            "Markdown twin",
            &mut report,
        );
        if page.stale == page.searchable {
            report.issues.push(format!(
                "{} has inconsistent stale/searchable claims",
                page.route
            ));
        }
        for id in &page.ids {
            if !strict_id(id) || !ids.contains(id) {
                report.issues.push(format!(
                    "{} claims missing or invalid ID {id}",
                    page.source_path
                ));
            } else if authority.owners.get(id) != Some(&page.source_path) {
                report.issues.push(format!(
                    "{} claims ID {id} owned by {:?}",
                    page.source_path,
                    authority.owners.get(id)
                ));
            }
            if let Some(prior) = id_owner.insert(id.clone(), page.route.clone()) {
                report.issues.push(format!(
                    "duplicate ID ownership for {id}: {prior} and {}",
                    page.route
                ));
            }
        }
        verify_snippets(&repository, &portal, page, &mut report);
        verify_rendered_claims(&portal, &evidence, page, &mut report);
        let mut relationship_claims = BTreeSet::new();
        for relationship in &page.relationships {
            if !relationship_kind(&relationship.kind)
                || !relationship_claims.insert(relationship.clone())
            {
                report.issues.push(format!(
                    "{} has an invalid or duplicate relationship",
                    page.route
                ));
            }
        }
        let mut backlink_claims = BTreeSet::new();
        for backlink in &page.backlinks {
            if !relationship_kind(&backlink.kind)
                || !safe_path_text(&backlink.source_route)
                || !backlink_claims.insert(backlink.clone())
            {
                report.issues.push(format!(
                    "{} has an invalid or duplicate backlink",
                    page.route
                ));
            }
        }
    }
    let mut expected = BTreeMap::<String, Vec<Backlink>>::new();
    let active_routes: BTreeSet<&str> = evidence
        .pages
        .iter()
        .filter(|page| !page.stale)
        .map(|page| page.route.as_str())
        .collect();
    for page in &evidence.pages {
        for relationship in &page.relationships {
            if relationship
                .source_id
                .as_ref()
                .is_some_and(|source_id| !strict_id(source_id) || !page.ids.contains(source_id))
            {
                report.issues.push(format!(
                    "{} relationship has an invalid source identity {:?}",
                    page.route, relationship.source_id
                ));
            }
            if !strict_id(&relationship.target) || !ids.contains(&relationship.target) {
                report.issues.push(format!(
                    "{} relationship targets missing or invalid ID {}",
                    page.route, relationship.target
                ));
            }
            if !page.stale {
                if let Some(target_route) = id_owner.get(&relationship.target) {
                    if active_routes.contains(target_route.as_str()) {
                        expected
                            .entry(target_route.clone())
                            .or_default()
                            .push(Backlink {
                                kind: relationship.kind.clone(),
                                source_route: page.route.clone(),
                                target: relationship.target.clone(),
                                source_id: relationship.source_id.clone(),
                            });
                    }
                }
            }
        }
    }
    for page in &evidence.pages {
        let mut actual = page.backlinks.clone();
        actual.sort();
        let mut wanted = expected.remove(&page.route).unwrap_or_default();
        wanted.sort();
        if !page.stale && actual != wanted {
            report.issues.push(format!(
                "{} backlinks do not match declared relationships",
                page.route
            ));
        }
    }
    verify_file(
        &portal,
        &evidence.llms.path,
        &evidence.llms.sha256,
        "llms.txt",
        &mut report,
    );
    if evidence.llms.path != "public/llms.txt" {
        report
            .issues
            .push("llms.txt claim must target public/llms.txt".into());
    }
    if let Ok(llms) = read_bounded_text(&portal.join("public/llms.txt"), MAX_CLAIMED_FILE_BYTES) {
        let actual: Vec<&str> = llms
            .lines()
            .filter(|line| line.starts_with("- ["))
            .collect();
        let expected: Vec<String> = evidence
            .pages
            .iter()
            .filter(|page| !page.stale)
            .map(|page| {
                format!(
                    "- [{}](./markdown/{}.md) — {}",
                    page.route, page.route, page.source_path
                )
            })
            .collect();
        if actual != expected.iter().map(String::as_str).collect::<Vec<_>>() {
            report
                .issues
                .push("llms.txt entries do not exactly match active evidenced pages".into());
        }
    }
    verify_primitive_tokens(&repository, &portal, &evidence, &mut report);
    verify_media(&repository, &portal, &evidence.media, &mut report);
    let mut artifact_paths = BTreeSet::new();
    for artifact in &evidence.artifacts {
        if !artifact.path.starts_with("dist/") || !safe_path_text(&artifact.path) {
            report.issues.push(format!(
                "built artifact path must stay under dist/: {}",
                artifact.path
            ));
            continue;
        }
        if !artifact_paths.insert(portable_key(&artifact.path)) {
            report
                .issues
                .push(format!("duplicate built artifact path: {}", artifact.path));
        }
        verify_file(
            &portal,
            &artifact.path,
            &artifact.sha256,
            "built artifact",
            &mut report,
        );
    }
    let actual_artifacts = collect_dist_artifacts(&portal, &mut report);
    for actual in actual_artifacts.difference(&artifact_paths) {
        report
            .issues
            .push(format!("built artifact is unclaimed: {actual}"));
    }
    for claimed in artifact_paths.difference(&actual_artifacts) {
        report
            .issues
            .push(format!("claimed built artifact is absent: {claimed}"));
    }
    for page in &evidence.pages {
        let built = if page.route == "index" {
            "dist/index.html".to_string()
        } else {
            format!("dist/{}/index.html", page.route)
        };
        if !artifact_paths.contains(&portable_key(&built)) {
            report
                .issues
                .push(format!("page is missing its built output: {}", page.route));
        }
        if let Ok(html) = read_bounded_text(&portal.join(&built), MAX_CLAIMED_FILE_BYTES) {
            let search_marker = format!(
                "data-pagefind-body data-codeflow-search-root=\"{}\"",
                page.route
            );
            if page.searchable && !html.contains(&search_marker) {
                report.issues.push(format!(
                    "searchable page lacks built Pagefind body evidence: {}",
                    page.route
                ));
            }
            if page.stale && html.contains("data-codeflow-search-root=") {
                report.issues.push(format!(
                    "stale page remains included in built Pagefind content: {}",
                    page.route
                ));
            }
        }
        if page.stale {
            match read_bounded_text(&portal.join(&page.output_markdown), MAX_CLAIMED_FILE_BYTES) {
                Ok(markdown)
                    if markdown.contains("data-pagefind-ignore=\"all\"")
                        && markdown.contains("Stale rendering") => {}
                _ => report.issues.push(format!(
                    "stale page lacks visible stale and search-exclusion markers: {}",
                    page.route
                )),
            }
        }
    }
    report
}

fn verify_config_contract(portal: &Path, evidence: &Evidence, report: &mut PortalValidationReport) {
    let path = portal.join("portal.config.json");
    let config: serde_json::Value = match read_bounded_regular(&path, 64 * 1024)
        .ok()
        .and_then(|bytes| parse_strict_json(&bytes).ok())
    {
        Some(config) => config,
        None => {
            report
                .issues
                .push("portal configuration contract is unreadable or invalid".into());
            return;
        }
    };
    let configured_release = config
        .get("release_version")
        .and_then(serde_json::Value::as_str);
    if configured_release != evidence.repository.release_version.as_deref()
        || (!config
            .get("release_version")
            .is_some_and(|value| value.is_null())
            && configured_release.is_none())
    {
        report
            .issues
            .push("repository release version does not match portal configuration".into());
    }
    let configured_tokens = config
        .get("primitive_tokens")
        .and_then(serde_json::Value::as_str);
    if configured_tokens
        != evidence
            .primitive_tokens
            .as_ref()
            .map(|value| value.source_path.as_str())
        || (!config
            .get("primitive_tokens")
            .is_some_and(|value| value.is_null())
            && configured_tokens.is_none())
    {
        report
            .issues
            .push("primitive-token evidence does not match portal configuration".into());
    }
}

fn verify_primitive_tokens(
    repository: &Path,
    portal: &Path,
    evidence: &Evidence,
    report: &mut PortalValidationReport,
) {
    let Some(tokens) = &evidence.primitive_tokens else {
        return;
    };
    if tokens.output_path != ".portal/generated/project-tokens.css" {
        report
            .issues
            .push("primitive-token output path is not canonical".into());
    }
    for (label, hash) in [
        ("primitive-token source", &tokens.source_sha256),
        ("primitive-token output", &tokens.output_sha256),
    ] {
        if !valid_sha256(hash) {
            report.issues.push(format!("{label} hash is invalid"));
        }
    }
    verify_file(
        repository,
        &tokens.source_path,
        &tokens.source_sha256,
        "primitive-token source",
        report,
    );
    verify_file(
        portal,
        &tokens.output_path,
        &tokens.output_sha256,
        "primitive-token output",
        report,
    );
}

fn verify_media(
    repository: &Path,
    portal: &Path,
    media: &[Media],
    report: &mut PortalValidationReport,
) {
    if media.len() > 1_000 {
        report
            .issues
            .push("referenced media count exceeds 1000".into());
        return;
    }
    let mut sources = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    let mut total_bytes = 0_u64;
    for item in media {
        let extension = Path::new(&item.source_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(
            extension.as_str(),
            "avif" | "gif" | "jpeg" | "jpg" | "png" | "webp"
        ) {
            report.issues.push(format!(
                "referenced media has an unsupported type: {}",
                item.source_path
            ));
        }
        let name = Path::new(&item.source_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let expected = format!(
            "public/media/{}-{name}",
            &sha256_hex(item.source_path.as_bytes())[..16]
        );
        if item.output_path != expected {
            report.issues.push(format!(
                "referenced media output is not canonical: {}",
                item.output_path
            ));
        }
        if !sources.insert(portable_key(&item.source_path))
            || !outputs.insert(portable_key(&item.output_path))
        {
            report
                .issues
                .push("referenced media contains duplicate portable paths".into());
        }
        if !valid_sha256(&item.source_sha256)
            || !valid_sha256(&item.output_sha256)
            || item.source_sha256 != item.output_sha256
        {
            report.issues.push(format!(
                "referenced media hashes are invalid or differ: {}",
                item.source_path
            ));
        }
        verify_file(
            repository,
            &item.source_path,
            &item.source_sha256,
            "referenced media source",
            report,
        );
        verify_file(
            portal,
            &item.output_path,
            &item.output_sha256,
            "referenced media output",
            report,
        );
        if let Some(path) = safe_join(
            repository,
            Path::new(&item.source_path),
            "referenced media source",
            report,
        ) {
            match read_bounded_regular(&path, 8 * 1024 * 1024) {
                Ok(bytes) => {
                    total_bytes = total_bytes.saturating_add(bytes.len() as u64);
                    if !raster_signature_matches(&extension, &bytes) {
                        report.issues.push(format!(
                            "referenced media bytes do not match the claimed type: {}",
                            item.source_path
                        ));
                    }
                }
                Err(error) => report.issues.push(format!(
                    "referenced media source is unreadable or exceeds 8388608 bytes: {}: {error}",
                    item.source_path
                )),
            }
        }
    }
    if total_bytes > 64 * 1024 * 1024 {
        report
            .issues
            .push("referenced media corpus exceeds 67108864 bytes".into());
    }
}

fn raster_signature_matches(extension: &str, bytes: &[u8]) -> bool {
    match extension {
        "png" => bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "jpg" | "jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "webp" => bytes.get(0..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "avif" => {
            bytes.get(4..8) == Some(b"ftyp")
                && matches!(bytes.get(8..12), Some(b"avif") | Some(b"avis"))
        }
        _ => false,
    }
}

fn collect_dist_artifacts(portal: &Path, report: &mut PortalValidationReport) -> BTreeSet<String> {
    fn visit(
        root: &Path,
        relative: &Path,
        depth: usize,
        total_bytes: &mut u64,
        paths: &mut BTreeSet<String>,
        report: &mut PortalValidationReport,
    ) {
        if depth > 32 {
            report
                .issues
                .push("built artifact tree exceeds 32 directory levels".into());
            return;
        }
        let directory = root.join(relative);
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                report.issues.push(format!(
                    "built artifact directory is unreadable {}: {error}",
                    directory.display()
                ));
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    report
                        .issues
                        .push(format!("built artifact entry is unreadable: {error}"));
                    continue;
                }
            };
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(error) => {
                    report.issues.push(format!(
                        "built artifact metadata is unreadable {}: {error}",
                        entry.path().display()
                    ));
                    continue;
                }
            };
            let child = relative.join(entry.file_name());
            if kind.is_symlink() {
                report.issues.push(format!(
                    "built artifact symlink is refused: {}",
                    child.display()
                ));
            } else if kind.is_dir() {
                visit(root, &child, depth + 1, total_bytes, paths, report);
            } else if kind.is_file() {
                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        report.issues.push(format!(
                            "built artifact metadata is unreadable {}: {error}",
                            child.display()
                        ));
                        continue;
                    }
                };
                *total_bytes = total_bytes.saturating_add(metadata.len());
                if *total_bytes > MAX_TOTAL_ARTIFACT_BYTES {
                    report.issues.push(format!(
                        "built artifact corpus exceeds {MAX_TOTAL_ARTIFACT_BYTES} bytes"
                    ));
                    return;
                }
                if paths.len() >= MAX_ARTIFACTS {
                    report
                        .issues
                        .push(format!("artifact count exceeds {MAX_ARTIFACTS}"));
                    return;
                }
                let artifact = portable_key(&format!(
                    "dist/{}",
                    child.to_string_lossy().replace('\\', "/")
                ));
                if !paths.insert(artifact.clone()) {
                    report.issues.push(format!(
                        "built artifact path collides case-insensitively: {artifact}"
                    ));
                }
            } else {
                report.issues.push(format!(
                    "non-regular built artifact is refused: {}",
                    child.display()
                ));
            }
        }
    }
    let mut paths = BTreeSet::new();
    let mut total_bytes = 0;
    let dist = portal.join("dist");
    match std::fs::symlink_metadata(&dist) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            report.issues.push(format!(
                "built artifact root is not a regular directory: {}",
                dist.display()
            ));
            return paths;
        }
        Err(error) => {
            report.issues.push(format!(
                "built artifact root is unreadable {}: {error}",
                dist.display()
            ));
            return paths;
        }
        Ok(_) => {}
    }
    visit(
        &dist,
        Path::new(""),
        0,
        &mut total_bytes,
        &mut paths,
        report,
    );
    paths
}

fn verify_snippets(
    repository: &Path,
    portal: &Path,
    page: &Page,
    report: &mut PortalValidationReport,
) {
    let Some(path) = safe_join(
        repository,
        Path::new(&page.source_path),
        "snippet source",
        report,
    ) else {
        return;
    };
    let text = match read_bounded_text(&path, MAX_SOURCE_BYTES) {
        Ok(text) => text,
        Err(error) => {
            report.issues.push(format!(
                "{} snippet source is not valid UTF-8 or readable: {error}",
                page.source_path
            ));
            return;
        }
    };
    let lines: Vec<&str> = text.split('\n').collect();
    for snippet in &page.snippets {
        if snippet.start_line == 0
            || snippet.end_line < snippet.start_line
            || snippet.end_line > lines.len()
        {
            report.issues.push(format!(
                "{} has invalid snippet range {}..{}",
                page.source_path, snippet.start_line, snippet.end_line
            ));
            continue;
        }
        let value = lines[snippet.start_line - 1..snippet.end_line].join("\n");
        if sha256_hex(value.as_bytes()) != snippet.sha256 {
            report.issues.push(format!(
                "{} snippet {}..{} hash mismatch",
                page.source_path, snippet.start_line, snippet.end_line
            ));
        }
        let marker = format!(
            "<!-- codeflow-source-snippet sha256={} lines={}-{} -->",
            snippet.sha256, snippet.start_line, snippet.end_line
        );
        match read_bounded_text(&portal.join(&page.output_markdown), MAX_CLAIMED_FILE_BYTES) {
            Ok(output) if output.lines().take(24).any(|line| line == marker) => {}
            _ => report.issues.push(format!(
                "{} snippet claim is not anchored in rendered Markdown",
                page.route
            )),
        }
    }
}

fn verify_rendered_claims(
    portal: &Path,
    evidence: &Evidence,
    page: &Page,
    report: &mut PortalValidationReport,
) {
    let Ok(output) = read_bounded_text(&portal.join(&page.output_markdown), MAX_CLAIMED_FILE_BYTES)
    else {
        return;
    };
    let release = evidence
        .repository
        .release_version
        .as_deref()
        .unwrap_or("none");
    let marker = format!(
        "<!-- codeflow-page-provenance source_sha256={} built_from_commit={} portal_version={} release_version={} -->",
        page.source_sha256,
        evidence.repository.commit,
        evidence.generator.version,
        release
    );
    if !output.lines().take(20).any(|line| line == marker) {
        report.issues.push(format!(
            "{} lacks its exact rendered provenance marker",
            page.route
        ));
    }
    if page.stale {
        let last_good_marker = format!(
            "<!-- codeflow-last-good-provenance source_sha256={} built_from_commit={} -->",
            page.last_good_source_sha256.as_deref().unwrap_or_default(),
            page.last_good_commit.as_deref().unwrap_or_default()
        );
        if !output.lines().take(20).any(|line| line == last_good_marker) {
            report.issues.push(format!(
                "{} lacks its exact last-good source marker",
                page.route
            ));
        }
    }
    if !output.contains(&format!("<code>{}</code>", page.source_path))
        || !output.contains(&format!("<code>{}</code>", evidence.repository.commit))
        || evidence
            .repository
            .release_version
            .as_ref()
            .is_some_and(|version| !output.contains(&format!("<code>{version}</code>")))
    {
        report.issues.push(format!(
            "{} lacks visible source, commit, or release provenance",
            page.route
        ));
    }
    for relationship in &page.relationships {
        if !output.contains(&format!("[{}](", relationship.target)) {
            report.issues.push(format!(
                "{} does not render relationship target {}",
                page.route, relationship.target
            ));
        }
    }
    for backlink in &page.backlinks {
        let label = backlink
            .source_id
            .as_ref()
            .unwrap_or(&backlink.source_route);
        if !output.contains(&format!("[{label}](")) {
            report.issues.push(format!(
                "{} does not render inverse link from {label}",
                page.route
            ));
        }
    }
}

fn verify_last_good_source(
    repository: &Path,
    current_commit: &str,
    page: &Page,
    report: &mut PortalValidationReport,
) {
    let Some(ancestor) = page.last_good_commit.as_deref() else {
        return;
    };
    let status = Command::new("git")
        .args(["-C"])
        .arg(repository)
        .args(["merge-base", "--is-ancestor", ancestor, current_commit])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !status.is_ok_and(|status| status.success()) {
        report.issues.push(format!(
            "{} last-good commit is not an ancestor of the evidenced commit",
            page.route
        ));
        return;
    }
    match git_blob_bounded(repository, ancestor, &page.source_path, MAX_SOURCE_BYTES) {
        Ok(bytes)
            if page
                .last_good_source_sha256
                .as_ref()
                .is_some_and(|expected| sha256_hex(&bytes) == *expected) => {}
        Ok(_) => report.issues.push(format!(
            "{} last-good Git blob hash does not match evidence",
            page.route
        )),
        Err(error) => report.issues.push(format!(
            "{} last-good Git blob cannot be verified: {error}",
            page.route
        )),
    }
}

fn verify_file(
    root: &Path,
    relative: &str,
    expected: &str,
    label: &str,
    report: &mut PortalValidationReport,
) {
    if !valid_sha256(expected) {
        report
            .issues
            .push(format!("{label} has an invalid SHA-256 claim: {relative}"));
        return;
    }
    let Some(path) = safe_join(root, Path::new(relative), label, report) else {
        return;
    };
    match read_bounded_regular(&path, MAX_CLAIMED_FILE_BYTES) {
        Ok(bytes) if sha256_hex(&bytes) == expected => {}
        Ok(_) => report
            .issues
            .push(format!("{label} hash mismatch: {relative}")),
        Err(error) => report
            .issues
            .push(format!("{label} unreadable {relative}: {error}")),
    }
}

fn safe_join(
    root: &Path,
    relative: &Path,
    label: &str,
    report: &mut PortalValidationReport,
) -> Option<PathBuf> {
    let text = relative.to_string_lossy();
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || text.contains('\\')
        || text
            .split('/')
            .next()
            .is_some_and(|part| part.ends_with(':'))
        || !text.split('/').all(portable_segment)
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        report.issues.push(format!(
            "{label} is not a safe relative path: {}",
            relative.display()
        ));
        return None;
    }
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if let Component::Normal(part) = component {
            current.push(part);
        }
        if std::fs::symlink_metadata(&current)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            report.issues.push(format!(
                "{label} traverses a symlink: {}",
                current.display()
            ));
            return None;
        }
    }
    Some(current)
}

fn safe_path_text(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !value.contains('\\')
        && !path.is_absolute()
        && !value
            .split('/')
            .next()
            .is_some_and(|part| part.ends_with(':'))
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && value.split('/').all(portable_segment)
}

fn portable_segment(segment: &str) -> bool {
    let normalized: String = segment.nfc().collect();
    let stem = segment.split('.').next().unwrap_or_default().to_uppercase();
    normalized == segment
        && segment.len() <= 255
        && segment.encode_utf16().count() <= 255
        && !segment
            .chars()
            .any(|character| character.is_control() || "<>:\"|?*".contains(character))
        && !segment.ends_with([' ', '.'])
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

fn portable_key(value: &str) -> String {
    value.nfc().collect::<String>().to_lowercase()
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn relationship_kind(value: &str) -> bool {
    matches!(
        value,
        "epic" | "spec" | "depends_on" | "capability" | "decision" | "related" | "superseded_by"
    )
}

fn git_text_bounded(root: &Path, args: &[&str], maximum_bytes: u64) -> std::io::Result<String> {
    String::from_utf8(git_output_bounded(root, args, maximum_bytes)?)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))
}

fn git_blob_bounded(
    root: &Path,
    commit: &str,
    source_path: &str,
    maximum_bytes: u64,
) -> std::io::Result<Vec<u8>> {
    if !valid_commit(commit) || !safe_path_text(source_path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid Git blob identity",
        ));
    }
    let object = format!("{commit}:{source_path}");
    git_output_bounded(root, &["cat-file", "blob", &object], maximum_bytes)
}

fn git_output_bounded(root: &Path, args: &[&str], maximum_bytes: u64) -> std::io::Result<Vec<u8>> {
    let mut child = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("Git stdout was not captured"))?
        .take(maximum_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let status = child.wait()?;
    if !status.success() {
        return Err(std::io::Error::other("Git object lookup failed"));
    }
    if bytes.len() as u64 > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Git output exceeds its byte limit",
        ));
    }
    Ok(bytes)
}

fn read_bounded_regular(path: &Path, maximum_bytes: u64) -> std::io::Result<Vec<u8>> {
    read_bounded_regular_with_hook(path, maximum_bytes, || Ok(()))
}

fn read_bounded_regular_with_hook(
    path: &Path,
    maximum_bytes: u64,
    after_open: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<Vec<u8>> {
    let before = std::fs::symlink_metadata(path)?;
    if before.file_type().is_symlink() || !before.is_file() || before.len() > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file is not regular or exceeds its byte limit",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || !same_file_identity(&before, &opened) || opened.len() > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file changed identity or type while opening",
        ));
    }
    after_open()?;
    let mut bytes = Vec::with_capacity(usize::try_from(opened.len()).unwrap_or(0));
    file.take(maximum_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file grew beyond its byte limit",
        ));
    }
    let after = std::fs::symlink_metadata(path)?;
    if after.file_type().is_symlink()
        || !after.is_file()
        || !same_file_identity(&opened, &after)
        || opened.len() != bytes.len() as u64
        || opened.len() != after.len()
        || opened.modified()? != after.modified()?
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file changed while it was being read",
        ));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn same_file_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(windows)]
fn same_file_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    left.volume_serial_number() == right.volume_serial_number()
        && left.file_index() == right.file_index()
}

fn read_bounded_text(path: &Path, maximum_bytes: u64) -> std::io::Result<String> {
    String::from_utf8(read_bounded_regular(path, maximum_bytes)?)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))
}

fn normalized_relative(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty() || path.is_absolute() || path.to_string_lossy().contains('\\') {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            _ => return None,
        }
    }
    let text = normalized.to_string_lossy().replace('\\', "/");
    (!normalized.as_os_str().is_empty() && text.split('/').all(portable_segment))
        .then_some(normalized)
}

fn paths_equal(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

struct StrictValue(serde_json::Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("JSON without duplicate object keys")
            }
            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|number| StrictValue(number.into()))
                    .ok_or_else(|| E::custom("non-finite number"))
            }
            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(serde_json::Value::Null))
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(serde_json::Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<StrictValue>()? {
                    values.push(value.0);
                }
                Ok(StrictValue(values.into()))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, StrictValue>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate JSON key {key:?}"
                        )));
                    }
                    values.insert(key, value.0);
                }
                Ok(StrictValue(values.into()))
            }
        }
        deserializer.deserialize_any(StrictVisitor)
    }
}

fn parse_strict_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    let value = serde_json::from_slice::<StrictValue>(bytes)?.0;
    serde_json::from_value(value)
}

struct AuthorityIds {
    owners: BTreeMap<String, String>,
    issues: Vec<String>,
}

fn collect_ids(root: &Path, pages: &[Page], current_commit: &str) -> AuthorityIds {
    let mut authority = AuthorityIds {
        owners: BTreeMap::new(),
        issues: Vec::new(),
    };
    fn add_id(id: String, owner: &Path, root: &Path, authority: &mut AuthorityIds) {
        let relative = owner
            .strip_prefix(root)
            .unwrap_or(owner)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(prior) = authority.owners.insert(id.clone(), relative.clone()) {
            authority.issues.push(format!(
                "authoritative ID {id} is declared more than once: {prior} and {relative}"
            ));
        }
    }
    fn frontmatter_id(text: &str) -> Option<String> {
        let mut lines = text.lines();
        if lines.next()? != "---" {
            return None;
        }
        for line in lines {
            if line == "---" {
                break;
            }
            let trimmed = line.trim().trim_start_matches('-').trim();
            if let Some(value) = trimmed.strip_prefix("id:").map(str::trim) {
                let value = value.trim_matches(['\'', '"']);
                if strict_id(value) {
                    return Some(value.into());
                }
            }
        }
        None
    }
    let mut total_bytes = 0_u64;
    let mut seen_sources = BTreeSet::new();
    for page in pages {
        if !seen_sources.insert(page.source_path.as_str()) || !safe_path_text(&page.source_path) {
            continue;
        }
        let mut path_report = PortalValidationReport::default();
        if safe_join(
            root,
            Path::new(&page.source_path),
            "authoritative source",
            &mut path_report,
        )
        .is_none()
        {
            authority.issues.extend(path_report.issues);
            continue;
        }
        let source_commit = if page.stale {
            page.last_good_commit.as_deref().unwrap_or_default()
        } else {
            current_commit
        };
        let expected_hash = if page.stale {
            page.last_good_source_sha256.as_deref().unwrap_or_default()
        } else {
            &page.source_sha256
        };
        let bytes = match git_blob_bounded(root, source_commit, &page.source_path, MAX_SOURCE_BYTES)
        {
            Ok(bytes) if sha256_hex(&bytes) == expected_hash => bytes,
            Ok(_) => {
                authority.issues.push(format!(
                    "authoritative Git source hash mismatch: {}",
                    page.source_path
                ));
                continue;
            }
            Err(error) => {
                authority.issues.push(format!(
                    "authoritative Git source is unreadable {}: {error}",
                    page.source_path
                ));
                continue;
            }
        };
        total_bytes = total_bytes.saturating_add(bytes.len() as u64);
        if total_bytes > MAX_TOTAL_SOURCE_BYTES {
            authority.issues.push(format!(
                "authoritative source corpus exceeds {MAX_TOTAL_SOURCE_BYTES} bytes"
            ));
            break;
        }
        let text = match String::from_utf8(bytes) {
            Ok(text) => text
                .strip_prefix('\u{feff}')
                .unwrap_or(&text)
                .replace("\r\n", "\n")
                .replace('\r', "\n"),
            Err(error) => {
                authority.issues.push(format!(
                    "authoritative source is not valid UTF-8 or readable {}: {error}",
                    page.source_path
                ));
                continue;
            }
        };
        if page.source_path == "docs/capabilities.md" {
            let (entries, parse_issues) = parse_capabilities(&text);
            for issue in parse_issues {
                authority.issues.push(format!(
                    "docs/capabilities.md:{}: {}",
                    issue.line, issue.message
                ));
            }
            for entry in entries {
                if strict_id(&entry.id) {
                    add_id(entry.id, Path::new(&page.source_path), root, &mut authority);
                }
            }
            continue;
        }
        if let Some(id) = frontmatter_id(&text) {
            add_id(id, Path::new(&page.source_path), root, &mut authority);
            continue;
        }
        if let Some(stem) = Path::new(&page.source_path)
            .file_stem()
            .and_then(|value| value.to_str())
        {
            let id = stem.to_ascii_uppercase();
            if strict_id(&id) {
                add_id(id, Path::new(&page.source_path), root, &mut authority);
            }
        }
    }
    authority
}

fn strict_id(value: &str) -> bool {
    let mut parts = value.split('-');
    let Some(prefix) = parts.next() else {
        return false;
    };
    let Some(number) = parts.next() else {
        return false;
    };
    let suffix = parts.next();
    parts.next().is_none()
        && matches!(prefix, "ADR" | "EPC" | "SPC" | "TSK" | "CAP")
        && number.len() >= 3
        && number.bytes().all(|b| b.is_ascii_digit())
        && suffix.is_none_or(|part| {
            prefix == "TSK" && part.len() >= 3 && part.bytes().all(|b| b.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(source_path: &str) -> Page {
        Page {
            source_path: source_path.into(),
            source_sha256: "0".repeat(64),
            built_from_commit: "0".repeat(40),
            route: "reference/page".into(),
            title: "Page".into(),
            status: None,
            output_markdown: "src/content/docs/reference/page.md".into(),
            output_markdown_sha256: "0".repeat(64),
            markdown_twin: "public/markdown/reference/page.md".into(),
            markdown_twin_sha256: "0".repeat(64),
            stale: false,
            searchable: true,
            ids: Vec::new(),
            relationships: Vec::new(),
            backlinks: Vec::new(),
            snippets: Vec::new(),
            last_good_commit: None,
            last_good_source_sha256: None,
        }
    }

    #[test]
    fn strict_ids_and_paths_fail_closed() {
        assert!(strict_id("TSK-009"));
        assert!(strict_id("TSK-003-001"));
        assert!(!strict_id("TSK-nine"));
        let mut report = PortalValidationReport::default();
        assert!(safe_join(Path::new("/tmp"), Path::new("../etc"), "test", &mut report).is_none());
        assert_eq!(report.issues.len(), 1);
        for path in [r"C:\portal", r"\\server\share", "a/../b", "/portal"] {
            assert!(!safe_path_text(path), "accepted {path}");
        }
        assert!(!safe_path_text(&format!("docs/{}", "é".repeat(128))));
        assert!(safe_path_text(&format!("docs/{}", "é".repeat(127))));
    }

    #[test]
    fn strict_json_rejects_duplicate_keys_at_any_depth() {
        assert!(parse_strict_json::<serde_json::Value>(
            br#"{"schema_version":1,"schema_version":1}"#
        )
        .is_err());
        assert!(
            parse_strict_json::<serde_json::Value>(br#"{"outer":{"path":"a","path":"b"}}"#)
                .is_err()
        );
    }

    #[test]
    fn strict_json_rejects_unknown_adoption_fields() {
        let value = br#"{
            "schema_version": 1,
            "root": "portal",
            "starter_version": "1.0.0",
            "files": {"package.json": {"ownership": "managed", "pristine_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}},
            "unexpected": true
        }"#;
        assert!(parse_strict_json::<AdoptionState>(value).is_err());
    }

    #[test]
    fn raster_media_requires_bytes_matching_the_claimed_type() {
        assert!(raster_signature_matches(
            "png",
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        ));
        assert!(raster_signature_matches("jpg", &[0xff, 0xd8, 0xff]));
        assert!(raster_signature_matches("webp", b"RIFFxxxxWEBP"));
        assert!(!raster_signature_matches("png", b"<svg></svg>"));
        assert!(!raster_signature_matches("gif", b"not-a-gif"));
    }

    #[test]
    fn authority_is_limited_to_evidenced_sources_and_canonical_capability_records() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("docs")).unwrap();
        std::fs::write(
            temp.path().join("docs/capabilities.md"),
            "~~~yaml\nid: CAP-101\nname: real\narea: core\nstatus: planned\n~~~\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("docs/example.md"),
            "~~~yaml\nid: CAP-999\nname: example\narea: docs\nstatus: planned\n~~~\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("docs/unclaimed.md"),
            "---\nid: TSK-999\n---\n",
        )
        .unwrap();
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "portal-tests@codeflow.invalid"][..],
            &["config", "user.name", "Portal tests"][..],
            &["add", "docs"][..],
            &["commit", "-q", "-m", "fixture"][..],
        ] {
            assert!(Command::new("git")
                .args(["-C"])
                .arg(temp.path())
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        let commit = git_text_bounded(temp.path(), &["rev-parse", "HEAD"], 1024)
            .unwrap()
            .trim()
            .to_owned();
        let mut capabilities = page("docs/capabilities.md");
        capabilities.source_sha256 =
            sha256_hex(&std::fs::read(temp.path().join("docs/capabilities.md")).unwrap());
        let mut example = page("docs/example.md");
        example.source_sha256 =
            sha256_hex(&std::fs::read(temp.path().join("docs/example.md")).unwrap());
        let pages = vec![capabilities, example];
        let authority = collect_ids(temp.path(), &pages, &commit);
        assert!(authority.issues.is_empty(), "{:?}", authority.issues);
        assert_eq!(
            authority.owners.get("CAP-101").map(String::as_str),
            Some("docs/capabilities.md")
        );
        assert!(!authority.owners.contains_key("CAP-999"));
        assert!(!authority.owners.contains_key("TSK-999"));
    }

    #[test]
    fn built_artifact_inventory_is_complete_and_rejects_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("dist/nested")).unwrap();
        std::fs::write(temp.path().join("dist/index.html"), "index").unwrap();
        std::fs::write(temp.path().join("dist/nested/page.html"), "page").unwrap();
        let mut report = PortalValidationReport::default();
        let paths = collect_dist_artifacts(temp.path(), &mut report);
        assert!(report.is_clean(), "{:?}", report.issues);
        assert_eq!(
            paths,
            BTreeSet::from(["dist/index.html".into(), "dist/nested/page.html".into()])
        );

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                temp.path().join("dist/index.html"),
                temp.path().join("dist/nested/link.html"),
            )
            .unwrap();
            let mut report = PortalValidationReport::default();
            collect_dist_artifacts(temp.path(), &mut report);
            assert!(report
                .issues
                .iter()
                .any(|issue| issue.contains("symlink is refused")));
        }
    }

    #[test]
    fn bounded_validation_reads_detect_growth_and_path_swaps() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("evidence.json");
        std::fs::write(&file, "stable").unwrap();
        assert!(read_bounded_regular_with_hook(&file, 32, || {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .open(&file)?
                .write_all(b"-growth")
        })
        .is_err());

        std::fs::write(&file, "stable").unwrap();
        let moved = temp.path().join("moved.json");
        assert!(read_bounded_regular_with_hook(&file, 32, || {
            std::fs::rename(&file, &moved)?;
            std::fs::write(&file, "replacement")
        })
        .is_err());
    }

    #[test]
    fn last_good_source_requires_an_ancestor_git_blob_and_exact_hash() {
        fn git(root: &Path, args: &[&str]) -> String {
            let output = Command::new("git")
                .args(["-C"])
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap()
        }
        fn initialize(root: &Path, content: &str) -> String {
            std::fs::create_dir_all(root.join("docs")).unwrap();
            std::fs::write(root.join("docs/guide.md"), content).unwrap();
            git(root, &["init", "-q"]);
            git(
                root,
                &["config", "user.email", "portal-tests@codeflow.invalid"],
            );
            git(root, &["config", "user.name", "Portal tests"]);
            git(root, &["add", "docs/guide.md"]);
            git(root, &["commit", "-q", "-m", "fixture"]);
            git(root, &["rev-parse", "HEAD"]).trim().to_owned()
        }

        let repository = tempfile::tempdir().unwrap();
        let ancestor = initialize(repository.path(), "---\nid: TSK-0102\n---\n# Valid\n");
        let ancestor_bytes = std::fs::read(repository.path().join("docs/guide.md")).unwrap();
        std::fs::write(repository.path().join("docs/guide.md"), "---\nbroken\n").unwrap();
        git(repository.path(), &["add", "docs/guide.md"]);
        git(repository.path(), &["commit", "-q", "-m", "break source"]);
        let current = git(repository.path(), &["rev-parse", "HEAD"])
            .trim()
            .to_owned();

        let mut stale = page("docs/guide.md");
        stale.stale = true;
        stale.searchable = false;
        stale.last_good_commit = Some(ancestor);
        stale.last_good_source_sha256 = Some(sha256_hex(&ancestor_bytes));
        let mut report = PortalValidationReport::default();
        verify_last_good_source(repository.path(), &current, &stale, &mut report);
        assert!(report.is_clean(), "{:?}", report.issues);

        let unrelated = tempfile::tempdir().unwrap();
        stale.last_good_commit = Some(initialize(unrelated.path(), "# Unrelated\n"));
        let mut report = PortalValidationReport::default();
        verify_last_good_source(repository.path(), &current, &stale, &mut report);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.contains("not an ancestor")));

        stale.last_good_commit = Some(
            git(repository.path(), &["rev-list", "--max-parents=0", "HEAD"])
                .trim()
                .to_owned(),
        );
        stale.last_good_source_sha256 = Some("f".repeat(64));
        let mut report = PortalValidationReport::default();
        verify_last_good_source(repository.path(), &current, &stale, &mut report);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.contains("hash does not match")));
    }

    #[cfg(unix)]
    #[test]
    fn authority_and_artifact_roots_refuse_parent_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("task.md"), "---\nid: TSK-777\n---\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("docs")).unwrap();
        let authority = collect_ids(temp.path(), &[page("docs/task.md")], &"0".repeat(40));
        assert!(!authority.owners.contains_key("TSK-777"));
        assert!(authority
            .issues
            .iter()
            .any(|issue| issue.contains("traverses a symlink")));

        let portal = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), portal.path().join("dist")).unwrap();
        let mut report = PortalValidationReport::default();
        assert!(collect_dist_artifacts(portal.path(), &mut report).is_empty());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.contains("not a regular directory")));
    }
}
