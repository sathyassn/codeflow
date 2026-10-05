//! Opt-in documentation-portal starter materialization and reconciliation.

pub(crate) mod state;
mod transaction_io;
use state::{Generator, PortalFileState, PortalState, RuntimeOwnership};
use transaction_io::PortalIo;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use unicode_normalization::UnicodeNormalization;

use super::report::{Action, Report};
use super::state::guard_beneath_root;
use super::{sha256_hex, AssetSource, ScaffoldError};

const ASSET_PREFIX: &str = "docs-portal/starter/";
const MANIFEST_ASSET: &str = "docs-portal/manifest.json";
const STATE_PATH: &str = ".codeflow/docs-portal.json";
const BASELINE_ROOT: &str = ".codeflow/.docs-portal-baseline";
const TRANSACTION_LOCK_PATH: &str = ".codeflow/.docs-portal.lock";
const TRANSACTION_PATH: &str = ".codeflow/.docs-portal-transaction";
const TRANSACTION_STAGE_PREFIX: &str = ".docs-portal-transaction-stage-";
const TRANSACTION_CLEANUP_PREFIX: &str = ".docs-portal-transaction-cleanup-";
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
    transaction_id: String,
    portal_root: String,
    state_path: String,
    baseline_root: String,
    mutations: Vec<PortalJournalMutation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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
    root: &'a PortalIo,
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
        root: &PortalIo,
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
        root: &PortalIo,
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

    fn commit(self, root: &PortalIo, portal_root: &Path) -> Result<(), ScaffoldError> {
        self.commit_inner(root, portal_root, None)
    }

    #[cfg(test)]
    fn commit_with_fault_after(
        self,
        root: &PortalIo,
        portal_root: &Path,
        mutation_index: usize,
    ) -> Result<(), ScaffoldError> {
        self.commit_inner(root, portal_root, Some(mutation_index))
    }

    fn commit_inner(
        self,
        root: &PortalIo,
        portal_root: &Path,
        fault_after: Option<usize>,
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
        let missing_directories = missing_parent_directories(root, ordered.iter().copied())?;
        prepare_portal_transaction(root, portal_root, &ordered)?;
        let mut applied = Vec::new();
        for (index, mutation) in ordered.into_iter().enumerate() {
            if let Err(error) = ensure_snapshot_unchanged(
                root,
                &mutation.path,
                mutation.before.as_deref(),
                "pre-apply",
            ) {
                rollback_mutations(root, &applied, &missing_directories)?;
                remove_portal_transaction(root, portal_root)?;
                return Err(error);
            }
            // Atomic writes may report a durability-sync failure after rename;
            // include the in-flight path in rollback even when apply returns Err.
            applied.push(mutation);
            let result = match &mutation.after {
                Some(bytes) => root.write(&mutation.path, bytes),
                None => root.remove(&mutation.path),
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
                remove_portal_transaction(root, portal_root)?;
                return Err(error);
            }
        }
        for mutation in self.mutations.values() {
            ensure_snapshot_unchanged(
                root,
                &mutation.path,
                mutation.after.as_deref(),
                "post-commit",
            )?;
        }
        remove_empty_legacy_baseline_directory(root)?;
        remove_portal_transaction(root, portal_root)?;
        Ok(())
    }

    #[cfg(test)]
    fn commit_with_abrupt_fault_after(
        self,
        root: &PortalIo,
        portal_root: &Path,
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
        prepare_portal_transaction(root, portal_root, &ordered)?;
        for (index, mutation) in ordered.into_iter().enumerate() {
            match &mutation.after {
                Some(bytes) => root.write(&mutation.path, bytes)?,
                None => root.remove(&mutation.path)?,
            }
            if index == mutation_index {
                return Err(ScaffoldError::InvalidState {
                    what: mutation.path.clone(),
                    detail: "injected abrupt portal transaction interruption".into(),
                });
            }
        }
        remove_portal_transaction(root, portal_root)
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

fn acquire_portal_transaction_lease(
    root: &PortalIo,
) -> Result<PortalTransactionLease, ScaffoldError> {
    match root.metadata(TRANSACTION_LOCK_PATH) {
        Ok(metadata) if !metadata.is_file() => {
            return Err(ScaffoldError::InvalidState {
                what: TRANSACTION_LOCK_PATH.into(),
                detail: "portal transaction lock is not a regular file".into(),
            })
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(lease_open_error(root, error)),
    }
    let file = root
        .lock_file(TRANSACTION_LOCK_PATH)
        .map_err(|error| lease_open_error(root, error))?;
    file.try_lock_exclusive()
        .map_err(|error| ScaffoldError::InvalidState {
            what: TRANSACTION_LOCK_PATH.into(),
            detail: format!("another portal setup/update is already in progress: {error}"),
        })?;
    Ok(PortalTransactionLease { file })
}

fn lease_open_error(root: &PortalIo, error: std::io::Error) -> ScaffoldError {
    #[cfg(windows)]
    if error.raw_os_error()
        == Some(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION.cast_signed())
    {
        // A newly created lease retains DELETE access for rejection cleanup;
        // another no-delete-sharing opener can be excluded before fs2 locking.
        return ScaffoldError::InvalidState {
            what: TRANSACTION_LOCK_PATH.into(),
            detail: "another portal setup/update is already in progress (lock sharing violation)"
                .into(),
        };
    }
    ScaffoldError::io(root.join(TRANSACTION_LOCK_PATH), error)
}

fn prepare_portal_transaction(
    root: &PortalIo,
    portal_root: &Path,
    mutations: &[&PortalMutation],
) -> Result<(), ScaffoldError> {
    let portal_root = validate_portal_root(portal_root)?;
    if root.inspect(TRANSACTION_PATH)?.is_some() {
        return Err(ScaffoldError::InvalidState {
            what: TRANSACTION_PATH.into(),
            detail: "an unrecovered portal transaction already exists".into(),
        });
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let transaction_id = sha256_hex(
        format!("{}-{nonce}-{}", std::process::id(), path_text(&portal_root)).as_bytes(),
    );
    let stage_relative = format!(
        ".codeflow/{TRANSACTION_STAGE_PREFIX}{}-{nonce}",
        std::process::id()
    );
    root.mkdir(&stage_relative)?;
    let prepared = (|| {
        let journal = PortalTransactionJournal {
            schema_version: 2,
            transaction_id,
            portal_root: path_text(&portal_root),
            state_path: STATE_PATH.into(),
            baseline_root: BASELINE_ROOT.into(),
            mutations: mutations
                .iter()
                .enumerate()
                .map(|(index, mutation)| PortalJournalMutation {
                    path: mutation.path.clone(),
                    before_sha256: mutation.before.as_ref().map(|bytes| sha256_hex(bytes)),
                    before_bytes: mutation.before.as_ref().map(|bytes| bytes.len() as u64),
                    after_sha256: mutation.after.as_ref().map(|bytes| sha256_hex(bytes)),
                    after_bytes: mutation.after.as_ref().map(|bytes| bytes.len() as u64),
                    staged_file: mutation.after.as_ref().map(|_| format!("after/{index:04}")),
                })
                .collect(),
        };
        // An unpublished stage gets its cleanup authentication anchor first.
        // Active publication still occurs only after every output is durable.
        root.write(
            &format!("{stage_relative}/manifest.json"),
            &serde_json::to_vec_pretty(&journal)?,
        )?;
        for (index, mutation) in mutations.iter().enumerate() {
            if let Some(bytes) = &mutation.after {
                root.write(&format!("{stage_relative}/after/{index:04}"), bytes)?;
            }
        }
        root.rename(&stage_relative, TRANSACTION_PATH)?;
        Ok(())
    })();
    if prepared.is_err() {
        // A complete staged journal authenticates cleanup. An incomplete or
        // externally modified stage is retained instead of recursively deleted.
        let _ = remove_staged_transaction(root, &stage_relative, &portal_root);
    }
    prepared
}

fn recover_portal_transaction(
    root: &PortalIo,
    expected_portal_root: &Path,
) -> Result<(), ScaffoldError> {
    let expected_portal_root = validate_portal_root(expected_portal_root)?;
    remove_abandoned_transaction_debris(root, &expected_portal_root)?;
    let transaction = guard_beneath_root(root, Path::new(TRANSACTION_PATH))?;
    let metadata = match root.metadata(TRANSACTION_PATH) {
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
    let journal = read_transaction_journal(root, TRANSACTION_PATH)?;
    validate_transaction_journal(&journal, &expected_portal_root, TRANSACTION_PATH)?;
    let mut paths = BTreeSet::new();
    let mut bytes_remaining = MAX_RECONCILIATION_READ_BYTES;
    for (index, mutation) in journal.mutations.iter().enumerate() {
        validate_journal_mutation(
            root,
            &expected_portal_root,
            index,
            mutation,
            &mut paths,
            &mut bytes_remaining,
        )?;
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
                let bytes = root
                    .read(&format!("{TRANSACTION_PATH}/{staged}"), expected_bytes)
                    .map_err(|error| ScaffoldError::InvalidState {
                        what: mutation.path.clone(),
                        detail: format!("staged portal transaction output is unreadable: {error}"),
                    })?;
                if bytes.len() as u64 != expected_bytes || sha256_hex(&bytes) != expected_hash {
                    return Err(ScaffoldError::InvalidState {
                        what: mutation.path.clone(),
                        detail: "staged portal transaction output does not match its journal"
                            .into(),
                    });
                }
                root.write(&mutation.path, &bytes)?;
            }
            (None, None, None) => root.remove(&mutation.path)?,
            _ => {
                return Err(ScaffoldError::InvalidState {
                    what: mutation.path.clone(),
                    detail: "portal transaction journal mutation is inconsistent".into(),
                });
            }
        }
    }
    remove_empty_legacy_baseline_directory(root)?;
    remove_portal_transaction(root, &expected_portal_root)
}

/// The validated transaction envelope owns this one reserved directory. Remove
/// only an empty regular directory, never recursively enumerate/delete contents.
/// Old journals may legitimately leave baseline blobs; nonempty is preserved.
fn remove_empty_legacy_baseline_directory(root: &PortalIo) -> Result<(), ScaffoldError> {
    match root.remove_empty(BASELINE_ROOT) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(()),
        Err(error) => Err(ScaffoldError::io(root.join(BASELINE_ROOT), error)),
    }
}

fn read_transaction_journal(
    root: &PortalIo,
    directory: &str,
) -> Result<PortalTransactionJournal, ScaffoldError> {
    let manifest = root
        .read(&format!("{directory}/manifest.json"), 512 * 1024)
        .map_err(|error| ScaffoldError::InvalidState {
            what: directory.into(),
            detail: format!("portal transaction journal is unreadable: {error}"),
        })?;
    crate::strict_json::parse_strict_json(&manifest).map_err(|_| ScaffoldError::InvalidState {
        what: directory.into(),
        detail: "portal transaction journal schema is invalid".into(),
    })
}

fn validate_transaction_journal(
    journal: &PortalTransactionJournal,
    expected_portal_root: &Path,
    label: &str,
) -> Result<(), ScaffoldError> {
    let expected_root = path_text(expected_portal_root);
    if journal.schema_version != 2
        || !valid_lower_sha256(&journal.transaction_id)
        || journal.portal_root != expected_root
        || journal.state_path != STATE_PATH
        || journal.baseline_root != BASELINE_ROOT
        || journal.mutations.is_empty()
        || journal.mutations.len() > 2_048
    {
        return Err(ScaffoldError::InvalidState {
            what: label.into(),
            detail: "portal transaction journal has an invalid or mismatched envelope".into(),
        });
    }
    Ok(())
}

fn validate_journal_mutation(
    root: &PortalIo,
    portal_root: &Path,
    index: usize,
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
    let portal_prefix = format!("{}/", path_text(portal_root));
    let baseline_prefix = format!("{BASELINE_ROOT}/");
    let in_portal = mutation.path.starts_with(&portal_prefix);
    let baseline_hash = mutation.path.strip_prefix(&baseline_prefix);
    let in_baseline = baseline_hash.is_some_and(valid_lower_sha256);
    if mutation.path != STATE_PATH && !in_portal && !in_baseline {
        return Err(ScaffoldError::InvalidState {
            what: mutation.path.clone(),
            detail: "portal transaction journal destination is outside its adopted namespaces"
                .into(),
        });
    }
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
    if let Some(baseline_hash) = baseline_hash {
        let content_hash = mutation
            .after_sha256
            .as_deref()
            .or(mutation.before_sha256.as_deref());
        if content_hash != Some(baseline_hash) {
            return Err(ScaffoldError::InvalidState {
                what: mutation.path.clone(),
                detail: "portal baseline destination is not bound to its content hash".into(),
            });
        }
    }
    let expected_stage = format!("after/{index:04}");
    if mutation.after_bytes.is_some() != mutation.staged_file.is_some()
        || mutation
            .staged_file
            .as_deref()
            .is_some_and(|path| path != expected_stage)
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

fn valid_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn remove_portal_transaction(root: &PortalIo, portal_root: &Path) -> Result<(), ScaffoldError> {
    let transaction = guard_beneath_root(root, Path::new(TRANSACTION_PATH))?;
    match root.metadata(TRANSACTION_PATH) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(ScaffoldError::InvalidState {
                what: TRANSACTION_PATH.into(),
                detail: "portal transaction journal is not a regular directory".into(),
            })
        }
        Ok(_) => {
            let journal = read_transaction_journal(root, TRANSACTION_PATH)?;
            validate_transaction_journal(&journal, portal_root, TRANSACTION_PATH)?;
            let cleanup_relative = format!(
                ".codeflow/{TRANSACTION_CLEANUP_PREFIX}{}",
                journal.transaction_id
            );
            let cleanup = guard_beneath_root(root, Path::new(&cleanup_relative))?;
            if root.inspect(&cleanup_relative)?.is_some() {
                return Err(ScaffoldError::InvalidState {
                    what: cleanup_relative,
                    detail: "portal transaction cleanup tombstone already exists".into(),
                });
            }
            root.rename(TRANSACTION_PATH, &cleanup_relative)?;
            remove_transaction_tombstone(root, &cleanup, portal_root, &journal.transaction_id)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ScaffoldError::io(&transaction, error)),
    }
}

