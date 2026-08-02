//! Opt-in documentation-portal starter materialization and reconciliation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;
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
const TRANSACTION_LOCK_PATH: &str = ".codeflow/.docs-portal.lock";
const TRANSACTION_PATH: &str = ".codeflow/.docs-portal-transaction";
const TRANSACTION_STAGE_PREFIX: &str = ".docs-portal-transaction-stage-";
const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_BUNDLE_FILES: usize = 256;
const MAX_STATE_BYTES: u64 = 64 * 1024;
const MAX_BASELINE_ENTRIES: usize = 1_024;
const MAX_MANAGED_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RECONCILIATION_READ_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
struct ReadBudget {
    remaining: u64,
}

#[derive(Clone, Debug)]
struct PortalMutation {
    path: String,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

#[derive(Clone)]
struct PortalMutationPlan {
    mutations: BTreeMap<String, PortalMutation>,
    snapshot_budget: ReadBudget,
    output_budget: ReadBudget,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PortalTransactionJournal {
    schema_version: u32,
    mutations: Vec<PortalJournalMutation>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PortalJournalMutation {
    path: String,
    before_sha256: Option<String>,
    before_bytes: Option<u64>,
    after_sha256: Option<String>,
    after_bytes: Option<u64>,
    staged_file: Option<String>,
}

#[derive(Debug)]
struct PortalTransactionLease {
    file: File,
}

impl Drop for PortalTransactionLease {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

impl Default for PortalMutationPlan {
    fn default() -> Self {
        Self {
            mutations: BTreeMap::new(),
            snapshot_budget: ReadBudget::new(),
            output_budget: ReadBudget::new(),
        }
    }
}

struct PortalReconciliation<'a> {
    root: &'a Path,
    read_budget: &'a mut ReadBudget,
    mutations: &'a mut PortalMutationPlan,
    report: &'a mut Report,
}

impl PortalMutationPlan {
    #[cfg(test)]
    fn with_limits(snapshot_limit: u64, output_limit: u64) -> Self {
        Self {
            mutations: BTreeMap::new(),
            snapshot_budget: ReadBudget::with_limit(snapshot_limit),
            output_budget: ReadBudget::with_limit(output_limit),
        }
    }

    fn write(
        &mut self,
        root: &Path,
        path: &str,
        bytes: Vec<u8>,
        maximum: u64,
        label: &str,
    ) -> Result<(), ScaffoldError> {
        let before = snapshot_project_file(root, path, maximum, label)?;
        self.insert(PortalMutation {
            path: path.into(),
            before,
            after: Some(bytes),
        })
    }

    fn remove(
        &mut self,
        root: &Path,
        path: &str,
        maximum: u64,
        label: &str,
    ) -> Result<(), ScaffoldError> {
        let before = snapshot_project_file(root, path, maximum, label)?;
        self.insert(PortalMutation {
            path: path.into(),
            before,
            after: None,
        })
    }

    fn insert(&mut self, mutation: PortalMutation) -> Result<(), ScaffoldError> {
        match self.mutations.get(&mutation.path) {
            Some(existing) if existing.after == mutation.after => Ok(()),
            Some(_) => Err(ScaffoldError::InvalidState {
                what: mutation.path,
                detail: "portal transaction planned conflicting mutations".into(),
            }),
            None => {
                let mut snapshot_budget = self.snapshot_budget;
                let mut output_budget = self.output_budget;
                if let Some(before) = &mutation.before {
                    snapshot_budget.charge(before.len()).map_err(|error| {
                        ScaffoldError::InvalidState {
                            what: mutation.path.clone(),
                            detail: format!(
                                "portal transaction snapshots exceed their aggregate byte limit: {error}"
                            ),
                        }
                    })?;
                }
                if let Some(after) = &mutation.after {
                    output_budget.charge(after.len()).map_err(|error| {
                        ScaffoldError::InvalidState {
                            what: mutation.path.clone(),
                            detail: format!(
                                "portal transaction outputs exceed their aggregate byte limit: {error}"
                            ),
                        }
                    })?;
                }
                self.snapshot_budget = snapshot_budget;
                self.output_budget = output_budget;
                self.mutations.insert(mutation.path.clone(), mutation);
                Ok(())
            }
        }
    }

    fn commit(self, root: &Path) -> Result<(), ScaffoldError> {
        self.commit_inner(root, None)
    }

    #[cfg(test)]
    fn commit_with_fault_after(
        self,
        root: &Path,
        mutation_index: usize,
    ) -> Result<(), ScaffoldError> {
        self.commit_inner(root, Some(mutation_index))
    }

    fn commit_inner(self, root: &Path, fault_after: Option<usize>) -> Result<(), ScaffoldError> {
        for mutation in self.mutations.values() {
            ensure_snapshot_unchanged(
                root,
                &mutation.path,
                mutation.before.as_deref(),
                "pre-commit",
            )?;
        }
        let ordered = ordered_mutations(&self.mutations);
        let missing_directories = missing_parent_directories(root, ordered.iter().copied())?;
        prepare_portal_transaction(root, &ordered)?;
        let mut applied = Vec::new();
        for (index, mutation) in ordered.into_iter().enumerate() {
            if let Err(error) = ensure_snapshot_unchanged(
                root,
                &mutation.path,
                mutation.before.as_deref(),
                "pre-apply",
            ) {
                rollback_mutations(root, &applied, &missing_directories)?;
                remove_portal_transaction(root)?;
                return Err(error);
            }
            // Atomic writes may report a durability-sync failure after rename;
            // include the in-flight path in rollback even when apply returns Err.
            applied.push(mutation);
            let result = match &mutation.after {
                Some(bytes) => write_beneath_root(root, &mutation.path, bytes),
                None => remove_beneath_root(root, &mutation.path),
            };
            let result = result.and_then(|()| {
                if fault_after == Some(index) {
                    Err(ScaffoldError::InvalidState {
                        what: mutation.path.clone(),
                        detail: "injected portal transaction failure".into(),
                    })
                } else {
                    Ok(())
                }
            });
            if let Err(error) = result {
                rollback_mutations(root, &applied, &missing_directories).map_err(
                    |rollback_error| ScaffoldError::InvalidState {
                        what: "docs portal transaction".into(),
                        detail: format!(
                            "commit failed ({error}); rollback also failed ({rollback_error})"
                        ),
                    },
                )?;
                remove_portal_transaction(root)?;
                return Err(error);
            }
        }
        remove_portal_transaction(root)?;
        Ok(())
    }

    #[cfg(test)]
    fn commit_with_abrupt_fault_after(
        self,
        root: &Path,
        mutation_index: usize,
    ) -> Result<(), ScaffoldError> {
        for mutation in self.mutations.values() {
            ensure_snapshot_unchanged(
                root,
                &mutation.path,
                mutation.before.as_deref(),
                "pre-commit",
            )?;
        }
        let ordered = ordered_mutations(&self.mutations);
        prepare_portal_transaction(root, &ordered)?;
        for (index, mutation) in ordered.into_iter().enumerate() {
            match &mutation.after {
                Some(bytes) => write_beneath_root(root, &mutation.path, bytes)?,
                None => remove_beneath_root(root, &mutation.path)?,
            }
            if index == mutation_index {
                return Err(ScaffoldError::InvalidState {
                    what: mutation.path.clone(),
                    detail: "injected abrupt portal transaction interruption".into(),
                });
            }
        }
        remove_portal_transaction(root)
    }
}

fn ordered_mutations(mutations: &BTreeMap<String, PortalMutation>) -> Vec<&PortalMutation> {
    let mut ordered: Vec<_> = mutations.values().collect();
    ordered.sort_by(|left, right| {
        mutation_rank(left)
            .cmp(&mutation_rank(right))
            .then_with(|| left.path.cmp(&right.path))
    });
    ordered
}

fn mutation_rank(mutation: &PortalMutation) -> u8 {
    if mutation.path == STATE_PATH {
        2
    } else if mutation.path.starts_with(&format!("{BASELINE_ROOT}/")) {
        if mutation.after.is_some() {
            1
        } else {
            3
        }
    } else {
        0
    }
}

fn acquire_portal_transaction_lease(root: &Path) -> Result<PortalTransactionLease, ScaffoldError> {
    let path = guard_beneath_root(root, Path::new(TRANSACTION_LOCK_PATH))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| ScaffoldError::io(parent, error))?;
    }
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ScaffoldError::InvalidState {
                what: TRANSACTION_LOCK_PATH.into(),
                detail: "portal transaction lock is not a regular file".into(),
            });
        }
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
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
    let file = options
        .open(&path)
        .map_err(|error| ScaffoldError::io(&path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| ScaffoldError::io(&path, error))?;
    if !metadata.is_file() {
        return Err(ScaffoldError::InvalidState {
            what: TRANSACTION_LOCK_PATH.into(),
            detail: "portal transaction lock is not a regular file".into(),
        });
    }
    file.try_lock_exclusive()
        .map_err(|error| ScaffoldError::InvalidState {
            what: TRANSACTION_LOCK_PATH.into(),
            detail: format!("another portal setup/update is already in progress: {error}"),
        })?;
    Ok(PortalTransactionLease { file })
}

