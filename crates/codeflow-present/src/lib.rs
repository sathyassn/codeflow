//! Bounded local presentation sessions for `codeflow present`.

#![allow(
    clippy::missing_errors_doc,
    reason = "this internal, non-published crate exposes one typed error boundary to its CLI adapter"
)]

pub mod browser;
pub mod config;
pub mod document;
pub mod error;
pub mod export;
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
