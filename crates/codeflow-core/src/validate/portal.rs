//! Read-only verification of documentation-portal evidence claims.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
const MAX_MEDIA_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_MEDIA_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RASTER_DIMENSION: u32 = 8_192;
const MAX_RASTER_PIXELS: u64 = 33_554_432;
const MAX_TOTAL_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_STALE_REASON_BYTES: usize = 512;
const MAX_STALE_STUB_BYTES: u64 = 4 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(30);

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
    stale_reason: Option<String>,
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
#[allow(clippy::too_many_lines)] // Sequential independent claims intentionally remain visible in one audit pipeline.
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
    let config_source_path = normalized_portal_root
        .join("portal.config.json")
        .to_string_lossy()
        .replace('\\', "/");
    match git_batch_blobs(
        &repository,
        &evidence.repository.commit,
        &[config_source_path.as_str()],
        64 * 1024,
        64 * 1024,
    ) {
        Ok(blobs)
            if blobs
                .get(&config_source_path)
                .is_some_and(|bytes| sha256_hex(bytes) == evidence.config_sha256) => {}
        Ok(_) => report
            .issues
            .push("configuration does not match its authoritative Git blob".into()),
        Err(error) => report.issues.push(format!(
            "authoritative Git configuration blob is unreadable: {error}"
        )),
    }
    let source_paths: Vec<&str> = evidence
        .pages
        .iter()
        .map(|page| page.source_path.as_str())
        .collect();
    let source_blobs = match git_batch_blobs(
        &repository,
        &evidence.repository.commit,
        &source_paths,
        MAX_SOURCE_BYTES,
        MAX_TOTAL_SOURCE_BYTES,
    ) {
        Ok(blobs) => blobs,
        Err(error) => {
            report.issues.push(format!(
                "authoritative Git source batch is unreadable: {error}"
            ));
            BTreeMap::new()
        }
    };
    let authority = collect_ids(&evidence.pages, &source_blobs);
    report.issues.extend(authority.issues.iter().cloned());
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
            || page.output_markdown_sha256 != page.markdown_twin_sha256
        {
            report.issues.push(format!(
                "{} output and Markdown twin do not match their canonical route/content",
                page.route
            ));
        }
        if page.stale {
            let fallback_title = Path::new(&page.source_path)
                .file_stem()
                .and_then(|value| value.to_str())
                .map(|value| value.replace('-', " "));
            if page.ids.len()
                + page.relationships.len()
                + page.backlinks.len()
                + page.snippets.len()
                != 0
                || page.status.is_some()
                || page.searchable
                || page.stale_reason.as_deref().is_none_or(|reason| {
                    reason.is_empty()
                        || reason.len() > MAX_STALE_REASON_BYTES
                        || reason.chars().any(char::is_control)
                })
                || fallback_title.as_deref() != Some(page.title.as_str())
            {
                report
                    .issues
                    .push(format!("{} has an invalid stale-stub envelope", page.route));
            }
        } else if page.stale_reason.is_some() {
            report.issues.push(format!(
                "{} active page unexpectedly claims a stale diagnostic",
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
        match source_blobs.get(&page.source_path) {
            Some(bytes) if sha256_hex(bytes) == page.source_sha256 => {}
            Some(_) => report.issues.push(format!(
                "{} authoritative Git source hash mismatch",
                page.route
            )),
            None => report
                .issues
                .push(format!("{} authoritative Git source is absent", page.route)),
        }
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
        let claimed_ids: BTreeSet<String> = page.ids.iter().cloned().collect();
        let expected_ids = authority
            .ids_by_source
            .get(&page.source_path)
            .cloned()
            .unwrap_or_default();
        if claimed_ids != expected_ids {
            report.issues.push(format!(
                "{} identities do not exactly match authoritative source",
                page.route
            ));
        }
        verify_snippets(
            &portal,
            page,
            source_blobs.get(&page.source_path).map(Vec::as_slice),
            &mut report,
        );
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
        if !relationships_match_authority(page, &relationship_claims, &authority) {
            report.issues.push(format!(
                "{} relationships do not exactly match authoritative source",
                page.route
            ));
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
                    escape_markdown_inline(&page.route),
                    page.route,
                    escape_markdown_inline(&page.source_path)
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
    verify_media(
        &repository,
        &portal,
        &evidence.repository.commit,
        &evidence.media,
        &mut report,
    );
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
                    if markdown.len() as u64 <= MAX_STALE_STUB_BYTES
                        && markdown.starts_with("---\n")
                        && markdown.contains("\npagefind: false\n")
                        && markdown.contains("data-pagefind-ignore=\"all\"")
                        && markdown.contains("Source unavailable")
                        && markdown.contains("the previous version of this page is not shown") => {}
                _ => report.issues.push(format!(
                    "stale page lacks its bounded visible error-stub contract: {}",
                    page.route
                )),
            }
        }
    }
    report
}