fn remove_transaction_tombstone(
    root: &PortalIo,
    tombstone: &Path,
    portal_root: &Path,
    expected_id: &str,
) -> Result<(), ScaffoldError> {
    let relative =
        path_text(
            tombstone
                .strip_prefix(root)
                .map_err(|_| ScaffoldError::InvalidState {
                    what: path_text(tombstone),
                    detail: "portal transaction tombstone escaped the repository".into(),
                })?,
        );
    let metadata = root
        .metadata(&relative)
        .map_err(|error| ScaffoldError::io(tombstone, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ScaffoldError::InvalidState {
            what: relative.clone(),
            detail: "portal transaction cleanup tombstone is not a regular directory".into(),
        });
    }
    if !valid_lower_sha256(expected_id)
        || tombstone.file_name().and_then(|name| name.to_str())
            != Some(&format!("{TRANSACTION_CLEANUP_PREFIX}{expected_id}"))
    {
        return Err(ScaffoldError::InvalidState {
            what: relative,
            detail: "invalid portal cleanup identity".into(),
        });
    }
    if root
        .entries(&relative, 2)
        .map_err(|e| ScaffoldError::io(tombstone, e))?
        .is_empty()
    {
        return root
            .remove_empty(&relative)
            .map_err(|e| ScaffoldError::io(tombstone, e));
    }
    let journal = read_transaction_journal(root, &relative)?;
    validate_transaction_journal(&journal, portal_root, &relative)?;
    if journal.transaction_id != expected_id
        || tombstone.file_name().and_then(|name| name.to_str())
            != Some(&format!("{TRANSACTION_CLEANUP_PREFIX}{expected_id}"))
    {
        return Err(ScaffoldError::InvalidState {
            what: relative,
            detail: "portal transaction cleanup tombstone identity does not match".into(),
        });
    }
    remove_authenticated_transaction_tree(root, &relative, &journal, portal_root, true)
}

