//! The schema v2 `form` block, the v2 `decision` as a form, and the rules an
//! answer must satisfy (SPC-014 B6, I1 and I3).
//!
//! The field types here are embedded in `document.rs`'s block enum, so a
//! form's digest is the block digest of `state.rs` (`block_digest`): the
//! SHA-256 of the block as the enum serializes it. The service renders it
//! on the form as `data-cf-form-digest` and checks the page's echo of it.
//! The answer ledger in `responses.rs` validates each request through a
//! [`FormView`] of the form at the revision the request names.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    document::Block,
    error::{PresentError, Result},
    limits,
};

/// The id of the one field a v2 decision renders as (B6).
pub const DECISION_FIELD_ID: &str = "choice";

/// The largest integer a JSON number carries exactly: 2^53 - 1.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// Whether and how a field takes a rationale (B6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RationaleMode {
    #[default]
    None,
    Optional,
    Required,
}

/// The field kinds of B6, a subset of the MCP elicitation form schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Number,
    Integer,
    Boolean,
    Choice,
    Choices,
}

/// The text formats of B6. `multiline` is a display hint and checks nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextFormat {
    Email,
    Uri,
    Date,
    DateTime,
    Multiline,
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
const fn is_false(value: &bool) -> bool {
    !*value
}

/// One option of a `choice` or `choices` field or of a v2 decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub value: String,
    pub label: String,
    /// Labelled "Recommended" on the page and never preselected.
    #[serde(default, skip_serializing_if = "is_false")]
    pub recommended: bool,
}

/// One field of a form, in the order shown. A constraint that does not
/// apply to the field's kind is refused. No field has a default value:
/// `default` is an unknown member and is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormField {
    pub id: String,
    pub label: String,
    pub kind: FieldKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<RationaleMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<TextFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<Choice>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
}

impl FormField {
    /// The rationale mode, `none` when the field does not name one.
    #[must_use]
    pub fn rationale_mode(&self) -> RationaleMode {
        self.rationale.unwrap_or_default()
    }

    /// The largest text length the field accepts, in UTF-16 units.
    #[must_use]
    pub fn text_max(&self) -> u32 {
        self.max_length.unwrap_or(limits::MAX_FORM_TEXT_UTF16)
    }
}

/// The B6 and I1 rules for a `form` block's own members. Document-level
/// rules (unique block ids, the form count) belong to the document.
pub fn validate_form(
    id: &str,
    title: &str,
    fields: &[FormField],
    required: &[String],
) -> Result<()> {
    let context = format!("form block {id}");
    validate_title(&context, title)?;
    if fields.is_empty() || fields.len() > limits::MAX_FORM_FIELDS {
        return Err(invalid(format!(
            "{context}: a form has 1 to {} fields",
            limits::MAX_FORM_FIELDS
        )));
    }
    let mut ids = HashSet::new();
    for field in fields {
        validate_field(&context, field)?;
        if !ids.insert(field.id.as_str()) {
            return Err(invalid(format!(
                "{context}: field id {:?} is used twice",
                field.id
            )));
        }
    }
    let mut named = HashSet::new();
    for field_id in required {
        if !ids.contains(field_id.as_str()) {
            return Err(invalid(format!(
                "{context}: required names {field_id:?}, which is not a field of this form"
            )));
        }
        if !named.insert(field_id.as_str()) {
            return Err(invalid(format!(
                "{context}: required names {field_id:?} twice"
            )));
        }
    }
    Ok(())
}

/// The B6 rules for a v2 decision's options: 2 to 8, at most one
/// recommended.
pub fn validate_decision_options(id: &str, options: &[Choice]) -> Result<()> {
    validate_options(
        &format!("decision block {id}"),
        options,
        limits::MAX_DECISION_OPTIONS,
    )
}

fn validate_title(context: &str, title: &str) -> Result<()> {
    if title.trim().is_empty() {
        return Err(invalid(format!("{context}: the title must not be empty")));
    }
    if title.len() > limits::MAX_TITLE_BYTES {
        return Err(invalid(format!(
            "{context}: the title exceeds {} bytes",
            limits::MAX_TITLE_BYTES
        )));
    }
    Ok(())
}