fn prepare_portal_transaction(
    root: &Path,
    mutations: &[&PortalMutation],
) -> Result<(), ScaffoldError> {
    let transaction = guard_beneath_root(root, Path::new(TRANSACTION_PATH))?;
    if transaction.exists() {
        return Err(ScaffoldError::InvalidState {
            what: TRANSACTION_PATH.into(),
            detail: "an unrecovered portal transaction already exists".into(),
        });
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage_relative = format!(
        ".codeflow/{TRANSACTION_STAGE_PREFIX}{}-{nonce}",
        std::process::id()
    );
    let stage = guard_beneath_root(root, Path::new(&stage_relative))?;
    std::fs::create_dir(&stage).map_err(|error| ScaffoldError::io(&stage, error))?;
    let prepared = (|| {
        let mut journal = PortalTransactionJournal {
            schema_version: 1,
            mutations: Vec::with_capacity(mutations.len()),
        };
        for (index, mutation) in mutations.iter().enumerate() {
            let staged_file = mutation.after.as_ref().map(|bytes| {
                let relative = format!("after/{index:04}");
                (relative, bytes)
            });
            if let Some((relative, bytes)) = &staged_file {
                write_beneath_root(root, &format!("{stage_relative}/{relative}"), bytes)?;
            }
            journal.mutations.push(PortalJournalMutation {
                path: mutation.path.clone(),
                before_sha256: mutation.before.as_ref().map(|bytes| sha256_hex(bytes)),
                before_bytes: mutation.before.as_ref().map(|bytes| bytes.len() as u64),
                after_sha256: mutation.after.as_ref().map(|bytes| sha256_hex(bytes)),
                after_bytes: mutation.after.as_ref().map(|bytes| bytes.len() as u64),
                staged_file: staged_file.map(|(relative, _)| relative),
            });
        }
        let manifest = serde_json::to_vec_pretty(&journal)?;
        write_beneath_root(root, &format!("{stage_relative}/manifest.json"), &manifest)?;
        std::fs::rename(&stage, &transaction)
            .map_err(|error| ScaffoldError::io(&transaction, error))?;
        sync_parent(&transaction)?;
        Ok(())
    })();
    if prepared.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    prepared
}

fn recover_portal_transaction(root: &Path) -> Result<(), ScaffoldError> {
    remove_abandoned_transaction_stages(root)?;
    let transaction = guard_beneath_root(root, Path::new(TRANSACTION_PATH))?;
    let metadata = match std::fs::symlink_metadata(&transaction) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ScaffoldError::io(&transaction, error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ScaffoldError::InvalidState {
            what: TRANSACTION_PATH.into(),
            detail: "portal transaction journal is not a regular directory".into(),
        });
    }
    let manifest_path = transaction.join("manifest.json");
    let manifest = read_bounded_regular(&manifest_path, 512 * 1024).map_err(|error| {
        ScaffoldError::InvalidState {
            what: TRANSACTION_PATH.into(),
            detail: format!("portal transaction journal is unreadable: {error}"),
        }
    })?;
    let journal: PortalTransactionJournal = serde_json::from_slice(&manifest)?;
    if journal.schema_version != 1
        || journal.mutations.is_empty()
        || journal.mutations.len() > 2_048
    {
        return Err(ScaffoldError::InvalidState {
            what: TRANSACTION_PATH.into(),
            detail: "portal transaction journal has an invalid envelope".into(),
        });
    }
    let mut paths = BTreeSet::new();
    let mut bytes_remaining = MAX_RECONCILIATION_READ_BYTES;
    for mutation in &journal.mutations {
        validate_journal_mutation(root, mutation, &mut paths, &mut bytes_remaining)?;
        let maximum = mutation
            .before_bytes
            .into_iter()
            .chain(mutation.after_bytes)
            .max()
            .unwrap_or(0);
        let current = snapshot_project_file(root, &mutation.path, maximum, "transaction recovery")
            .map_err(|error| ScaffoldError::InvalidState {
                what: mutation.path.clone(),
                detail: format!(
                    "portal transaction recovery found a concurrent edit; preserving it: {error}"
                ),
            })?;
        if snapshot_matches(
            current.as_deref(),
            mutation.after_bytes,
            mutation.after_sha256.as_deref(),
        ) {
            continue;
        }
        if !snapshot_matches(
            current.as_deref(),
            mutation.before_bytes,
            mutation.before_sha256.as_deref(),
        ) {
            return Err(ScaffoldError::InvalidState {
                what: mutation.path.clone(),
                detail: "portal transaction recovery found a concurrent edit; preserving it".into(),
            });
        }
        match (
            &mutation.staged_file,
            mutation.after_bytes,
            mutation.after_sha256.as_deref(),
        ) {
            (Some(staged), Some(expected_bytes), Some(expected_hash)) => {
                let staged_path =
                    guard_beneath_root(root, &Path::new(TRANSACTION_PATH).join(staged))?;
                let bytes =
                    read_bounded_regular(&staged_path, expected_bytes).map_err(|error| {
                        ScaffoldError::InvalidState {
                            what: mutation.path.clone(),
                            detail: format!(
                                "staged portal transaction output is unreadable: {error}"
                            ),
                        }
                    })?;
                if bytes.len() as u64 != expected_bytes || sha256_hex(&bytes) != expected_hash {
                    return Err(ScaffoldError::InvalidState {
                        what: mutation.path.clone(),
                        detail: "staged portal transaction output does not match its journal"
                            .into(),
                    });
                }
                write_beneath_root(root, &mutation.path, &bytes)?;
            }
            (None, None, None) => remove_beneath_root(root, &mutation.path)?,
            _ => {
                return Err(ScaffoldError::InvalidState {
                    what: mutation.path.clone(),
                    detail: "portal transaction journal mutation is inconsistent".into(),
                });
            }
        }
    }
    remove_portal_transaction(root)
}

