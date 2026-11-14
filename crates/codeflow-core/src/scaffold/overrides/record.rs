//! What update records about the overrides it applied to a file, and the
//! journal that makes one file's update recoverable (TSK-1302).
//!
//! The override record sits beside the pristine baseline, at
//! `.codeflow/.baseline/<dest>.overrides.json`. It never replaces the
//! pristine invariant: the installed manifest's hash and the baseline stay
//! the shipped text `P`. The record adds the applied set `A` with every
//! unit's shipped and replacement text, the hash of the effective baseline
//! `E = apply(P, A)` (never a hand edit), and the hash `W` of what update
//! actually wrote, which is only for recovery and doctor.
//!
//! The journal sits at `.codeflow/.pending-update/<dest>.overrides.json`
//! and is written before the file is touched. It binds the old state, the
//! intended new metadata and the hash of the exact bytes to be written, so
//! the next update can roll an interrupted write forward, drop a journal
//! whose write never landed, or refuse a state that matches neither.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::Entry;
use crate::scaffold::state::{read_beneath_root, remove_beneath_root, write_record};
use crate::scaffold::{hash, ScaffoldError, Shown};
use crate::shown;

const RECORD_SCHEMA: u32 = 1;
const JOURNAL_SCHEMA: u32 = 1;

/// One unit update applied: the declared entry, the shipped text it
/// replaced and the text it wrote in its place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedUnit {
    pub entry: Entry,
    /// The shipped unit text: the row line, or the section body.
    pub replaced: String,
    /// What was written in its place.
    pub replacement: String,
}

/// The override record of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema_version: u32,
    /// Counts up by one with every update that changes the record.
    pub generation: u64,
    /// The hash of the pristine shipped text the overrides were applied to
    /// (the managed block for a managed-region file).
    pub pristine_sha256: String,
    /// The applied set, in declaration order.
    pub applied: Vec<AppliedUnit>,
    /// The hash of the pristine text with the applied set, and nothing else.
    pub effective_sha256: String,
    /// The hash of the whole file as update last wrote or adopted it.
    pub written_sha256: String,
}

/// The record path of `dest`, repository relative.
#[must_use]
pub fn record_path(dest: &str) -> String {
    format!(".codeflow/.baseline/{dest}.overrides.json")
}

/// The journal path of `dest`, repository relative.
#[must_use]
pub fn journal_path(dest: &str) -> String {
    format!(".codeflow/.pending-update/{dest}.overrides.json")
}

#[allow(clippy::needless_pass_by_value)] // built at each call site, used once
fn invalid(dest: &str, detail: Shown) -> ScaffoldError {
    ScaffoldError::invalid(
        shown!("override state of {}", super::dest_shown(dest)),
        shown!("{}; update changed nothing", detail),
    )
}

impl Record {
    /// A new record.
    #[must_use]
    pub fn new(
        generation: u64,
        pristine: &str,
        applied: Vec<AppliedUnit>,
        effective: &str,
        written: &str,
    ) -> Self {
        Self {
            schema_version: RECORD_SCHEMA,
            generation,
            pristine_sha256: hash::sha256_hex(pristine.as_bytes()),
            applied,
            effective_sha256: hash::sha256_hex(effective.as_bytes()),
            written_sha256: hash::sha256_hex(written.as_bytes()),
        }
    }

    /// The record of `dest`, `None` when there is none.
    ///
    /// # Errors
    ///
    /// The record cannot be read, does not parse or has another schema.
    pub fn read(root: &Path, dest: &str) -> Result<Option<Self>, ScaffoldError> {
        let Some(text) = read_beneath_root(root, &record_path(dest))? else {
            return Ok(None);
        };
        let record: Self = serde_json::from_str(&text)
            .map_err(|_| invalid(dest, Shown::fixed("the override record does not parse")))?;
        if record.schema_version != RECORD_SCHEMA {
            return Err(invalid(
                dest,
                Shown::fixed("the override record has an unknown schema version"),
            ));
        }
        Ok(Some(record))
    }

