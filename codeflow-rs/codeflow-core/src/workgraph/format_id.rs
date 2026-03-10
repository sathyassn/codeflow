//! Format ID generation for epics and tasks.
//!
//! Format IDs are human-readable identifiers following conventions:
//! - Epics: `{AREA}-EPC-{NNN}` (e.g., `INF-EPC-022`)
//! - Tasks: `{AREA}-TSK-{EPIC_SEQ}-{TASK_SEQ}` (e.g., `INF-TSK-022-013`)
//!
//! Sequence numbers are determined by querying the `DataStore` for existing
//! records with the same area type, then incrementing.

use crate::error::WorkgraphError;
use crate::models::EpicFilter;
use crate::models::TaskFilter;
use crate::store::DataStore;
use crate::types::{AreaType, FormatId};

/// Generate the next epic format ID for the given area type.
///
/// Queries the store for existing epics with the same area type to determine
/// the next sequence number. Returns `FormatId` newtype.
///
/// # Errors
///
/// Returns `WorkgraphError::Db` on store failure, or
/// `WorkgraphError::FormatIdGeneration` if the generated ID is invalid.
pub async fn generate_epic_format_id(
    store: &impl DataStore,
    area: AreaType,
) -> Result<FormatId, WorkgraphError> {
    let filter = EpicFilter {
        area_type: Some(area),
        ..Default::default()
    };
    let existing = store.list_epics(filter).await?;

    let max_seq = existing
        .iter()
        .filter_map(|e| parse_epic_sequence(&e.format_id, &area.to_string()))
        .max()
        .unwrap_or(0);

    let next_seq = max_seq + 1;
    let format_id_str = format!("{area}-EPC-{next_seq:03}");

    FormatId::new(&format_id_str)
        .map_err(|e| WorkgraphError::FormatIdGeneration(format!("invalid format ID: {e}")))
}

/// Generate the next task format ID within an epic.
///
/// The format is `{AREA}-TSK-{EPIC_SEQ}-{TASK_SEQ}`. The epic sequence
/// is extracted from the parent epic's format ID.
///
/// # Errors
///
/// Returns `WorkgraphError::FormatIdGeneration` if the epic format ID
/// cannot be parsed, or `WorkgraphError::Db` on store failure.
pub async fn generate_task_format_id(
    store: &impl DataStore,
    area: AreaType,
    epic_format_id: &FormatId,
) -> Result<FormatId, WorkgraphError> {
    let area_str = area.to_string();
    let epic_seq = parse_epic_sequence(epic_format_id.as_str(), &area_str).ok_or_else(|| {
        WorkgraphError::FormatIdGeneration(format!(
            "cannot parse epic sequence from '{epic_format_id}'"
        ))
    })?;

    // Query all tasks for this area to find max task sequence within this epic
    let filter = TaskFilter {
        area_type: Some(area),
        ..Default::default()
    };
    let existing = store.list_tasks(filter).await?;

    let max_task_seq = existing
        .iter()
        .filter_map(|t| parse_task_sequence(&t.format_id, &area_str, epic_seq))
        .max()
        .unwrap_or(0);

    let next_seq = max_task_seq + 1;
    let format_id_str = format!("{area}-TSK-{epic_seq:03}-{next_seq:03}");

    FormatId::new(&format_id_str)
        .map_err(|e| WorkgraphError::FormatIdGeneration(format!("invalid format ID: {e}")))
}

/// Parse the epic sequence number from a format ID like `INF-EPC-022`.
fn parse_epic_sequence(format_id: &str, area: &str) -> Option<u32> {
    let prefix = format!("{area}-EPC-");
    let suffix = format_id.strip_prefix(&prefix)?;
    suffix.parse().ok()
}

/// Parse the task sequence number from a format ID like `INF-TSK-022-013`.
fn parse_task_sequence(format_id: &str, area: &str, epic_seq: u32) -> Option<u32> {
    let prefix = format!("{area}-TSK-{epic_seq:03}-");
    let suffix = format_id.strip_prefix(&prefix)?;
    suffix.parse().ok()
}

#[cfg(test)]
mod tests {
    use crate::types::AreaType;

    use super::super::test_support::{default_epic_input, default_task_input, new_service};
    use super::{parse_epic_sequence, parse_task_sequence};