fn escape_markdown_inline(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut replacing_spacing = false;
    for character in value.chars() {
        if matches!(character, '\r' | '\n' | '\t') {
            if !replacing_spacing {
                normalized.push(' ');
                replacing_spacing = true;
            }
        } else {
            normalized.push(character);
            replacing_spacing = false;
        }
    }

    let mut escaped = String::with_capacity(normalized.len());
    for character in normalized.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '{' | '}' | '(' | ')' | '#' | '+' | '.' | '!' => {
                escaped.push('\\');
                escaped.push(character);
            }
            ':' => escaped.push_str("&#58;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn verify_config_contract(portal: &Path, evidence: &Evidence, report: &mut PortalValidationReport) {
    let path = portal.join("portal.config.json");
    let Some(config): Option<serde_json::Value> = read_bounded_regular(&path, 64 * 1024)
        .ok()
        .and_then(|bytes| parse_strict_json(&bytes).ok())
    else {
        report
            .issues
            .push("portal configuration contract is unreadable or invalid".into());
        return;
    };
    let configured_release = config
        .get("release_version")
        .and_then(serde_json::Value::as_str);
    if configured_release != evidence.repository.release_version.as_deref()
        || (!config
            .get("release_version")
            .is_some_and(serde_json::Value::is_null)
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
            .is_some_and(serde_json::Value::is_null)
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
    match git_batch_blobs(
        repository,
        &evidence.repository.commit,
        &[tokens.source_path.as_str()],
        16 * 1024,
        16 * 1024,
    ) {
        Ok(blobs)
            if blobs
                .get(&tokens.source_path)
                .is_some_and(|bytes| sha256_hex(bytes) == tokens.source_sha256) => {}
        Ok(_) => report
            .issues
            .push("primitive-token authoritative Git blob hash mismatch".into()),
        Err(error) => report.issues.push(format!(
            "primitive-token authoritative Git blob is unreadable: {error}"
        )),
    }
    verify_file(
        portal,
        &tokens.output_path,
        &tokens.output_sha256,
        "primitive-token output",
        report,
    );
}

#[allow(clippy::too_many_lines)] // Media validation keeps all evidence checks in one fail-closed pass.
fn verify_media(
    repository: &Path,
    portal: &Path,
    commit: &str,
    media: &[Media],
    report: &mut PortalValidationReport,
) {
    if media.len() > 1_000 {
        report
            .issues
            .push("referenced media count exceeds 1000".into());
        return;
    }
    let source_paths: Vec<&str> = media.iter().map(|item| item.source_path.as_str()).collect();
    let source_blobs = match git_batch_blobs(
        repository,
        commit,
        &source_paths,
        MAX_MEDIA_BYTES,
        MAX_TOTAL_MEDIA_BYTES,
    ) {
        Ok(blobs) => blobs,
        Err(error) => {
            report.issues.push(format!(
                "referenced media authoritative Git batch is unreadable: {error}"
            ));
            BTreeMap::new()
        }
    };
    let mut sources = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    let mut total_bytes = 0_u64;
    for item in media {
        let extension = Path::new(&item.source_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "gif" | "jpeg" | "jpg" | "png" | "webp") {
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
            portal,
            &item.output_path,
            &item.output_sha256,
            "referenced media output",
            report,
        );
        match source_blobs.get(&item.source_path) {
            Some(bytes) => {
                total_bytes = total_bytes.saturating_add(bytes.len() as u64);
                if sha256_hex(bytes) != item.source_sha256 {
                    report.issues.push(format!(
                        "referenced media authoritative Git blob hash mismatch: {}",
                        item.source_path
                    ));
                }
                match raster_dimensions(&extension, bytes) {
                    Some((width, height))
                        if width > 0
                            && height > 0
                            && width <= MAX_RASTER_DIMENSION
                            && height <= MAX_RASTER_DIMENSION
                            && u64::from(width) * u64::from(height) <= MAX_RASTER_PIXELS => {}
                    _ => report.issues.push(format!(
                        "referenced media bytes or dimensions are invalid: {}",
                        item.source_path
                    )),
                }
            }
            None => report.issues.push(format!(
                "referenced media authoritative Git blob is absent: {}",
                item.source_path
            )),
        }
    }
    if total_bytes > 64 * 1024 * 1024 {
        report
            .issues
            .push("referenced media corpus exceeds 67108864 bytes".into());
    }
}

fn raster_dimensions(extension: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    match extension {
        "png"
            if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
                && bytes.get(8..12) == Some(&13_u32.to_be_bytes())
                && bytes.get(12..16) == Some(b"IHDR") =>
        {
            Some((read_u32_be(bytes, 16)?, read_u32_be(bytes, 20)?))
        }
        "gif" if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => Some((
            u32::from(read_u16_le(bytes, 6)?),
            u32::from(read_u16_le(bytes, 8)?),
        )),
        "webp" if bytes.get(0..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") => {
            webp_dimensions(bytes)
        }
        "jpg" | "jpeg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => jpeg_dimensions(bytes),
        _ => None,
    }
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u16_be(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u24_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let chunk = bytes.get(offset..offset + 3)?;
    Some(u32::from(chunk[0]) | (u32::from(chunk[1]) << 8) | (u32::from(chunk[2]) << 16))
}

fn read_u32_be(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    match bytes.get(12..16)? {
        b"VP8X" => Some((read_u24_le(bytes, 24)? + 1, read_u24_le(bytes, 27)? + 1)),
        b"VP8L" if bytes.get(20) == Some(&0x2f) => {
            let one = u32::from(*bytes.get(21)?);
            let two = u32::from(*bytes.get(22)?);
            let three = u32::from(*bytes.get(23)?);
            let four = u32::from(*bytes.get(24)?);
            Some((
                1 + one + ((two & 0x3f) << 8),
                1 + (two >> 6) + (three << 2) + ((four & 0x0f) << 10),
            ))
        }
        b"VP8 " if bytes.get(23..26) == Some(&[0x9d, 0x01, 0x2a]) => Some((
            u32::from(read_u16_le(bytes, 26)? & 0x3fff),
            u32::from(read_u16_le(bytes, 28)? & 0x3fff),
        )),
        _ => None,
    }
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut offset = 2_usize;
    while offset < bytes.len() {
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        if matches!(marker, 0xd9 | 0xda) {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = usize::from(read_u16_be(bytes, offset)?);
        if length < 2 || offset.checked_add(length)? > bytes.len() {
            return None;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if length < 7 {
                return None;
            }
            return Some((
                u32::from(read_u16_be(bytes, offset + 5)?),
                u32::from(read_u16_be(bytes, offset + 3)?),
            ));
        }
        offset += length;
    }
    None
}

#[allow(clippy::too_many_lines)] // Includes the bounded recursive visitor and its top-level invariants.
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
    portal: &Path,
    page: &Page,
    source: Option<&[u8]>,
    report: &mut PortalValidationReport,
) {
    if page.stale {
        if !page.snippets.is_empty() {
            report
                .issues
                .push(format!("{} stale stub claims source snippets", page.route));
        }
        return;
    }
    let Some(text) = source.and_then(|bytes| std::str::from_utf8(bytes).ok()) else {
        report.issues.push(format!(
            "{} snippet source is not valid UTF-8 or readable",
            page.source_path
        ));
        return;
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
    if !output.lines().any(|line| line == marker) {
        report.issues.push(format!(
            "{} lacks its exact rendered provenance marker",
            page.route
        ));
    }
    if page.stale
        && (!output.contains("\npagefind: false\n")
            || !output.contains("data-pagefind-ignore=\"all\"")
            || !output.contains("Source unavailable")
            || !output.contains("the previous version of this page is not shown")
            || output.len() as u64 > MAX_STALE_STUB_BYTES)
    {
        report.issues.push(format!(
            "{} lacks its exact bounded stale-stub markers",
            page.route
        ));
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
    String::from_utf8(git_output_bounded(root, args, None, maximum_bytes)?)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))
}

fn git_batch_blobs(
    root: &Path,
    commit: &str,
    source_paths: &[&str],
    maximum_blob_bytes: u64,
    maximum_total_bytes: u64,
) -> std::io::Result<BTreeMap<String, Vec<u8>>> {
    if !valid_commit(commit) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid Git commit identity",
        ));
    }
    let paths: BTreeSet<&str> = source_paths.iter().copied().collect();
    if paths.len() > MAX_PAGES || paths.iter().any(|path| !safe_path_text(path)) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid or excessive Git blob paths",
        ));
    }
    let mut input = Vec::new();
    for path in &paths {
        writeln!(input, "{commit}:{path}")?;
    }
    let framing_bytes = u64::try_from(paths.len())
        .unwrap_or(u64::MAX)
        .saturating_mul(128);
    let maximum_output = maximum_total_bytes.saturating_add(framing_bytes);
    let output = git_output_bounded(root, &["cat-file", "--batch"], Some(input), maximum_output)?;
    parse_git_batch(&output, &paths, maximum_blob_bytes, maximum_total_bytes)
}