fn remove_abandoned_transaction_debris(
    root: &PortalIo,
    portal_root: &Path,
) -> Result<(), ScaffoldError> {
    let entries = root
        .entries(".codeflow", 4096)
        .map_err(|e| ScaffoldError::io(root.join(".codeflow"), e))?;
    let mut debris_seen = 0;
    for entry in entries {
        // Transaction debris is named by this tool in ASCII, so a name that
        // is not valid UTF-8 is not debris (OS text rule, issue 79).
        let Some(name) = entry.to_str() else {
            continue;
        };
        let is_stage = name.starts_with(TRANSACTION_STAGE_PREFIX);
        let cleanup_id = name.strip_prefix(TRANSACTION_CLEANUP_PREFIX);
        if !is_stage && cleanup_id.is_none() {
            continue;
        }
        debris_seen += 1;
        if debris_seen > 64 {
            return Err(ScaffoldError::InvalidState {
                what: ".codeflow".into(),
                detail: "too many abandoned portal transaction recovery entries".into(),
            });
        }
        let relative = format!(".codeflow/{name}");
        if let Some(id) = cleanup_id {
            if !valid_lower_sha256(id) {
                return Err(ScaffoldError::InvalidState {
                    what: relative,
                    detail: "invalid portal transaction cleanup name".into(),
                });
            }
            remove_transaction_tombstone(root, &root.join(&relative), portal_root, id)?;
        } else {
            remove_staged_transaction(root, &relative, portal_root)?;
        }
    }
    Ok(())
}

fn remove_staged_transaction(
    root: &PortalIo,
    directory: &str,
    portal_root: &Path,
) -> Result<(), ScaffoldError> {
    if !root
        .metadata(directory)
        .map_err(|e| ScaffoldError::io(root.join(directory), e))?
        .is_dir()
    {
        return Err(ScaffoldError::InvalidState {
            what: directory.into(),
            detail: "portal transaction stage is not a regular directory".into(),
        });
    }
    let name = Path::new(directory)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let valid_name = name
        .strip_prefix(TRANSACTION_STAGE_PREFIX)
        .and_then(|suffix| suffix.split_once('-'))
        .is_some_and(|(pid, nonce)| {
            !pid.is_empty()
                && !nonce.is_empty()
                && pid.len() <= 10
                && nonce.len() <= 39
                && pid.bytes().all(|byte| byte.is_ascii_digit())
                && nonce.bytes().all(|byte| byte.is_ascii_digit())
        });
    if !valid_name {
        return Err(ScaffoldError::InvalidState {
            what: directory.into(),
            detail: "unrecognized portal stage name; preserving contents".into(),
        });
    }
    if root
        .entries(directory, 2)
        .map_err(|e| ScaffoldError::io(root.join(directory), e))?
        .is_empty()
    {
        return root
            .remove_empty(directory)
            .map_err(|e| ScaffoldError::io(root.join(directory), e));
    }
    let journal = read_transaction_journal(root, directory)?;
    validate_transaction_journal(&journal, portal_root, directory)?;
    remove_authenticated_transaction_tree(root, directory, &journal, portal_root, false)
}

/// Authenticate the entire bounded inventory before deleting any journal bytes.
/// Unknown, changed, nonregular and symlinked entries remain preserved.
fn remove_authenticated_transaction_tree(
    root: &PortalIo,
    directory: &str,
    journal: &PortalTransactionJournal,
    portal_root: &Path,
    completed: bool,
) -> Result<(), ScaffoldError> {
    let removals = authenticated_cleanup_outputs(root, directory, journal, portal_root, completed)?;
    let manifest_path = format!("{directory}/manifest.json");
    let manifest = root
        .read(&manifest_path, 512 * 1024)
        .map_err(|e| ScaffoldError::io(root.join(&manifest_path), e))?;
    // Keep the manifest until outputs are gone, so interrupted cleanup retains
    // its ownership/evidence anchor and can safely resume with missing outputs.
    for (relative, bytes) in removals {
        ensure_snapshot_unchanged(root, &relative, Some(&bytes), "cleanup")?;
        root.remove(&relative)?;
    }
    let after = format!("{directory}/after");
    root.remove_empty(&after)
        .map_err(|e| ScaffoldError::io(root.join(&after), e))?;
    ensure_snapshot_unchanged(root, &manifest_path, Some(&manifest), "cleanup")?;
    root.remove(&manifest_path)?;
    root.remove_empty(directory)
        .map_err(|e| ScaffoldError::io(root.join(directory), e))
}

fn authenticated_cleanup_outputs(
    root: &PortalIo,
    directory: &str,
    journal: &PortalTransactionJournal,
    portal_root: &Path,
    completed: bool,
) -> Result<Vec<(String, Vec<u8>)>, ScaffoldError> {
    let mut paths = BTreeSet::new();
    let mut remaining = MAX_RECONCILIATION_READ_BYTES;
    let mut allowed = BTreeMap::new();
    let mut all_before = true;
    let mut all_after = true;
    for (index, mutation) in journal.mutations.iter().enumerate() {
        validate_journal_mutation(
            root,
            portal_root,
            index,
            mutation,
            &mut paths,
            &mut remaining,
        )?;
        if completed {
            let maximum = mutation
                .before_bytes
                .into_iter()
                .chain(mutation.after_bytes)
                .max()
                .unwrap_or(0);
            let current = snapshot_project_file(root, &mutation.path, maximum, "cleanup outcome")?;
            all_before &= snapshot_matches(
                current.as_deref(),
                mutation.before_bytes,
                mutation.before_sha256.as_deref(),
            );
            all_after &= snapshot_matches(
                current.as_deref(),
                mutation.after_bytes,
                mutation.after_sha256.as_deref(),
            );
        }
        if let (Some(staged), Some(size), Some(hash)) = (
            &mutation.staged_file,
            mutation.after_bytes,
            &mutation.after_sha256,
        ) {
            allowed.insert(staged.clone(), (size, hash));
        }
    }
    if completed && !all_before && !all_after {
        return Err(ScaffoldError::InvalidState {
            what: directory.into(),
            detail: "cleanup journal does not describe a completed application or rollback; preserving recovery evidence".into(),
        });
    }
    let entries = root
        .entries(directory, 2)
        .map_err(|e| ScaffoldError::io(root.join(directory), e))?;
    if entries
        .iter()
        .any(|name| name != "after" && name != "manifest.json")
    {
        return Err(ScaffoldError::InvalidState {
            what: directory.into(),
            detail: "unknown portal transaction cleanup entry".into(),
        });
    }
    let after = format!("{directory}/after");
    let staged = match root.entries(&after, 2048) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(ScaffoldError::io(root.join(&after), error)),
    };
    let mut removals = Vec::new();
    for name in staged {
        // OS text rule (issue 79, `docs/architecture.md`): kept strict. Each
        // staged name is looked up in the transaction's own list of outputs,
        // so a name that is not valid UTF-8 is foreign state, and the cleanup
        // refuses it instead of deleting around it.
        let name = name.to_str().ok_or_else(|| ScaffoldError::InvalidState {
            what: directory.into(),
            detail: "non-UTF-8 staged output name".into(),
        })?;
        let stage = format!("after/{name}");
        let (size, hash) = allowed
            .get(&stage)
            .ok_or_else(|| ScaffoldError::InvalidState {
                what: directory.into(),
                detail: "unknown staged portal output".into(),
            })?;
        let relative = format!("{directory}/{stage}");
        let bytes = root
            .read(&relative, *size)
            .map_err(|e| ScaffoldError::io(root.join(&relative), e))?;
        if bytes.len() as u64 != *size || sha256_hex(&bytes) != **hash {
            return Err(ScaffoldError::InvalidState {
                what: directory.into(),
                detail: "staged portal output changed before cleanup".into(),
            });
        }
        removals.push((relative, bytes));
    }
    Ok(removals)
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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum BundleOwnership {
    Managed,
    UserOwned,
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
    let io = PortalIo::open(repo_root)?;
    let repo_root = &io;
    let _lease = acquire_portal_transaction_lease(repo_root)?;
    recover_adoption(repo_root, Some(portal_root))?;
    setup_portal_locked(source, repo_root, portal_root)
}

