//! Bounded local presentation sessions for `codeflow present`.

pub mod browser;
pub mod config;
pub mod document;
pub mod error;
pub mod export;
pub mod limits;
pub mod render;
pub mod service;
pub mod state;

pub use document::PresentationDocument;
pub use error::{PresentError, Result};
pub use state::{FeedbackEnvelope, FeedbackVerdict, SessionRecord, SessionStore};
