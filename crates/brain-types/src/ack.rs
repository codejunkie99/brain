//! Ack levels, event states, and return shapes for the write path.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How durable the caller needs the write to be before the call returns.
/// Default for MCP tool calls is `Committed`.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AckLevel {
    /// In the writer's mpsc queue. Fast (~1ms), no durability yet.
    Accepted,
    /// Batch flushed to git, commit_oid known. Durable (~10-50ms).
    #[default]
    Committed,
    /// sqlite index watermark has caught up to this commit. Searchable.
    Indexed,
}

/// Terminal state of an event.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EventState {
    Accepted,
    Committed,
    Indexed,
    Rejected {
        reason: RejectReason,
    },
    CommitFailedRetryable {
        reason: FailureReason,
        retry_count: u32,
    },
    CommitFailedFinal {
        reason: FailureReason,
    },
}

/// Why pre-validation or the redaction check rejected a draft.
///
/// This is the single source of truth for draft-rejection reasons: the
/// same type surfaces in `shallow_validate`, in `BatchEntryResult`, and on
/// the RPC wire. (Codex 11th-pass finding #3: unified from the former
/// `DraftValidationError`.)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RejectReason {
    #[error("event_type does not match payload variant: {detail}")]
    TypeMismatch { detail: String },
    #[error("schema_version {given} not supported by this binary (expected {expected})")]
    UnsupportedSchema { given: u32, expected: u32 },
    #[error("idempotency_key is empty")]
    EmptyIdempotencyKey,
    #[error("idempotency_key exceeds max length: {len} > {max}")]
    IdempotencyKeyTooLong { len: usize, max: usize },
    #[error("idempotency_key contains non-ASCII-printable characters")]
    IdempotencyKeyInvalidChars,
    #[error("secret pattern detected: {pattern}")]
    SecretDetected { pattern: String },
    #[error("payload is missing required field: {field}")]
    MissingField { field: String },
    #[error("unknown actor id: {id}")]
    UnknownActor { id: String },
    #[error("layer {layer} is invalid for payload type {payload_type}")]
    LayerMismatch { layer: String, payload_type: String },
    /// `time_observed` outside the supported range. A caller-controlled
    /// far-future or far-past timestamp commits fine but fails to round-
    /// trip through `DateTime::from_timestamp_millis` on read, making the
    /// event durably invisible — a content-hiding primitive. Bounds:
    /// [1970-01-01, 9999-12-31]. Caught at `shallow_validate`.
    #[error("time_observed {given} is outside the supported range {min}..={max}")]
    TimeObservedOutOfRange {
        given: String,
        min: String,
        max: String,
    },
    #[error("{msg}")]
    Other { msg: String },
}

/// Maximum allowed byte length for `EventDraft::idempotency_key`.
/// (Codex 11th-pass finding #4: cap attack surface on memory/log/DB amplification.)
pub const IDEMPOTENCY_KEY_MAX_LEN: usize = 128;

/// Why a commit failed after pre-validation passed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FailureReason {
    DiskFull,
    FsyncError { detail: String },
    RefCasLost { detail: String },
    GitInternal { detail: String },
    SigningFailed { detail: String },
    DaemonDown,
    Timeout { seconds: u32 },
    Other { msg: String },
}

/// Is the derived index caught up to this event's commit?
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IndexStatus {
    /// Index watermark >= this commit.
    Ok,
    /// Index is behind HEAD but actively catching up.
    Lagging,
    /// Rebuild loop is stuck or a retry is active. Queries should warn.
    Degraded,
}

/// Return shape from `append`. `commit_oid` is present once state >= Committed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventRef {
    pub event_id: Uuid,
    pub state: EventState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_oid: Option<String>,
    pub index_status: IndexStatus,
    pub reached_at: DateTime<Utc>,
    /// True if this call matched a prior idempotency_key and returned the
    /// previously-committed EventRef rather than writing a new event.
    pub was_idempotent_replay: bool,
}

/// Per-event result for `append_batch`. The batch is prevalidated in full
/// before any commit is opened; rejected drafts surface here.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchResult {
    pub batch_id: Uuid,
    pub results: Vec<BatchEntryResult>,
    /// The commit that landed, if any valid events were committed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed_oid: Option<String>,
    pub index_status: IndexStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum BatchEntryResult {
    Ok {
        event_ref: EventRef,
    },
    Rejected {
        reason: RejectReason,
    },
    /// Non-rejection runtime failure (git write error, io fault, etc).
    /// Kept per-entry so a partial batch doesn't drop its successes on
    /// the floor. Codex round 4 P1: "append_batch returned top-level Err
    /// on first non-rejection failure, discarding prior successes."
    Failed {
        detail: String,
    },
}