#[allow(clippy::too_many_lines)]
fn setup_portal_locked(
    source: &dyn AssetSource,
    repo_root: &PortalIo,
    portal_root: &Path,
) -> Result<Report, ScaffoldError> {
    let relative_root = validate_portal_root(portal_root)?;
    if let Some(metadata) = repo_root.inspect(&path_text(&relative_root))? {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ScaffoldError::InvalidState {
                what: path_text(&relative_root),
                detail: "portal root exists but is not a regular directory".into(),
            });
        }
    }

    let previous = load_state(repo_root)?;
    if let Some(state) = &previous {
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
    if previous
        .as_ref()
        .is_some_and(|state| state.runtime_ownership == RuntimeOwnership::Transferred)
    {
        return Ok(transferred_report(&path_text(&relative_root), "update"));
    }
    let manifest_bytes = source
        .read(MANIFEST_ASSET)
        .ok_or_else(|| ScaffoldError::ManifestMissing(MANIFEST_ASSET.into()))?;
    if manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ScaffoldError::ManifestInvalid(
            "docs portal manifest exceeds its byte limit".into(),
        ));
    }
    let manifest: BundleManifest = crate::strict_json::parse_strict_json(&manifest_bytes)?;
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
    let mut report = Report::new(format!(
        "codeflow portal setup ({})",
        relative_root.display()
    ));
    let mut read_budget = ReadBudget::new();
    let mut bundle_budget = ReadBudget::new();
    let mut mutations = PortalMutationPlan::default();
    let mut next_files = previous
        .as_ref()
        .map(|state| {
            state
                .files
                .iter()
                .filter(|(_, file)| file.ownership == "user-owned")
                .map(|(path, file)| (path.clone(), file.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    for file in &manifest.files {
        let asset = source
            .read(&format!("{ASSET_PREFIX}{}", file.path))
            .ok_or_else(|| {
                ScaffoldError::ManifestMissing(format!("{ASSET_PREFIX}{}", file.path))
            })?;
        charge_bundle_asset(&mut bundle_budget, &file.path, &asset)?;
        let mut pristine = asset;
        let dest_rel = relative_root.join(&file.path);
        let dest_text = path_text(&dest_rel);
        if let Some(metadata) = repo_root.inspect(&dest_text)? {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(ScaffoldError::InvalidState {
                    what: dest_text.clone(),
                    detail: "portal destination exists but is not a regular file".into(),
                });
            }
        }
        if previous.is_none()
            && file.path == "portal.config.json"
            && repo_root.inspect(&dest_text)?.is_none()
        {
            let mut config: serde_json::Value = serde_json::from_slice(&pristine)?;
            let depth = relative_root
                .components()
                .filter(|component| matches!(component, Component::Normal(_)))
                .count();
            config["repository_root"] = serde_json::Value::String(
                std::iter::repeat_n("..", depth)
                    .collect::<Vec<_>>()
                    .join("/"),
            );
            pristine = format!("{}\n", serde_json::to_string_pretty(&config)?).into_bytes();
        }
        let pristine_hash = sha256_hex(&pristine);
        let old_file = previous
            .as_ref()
            .and_then(|state| state.files.get(&file.path));
        let ownership = if old_file.is_some_and(|state| state.ownership == "user-owned") {
            BundleOwnership::UserOwned
        } else {
            file.ownership
        };

        if ownership == BundleOwnership::UserOwned {
            if repo_root.inspect(&dest_text)?.is_some() {
                if old_file.is_some_and(|state| state.ownership == "managed") {
                    let current = read_project_file(
                        repo_root,
                        &dest_text,
                        "formerly managed portal file",
                        &mut read_budget,
                    )?
                    .ok_or_else(|| ScaffoldError::InvalidState {
                        what: dest_text.clone(),
                        detail: "portal file disappeared during preflight".into(),
                    })?;
                    if current != pristine
                        && old_file
                            .is_some_and(|state| sha256_hex(&current) != state.pristine_sha256)
                    {
                        report.file_with_notes(
                            dest_text,
                            Action::Conflicted,
                            vec!["edited managed file; portal update stopped".into()],
                        );
                    } else {
                        report.file(dest_text, Action::Skipped);
                    }
                } else {
                    report.file(dest_text, Action::Skipped);
                }
            } else if previous.is_some() {
                report.file_with_notes(
                    dest_text,
                    Action::Skipped,
                    vec!["project-owned file is missing; not recreated".into()],
                );
            } else {
                mutations.write(
                    repo_root,
                    &dest_text,
                    pristine.clone(),
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
                &pristine,
                old_file,
            )?;
        }
        next_files.insert(
            file.path.clone(),
            PortalFileState {
                ownership: match ownership {
                    BundleOwnership::Managed => "managed",
                    BundleOwnership::UserOwned => "user-owned",
                }
                .into(),
                pristine_sha256: if ownership == BundleOwnership::UserOwned {
                    old_file.map_or(pristine_hash, |state| state.pristine_sha256.clone())
                } else {
                    pristine_hash
                },
            },
        );
    }
    if let Some(old) = &previous {
        for (path, state) in &old.files {
            if state.ownership == "user-owned" && !seen.contains(&portable_key(path)) {
                let relative = path_text(&relative_root.join(path));
                if repo_root.inspect(&relative)?.is_none() {
                    report.file_with_notes(
                        relative,
                        Action::Skipped,
                        vec!["project-owned file is missing; not recreated".into()],
                    );
                }
            }
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
                report.file_with_notes(
                    dest_text,
                    Action::Conflicted,
                    vec!["edited retiring managed file; portal update stopped".into()],
                );
            }
        }
    }
    if let Some(old) = previous.as_ref().filter(|state| state.schema_version == 1) {
        plan_legacy_baseline_removal(
            repo_root,
            old,
            &mut read_budget,
            &mut mutations,
            &mut report,
        )?;
    }
    if report.has_conflicts() {
        for file in &mut report.files {
            if matches!(
                file.action,
                Action::Created | Action::Added | Action::Changed | Action::Removed
            ) {
                file.action = Action::Skipped;
                file.notes
                    .push("planned change not applied: portal preflight failed".into());
            }
        }
        report.notes.push("Portal update stopped without changes. Rehome supported customization, explicitly restore reviewed managed bytes, or use codeflow portal transfer --confirm. Ownership is never transferred automatically.".into());
        return Ok(report);
    }
    let state = PortalState {
        schema_version: 2,
        root: path_text(&relative_root),
        generator: Generator::managed(&manifest.version),
        runtime_ownership: RuntimeOwnership::Managed,
        starter_version: manifest.version,
        files: next_files,
    };
    plan_state(repo_root, &state, &mut mutations)?;
    mutations.commit(repo_root, &relative_root)?;
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
    let io = PortalIo::open(repo_root)?;
    let repo_root = &io;
    let _lease = acquire_portal_transaction_lease(repo_root)?;
    let Some(root) = recover_adoption(repo_root, None)? else {
        return Ok(None);
    };
    setup_portal_locked(source, repo_root, &root).map(Some)
}

fn reconcile_managed(
    context: &mut PortalReconciliation<'_>,
    dest_text: &str,
    pristine: &[u8],
    old: Option<&PortalFileState>,
) -> Result<(), ScaffoldError> {
    let current = read_project_file(
        context.root,
        dest_text,
        "managed portal file",
        context.read_budget,
    )?;
    let action = match current {
        None => Action::Added,
        Some(bytes) if bytes == pristine => {
            context.report.file(dest_text, Action::Unchanged);
            return Ok(());
        }
        Some(bytes) if old.is_some_and(|state| sha256_hex(&bytes) == state.pristine_sha256) => {
            Action::Changed
        }
        Some(_) => {
            context.report.file_with_notes(dest_text, Action::Conflicted,
                vec!["modified managed file or unknown incoming-path collision; no portal changes applied".into()]);
            return Ok(());
        }
    };
    context.mutations.write(
        context.root,
        dest_text,
        pristine.to_vec(),
        MAX_MANAGED_FILE_BYTES,
        "managed portal destination",
    )?;
    context.report.file(dest_text, action);
    Ok(())
}

fn read_project_file(
    root: &PortalIo,
    relative: &str,
    label: &str,
    budget: &mut ReadBudget,
) -> Result<Option<Vec<u8>>, ScaffoldError> {
    let maximum = MAX_MANAGED_FILE_BYTES.min(budget.remaining);
    match root.read(relative, maximum) {
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

fn read_state_bytes(root: &PortalIo) -> Result<Option<Vec<u8>>, ScaffoldError> {
    let path = guard_beneath_root(root, Path::new(STATE_PATH))?;
    match root.read(STATE_PATH, MAX_STATE_BYTES) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ScaffoldError::io(&path, error)),
    }
}

fn load_state(root: &PortalIo) -> Result<Option<PortalState>, ScaffoldError> {
    read_state_bytes(root)?
        .map(|bytes| {
            state::parse(&bytes).map_err(|detail| ScaffoldError::InvalidState {
                what: STATE_PATH.into(),
                detail,
            })
        })
        .transpose()
}

/// Recover against the caller's explicit root or the bounded root-only claim;
/// the full state parser intentionally runs only after recovery.
fn recover_adoption(
    root: &PortalIo,
    explicit: Option<&Path>,
) -> Result<Option<PathBuf>, ScaffoldError> {
    let adopted = read_state_bytes(root)?
        .map(|bytes| {
            state::recovery_root(&bytes)
                .map(PathBuf::from)
                .map_err(|detail| ScaffoldError::InvalidState {
                    what: STATE_PATH.into(),
                    detail,
                })
        })
        .transpose()?;
    let expected = match (explicit, adopted.as_deref()) {
        (Some(requested), Some(adopted)) => {
            let requested = validate_portal_root(requested)?;
            if !roots_equal(&path_text(&requested), &path_text(adopted)) {
                return Err(ScaffoldError::InvalidState {
                    what: STATE_PATH.into(),
                    detail: "portal is already adopted at another root".into(),
                });
            }
            Some(requested)
        }
        (Some(requested), None) => Some(validate_portal_root(requested)?),
        (None, adopted) => adopted.map(Path::to_path_buf),
    };
    if let Some(expected) = &expected {
        recover_portal_transaction(root, expected)?;
    } else if root.inspect(TRANSACTION_PATH)?.is_some() {
        return Err(ScaffoldError::InvalidState {
            what: TRANSACTION_PATH.into(),
            detail:
                "pending portal transaction needs an adopted or explicit setup root before recovery"
                    .into(),
        });
    }
    Ok(expected)
}

fn baseline_path(pristine_sha256: &str) -> String {
    format!("{BASELINE_ROOT}/{pristine_sha256}")
}

fn plan_legacy_baseline_removal(
    root: &PortalIo,
    state: &PortalState,
    budget: &mut ReadBudget,
    mutations: &mut PortalMutationPlan,
    report: &mut Report,
) -> Result<(), ScaffoldError> {
    let expected: BTreeSet<&str> = state
        .files
        .values()
        .filter(|file| file.ownership == "managed")
        .map(|file| file.pristine_sha256.as_str())
        .collect();
    let directory = guard_beneath_root(root, Path::new(BASELINE_ROOT))?;
    let entries = match root.entries(BASELINE_ROOT, MAX_BASELINE_ENTRIES) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ScaffoldError::io(&directory, error)),
    };
    for (index, entry) in entries.into_iter().enumerate() {
        if index >= MAX_BASELINE_ENTRIES {
            return Err(ScaffoldError::InvalidState {
                what: BASELINE_ROOT.into(),
                detail: "baseline directory contains too many entries".into(),
            });
        }
        // Kept strict for the reason given for staged names above: an entry
        // that is not valid UTF-8 is unknown content, and the migration
        // preserves all content.
        let name = entry
            .into_string()
            .map_err(|_| ScaffoldError::InvalidState {
                what: BASELINE_ROOT.into(),
                detail: "unknown non-UTF-8 legacy baseline entry".into(),
            })?;
        if !expected.contains(name.as_str()) {
            return Err(ScaffoldError::InvalidState {
                what: BASELINE_ROOT.into(),
                detail: "unknown legacy baseline entry; preserving all content".into(),
            });
        }
        let relative = baseline_path(&name);
        let bytes =
            read_project_file(root, &relative, "legacy baseline", budget)?.ok_or_else(|| {
                ScaffoldError::InvalidState {
                    what: BASELINE_ROOT.into(),
                    detail: "legacy baseline disappeared during preflight".into(),
                }
            })?;
        if sha256_hex(&bytes) != name {
            return Err(ScaffoldError::InvalidState {
                what: BASELINE_ROOT.into(), detail: "legacy baseline content does not match its recorded hash; preserving all content".into(),
            });
        }
        mutations.remove(
            root,
            &relative,
            MAX_MANAGED_FILE_BYTES,
            "authenticated legacy baseline",
        )?;
        report.file_with_notes(
            relative,
            Action::Removed,
            vec!["authenticated legacy baseline".into()],
        );
    }
    Ok(())
}

