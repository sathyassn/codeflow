//! Bounded local presentation sessions for `codeflow present`.

#![allow(
    clippy::missing_errors_doc,
    reason = "this internal, non-published crate exposes one typed error boundary to its CLI adapter"
)]

// The output sink (TSK-388): `eprint!` and `eprintln!` print through
// [`output::display_output`]. They are this crate's only terminal prints
// outside tests; a warning can quote a repository error that names a path.
macro_rules! eprint {
    ($($arg:tt)*) => {
        ::std::eprint!("{}", $crate::output::display_print(&::std::format!($($arg)*)))
    };
}
// Through `eprint!`, so both stay in use; escaping is idempotent.
macro_rules! eprintln {
    ($($arg:tt)*) => {
        eprint!("{}\n", ::std::format!($($arg)*))
    };
}

pub mod output;

#[cfg(test)]
mod answer_contract_tests;
pub mod browser;
pub mod config;
#[cfg(test)]
mod contract_tests;
pub mod conversation;
#[cfg(test)]
mod conversation_tests;
pub mod delivery;
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
pub mod responses;
pub mod retired;
pub mod revision;
mod safe_html;
mod scoped_css;
pub mod service;
pub mod state;

pub use document::PresentationDocument;
pub use error::{PresentError, Result};
pub use state::{FeedbackEnvelope, FeedbackVerdict, SessionRecord, SessionStore};
