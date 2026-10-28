//! Shipped content retained until a person finishes a conflict proposal.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::scaffold::state::{
    guard_beneath_root, read_beneath_root, write_record, Baseline, InstalledManifest,
};
use crate::scaffold::{hash, ScaffoldError};

#[derive(Serialize, Deserialize)]
pub(super) struct PendingResolution {
    old_sha256: String,
    pub new_sha256: String,
    pub shipped: String,
}

pub(super) fn path(dest: &str) -> String {
    format!(".codeflow/.pending-update/{dest}.json")
}

fn invalid(dest: &str, detail: &str) -> ScaffoldError {
    ScaffoldError::InvalidState {
        what: format!("update resolution for {dest}"),
        detail: format!("{detail}; update changed nothing"),
    }
}

impl PendingResolution {
    /// Refuse before any update writes. Both old and new hashes are allowed
    /// independently, so an interrupted baseline/manifest write can resume.
    pub fn read(
        root: &Path,
        dest: &str,
        installed: &InstalledManifest,
    ) -> Result<Option<Self>, ScaffoldError> {
        let text = read_beneath_root(root, &path(dest))?;
        // Preserve update's existing skip for an unsafe live destination when
        // it has no pending resolution. Pending paths always fail closed.
        if text.is_none()
            && matches!(
                guard_beneath_root(root, Path::new(dest)),
                Err(ScaffoldError::UnsafeSymlink { .. })
            )
        {
            return Ok(None);
        }
        if read_beneath_root(root, &format!("{dest}.new"))?.is_some() {
            return Err(invalid(
                dest,
                &format!("resolve {dest}.new and move it over {dest}, then retry"),
            ));
        }
        let Some(text) = text else {
            return Ok(None);
        };
        let pending: Self = serde_json::from_str(&text).map_err(|error| {
            invalid(
                dest,
                &format!(
                    "cannot read {}: {}; restore the pending record",
                    path(dest),
                    crate::scaffold::json_error(&error)
                ),
            )
        })?;
        let baseline = Baseline::read(root, dest)?;
        let recorded = installed.files.get(dest).map(|file| file.sha256.as_str());
        let base_hash = baseline
            .as_ref()
            .map(|text| hash::sha256_hex(text.as_bytes()));
        let known = |value: Option<&str>| {
            value.is_some_and(|value| value == pending.old_sha256 || value == pending.new_sha256)
        };
        if hash::sha256_hex(pending.shipped.as_bytes()) != pending.new_sha256
            || !known(recorded)
            || !known(base_hash.as_deref())
        {
            return Err(invalid(
                dest,
                "baseline, manifest or pending content does not match the recorded shipped versions; restore the metadata from a known-good copy and retry",
            ));
        }
        let current = read_beneath_root(root, dest)?;
        let Some(current) = current else {
            return Err(invalid(
                dest,
                "resolved file is missing; restore it and retry",
            ));
        };
        // diffy's proposal always uses seven-character markers. Check each
        // marker independently, including a leftover separator.
        if current.split_terminator('\n').any(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            ["<<<<<<<", "|||||||", "=======", ">>>>>>>"]
                .iter()
                .any(|marker| line == *marker || line.starts_with(&format!("{marker} ")))
        }) {
            return Err(invalid(
                dest,
                "resolve the remaining conflict markers and retry",
            ));
        }
        Ok(Some(pending))
    }

    pub fn write(root: &Path, dest: &str, base: &str, shipped: &str) -> Result<(), ScaffoldError> {
        let pending = Self {
            old_sha256: hash::sha256_hex(base.as_bytes()),
            new_sha256: hash::sha256_hex(shipped.as_bytes()),
            shipped: shipped.to_string(),
        };
        write_record(root, &path(dest), &serde_json::to_vec_pretty(&pending)?)
    }
}
