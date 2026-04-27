//! brain-index: derived SQLite FTS5 index + BM25 retrieval + current-truth
//! projections over the event journal.
//!
//! The index is derived — it can be wiped and rebuilt from `git log` at
//! any time. Lives at `<brain_dir>/.brain/index.sqlite` (outside `.git/`).
//!
//! Two-phase write model: `BrainRepo::append_event` commits to git first,
//! then the caller calls `BrainIndex::ingest(&event)` in a best-effort
//! second phase. If the process dies between the two, `catch_up_from`
//! reconciles on the next open. See brain-app for the wrapper.

pub mod conn;
pub mod error;
pub mod ingest;
pub mod query;
pub mod schema;

pub use brain_types;
pub use error::IndexError;
pub use query::{ClaimProjection, PrefProjection, SearchHit, SearchQuery};

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

/// Derived SQLite index over the event journal.
///
/// Single-writer, multi-reader: one `BrainIndex` per process holds the
/// only `Connection` that writes. Read-only queries can open a fresh
/// `Connection` cheaply (< 1ms) if concurrent reads are needed — WAL mode
/// means those reads never block the writer.
pub struct BrainIndex {
    path: PathBuf,
    conn: Connection,
}

impl std::fmt::Debug for BrainIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrainIndex")
            .field("path", &self.path)
            .finish()
    }
}

impl BrainIndex {
    /// Open the index at `path`, creating + initializing if it doesn't
    /// exist. The parent directory is created if missing.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, IndexError> {
        let path = path.as_ref().to_path_buf();
        let conn = conn::open_connection(&path)?;
        Ok(Self { path, conn })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Borrow the underlying connection for reads. Use sparingly; prefer
    /// the typed API (`search`, `current_pref`, etc.).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// The commit oid of the last event successfully indexed. None on a
    /// fresh index. Used by `catch_up_from` to find unindexed events.
    pub fn last_committed_oid(&self) -> Result<Option<String>, IndexError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT value FROM watermark WHERE key = 'last_commit_oid'")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_last_committed_oid(&self, oid: &str) -> Result<(), IndexError> {
        self.conn.execute(
            "INSERT INTO watermark(key, value) VALUES('last_commit_oid', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![oid],
        )?;
        Ok(())
    }

    /// Number of indexed events. Cheap (uses the table count, not FTS5).
    pub fn event_count(&self) -> Result<usize, IndexError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    /// Every `event_id` currently in the index, as hyphenated-string
    /// form. Used by catch-up for a set-difference against git, closing
    /// the "watermark advanced past an unindexed commit" race that a
    /// single oid watermark can't detect.
    pub fn indexed_event_ids(&self) -> Result<std::collections::HashSet<String>, IndexError> {
        let mut stmt = self.conn.prepare_cached("SELECT event_id FROM events")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<std::collections::HashSet<_>, _>>()?;
        Ok(ids)
    }

    /// Delete `event_id`s from every derived table. Called by catch-up
    /// when an indexed event no longer exists in git (rewritten history,
    /// foreign-index import). All deletions run in one IMMEDIATE txn so
    /// an interrupted prune leaves no torn state.
    pub fn prune_events(&mut self, event_ids: &[String]) -> Result<(), IndexError> {
        if event_ids.is_empty() {
            return Ok(());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for id in event_ids {
            tx.execute("DELETE FROM events WHERE event_id = ?1", [id])?;
            tx.execute("DELETE FROM events_fts WHERE event_id = ?1", [id])?;
            tx.execute("DELETE FROM claim_current WHERE event_id = ?1", [id])?;
            tx.execute("DELETE FROM pref_current WHERE event_id = ?1", [id])?;
            tx.execute("DELETE FROM entity_archive WHERE target_entity = ?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Wipe every derived table. Used before a full rebuild. Schema and
    /// pragmas are preserved.
    pub fn truncate(&mut self) -> Result<(), IndexError> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(
            r#"
            DELETE FROM events;
            DELETE FROM events_fts;
            DELETE FROM pref_current;
            DELETE FROM claim_current;
            DELETE FROM entity_archive;
            DELETE FROM watermark;
            "#,
        )?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_creates_empty_index() {
        let td = TempDir::new().unwrap();
        let idx = BrainIndex::open(td.path().join("index.sqlite")).unwrap();
        assert_eq!(idx.event_count().unwrap(), 0);
        assert!(idx.last_committed_oid().unwrap().is_none());
    }

    #[test]
    fn watermark_round_trips() {
        let td = TempDir::new().unwrap();
        let path = td.path().join("index.sqlite");
        {
            let idx = BrainIndex::open(&path).unwrap();
            idx.set_last_committed_oid("abc123").unwrap();
            assert_eq!(
                idx.last_committed_oid().unwrap(),
                Some("abc123".to_string())
            );
        }
        let idx = BrainIndex::open(&path).unwrap();
        assert_eq!(
            idx.last_committed_oid().unwrap(),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn schema_version_mismatch_refuses_to_open() {
        let td = TempDir::new().unwrap();
        let path = td.path().join("index.sqlite");
        {
            let idx = BrainIndex::open(&path).unwrap();
            idx.conn
                .pragma_update(None, "user_version", 999u32)
                .unwrap();
        }
        let err = BrainIndex::open(&path).unwrap_err();
        match err {
            IndexError::SchemaMismatch { on_disk: 999, .. } => {}
            other => panic!("expected SchemaMismatch, got: {other:?}"),
        }
    }

    #[test]
    fn truncate_clears_watermark_and_counts() {
        let td = TempDir::new().unwrap();
        let mut idx = BrainIndex::open(td.path().join("index.sqlite")).unwrap();
        idx.set_last_committed_oid("abc").unwrap();
        assert!(idx.last_committed_oid().unwrap().is_some());
        idx.truncate().unwrap();
        assert!(idx.last_committed_oid().unwrap().is_none());
        assert_eq!(idx.event_count().unwrap(), 0);
    }
}