fn validate_journal_mutation(
    root: &Path,
    mutation: &PortalJournalMutation,
    paths: &mut BTreeSet<String>,
    bytes_remaining: &mut u64,
) -> Result<(), ScaffoldError> {
    if mutation.path.len() > 4_096 || mutation.path.contains('\\') {
        return Err(ScaffoldError::InvalidState {
            what: mutation.path.clone(),
            detail: "portal transaction journal has an unsafe destination".into(),
        });
    }
    guard_beneath_root(root, Path::new(&mutation.path))?;
    if !paths.insert(portable_key(&mutation.path)) {
        return Err(ScaffoldError::InvalidState {
            what: mutation.path.clone(),
            detail: "portal transaction journal repeats a destination".into(),
        });
    }
    for (size, hash) in [
        (mutation.before_bytes, mutation.before_sha256.as_deref()),
        (mutation.after_bytes, mutation.after_sha256.as_deref()),
    ] {
        if size.is_some() != hash.is_some()
            || hash.is_some_and(|value| {
                value.len() != 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
        {
            return Err(ScaffoldError::InvalidState {
                what: mutation.path.clone(),
                detail: "portal transaction journal has an invalid snapshot claim".into(),
            });
        }
        if let Some(size) = size {
            *bytes_remaining =
                bytes_remaining
                    .checked_sub(size)
                    .ok_or_else(|| ScaffoldError::InvalidState {
                        what: TRANSACTION_PATH.into(),
                        detail: "portal transaction journal exceeds its aggregate byte limit"
                            .into(),
                    })?;
        }
    }
    if mutation.after_bytes.is_some() != mutation.staged_file.is_some()
        || mutation
            .staged_file
            .as_deref()
            .is_some_and(|path| !path.starts_with("after/") || validate_asset_path(path).is_err())
    {
        return Err(ScaffoldError::InvalidState {
            what: mutation.path.clone(),
            detail: "portal transaction journal has an invalid staged output".into(),
        });
    }
    Ok(())
}

fn snapshot_matches(snapshot: Option<&[u8]>, bytes: Option<u64>, hash: Option<&str>) -> bool {
    match (snapshot, bytes, hash) {
        (None, None, None) => true,
        (Some(value), Some(bytes), Some(hash)) => {
            value.len() as u64 == bytes && sha256_hex(value) == hash
        }
        _ => false,
    }
}

fn remove_portal_transaction(root: &Path) -> Result<(), ScaffoldError> {
    let transaction = guard_beneath_root(root, Path::new(TRANSACTION_PATH))?;
    match std::fs::symlink_metadata(&transaction) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(ScaffoldError::InvalidState {
                what: TRANSACTION_PATH.into(),
                detail: "portal transaction journal is not a regular directory".into(),
            })
        }
        Ok(_) => {
            std::fs::remove_dir_all(&transaction)
                .map_err(|error| ScaffoldError::io(&transaction, error))?;
            sync_parent(&transaction)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ScaffoldError::io(&transaction, error)),
    }
}

