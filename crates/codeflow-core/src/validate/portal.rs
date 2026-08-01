//! Read-only verification of documentation-portal evidence claims.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::de::{DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;

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
    route: String,
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
    let adoption_bytes = match std::fs::read(repo_root.join(".codeflow/docs-portal.json")) {
        Ok(bytes) if bytes.len() <= 64 * 1024 => bytes,
        Ok(_) => {
            report
                .issues
                .push("portal adoption state exceeds 65536 bytes".into());
            return report;
        }
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
    let bytes = match std::fs::read(&evidence_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            report
                .issues
                .push(format!("{}: {error}", evidence_path.display()));
            return report;
        }
    };
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        report.issues.push(format!(
            "evidence manifest exceeds {MAX_MANIFEST_BYTES} bytes"
        ));
        return report;
    }
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
    if evidence.repository.commit != "unavailable"
        && (evidence.repository.commit.len() != 40
            || !evidence
                .repository
                .commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit()))
    {
        report
            .issues
            .push("repository commit is not a full Git object ID".into());
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
    let authority = collect_ids(&repository, &evidence.pages);
    report.issues.extend(authority.issues);
    let ids: BTreeSet<String> = authority.owners.keys().cloned().collect();
    let mut routes = BTreeSet::new();
    let mut claimed_paths = BTreeSet::new();
    let mut id_owner = BTreeMap::new();
    for page in &evidence.pages {
        report.checked_pages += 1;
        if !safe_path_text(&page.route) || !routes.insert(page.route.to_ascii_lowercase()) {
            report
                .issues
                .push(format!("duplicate or unsafe route {:?}", page.route));
        }
        for claimed in [
            &page.source_path,
            &page.output_markdown,
            &page.markdown_twin,
        ] {
            if !claimed_paths.insert(claimed.to_ascii_lowercase()) {
                report
                    .issues
                    .push(format!("path is claimed more than once: {claimed}"));
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
    }
    let mut expected = BTreeMap::<String, Vec<Backlink>>::new();
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
            if let Some(target_route) = id_owner.get(&relationship.target) {
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
    if let Ok(llms) = std::fs::read_to_string(portal.join("public/llms.txt")) {
        for page in evidence.pages.iter().filter(|page| !page.stale) {
            let expected = format!("./markdown/{}.md", page.route);
            if !llms.contains(&expected) {
                report.issues.push(format!(
                    "llms.txt does not link the served Markdown twin for {}",
                    page.route
                ));
            }
        }
        for page in evidence.pages.iter().filter(|page| page.stale) {
            let forbidden = format!("./markdown/{}.md", page.route);
            if llms.contains(&forbidden) {
                report.issues.push(format!(
                    "llms.txt exposes stale Markdown twin for {}",
                    page.route
                ));
            }
        }
    }
    let mut artifact_paths = BTreeSet::new();
    for artifact in &evidence.artifacts {
        if !artifact.path.starts_with("dist/") || !safe_path_text(&artifact.path) {
            report.issues.push(format!(
                "built artifact path must stay under dist/: {}",
                artifact.path
            ));
            continue;
        }
        if !artifact_paths.insert(artifact.path.to_ascii_lowercase()) {
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
        if !artifact_paths.contains(&built.to_ascii_lowercase()) {
            report
                .issues
                .push(format!("page is missing its built output: {}", page.route));
        }
        if let Ok(html) = std::fs::read_to_string(portal.join(&built)) {
            if page.searchable && !html.contains("data-pagefind-body") {
                report.issues.push(format!(
                    "searchable page lacks built Pagefind body evidence: {}",
                    page.route
                ));
            }
            if page.stale && html.contains("data-pagefind-body") {
                report.issues.push(format!(
                    "stale page remains included in built Pagefind content: {}",
                    page.route
                ));
            }
        }
        if page.stale {
            match std::fs::read_to_string(portal.join(&page.output_markdown)) {
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
                let artifact = format!("dist/{}", child.to_string_lossy().replace('\\', "/"))
                    .to_ascii_lowercase();
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
    let text = match std::fs::read_to_string(path) {
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
            "codeflow-source-snippet sha256={} lines={}-{}",
            snippet.sha256, snippet.start_line, snippet.end_line
        );
        match std::fs::read_to_string(portal.join(&page.output_markdown)) {
            Ok(output) if output.contains(&marker) => {}
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
    let Ok(output) = std::fs::read_to_string(portal.join(&page.output_markdown)) else {
        return;
    };
    if !output.contains(&page.source_path) || !output.contains(&evidence.generator.version) {
        report.issues.push(format!(
            "{} lacks rendered source/version provenance",
            page.route
        ));
    }
    if evidence.repository.commit != "unavailable" {
        match evidence.repository.commit.get(..12) {
            Some(prefix) if output.contains(prefix) => {}
            _ => report.issues.push(format!(
                "{} lacks rendered pinned repository provenance",
                page.route
            )),
        }
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
    let Some(path) = safe_join(root, Path::new(relative), label, report) else {
        return;
    };
    match std::fs::metadata(&path) {
        Ok(metadata) if metadata.len() > MAX_CLAIMED_FILE_BYTES => {
            report.issues.push(format!(
                "{label} exceeds {MAX_CLAIMED_FILE_BYTES} bytes: {relative}"
            ));
            return;
        }
        Err(error) => {
            report
                .issues
                .push(format!("{label} unreadable {relative}: {error}"));
            return;
        }
        _ => {}
    }
    match std::fs::read(&path) {
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
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    (!normalized.as_os_str().is_empty()).then_some(normalized)
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

fn collect_ids(root: &Path, pages: &[Page]) -> AuthorityIds {
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
        let Some(path) = safe_join(
            root,
            Path::new(&page.source_path),
            "authoritative source",
            &mut path_report,
        ) else {
            authority.issues.extend(path_report.issues);
            continue;
        };
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                authority.issues.push(format!(
                    "authoritative source is not a regular file: {}",
                    page.source_path
                ));
                continue;
            }
            Ok(metadata) if metadata.len() > MAX_SOURCE_BYTES => {
                authority.issues.push(format!(
                    "authoritative source exceeds {MAX_SOURCE_BYTES} bytes: {}",
                    page.source_path
                ));
                continue;
            }
            Ok(metadata) => metadata,
            Err(error) => {
                authority.issues.push(format!(
                    "authoritative source is unreadable {}: {error}",
                    page.source_path
                ));
                continue;
            }
        };
        total_bytes = total_bytes.saturating_add(metadata.len());
        if total_bytes > MAX_TOTAL_SOURCE_BYTES {
            authority.issues.push(format!(
                "authoritative source corpus exceeds {MAX_TOTAL_SOURCE_BYTES} bytes"
            ));
            break;
        }
        let text = match std::fs::read_to_string(&path) {
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
                    add_id(entry.id, &path, root, &mut authority);
                }
            }
            continue;
        }
        if let Some(id) = frontmatter_id(&text) {
            add_id(id, &path, root, &mut authority);
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
            let id = stem.to_ascii_uppercase();
            if strict_id(&id) {
                add_id(id, &path, root, &mut authority);
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
            route: "reference/page".into(),
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
        let pages = vec![page("docs/capabilities.md"), page("docs/example.md")];
        let authority = collect_ids(temp.path(), &pages);
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

    #[cfg(unix)]
    #[test]
    fn authority_and_artifact_roots_refuse_parent_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("task.md"), "---\nid: TSK-777\n---\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("docs")).unwrap();
        let authority = collect_ids(temp.path(), &[page("docs/task.md")]);
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