    /// The record's bytes as written.
    ///
    /// # Panics
    ///
    /// Never in practice: the record holds only strings and numbers, which
    /// always serialize.
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        let mut text = serde_json::to_string_pretty(self).expect("a record serializes");
        text.push('\n');
        text.into_bytes()
    }

    /// Write the record of `dest`.
    ///
    /// # Errors
    ///
    /// IO failures, or a symlinked record path.
    pub fn write(&self, root: &Path, dest: &str) -> Result<(), ScaffoldError> {
        write_record(root, &record_path(dest), &self.bytes())
    }
}

/// Remove the record of `dest`; a missing record is not an error.
///
/// # Errors
///
/// IO failures, or a symlinked record path.
pub fn remove_record(root: &Path, dest: &str) -> Result<(), ScaffoldError> {
    remove_beneath_root(root, &record_path(dest))
}

fn file_hash(root: &Path, rel: &str) -> Result<Option<String>, ScaffoldError> {
    Ok(read_beneath_root(root, rel)?.map(|text| hash::sha256_hex(text.as_bytes())))
}

/// The metadata of one file at one moment: the destination's hash, the
/// pristine baseline's hash, the override record's hash and the installed
/// manifest's hash for it. `None` means absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generation {
    pub file: Option<String>,
    pub baseline: Option<String>,
    pub record: Option<String>,
    pub installed: Option<String>,
}

/// The journal of one file's update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub schema_version: u32,
    /// The state before this update touched the file.
    pub old: Generation,
    /// The state this update writes.
    pub new: Generation,
    /// The pristine baseline this update writes.
    pub baseline_text: String,
    /// The override record this update writes; `None` removes it.
    pub record: Option<Record>,
}

impl Journal {
    /// The state of `dest` on disk now, with `installed` the hash the
    /// installed manifest holds in memory.
    ///
    /// # Errors
    ///
    /// A file cannot be read.
    pub fn observe(
        root: &Path,
        dest: &str,
        installed: Option<&str>,
    ) -> Result<Generation, ScaffoldError> {
        Ok(Generation {
            file: file_hash(root, dest)?,
            baseline: file_hash(root, &format!(".codeflow/.baseline/{dest}"))?,
            record: file_hash(root, &record_path(dest))?,
            installed: installed.map(str::to_string),
        })
    }

    /// A journal from the observed old state to the new one.
    #[must_use]
    pub fn new(
        old: Generation,
        written: &str,
        baseline_text: &str,
        record: Option<Record>,
        installed: &str,
    ) -> Self {
        let new = Generation {
            file: Some(hash::sha256_hex(written.as_bytes())),
            baseline: Some(hash::sha256_hex(baseline_text.as_bytes())),
            record: record
                .as_ref()
                .map(|record| hash::sha256_hex(&record.bytes())),
            installed: Some(installed.to_string()),
        };
        Self {
            schema_version: JOURNAL_SCHEMA,
            old,
            new,
            baseline_text: baseline_text.to_string(),
            record,
        }
    }

    /// Write the journal of `dest`, atomically.
    ///
    /// # Errors
    ///
    /// IO failures, or a symlinked journal path.
    ///
    /// # Panics
    ///
    /// Never in practice: the journal holds only strings and numbers, which
    /// always serialize.
    pub fn write(&self, root: &Path, dest: &str) -> Result<(), ScaffoldError> {
        let mut text = serde_json::to_string_pretty(self).expect("a journal serializes");
        text.push('\n');
        write_record(root, &journal_path(dest), text.as_bytes())
    }

    /// The journal of `dest`, `None` when there is none.
    ///
    /// # Errors
    ///
    /// The journal cannot be read, does not parse or has another schema:
    /// unverifiable state, refused.
    pub fn read(root: &Path, dest: &str) -> Result<Option<Self>, ScaffoldError> {
        let Some(text) = read_beneath_root(root, &journal_path(dest))? else {
            return Ok(None);
        };
        let journal: Self = serde_json::from_str(&text).map_err(|_| {
            invalid(
                dest,
                Shown::fixed("an interrupted update left a journal that does not parse"),
            )
        })?;
        if journal.schema_version != JOURNAL_SCHEMA {
            return Err(invalid(
                dest,
                Shown::fixed("an interrupted update left a journal with an unknown schema version"),
            ));
        }
        // The payload a roll forward writes must be the bytes the journal's
        // new state names: the baseline text and the record, by digest.
        let baseline = Some(hash::sha256_hex(journal.baseline_text.as_bytes()));
        let record = journal
            .record
            .as_ref()
            .map(|record| hash::sha256_hex(&record.bytes()));
        if baseline != journal.new.baseline || record != journal.new.record {
            return Err(invalid(
                dest,
                Shown::fixed(
                    "an interrupted update left a journal whose baseline or record does not match the digests it names",
                ),
            ));
        }
        Ok(Some(journal))
    }
}

