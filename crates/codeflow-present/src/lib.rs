//! Bounded local presentation sessions for `codeflow present`.

#![allow(
    clippy::missing_errors_doc,
    reason = "this internal, non-published crate exposes one typed error boundary to its CLI adapter"
)]

pub mod browser;
pub mod config;
#[cfg(test)]
mod contract_tests;
pub mod document;
pub mod entity;
pub mod error;
pub mod export;
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
