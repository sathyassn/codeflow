//! Create a planned capability with an exclusively written file and reserved id.

use std::path::Path;

use serde::Serialize;

use super::{
    catalog_error, parse_record, validate_record, AreaConfig, CapabilityCatalogError,
    CAPABILITY_MAX_FILE_BYTES, PROJECT_PATH,
};
use crate::ids::{issue, Kind};

/// The facets a caller must decide when creating a capability.
#[derive(Debug)]
pub struct NewCapability {
    pub name: String,
    pub summary: String,
    pub area: String,
    pub audience: Vec<String>,
    pub kind: String,
    pub quality: Option<String>,
    pub measure: Option<String>,
}

#[derive(Serialize)]
struct Record<'a> {
    id: &'a str,
    uid: &'a str,
    name: &'a str,
    summary: &'a str,
    status: &'static str,
    kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    quality: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    measure: &'a Option<String>,
    audience: &'a [String],
    publish: bool,
    area: &'a str,
}

fn render(options: &NewCapability, id: &str, uid: &str) -> Result<String, CapabilityCatalogError> {
    let yaml = serde_yaml::to_string(&Record {
        id,
        uid,
        name: &options.name,
        summary: &options.summary,
        status: "planned",
        kind: &options.kind,
        quality: &options.quality,
        measure: &options.measure,
        audience: &options.audience,
        publish: false,
        area: &options.area,
    })
    .map_err(|error| catalog_error("<new capability>", error.to_string()))?;
    Ok(format!(
        "---\n{yaml}---\n\n# {id}: {}\n\n{}\n",
        options.name, options.summary
    ))
}

/// Validate all supplied facets before reserving an id, then write a new record.
/// Tracking-off issuance stays local, but retains its binding for later sync.
///
/// # Errors
/// Refuses invalid input, invalid existing catalogs, registry failures and unsafe
/// or occupied output paths. A failed write leaves its reserved number occupied.
pub fn create(
    root: &Path,
    options: &NewCapability,
    tracking: bool,
) -> Result<issue::Reservation, CapabilityCatalogError> {
    let tree = crate::contained::Tree::open(root)
        .map_err(|error| catalog_error("<checkout>", error.to_string()))?;
    let catalog = super::load_capability_catalog(root)?;
    let bytes = tree
        .read(PROJECT_PATH, CAPABILITY_MAX_FILE_BYTES as u64)
        .map_err(|error| catalog_error(PROJECT_PATH, error.to_string()))?;
    let config: AreaConfig = toml::from_str(
        std::str::from_utf8(&bytes)
            .map_err(|error| catalog_error(PROJECT_PATH, error.to_string()))?,
    )
    .map_err(|error| catalog_error(PROJECT_PATH, error.to_string()))?;
    let placeholder = format!("docs/capabilities/CAP-000-{}.md", options.name);
    super::validate_capability_path(&placeholder)?;
    let text = render(options, "CAP-000", &crate::ids::new_uid())?;
    let record = parse_record(&placeholder, &text)?;
    validate_record(&record, &config.areas, catalog.vocabulary.as_ref())?;
    let request = issue::Request::issue(Kind::Cap, &options.summary, "none");
    let reservation = if tracking {
        issue::reserve(root, &request)
    } else {
        issue::reserve_local(root, &request)
    }
    .map_err(|error| catalog_error("<registry>", error.to_string()))?;
    let id = reservation.id.to_string();
    let file = format!("docs/capabilities/{id}-{}.md", options.name);
    let text = render(options, &id, &reservation.uid)?;
    tree.create_new(&file, text.as_bytes())
        .map_err(|error| catalog_error(&file, format!("{id} reserved but not written: {error}")))?;
    issue::written(root, &reservation.id).map_err(|error| {
        catalog_error(
            &file,
            format!("record written; reservation cleanup failed: {error}"),
        )
    })?;
    Ok(reservation)
}