    #[tokio::test]
    async fn test_epic_format_id_generation() {
        let (service, _) = new_service();

        let epic1 = service.create_epic(default_epic_input()).await.unwrap();
        assert_eq!(epic1.format_id.as_str(), "INF-EPC-001");

        let epic2 = service.create_epic(default_epic_input()).await.unwrap();
        assert_eq!(epic2.format_id.as_str(), "INF-EPC-002");

        let epic3 = service.create_epic(default_epic_input()).await.unwrap();
        assert_eq!(epic3.format_id.as_str(), "INF-EPC-003");
    }

    #[tokio::test]
    async fn test_task_format_id_generation() {
        let (service, _) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();

        let task1 = service
            .create_task(default_task_input(epic.id.clone()))
            .await
            .unwrap();
        assert_eq!(task1.format_id.as_str(), "INF-TSK-001-001");

        let task2 = service
            .create_task(default_task_input(epic.id.clone()))
            .await
            .unwrap();
        assert_eq!(task2.format_id.as_str(), "INF-TSK-001-002");
    }

    #[tokio::test]
    async fn test_format_id_different_areas() {
        let (service, _) = new_service();

        let mut doc_input = default_epic_input();
        doc_input.area_type = AreaType::Doc;
        let doc_epic = service.create_epic(doc_input).await.unwrap();
        assert_eq!(doc_epic.format_id.as_str(), "DOC-EPC-001");

        let inf_epic = service.create_epic(default_epic_input()).await.unwrap();
        assert_eq!(inf_epic.format_id.as_str(), "INF-EPC-001");
    }

    // -- Direct tests for private parse functions --

    #[test]
    fn test_parse_epic_sequence_valid() {
        assert_eq!(parse_epic_sequence("INF-EPC-001", "INF"), Some(1));
        assert_eq!(parse_epic_sequence("INF-EPC-022", "INF"), Some(22));
        assert_eq!(parse_epic_sequence("DOC-EPC-100", "DOC"), Some(100));
    }

    #[test]
    fn test_parse_epic_sequence_wrong_area() {
        assert_eq!(parse_epic_sequence("INF-EPC-001", "DOC"), None);
    }

    #[test]
    fn test_parse_epic_sequence_malformed() {
        assert_eq!(parse_epic_sequence("INF-EPC-", "INF"), None);
        assert_eq!(parse_epic_sequence("INF-EPC-abc", "INF"), None);
        assert_eq!(parse_epic_sequence("not-a-format-id", "INF"), None);
        assert_eq!(parse_epic_sequence("", "INF"), None);
    }

    #[test]
    fn test_parse_task_sequence_valid() {
        assert_eq!(parse_task_sequence("INF-TSK-001-001", "INF", 1), Some(1));
        assert_eq!(parse_task_sequence("INF-TSK-022-013", "INF", 22), Some(13));
        assert_eq!(parse_task_sequence("DOC-TSK-005-042", "DOC", 5), Some(42));
    }

    #[test]
    fn test_parse_task_sequence_wrong_area() {
        assert_eq!(parse_task_sequence("INF-TSK-001-001", "DOC", 1), None);
    }

    #[test]
    fn test_parse_task_sequence_wrong_epic_seq() {
        assert_eq!(parse_task_sequence("INF-TSK-001-001", "INF", 2), None);
    }

    #[test]
    fn test_parse_task_sequence_malformed() {
        assert_eq!(parse_task_sequence("INF-TSK-001-", "INF", 1), None);
        assert_eq!(parse_task_sequence("INF-TSK-001-abc", "INF", 1), None);
        assert_eq!(parse_task_sequence("not-a-format-id", "INF", 1), None);
        assert_eq!(parse_task_sequence("", "INF", 1), None);
    }

    #[tokio::test]
    async fn test_task_format_id_across_epics() {
        let (service, _) = new_service();

        let epic1 = service.create_epic(default_epic_input()).await.unwrap();
        let epic2 = service.create_epic(default_epic_input()).await.unwrap();

        let task1_e1 = service
            .create_task(default_task_input(epic1.id.clone()))
            .await
            .unwrap();
        assert_eq!(task1_e1.format_id.as_str(), "INF-TSK-001-001");

        let task1_e2 = service
            .create_task(default_task_input(epic2.id.clone()))
            .await
            .unwrap();
        assert_eq!(task1_e2.format_id.as_str(), "INF-TSK-002-001");

        let task2_e1 = service
            .create_task(default_task_input(epic1.id.clone()))
            .await
            .unwrap();
        assert_eq!(task2_e1.format_id.as_str(), "INF-TSK-001-002");
    }
}
