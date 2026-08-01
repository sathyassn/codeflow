//! Opt-in documentation-portal starter materialization and reconciliation.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use unicode_normalization::UnicodeNormalization;

use super::report::{Action, Report};
use super::state::{guard_beneath_root, remove_beneath_root, write_beneath_root};
use super::{sha256_hex, AssetSource, ScaffoldError};

const ASSET_PREFIX: &str = "docs-portal/starter/";
const MANIFEST_ASSET: &str = "docs-portal/manifest.json";
const STATE_PATH: &str = ".codeflow/docs-portal.json";
const BASELINE_ROOT: &str = ".codeflow/.docs-portal-baseline";
const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_BUNDLE_FILES: usize = 256;
const MAX_STATE_BYTES: u64 = 64 * 1024;
const MAX_BASELINE_ENTRIES: usize = 1_024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleManifest {
    schema_version: u32,
    version: String,
    #[serde(deserialize_with = "deserialize_bundle_files")]
    files: Vec<BundleFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleFile {
    path: String,
    ownership: BundleOwnership,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum BundleOwnership {
    Managed,
    UserOwned,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortalState {
    schema_version: u32,
    root: String,
    starter_version: String,
    #[serde(deserialize_with = "deserialize_portal_files")]
    files: BTreeMap<String, PortalFileState>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortalFileState {
    ownership: String,
    pristine_sha256: String,
}

fn deserialize_bundle_files<'de, D>(deserializer: D) -> Result<Vec<BundleFile>, D::Error>
where
    D: Deserializer<'de>,
{
    struct FilesVisitor;
    impl<'de> Visitor<'de> for FilesVisitor {
        type Value = Vec<BundleFile>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a bounded portal bundle file list")
        }
        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut files =
                Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX_BUNDLE_FILES));
            while let Some(file) = sequence.next_element()? {
                if files.len() >= MAX_BUNDLE_FILES {
                    return Err(serde::de::Error::custom(
                        "portal bundle contains too many files",
                    ));
                }
                files.push(file);
            }
            Ok(files)
        }
    }
    deserializer.deserialize_seq(FilesVisitor)
}

fn deserialize_portal_files<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, PortalFileState>, D::Error>
where
    D: Deserializer<'de>,
{
    struct FilesVisitor;
    impl<'de> Visitor<'de> for FilesVisitor {
        type Value = BTreeMap<String, PortalFileState>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a bounded portal state file map")
        }
        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut files = BTreeMap::new();
            while let Some((path, state)) = map.next_entry()? {
                if files.len() >= MAX_BUNDLE_FILES {
                    return Err(serde::de::Error::custom(
                        "portal state contains too many files",
                    ));
                }
                if files.insert(path, state).is_some() {
                    return Err(serde::de::Error::custom("duplicate portal state file"));
                }
            }
            Ok(files)
        }
    }
    deserializer.deserialize_map(FilesVisitor)
}

