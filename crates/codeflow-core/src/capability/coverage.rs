//! Inventory and declared evidence are independent catalog facts.

use super::CapabilityCatalog;

/// Describe inventory without treating declared evidence as resolved proof.
#[must_use]
fn counts(catalog: &CapabilityCatalog) -> (&'static str, usize, usize, usize, usize) {
    let total = catalog.records.len();
    let inventory = if total > 0
        && catalog
            .vocabulary
            .as_ref()
            .is_some_and(|v| v.inventory == "complete")
    {
        "complete"
    } else {
        "incomplete"
    };
    let shipped = catalog
        .records
        .iter()
        .filter(|r| r.entry.status == "shipped")
        .count();
    let unassessed = catalog
        .records
        .iter()
        .filter(|r| r.stage.as_deref().is_none_or(|stage| stage == "unassessed"))
        .count();
    let declared = catalog
        .records
        .iter()
        .filter(|r| {
            r.entry.status == "shipped"
                && r.stage
                    .as_deref()
                    .is_some_and(|stage| stage != "unassessed")
                && !r.entry.verified_by.is_empty()
                && r.entry
                    .verified_by
                    .iter()
                    .all(|value| super::parse_evidence(value).is_ok_and(|e| e.is_proof()))
        })
        .count();
    (inventory, total, declared, shipped, unassessed)
}

/// Describe inventory without treating declared evidence as resolved proof.
#[must_use]
pub fn line(catalog: &CapabilityCatalog) -> String {
    let (inventory, total, declared, shipped, unassessed) = counts(catalog);
    format!("inventory: {inventory} ({total} capabilities); evidence declarations: {declared}/{shipped} shipped assessed without gaps; {unassessed} unassessed")
}

/// A scaffold report contains only fixed labels and counts, never file values.
#[must_use]
pub fn report(root: &std::path::Path) -> crate::scaffold::shown::Shown {
    use crate::scaffold::shown::Shown;
    let Ok(catalog) = super::load_capability_catalog(root) else {
        return Shown::fixed("coverage unavailable; run codeflow validate --docs");
    };
    let (inventory, total, declared, shipped, unassessed) = counts(&catalog);
    Shown::format("inventory: {} ({} capabilities); evidence declarations: {}/{} shipped assessed without gaps; {} unassessed", &[
        &Shown::fixed(inventory), &Shown::count(total), &Shown::count(declared),
        &Shown::count(shipped), &Shown::count(unassessed),
    ])
}