fn remove_abandoned_transaction_stages(root: &Path) -> Result<(), ScaffoldError> {
    let codeflow = guard_beneath_root(root, Path::new(".codeflow"))?;
    let entries =
        std::fs::read_dir(&codeflow).map_err(|error| ScaffoldError::io(&codeflow, error))?;
    let mut seen = 0_usize;
    for entry in entries {
        let entry = entry.map_err(|error| ScaffoldError::io(&codeflow, error))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(TRANSACTION_STAGE_PREFIX) {
            continue;
        }
        seen += 1;
        if seen > 64 {
            return Err(ScaffoldError::InvalidState {
                what: ".codeflow".into(),
                detail: "too many abandoned portal transaction stages".into(),
            });
        }
        let metadata = entry
            .metadata()
            .map_err(|error| ScaffoldError::io(entry.path(), error))?;
        if entry
            .file_type()
            .map_err(|error| ScaffoldError::io(entry.path(), error))?
            .is_symlink()
            || !metadata.is_dir()
        {
            return Err(ScaffoldError::InvalidState {
                what: path_text(&entry.path()),
                detail: "abandoned portal transaction stage is not a regular directory".into(),
            });
        }
        std::fs::remove_dir_all(entry.path())
            .map_err(|error| ScaffoldError::io(entry.path(), error))?;
    }
    Ok(())
}

fn sync_parent(path: &Path) -> Result<(), ScaffoldError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    match File::open(parent).and_then(|directory| directory.sync_all()) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(ScaffoldError::io(parent, error)),
    }
}

impl ReadBudget {
    fn new() -> Self {
        Self::with_limit(MAX_RECONCILIATION_READ_BYTES)
    }

    fn with_limit(limit: u64) -> Self {
        Self { remaining: limit }
    }

    fn charge(&mut self, bytes: usize) -> std::io::Result<()> {
        let bytes = u64::try_from(bytes).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "file length overflow")
        })?;
        self.remaining = self.remaining.checked_sub(bytes).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "portal reconciliation read budget exceeded",
            )
        })?;
        Ok(())
    }
}

fn charge_bundle_asset(
    budget: &mut ReadBudget,
    path: &str,
    asset: &[u8],
) -> Result<(), ScaffoldError> {
    if asset.len() as u64 > MAX_MANAGED_FILE_BYTES {
        return Err(ScaffoldError::ManifestInvalid(format!(
            "{path} exceeds the managed portal file byte limit"
        )));
    }
    budget.charge(asset.len()).map_err(|error| {
        ScaffoldError::ManifestInvalid(format!(
            "docs portal bundle exceeds its aggregate byte limit: {error}"
        ))
    })
}

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
///
/// # Errors
///
/// Returns an error when the repository is not initialized, the selected path
/// is unsafe, the embedded bundle/state is invalid, or a managed update cannot
/// be applied without violating the never-clobber contract.
#[allow(clippy::too_many_lines)] // One transactional reconciliation pipeline; splitting would obscure rollback/state ordering.
pub fn setup_portal(
    source: &dyn AssetSource,
    repo_root: &Path,
    portal_root: &Path,
) -> Result<Report, ScaffoldError> {
    if !repo_root.join(".codeflow/project.toml").exists() {
        return Err(ScaffoldError::NotInitialized);
    }
    let _lease = acquire_portal_transaction_lease(repo_root)?;
    recover_portal_transaction(repo_root)?;
    setup_portal_locked(source, repo_root, portal_root)
}