/// Adopt or reconcile the embedded portal starter beneath `portal_root`.
pub fn setup_portal(
    source: &dyn AssetSource,
    repo_root: &Path,
    portal_root: &Path,
) -> Result<Report, ScaffoldError> {
    let relative_root = validate_portal_root(portal_root)?;
    guard_beneath_root(repo_root, &relative_root)?;
    if !repo_root.join(".codeflow/project.toml").exists() {
        return Err(ScaffoldError::NotInitialized);
    }
    let manifest_bytes = source
        .read(MANIFEST_ASSET)
        .ok_or_else(|| ScaffoldError::ManifestMissing(MANIFEST_ASSET.into()))?;
    if manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ScaffoldError::ManifestInvalid(
            "docs portal manifest exceeds its byte limit".into(),
        ));
    }
    let manifest: BundleManifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.schema_version != 1
        || manifest.version.is_empty()
        || manifest.version.len() > 128
        || manifest.files.is_empty()
        || manifest.files.len() > MAX_BUNDLE_FILES
    {
        return Err(ScaffoldError::ManifestInvalid(
            "docs portal manifest must use schema 1 and contain files".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    for file in &manifest.files {
        validate_asset_path(&file.path)?;
        if !seen.insert(portable_key(&file.path)) {
            return Err(ScaffoldError::ManifestInvalid(format!(
                "duplicate docs portal path {:?}",
                file.path
            )));
        }
    }
    let portal_destination = guard_beneath_root(repo_root, &relative_root)?;
    if let Ok(metadata) = std::fs::symlink_metadata(&portal_destination) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ScaffoldError::InvalidState {
                what: path_text(&relative_root),
                detail: "portal root exists but is not a regular directory".into(),
            });
        }
    }

    let previous = load_state(repo_root)?;
    if let Some(state) = &previous {
        if state.schema_version != 1 {
            return Err(ScaffoldError::InvalidState {
                what: STATE_PATH.into(),
                detail: "unsupported schema_version".into(),
            });
        }
        if !roots_equal(&state.root, &path_text(&relative_root)) {
            return Err(ScaffoldError::InvalidState {
                what: STATE_PATH.into(),
                detail: format!(
                    "portal is already adopted at {:?}; refusing a second root",
                    state.root
                ),
            });
        }
    }
    let mut report = Report::new(format!(
        "codeflow portal setup ({})",
        relative_root.display()
    ));
    let mut next_files = BTreeMap::new();
    for file in &manifest.files {
        let asset = source
            .read(&format!("{ASSET_PREFIX}{}", file.path))
            .ok_or_else(|| {
                ScaffoldError::ManifestMissing(format!("{ASSET_PREFIX}{}", file.path))
            })?;
        let mut pristine = String::from_utf8(asset).map_err(|e| {
            ScaffoldError::ManifestInvalid(format!("{} is not UTF-8: {e}", file.path))
        })?;
        let dest_rel = relative_root.join(&file.path);
        let dest_text = path_text(&dest_rel);
        let dest = guard_beneath_root(repo_root, &dest_rel)?;
        if let Ok(metadata) = std::fs::symlink_metadata(&dest) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(ScaffoldError::InvalidState {
                    what: dest_text.clone(),
                    detail: "portal destination exists but is not a regular file".into(),
                });
            }
        }
        if file.path == "portal.config.json" && !dest.exists() {
            let mut config: serde_json::Value = serde_json::from_str(&pristine)?;
            let depth = relative_root
                .components()
                .filter(|component| matches!(component, Component::Normal(_)))
                .count();
            config["repository_root"] = serde_json::Value::String(
                std::iter::repeat_n("..", depth)
                    .collect::<Vec<_>>()
                    .join("/"),
            );
            pristine = format!("{}\n", serde_json::to_string_pretty(&config)?);
        }
        let pristine_hash = sha256_hex(pristine.as_bytes());
        let old_file = previous
            .as_ref()
            .and_then(|state| state.files.get(&file.path));
        let baseline_rel = baseline_path(&pristine_hash);
        let prior_baseline_rel = old_file.map(|state| baseline_path(&state.pristine_sha256));

        if file.ownership == BundleOwnership::UserOwned {
            if dest.exists() {
                report.file(dest_text, Action::Skipped);
            } else {
                write_beneath_root(repo_root, &dest_text, pristine.as_bytes())?;
                report.file(dest_text, Action::Created);
            }
        } else {
            reconcile_managed(
                repo_root,
                &dest_text,
                &dest,
                prior_baseline_rel.as_deref(),
                &pristine,
                old_file,
                &mut report,
            )?;
            write_beneath_root(repo_root, &baseline_rel, pristine.as_bytes())?;
        }
        next_files.insert(
            file.path.clone(),
            PortalFileState {
                ownership: match file.ownership {
                    BundleOwnership::Managed => "managed",
                    BundleOwnership::UserOwned => "user-owned",
                }
                .into(),
                pristine_sha256: if file.ownership == BundleOwnership::UserOwned {
                    old_file.map_or(pristine_hash, |state| state.pristine_sha256.clone())
                } else {
                    pristine_hash
                },
            },
        );
    }
    if let Some(old) = &previous {
        for (path, state) in &old.files {
            if next_files.contains_key(path) || state.ownership != "managed" {
                continue;
            }
            let dest_text = path_text(&relative_root.join(path));
            let dest = guard_beneath_root(repo_root, Path::new(&dest_text))?;
            let current = std::fs::read(&dest).ok();
            if current
                .as_deref()
                .is_some_and(|bytes| sha256_hex(bytes) == state.pristine_sha256)
            {
                remove_beneath_root(repo_root, &dest_text)?;
                report.file(dest_text, Action::Removed);
            } else if current.is_some() {
                report.file(dest_text, Action::KeptUserModified);
            }
        }
    }
    let state = PortalState {
        schema_version: 1,
        root: path_text(&relative_root),
        starter_version: manifest.version,
        files: next_files,
    };
    let encoded = serde_json::to_vec_pretty(&state)?;
    write_beneath_root(repo_root, STATE_PATH, &encoded)?;
    prune_unreferenced_baselines(repo_root, &state)?;
    Ok(report)
}