fn validate_label(context: &str, what: &str, text: &str) -> Result<()> {
    if text.trim().is_empty() || text.chars().count() > limits::MAX_FORM_LABEL_CHARS {
        return Err(invalid(format!(
            "{context}: {what} must be 1 to {} characters",
            limits::MAX_FORM_LABEL_CHARS
        )));
    }
    if text.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{context}: {what} must not contain control characters"
        )));
    }
    Ok(())
}

fn validate_field(form_context: &str, field: &FormField) -> Result<()> {
    let context = format!("{form_context} field {:?}", field.id);
    if !crate::entity::is_entity_id(&field.id) {
        return Err(invalid(format!(
            "{context}: the id must be lower-case kebab-case, at most 64 characters"
        )));
    }
    validate_label(&context, "the label", &field.label)?;
    if let Some(description) = &field.description {
        if description.trim().is_empty()
            || description.chars().count() > limits::MAX_DESCRIPTION_CHARS
        {
            return Err(invalid(format!(
                "{context}: the description must be 1 to {} characters",
                limits::MAX_DESCRIPTION_CHARS
            )));
        }
    }
    let kind = field.kind;
    let text = kind == FieldKind::Text;
    let numeric = matches!(kind, FieldKind::Number | FieldKind::Integer);
    let choice = matches!(kind, FieldKind::Choice | FieldKind::Choices);
    let misplaced = [
        ("min_length", field.min_length.is_some() && !text),
        ("max_length", field.max_length.is_some() && !text),
        ("format", field.format.is_some() && !text),
        ("minimum", field.minimum.is_some() && !numeric),
        ("maximum", field.maximum.is_some() && !numeric),
        ("options", field.options.is_some() && !choice),
        (
            "min_items",
            field.min_items.is_some() && kind != FieldKind::Choices,
        ),
        (
            "max_items",
            field.max_items.is_some() && kind != FieldKind::Choices,
        ),
    ];
    if let Some((name, _)) = misplaced.iter().find(|(_, misplaced)| *misplaced) {
        return Err(invalid(format!(
            "{context}: {name} does not apply to a {} field",
            kind_name(kind)
        )));
    }
    match kind {
        FieldKind::Text => {
            let max = field.text_max();
            if max > limits::MAX_FORM_TEXT_UTF16 || field.min_length.unwrap_or(0) > max {
                return Err(invalid(format!(
                    "{context}: lengths must satisfy min_length <= max_length <= {}",
                    limits::MAX_FORM_TEXT_UTF16
                )));
            }
        }
        FieldKind::Number => {
            if let (Some(min), Some(max)) = (&field.minimum, &field.maximum) {
                if number_value(min) > number_value(max) {
                    return Err(invalid(format!("{context}: minimum exceeds maximum")));
                }
            }
        }
        FieldKind::Integer => {
            let bound = |number: Option<&Number>| -> Result<Option<i64>> {
                number
                    .map(|number| {
                        number
                            .as_i64()
                            .filter(|value| value.abs() <= MAX_SAFE_INTEGER)
                            .ok_or_else(|| {
                                invalid(format!("{context}: integer bounds must be safe integers"))
                            })
                    })
                    .transpose()
            };
            if let (Some(min), Some(max)) = (
                bound(field.minimum.as_ref())?,
                bound(field.maximum.as_ref())?,
            ) {
                if min > max {
                    return Err(invalid(format!("{context}: minimum exceeds maximum")));
                }
            }
        }
        FieldKind::Boolean => {}
        FieldKind::Choice | FieldKind::Choices => {
            let Some(options) = &field.options else {
                return Err(invalid(format!("{context}: a choice field needs options")));
            };
            validate_options(&context, options, limits::MAX_FIELD_OPTIONS)?;
            if kind == FieldKind::Choices {
                let count = u32::try_from(options.len()).unwrap_or(u32::MAX);
                let max = field.max_items.unwrap_or(count);
                if field.min_items.unwrap_or(0) > max || max > count {
                    return Err(invalid(format!(
                        "{context}: items must satisfy min_items <= max_items <= the option count"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn validate_options(context: &str, options: &[Choice], max: usize) -> Result<()> {
    if options.len() < 2 || options.len() > max {
        return Err(invalid(format!(
            "{context}: 2 to {max} options are allowed"
        )));
    }
    let mut values = HashSet::new();
    for option in options {
        validate_label(context, "an option value", &option.value)?;
        validate_label(context, "an option label", &option.label)?;
        if !values.insert(option.value.as_str()) {
            return Err(invalid(format!(
                "{context}: option value {:?} is used twice",
                option.value
            )));
        }
    }
    if options.iter().filter(|option| option.recommended).count() > 1 {
        return Err(invalid(format!(
            "{context}: at most one option may be recommended"
        )));
    }
    Ok(())
}

const fn kind_name(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::Text => "text",
        FieldKind::Number => "number",
        FieldKind::Integer => "integer",
        FieldKind::Boolean => "boolean",
        FieldKind::Choice => "choice",
        FieldKind::Choices => "choices",
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "a JSON number is compared as the double the page reads"
)]
fn number_value(number: &Number) -> f64 {
    number
        .as_f64()
        .or_else(|| number.as_i64().map(|value| value as f64))
        .or_else(|| number.as_u64().map(|value| value as f64))
        .unwrap_or(f64::NAN)
}

/// A `form` block or a v2 `decision`, as the page shows it and the ledger
/// checks it: the fields in order and which of them a submit must answer.
#[derive(Debug, Clone)]
pub struct FormView<'a> {
    pub block: &'a Block,
    pub id: &'a str,
    pub title: &'a str,
    pub fields: Cow<'a, [FormField]>,
    required: Cow<'a, [String]>,
}

impl<'a> FormView<'a> {
    /// The form view of a block, or `None` for any other block, a v1
    /// decision (with its author-set status) included.
    #[must_use]
    pub fn of(block: &'a Block) -> Option<Self> {
        match block {
            Block::Form {
                id,
                title,
                fields,
                required,
                ..
            } => Some(Self {
                block,
                id,
                title,
                fields: Cow::Borrowed(fields),
                required: Cow::Borrowed(required),
            }),
            Block::Decision {
                id,
                title,
                status: None,
                options: Some(options),
                rationale,
                ..
            } => Some(Self {
                block,
                id,
                title,
                fields: Cow::Owned(vec![decision_field(
                    title,
                    options,
                    rationale.unwrap_or(RationaleMode::Optional),
                )]),
                required: Cow::Owned(vec![DECISION_FIELD_ID.to_string()]),
            }),
            _ => None,
        }
    }

    /// Whether a submit must answer the field. A decision's `choice` is.
    #[must_use]
    pub fn is_required(&self, field_id: &str) -> bool {
        self.required.iter().any(|id| id == field_id)
    }

    /// The form digest (B6): the block digest of `state.rs`, which the
    /// runtime renders as `data-cf-form-digest`.
    #[must_use]
    pub fn digest(&self) -> String {
        crate::state::block_digest(self.block)
    }

    /// The question as the reviewer saw it (B7): title, field labels and
    /// option labels.
    #[must_use]
    pub fn snapshot(&self) -> QuestionSnapshot {
        QuestionSnapshot {
            title: self.title.to_string(),
            fields: self
                .fields
                .iter()
                .map(|field| FieldSnapshot {
                    id: field.id.clone(),
                    label: field.label.clone(),
                    kind: field.kind,
                    options: field.options.as_ref().map(|options| {
                        options
                            .iter()
                            .map(|option| OptionSnapshot {
                                value: option.value.clone(),
                                label: option.label.clone(),
                            })
                            .collect()
                    }),
                })
                .collect(),
        }
    }

    /// Checks the outcome, values, rationales and reason of a request
    /// against this form (B6). The revision, digest, request id and
    /// amendment checks are the ledger's. Every problem is reported, at
    /// most one value problem and one rationale problem per field, in field
    /// order, then unknown names in sorted order.
    pub fn validate_answer(&self, request: &AnswerRequest) -> Result<()> {
        let mut errors = Vec::new();
        match request.outcome {
            Outcome::Submit => {
                if request.reason.is_some() {
                    errors.push(FieldError::new("reason", FieldErrorCode::UnknownField));
                }
                for field in self.fields.iter() {
                    let value = request
                        .values
                        .get(&field.id)
                        .filter(|value| !value.is_null());
                    if value.is_none_or(is_empty_answer) && self.is_required(&field.id) {
                        errors.push(FieldError::new(&field.id, FieldErrorCode::Required));
                    } else if let Some(value) = value {
                        if let Err(code) = check_value(field, value) {
                            errors.push(FieldError::new(&field.id, code));
                        }
                    }
                    let answered = value.is_some_and(|value| !is_empty_answer(value));
                    if let Some(code) = check_rationale(
                        field,
                        request.rationales.get(&field.id).map(String::as_str),
                        answered,
                    ) {
                        errors.push(FieldError::new(&field.id, code));
                    }
                }
                let mut unknown: Vec<&String> = request
                    .values
                    .keys()
                    .chain(request.rationales.keys())
                    .filter(|key| !self.fields.iter().any(|field| &field.id == *key))
                    .collect();
                unknown.sort();
                unknown.dedup();
                errors.extend(
                    unknown
                        .into_iter()
                        .map(|key| FieldError::new(key, FieldErrorCode::UnknownField)),
                );
            }
            Outcome::Decline | Outcome::Cancel => {
                let mut values: Vec<&String> = request.values.keys().collect();
                values.sort();
                errors.extend(
                    values
                        .into_iter()
                        .map(|key| FieldError::new(key, FieldErrorCode::UnknownField)),
                );
                errors.extend(
                    request
                        .rationales
                        .keys()
                        .map(|key| FieldError::new(key, FieldErrorCode::RationaleNotAllowed)),
                );
                match (&request.reason, request.outcome) {
                    (Some(_), Outcome::Cancel) => {
                        errors.push(FieldError::new("reason", FieldErrorCode::UnknownField));
                    }
                    (Some(reason), _) if reason.len() > limits::MAX_DECLINE_REASON_BYTES => {
                        errors.push(FieldError::new("reason", FieldErrorCode::TooLong));
                    }
                    _ => {}
                }
            }
        }
        if errors.is_empty() {
            return Ok(());
        }
        let message = match request.outcome {
            Outcome::Submit => "the answer does not satisfy the form",
            Outcome::Decline => {
                "a decline carries no values or rationales and at most a 4 KiB reason"
            }
            Outcome::Cancel => "a cancel carries no values, rationales or reason",
        };
        Err(invalid_answer(message, &errors))
    }
}

fn decision_field(title: &str, options: &[Choice], rationale: RationaleMode) -> FormField {
    FormField {
        id: DECISION_FIELD_ID.to_string(),
        label: title.to_string(),
        kind: FieldKind::Choice,
        description: None,
        rationale: Some(rationale),
        min_length: None,
        max_length: None,
        format: None,
        minimum: None,
        maximum: None,
        options: Some(options.to_vec()),
        min_items: None,
        max_items: None,
    }
}

/// An empty string or an empty list answers nothing.
fn is_empty_answer(value: &Value) -> bool {
    match value {
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

fn check_rationale(
    field: &FormField,
    rationale: Option<&str>,
    answered: bool,
) -> Option<FieldErrorCode> {
    match field.rationale_mode() {
        RationaleMode::None if rationale.is_some() => Some(FieldErrorCode::RationaleNotAllowed),
        RationaleMode::Required
            if answered && rationale.is_none_or(|text| text.trim().is_empty()) =>
        {
            Some(FieldErrorCode::RationaleRequired)
        }
        _ => None,
    }
}

fn check_value(field: &FormField, value: &Value) -> std::result::Result<(), FieldErrorCode> {
    match field.kind {
        FieldKind::Text => {
            let text = value.as_str().ok_or(FieldErrorCode::WrongKind)?;
            let units = text.encode_utf16().count();
            if units < field.min_length.unwrap_or(0) as usize {
                return Err(FieldErrorCode::TooShort);
            }
            if units > field.text_max() as usize {
                return Err(FieldErrorCode::TooLong);
            }
            let valid = match field.format {
                None | Some(TextFormat::Multiline) => true,
                Some(TextFormat::Email) => is_email(text),
                Some(TextFormat::Uri) => is_uri(text),
                Some(TextFormat::Date) => is_full_date(text),
                Some(TextFormat::DateTime) => is_date_time(text),
            };
            if valid {
                Ok(())
            } else {
                Err(FieldErrorCode::Format)
            }
        }
        FieldKind::Number => {
            let Value::Number(number) = value else {
                return Err(FieldErrorCode::WrongKind);
            };
            check_bounds(field, number_value(number))
        }
        FieldKind::Integer => {
            let Value::Number(number) = value else {
                return Err(FieldErrorCode::WrongKind);
            };
            let value = number_value(number);
            if value.fract() != 0.0 {
                return Err(FieldErrorCode::NotInteger);
            }
            #[allow(clippy::cast_precision_loss, reason = "2^53 - 1 is exact in a double")]
            let safe = MAX_SAFE_INTEGER as f64;
            if value < -safe {
                return Err(FieldErrorCode::BelowMinimum);
            }
            if value > safe {
                return Err(FieldErrorCode::AboveMaximum);
            }
            check_bounds(field, value)
        }
        FieldKind::Boolean => {
            if value.is_boolean() {
                Ok(())
            } else {
                Err(FieldErrorCode::WrongKind)
            }
        }
        FieldKind::Choice => {
            let chosen = value.as_str().ok_or(FieldErrorCode::WrongKind)?;
            if has_option(field, chosen) {
                Ok(())
            } else {
                Err(FieldErrorCode::UnknownOption)
            }
        }
        FieldKind::Choices => {
            let items = value.as_array().ok_or(FieldErrorCode::WrongKind)?;
            let chosen = items
                .iter()
                .map(|item| item.as_str().ok_or(FieldErrorCode::WrongKind))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            if chosen.iter().any(|value| !has_option(field, value)) {
                return Err(FieldErrorCode::UnknownOption);
            }
            let count = u32::try_from(chosen.len()).unwrap_or(u32::MAX);
            if count < field.min_items.unwrap_or(0) {
                return Err(FieldErrorCode::TooFew);
            }
            if field.max_items.is_some_and(|max| count > max) {
                return Err(FieldErrorCode::TooMany);
            }
            // A choices value is a set: a repeated option is the wrong shape.
            let distinct: HashSet<&str> = chosen.iter().copied().collect();
            if distinct.len() == chosen.len() {
                Ok(())
            } else {
                Err(FieldErrorCode::WrongKind)
            }
        }
    }
}

fn has_option(field: &FormField, value: &str) -> bool {
    field
        .options
        .as_ref()
        .is_some_and(|options| options.iter().any(|option| option.value == value))
}

fn check_bounds(field: &FormField, value: f64) -> std::result::Result<(), FieldErrorCode> {
    if field
        .minimum
        .as_ref()
        .is_some_and(|min| value < number_value(min))
    {
        return Err(FieldErrorCode::BelowMinimum);
    }
    if field
        .maximum
        .as_ref()
        .is_some_and(|max| value > number_value(max))
    {
        return Err(FieldErrorCode::AboveMaximum);
    }
    Ok(())
}

/// B6 `email`: one `@` with a non-empty local part and a domain holding a
/// dot, and no whitespace. The page's `isEmail` is the same check.
#[must_use]
pub fn is_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !text.chars().any(char::is_whitespace)
}

/// B6 `uri`: an absolute URI, that is an RFC 3986 scheme (a letter, then
/// letters, digits, `+`, `-` or `.`), a colon and a non-empty rest of
/// printable ASCII without spaces.
#[must_use]
pub fn is_uri(text: &str) -> bool {
    let Some((scheme, rest)) = text.split_once(':') else {
        return false;
    };
    let mut scheme_bytes = scheme.bytes();
    scheme_bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && scheme_bytes
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
        && !rest.is_empty()
        && rest.bytes().all(|byte| byte.is_ascii_graphic())
}

fn digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// RFC 3339 `full-date`: `YYYY-MM-DD` naming a real day.
#[must_use]
pub fn is_full_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let (Some(year), Some(month), Some(day)) = (
        digits(&text[0..4]),
        digits(&text[5..7]),
        digits(&text[8..10]),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// RFC 3339 `date-time`: a full-date, `T`, `HH:MM:SS`, an optional
/// fraction, and `Z` or a `+HH:MM` or `-HH:MM` offset.
#[must_use]
pub fn is_date_time(text: &str) -> bool {
    if !text.is_ascii() || text.len() < 20 {
        return false;
    }
    let (date, rest) = text.split_at(10);
    if !is_full_date(date) || !matches!(rest.as_bytes()[0], b'T' | b't') {
        return false;
    }
    let time = &rest[1..];
    let bytes = time.as_bytes();
    if bytes.len() < 9 || bytes[2] != b':' || bytes[5] != b':' {
        return false;
    }
    let (Some(hour), Some(minute), Some(second)) = (
        digits(&time[0..2]),
        digits(&time[3..5]),
        digits(&time[6..8]),
    ) else {
        return false;
    };
    if hour > 23 || minute > 59 || second > 60 {
        return false;
    }
    let mut offset = &time[8..];
    if let Some(fraction) = offset.strip_prefix('.') {
        let end = fraction
            .bytes()
            .position(|byte| !byte.is_ascii_digit())
            .unwrap_or(fraction.len());
        if end == 0 {
            return false;
        }
        offset = &fraction[end..];
    }
    match offset.as_bytes() {
        [b'Z' | b'z'] => true,
        [b'+' | b'-', _, _, b':', _, _] => matches!(
            (digits(&offset[1..3]), digits(&offset[4..6])),
            (Some(hours), Some(minutes)) if hours <= 23 && minutes <= 59
        ),
        _ => false,
    }
}

/// The three outcomes of B6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Submit,
    Decline,
    Cancel,
}

/// The body of `POST /app/api/answers`, chosen by the page (B6). The actor
/// and the time are the server's: a body that names either is refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerRequest {
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub revision: u64,
    pub form_id: String,
    pub form_digest: String,
    pub outcome: Outcome,
    #[serde(default)]
    pub values: Map<String, Value>,
    #[serde(default)]
    pub rationales: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amends: Option<Uuid>,
}

/// A parsed request and the digest of its body bytes as received.
#[derive(Debug, Clone, PartialEq)]
pub struct AnswerSubmission {
    pub request: AnswerRequest,
    /// SHA-256 of the body bytes as received (B6): a retry is the same
    /// payload only when the page resends the same bytes.
    pub payload_digest: String,
}

impl AnswerSubmission {
    /// Parses a request body: the 64 KiB bound first, then the shape.
    pub fn parse(body: &[u8]) -> Result<Self> {
        if body.len() > limits::MAX_ANSWER_REQUEST_BYTES {
            return Err(PresentError::review(
                "answer_too_large",
                format!(
                    "the answer request is {} bytes; the limit is {} bytes",
                    body.len(),
                    limits::MAX_ANSWER_REQUEST_BYTES
                ),
                serde_json::json!({ "limit_bytes": limits::MAX_ANSWER_REQUEST_BYTES }),
            ));
        }
        let request = serde_json::from_slice(body).map_err(|error| {
            invalid_answer(&format!("the answer request is malformed: {error}"), &[])
        })?;
        Ok(Self {
            request,
            payload_digest: sha256_hex(body),
        })
    }
}

/// The question as the reviewer saw it, stored with each answer (B7, I4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionSnapshot {
    pub title: String,
    pub fields: Vec<FieldSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSnapshot {
    pub id: String,
    pub label: String,
    pub kind: FieldKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<OptionSnapshot>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionSnapshot {
    pub value: String,
    pub label: String,
}

/// The per-field codes of I3 `invalid_answer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldErrorCode {
    Required,
    UnknownField,
    WrongKind,
    TooShort,
    TooLong,
    Format,
    BelowMinimum,
    AboveMaximum,
    NotInteger,
    UnknownOption,
    TooFew,
    TooMany,
    RationaleRequired,
    RationaleNotAllowed,
}

/// One entry of `details.fields` in an `invalid_answer` body. A problem
/// with the decline `reason` names the field `reason`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldError {
    pub field: String,
    pub code: FieldErrorCode,
}

impl FieldError {
    #[must_use]
    pub fn new(field: &str, code: FieldErrorCode) -> Self {
        Self {
            field: field.to_string(),
            code,
        }
    }
}

/// An `invalid_answer` refusal (I3, 422) with its field errors.
#[must_use]
pub fn invalid_answer(message: &str, fields: &[FieldError]) -> PresentError {
    PresentError::review(
        "invalid_answer",
        message,
        serde_json::json!({ "fields": fields }),
    )
}

/// Lowercase hex SHA-256.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn invalid(message: impl Into<String>) -> PresentError {
    PresentError::InvalidDocument(message.into())
}
