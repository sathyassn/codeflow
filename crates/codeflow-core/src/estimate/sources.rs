//! Read-only evidence resolution, never a second persisted work graph.

use super::model::{
    Forecast, ForecastReport, Package, Pin, Source, SourceAssurance, SourceDigest,
    MAX_SOURCE_BYTES, MAX_TOTAL_SOURCE_BYTES,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

mod identity;
use identity::RecordIndex;

pub(super) fn sha256(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

#[derive(Default)]
pub(super) struct SourceGraph {
    pub package_edges: Vec<(usize, usize)>,
    pub boundary_edges: Vec<(usize, usize)>,
}

struct TaskRecord {
    dependencies: Vec<String>,
    specs: Vec<String>,
    epic: Option<String>,
}
type Fields = HashMap<String, serde_yaml::Value>;

pub(super) struct Reader {
    root: PathBuf,
    confined: crate::bounded_file::ConfinedRoot,
    remaining: u64,
    cache: BTreeMap<String, Vec<u8>>,
}

impl Reader {
    pub fn new(root: &Path) -> Option<Self> {
        let root = root.canonicalize().ok()?;
        let confined = crate::bounded_file::ConfinedRoot::open(&root).ok()?;
        root.is_dir().then_some(Self {
            root,
            confined,
            remaining: MAX_TOTAL_SOURCE_BYTES,
            cache: BTreeMap::new(),
        })
    }

    fn read(&mut self, path: &str, report: &mut ForecastReport) -> Option<Vec<u8>> {
        if let Some(bytes) = self.cache.get(path) {
            return Some(bytes.clone());
        }
        let Some(full) = safe_path(&self.root, path) else {
            report.finding(
                "unsafe_source_path",
                "sources",
                "Source path is not a confined, non-secret regular-file path.",
            );
            return None;
        };
        let Ok(bytes) = self
            .confined
            .read(Path::new(path), MAX_SOURCE_BYTES.min(self.remaining))
        else {
            report.finding("source_read","sources","Source is missing, unreadable, changed, non-regular, or exceeds the source byte budget.");
            return None;
        };
        // Recheck ancestors after reading too; source contents never enter diagnostics.
        if safe_path(&self.root, path).as_ref() != Some(&full) {
            report.finding(
                "unsafe_source_path",
                "sources",
                "Source path changed while being read.",
            );
            return None;
        }
        self.remaining -= bytes.len() as u64;
        report.source_digests.push(SourceDigest {
            path: path.to_owned(),
            sha256: sha256(&bytes),
        });
        self.cache.insert(path.to_owned(), bytes.clone());
        Some(bytes)
    }

    fn pin(&mut self, pin: &Pin, report: &mut ForecastReport) -> Option<Vec<u8>> {
        let bytes = self.read(&pin.path, report)?;
        if sha256(&bytes) != pin.sha256 {
            report.finding(
                "stale_pin",
                "sources",
                "Whole-file source digest does not match its pin.",
            );
            return None;
        }
        Some(bytes)
    }

    fn record(&mut self, path: &Path, report: &mut ForecastReport) -> Option<(String, Fields)> {
        let relative_path = path.strip_prefix(&self.root).ok()?;
        let Some(relative) = relative_path.to_str() else {
            report.finding(
                "source_record",
                "sources",
                "Work record path is not valid UTF-8.",
            );
            return None;
        };
        let relative = relative.replace('\\', "/");
        let bytes = self.read(&relative, report)?;
        if std::str::from_utf8(&bytes).is_err() {
            report.finding(
                "source_record",
                "sources",
                "Work record is not valid UTF-8 frontmatter.",
            );
            return None;
        }
        if let Ok((fields, _)) = crate::validate::parse_frontmatter(&bytes) {
            Some((relative, fields))
        } else {
            report.finding(
                "source_record",
                "sources",
                "Work record has invalid frontmatter.",
            );
            None
        }
    }
}

pub(super) fn secret_path(path: &Path) -> bool {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|part| {
            let p = part.to_ascii_lowercase();
            matches!(
                p.as_str(),
                ".git"
                    | ".ssh"
                    | ".aws"
                    | ".azure"
                    | ".gnupg"
                    | ".kube"
                    | "credentials"
                    | "credentials.json"
                    | "auth.json"
                    | ".credentials.json"
                    | "id_rsa"
                    | "id_ed25519"
                    | "id_dsa"
                    | ".npmrc"
                    | ".pypirc"
                    | ".netrc"
            ) || p == ".env"
                || p.starts_with(".env.")
                || [".pem", ".key", ".p12", ".pfx", ".keystore"]
                    .iter()
                    .any(|suffix| p.ends_with(suffix))
        })
}

fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn portable_component(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !part.is_empty()
        && part.len() <= 255
        && !part.ends_with([' ', '.'])
        && !part
            .chars()
            .any(|c| c.is_control() || "<>:\"|?*\\".contains(c))
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

fn safe_path(root: &Path, value: &str) -> Option<PathBuf> {
    let relative = Path::new(value);
    if value.is_empty()
        || value.len() > 1024
        || value.contains('\\')
        || relative.is_absolute()
        || secret_path(relative)
        || !value.split('/').all(portable_component)
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let mut full = root.to_path_buf();
    for component in relative.components() {
        full.push(component);
        if let Ok(metadata) = std::fs::symlink_metadata(&full) {
            if is_link(&metadata) {
                return None;
            }
        }
    }
    Some(full)
}

fn strings(fields: &Fields, key: &str) -> Option<Vec<String>> {
    let Some(value) = fields.get(key) else {
        return Some(Vec::new());
    };
    let values = value.as_sequence()?;
    if values.len() > 256 {
        return None;
    }
    let result = values
        .iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    (result.iter().collect::<BTreeSet<_>>().len() == result.len()).then_some(result)
}

fn task(
    reader: &mut Reader,
    id: &str,
    digest: &str,
    paths: &RecordIndex,
    report: &mut ForecastReport,
) -> Option<TaskRecord> {
    let path = paths.resolve(id, report, "task_resolution")?;
    let (relative, fields) = reader.record(path, report)?;
    if sha256(reader.cache.get(&relative)?) != digest {
        report.finding(
            "stale_pin",
            "sources",
            "Whole-file task digest does not match its pin.",
        );
        return None;
    }
    let record = (|| {
        if fields.contains_key("depends_on") && fields.contains_key("dependencies") {
            return None;
        }
        let key = if fields.contains_key("depends_on") {
            "depends_on"
        } else {
            "dependencies"
        };
        let dependencies = strings(&fields, key)?;
        let specs = strings(&fields, "specs")?;
        if dependencies
            .iter()
            .any(|v| !crate::workgraph::is_valid_task_format_id(v))
            || specs
                .iter()
                .any(|v| !crate::workgraph::is_valid_spec_format_id(v))
        {
            return None;
        }
        let epic = match fields.get("epic_id") {
            None | Some(serde_yaml::Value::Null) => None,
            Some(v) => Some(v.as_str()?.to_owned()),
        };
        Some(TaskRecord {
            dependencies,
            specs,
            epic,
        })
    })();
    if record.is_none() {
        report.finding(
            "source_record",
            "sources",
            "Task identity, dependency, parent, or specification fields are invalid.",
        );
    }
    record
}

fn inherited_specs(
    reader: &mut Reader,
    id: &str,
    paths: &RecordIndex,
    report: &mut ForecastReport,
) -> Vec<String> {
    let Some(path) = paths.resolve(id, report, "epic_resolution") else {
        return Vec::new();
    };
    let Some((_, fields)) = reader.record(path, report) else {
        return Vec::new();
    };
    let specs = strings(&fields, "specs");
    match specs {
        Some(specs)
            if specs
                .iter()
                .all(|s| crate::workgraph::is_valid_spec_format_id(s)) =>
        {
            specs
        }
        _ => {
            report.finding(
                "source_record",
                "sources",
                "Parent specification references are invalid.",
            );
            Vec::new()
        }
    }
}

fn inventory(
    f: &Forecast,
    root: &Path,
    report: &mut ForecastReport,
) -> Option<crate::workgraph::layout::RecordFiles> {
    let has_canonical = f
        .packages
        .iter()
        .map(|p| &p.source)
        .chain(f.boundaries.iter().map(|b| &b.source))
        .any(|s| s.task_id().is_some());
    if has_canonical {
        match crate::workgraph::layout::bounded_record_files(
            &root.join("project-management"),
            16_384,
        ) {
            Ok(files) => Some(files),
            Err(crate::workgraph::layout::InventoryError::LimitExceeded) => {
                report.finding(
                    "source_inventory_limit",
                    "sources",
                    "Work-record inventory exceeds 16384 directory entries.",
                );
                None
            }
            Err(crate::workgraph::layout::InventoryError::Unreadable) => {
                report.finding(
                    "source_inventory_read",
                    "sources",
                    "Work-record inventory cannot be read safely.",
                );
                None
            }
        }
    } else {
        Some(crate::workgraph::layout::RecordFiles::default())
    }
}

pub(super) fn check(f: &Forecast, reader: &mut Reader, report: &mut ForecastReport) -> SourceGraph {
    let mut graph = SourceGraph::default();
    reader.pin(&f.profile, report);
    reader.pin(&f.rubric, report);
    let Some(inventory) = inventory(f, &reader.root, report) else {
        return graph;
    };
    let paths = RecordIndex::read(
        reader,
        &inventory.tasks,
        crate::workgraph::is_valid_task_format_id,
        "TSK-NNN",
        report,
    );
    let epic_paths = RecordIndex::read(
        reader,
        &inventory.epics,
        crate::workgraph::is_valid_epic_format_id,
        "EPC-NNN",
        report,
    );
    let spec_paths = RecordIndex::read(
        reader,
        &inventory.specs,
        crate::workgraph::is_valid_spec_format_id,
        "SPC-NNN",
        report,
    );
    let mut records = BTreeMap::new();
    let mut limited = f.packages.is_empty() && f.boundaries.is_empty();
    for source in f
        .packages
        .iter()
        .map(|p| &p.source)
        .chain(f.boundaries.iter().map(|b| &b.source))
    {
        match source {
            Source::CodeflowTask { task_id, sha256 } => {
                if let Some(record) = task(reader, task_id, sha256, &paths, report) {
                    records.insert(task_id.as_str(), record);
                }
            }
            Source::External { pin, .. } | Source::Declared { pin, .. } => {
                limited = true;
                reader.pin(pin, report);
            }
        }
    }
    for (i, p) in f.packages.iter().enumerate() {
        for pin in &p.context_pins {
            reader.pin(pin, report);
        }
        let Some(record) = p.source.task_id().and_then(|id| records.get(id)) else {
            continue;
        };
        check_specs(p, record, &epic_paths, &spec_paths, reader, report);
        for predecessor in &record.dependencies {
            if let Some(j) = f
                .packages
                .iter()
                .position(|q| q.source.task_id() == Some(predecessor))
            {
                graph.package_edges.push((j, i));
            } else if let Some(j) = f
                .boundaries
                .iter()
                .position(|b| b.source.task_id() == Some(predecessor))
            {
                graph.boundary_edges.push((j, i));
            } else {
                report.finding(
                    "missing_predecessor",
                    "packages",
                    "Every canonical direct predecessor must be a package or pinned boundary.",
                );
            }
        }
    }
    for b in &f.boundaries {
        if let Some(record) = b.source.task_id().and_then(|id| records.get(id)) {
            if record
                .dependencies
                .iter()
                .any(|id| f.packages.iter().any(|p| p.source.task_id() == Some(id)))
            {
                report.finding(
                    "contradictory_boundary",
                    "boundaries",
                    "A boundary directly depends on scheduled scope.",
                );
            }
        }
    }
    if report.is_valid() {
        report.source_assurance = if limited {
            SourceAssurance::Limited
        } else {
            SourceAssurance::CanonicalLocal
        };
    }
    graph
}

fn check_specs(
    p: &Package,
    record: &TaskRecord,
    epic_paths: &RecordIndex,
    spec_paths: &RecordIndex,
    reader: &mut Reader,
    report: &mut ForecastReport,
) {
    let mut specs = record.specs.clone();
    if let Some(epic) = &record.epic {
        specs.extend(inherited_specs(reader, epic, epic_paths, report));
    }
    for spec in specs.into_iter().collect::<BTreeSet<_>>() {
        let Some(path) = spec_paths.resolve(&spec, report, "spec_resolution") else {
            continue;
        };
        let Some((relative, _)) = reader.record(path, report) else {
            continue;
        };
        if !p.context_pins.iter().any(|pin| pin.path == relative) {
            report.finding("missing_context_pin","packages.context_pins","Direct and inherited specifications require matching identity and a whole-file context pin.");
        }
    }
}

#[cfg(all(test, unix))]
mod unconfined_wrapper_scope_tests {
    use super::safe_path;

    #[test]
    fn unconfined_wrapper_plus_path_rechecks_cannot_confine_hostile_ancestor_swaps() {
        // Deterministically interleave the exact precheck/read/postcheck steps.
        // This documents the existing primitive's boundary, not an escape fix.
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let ancestor = root.path().join("evidence");
        let saved = root.path().join("saved");
        std::fs::create_dir(&ancestor).unwrap();
        std::fs::write(ancestor.join("brief.md"), "inside").unwrap();
        std::fs::write(outside.path().join("brief.md"), "outside").unwrap();
        let checked = safe_path(root.path(), "evidence/brief.md").unwrap();
        std::fs::rename(&ancestor, &saved).unwrap();
        std::os::unix::fs::symlink(outside.path(), &ancestor).unwrap();
        let bytes = crate::bounded_file::read_bounded_regular(&checked, 100).unwrap();
        std::fs::remove_file(&ancestor).unwrap();
        std::fs::rename(&saved, &ancestor).unwrap();
        assert_eq!(safe_path(root.path(), "evidence/brief.md"), Some(checked));
        assert_eq!(bytes, b"outside");
    }
}
