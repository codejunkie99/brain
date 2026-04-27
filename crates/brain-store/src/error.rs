//! Store error type. Wraps git2, io, serde, and surfaces RejectReason
//! from brain-types so callers see one error taxonomy.

use brain_types::RejectReason;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("git: {0}")]
    Git(#[from] git2::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("rejected: {0}")]
    Rejected(#[from] RejectReason),

    #[error("brain already exists at {path}")]
    AlreadyExists { path: String },

    #[error("brain not found at {path}")]
    NotFound { path: String },

    #[error("{path} is a git repository but not a brain (no SCHEMA marker); refusing to touch it")]
    NotABrain { path: String },

    /// HEAD is detached. Commits would land on an anonymous chain that
    /// evaporates on the next branch checkout. Refuse at both `open`
    /// and `append` time.
    #[error("brain at {path} is on a detached HEAD; check out a branch before using it")]
    DetachedHead { path: String },

    /// Event blob exceeds the size cap. Applies to both write-time
    /// serialization and read-time parsing so an oversized blob never
    /// poisons either path.
    #[error("event blob too large: {bytes} bytes exceeds {cap}-byte cap")]
    OversizedBlob { bytes: usize, cap: usize },

    /// Commit passed the trailer parse but the blob it points to
    /// doesn't belong to that trailer — either the blob's internal
    /// `event_id` disagrees with the trailer, or the trailer reused an
    /// old blob that was inherited from a parent tree. Both are only
    /// reachable under deliberate tampering.
    #[error("forged commit {oid}: {detail}")]
    ForgedCommit { oid: String, detail: String },

    /// Commit OID failed to parse as hex. Caller typo or corruption.
    #[error("bad commit oid: {oid}")]
    BadOid { oid: String },

    /// The on-disk SCHEMA blob declares a different schema version than
    /// this binary supports. Caller decides whether to migrate or
    /// rebuild from a different binary.
    #[error(
        "brain at {path} is schema_version={on_disk}, this binary supports schema_version={expected}"
    )]
    SchemaMismatch {
        path: String,
        on_disk: u32,
        expected: u32,
    },

    /// Last-resort variant. Use only at internal invariants callers
    /// cannot meaningfully handle beyond surfacing the message.
    #[error("unexpected: {0}")]
    Unexpected(String),
}

impl StoreError {
    /// True if this error looks like transient multi-process init contention:
    /// either another process holds a libgit2 lockfile (`.git/config.lock`)
    /// right now, OR the winner is mid-init and hasn't written the bootstrap
    /// commit yet (HEAD is unborn, so we see `NotABrain`), OR the winner just
    /// beat us to the create-dir step (`AlreadyExists`).
    ///
    /// Callers use this to decide whether to back off and retry. Keep the
    /// set narrow — NotFound and Rejected are NOT transient; they mean the
    /// caller's request is wrong, not that someone else is mid-flight.
    pub fn is_transient_init_race(&self) -> bool {
        match self {
            Self::Git(e) => e.code() == git2::ErrorCode::Locked,
            Self::AlreadyExists { .. } => true,
            Self::NotABrain { .. } => true,
            _ => false,
        }
    }
}
