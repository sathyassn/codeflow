//! Central resource limits mirrored by the public schemas and adversarial tests.

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_DOCUMENT_BYTES: usize = codeflow_limits::PRESENT_MAX_DOCUMENT_BYTES.default_count();
pub const MAX_BLOCKS: usize = codeflow_limits::PRESENT_MAX_BLOCKS.default_count();
pub const MAX_NESTING: usize = codeflow_limits::PRESENT_MAX_NESTING.default_count();
pub const MAX_TITLE_BYTES: usize = codeflow_limits::PRESENT_MAX_TITLE_BYTES.default_count();
pub const MAX_PROSE_BYTES: usize = codeflow_limits::PRESENT_MAX_PROSE_BYTES.default_count();
pub const MAX_CODE_BYTES: usize = codeflow_limits::PRESENT_MAX_CODE_BYTES.default_count();
/// The drawn block budget: figure blocks across the whole nested tree.
pub const MAX_FIGURE_BLOCKS: usize = codeflow_limits::PRESENT_MAX_FIGURE_BLOCKS.default_count();
pub const MAX_FIGURE_DECLARATION_BYTES: usize =
    codeflow_limits::PRESENT_MAX_FIGURE_DECLARATION_BYTES.default_count();
/// The removed diagram block's bounds, kept to judge a stored pre-release
/// revision: one outside them was never admitted (TSK-114).
pub const MAX_DIAGRAM_BYTES: usize = codeflow_limits::PRESENT_MAX_DIAGRAM_BYTES.default_count();
pub const MAX_DIAGRAM_BLOCKS: usize = codeflow_limits::PRESENT_MAX_DIAGRAM_BLOCKS.default_count();
pub const MAX_HTML_BYTES: usize = codeflow_limits::PRESENT_MAX_HTML_BYTES.default_count();
pub const MAX_MEDIA_BYTES: usize = codeflow_limits::PRESENT_MAX_MEDIA_BYTES.default_count();
pub const MAX_RASTER_DIMENSION: u32 = codeflow_limits::PRESENT_MAX_RASTER_DIMENSION.default_small();
pub const MAX_RASTER_PIXELS: u64 = codeflow_limits::PRESENT_MAX_RASTER_PIXELS.default;
pub const MAX_LABEL_BYTES: usize = codeflow_limits::PRESENT_MAX_LABEL_BYTES.default_count();
pub const MAX_LANGUAGE_BYTES: usize = codeflow_limits::PRESENT_MAX_LANGUAGE_BYTES.default_count();
pub const MAX_TABLE_ROWS: usize = codeflow_limits::PRESENT_MAX_TABLE_ROWS.default_count();
pub const MAX_TABLE_COLUMNS: usize = codeflow_limits::PRESENT_MAX_TABLE_COLUMNS.default_count();
pub const MAX_COLLECTION_ITEMS_PER_BLOCK: usize =
    codeflow_limits::PRESENT_MAX_COLLECTION_ITEMS_PER_BLOCK.default_count();
pub const MAX_DOCUMENT_COLLECTION_ITEMS: usize =
    codeflow_limits::PRESENT_MAX_DOCUMENT_COLLECTION_ITEMS.default_count();
/// Agent reply text is bounded in UTF-8 bytes, independently of review-note UTF-16 limits.
pub const MAX_REPLY_TEXT_BYTES: usize =
    codeflow_limits::PRESENT_MAX_REPLY_TEXT_BYTES.default_count();
pub const MAX_FEEDBACK_NOTES: usize = codeflow_limits::PRESENT_MAX_FEEDBACK_NOTES.default_count();
pub const MAX_FEEDBACK_TEXT_UTF16: usize =
    codeflow_limits::PRESENT_MAX_FEEDBACK_TEXT_UTF16.default_count();
pub const MAX_SELECTOR_EXACT_UTF16: usize =
    codeflow_limits::PRESENT_MAX_SELECTOR_EXACT_UTF16.default_count();
pub const MAX_VISUAL_ANCHOR_BYTES: usize =
    codeflow_limits::PRESENT_MAX_VISUAL_ANCHOR_BYTES.default_count();
pub const REGION_COORDINATE_SCALE: u32 =
    codeflow_limits::PRESENT_REGION_COORDINATE_SCALE.default_small();
pub const MAX_VISIBLE_FEEDBACK: usize =
    codeflow_limits::PRESENT_MAX_VISIBLE_FEEDBACK.default_count();
pub const MAX_FEEDBACK_BYTES: usize = codeflow_limits::PRESENT_MAX_FEEDBACK_BYTES.default_count();
pub const MAX_EXCERPT_TEXT_BYTES: usize =
    codeflow_limits::PRESENT_MAX_EXCERPT_TEXT_BYTES.default_count();
