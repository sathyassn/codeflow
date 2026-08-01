//! Opt-in documentation-portal starter materialization and reconciliation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::report::{Action, Report};
use super::state::{guard_beneath_root, remove_beneath_root, write_beneath_root};
use super::{sha256_hex, AssetSource, ScaffoldError};

const ASSET_PREFIX: &str = "docs-portal/starter/";
const MANIFEST_ASSET: &str = "docs-portal/manifest.json";
const STATE_PATH: &str = ".codeflow/docs-portal.json";
const BASELINE_ROOT: &str = ".codeflow/.docs-portal-baseline";

#[derive(Debug, Deserialize)]
struct BundleManifest {
    schema_version: u32,
    version: String,
    files: Vec<BundleFile>,
}

#[derive(Debug, Deserialize)]
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
    files: BTreeMap<String, PortalFileState>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortalFileState {
    ownership: String,
    pristine_sha256: String,
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
    let manifest: BundleManifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.schema_version != 1 || manifest.files.is_empty() {
        return Err(ScaffoldError::ManifestInvalid(
            "docs portal manifest must use schema 1 and contain files".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    for file in &manifest.files {
        validate_asset_path(&file.path)?;
        if !seen.insert(file.path.as_str()) {
            return Err(ScaffoldError::ManifestInvalid(format!(
                "duplicate docs portal path {:?}",
                file.path
            )));
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
    let conflict = format!("{dest_text}.new");
    write_beneath_root(root, &conflict, pristine.as_bytes())?;
    report.file_with_notes(
        dest_text,
        Action::Conflicted,
        vec![format!("new starter written to {conflict}")],
    );
    Ok(())
}

fn load_state(root: &Path) -> Result<Option<PortalState>, ScaffoldError> {
    let Some(text) = super::state::read_beneath_root(root, STATE_PATH)? else {
        return Ok(None);
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
    for (file, metadata) in &state.files {
        validate_asset_path(file).map_err(|_| ScaffoldError::InvalidState {
            what: STATE_PATH.into(),
            detail: format!("unsafe portal file state {file:?}"),
        })?;
        if !matches!(metadata.ownership.as_str(), "managed" | "user-owned")
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
    for entry in entries {
        let entry = entry.map_err(|error| ScaffoldError::io(&directory, error))?;
        let kind = entry
            .file_type()
            .map_err(|error| ScaffoldError::io(entry.path(), error))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_file()
            && name.len() == 64
            && name.bytes().all(|byte| byte.is_ascii_hexdigit())
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
        for path in [".", "../guide", ".git/guide", ".codeflow/guide"] {
            assert!(setup_portal(&source, temp.path(), Path::new(path)).is_err());
        }
        setup_portal(&source, temp.path(), Path::new("./guide")).unwrap();
        assert!(setup_portal(&source, temp.path(), Path::new("other")).is_err());
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
            std::fs::read_to_string(temp.path().join("guide/managed.txt.new")).unwrap(),
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
