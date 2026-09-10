//! A bounded identity index, not validation of unrelated record relationships.

use super::{ForecastReport, Reader};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

struct Entry {
    path: PathBuf,
    claims: BTreeSet<String>,
    valid: bool,
}

pub(super) struct RecordIndex(Vec<Entry>);

impl RecordIndex {
    pub(super) fn read(
        reader: &mut Reader,
        paths: &[PathBuf],
        valid_id: fn(&str) -> bool,
        expected: &str,
        report: &mut ForecastReport,
    ) -> Self {
        let mut entries = Vec::new();
        // Inventory size and every source read are bounded before this pass.
        // Inspect metadata once, including alternate filenames, then retain only
        // identity claims. Do not follow dependencies or validate unrelated work.
        for path in paths {
            let Some((_, fields)) = reader.record(path, report) else {
                report.finding(
                    "identity_scan_incomplete",
                    "sources",
                    "The bounded work-record identity scan could not be completed.",
                );
                break; // An incomplete identity scan can never produce validity.
            };
            let (canonical, errors, _) =
                crate::validate::canonical_identity(path, &fields, valid_id, expected);
            let claims = [
                path.file_stem().and_then(|stem| stem.to_str()),
                fields.get("id").and_then(serde_yaml::Value::as_str),
                fields.get("format_id").and_then(serde_yaml::Value::as_str),
            ]
            .into_iter()
            .flatten()
            .map(str::to_owned)
            .collect();
            entries.push(Entry {
                path: path.clone(),
                claims,
                valid: !canonical.is_empty() && errors.is_empty(),
            });
        }
        Self(entries)
    }

    pub(super) fn resolve(
        &self,
        id: &str,
        report: &mut ForecastReport,
        code: &'static str,
    ) -> Option<&Path> {
        let mut candidates = self.0.iter().filter(|entry| entry.claims.contains(id));
        let first = candidates.next();
        if first.is_none() || candidates.next().is_some() {
            report.finding(
                code,
                "sources",
                "Work-record identity is missing or ambiguous across the bounded inventory.",
            );
            return None;
        }
        let first = first?;
        if !first.valid {
            report.finding("source_record", "sources", "Work-record filename, identity, or historical alias conflicts with the canonical identity contract.");
            return None;
        }
        Some(&first.path)
    }
}