fn parse_git_batch(
    output: &[u8],
    paths: &BTreeSet<&str>,
    maximum_blob_bytes: u64,
    maximum_total_bytes: u64,
) -> std::io::Result<BTreeMap<String, Vec<u8>>> {
    let mut cursor = 0_usize;
    let mut total = 0_u64;
    let mut blobs = BTreeMap::new();
    for path in paths {
        let line_end = output[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| cursor + offset)
            .ok_or_else(|| std::io::Error::other("Git batch header is truncated"))?;
        let header = std::str::from_utf8(&output[cursor..line_end]).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })?;
        cursor = line_end + 1;
        if header.ends_with(" missing") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Git blob is absent at the evidenced commit: {path}"),
            ));
        }
        let mut fields = header.split_ascii_whitespace();
        let object_id = fields.next().unwrap_or_default();
        let object_type = fields.next().unwrap_or_default();
        let size = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| std::io::Error::other("Git batch size is invalid"))?;
        if fields.next().is_some()
            || object_id.len() < 40
            || !object_id.bytes().all(|byte| byte.is_ascii_hexdigit())
            || object_type != "blob"
            || size > maximum_blob_bytes
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Git batch blob metadata is invalid: {path}"),
            ));
        }
        total = total.checked_add(size).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Git blob total overflow")
        })?;
        if total > maximum_total_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Git blob corpus exceeds its byte limit",
            ));
        }
        let size = usize::try_from(size).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Git blob size is unsupported",
            )
        })?;
        let end = cursor
            .checked_add(size)
            .ok_or_else(|| std::io::Error::other("Git batch offset overflow"))?;
        if end >= output.len() || output[end] != b'\n' {
            return Err(std::io::Error::other("Git batch blob is truncated"));
        }
        blobs.insert((*path).to_string(), output[cursor..end].to_vec());
        cursor = end + 1;
    }
    if cursor != output.len() {
        return Err(std::io::Error::other(
            "Git batch emitted unexpected trailing output",
        ));
    }
    Ok(blobs)
}