/// Remove the journal of `dest`; a missing journal is not an error.
///
/// # Errors
///
/// IO failures, or a symlinked journal path.
pub fn remove_journal(root: &Path, dest: &str) -> Result<(), ScaffoldError> {
    remove_beneath_root(root, &journal_path(dest))
}

/// What the next update does with a journal it finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// The file holds the intended bytes and every metadata part is old or
    /// new: write the new metadata and drop the journal.
    RollForward,
    /// Nothing of the update landed: drop the journal and update afresh.
    Discard,
    /// Anything else: refuse and overwrite nothing.
    Refuse,
}

/// Classify the state found on disk (`now`) against `journal`.
#[must_use]
pub fn classify(journal: &Journal, now: &Generation) -> Recovery {
    let (old, new) = (&journal.old, &journal.new);
    let either = |current: &Option<String>, a: &Option<String>, b: &Option<String>| {
        current == a || current == b
    };
    if now.file == new.file
        && either(&now.baseline, &old.baseline, &new.baseline)
        && either(&now.record, &old.record, &new.record)
        && either(&now.installed, &old.installed, &new.installed)
    {
        return Recovery::RollForward;
    }
    if now == old {
        return Recovery::Discard;
    }
    Recovery::Refuse
}

/// The refusal for a journal that matches neither generation.
#[must_use]
pub fn refusal(dest: &str) -> ScaffoldError {
    invalid(
        dest,
        Shown::fixed(
            "an interrupted update left this file in a state that matches neither the state before it nor the state it was writing; \
             check the file, its baseline under .codeflow/.baseline and its journal under .codeflow/.pending-update, \
             restore one coherent state, then rerun codeflow update",
        ),
    )
}

/// The refusal for a landed write whose journal holds metadata other than
/// what this update writes for the file.
#[must_use]
pub fn payload_refusal(dest: &str) -> ScaffoldError {
    invalid(
        dest,
        Shown::fixed(
            "an interrupted update left a journal whose baseline or override record is not the shipped text and \
             declared overrides this update applies; restore the file and its metadata to one coherent state, \
             or rerun with the build and .codeflow/overrides.toml of the interrupted update",
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::unnecessary_wraps)] // the generation fields are options
    fn h(text: &str) -> Option<String> {
        Some(hash::sha256_hex(text.as_bytes()))
    }

    fn journal() -> Journal {
        let old = Generation {
            file: h("old file"),
            baseline: h("old base"),
            record: None,
            installed: h("old base"),
        };
        Journal::new(old, "new file", "new base", None, "new-installed")
    }

    #[test]
    fn a_landed_write_rolls_forward_with_any_mix_of_metadata() {
        let journal = journal();
        let mut now = journal.old.clone();
        now.file = journal.new.file.clone();
        assert_eq!(classify(&journal, &now), Recovery::RollForward);
        now.baseline = journal.new.baseline.clone();
        assert_eq!(classify(&journal, &now), Recovery::RollForward);
        now.installed = journal.new.installed.clone();
        assert_eq!(classify(&journal, &now), Recovery::RollForward);
    }

    #[test]
    fn nothing_landed_discards_and_a_mixed_state_refuses() {
        let journal = journal();
        assert_eq!(classify(&journal, &journal.old.clone()), Recovery::Discard);
        let mut mixed = journal.old.clone();
        mixed.file = h("a third text");
        assert_eq!(classify(&journal, &mixed), Recovery::Refuse);
        let mut stale = journal.old.clone();
        stale.baseline = journal.new.baseline.clone();
        assert_eq!(
            classify(&journal, &stale),
            Recovery::Refuse,
            "new metadata with the old file is not a state update writes"
        );
        let mut foreign = journal.old.clone();
        foreign.file = journal.new.file.clone();
        foreign.record = h("someone else's record");
        assert_eq!(classify(&journal, &foreign), Recovery::Refuse);
    }
}