#[allow(clippy::too_many_lines)]
fn setup_portal_locked(
    source: &dyn AssetSource,
    repo_root: &Path,
    portal_root: &Path,
) -> Result<Report, ScaffoldError> {
    let relative_root = validate_portal_root(portal_root)?;
    guard_beneath_root(repo_root, &relative_root)?;
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
    let mut read_budget = ReadBudget::new();
    let mut bundle_budget = ReadBudget::new();
    let mut mutations = PortalMutationPlan::default();
    let mut next_files = BTreeMap::new();
    for file in &manifest.files {
        let asset = source
            .read(&format!("{ASSET_PREFIX}{}", file.path))
            .ok_or_else(|| {
                ScaffoldError::ManifestMissing(format!("{ASSET_PREFIX}{}", file.path))
            })?;
        charge_bundle_asset(&mut bundle_budget, &file.path, &asset)?;
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
                mutations.write(
                    repo_root,
                    &dest_text,
                    pristine.as_bytes().to_vec(),
                    MAX_MANAGED_FILE_BYTES,
                    "user-owned portal destination",
                )?;
                report.file(dest_text, Action::Created);
            }
        } else {
            reconcile_managed(
                &mut PortalReconciliation {
                    root: repo_root,
                    read_budget: &mut read_budget,
                    mutations: &mut mutations,
                    report: &mut report,
                },
                &dest_text,
                prior_baseline_rel.as_deref(),
                &pristine,
                old_file,
            )?;
            mutations.write(
                repo_root,
                &baseline_rel,
                pristine.as_bytes().to_vec(),
                MAX_MANAGED_FILE_BYTES,
                "portal baseline destination",
            )?;
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
            let current = read_project_file(
                repo_root,
                &dest_text,
                "retired managed portal file",
                &mut read_budget,
            )?;
            if current
                .as_deref()
                .is_some_and(|bytes| sha256_hex(bytes) == state.pristine_sha256)
            {
                mutations.remove(
                    repo_root,
                    &dest_text,
                    MAX_MANAGED_FILE_BYTES,
                    "retired managed portal file",
                )?;
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
    mutations.write(
        repo_root,
        STATE_PATH,
        encoded,
        MAX_STATE_BYTES,
        "portal adoption state",
    )?;
    plan_unreferenced_baseline_pruning(repo_root, &state, &mut mutations)?;
    mutations.commit(repo_root)?;
    Ok(report)
}

/// Reconcile an adopted portal during ordinary `codeflow update`.
///
/// # Errors
///
/// Returns an error when adoption state is invalid or reconciliation fails.
pub fn update_adopted_portal(
    source: &dyn AssetSource,
    repo_root: &Path,
) -> Result<Option<Report>, ScaffoldError> {
    if !repo_root.join(".codeflow/project.toml").exists() {
        return Ok(None);
    }
    let _lease = acquire_portal_transaction_lease(repo_root)?;
    recover_portal_transaction(repo_root)?;
    let Some(state) = load_state(repo_root)? else {
        return Ok(None);
    };
    setup_portal_locked(source, repo_root, Path::new(&state.root)).map(Some)
}

fn reconcile_managed(
    context: &mut PortalReconciliation<'_>,
    dest_text: &str,
    baseline_rel: Option<&str>,
    pristine: &str,
    old: Option<&PortalFileState>,
) -> Result<(), ScaffoldError> {
    let Some(current_bytes) = read_project_file(
        context.root,
        dest_text,
        "managed portal file",
        context.read_budget,
    )?
    else {
        context.mutations.write(
            context.root,
            dest_text,
            pristine.as_bytes().to_vec(),
            MAX_MANAGED_FILE_BYTES,
            "managed portal destination",
        )?;
        context.report.file(dest_text, Action::Added);
        return Ok(());
    };
    let current =
        String::from_utf8(current_bytes).map_err(|error| ScaffoldError::InvalidState {
            what: dest_text.into(),
            detail: format!("managed portal file is not UTF-8: {error}"),
        })?;
    if current == pristine {
        context.report.file(dest_text, Action::Unchanged);
        return Ok(());
    }
    if old.is_some_and(|state| sha256_hex(current.as_bytes()) == state.pristine_sha256) {
        context.mutations.write(
            context.root,
            dest_text,
            pristine.as_bytes().to_vec(),
            MAX_MANAGED_FILE_BYTES,
            "managed portal destination",
        )?;
        context.report.file(dest_text, Action::Changed);
        return Ok(());
    }
    if old.is_some_and(|state| state.pristine_sha256 == sha256_hex(pristine.as_bytes())) {
        context.report.file(dest_text, Action::KeptUserModified);
        return Ok(());
    }
    let base = match baseline_rel {
        Some(path) => {
            read_project_file(context.root, path, "portal baseline", context.read_budget)?
                .map(String::from_utf8)
                .transpose()
                .map_err(|error| ScaffoldError::InvalidState {
                    what: path.into(),
                    detail: format!("portal baseline is not UTF-8: {error}"),
                })?
                .unwrap_or_default()
        }
        None => String::new(),
    };
    let baseline_is_authentic =
        old.is_some_and(|state| sha256_hex(base.as_bytes()) == state.pristine_sha256);
    if !base.is_empty() && baseline_is_authentic {
        if let Ok(merged) = diffy::merge(&base, &current, pristine) {
            context.mutations.write(
                context.root,
                dest_text,
                merged.into_bytes(),
                MAX_MANAGED_FILE_BYTES,
                "managed portal destination",
            )?;
            context.report.file(dest_text, Action::Merged);
            return Ok(());
        }
    }
    let conflict = format!(
        "{dest_text}.codeflow-{}.new",
        &sha256_hex(pristine.as_bytes())[..12]
    );
    let conflict_path = guard_beneath_root(context.root, Path::new(&conflict))?;
    if std::fs::symlink_metadata(&conflict_path).is_ok() {
        return Err(ScaffoldError::InvalidState {
            what: conflict,
            detail: "refusing to overwrite a pre-existing portal conflict sidecar".into(),
        });
    }
    context.mutations.write(
        context.root,
        &conflict,
        pristine.as_bytes().to_vec(),
        MAX_MANAGED_FILE_BYTES,
        "portal conflict sidecar",
    )?;
    context.report.file_with_notes(
        dest_text,
        Action::Conflicted,
        vec![format!("new starter written to {conflict}")],
    );
    Ok(())
}

fn read_project_file(
    root: &Path,
    relative: &str,
    label: &str,
    budget: &mut ReadBudget,
) -> Result<Option<Vec<u8>>, ScaffoldError> {
    let path = guard_beneath_root(root, Path::new(relative))?;
    let maximum = MAX_MANAGED_FILE_BYTES.min(budget.remaining);
    match read_bounded_regular(&path, maximum) {
        Ok(bytes) => {
            budget
                .charge(bytes.len())
                .map_err(|error| ScaffoldError::InvalidState {
                    what: relative.into(),
                    detail: format!("{label} exceeds the reconciliation read budget: {error}"),
                })?;
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ScaffoldError::InvalidState {
            what: relative.into(),
            detail: format!(
                "{label} is not a stable regular file or exceeds {maximum} bytes: {error}"
            ),
        }),
    }
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

fn plan_unreferenced_baseline_pruning(
    root: &Path,
    state: &PortalState,
    mutations: &mut PortalMutationPlan,
) -> Result<(), ScaffoldError> {
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
            mutations.remove(
                root,
                &format!("{BASELINE_ROOT}/{name}"),
                MAX_MANAGED_FILE_BYTES,
                "unreferenced portal baseline",
            )?;
        }
    }
    Ok(())
}

