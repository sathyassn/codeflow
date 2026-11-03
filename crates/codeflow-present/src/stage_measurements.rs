//! Advisory export geometry. A measurement never changes authored content.
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    document::{Block, PresentationDocument},
    error::{PresentError, Result},
};

// Seven stage viewports, each measured in three themes and two modes.
pub(crate) const CONTENT_WIDTHS: [f64; 7] = [360.0, 372.8, 480.0, 640.0, 768.0, 832.0, 900.0];
pub(crate) const MIN_WIDTH: f64 = 320.0;
pub(crate) const MAX_WIDTH: f64 = 1920.0;
pub(crate) const MAX_ENTRIES_PER_STAGE: usize = CONTENT_WIDTHS.len() * 6;
pub(crate) const MAX_HEIGHT: f64 = 1_000_000.0;
pub(crate) const MIN_HEIGHT: f64 = 24.0;

/// An advisory report either updates its revision's state or loses a race
/// with a newer revision and is acknowledged without changing state.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StageMeasurementOutcome {
    Recorded,
    IgnoredStaleRevision,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct StageMeasurement {
    pub stage_id: String,
    pub revision: u64,
    pub content_width: f64,
    pub contract: String,
    pub height: f64,
    pub theme: String,
    pub mode: String,
}

pub(crate) fn rendering_contract(utility_style: &str) -> String {
    let mut digest = Sha256::new();
    for bytes in [
        include_bytes!("../web/src/styles.css").as_slice(),
        include_bytes!("../web/src/figure.css").as_slice(),
        include_bytes!("../web/src/export-fallback.css").as_slice(),
        include_bytes!("../web/src/export-stages.ts").as_slice(),
        include_bytes!("../web/src/stage-measuring.ts").as_slice(),
        include_bytes!("../web/src/export.ts").as_slice(),
        include_bytes!("../web/src/fonts/archivo-latin-wght-normal.woff2").as_slice(),
        include_bytes!("../web/src/fonts/inter-latin-wght-normal.woff2").as_slice(),
        include_bytes!("../web/src/fonts/ibm-plex-sans-latin-wght-normal.woff2").as_slice(),
        utility_style.as_bytes(),
    ] {
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    digest.update(b"export geometry v2; measured viewport retained; scrolling=no; floor=24");
    for width in CONTENT_WIDTHS.into_iter().chain([MIN_WIDTH, MAX_WIDTH]) {
        digest.update(width.to_le_bytes());
    }
    digest.update((MAX_ENTRIES_PER_STAGE as u64).to_le_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

impl StageMeasurement {
    pub(crate) fn validate(
        &self,
        document: &PresentationDocument,
        revision: u64,
        contract: &str,
    ) -> Result<()> {
        if self.revision != revision
            || self.contract != contract
            || !self.height.is_finite()
            || !(MIN_HEIGHT..=MAX_HEIGHT).contains(&self.height)
            || !self.content_width.is_finite()
            || !(MIN_WIDTH..=MAX_WIDTH).contains(&self.content_width)
            || !["graphite", "slate", "sage"].contains(&self.theme.as_str())
            || !["light", "dark"].contains(&self.mode.as_str())
            || !document
                .walk()
                .iter()
                .any(|block| matches!(block, Block::Html { .. }) && block.id() == self.stage_id)
        {
            return Err(PresentError::InvalidRequest(
                "stage measurement does not match the current export geometry".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn same_key(&self, other: &Self) -> bool {
        self.stage_id == other.stage_id
            && self.revision == other.revision
            && self.contract == other.contract
            && (self.content_width - other.content_width).abs() <= 1.0 / 64.0
            && self.theme == other.theme
            && self.mode == other.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::ParsedDocument, state::SessionStore};
    use tempfile::tempdir;

    fn document() -> PresentationDocument {
        serde_json::from_value(serde_json::json!({
            "schema_version": 2, "title": "Measured stages", "blocks": [
                {"type": "html", "id": "tall", "title": "Tall stage", "caption": "Nine rows.", "html": "<div style=\"height:900px\">Nine rows</div>"},
                {"type": "html", "id": "short", "title": "Short stage", "caption": "One row.", "html": "<p>One row</p>"}
            ]
        })).unwrap()
    }

    fn report() -> StageMeasurement {
        StageMeasurement {
            stage_id: "tall".into(),
            revision: 1,
            content_width: 832.0,
            contract: rendering_contract(""),
            height: 916.0,
            theme: "graphite".into(),
            mode: "light".into(),
        }
    }

    #[test]
    fn reports_require_known_geometry_revision_stage_and_contract() {
        let document = document();
        let valid = report();
        valid.validate(&document, 1, &valid.contract).unwrap();
        for width in [
            0.0,
            MIN_WIDTH - 1.0,
            MAX_WIDTH + 1.0,
            f64::NAN,
            f64::INFINITY,
        ] {
            let mut invalid = valid.clone();
            invalid.content_width = width;
            assert!(invalid.validate(&document, 1, &valid.contract).is_err());
        }
        for height in [0.0, 23.99, MAX_HEIGHT + 1.0, f64::NAN, f64::INFINITY] {
            let mut invalid = valid.clone();
            invalid.height = height;
            assert!(invalid.validate(&document, 1, &valid.contract).is_err());
        }
        for height in [MIN_HEIGHT, MAX_HEIGHT] {
            let mut accepted = valid.clone();
            accepted.height = height;
            accepted.validate(&document, 1, &valid.contract).unwrap();
        }
        let mut invalid = valid.clone();
        invalid.stage_id = "unknown".into();
        assert!(invalid.validate(&document, 1, &valid.contract).is_err());
        assert!(valid.validate(&document, 2, &valid.contract).is_err());
        assert!(valid.validate(&document, 1, "obsolete").is_err());
        let mut encoded = serde_json::to_value(&valid).unwrap();
        encoded["height"] = "900px".into();
        assert!(serde_json::from_value::<StageMeasurement>(encoded).is_err());
    }

    #[test]
    fn a_later_report_replaces_the_same_key_and_export_drops_old_contracts() {
        let scratch = tempdir().unwrap();
        let store = SessionStore::at_root(scratch.path().join("project"), "key".into()).unwrap();
        let session = store.create(ParsedDocument::Supported(document())).unwrap();
        let mut measurement = report();
        store
            .record_stage_measurement(session.id, measurement.clone())
            .unwrap();
        measurement.height = 1000.0;
        store
            .record_stage_measurement(session.id, measurement.clone())
            .unwrap();
        assert_eq!(
            store.stage_measurements(session.id).unwrap(),
            vec![measurement.clone()]
        );
        // A stored record from a different stylesheet/root scale cannot be reused.
        measurement.contract = rendering_contract(":root{font-size:18px}");
        crate::state::write_json_atomic(
            &store
                .session_dir(session.id)
                .join("stage-measurements.json"),
            &vec![measurement],
        )
        .unwrap();
        let output = scratch.path().join("old.html");
        crate::export::export_session(
            &store,
            session.id,
            &output,
            crate::export::ExportTheme::Graphite,
            crate::export::ExportMode::Light,
        )
        .unwrap();
        let html = std::fs::read_to_string(output).unwrap();
        assert!(html.contains("id=\"cf-stage-measurements\">[]</template>"));
        assert!(html.contains("height not measured at this width"));
        store
            .record_stage_measurement(session.id, report())
            .unwrap();
        assert_eq!(
            store.stage_measurements(session.id).unwrap(),
            vec![report()]
        );
        let output = scratch.path().join("fresh.html");
        crate::export::export_session(
            &store,
            session.id,
            &output,
            crate::export::ExportTheme::Graphite,
            crate::export::ExportMode::Light,
        )
        .unwrap();
        let html = std::fs::read_to_string(output).unwrap();
        assert!(html.contains("916"));
        assert!(!html.contains("allow-same-origin"));
        assert!(!html.contains("allow-scripts"));
    }

    #[test]
    fn corrupt_advisory_measurements_degrade_and_are_replaced() {
        let scratch = tempdir().unwrap();
        let store = SessionStore::at_root(scratch.path().join("project"), "key".into()).unwrap();
        let session = store.create(ParsedDocument::Supported(document())).unwrap();
        let path = store
            .session_dir(session.id)
            .join("stage-measurements.json");
        store
            .record_stage_measurement(session.id, report())
            .unwrap();
        // Preserve the private file permissions while corrupting its JSON.
        std::fs::write(&path, b"{invalid json").unwrap();
        let output = scratch.path().join("unmeasured.html");
        crate::export::export_session(
            &store,
            session.id,
            &output,
            crate::export::ExportTheme::Graphite,
            crate::export::ExportMode::Light,
        )
        .unwrap();
        let html = std::fs::read_to_string(output).unwrap();
        assert!(html.contains("id=\"cf-stage-measurements\">[]</template>"));
        assert!(html.contains("height not measured at this width"));
        store
            .record_stage_measurement(session.id, report())
            .unwrap();
        assert_eq!(
            store.stage_measurements(session.id).unwrap(),
            vec![report()]
        );
        // A non-file is also unreadable advisory data, not a broken session.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(store.stage_measurements(session.id).unwrap().is_empty());
    }

    #[test]
    fn bounded_width_range_accepts_scrollbar_and_laptop_geometry() {
        let document = document();
        for width in [320.0, 372.8125, 817.0, 990.0, 1920.0] {
            let mut measurement = report();
            measurement.content_width = width;
            measurement
                .validate(&document, 1, &measurement.contract)
                .unwrap();
        }
        for width in [319.99, 1920.01, f64::NAN, f64::INFINITY] {
            let mut measurement = report();
            measurement.content_width = width;
            assert!(measurement
                .validate(&document, 1, &measurement.contract)
                .is_err());
        }
    }

    #[test]
    fn stage_entry_limit_rejects_growth_but_allows_replacement() {
        let scratch = tempdir().unwrap();
        let store = SessionStore::at_root(scratch.path().join("project"), "key".into()).unwrap();
        let session = store.create(ParsedDocument::Supported(document())).unwrap();
        let records: Vec<_> = (0..42)
            .map(|index| {
                let mut measurement = report();
                measurement.content_width = 400.0 + f64::from(index);
                measurement
            })
            .collect();
        crate::state::write_json_atomic(
            &store
                .session_dir(session.id)
                .join("stage-measurements.json"),
            &records,
        )
        .unwrap();
        assert!(store
            .record_stage_measurement(session.id, report())
            .is_err());
        assert_eq!(store.stage_measurements(session.id).unwrap(), records);
        let mut replacement = records[0].clone();
        replacement.height = 1200.0;
        // A different engine's subpixel grid must replace the same rung,
        // not consume an extra slot in an already full ladder.
        replacement.content_width += 1.0 / 128.0;
        store
            .record_stage_measurement(session.id, replacement.clone())
            .unwrap();
        let saved = store.stage_measurements(session.id).unwrap();
        assert_eq!(saved.len(), 42);
        assert!(saved.contains(&replacement));
        let mut other_stage = report();
        other_stage.stage_id = "short".into();
        store
            .record_stage_measurement(session.id, other_stage)
            .unwrap();
    }

    #[test]
    fn invalid_reports_do_not_replace_valid_state() {
        let scratch = tempdir().unwrap();
        let store = SessionStore::at_root(scratch.path().join("project"), "key".into()).unwrap();
        let session = store.create(ParsedDocument::Supported(document())).unwrap();
        store
            .record_stage_measurement(session.id, report())
            .unwrap();
        let mut invalid = report();
        invalid.revision = 2;
        assert!(store.record_stage_measurement(session.id, invalid).is_err());
        assert_eq!(
            store.stage_measurements(session.id).unwrap(),
            vec![report()]
        );
    }
}