pub const MAX_EXCERPT_IMAGE_BYTES: usize =
    codeflow_limits::PRESENT_MAX_EXCERPT_IMAGE_BYTES.default_count();
pub const MAX_EXCERPT_IMAGE_B64_BYTES: usize =
    codeflow_limits::PRESENT_MAX_EXCERPT_IMAGE_B64_BYTES.default_count();
/// The largest crop the page captures (`web/src/excerpt.ts` `MAX_CROP_*`);
/// a PNG crop over either bound is refused before anything decodes it.
pub const MAX_CROP_WIDTH: u32 = codeflow_limits::PRESENT_MAX_CROP_WIDTH.default_small();
pub const MAX_CROP_HEIGHT: u32 = codeflow_limits::PRESENT_MAX_CROP_HEIGHT.default_small();
pub const MAX_EVENTS_PER_RESPONSE: usize =
    codeflow_limits::PRESENT_MAX_EVENTS_PER_RESPONSE.default_count();
pub const MAX_EVENT_RESPONSE_BYTES: usize =
    codeflow_limits::PRESENT_MAX_EVENT_RESPONSE_BYTES.default_count();
pub const MAX_EVENT_RECORD_BYTES: u64 = codeflow_limits::PRESENT_MAX_EVENT_RECORD_BYTES.default;
pub const MAX_EVENT_LOG_BYTES: u64 = codeflow_limits::PRESENT_MAX_EVENT_LOG_BYTES.default;
pub const MAX_RUNTIME_CONTROL_BYTES: u64 =
    codeflow_limits::PRESENT_MAX_RUNTIME_CONTROL_BYTES.default;
pub const MAX_FEEDBACK_EVENTS: usize = codeflow_limits::PRESENT_MAX_FEEDBACK_EVENTS.default_count();
pub const MAX_REVISIONS: u64 = codeflow_limits::PRESENT_MAX_REVISIONS.default;
pub const MAX_STATE_ENTRIES: usize = codeflow_limits::PRESENT_MAX_STATE_ENTRIES.default_count();
pub const MAX_STATE_DEPTH: usize = codeflow_limits::PRESENT_MAX_STATE_DEPTH.default_count();
pub const EVENT_POLL_SECONDS: u64 = codeflow_limits::PRESENT_EVENT_POLL_SECONDS.default;
pub const BOOTSTRAP_TTL_SECONDS: u64 = codeflow_limits::PRESENT_BOOTSTRAP_TTL_SECONDS.default;
pub const SESSION_IDLE_SECONDS: u64 = codeflow_limits::PRESENT_SESSION_IDLE_SECONDS.default;
/// How long the isolated browser stays running once its last review window
/// has closed: a window reopened within it, by `present show` or a reload,
/// keeps the browser; after it the browser quits and the session stays open.
pub const WINDOW_CLOSE_GRACE_SECONDS: u64 =
    codeflow_limits::PRESENT_WINDOW_CLOSE_GRACE_SECONDS.default;
pub const CLOSED_RETENTION_DAYS: u64 = codeflow_limits::PRESENT_CLOSED_RETENTION_DAYS.default;
pub const MAX_CLOSED_SESSIONS: usize = codeflow_limits::PRESENT_MAX_CLOSED_SESSIONS.default_count();
pub const MAX_PROJECT_STATE_BYTES: u64 = codeflow_limits::PRESENT_MAX_PROJECT_STATE_BYTES.default;
pub const MAX_SESSION_STATE_BYTES: u64 = codeflow_limits::PRESENT_MAX_SESSION_STATE_BYTES.default;
pub const MAX_REVISION_STATE_BYTES: u64 = (MAX_DOCUMENT_BYTES as u64) + 1024 * 1024;
pub const MAX_HISTORY_READ_BYTES: u64 = codeflow_limits::PRESENT_MAX_HISTORY_READ_BYTES.default;

pub const MAX_RAW_ASSET_BYTES: u64 = codeflow_limits::PRESENT_MAX_RAW_ASSET_BYTES.default;
pub const MAX_RAW_CHUNK_BYTES: u64 = codeflow_limits::PRESENT_MAX_RAW_CHUNK_BYTES.default;
pub const MAX_BROTLI_ASSET_BYTES: u64 = codeflow_limits::PRESENT_MAX_BROTLI_ASSET_BYTES.default;
pub const MAX_GZIP_ASSET_BYTES: u64 = codeflow_limits::PRESENT_MAX_GZIP_ASSET_BYTES.default;
pub const MAX_GZIP_CHUNK_BYTES: u64 = codeflow_limits::PRESENT_MAX_GZIP_CHUNK_BYTES.default;
pub const MAX_BROTLI_CHUNK_BYTES: u64 = codeflow_limits::PRESENT_MAX_BROTLI_CHUNK_BYTES.default;
pub const MAX_SERVICE_BINARY_DELTA_BYTES: u64 =
    codeflow_limits::PRESENT_MAX_SERVICE_BINARY_DELTA_BYTES.default;
