//! Bounded local presentation sessions for `codeflow present`.

#![allow(
    clippy::missing_errors_doc,
    reason = "this internal, non-published crate exposes one typed error boundary to its CLI adapter"
)]

#[cfg(test)]
mod answer_contract_tests;
pub mod browser;
pub mod config;
#[cfg(test)]
mod contract_tests;
pub mod document;
pub mod entity;
pub mod error;
pub mod export;
pub mod form;
mod fuzzy;
pub mod limits;
mod media;
mod platform;
pub mod render;
pub mod retired;
mod safe_html;
mod scoped_css;
pub mod service;
pub mod state;

pub use document::PresentationDocument;
pub use error::{PresentError, Result};
pub use state::{FeedbackEnvelope, FeedbackVerdict, SessionRecord, SessionStore};