fn plan_state(
    root: &PortalIo,
    state: &PortalState,
    mutations: &mut PortalMutationPlan,
) -> Result<(), ScaffoldError> {
    state::validate(state).map_err(|detail| ScaffoldError::InvalidState {
        what: STATE_PATH.into(),
        detail,
    })?;
    let bytes = serde_json::to_vec_pretty(state)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(ScaffoldError::InvalidState {
            what: STATE_PATH.into(),
            detail: "portal adoption state exceeds its byte limit".into(),
        });
    }
    mutations.write(
        root,
        STATE_PATH,
        bytes,
        MAX_STATE_BYTES,
        "portal adoption state",
    )
}

fn transferred_report(root: &str, operation: &str) -> Report {
    let mut report = Report::new(format!("codeflow portal {operation} ({root})"));
    report.notes.push("Runtime is project-owned after transfer; no runtime files, configuration or transferred-from provenance changed. Upstream reconciliation is the project's responsibility.".into());
    report
}

/// Transfer the whole adopted runtime, without applying an embedded release.
///
/// # Errors
/// Requires explicit confirmation, valid adoption and safely recoverable state.
pub fn transfer_portal(root: &Path, confirmed: bool) -> Result<Report, ScaffoldError> {
    if !confirmed {
        return Err(ScaffoldError::InvalidState {
            what: "portal transfer".into(),
            detail: "explicit --confirm is required".into(),
        });
    }
    if !root.join(".codeflow/project.toml").exists() {
        return Err(ScaffoldError::NotInitialized);
    }
    let io = PortalIo::open(root)?;
    let root = &io;
    let _lease = acquire_portal_transaction_lease(root)?;
    let portal = recover_adoption(root, None)?.ok_or_else(|| ScaffoldError::InvalidState {
        what: STATE_PATH.into(),
        detail: "no portal is adopted".into(),
    })?;
    let destination = guard_beneath_root(root, &portal)?;
    match root.metadata(&path_text(&portal)) {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(ScaffoldError::InvalidState {
                what: path_text(&portal),
                detail: "adopted portal root is not a regular directory".into(),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(ScaffoldError::io(&destination, error)),
    }
    let mut state = load_state(root)?.ok_or_else(|| ScaffoldError::InvalidState {
        what: STATE_PATH.into(),
        detail: "portal adoption disappeared after recovery".into(),
    })?;
    if state.runtime_ownership == RuntimeOwnership::Transferred {
        return Ok(transferred_report(
            &state.root,
            "transfer (already transferred)",
        ));
    }
    // Preserve modified bytes and intentional deletions, but retain existing
    // path, regular-file and resource bounds. Never load the incoming bundle.
    let mut budget = ReadBudget::new();
    for path in state.files.keys() {
        read_project_file(
            root,
            &path_text(&portal.join(path)),
            "transferred portal file",
            &mut budget,
        )?;
    }
    let mut mutations = PortalMutationPlan::default();
    let mut report = Report::new(format!("codeflow portal transfer ({})", portal.display()));
    if state.schema_version == 1 {
        plan_legacy_baseline_removal(root, &state, &mut budget, &mut mutations, &mut report)?;
    }
    state.schema_version = 2;
    state.runtime_ownership = RuntimeOwnership::Transferred;
    plan_state(root, &state, &mut mutations)?;
    mutations.commit(root, &portal)?;
    report.notes.push("Whole runtime ownership transferred to the project. Existing bytes and deletions are preserved; adopted starter version and hashes are frozen transferred-from provenance.".into());
    Ok(report)
}

fn snapshot_project_file(
    root: &PortalIo,
    relative: &str,
    maximum: u64,
    label: &str,
) -> Result<Option<Vec<u8>>, ScaffoldError> {
    match root.read(relative, maximum) {
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
    root: &PortalIo,
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
    root: &PortalIo,
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
            match root.metadata(&path_text(ancestor)) {
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
    root: &PortalIo,
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
            Some(bytes) => root.write(&mutation.path, bytes)?,
            None => root.remove(&mutation.path)?,
        }
    }
    for relative in missing_directories.iter().rev() {
        let path = guard_beneath_root(root, Path::new(relative))?;
        match root.remove_empty(relative) {
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
    crate::portable_path::slashed(path)
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

#[cfg(test)]
use crate::bounded_file::read_bounded_regular_with_hook;

fn validate_asset_path(path: &str) -> Result<(), ScaffoldError> {
    validate_portal_root(Path::new(path))
        .and_then(|normalized| {
            if path_text(&normalized) == path {
                Ok(())
            } else {
                Err(ScaffoldError::ManifestInvalid(
                    "noncanonical portal asset path".into(),
                ))
            }
        })
        .map_err(|_| {
            ScaffoldError::ManifestInvalid(format!("unsafe docs portal asset path {path:?}"))
        })
}

#[cfg(test)]
mod tests {
    mod ownership;

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

    fn byte_source(version: &str, content: &[u8]) -> MapSource {
        let mut bundle = source(version, "");
        bundle
            .0
            .insert(format!("{ASSET_PREFIX}managed.txt"), content.to_vec());
        bundle
    }

    // Build genuine v1 input independently of the new install path: legacy
    // managed blobs existed, while configuration never had pristine copies.
    fn make_legacy(root: &Path) {
        let state = load_state(&PortalIo::open(root).unwrap()).unwrap().unwrap();
        let mut json = serde_json::to_value(&state).unwrap();
        json["schema_version"] = 1.into();
        json.as_object_mut().unwrap().remove("runtime_ownership");
        json.as_object_mut().unwrap().remove("generator");
        std::fs::write(root.join(STATE_PATH), serde_json::to_vec(&json).unwrap()).unwrap();
        std::fs::create_dir_all(root.join(BASELINE_ROOT)).unwrap();
        for (path, file) in &state.files {
            if file.ownership == "managed" {
                let bytes = std::fs::read(root.join(&state.root).join(path)).unwrap();
                assert_eq!(sha256_hex(&bytes), file.pristine_sha256);
                std::fs::write(root.join(baseline_path(&file.pristine_sha256)), bytes).unwrap();
            }
        }
    }

    #[test]
    fn binary_setup_and_pristine_update_preserve_exact_bytes() {
        let temp = initialized_root();
        let first = b"\0\xff\x80original";
        let next = b"\0\xfe\x81updated";
        setup_portal(&byte_source("1", first), temp.path(), Path::new("guide")).unwrap();
        let destination = temp.path().join("guide/managed.txt");
        assert_eq!(std::fs::read(&destination).unwrap(), first);
        assert!(!temp.path().join(BASELINE_ROOT).exists());
        let report = update_adopted_portal(&byte_source("2", next), temp.path())
            .unwrap()
            .unwrap();
        assert!(!report.has_conflicts());
        assert_eq!(std::fs::read(&destination).unwrap(), next);
        let repeated = update_adopted_portal(&byte_source("2", next), temp.path())
            .unwrap()
            .unwrap();
        assert!(repeated.files.iter().any(|file| {
            file.dest == "guide/managed.txt" && matches!(file.action, Action::Unchanged)
        }));
    }

    #[test]
    fn binary_user_edits_conflict_without_clobber_and_repeat_idempotently() {
        for (first, custom, next) in [
            (&b"\xfforiginal"[..], &b"\xfecustom"[..], &b"\xfdnext"[..]),
            (
                &b"a\0\nb\nc\n"[..],
                &b"custom\0\nb\nc\n"[..],
                &b"a\0\nb\nnext\n"[..],
            ),
            (&b"plain\n"[..], &b"\xffcustom"[..], &b"updated\n"[..]),
        ] {
            let temp = initialized_root();
            setup_portal(&byte_source("1", first), temp.path(), Path::new("guide")).unwrap();
            let destination = temp.path().join("guide/managed.txt");
            std::fs::write(&destination, custom).unwrap();
            let report = update_adopted_portal(&byte_source("2", next), temp.path())
                .unwrap()
                .unwrap();
            assert!(report.has_conflicts());
            let sidecar = temp.path().join(format!(
                "guide/managed.txt.codeflow-{}.new",
                &sha256_hex(next)[..12]
            ));
            assert_eq!(std::fs::read(&destination).unwrap(), custom);
            assert!(!sidecar.exists());
            let state = std::fs::read(temp.path().join(STATE_PATH)).unwrap();
            // A user's annotation to the sidecar must survive a repeated update too.
            std::fs::write(&sidecar, b"user retained conflict notes").unwrap();
            update_adopted_portal(&byte_source("2", next), temp.path()).unwrap();
            assert_eq!(std::fs::read(&destination).unwrap(), custom);
            assert_eq!(
                std::fs::read(&sidecar).unwrap(),
                b"user retained conflict notes"
            );
            assert_eq!(std::fs::read(temp.path().join(STATE_PATH)).unwrap(), state);
        }
    }

    #[test]
    fn text_updates_never_three_way_merge_independent_changes() {
        let temp = initialized_root();
        setup_portal(
            &source("1", "first\nmiddle\nlast\n"),
            temp.path(),
            Path::new("guide"),
        )
        .unwrap();
        let destination = temp.path().join("guide/managed.txt");
        std::fs::write(&destination, "custom\nmiddle\nlast\n").unwrap();
        let report = update_adopted_portal(&source("2", "first\nmiddle\nnext\n"), temp.path())
            .unwrap()
            .unwrap();
        assert!(report.has_conflicts());
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"custom\nmiddle\nlast\n"
        );
        assert!(!report
            .files
            .iter()
            .any(|file| matches!(file.action, Action::Merged)));
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
        assert!(!temp.path().join(BASELINE_ROOT).exists());

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
        assert!(!temp.path().join(BASELINE_ROOT).exists());
    }

    #[test]
    fn commit_phase_failure_rolls_back_files_and_created_directories() {
        let temp = initialized_root();
        let mut plan = PortalMutationPlan::default();
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            "guide/a.txt",
            b"a\n".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            "guide/b.txt",
            b"b\n".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        assert!(plan
            .commit_with_fault_after(&PortalIo::open(temp.path()).unwrap(), Path::new("guide"), 0)
            .is_err());
        assert!(!temp.path().join("guide/a.txt").exists());
        assert!(!temp.path().join("guide/b.txt").exists());
        assert!(!temp.path().join("guide").exists());
    }

    #[test]
    fn abrupt_transaction_is_recovered_by_roll_forward_at_every_boundary() {
        for boundary in 0..3 {
            let temp = initialized_root();
            std::fs::create_dir(temp.path().join("guide")).unwrap();
            std::fs::write(temp.path().join("guide/old.txt"), b"old\n").unwrap();
            let mut plan = PortalMutationPlan::default();
            plan.write(
                &PortalIo::open(temp.path()).unwrap(),
                "guide/a.txt",
                b"a\n".to_vec(),
                32,
                "fixture",
            )
            .unwrap();
            plan.remove(
                &PortalIo::open(temp.path()).unwrap(),
                "guide/old.txt",
                32,
                "fixture",
            )
            .unwrap();
            plan.write(
                &PortalIo::open(temp.path()).unwrap(),
                STATE_PATH,
                br#"{"schema_version":1}"#.to_vec(),
                MAX_STATE_BYTES,
                "fixture",
            )
            .unwrap();

            let error = plan
                .commit_with_abrupt_fault_after(
                    &PortalIo::open(temp.path()).unwrap(),
                    Path::new("guide"),
                    boundary,
                )
                .unwrap_err();
            assert!(error.to_string().contains("abrupt"));
            assert!(temp.path().join(TRANSACTION_PATH).is_dir());

            recover_portal_transaction(&PortalIo::open(temp.path()).unwrap(), Path::new("guide"))
                .unwrap();
            assert_eq!(
                std::fs::read(temp.path().join("guide/a.txt")).unwrap(),
                b"a\n"
            );
            assert!(!temp.path().join("guide/old.txt").exists());
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
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            STATE_PATH,
            b"state\n".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            "guide/a.txt",
            b"a\n".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            &active,
            b"new\n".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        plan.remove(
            &PortalIo::open(temp.path()).unwrap(),
            &retired,
            32,
            "fixture",
        )
        .unwrap();

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
        std::fs::create_dir(temp.path().join("guide")).unwrap();
        std::fs::write(temp.path().join("guide/value.txt"), b"before\n").unwrap();
        let mut plan = PortalMutationPlan::default();
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            "guide/value.txt",
            b"after\n".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        assert!(plan
            .commit_with_abrupt_fault_after(
                &PortalIo::open(temp.path()).unwrap(),
                Path::new("guide"),
                0
            )
            .is_err());
        std::fs::write(temp.path().join("guide/value.txt"), b"operator edit\n").unwrap();

        let error =
            recover_portal_transaction(&PortalIo::open(temp.path()).unwrap(), Path::new("guide"))
                .unwrap_err();
        assert!(error.to_string().contains("concurrent edit"));
        assert_eq!(
            std::fs::read(temp.path().join("guide/value.txt")).unwrap(),
            b"operator edit\n"
        );
        assert!(temp.path().join(TRANSACTION_PATH).is_dir());
    }

    #[test]
    fn portal_transaction_lease_excludes_concurrent_reconciliation() {
        let temp = initialized_root();
        let first =
            acquire_portal_transaction_lease(&PortalIo::open(temp.path()).unwrap()).unwrap();
        let error =
            acquire_portal_transaction_lease(&PortalIo::open(temp.path()).unwrap()).unwrap_err();
        assert!(error.to_string().contains("already in progress"));
        drop(first);
        acquire_portal_transaction_lease(&PortalIo::open(temp.path()).unwrap()).unwrap();

        std::fs::remove_file(temp.path().join(TRANSACTION_LOCK_PATH)).unwrap();
        std::fs::create_dir(temp.path().join(TRANSACTION_LOCK_PATH)).unwrap();
        let error =
            acquire_portal_transaction_lease(&PortalIo::open(temp.path()).unwrap()).unwrap_err();
        assert!(error.to_string().contains("not a regular file"));
    }

    #[test]
    fn transaction_recovery_rejects_tampered_staged_output() {
        let temp = initialized_root();
        let mut plan = PortalMutationPlan::default();
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            "guide/value.txt",
            b"after\n".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        assert!(plan
            .commit_with_abrupt_fault_after(
                &PortalIo::open(temp.path()).unwrap(),
                Path::new("guide"),
                0
            )
            .is_err());
        std::fs::remove_file(temp.path().join("guide/value.txt")).unwrap();
        std::fs::write(
            temp.path().join(TRANSACTION_PATH).join("after/0000"),
            b"other\n",
        )
        .unwrap();

        let error =
            recover_portal_transaction(&PortalIo::open(temp.path()).unwrap(), Path::new("guide"))
                .unwrap_err();
        assert!(error.to_string().contains("does not match its journal"));
        assert!(!temp.path().join("guide/value.txt").exists());
        assert!(temp.path().join(TRANSACTION_PATH).is_dir());
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One table-style test exercises every closed journal field and namespace.
    fn transaction_journal_validation_is_closed_and_bounded() {
        let temp = initialized_root();
        let valid = PortalJournalMutation {
            path: "guide/value.txt".into(),
            before_sha256: None,
            before_bytes: None,
            after_sha256: Some("a".repeat(64)),
            after_bytes: Some(1),
            staged_file: Some("after/0000".into()),
        };
        let validate = |mutation: &PortalJournalMutation,
                        paths: &mut BTreeSet<String>,
                        remaining: &mut u64| {
            validate_journal_mutation(
                &PortalIo::open(temp.path()).unwrap(),
                Path::new("guide"),
                0,
                mutation,
                paths,
                remaining,
            )
        };

        let mut paths = BTreeSet::new();
        let mut remaining = 2;
        validate(&valid, &mut paths, &mut remaining).unwrap();
        assert_eq!(remaining, 1);
        let error = validate(&valid, &mut paths, &mut remaining).unwrap_err();
        assert!(error.to_string().contains("repeats a destination"));

        let mut unsafe_path = valid.clone();
        unsafe_path.path = "guide\\value.txt".into();
        let error = validate(&unsafe_path, &mut BTreeSet::new(), &mut 2).unwrap_err();
        assert!(error.to_string().contains("unsafe destination"));

        let mut invalid_hash = valid.clone();
        invalid_hash.after_sha256 = Some("A".repeat(64));
        let error = validate(&invalid_hash, &mut BTreeSet::new(), &mut 2).unwrap_err();
        assert!(error.to_string().contains("invalid snapshot claim"));

        let mut oversized = valid.clone();
        oversized.after_bytes = Some(3);
        let error = validate(&oversized, &mut BTreeSet::new(), &mut 2).unwrap_err();
        assert!(error.to_string().contains("aggregate byte limit"));

        let mut invalid_stage = valid;
        invalid_stage.staged_file = Some("elsewhere/0000".into());
        let error = validate(&invalid_stage, &mut BTreeSet::new(), &mut 2).unwrap_err();
        assert!(error.to_string().contains("invalid staged output"));

        let mut aliased_stage = PortalJournalMutation {
            path: "guide/other.txt".into(),
            before_sha256: None,
            before_bytes: None,
            after_sha256: Some("b".repeat(64)),
            after_bytes: Some(1),
            staged_file: Some("after/0000".into()),
        };
        let error = validate_journal_mutation(
            &PortalIo::open(temp.path()).unwrap(),
            Path::new("guide"),
            1,
            &aliased_stage,
            &mut BTreeSet::new(),
            &mut 2,
        )
        .unwrap_err();
        assert!(error.to_string().contains("invalid staged output"));
        aliased_stage.staged_file = Some("after/0001".into());
        validate_journal_mutation(
            &PortalIo::open(temp.path()).unwrap(),
            Path::new("guide"),
            1,
            &aliased_stage,
            &mut BTreeSet::new(),
            &mut 2,
        )
        .unwrap();

        let mut outside = PortalJournalMutation {
            path: "outside.txt".into(),
            before_sha256: None,
            before_bytes: None,
            after_sha256: None,
            after_bytes: None,
            staged_file: None,
        };
        let error = validate_journal_mutation(
            &PortalIo::open(temp.path()).unwrap(),
            Path::new("guide"),
            0,
            &outside,
            &mut BTreeSet::new(),
            &mut 2,
        )
        .unwrap_err();
        assert!(error.to_string().contains("outside its adopted namespaces"));
        outside.path = format!("{BASELINE_ROOT}/not-a-content-hash");
        let error = validate_journal_mutation(
            &PortalIo::open(temp.path()).unwrap(),
            Path::new("guide"),
            0,
            &outside,
            &mut BTreeSet::new(),
            &mut 2,
        )
        .unwrap_err();
        assert!(error.to_string().contains("outside its adopted namespaces"));
        outside.path = format!("{BASELINE_ROOT}/{}", "a".repeat(64));
        let error = validate_journal_mutation(
            &PortalIo::open(temp.path()).unwrap(),
            Path::new("guide"),
            0,
            &outside,
            &mut BTreeSet::new(),
            &mut 2,
        )
        .unwrap_err();
        assert!(error.to_string().contains("content hash"));
    }

    #[test]
    fn recovery_is_bound_to_the_exact_portal_root_and_namespaces() {
        let temp = initialized_root();
        let mut plan = PortalMutationPlan::default();
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            "guide/value.txt",
            b"after\n".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        assert!(plan
            .commit_with_abrupt_fault_after(
                &PortalIo::open(temp.path()).unwrap(),
                Path::new("guide"),
                0
            )
            .is_err());

        let error =
            recover_portal_transaction(&PortalIo::open(temp.path()).unwrap(), Path::new("other"))
                .unwrap_err();
        assert!(error.to_string().contains("mismatched envelope"));
        assert!(temp.path().join(TRANSACTION_PATH).is_dir());

        let manifest_path = temp.path().join(TRANSACTION_PATH).join("manifest.json");
        let mut journal: PortalTransactionJournal =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        journal.state_path = ".codeflow/other.json".into();
        std::fs::write(&manifest_path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();
        let error =
            recover_portal_transaction(&PortalIo::open(temp.path()).unwrap(), Path::new("guide"))
                .unwrap_err();
        assert!(error.to_string().contains("mismatched envelope"));
    }

    #[test]
    fn completed_transaction_tombstone_is_recovered_by_matching_identity() {
        let temp = initialized_root();
        let mut plan = PortalMutationPlan::default();
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            "guide/value.txt",
            b"after\n".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        assert!(plan
            .commit_with_abrupt_fault_after(
                &PortalIo::open(temp.path()).unwrap(),
                Path::new("guide"),
                0
            )
            .is_err());
        let transaction = temp.path().join(TRANSACTION_PATH);
        let journal =
            read_transaction_journal(&PortalIo::open(temp.path()).unwrap(), TRANSACTION_PATH)
                .unwrap();
        let tombstone = temp.path().join(format!(
            ".codeflow/{TRANSACTION_CLEANUP_PREFIX}{}",
            journal.transaction_id
        ));
        std::fs::rename(&transaction, &tombstone).unwrap();

        recover_portal_transaction(&PortalIo::open(temp.path()).unwrap(), Path::new("guide"))
            .unwrap();
        assert!(!tombstone.exists());
        assert_eq!(
            std::fs::read(temp.path().join("guide/value.txt")).unwrap(),
            b"after\n"
        );
    }

    #[test]
    fn recovery_refuses_malformed_journal_shapes_and_cleans_abandoned_stages() {
        let temp = initialized_root();
        let transaction = temp.path().join(TRANSACTION_PATH);
        std::fs::write(&transaction, b"not a directory").unwrap();
        assert!(recover_portal_transaction(
            &PortalIo::open(temp.path()).unwrap(),
            Path::new("guide")
        )
        .unwrap_err()
        .to_string()
        .contains("not a regular directory"));
        std::fs::remove_file(&transaction).unwrap();

        std::fs::create_dir(&transaction).unwrap();
        assert!(recover_portal_transaction(
            &PortalIo::open(temp.path()).unwrap(),
            Path::new("guide")
        )
        .unwrap_err()
        .to_string()
        .contains("journal is unreadable"));
        std::fs::write(
            transaction.join("manifest.json"),
            br#"{"schema_version":2,"transaction_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","portal_root":"guide","state_path":".codeflow/docs-portal.json","baseline_root":".codeflow/.docs-portal-baseline","mutations":[]}"#,
        )
        .unwrap();
        assert!(recover_portal_transaction(
            &PortalIo::open(temp.path()).unwrap(),
            Path::new("guide")
        )
        .unwrap_err()
        .to_string()
        .contains("invalid or mismatched envelope"));
        std::fs::remove_dir_all(&transaction).unwrap();

        let abandoned = temp
            .path()
            .join(".codeflow/.docs-portal-transaction-stage-123-456");
        std::fs::create_dir(&abandoned).unwrap();
        recover_portal_transaction(&PortalIo::open(temp.path()).unwrap(), Path::new("guide"))
            .unwrap();
        assert!(!abandoned.exists());

        std::fs::write(&abandoned, b"wrong type").unwrap();
        assert!(recover_portal_transaction(
            &PortalIo::open(temp.path()).unwrap(),
            Path::new("guide")
        )
        .unwrap_err()
        .to_string()
        .contains("not a regular directory"));
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
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            "first.txt",
            b"one".to_vec(),
            8,
            "fixture",
        )
        .unwrap();
        let error = plan
            .write(
                &PortalIo::open(temp.path()).unwrap(),
                "second.txt",
                b"two".to_vec(),
                8,
                "fixture",
            )
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
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            "first.txt",
            b"123".to_vec(),
            8,
            "fixture",
        )
        .unwrap();
        let error = plan
            .write(
                &PortalIo::open(temp.path()).unwrap(),
                "second.txt",
                b"456".to_vec(),
                8,
                "fixture",
            )
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
            &PortalIo::open(temp.path()).unwrap(),
            "first.txt",
            b"after one".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        plan.write(
            &PortalIo::open(temp.path()).unwrap(),
            "second.txt",
            b"after two".to_vec(),
            32,
            "fixture",
        )
        .unwrap();
        std::fs::write(temp.path().join("second.txt"), b"concurrent edit").unwrap();

        let error = plan
            .commit(&PortalIo::open(temp.path()).unwrap(), Path::new("guide"))
            .unwrap_err();
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
        make_legacy(baseline.path());
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
        make_legacy(baseline.path());
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
        let sidecar = temp.path().join(format!(
            "guide/managed.txt.codeflow-{}.new",
            &sha256_hex(b"new\n")[..12]
        ));
        std::fs::write(&sidecar, "user-owned\n").unwrap();
        assert!(
            setup_portal(&source("2.0.0", "new\n"), temp.path(), Path::new("guide"))
                .unwrap()
                .has_conflicts()
        );
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
    fn fresh_and_repeated_adoption_never_create_baselines() {
        let temp = initialized_root();
        let source = DirSource::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
        for _ in 0..2 {
            assert!(!setup_portal(&source, temp.path(), Path::new("guide"))
                .unwrap()
                .has_conflicts());
            assert!(!temp.path().join(BASELINE_ROOT).exists());
            let state = load_state(&PortalIo::open(temp.path()).unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(state.schema_version, 2);
            assert_eq!(state.runtime_ownership, RuntimeOwnership::Managed);
        }
    }

    #[test]
    fn corrupted_legacy_baseline_stops_migration_and_transfer() {
        let temp = initialized_root();
        setup_portal(&source("1.0.0", "old\n"), temp.path(), Path::new("guide")).unwrap();
        make_legacy(temp.path());
        let baseline = temp.path().join(baseline_path(&sha256_hex(b"old\n")));
        std::fs::write(&baseline, "corrupt\n").unwrap();
        std::fs::write(temp.path().join("guide/managed.txt"), "user edit\n").unwrap();
        let before = std::fs::read(temp.path().join(STATE_PATH)).unwrap();
        assert!(setup_portal(&source("2.0.0", "new\n"), temp.path(), Path::new("guide")).is_err());
        assert!(transfer_portal(temp.path(), true).is_err());
        assert_eq!(std::fs::read(temp.path().join(STATE_PATH)).unwrap(), before);
        assert_eq!(std::fs::read(baseline).unwrap(), b"corrupt\n");
        assert_eq!(
            std::fs::read(temp.path().join("guide/managed.txt")).unwrap(),
            b"user edit\n"
        );
    }

    /// A file named `caf\xe9`, which is not valid UTF-8, in `directory`.
    /// `false` when the file system refuses the name (APFS does), so the tests
    /// below run where they can, as on Linux.
    #[cfg(unix)]
    fn write_latin1_file(directory: &Path) -> bool {
        use std::os::unix::ffi::OsStrExt as _;
        std::fs::write(
            directory.join(std::ffi::OsStr::from_bytes(b"caf\xe9")),
            b"x",
        )
        .is_ok()
    }

    /// Kept strict (issue 79): a legacy baseline entry that is not valid UTF-8
    /// is unknown content, and the migration preserves all content.
    #[cfg(unix)]
    #[test]
    fn a_legacy_baseline_entry_that_is_not_utf8_stops_migration() {
        let temp = initialized_root();
        setup_portal(&source("1.0.0", "old\n"), temp.path(), Path::new("guide")).unwrap();
        make_legacy(temp.path());
        if !write_latin1_file(&temp.path().join(BASELINE_ROOT)) {
            return;
        }
        let before = std::fs::read(temp.path().join(STATE_PATH)).unwrap();
        let error =
            setup_portal(&source("2.0.0", "new\n"), temp.path(), Path::new("guide")).unwrap_err();
        assert!(error.to_string().contains("non-UTF-8"), "{error}");
        assert_eq!(std::fs::read(temp.path().join(STATE_PATH)).unwrap(), before);
    }

    /// Kept strict (issue 79): a staged output that is not valid UTF-8 is not
    /// one of the transaction's own outputs, so the cleanup refuses it.
    #[cfg(unix)]
    #[test]
    fn a_staged_output_that_is_not_utf8_stops_the_cleanup() {
        let temp = initialized_root();
        let transaction = temp.path().join(TRANSACTION_PATH);
        std::fs::create_dir_all(transaction.join("after")).unwrap();
        if !write_latin1_file(&transaction.join("after")) {
            return;
        }
        let journal: PortalTransactionJournal = serde_json::from_str(
            r#"{"schema_version":2,"transaction_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","portal_root":"guide","state_path":".codeflow/docs-portal.json","baseline_root":".codeflow/.docs-portal-baseline","mutations":[]}"#,
        )
        .unwrap();
        let error = authenticated_cleanup_outputs(
            &PortalIo::open(temp.path()).unwrap(),
            TRANSACTION_PATH,
            &journal,
            Path::new("guide"),
            true,
        )
        .unwrap_err();
        assert!(error.to_string().contains("non-UTF-8"), "{error}");
    }

    #[test]
    fn successful_migration_removes_only_authenticated_legacy_blobs() {
        let temp = initialized_root();
        setup_portal(&source("1.0.0", "old\n"), temp.path(), Path::new("guide")).unwrap();
        make_legacy(temp.path());
        let report =
            setup_portal(&source("2.0.0", "new\n"), temp.path(), Path::new("guide")).unwrap();
        assert!(!report.has_conflicts());
        assert!(!temp
            .path()
            .join(baseline_path(&sha256_hex(b"old\n")))
            .exists());
        assert!(!temp
            .path()
            .join(baseline_path(&sha256_hex(b"new\n")))
            .exists());
        assert_eq!(
            load_state(&PortalIo::open(temp.path()).unwrap())
                .unwrap()
                .unwrap()
                .schema_version,
            2
        );
        assert!(report
            .files
            .iter()
            .any(|file| file.action == Action::Removed && file.dest.starts_with(BASELINE_ROOT)));
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
