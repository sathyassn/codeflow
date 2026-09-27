//! Central resource limits mirrored by the public schemas and adversarial tests.

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_BLOCKS: usize = 512;
pub const MAX_NESTING: usize = 12;
pub const MAX_TITLE_BYTES: usize = 512;
pub const MAX_PROSE_BYTES: usize = 512 * 1024;
pub const MAX_CODE_BYTES: usize = 1024 * 1024;
/// The drawn block budget: figure blocks across the whole nested tree.
pub const MAX_FIGURE_BLOCKS: usize = 24;
pub const MAX_FIGURE_DECLARATION_BYTES: usize = 64 * 1024;
pub const MAX_HTML_BYTES: usize = 512 * 1024;
pub const MAX_MEDIA_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_RASTER_DIMENSION: u32 = 16_384;
pub const MAX_RASTER_PIXELS: u64 = 40_000_000;
pub const MAX_LABEL_BYTES: usize = 4 * 1024;
pub const MAX_LANGUAGE_BYTES: usize = 64;
pub const MAX_TABLE_ROWS: usize = 2_000;
pub const MAX_TABLE_COLUMNS: usize = 64;
pub const MAX_COLLECTION_ITEMS_PER_BLOCK: usize = 2_048;
pub const MAX_DOCUMENT_COLLECTION_ITEMS: usize = 16_384;
pub const MAX_FEEDBACK_NOTES: usize = 100;
pub const MAX_FEEDBACK_TEXT_UTF16: usize = 16 * 1024;
pub const MAX_SELECTOR_EXACT_UTF16: usize = 4 * 1024;
pub const MAX_VISUAL_ANCHOR_BYTES: usize = 2 * 1024;
pub const REGION_COORDINATE_SCALE: u32 = 1_000_000;
pub const MAX_VISIBLE_FEEDBACK: usize = 256;
pub const MAX_FEEDBACK_BYTES: usize = 256 * 1024;
pub const MAX_EXCERPT_TEXT_BYTES: usize = 4 * 1024;
pub const MAX_EXCERPT_IMAGE_BYTES: usize = 24 * 1024;
pub const MAX_EXCERPT_IMAGE_B64_BYTES: usize = 32 * 1024;
pub const MAX_EVENTS_PER_RESPONSE: usize = 100;
pub const MAX_EVENT_RESPONSE_BYTES: usize = 1024 * 1024;
pub const MAX_EVENT_RECORD_BYTES: u64 = 256 * 1024;
pub const MAX_EVENT_LOG_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_RUNTIME_CONTROL_BYTES: u64 = 64 * 1024;
pub const MAX_FEEDBACK_EVENTS: usize = 100_000;
pub const MAX_REVISIONS: u64 = 10_000;
pub const MAX_STATE_ENTRIES: usize = 200_000;
pub const MAX_STATE_DEPTH: usize = 24;
pub const EVENT_POLL_SECONDS: u64 = 25;
pub const BOOTSTRAP_TTL_SECONDS: u64 = 120;
pub const SESSION_IDLE_SECONDS: u64 = 4 * 60 * 60;
pub const CLOSED_RETENTION_DAYS: u64 = 30;
pub const MAX_CLOSED_SESSIONS: usize = 100;
pub const MAX_PROJECT_STATE_BYTES: u64 = 500 * 1024 * 1024;
pub const MAX_SESSION_STATE_BYTES: u64 = 64 * 1024;
pub const MAX_REVISION_STATE_BYTES: u64 = (MAX_DOCUMENT_BYTES as u64) + 1024 * 1024;
pub const MAX_HISTORY_READ_BYTES: u64 = 32 * 1024 * 1024;

pub const MAX_RAW_ASSET_BYTES: u64 = 5_000_000;
pub const MAX_RAW_CHUNK_BYTES: u64 = 850_000;
pub const MAX_BROTLI_ASSET_BYTES: u64 = 1_150_000;
pub const MAX_BROTLI_CHUNK_BYTES: u64 = 150_000;
pub const MAX_SERVICE_BINARY_DELTA_BYTES: u64 = 1_250_000;
pub const MAX_EXPORT_PAYLOAD_BYTES: u64 = 1_300_000;
pub const MAX_EXPORT_SHELL_BYTES: u64 = 1_750_000;
pub const MAX_COMBINED_BINARY_DELTA_BYTES: u64 = 2_600_000;

/// The newest presentation document schema this build reads; it reads every
/// version from 1 to this one. `SCHEMA_VERSION` above stays the version of
/// the utility-token configuration.
pub const MAX_DOCUMENT_SCHEMA_VERSION: u32 = 2;
/// Entity labels are collapsed and cut to this many characters (SPC-014 B2).
pub const MAX_ENTITY_LABEL_CHARS: usize = 120;
/// A `data-cf-for` names at most this many entities.
pub const MAX_ENTITY_FOR_IDS: usize = 8;
/// A v2 `html` legend lists 1 to this many entries.
pub const MAX_LEGEND_ENTRIES: usize = 12;
/// A v2 `html` description is at most this many characters.
pub const MAX_DESCRIPTION_CHARS: usize = 2_000;
/// A v2 document summary is 1 to this many characters.
pub const MAX_SUMMARY_CHARS: usize = 200;
/// An entity crop may exceed the entity bounds by this many user units on
/// every side (SPC-014 B4).
pub const ENTITY_CROP_TOLERANCE: f64 = 8.0;
/// The fuzzy quote step runs for quotes up to this many UTF-16 units ...
pub const MAX_FUZZY_QUOTE_UTF16: usize = 512;
/// ... inside block texts up to this many (SPC-014 B1).
pub const MAX_FUZZY_TEXT_UTF16: usize = 65_536;
/// The score a fuzzy candidate needs to re-anchor a note (SPC-014 B1).
pub const FUZZY_THRESHOLD: f64 = 0.75;