/// Reconcile an adopted portal during ordinary `codeflow update`.
pub fn update_adopted_portal(
    source: &dyn AssetSource,
    repo_root: &Path,
) -> Result<Option<Report>, ScaffoldError> {
    let Some(state) = load_state(repo_root)? else {
        return Ok(None);
    };
    setup_portal(source, repo_root, Path::new(&state.root)).map(Some)
}

fn reconcile_managed(
    root: &Path,
    dest_text: &str,
    dest: &Path,
    baseline_rel: Option<&str>,
    pristine: &str,
    old: Option<&PortalFileState>,
    report: &mut Report,
) -> Result<(), ScaffoldError> {
    let current = match std::fs::read_to_string(dest) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_beneath_root(root, dest_text, pristine.as_bytes())?;
            report.file(dest_text, Action::Added);
            return Ok(());
        }
        Err(error) => return Err(ScaffoldError::io(dest, error)),
    };
    if current == pristine {
        report.file(dest_text, Action::Unchanged);
        return Ok(());
    }
    if old.is_some_and(|state| sha256_hex(current.as_bytes()) == state.pristine_sha256) {
        write_beneath_root(root, dest_text, pristine.as_bytes())?;
        report.file(dest_text, Action::Changed);
        return Ok(());
    }
    if old.is_some_and(|state| state.pristine_sha256 == sha256_hex(pristine.as_bytes())) {
        report.file(dest_text, Action::KeptUserModified);
        return Ok(());
    }
    let base = match baseline_rel {
        Some(path) => super::state::read_beneath_root(root, path)?.unwrap_or_default(),
        None => String::new(),
    };
    let baseline_is_authentic =
        old.is_some_and(|state| sha256_hex(base.as_bytes()) == state.pristine_sha256);
    if !base.is_empty() && baseline_is_authentic {
        if let Ok(merged) = diffy::merge(&base, &current, pristine) {
            write_beneath_root(root, dest_text, merged.as_bytes())?;
            report.file(dest_text, Action::Merged);
            return Ok(());
        }
    }
    let conflict = format!(
        "{dest_text}.codeflow-{}.new",
        &sha256_hex(pristine.as_bytes())[..12]
    );
    let conflict_path = guard_beneath_root(root, Path::new(&conflict))?;
    if std::fs::symlink_metadata(&conflict_path).is_ok() {
        return Err(ScaffoldError::InvalidState {
            what: conflict,
            detail: "refusing to overwrite a pre-existing portal conflict sidecar".into(),
        });
    }
    write_beneath_root(root, &conflict, pristine.as_bytes())?;
    report.file_with_notes(
        dest_text,
        Action::Conflicted,
        vec![format!("new starter written to {conflict}")],
    );
    Ok(())
}

fn load_state(root: &Path) -> Result<Option<PortalState>, ScaffoldError> {
    let path = guard_beneath_root(root, Path::new(STATE_PATH))?;
    let bytes = match read_bounded_regular(&path, MAX_STATE_BYTES) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ScaffoldError::io(&path, error)),
    };
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(ScaffoldError::InvalidState {
            what: STATE_PATH.into(),
            detail: "state exceeds its byte limit".into(),
        });
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Err(ScaffoldError::InvalidState {
            what: STATE_PATH.into(),
            detail: "state is not UTF-8".into(),
        });
    };
    let state: PortalState = serde_json::from_str(&text)?;
    validate_state(&state)?;
    Ok(Some(state))
}