fn hardened_git(
    root: &Path,
    args: &[&str],
    piped_stdin: bool,
) -> std::io::Result<std::process::Child> {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .args(["-C"])
        .arg(root)
        .args(["-c", "core.fsmonitor=false", "-c", "core.pager=cat"])
        .args(args)
        .stdin(if piped_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env("LC_ALL", "C");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "GIT_EXTERNAL_DIFF",
        "GIT_DIFF_OPTS",
    ] {
        command.env_remove(name);
    }
    command.spawn()
}

fn drain_bounded(
    mut stream: impl Read,
    maximum_bytes: u64,
    overflow: Option<&AtomicBool>,
) -> std::io::Result<Vec<u8>> {
    let mut retained = Vec::new();
    let mut total = 0_u64;
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        let remaining = maximum_bytes.saturating_sub(retained.len() as u64);
        let keep = usize::try_from(remaining.min(count as u64)).unwrap_or(0);
        retained.extend_from_slice(&chunk[..keep]);
        if total > maximum_bytes {
            if let Some(flag) = overflow {
                flag.store(true, Ordering::Release);
            }
        }
    }
    Ok(retained)
}

fn git_output_bounded(
    root: &Path,
    args: &[&str],
    input: Option<Vec<u8>>,
    maximum_bytes: u64,
) -> std::io::Result<Vec<u8>> {
    let mut child = hardened_git(root, args, input.is_some())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("Git stdout was not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("Git stderr was not captured"))?;
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout_overflow = Arc::clone(&overflow);
    let stdout_thread =
        std::thread::spawn(move || drain_bounded(stdout, maximum_bytes, Some(&stdout_overflow)));
    let stderr_thread = std::thread::spawn(move || drain_bounded(stderr, 4 * 1024, None));
    let stdin_thread = input.map(|bytes| {
        let mut stdin = child.stdin.take().expect("piped Git stdin");
        std::thread::spawn(move || stdin.write_all(&bytes))
    });
    let started = Instant::now();
    let (status, aborted) = loop {
        if overflow.load(Ordering::Acquire) {
            let _ = child.kill();
            break (child.wait()?, Some("Git output exceeds its byte limit"));
        }
        if started.elapsed() >= GIT_TIMEOUT {
            let _ = child.kill();
            break (child.wait()?, Some("Git command exceeded its time limit"));
        }
        if let Some(status) = child.try_wait()? {
            break (status, None);
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let bytes = stdout_thread
        .join()
        .map_err(|_| std::io::Error::other("Git stdout reader panicked"))??;
    let error_bytes = stderr_thread
        .join()
        .map_err(|_| std::io::Error::other("Git stderr reader panicked"))??;
    if let Some(stdin_thread) = stdin_thread {
        match stdin_thread.join() {
            Ok(Err(error)) if aborted.is_none() => return Err(error),
            Ok(Ok(()) | Err(_)) => {}
            Err(_) => return Err(std::io::Error::other("Git stdin writer panicked")),
        }
    }
    if let Some(message) = aborted {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message,
        ));
    }
    if !status.success() {
        let detail = String::from_utf8_lossy(&error_bytes);
        return Err(std::io::Error::other(format!(
            "Git command failed{}",
            if detail.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", detail.trim())
            }
        )));
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
    if !opened.is_file() || opened.len() > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file changed identity or type while opening",
        ));
    }
    let opened_identity = same_file::Handle::from_file(file.try_clone()?)?;
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
    let linked_identity = same_file::Handle::from_path(path)?;
    if after.file_type().is_symlink()
        || !after.is_file()
        || opened_identity != linked_identity
        || opened.len() != bytes.len() as u64
        || opened.len() != after.len()
        || !stable_metadata(&opened, &after)?
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file changed while it was being read",
        ));
    }
    Ok(bytes)
}

