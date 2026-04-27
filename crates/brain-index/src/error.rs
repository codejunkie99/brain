//! Index error type. Wraps rusqlite + io + serde, plus staleness signals.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),

    /// Index schema on disk doesn't match the version this binary knows how
    /// to read. Bump requires a full rebuild from git.
    #[error("index schema version mismatch at {path}: on disk = {on_disk}, expected = {expected}")]
    SchemaMismatch {
        path: String,
        on_disk: u32,
        expected: u32,
    },

    /// Catch-all for logic errors where an invariant was broken (e.g., an
    /// event in events_fts with no matching row in events).
    #[error("invariant violated: {detail}")]
    Invariant { detail: String },
}