fn validate_state(state: &PortalState) -> Result<(), ScaffoldError> {
    if state.schema_version != 1 {
        return Err(ScaffoldError::InvalidState {
            what: STATE_PATH.into(),
            detail: "unsupported schema_version".into(),
        });
    }
    let root =
        validate_portal_root(Path::new(&state.root)).map_err(|_| ScaffoldError::InvalidState {
            what: STATE_PATH.into(),
            detail: "portal root is not a safe repository-relative path".into(),
        })?;
    if path_text(&root) != state.root || state.starter_version.trim().is_empty() {
        return Err(ScaffoldError::InvalidState {
            what: STATE_PATH.into(),
            detail: "portal root is not canonical or starter version is empty".into(),
        });
    }
    if state.files.len() > MAX_BUNDLE_FILES {
        return Err(ScaffoldError::InvalidState {
            what: STATE_PATH.into(),
            detail: "state contains too many portal files".into(),
        });
    }
    let mut portable_paths = BTreeSet::new();
    for (file, metadata) in &state.files {
        validate_asset_path(file).map_err(|_| ScaffoldError::InvalidState {
            what: STATE_PATH.into(),
            detail: format!("unsafe portal file state {file:?}"),
        })?;
        if !portable_paths.insert(portable_key(file))
            || !matches!(metadata.ownership.as_str(), "managed" | "user-owned")
            || metadata.pristine_sha256.len() != 64
            || !metadata
                .pristine_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ScaffoldError::InvalidState {
                what: STATE_PATH.into(),
                detail: format!("invalid portal file metadata {file:?}"),
            });
        }
    }
    Ok(())
}

fn baseline_path(pristine_sha256: &str) -> String {
    format!("{BASELINE_ROOT}/{pristine_sha256}")
}

fn prune_unreferenced_baselines(root: &Path, state: &PortalState) -> Result<(), ScaffoldError> {
    let keep: BTreeSet<&str> = state
        .files
        .values()
        .filter(|file| file.ownership == "managed")
        .map(|file| file.pristine_sha256.as_str())
        .collect();
    let directory = guard_beneath_root(root, Path::new(BASELINE_ROOT))?;
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ScaffoldError::io(&directory, error)),
    };
    for (index, entry) in entries.enumerate() {
        if index >= MAX_BASELINE_ENTRIES {
            return Err(ScaffoldError::InvalidState {
                what: BASELINE_ROOT.into(),
                detail: "baseline directory contains too many entries".into(),
            });
        }
        let entry = entry.map_err(|error| ScaffoldError::io(&directory, error))?;
        let kind = entry
            .file_type()
            .map_err(|error| ScaffoldError::io(entry.path(), error))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !kind.is_file() {
            return Err(ScaffoldError::InvalidState {
                what: BASELINE_ROOT.into(),
                detail: format!("baseline entry is not a regular file: {name}"),
            });
        }
        if name.len() == 64
            && name
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && !keep.contains(name.as_str())
        {
            remove_beneath_root(root, &format!("{BASELINE_ROOT}/{name}"))?;
        }
    }
    Ok(())
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn roots_equal(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn validate_portal_root(path: &Path) -> Result<PathBuf, ScaffoldError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(ScaffoldError::InvalidState {
            what: "portal path".into(),
            detail: "expected a non-empty repository-relative path without traversal".into(),
        });
    }
    let normalized: PathBuf = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part),
            Component::CurDir => None,
            _ => None,
        })
        .collect();
    if normalized.as_os_str().is_empty() {
        return Err(ScaffoldError::InvalidState {
            what: "portal path".into(),
            detail: "expected a named repository-relative directory".into(),
        });
    }
    let text = path_text(&normalized);
    if !text.split('/').all(portable_segment) {
        return Err(ScaffoldError::InvalidState {
            what: "portal path".into(),
            detail: "path is not portable across macOS, Linux, Windows, and WSL".into(),
        });
    }
    if text == ".git"
        || text.starts_with(".git/")
        || text == ".codeflow"
        || text.starts_with(".codeflow/")
    {
        return Err(ScaffoldError::InvalidState {
            what: "portal path".into(),
            detail: "reserved .git and .codeflow state cannot contain the portal".into(),
        });
    }
    Ok(normalized)
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
        && !segment.ends_with(' ')
        && !segment.ends_with('.')
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