fn stable_metadata(left: &std::fs::Metadata, right: &std::fs::Metadata) -> std::io::Result<bool> {
    if left.modified()? != right.modified()? {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(left.ctime() == right.ctime() && left.ctime_nsec() == right.ctime_nsec())
    }
    #[cfg(not(unix))]
    Ok(true)
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
    ids_by_source: BTreeMap<String, BTreeSet<String>>,
    relationships: BTreeMap<String, BTreeSet<Relationship>>,
    issues: Vec<String>,
}

fn relationships_match_authority(
    page: &Page,
    claims: &BTreeSet<Relationship>,
    authority: &AuthorityIds,
) -> bool {
    claims
        == &authority
            .relationships
            .get(&page.source_path)
            .cloned()
            .unwrap_or_default()
}

fn collect_ids(pages: &[Page], source_blobs: &BTreeMap<String, Vec<u8>>) -> AuthorityIds {
    let mut authority = AuthorityIds {
        owners: BTreeMap::new(),
        ids_by_source: BTreeMap::new(),
        relationships: BTreeMap::new(),
        issues: Vec::new(),
    };
    let mut seen_sources = BTreeSet::new();
    for page in pages {
        if page.stale
            || !seen_sources.insert(page.source_path.as_str())
            || !safe_path_text(&page.source_path)
        {
            continue;
        }
        let Some(bytes) = source_blobs.get(&page.source_path) else {
            continue;
        };
        let text = match String::from_utf8(bytes.clone()) {
            Ok(text) => text
                .strip_prefix('\u{feff}')
                .unwrap_or(&text)
                .replace("\r\n", "\n")
                .replace('\r', "\n"),
            Err(error) => {
                authority.issues.push(format!(
                    "authoritative source is not valid UTF-8 {}: {error}",
                    page.source_path
                ));
                continue;
            }
        };
        if page.source_path == "docs/capabilities.md" {
            collect_capability_authority(&text, &page.source_path, &mut authority);
            continue;
        }
        let frontmatter = match parse_frontmatter_mapping(&text) {
            Ok(frontmatter) => frontmatter,
            Err(error) => {
                authority.issues.push(format!(
                    "authoritative frontmatter is invalid {}: {error}",
                    page.source_path
                ));
                continue;
            }
        };
        let source_id = mapping_string(&frontmatter, "id").filter(|id| strict_id(id));
        if let Some(id) = source_id {
            add_authoritative_id(id, &page.source_path, &mut authority);
        } else if let Some(stem) = Path::new(&page.source_path)
            .file_stem()
            .and_then(|value| value.to_str())
        {
            let id = stem.to_ascii_uppercase();
            if strict_id(&id) {
                add_authoritative_id(&id, &page.source_path, &mut authority);
            }
        }
        for relationship in relationships_from_mapping(&frontmatter, source_id) {
            authority
                .relationships
                .entry(page.source_path.clone())
                .or_default()
                .insert(relationship);
        }
    }
    authority
}