pub const MAX_EXPORT_PAYLOAD_BYTES: u64 = codeflow_limits::PRESENT_MAX_EXPORT_PAYLOAD_BYTES.default;
pub const MAX_EXPORT_SHELL_BYTES: u64 = codeflow_limits::PRESENT_MAX_EXPORT_SHELL_BYTES.default;
pub const MAX_COMBINED_BINARY_DELTA_BYTES: u64 =
    codeflow_limits::PRESENT_MAX_COMBINED_BINARY_DELTA_BYTES.default;

/// The newest presentation document schema this build reads; it reads every
/// version from 1 to this one. `SCHEMA_VERSION` above stays the version of
/// the utility-token configuration.
pub const MAX_DOCUMENT_SCHEMA_VERSION: u32 = 2;
/// Entity labels are collapsed and cut to this many characters (SPC-014 B2).
pub const MAX_ENTITY_LABEL_CHARS: usize =
    codeflow_limits::PRESENT_MAX_ENTITY_LABEL_CHARS.default_count();
/// A `data-cf-for` names at most this many entities.
pub const MAX_ENTITY_FOR_IDS: usize = codeflow_limits::PRESENT_MAX_ENTITY_FOR_IDS.default_count();
/// A v2 `html` legend lists 1 to this many entries.
pub const MAX_LEGEND_ENTRIES: usize = codeflow_limits::PRESENT_MAX_LEGEND_ENTRIES.default_count();
/// A v2 `html` description is at most this many characters.
pub const MAX_DESCRIPTION_CHARS: usize =
    codeflow_limits::PRESENT_MAX_DESCRIPTION_CHARS.default_count();
/// A v2 document summary is 1 to this many characters.
pub const MAX_SUMMARY_CHARS: usize = codeflow_limits::PRESENT_MAX_SUMMARY_CHARS.default_count();
/// An entity crop may exceed the entity bounds by this many user units on
/// every side (SPC-014 B4).
pub const ENTITY_CROP_TOLERANCE: f64 = 8.0;
/// The fuzzy quote step runs for quotes up to this many UTF-16 units ...
pub const MAX_FUZZY_QUOTE_UTF16: usize =
    codeflow_limits::PRESENT_MAX_FUZZY_QUOTE_UTF16.default_count();
/// ... inside block texts up to this many (SPC-014 B1).
pub const MAX_FUZZY_TEXT_UTF16: usize =
    codeflow_limits::PRESENT_MAX_FUZZY_TEXT_UTF16.default_count();
/// The score a fuzzy candidate needs to re-anchor a note (SPC-014 B1).
pub const FUZZY_THRESHOLD: f64 = 0.75;
/// The edit-distance cells one fuzzy quote search may compute. A search that
/// needs more stops before scoring and the note shows on its block with a
/// notice (SPC-014 B1, TSK-118 review round 1).
pub const FUZZY_WORK_BUDGET: u64 = codeflow_limits::PRESENT_FUZZY_WORK_BUDGET.default;

/// At most this many `form` and v2 `decision` blocks in one document
/// (SPC-014 I1).
pub const MAX_FORMS_PER_DOCUMENT: usize =
    codeflow_limits::PRESENT_MAX_FORMS_PER_DOCUMENT.default_count();
/// At most this many fields in one form (I1).
pub const MAX_FORM_FIELDS: usize = codeflow_limits::PRESENT_MAX_FORM_FIELDS.default_count();
/// A choice field lists 2 to this many options (B6, I1).
pub const MAX_FIELD_OPTIONS: usize = codeflow_limits::PRESENT_MAX_FIELD_OPTIONS.default_count();
/// A v2 decision lists 2 to this many options (B6).
pub const MAX_DECISION_OPTIONS: usize =
    codeflow_limits::PRESENT_MAX_DECISION_OPTIONS.default_count();
/// A field or option label is 1 to this many characters (B6).
pub const MAX_FORM_LABEL_CHARS: usize =
    codeflow_limits::PRESENT_MAX_FORM_LABEL_CHARS.default_count();
/// The largest `max_length` a text field may declare, in UTF-16 units (B6).
pub const MAX_FORM_TEXT_UTF16: u32 = codeflow_limits::PRESENT_MAX_FORM_TEXT_UTF16.default_small();
/// A decline reason is at most this many bytes (B6).
pub const MAX_DECLINE_REASON_BYTES: usize =
    codeflow_limits::PRESENT_MAX_DECLINE_REASON_BYTES.default_count();