fn snapshot_project_file(
    root: &Path,
    relative: &str,
    maximum: u64,
    label: &str,
) -> Result<Option<Vec<u8>>, ScaffoldError> {
    let path = guard_beneath_root(root, Path::new(relative))?;
    match read_bounded_regular(&path, maximum) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ScaffoldError::InvalidState {
            what: relative.into(),
            detail: format!(
                "{label} is not a stable regular file or exceeds {maximum} bytes: {error}"
            ),
        }),
    }
}

fn ensure_snapshot_unchanged(
    root: &Path,
    relative: &str,
    expected: Option<&[u8]>,
    phase: &str,
) -> Result<(), ScaffoldError> {
    let maximum = expected.map_or(0, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
    let current = snapshot_project_file(root, relative, maximum, phase).map_err(|error| {
        ScaffoldError::InvalidState {
            what: relative.into(),
            detail: format!(
                "portal transaction {phase} snapshot changed; refusing to overwrite concurrent edits: {error}"
            ),
        }
    })?;
    if current.as_deref() == expected {
        Ok(())
    } else {
        Err(ScaffoldError::InvalidState {
            what: relative.into(),
            detail: format!(
                "portal transaction {phase} snapshot changed; refusing to overwrite concurrent edits"
            ),
        })
    }
}

fn missing_parent_directories<'a>(
    root: &Path,
    mutations: impl Iterator<Item = &'a PortalMutation>,
) -> Result<Vec<String>, ScaffoldError> {
    let mut missing = BTreeSet::new();
    for mutation in mutations.filter(|mutation| mutation.after.is_some()) {
        let relative = Path::new(&mutation.path);
        for ancestor in relative.ancestors().skip(1) {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            let destination = guard_beneath_root(root, ancestor)?;
            match std::fs::symlink_metadata(&destination) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) => {
                    return Err(ScaffoldError::InvalidState {
                        what: path_text(ancestor),
                        detail: "portal transaction parent is not a regular directory".into(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing.insert(path_text(ancestor));
                }
                Err(error) => return Err(ScaffoldError::io(&destination, error)),
            }
        }
    }
    let mut missing: Vec<_> = missing.into_iter().collect();
    missing.sort_by_key(|path| path.matches('/').count());
    Ok(missing)
}

fn rollback_mutations(
    root: &Path,
    applied: &[&PortalMutation],
    missing_directories: &[String],
) -> Result<(), ScaffoldError> {
    for mutation in applied.iter().rev() {
        let maximum = mutation
            .before
            .as_ref()
            .into_iter()
            .chain(mutation.after.as_ref())
            .map(Vec::len)
            .max()
            .and_then(|bytes| u64::try_from(bytes).ok())
            .unwrap_or(0);
        let current = snapshot_project_file(root, &mutation.path, maximum, "rollback")?;
        if current == mutation.before {
            continue;
        }
        if current != mutation.after {
            return Err(ScaffoldError::InvalidState {
                what: mutation.path.clone(),
                detail: "portal transaction rollback found a concurrent edit; preserving it".into(),
            });
        }
        match &mutation.before {
            Some(bytes) => write_beneath_root(root, &mutation.path, bytes)?,
            None => remove_beneath_root(root, &mutation.path)?,
        }
    }
    for relative in missing_directories.iter().rev() {
        let path = guard_beneath_root(root, Path::new(relative))?;
        match std::fs::remove_dir(&path) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(error) => return Err(ScaffoldError::io(path, error)),
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
    fn late_preflight_failure_leaves_portal_adoption_byte_identical() {
        let temp = initialized_root();
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let source = DirSource::new(assets);
        setup_portal(&source, temp.path(), Path::new("guide")).unwrap();

        let user_config = b"{\"mine\":true}\n";
        std::fs::write(temp.path().join("guide/portal.config.json"), user_config).unwrap();
        std::fs::remove_file(temp.path().join("guide/.node-version")).unwrap();
        std::fs::remove_file(temp.path().join("guide/astro.config.mjs")).unwrap();
        std::fs::create_dir(temp.path().join("guide/astro.config.mjs")).unwrap();
        let state_before = std::fs::read(temp.path().join(STATE_PATH)).unwrap();
        let baselines_before: BTreeMap<_, _> = std::fs::read_dir(temp.path().join(BASELINE_ROOT))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name(),
                    std::fs::read(entry.path()).expect("baseline must remain a regular file"),
                )
            })
            .collect();

        let error = setup_portal(&source, temp.path(), Path::new("guide")).unwrap_err();
        assert!(error.to_string().contains("astro.config.mjs"));
        assert!(!temp.path().join("guide/.node-version").exists());
        assert!(temp.path().join("guide/astro.config.mjs").is_dir());
        assert_eq!(
            std::fs::read(temp.path().join("guide/portal.config.json")).unwrap(),
            user_config
        );
        assert_eq!(
            std::fs::read(temp.path().join(STATE_PATH)).unwrap(),
            state_before
        );
        let baselines_after: BTreeMap<_, _> = std::fs::read_dir(temp.path().join(BASELINE_ROOT))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name(),
                    std::fs::read(entry.path()).expect("baseline must remain a regular file"),
                )
            })
            .collect();
        assert_eq!(baselines_after, baselines_before);
    }

    #[test]
    fn commit_phase_failure_rolls_back_files_and_created_directories() {
        let temp = initialized_root();
        let mut plan = PortalMutationPlan::default();
        plan.write(temp.path(), "guide/a.txt", b"a\n".to_vec(), 32, "fixture")
            .unwrap();
        plan.write(temp.path(), "guide/b.txt", b"b\n".to_vec(), 32, "fixture")
            .unwrap();
        assert!(plan.commit_with_fault_after(temp.path(), 0).is_err());
        assert!(!temp.path().join("guide/a.txt").exists());
        assert!(!temp.path().join("guide/b.txt").exists());
        assert!(!temp.path().join("guide").exists());
    }

    #[test]
    fn abrupt_transaction_is_recovered_by_roll_forward_at_every_boundary() {
        for boundary in 0..3 {
            let temp = initialized_root();
            std::fs::write(temp.path().join("old.txt"), b"old\n").unwrap();
            let mut plan = PortalMutationPlan::default();
            plan.write(temp.path(), "guide/a.txt", b"a\n".to_vec(), 32, "fixture")
                .unwrap();
            plan.remove(temp.path(), "old.txt", 32, "fixture").unwrap();
            plan.write(
                temp.path(),
                STATE_PATH,
                br#"{"schema_version":1}"#.to_vec(),
                MAX_STATE_BYTES,
                "fixture",
            )
            .unwrap();

            let error = plan
                .commit_with_abrupt_fault_after(temp.path(), boundary)
                .unwrap_err();
            assert!(error.to_string().contains("abrupt"));
            assert!(temp.path().join(TRANSACTION_PATH).is_dir());

            recover_portal_transaction(temp.path()).unwrap();
            assert_eq!(
                std::fs::read(temp.path().join("guide/a.txt")).unwrap(),
                b"a\n"
            );
            assert!(!temp.path().join("old.txt").exists());
            assert_eq!(
                std::fs::read(temp.path().join(STATE_PATH)).unwrap(),
                br#"{"schema_version":1}"#
            );
            assert!(!temp.path().join(TRANSACTION_PATH).exists());
        }
    }

    #[test]
    fn transaction_orders_content_then_state_then_baseline_pruning() {
        let temp = initialized_root();
        let retired = format!("{BASELINE_ROOT}/{}", "a".repeat(64));
        std::fs::create_dir_all(temp.path().join(BASELINE_ROOT)).unwrap();
        std::fs::write(temp.path().join(&retired), b"old\n").unwrap();
        let active = format!("{BASELINE_ROOT}/{}", "b".repeat(64));
        let mut plan = PortalMutationPlan::default();
        plan.write(temp.path(), STATE_PATH, b"state\n".to_vec(), 32, "fixture")
            .unwrap();
        plan.write(temp.path(), "guide/a.txt", b"a\n".to_vec(), 32, "fixture")
            .unwrap();
        plan.write(temp.path(), &active, b"new\n".to_vec(), 32, "fixture")
            .unwrap();
        plan.remove(temp.path(), &retired, 32, "fixture").unwrap();

        let ordered: Vec<_> = ordered_mutations(&plan.mutations)
            .into_iter()
            .map(|mutation| mutation.path.as_str())
            .collect();
        assert_eq!(
            ordered,
            ["guide/a.txt", active.as_str(), STATE_PATH, retired.as_str()]
        );
    }

    #[test]
    fn transaction_recovery_preserves_unexpected_external_edits() {
        let temp = initialized_root();
        std::fs::write(temp.path().join("value.txt"), b"before\n").unwrap();
        let mut plan = PortalMutationPlan::default();
        plan.write(temp.path(), "value.txt", b"after\n".to_vec(), 32, "fixture")
            .unwrap();
        assert!(plan.commit_with_abrupt_fault_after(temp.path(), 0).is_err());
        std::fs::write(temp.path().join("value.txt"), b"operator edit\n").unwrap();

        let error = recover_portal_transaction(temp.path()).unwrap_err();
        assert!(error.to_string().contains("concurrent edit"));
        assert_eq!(
            std::fs::read(temp.path().join("value.txt")).unwrap(),
            b"operator edit\n"
        );
        assert!(temp.path().join(TRANSACTION_PATH).is_dir());
    }

    #[test]
    fn portal_transaction_lease_excludes_concurrent_reconciliation() {
        let temp = initialized_root();
        let first = acquire_portal_transaction_lease(temp.path()).unwrap();
        let error = acquire_portal_transaction_lease(temp.path()).unwrap_err();
        assert!(error.to_string().contains("already in progress"));
        drop(first);
        acquire_portal_transaction_lease(temp.path()).unwrap();
    }

    #[test]
    fn aggregate_bundle_and_snapshot_budgets_fail_before_mutation() {
        let mut bundle_budget = ReadBudget::with_limit(5);
        charge_bundle_asset(&mut bundle_budget, "a.txt", b"123").unwrap();
        let error = charge_bundle_asset(&mut bundle_budget, "b.txt", b"456").unwrap_err();
        assert!(error.to_string().contains("aggregate byte limit"));

        let temp = initialized_root();
        std::fs::write(temp.path().join("first.txt"), b"123").unwrap();
        std::fs::write(temp.path().join("second.txt"), b"456").unwrap();
        let mut plan = PortalMutationPlan::with_limits(5, 64);
        plan.write(temp.path(), "first.txt", b"one".to_vec(), 8, "fixture")
            .unwrap();
        let error = plan
            .write(temp.path(), "second.txt", b"two".to_vec(), 8, "fixture")
            .unwrap_err();
        assert!(error.to_string().contains("aggregate byte limit"));
        assert_eq!(
            std::fs::read(temp.path().join("first.txt")).unwrap(),
            b"123"
        );
        assert_eq!(
            std::fs::read(temp.path().join("second.txt")).unwrap(),
            b"456"
        );

        let mut plan = PortalMutationPlan::with_limits(64, 5);
        plan.write(temp.path(), "first.txt", b"123".to_vec(), 8, "fixture")
            .unwrap();
        let error = plan
            .write(temp.path(), "second.txt", b"456".to_vec(), 8, "fixture")
            .unwrap_err();
        assert!(error.to_string().contains("outputs exceed"));
        assert_eq!(
            std::fs::read(temp.path().join("first.txt")).unwrap(),
            b"123"
        );
        assert_eq!(
            std::fs::read(temp.path().join("second.txt")).unwrap(),
            b"456"
        );
    }

    #[test]
    fn transaction_refuses_stale_snapshots_before_writing() {
        let temp = initialized_root();
        std::fs::write(temp.path().join("first.txt"), b"before one").unwrap();
        std::fs::write(temp.path().join("second.txt"), b"before two").unwrap();
        let mut plan = PortalMutationPlan::default();
        plan.write(
            temp.path(),
            "first.txt",
            b"after one".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        plan.write(
            temp.path(),
            "second.txt",
            b"after two".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        std::fs::write(temp.path().join("second.txt"), b"concurrent edit").unwrap();

        let error = plan.commit(temp.path()).unwrap_err();
        assert!(error.to_string().contains("snapshot changed"));
        assert_eq!(
            std::fs::read(temp.path().join("first.txt")).unwrap(),
            b"before one"
        );
        assert_eq!(
            std::fs::read(temp.path().join("second.txt")).unwrap(),
            b"concurrent edit"
        );
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
            vec![b'x'; usize::try_from(MAX_STATE_BYTES).unwrap() + 1],
        )
        .unwrap();
        assert!(update_adopted_portal(&source("1.0.0", "one\n"), temp.path()).is_err());
    }

    #[test]
    fn reconciliation_refuses_oversized_current_baseline_and_retired_files() {
        let oversized = vec![b'x'; usize::try_from(MAX_MANAGED_FILE_BYTES).unwrap() + 1];

        let current = initialized_root();
        setup_portal(
            &source("1.0.0", "old\n"),
            current.path(),
            Path::new("guide"),
        )
        .unwrap();
        std::fs::write(current.path().join("guide/managed.txt"), &oversized).unwrap();
        assert!(setup_portal(
            &source("2.0.0", "new\n"),
            current.path(),
            Path::new("guide")
        )
        .is_err());

        let baseline = initialized_root();
        setup_portal(
            &source("1.0.0", "old\n"),
            baseline.path(),
            Path::new("guide"),
        )
        .unwrap();
        std::fs::write(baseline.path().join("guide/managed.txt"), "user edit\n").unwrap();
        std::fs::write(
            baseline.path().join(baseline_path(&sha256_hex(b"old\n"))),
            &oversized,
        )
        .unwrap();
        assert!(setup_portal(
            &source("2.0.0", "new\n"),
            baseline.path(),
            Path::new("guide")
        )
        .is_err());

        let retired = initialized_root();
        setup_portal(
            &source("1.0.0", "old\n"),
            retired.path(),
            Path::new("guide"),
        )
        .unwrap();
        std::fs::write(retired.path().join("guide/managed.txt"), &oversized).unwrap();
        let replacement = MapSource(BTreeMap::from([
            (
                MANIFEST_ASSET.into(),
                br#"{"schema_version":1,"version":"2.0.0","files":[{"path":"replacement.txt","ownership":"managed"}]}"#.to_vec(),
            ),
            (format!("{ASSET_PREFIX}replacement.txt"), b"replacement\n".to_vec()),
        ]));
        assert!(setup_portal(&replacement, retired.path(), Path::new("guide")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn reconciliation_refuses_symlinked_current_baseline_and_retired_files() {
        use std::os::unix::fs::symlink;

        let current = initialized_root();
        setup_portal(
            &source("1.0.0", "old\n"),
            current.path(),
            Path::new("guide"),
        )
        .unwrap();
        let outside = current.path().join("outside.txt");
        std::fs::write(&outside, "outside\n").unwrap();
        std::fs::remove_file(current.path().join("guide/managed.txt")).unwrap();
        symlink(&outside, current.path().join("guide/managed.txt")).unwrap();
        assert!(setup_portal(
            &source("2.0.0", "new\n"),
            current.path(),
            Path::new("guide")
        )
        .is_err());

        let baseline = initialized_root();
        setup_portal(
            &source("1.0.0", "old\n"),
            baseline.path(),
            Path::new("guide"),
        )
        .unwrap();
        std::fs::write(baseline.path().join("guide/managed.txt"), "user edit\n").unwrap();
        let baseline_file = baseline.path().join(baseline_path(&sha256_hex(b"old\n")));
        std::fs::remove_file(&baseline_file).unwrap();
        symlink(&outside, &baseline_file).unwrap();
        assert!(setup_portal(
            &source("2.0.0", "new\n"),
            baseline.path(),
            Path::new("guide")
        )
        .is_err());

        let retired = initialized_root();
        setup_portal(
            &source("1.0.0", "old\n"),
            retired.path(),
            Path::new("guide"),
        )
        .unwrap();
        let outside = retired.path().join("outside.txt");
        std::fs::write(&outside, "outside\n").unwrap();
        std::fs::remove_file(retired.path().join("guide/managed.txt")).unwrap();
        symlink(&outside, retired.path().join("guide/managed.txt")).unwrap();
        let replacement = MapSource(BTreeMap::from([
            (
                MANIFEST_ASSET.into(),
                br#"{"schema_version":1,"version":"2.0.0","files":[{"path":"replacement.txt","ownership":"managed"}]}"#.to_vec(),
            ),
            (format!("{ASSET_PREFIX}replacement.txt"), b"replacement\n".to_vec()),
        ]));
        assert!(setup_portal(&replacement, retired.path(), Path::new("guide")).is_err());
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

    #[test]
    fn bounded_reconciliation_reads_detect_growth_and_path_swaps() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("managed.txt");
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
        let moved = temp.path().join("moved.txt");
        assert!(read_bounded_regular_with_hook(&file, 32, || {
            std::fs::rename(&file, &moved)?;
            std::fs::write(&file, "replacement")
        })
        .is_err());
    }
}