fn add_authoritative_id(id: &str, source_path: &str, authority: &mut AuthorityIds) {
    authority
        .ids_by_source
        .entry(source_path.to_string())
        .or_default()
        .insert(id.to_string());
    if let Some(prior) = authority
        .owners
        .insert(id.to_string(), source_path.to_string())
    {
        if prior != source_path {
            authority.issues.push(format!(
                "authoritative ID {id} is declared more than once: {prior} and {source_path}"
            ));
        }
    }
}

fn collect_capability_authority(text: &str, source_path: &str, authority: &mut AuthorityIds) {
    let (entries, parse_issues) = parse_capabilities(text);
    for issue in parse_issues {
        authority.issues.push(format!(
            "docs/capabilities.md:{}: {}",
            issue.line, issue.message
        ));
    }
    for entry in entries {
        if !strict_id(&entry.id) {
            continue;
        }
        add_authoritative_id(&entry.id, source_path, authority);
        for (kind, targets) in [
            ("epic", entry.epic_id),
            ("epic", entry.epics),
            ("spec", entry.specs),
            ("depends_on", entry.depends_on),
            ("capability", entry.capabilities),
            ("decision", entry.adrs),
            ("related", entry.related),
            ("superseded_by", entry.superseded_by),
        ] {
            for target in targets.into_iter().filter(|target| strict_id(target)) {
                authority
                    .relationships
                    .entry(source_path.to_string())
                    .or_default()
                    .insert(Relationship {
                        kind: kind.to_string(),
                        target,
                        source_id: Some(entry.id.clone()),
                    });
            }
        }
    }
}