/// An answer request body is at most this many bytes; the bound is checked
/// before anything else in the body (I3 `answer_too_large`).
pub const MAX_ANSWER_REQUEST_BYTES: usize =
    codeflow_limits::PRESENT_MAX_ANSWER_REQUEST_BYTES.default_count();
/// The most JSON bytes one character of question text takes in a record:
/// a `\uXXXX` escape (6) is longer than any UTF-8 character (4).
pub const MAX_JSON_CHAR_BYTES: usize = codeflow_limits::PRESENT_MAX_JSON_CHAR_BYTES.default_count();
/// The most bytes a number grows when the record writes it again: at most
/// 24 bytes out (`-2.2250738585072014e-308`) for at least 1 byte in.
pub const MAX_JSON_NUMBER_GROWTH: usize =
    codeflow_limits::PRESENT_MAX_JSON_NUMBER_GROWTH.default_count();
/// The question text a record can repeat, at its worst encoding: a title of
/// `MAX_TITLE_BYTES` (each byte one character at worst) and 32 fields, each
/// a 64-byte id, a label and 24 options of a value and a label. A v2
/// decision is one field whose label is its title with 8 options, so it
/// never repeats more (checked below).
pub const MAX_SNAPSHOT_TEXT_BYTES: usize = MAX_TITLE_BYTES * MAX_JSON_CHAR_BYTES
    + MAX_FORM_FIELDS
        * (64
            + MAX_FORM_LABEL_CHARS * MAX_JSON_CHAR_BYTES
            + MAX_FIELD_OPTIONS * 2 * MAX_FORM_LABEL_CHARS * MAX_JSON_CHAR_BYTES);
/// What a record repeats from its request: values, rationales and reason
/// are at most the request's own bytes (`serde_json` never writes a string
/// or key longer than the request spelled it). A number can grow when it is
/// written again, but a number field has no options, so a form with one
/// repeats far less question text than the largest form (checked below).
pub const MAX_RECORD_REQUEST_BYTES: usize = MAX_ANSWER_REQUEST_BYTES;
/// The rest of a record at its largest: its JSON structure for 32 fields of
/// 24 options, the member names, the longest event and outcome names,
/// numbers at `u64::MAX`, the UUIDs and digests. Pinned by the test that
/// writes that record (`the_answer_record_cap_is_the_largest_record`).
pub const MAX_RECORD_FRAME_BYTES: usize =
    codeflow_limits::PRESENT_MAX_RECORD_FRAME_BYTES.default_count();
/// One `responses.jsonl` line, newline excluded: the largest record the
/// bounds above allow, so every form the service accepts can be answered,
/// declined and dismissed. The 64 MiB ledger holds at least 34 of them.
pub const MAX_RESPONSE_RECORD_BYTES: u64 =
    (MAX_SNAPSHOT_TEXT_BYTES + MAX_RECORD_REQUEST_BYTES + MAX_RECORD_FRAME_BYTES) as u64;
const _: () = {
    // A decision repeats less than the largest form: its title twice (as
    // the title and as its one field's label) and 8 options.
    assert!(
        2 * MAX_TITLE_BYTES * MAX_JSON_CHAR_BYTES
            + MAX_DECISION_OPTIONS * 2 * MAX_FORM_LABEL_CHARS * MAX_JSON_CHAR_BYTES
            < MAX_FORM_FIELDS * MAX_FIELD_OPTIONS * 2 * MAX_FORM_LABEL_CHARS * MAX_JSON_CHAR_BYTES
    );
    // Every number growing at once is less than one field's options.
    assert!(
        MAX_FORM_FIELDS * MAX_JSON_NUMBER_GROWTH
            < MAX_FIELD_OPTIONS * 2 * MAX_FORM_LABEL_CHARS * MAX_JSON_CHAR_BYTES
    );
    // A decline reason is part of the request, within its bound.
    assert!(MAX_DECLINE_REASON_BYTES < MAX_ANSWER_REQUEST_BYTES);
};
/// The whole `responses.jsonl` ledger.
pub const MAX_RESPONSE_LOG_BYTES: u64 = codeflow_limits::PRESENT_MAX_RESPONSE_LOG_BYTES.default;
/// Lines in `responses.jsonl`.
pub const MAX_RESPONSE_EVENTS: usize = codeflow_limits::PRESENT_MAX_RESPONSE_EVENTS.default_count();

/// How long `present close` waits for the session service to exit.
pub const SERVICE_EXIT_WAIT_SECONDS: u64 =
    codeflow_limits::PRESENT_SERVICE_EXIT_WAIT_SECONDS.default;
