//! SQLite schema for the derived event index.
//!
//! Design notes from architectural review:
//!
//! - `events` is STRICT so schema drift is caught at insert time.
//! - `SubjectRef` is flattened into (subject_kind, subject_id). Without the
//!   discriminator, `WHERE subject_id = ...` would ambiguously hit both
//!   Entity and Batch ids.
//! - `authority` is split into three columns (source_kind, score,
//!   attested_by). Callers want to filter by provenance, not score alone.
//! - `chain_id` + `parent_event_id` are first-class so supersession chains
//!   are queryable without parsing payload_json.
//! - `is_redacted` is a first-class column updated by Redact ingest,
//!   because downstream readers need to hide redacted events cheaply.
//! - FTS5 has THREE columns (title/body/tags) so BM25 can weight them
//!   differently at search time. Tokenizer is `unicode61 remove_diacritics`
//!   with prefix indexing 2/3/4 chars — agents type fragments, and Porter
//!   stemming would destroy recall on identifiers like `fastapi-users`.
//! - `pref_current` and `claim_current` are materialized projections.
//!   `claim_current` holds the chain tip; Verify events annotate `verdict`
//!   without removing the row (refutation is information, not deletion).
//! - Schema version lives in `PRAGMA user_version`. The `watermark` table
//!   only carries `last_commit_oid` (string; pragma values are int-only).
//!
//! Bump `SCHEMA_VERSION` for any breaking schema change — openers will
//! raise `IndexError::SchemaMismatch` and the user can rebuild from git.

pub const SCHEMA_VERSION: u32 = 1;

/// All DDL, run in one transaction by `apply()`.
///
/// Statements are separated by `;\n` and executed with `execute_batch`.
/// Virtual-table creation must happen in the same batch as the rest so a
/// partially-initialized db can't be opened.
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE events (
    event_id               TEXT PRIMARY KEY NOT NULL,
    commit_oid             TEXT NOT NULL,
    event_type             TEXT NOT NULL,
    subject_kind           TEXT NOT NULL,
    subject_id             TEXT,
    chain_id               TEXT,
    parent_event_id        TEXT,
    actor_kind             TEXT NOT NULL,
    actor_id               TEXT NOT NULL,
    actor_harness          TEXT,
    layer                  TEXT NOT NULL,
    authority_source_kind  TEXT NOT NULL,
    authority_score        INTEGER,
    authority_attested_by  TEXT,
    signature_state        TEXT NOT NULL,
    classification         TEXT NOT NULL,
    time_observed          INTEGER NOT NULL,
    time_recorded          INTEGER NOT NULL,
    idempotency_key        TEXT,
    is_redacted            INTEGER NOT NULL DEFAULT 0,
    payload_json           TEXT NOT NULL
) STRICT;

CREATE INDEX idx_events_type_time ON events(event_type, time_observed DESC);
CREATE INDEX idx_events_actor ON events(actor_id);
CREATE INDEX idx_events_subject ON events(subject_kind, subject_id);
CREATE INDEX idx_events_chain ON events(chain_id) WHERE chain_id IS NOT NULL;
CREATE UNIQUE INDEX idx_events_idem ON events(idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE VIRTUAL TABLE events_fts USING fts5(
    event_id UNINDEXED,
    title,
    body,
    tags,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix   = '2 3 4'
);

CREATE TABLE pref_current (
    category   TEXT NOT NULL,
    key        TEXT NOT NULL,
    value_json TEXT NOT NULL,
    event_id   TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (category, key)
) STRICT;

CREATE TABLE claim_current (
    schema_type           TEXT NOT NULL,
    key                   TEXT NOT NULL,
    event_id              TEXT NOT NULL,
    chain_id              TEXT NOT NULL,
    content_json          TEXT NOT NULL,
    supersedes_event_id   TEXT,
    verdict               TEXT,
    verdict_event_id      TEXT,
    verdict_at            INTEGER,
    is_archived           INTEGER NOT NULL DEFAULT 0,
    updated_at            INTEGER NOT NULL,
    PRIMARY KEY (schema_type, key)
) STRICT;

CREATE TABLE entity_archive (
    target_entity TEXT PRIMARY KEY NOT NULL,
    direction     TEXT NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;

-- Free-form key/value for string-valued watermarks. PRAGMA user_version
-- can only store integers, so last_commit_oid lives here.
CREATE TABLE watermark (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;
"#;

/// Apply the schema in one transaction. Idempotent: safe to call repeatedly
/// because `apply()` first checks `user_version` and short-circuits if the
/// tables are already at the right version.
pub fn apply(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current == SCHEMA_VERSION {
        return Ok(());
    }
    if current != 0 {
        // Non-zero, non-matching — caller should have raised SchemaMismatch
        // before reaching this function. Guard anyway.
        return Err(rusqlite::Error::InvalidQuery);
    }
    conn.execute_batch(SCHEMA_SQL)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}