fn parse_frontmatter_mapping(text: &str) -> Result<serde_yaml::Mapping, String> {
    if !text.starts_with("---\n") {
        return Ok(serde_yaml::Mapping::new());
    }
    let Some(end) = text[4..].find("\n---\n").map(|index| index + 4) else {
        return Err("unclosed YAML frontmatter".into());
    };
    let value: serde_yaml::Value =
        serde_yaml::from_str(&text[4..end]).map_err(|error| error.to_string())?;
    value
        .as_mapping()
        .cloned()
        .ok_or_else(|| "frontmatter must be a mapping".into())
}

fn mapping_string<'a>(mapping: &'a serde_yaml::Mapping, key: &str) -> Option<&'a str> {
    mapping
        .get(serde_yaml::Value::String(key.to_string()))
        .and_then(serde_yaml::Value::as_str)
}

fn relationships_from_mapping(
    mapping: &serde_yaml::Mapping,
    source_id: Option<&str>,
) -> BTreeSet<Relationship> {
    const FIELDS: [(&str, &str); 8] = [
        ("epic_id", "epic"),
        ("epics", "epic"),
        ("specs", "spec"),
        ("depends_on", "depends_on"),
        ("capabilities", "capability"),
        ("adrs", "decision"),
        ("related", "related"),
        ("superseded_by", "superseded_by"),
    ];
    let mut relationships = BTreeSet::new();
    for (field, kind) in FIELDS {
        let Some(value) = mapping.get(serde_yaml::Value::String(field.to_string())) else {
            continue;
        };
        let targets: Vec<&str> = match value {
            serde_yaml::Value::String(target) => vec![target],
            serde_yaml::Value::Sequence(targets) => targets
                .iter()
                .filter_map(serde_yaml::Value::as_str)
                .collect(),
            _ => Vec::new(),
        };
        for target in targets.into_iter().filter(|target| strict_id(target)) {
            relationships.insert(Relationship {
                kind: kind.to_string(),
                target: target.to_string(),
                source_id: source_id.map(str::to_string),
            });
        }
    }
    relationships
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
            stale_reason: None,
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
    fn llms_markdown_escaping_matches_the_adapter_contract() {
        assert_eq!(
            escape_markdown_inline("docs/a.b_[c]:d&<e>\t\nnext"),
            r"docs/a\.b\_\[c\]&#58;d&amp;&lt;e&gt; next"
        );
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
    fn raster_media_requires_bounded_dimensions_matching_the_claimed_type() {
        let mut png = vec![0_u8; 24];
        png[..8].copy_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        png[8..12].copy_from_slice(&13_u32.to_be_bytes());
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&1_u32.to_be_bytes());
        png[20..24].copy_from_slice(&1_u32.to_be_bytes());
        assert_eq!(raster_dimensions("png", &png), Some((1, 1)));
        png[16..20].copy_from_slice(&65_535_u32.to_be_bytes());
        assert!(
            raster_dimensions("png", &png).is_some_and(|(width, height)| {
                width > MAX_RASTER_DIMENSION
                    || u64::from(width) * u64::from(height) > MAX_RASTER_PIXELS
            })
        );
        assert_eq!(raster_dimensions("png", b"<svg></svg>"), None);
        assert_eq!(raster_dimensions("gif", b"not-a-gif"), None);
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
        let paths = ["docs/capabilities.md", "docs/example.md"];
        let blobs = git_batch_blobs(
            temp.path(),
            &commit,
            &paths,
            MAX_SOURCE_BYTES,
            MAX_TOTAL_SOURCE_BYTES,
        )
        .unwrap();
        let authority = collect_ids(&pages, &blobs);
        assert!(authority.issues.is_empty(), "{:?}", authority.issues);
        assert_eq!(
            authority.owners.get("CAP-101").map(String::as_str),
            Some("docs/capabilities.md")
        );
        assert!(!authority.owners.contains_key("CAP-999"));
        assert!(!authority.owners.contains_key("TSK-999"));
    }

    #[test]
    fn relationship_evidence_must_exactly_match_authoritative_kind_target_and_source_id() {
        let mut record = page("project-management/tasks/TSK-101.md");
        record.ids = vec!["TSK-101".into()];
        let bytes = b"---\nid: TSK-101\nepic_id: EPC-100\ndepends_on: [TSK-099]\nadrs: [ADR-0001]\n---\n# Task\n".to_vec();
        let blobs = BTreeMap::from([(record.source_path.clone(), bytes)]);
        let authority = collect_ids(&[page("project-management/tasks/TSK-101.md")], &blobs);
        let exact = authority
            .relationships
            .get(&record.source_path)
            .cloned()
            .unwrap();
        assert!(relationships_match_authority(&record, &exact, &authority));

        let mut omitted = exact.clone();
        omitted.pop_first();
        assert!(!relationships_match_authority(
            &record, &omitted, &authority
        ));

        let mut extra = exact.clone();
        extra.insert(Relationship {
            kind: "related".into(),
            target: "CAP-999".into(),
            source_id: Some("TSK-101".into()),
        });
        assert!(!relationships_match_authority(&record, &extra, &authority));

        let mut wrong_kind = exact.clone();
        let relation = wrong_kind.pop_first().unwrap();
        wrong_kind.insert(Relationship {
            kind: "related".into(),
            ..relation
        });
        assert!(!relationships_match_authority(
            &record,
            &wrong_kind,
            &authority
        ));

        let mut wrong_source = exact.clone();
        let relation = wrong_source.pop_first().unwrap();
        wrong_source.insert(Relationship {
            source_id: Some("TSK-404".into()),
            ..relation
        });
        assert!(!relationships_match_authority(
            &record,
            &wrong_source,
            &authority
        ));
    }

    #[test]
    fn bounded_git_output_aborts_large_blobs_without_deadlock() {
        let repository = tempfile::tempdir().unwrap();
        std::fs::write(
            repository.path().join("large.bin"),
            vec![b'x'; 8 * 1024 * 1024],
        )
        .unwrap();
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "portal-tests@codeflow.invalid"][..],
            &["config", "user.name", "Portal tests"][..],
            &["add", "large.bin"][..],
            &["commit", "-q", "-m", "large fixture"][..],
        ] {
            assert!(Command::new("git")
                .args(["-C"])
                .arg(repository.path())
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        let started = Instant::now();
        let error = git_output_bounded(repository.path(), &["show", "HEAD:large.bin"], None, 1024)
            .unwrap_err();
        assert!(error.to_string().contains("byte limit"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn batch_blob_lookup_fails_closed_when_the_local_object_is_missing() {
        let repository = tempfile::tempdir().unwrap();
        std::fs::create_dir(repository.path().join("docs")).unwrap();
        std::fs::write(repository.path().join("docs/guide.md"), "# Guide\n").unwrap();
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "portal-tests@codeflow.invalid"][..],
            &["config", "user.name", "Portal tests"][..],
            &["add", "docs/guide.md"][..],
            &["commit", "-q", "-m", "fixture"][..],
        ] {
            assert!(Command::new("git")
                .args(["-C"])
                .arg(repository.path())
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        let commit = git_text_bounded(repository.path(), &["rev-parse", "HEAD"], 1024)
            .unwrap()
            .trim()
            .to_owned();
        let object = git_text_bounded(
            repository.path(),
            &["rev-parse", "HEAD:docs/guide.md"],
            1024,
        )
        .unwrap()
        .trim()
        .to_owned();
        std::fs::remove_file(
            repository
                .path()
                .join(".git/objects")
                .join(&object[..2])
                .join(&object[2..]),
        )
        .unwrap();
        let result = git_batch_blobs(
            repository.path(),
            &commit,
            &["docs/guide.md"],
            MAX_SOURCE_BYTES,
            MAX_TOTAL_SOURCE_BYTES,
        );
        assert!(result.is_err());
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

    #[cfg(unix)]
    #[test]
    fn authority_and_artifact_roots_refuse_parent_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("task.md"), "---\nid: TSK-777\n---\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("docs")).unwrap();
        let authority = collect_ids(&[page("docs/task.md")], &BTreeMap::new());
        assert!(!authority.owners.contains_key("TSK-777"));
        assert!(authority.issues.is_empty());

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
