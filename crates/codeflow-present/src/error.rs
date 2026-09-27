use std::path::PathBuf;

/// Errors surfaced by the presentation boundary.
#[derive(Debug, thiserror::Error)]
pub enum PresentError {
    #[error("presentation document exceeds {limit} bytes")]
    DocumentTooLarge { limit: usize },
    #[error("invalid presentation document: {0}")]
    InvalidDocument(String),
    #[error("unsupported presentation schema version {found}; supported version is {supported}")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("invalid presentation session id: {0}")]
    InvalidSessionId(String),
    #[error("presentation session not found: {0}")]
    SessionNotFound(String),
    #[error("presentation session is closed: {0}")]
    SessionClosed(String),
    #[error("unsafe presentation state path: {0}")]
    UnsafePath(PathBuf),
    #[error("presentation state is corrupt: {0}")]
    CorruptState(String),
    #[error("presentation browser launch is not qualified: {0}")]
    BrowserUnavailable(String),
    #[error("presentation service did not become ready: {0}")]
    ServiceUnavailable(String),
    #[error("presentation cleanup was partial; removed {removed:?}; retained {failures:?}")]
    PartialCleanup {
        removed: Vec<String>,
        failures: Vec<String>,
    },
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A review or answer the service refuses with a typed code (SPC-014
    /// I3); the page reads the code, the CLI treats it as invalid input.
    #[error("{message}")]
    Review {
        code: &'static str,
        message: String,
        details: serde_json::Value,
    },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl PresentError {
    pub fn review(
        code: &'static str,
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self::Review {
            code,
            message: message.into(),
            details,
        }
    }

    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, PresentError>;