fn portable_key(value: &str) -> String {
    value.nfc().collect::<String>().to_lowercase()
}

fn read_bounded_regular(path: &Path, maximum_bytes: u64) -> std::io::Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > maximum_bytes {
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
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file changed type while opening",
        ));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(maximum_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file grew beyond its byte limit",
        ));
    }
    Ok(bytes)
}

fn validate_asset_path(path: &str) -> Result<(), ScaffoldError> {
    validate_portal_root(Path::new(path))
        .map(|_| ())
        .map_err(|_| {
            ScaffoldError::ManifestInvalid(format!("unsafe docs portal asset path {path:?}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scaffold::DirSource;

    struct MapSource(BTreeMap<String, Vec<u8>>);

    impl AssetSource for MapSource {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
        }
    }

    fn source(version: &str, content: &str) -> MapSource {
        MapSource(BTreeMap::from([
            (
                MANIFEST_ASSET.into(),
                format!(
                    r#"{{"schema_version":1,"version":"{version}","files":[{{"path":"managed.txt","ownership":"managed"}}]}}"#
                )
                .into_bytes(),
            ),
            (
                format!("{ASSET_PREFIX}managed.txt"),
                content.as_bytes().to_vec(),
            ),
        ]))
    }

    fn initialized_root() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".codeflow")).unwrap();
        std::fs::write(
            temp.path().join(".codeflow/project.toml"),
            "schema_version = 1\n",
        )
        .unwrap();
        temp
    }

    #[test]
    fn setup_is_opt_in_idempotent_and_preserves_user_configuration() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".codeflow")).unwrap();
        std::fs::write(
            temp.path().join(".codeflow/project.toml"),
            "schema_version = 1\n",
        )
        .unwrap();
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let source = DirSource::new(assets);
        let first = setup_portal(&source, temp.path(), Path::new("guide")).unwrap();
        assert!(first
            .files
            .iter()
            .any(|file| file.dest == "guide/package-lock.json"));
        std::fs::write(
            temp.path().join("guide/portal.config.json"),
            "{\"mine\":true}\n",
        )
        .unwrap();
        let second = setup_portal(&source, temp.path(), Path::new("guide")).unwrap();
        assert_eq!(
            std::fs::read_to_string(temp.path().join("guide/portal.config.json")).unwrap(),
            "{\"mine\":true}\n"
        );
        assert!(!second.has_conflicts());
    }

    #[test]
    fn setup_refuses_escape_reserved_and_changed_roots() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".codeflow")).unwrap();
        std::fs::write(
            temp.path().join(".codeflow/project.toml"),
            "schema_version = 1\n",
        )
        .unwrap();
        let source = DirSource::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
        for path in [
            ".",
            "../guide",
            ".git/guide",
            ".codeflow/guide",
            "CON",
            "guide.",
            "guide:name",
            "cafe\u{301}",
        ] {
            assert!(setup_portal(&source, temp.path(), Path::new(path)).is_err());
        }
        setup_portal(&source, temp.path(), Path::new("./guide")).unwrap();
        assert!(setup_portal(&source, temp.path(), Path::new("other")).is_err());
    }

    #[test]
    fn setup_refuses_wrong_type_roots_unknown_manifest_fields_and_oversized_state() {
        let temp = initialized_root();
        std::fs::write(temp.path().join("guide"), "not a directory").unwrap();
        assert!(setup_portal(&source("1.0.0", "one\n"), temp.path(), Path::new("guide")).is_err());

        let temp = initialized_root();
        let invalid = MapSource(BTreeMap::from([(
            MANIFEST_ASSET.into(),
            br#"{"schema_version":1,"version":"1.0.0","files":[{"path":"managed.txt","ownership":"managed","extra":true}]}"#.to_vec(),
        )]));
        assert!(setup_portal(&invalid, temp.path(), Path::new("guide")).is_err());

        std::fs::write(
            temp.path().join(STATE_PATH),
            vec![b'x'; MAX_STATE_BYTES as usize + 1],
        )
        .unwrap();
        assert!(update_adopted_portal(&source("1.0.0", "one\n"), temp.path()).is_err());
    }

    #[test]
    fn conflict_sidecars_never_overwrite_preexisting_files() {
        let temp = initialized_root();
        setup_portal(&source("1.0.0", "old\n"), temp.path(), Path::new("guide")).unwrap();
        std::fs::write(temp.path().join("guide/managed.txt"), "user edit\n").unwrap();
        std::fs::write(
            temp.path().join(baseline_path(&sha256_hex(b"old\n"))),
            "corrupt\n",
        )
        .unwrap();
        let sidecar = temp.path().join(format!(
            "guide/managed.txt.codeflow-{}.new",
            &sha256_hex(b"new\n")[..12]
        ));
        std::fs::write(&sidecar, "user-owned\n").unwrap();
        assert!(setup_portal(&source("2.0.0", "new\n"), temp.path(), Path::new("guide")).is_err());
        assert_eq!(std::fs::read_to_string(sidecar).unwrap(), "user-owned\n");
    }

    #[test]
    fn update_does_not_materialize_a_portal_for_non_adopters() {
        let temp = tempfile::tempdir().unwrap();
        let source = DirSource::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
        assert!(update_adopted_portal(&source, temp.path())
            .unwrap()
            .is_none());
        assert!(!temp.path().join(BASELINE_ROOT).exists());
    }

    #[test]
    fn adopted_baselines_are_opaque_content_addressed_files() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".codeflow")).unwrap();
        std::fs::write(
            temp.path().join(".codeflow/project.toml"),
            "schema_version = 1\n",
        )
        .unwrap();
        let source = DirSource::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
        setup_portal(&source, temp.path(), Path::new("guide")).unwrap();
        let names: Vec<String> = std::fs::read_dir(temp.path().join(BASELINE_ROOT))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!names.is_empty());
        assert!(names
            .iter()
            .all(|name| name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit())));
        assert!(!temp
            .path()
            .join(BASELINE_ROOT)
            .join("package-lock.json")
            .exists());
    }

    #[test]
    fn corrupted_baseline_never_authorizes_overwrite_or_merge() {
        let temp = initialized_root();
        let first = source("1.0.0", "old\n");
        setup_portal(&first, temp.path(), Path::new("guide")).unwrap();
        std::fs::write(temp.path().join("guide/managed.txt"), "user edit\n").unwrap();
        std::fs::write(
            temp.path().join(baseline_path(&sha256_hex(b"old\n"))),
            "corrupt\n",
        )
        .unwrap();

        let report =
            setup_portal(&source("2.0.0", "new\n"), temp.path(), Path::new("guide")).unwrap();
        assert!(report.has_conflicts());
        assert_eq!(
            std::fs::read_to_string(temp.path().join("guide/managed.txt")).unwrap(),
            "user edit\n"
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join(format!(
                "guide/managed.txt.codeflow-{}.new",
                &sha256_hex(b"new\n")[..12]
            )))
            .unwrap(),
            "new\n"
        );
    }

    #[test]
    fn successful_upgrade_prunes_unreferenced_baselines() {
        let temp = initialized_root();
        setup_portal(&source("1.0.0", "old\n"), temp.path(), Path::new("guide")).unwrap();
        setup_portal(&source("2.0.0", "new\n"), temp.path(), Path::new("guide")).unwrap();
        let names: BTreeSet<String> = std::fs::read_dir(temp.path().join(BASELINE_ROOT))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, BTreeSet::from([sha256_hex(b"new\n")]));
    }

    #[test]
    fn forged_adoption_state_fails_before_reconciliation() {
        let temp = initialized_root();
        std::fs::write(
            temp.path().join(STATE_PATH),
            r#"{"schema_version":1,"root":"../escape","starter_version":"1.0.0","files":{}}"#,
        )
        .unwrap();
        assert!(update_adopted_portal(&source("1.0.0", "old\n"), temp.path()).is_err());
        assert!(!temp.path().join("escape").exists());
    }
}
