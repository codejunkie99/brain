//! Search + projection accessors. FTS5 BM25 when a text query is given,
//! pure filter over `events` when it isn't.

use brain_types::EventType;
use chrono::{DateTime, Utc};
use rusqlite::{ToSql, params_from_iter};
use serde_json::Value;
use tracing::warn;
use uuid::Uuid;

use crate::BrainIndex;
use crate::error::IndexError;

/// Filter + full-text spec for a search. `text` is passed through a
/// lightweight rewriter (`escape_fts`) before hitting FTS5 MATCH.
/// `unicode61` tokenizer + prefix indexing on 2/3/4 chars at ingest,
/// and the rewriter auto-appends `*` to each bare word at query time so
/// typing `fast` matches `fastapi-users` without explicit syntax. Use
/// `"exact phrase"`, `foo*`, AND/OR/NOT, or `title:foo` for power-user
/// queries and the rewriter passes the text through verbatim (just
/// doubling stray `"`).
///
/// Bitemporal axes:
///   - `since` / `until` filter **event-time** (`time_observed`): when did
///     the fact happen in reality?
///   - `known_since` / `known_until` filter **transaction-time**
///     (`time_recorded`): restricts to events whose record time falls in
///     the given range.
///
/// NOTE: `known_until` is NOT a full "as known at T" bitemporal query.
/// It filters `time_recorded` but evaluates archive/redact state at the
/// CURRENT moment, not at T. If an event was recorded before T and
/// archived after T, this filter still hides it even though at T the
/// system hadn't yet learned the archive. True bitemporal point-in-time
/// reads need archive/redact state to carry effective-from/to columns
/// — a schema change deferred to a later version. Codex round 9 P1.
///
/// All fields default to "no filter". `limit` defaults to 20 if left at 0.
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub text: Option<String>,
    pub event_types: Option<Vec<EventType>>,
    pub actor_id: Option<String>,
    /// Event-time lower bound: `time_observed >= since`.
    pub since: Option<DateTime<Utc>>,
    /// Event-time upper bound: `time_observed <= until`.
    pub until: Option<DateTime<Utc>>,
    /// Transaction-time lower bound: `time_recorded >= known_since`.
    pub known_since: Option<DateTime<Utc>>,
    /// Transaction-time upper bound: `time_recorded <= known_until`.
    pub known_until: Option<DateTime<Utc>>,
    /// Include hidden rows. This disables both redaction and archive filters;
    /// it is intended for doctor/audit views, not normal user search.
    pub include_redacted: bool,
    pub limit: usize,
}

impl SearchQuery {
    /// Convenience: a text-only search with default limit (20).
    pub fn text(q: impl Into<String>) -> Self {
        Self {
            text: Some(q.into()),
            limit: 20,
            ..Self::default()
        }
    }
}

/// One search result. `score` is BM25 when a text query was present —
/// in SQLite FTS5, LOWER bm25 means a better match, so results are
/// returned ordered by ascending score. `score` is 0.0 for pure filter
/// queries (which instead sort by `time_observed DESC`).
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub event_id: Uuid,
    pub commit_oid: String,
    pub event_type: EventType,
    pub time_observed: DateTime<Utc>,
    pub score: f64,
    pub snippet: Option<String>,
}

/// Current-truth projection row for a preference.
#[derive(Debug, Clone)]
pub struct PrefProjection {
    pub category: String,
    pub key: String,
    pub value: Value,
    pub event_id: Uuid,
    pub updated_at: DateTime<Utc>,
}

/// Current-truth projection row for a claim. `verdict` is None unless a
/// Verify event has annotated this claim; `is_archived` reflects the
/// latest Archive event targeting it.
#[derive(Debug, Clone)]
pub struct ClaimProjection {
    pub schema_type: String,
    pub key: String,
    pub event_id: Uuid,
    pub chain_id: String,
    pub content: Value,
    pub supersedes_event_id: Option<Uuid>,
    pub verdict: Option<String>,
    pub verdict_event_id: Option<Uuid>,
    pub verdict_at: Option<DateTime<Utc>>,
    pub is_archived: bool,
    pub updated_at: DateTime<Utc>,
}

impl BrainIndex {
    /// Run a search.
    ///
    /// - With `q.text`: FTS5 MATCH with `bm25(events_fts, 10.0, 1.0, 2.0)` —
    ///   title is weighted 10x body and 5x tags. Results ordered by ascending
    ///   score (better match = smaller number).
    /// - Without `q.text`: filter-only query ordered by
    ///   `time_observed DESC, event_id DESC`.
    ///
    /// Redacted and archived events are excluded by default; set
    /// `include_redacted` to include hidden rows (e.g., for a `doctor` audit).
    pub fn search(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, IndexError> {
        // Empty or whitespace-only text queries would hit FTS5 with an
        // empty MATCH, which is a syntax error. Treat them as "no text
        // filter" and fall through to the pure-filter path.
        let effective_text = q
            .text
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let limit: i64 = if q.limit == 0 { 20 } else { q.limit as i64 };

        let mut binds: Vec<Box<dyn ToSql>> = Vec::new();
        let mut predicates: Vec<String> = Vec::new();

        if !q.include_redacted {
            predicates.push("e.is_redacted = 0".into());
            // Exclude any event targeted by an active Archive (direct hit).
            predicates.push(
                "NOT EXISTS (SELECT 1 FROM entity_archive ea WHERE ea.target_entity = e.event_id AND ea.direction = 'archive')".into()
            );
            // Also exclude chain tips whose claim_current row carries
            // is_archived=1. This catches the archive-then-supersede case
            // (Codex P1 from this session): Archive targets claim A;
            // Claim B supersedes A; B is the new tip with is_archived=1,
            // but B is NOT in entity_archive. Without this clause, search
            // would surface B while current_claim says is_archived=true —
            // a projection inconsistency.
            predicates.push(
                "NOT EXISTS (SELECT 1 FROM claim_current cc WHERE cc.event_id = e.event_id AND cc.is_archived = 1)".into()
            );
        }
        if let Some(types) = &q.event_types {
            if !types.is_empty() {
                let placeholders = vec!["?"; types.len()].join(",");
                predicates.push(format!("e.event_type IN ({placeholders})"));
                for et in types {
                    binds.push(Box::new(event_type_to_str(et)));
                }
            }
        }
        if let Some(a) = &q.actor_id {
            predicates.push("e.actor_id = ?".into());
            binds.push(Box::new(a.clone()));
        }
        if let Some(s) = q.since {
            predicates.push("e.time_observed >= ?".into());
            binds.push(Box::new(s.timestamp_millis()));
        }
        if let Some(u) = q.until {
            predicates.push("e.time_observed <= ?".into());
            binds.push(Box::new(u.timestamp_millis()));
        }
        if let Some(s) = q.known_since {
            predicates.push("e.time_recorded >= ?".into());
            binds.push(Box::new(s.timestamp_millis()));
        }
        if let Some(u) = q.known_until {
            predicates.push("e.time_recorded <= ?".into());
            binds.push(Box::new(u.timestamp_millis()));
        }

        let where_clause = if predicates.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", predicates.join(" AND "))
        };

        let (sql, all_binds) = match &effective_text {
            Some(raw) => {
                let match_expr = escape_fts(raw);
                // MATCH must be bound first in the WHERE to let SQLite see
                // it as the FTS5 virtual-table constraint.
                let mut with_match: Vec<Box<dyn ToSql>> = Vec::with_capacity(binds.len() + 2);
                with_match.push(Box::new(match_expr));
                with_match.extend(binds);
                with_match.push(Box::new(limit));
                let where_match = if predicates.is_empty() {
                    "WHERE events_fts MATCH ?".to_string()
                } else {
                    format!("WHERE events_fts MATCH ? AND {}", predicates.join(" AND "))
                };
                let sql = format!(
                    r#"SELECT e.event_id, e.commit_oid, e.event_type, e.time_observed,
                              bm25(events_fts, 10.0, 1.0, 2.0) AS score,
                              snippet(events_fts, 1, '[', ']', '…', 10) AS snip
                         FROM events_fts
                         JOIN events e ON e.event_id = events_fts.event_id
                         {where_match}
                       ORDER BY score, e.time_observed DESC, e.event_id DESC
                        LIMIT ?"#
                );
                (sql, with_match)
            }
            None => {
                let mut with_limit = binds;
                with_limit.push(Box::new(limit));
                let sql = format!(
                    r#"SELECT e.event_id, e.commit_oid, e.event_type, e.time_observed,
                              0.0 AS score, NULL AS snip
                         FROM events e
                         {where_clause}
                       ORDER BY e.time_observed DESC, e.event_id DESC
                        LIMIT ?"#
                );
                (sql, with_limit)
            }
        };

        // FTS5 MATCH parse errors (malformed operators like `foo(bar`,
        // unbalanced parens, bad column filters) surface as a sqlite
        // SqliteFailure with message "fts5: syntax error near …".
        // Treat them as "no matches" rather than bubbling a 500 up to
        // MCP/TUI callers — an invalid search query should feel like
        // an empty result, not a crash. Every OTHER sqlite error
        // propagates normally.
        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) if is_fts5_parse_error(&e) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let rows = match stmt.query_map(
            params_from_iter(all_binds.iter().map(|b| b.as_ref())),
            |row| {
                let event_id_s: String = row.get(0)?;
                let commit_oid: String = row.get(1)?;
                let event_type_s: String = row.get(2)?;
                let time_ms: i64 = row.get(3)?;
                let score: f64 = row.get(4)?;
                let snip: Option<String> = row.get(5)?;
                Ok((event_id_s, commit_oid, event_type_s, time_ms, score, snip))
            },
        ) {
            Ok(r) => r,
            Err(e) if is_fts5_parse_error(&e) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        let mut hits = Vec::new();
        for row in rows {
            let (event_id_s, commit_oid, event_type_s, time_ms, score, snippet) = match row {
                Ok(r) => r,
                Err(e) if is_fts5_parse_error(&e) => return Ok(Vec::new()),
                Err(e) => return Err(e.into()),
            };
            let Ok(event_id) = Uuid::parse_str(&event_id_s) else {
                warn!(event_id = %event_id_s, "invalid uuid in events row; skipping");
                continue;
            };
            let Some(event_type) = event_type_from_str(&event_type_s) else {
                warn!(event_type = %event_type_s, "unknown event_type in events row; skipping");
                continue;
            };
            let Some(time_observed) = DateTime::<Utc>::from_timestamp_millis(time_ms) else {
                warn!(time_ms, "bad timestamp in events row; skipping");
                continue;
            };
            hits.push(SearchHit {
                event_id,
                commit_oid,
                event_type,
                time_observed,
                score,
                snippet,
            });
        }
        Ok(hits)
    }

    /// Current-truth preference for `(category, key)`. Returns None if
    /// the underlying event has been archived — Archive is a visibility
    /// signal, so the "current truth" API honors it just like `search()`
    /// does. Callers who need to inspect archived state use a different
    /// path (index internals + `is_archived()`).
    pub fn current_pref(
        &self,
        category: &str,
        key: &str,
    ) -> Result<Option<PrefProjection>, IndexError> {
        let mut stmt = self.conn.prepare_cached(
            r#"SELECT p.category, p.key, p.value_json, p.event_id, p.updated_at
                 FROM pref_current p
                WHERE p.category = ?1 AND p.key = ?2
                  AND NOT EXISTS (
                      SELECT 1 FROM entity_archive ea
                       WHERE ea.target_entity = p.event_id
                         AND ea.direction = 'archive'
                  )"#,
        )?;
        let mut rows = stmt.query([category, key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let value_json: String = row.get(2)?;
        let event_id_s: String = row.get(3)?;
        let updated_ms: i64 = row.get(4)?;
        Ok(Some(PrefProjection {
            category: row.get(0)?,
            key: row.get(1)?,
            value: serde_json::from_str(&value_json)?,
            event_id: Uuid::parse_str(&event_id_s).map_err(|e| IndexError::Invariant {
                detail: format!("bad event_id in pref_current: {e}"),
            })?,
            updated_at: DateTime::<Utc>::from_timestamp_millis(updated_ms).ok_or_else(|| {
                IndexError::Invariant {
                    detail: "bad updated_at in pref_current".into(),
                }
            })?,
        }))
    }

    /// Current-truth claim for `(schema_type, key)`. Returns the chain
    /// tip annotated with the most recent verification verdict — only
    /// if NOT archived. `is_archived=1` hides the row the same way
    /// `search()` does, so default reads never surface archived claims.
    pub fn current_claim(
        &self,
        schema_type: &str,
        key: &str,
    ) -> Result<Option<ClaimProjection>, IndexError> {
        let mut stmt = self.conn.prepare_cached(
            r#"SELECT schema_type, key, event_id, chain_id, content_json,
                      supersedes_event_id, verdict, verdict_event_id, verdict_at,
                      is_archived, updated_at
                 FROM claim_current
                WHERE schema_type = ?1 AND key = ?2 AND is_archived = 0"#,
        )?;
        let mut rows = stmt.query([schema_type, key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };

        let event_id_s: String = row.get(2)?;
        let chain_id_s: String = row.get(3)?;
        let content_json: String = row.get(4)?;
        let supersedes_s: Option<String> = row.get(5)?;
        let verdict: Option<String> = row.get(6)?;
        let verdict_event_s: Option<String> = row.get(7)?;
        let verdict_at_ms: Option<i64> = row.get(8)?;
        let is_archived: i64 = row.get(9)?;
        let updated_ms: i64 = row.get(10)?;

        let bad_id = |e: uuid::Error| IndexError::Invariant {
            detail: format!("bad uuid in claim_current: {e}"),
        };

        Ok(Some(ClaimProjection {
            schema_type: row.get(0)?,
            key: row.get(1)?,
            event_id: Uuid::parse_str(&event_id_s).map_err(bad_id)?,
            chain_id: chain_id_s,
            content: serde_json::from_str(&content_json)?,
            supersedes_event_id: match supersedes_s {
                Some(s) => Some(Uuid::parse_str(&s).map_err(bad_id)?),
                None => None,
            },
            verdict,
            verdict_event_id: match verdict_event_s {
                Some(s) => Some(Uuid::parse_str(&s).map_err(bad_id)?),
                None => None,
            },
            verdict_at: match verdict_at_ms {
                Some(ms) => DateTime::<Utc>::from_timestamp_millis(ms),
                None => None,
            },
            is_archived: is_archived != 0,
            updated_at: DateTime::<Utc>::from_timestamp_millis(updated_ms).ok_or_else(|| {
                IndexError::Invariant {
                    detail: "bad updated_at in claim_current".into(),
                }
            })?,
        }))
    }

    /// Has this entity been archived?
    pub fn is_archived(&self, target_entity: Uuid) -> Result<bool, IndexError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT direction FROM entity_archive WHERE target_entity = ?1")?;
        let mut rows = stmt.query([target_entity.to_string()])?;
        match rows.next()? {
            Some(row) => {
                let dir: String = row.get(0)?;
                Ok(dir == "archive")
            }
            None => Ok(false),
        }
    }
}

fn event_type_to_str(et: &EventType) -> &'static str {
    match et {
        EventType::Observe => "observe",
        EventType::Claim => "claim",
        EventType::Lesson => "lesson",
        EventType::Pref => "pref",
        EventType::SkillEdit => "skill_edit",
        EventType::Verify => "verify",
        EventType::Archive => "archive",
        EventType::Redact => "redact",
        EventType::Import => "import",
        EventType::Audit => "audit",
    }
}

fn event_type_from_str(s: &str) -> Option<EventType> {
    match s {
        "observe" => Some(EventType::Observe),
        "claim" => Some(EventType::Claim),
        "lesson" => Some(EventType::Lesson),
        "pref" => Some(EventType::Pref),
        "skill_edit" => Some(EventType::SkillEdit),
        "verify" => Some(EventType::Verify),
        "archive" => Some(EventType::Archive),
        "redact" => Some(EventType::Redact),
        "import" => Some(EventType::Import),
        "audit" => Some(EventType::Audit),
        _ => None,
    }
}

/// Detect FTS5-specific parse errors so the search path can translate
/// them to "no matches" instead of bubbling up. Matches on the SQLite
/// error message since FTS5 doesn't expose a distinct error code.
fn is_fts5_parse_error(err: &rusqlite::Error) -> bool {
    let msg = err.to_string();
    msg.contains("fts5: syntax error")
        || msg.contains("fts5: parse error")
        || msg.contains("malformed MATCH expression")
}

/// Build the FTS5 MATCH expression for a user query.
///
/// Default behavior: treat every bare word as a **prefix** match. A user
/// typing `fast` finds `fastapi-users` without having to know FTS5
/// syntax. Hyphens / dots / slashes act as word boundaries (matching
/// what the `unicode61` tokenizer already does at index time), so
/// `fastapi-users` becomes `fastapi* users*` and hits either token.
///
/// Power-user passthrough: if the query contains any explicit FTS5
/// operator (`*`, `"`, `(`, `)`, `:`) or a standalone AND/OR/NOT, we
/// respect the user's intent and only double quotes to keep phrase
/// parsing sound. `auth*` stays `auth*` (no double-suffix), `"exact
/// phrase"` stays a phrase query, `title:foo` is a column filter.
fn escape_fts(raw: &str) -> String {
    let has_explicit_operator = raw
        .chars()
        .any(|c| matches!(c, '*' | '"' | '(' | ')' | ':'))
        || raw.split_whitespace().any(|w| {
            w.eq_ignore_ascii_case("AND")
                || w.eq_ignore_ascii_case("OR")
                || w.eq_ignore_ascii_case("NOT")
        });
    if has_explicit_operator {
        // Balanced quotes → legitimate phrase query, pass through as-is.
        // Odd quote count → stray `"`, double it so FTS5 doesn't see an
        // unterminated phrase.
        let quote_count = raw.chars().filter(|&c| c == '"').count();
        if quote_count % 2 == 0 {
            return raw.to_string();
        }
        return raw.replace('"', "\"\"");
    }
    // Split on tokenizer boundaries (anything non-alphanumeric + not `_`)
    // and append `*` to each token so bare words become prefix queries.
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in raw.chars() {
        if c.is_alphanumeric() || c == '_' {
            current.push(c);
        } else if !current.is_empty() {
            tokens.push(format!("{current}*"));
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.push(format!("{current}*"));
    }
    tokens.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use tempfile::TempDir;

    // Direct inserts into events + events_fts — bypasses the Phase 2 ingest
    // path so these tests stay self-contained and fast.
    #[allow(clippy::too_many_arguments)]
    fn insert_raw(
        idx: &BrainIndex,
        event_id: &str,
        event_type: &str,
        actor_id: &str,
        time_ms: i64,
        title: &str,
        body: &str,
        tags: &str,
        is_redacted: i64,
    ) {
        let conn = idx.connection();
        conn.execute(
            r#"INSERT INTO events(
                 event_id, commit_oid, event_type,
                 subject_kind, subject_id, chain_id, parent_event_id,
                 actor_kind, actor_id, actor_harness,
                 layer, authority_source_kind, authority_score, authority_attested_by,
                 signature_state, classification,
                 time_observed, time_recorded, idempotency_key, is_redacted, payload_json
               ) VALUES (?1,'cmt',?2,'entity',?3,NULL,NULL,'agent',?4,NULL,'episodic','agent',NULL,NULL,'unsigned','private',?5,?5,NULL,?6,'{}')"#,
            params![event_id, event_type, event_id, actor_id, time_ms, is_redacted],
        ).unwrap();
        if is_redacted == 0 {
            conn.execute(
                "INSERT INTO events_fts(event_id, title, body, tags) VALUES (?1,?2,?3,?4)",
                params![event_id, title, body, tags],
            )
            .unwrap();
        }
    }

    fn new_index() -> (TempDir, BrainIndex) {
        let td = TempDir::new().unwrap();
        let idx = BrainIndex::open(td.path().join("i.sqlite")).unwrap();
        (td, idx)
    }

    /// Like `insert_raw` but lets event-time and transaction-time differ,
    /// for bitemporal filter tests.
    fn insert_raw_bitemporal(
        idx: &BrainIndex,
        event_id: &str,
        time_observed_ms: i64,
        time_recorded_ms: i64,
    ) {
        let conn = idx.connection();
        conn.execute(
            r#"INSERT INTO events(
                 event_id, commit_oid, event_type,
                 subject_kind, subject_id, chain_id, parent_event_id,
                 actor_kind, actor_id, actor_harness,
                 layer, authority_source_kind, authority_score, authority_attested_by,
                 signature_state, classification,
                 time_observed, time_recorded, idempotency_key, is_redacted, payload_json
               ) VALUES (?1,'cmt','observe','entity',?1,NULL,NULL,'agent','u',NULL,
                         'episodic','agent',NULL,NULL,'unsigned','private',?2,?3,NULL,0,'{}')"#,
            params![event_id, time_observed_ms, time_recorded_ms],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events_fts(event_id, title, body, tags) VALUES (?1,'x','','')",
            params![event_id],
        )
        .unwrap();
    }

    #[test]
    fn search_empty_index_returns_empty() {
        let (_td, idx) = new_index();
        let hits = idx.search(&SearchQuery::text("anything")).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn text_search_matches_title() {
        let (_td, idx) = new_index();
        let id = Uuid::now_v7().to_string();
        insert_raw(
            &idx,
            &id,
            "observe",
            "alice",
            1000,
            "fastapi users",
            "",
            "",
            0,
        );
        let hits = idx.search(&SearchQuery::text("fastapi")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].commit_oid, "cmt");
    }

    #[test]
    fn text_search_prefix_matches_with_star() {
        let (_td, idx) = new_index();
        let id = Uuid::now_v7().to_string();
        insert_raw(
            &idx,
            &id,
            "observe",
            "a",
            1000,
            "authentication strategy",
            "",
            "",
            0,
        );
        let hits = idx.search(&SearchQuery::text("auth*")).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "prefix index should match auth* to authentication"
        );
    }

    #[test]
    fn text_search_title_match_returns_hit() {
        // Sanity: title + body are both indexed and queryable. The exact
        // BM25 ordering with column weights depends on corpus size and
        // average-document-length signals that don't stabilize at N=2,
        // so we don't assert strict order here — the weights are a known
        // SQLite FTS5 feature, not something we own.
        let (_td, idx) = new_index();
        let a = Uuid::now_v7().to_string();
        let b = Uuid::now_v7().to_string();
        insert_raw(
            &idx,
            &a,
            "observe",
            "a",
            1000,
            "noise",
            "query term here",
            "",
            0,
        );
        insert_raw(
            &idx,
            &b,
            "observe",
            "a",
            1001,
            "query term here",
            "noise",
            "",
            0,
        );
        let hits = idx.search(&SearchQuery::text("query")).unwrap();
        assert_eq!(hits.len(), 2, "both title- and body-matches must surface");
    }

    #[test]
    fn filter_by_event_type() {
        let (_td, idx) = new_index();
        let a = Uuid::now_v7().to_string();
        let b = Uuid::now_v7().to_string();
        insert_raw(&idx, &a, "observe", "u", 1000, "x", "", "", 0);
        insert_raw(&idx, &b, "claim", "u", 1001, "x", "", "", 0);
        let q = SearchQuery {
            event_types: Some(vec![EventType::Observe]),
            limit: 10,
            ..Default::default()
        };
        let hits = idx.search(&q).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(matches!(hits[0].event_type, EventType::Observe));
    }

    #[test]
    fn filter_by_actor_id() {
        let (_td, idx) = new_index();
        let a = Uuid::now_v7().to_string();
        let b = Uuid::now_v7().to_string();
        insert_raw(&idx, &a, "observe", "alice", 1000, "x", "", "", 0);
        insert_raw(&idx, &b, "observe", "bob", 1001, "x", "", "", 0);
        let q = SearchQuery {
            actor_id: Some("alice".into()),
            limit: 10,
            ..Default::default()
        };
        let hits = idx.search(&q).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn filter_by_time_range() {
        let (_td, idx) = new_index();
        for t in [1000i64, 2000, 3000] {
            let id = Uuid::now_v7().to_string();
            insert_raw(&idx, &id, "observe", "u", t, "x", "", "", 0);
        }
        let q = SearchQuery {
            since: Some(DateTime::<Utc>::from_timestamp_millis(2000).unwrap()),
            limit: 10,
            ..Default::default()
        };
        let hits = idx.search(&q).unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn archived_events_excluded_by_default() {
        // Codex P1: search never consulted entity_archive. Archived
        // claims must be hidden by default; visible with include_redacted.
        let (_td, idx) = new_index();
        let visible = Uuid::now_v7().to_string();
        let archived = Uuid::now_v7().to_string();
        insert_raw(&idx, &visible, "claim", "u", 1000, "open plan", "", "", 0);
        insert_raw(
            &idx,
            &archived,
            "claim",
            "u",
            1001,
            "archived plan",
            "",
            "",
            0,
        );
        // Archive only the second one.
        idx.connection().execute(
            "INSERT INTO entity_archive(target_entity,direction,updated_at) VALUES(?1,'archive',2000)",
            params![archived],
        ).unwrap();

        let default_hits = idx
            .search(&SearchQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(default_hits.len(), 1, "archived claim must be hidden");
        assert_eq!(default_hits[0].event_id.to_string(), visible);

        let all_hits = idx
            .search(&SearchQuery {
                include_redacted: true,
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(all_hits.len(), 2, "include_redacted shows archived");
    }

    #[test]
    fn archived_chain_tip_hidden_from_search_even_after_supersede() {
        // Codex P1 round 3: Archive targets claim A; Claim B with
        // supersedes=A becomes the new chain tip inheriting is_archived=1.
        // entity_archive only has A. Without the extra claim_current
        // filter, search would surface B while current_claim hides it.
        let (_td, idx) = new_index();
        // Two rows in events/events_fts (chain tip + prior).
        let a = Uuid::now_v7().to_string();
        let b = Uuid::now_v7().to_string();
        insert_raw(&idx, &a, "claim", "u", 1000, "old tip", "", "", 0);
        insert_raw(&idx, &b, "claim", "u", 1001, "new tip", "", "", 0);
        // entity_archive points at A only.
        idx.connection().execute(
            "INSERT INTO entity_archive(target_entity,direction,updated_at) VALUES(?1,'archive',1)",
            params![a],
        ).unwrap();
        // claim_current carries is_archived=1 on B (supersession preserves).
        idx.connection()
            .execute(
                r#"INSERT INTO claim_current(schema_type,key,event_id,chain_id,content_json,
                supersedes_event_id,verdict,verdict_event_id,verdict_at,is_archived,updated_at)
               VALUES('s','k',?1,?1,'"v"',?2,NULL,NULL,NULL,1,1001)"#,
                params![b, a],
            )
            .unwrap();

        let hits = idx
            .search(&SearchQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        let ids: std::collections::HashSet<_> =
            hits.iter().map(|h| h.event_id.to_string()).collect();
        assert!(!ids.contains(&a), "archived original must be hidden");
        assert!(
            !ids.contains(&b),
            "superseding chain tip with is_archived=1 must also be hidden"
        );
    }

    #[test]
    fn current_pref_hides_archived_target() {
        // Codex P1 round 3: Archive on a Pref was a no-op on current_pref.
        let (_td, idx) = new_index();
        let pref_event = Uuid::now_v7().to_string();
        idx.connection()
            .execute(
                "INSERT INTO pref_current(category,key,value_json,event_id,updated_at)
             VALUES('c','k','\"v\"',?1,1)",
                params![pref_event],
            )
            .unwrap();
        assert!(idx.current_pref("c", "k").unwrap().is_some());
        idx.connection().execute(
            "INSERT INTO entity_archive(target_entity,direction,updated_at) VALUES(?1,'archive',2)",
            params![pref_event],
        ).unwrap();
        assert!(
            idx.current_pref("c", "k").unwrap().is_none(),
            "archived pref must be hidden from current_pref"
        );
    }

    #[test]
    fn current_claim_hides_archived_chain_tip() {
        // current_claim should return None when is_archived=1.
        let (_td, idx) = new_index();
        let claim_event = Uuid::now_v7().to_string();
        idx.connection()
            .execute(
                r#"INSERT INTO claim_current(schema_type,key,event_id,chain_id,content_json,
                supersedes_event_id,verdict,verdict_event_id,verdict_at,is_archived,updated_at)
               VALUES('s','k',?1,?1,'"v"',NULL,NULL,NULL,NULL,1,1)"#,
                params![claim_event],
            )
            .unwrap();
        assert!(
            idx.current_claim("s", "k").unwrap().is_none(),
            "archived claim must be hidden from current_claim"
        );
    }

    #[test]
    fn unarchive_re_exposes_event() {
        // An Archive followed by an Unarchive (same target) must make
        // the event visible again in default search.
        let (_td, idx) = new_index();
        let id = Uuid::now_v7().to_string();
        insert_raw(&idx, &id, "claim", "u", 1000, "plan", "", "", 0);
        idx.connection().execute(
            "INSERT INTO entity_archive(target_entity,direction,updated_at) VALUES(?1,'archive',2000)",
            params![id],
        ).unwrap();
        assert_eq!(
            idx.search(&SearchQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap()
            .len(),
            0
        );
        idx.connection().execute(
            "INSERT OR REPLACE INTO entity_archive(target_entity,direction,updated_at) VALUES(?1,'unarchive',3000)",
            params![id],
        ).unwrap();
        assert_eq!(
            idx.search(&SearchQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn empty_text_query_returns_no_results_not_error() {
        // Codex P2: MATCH '' is an FTS5 syntax error. An empty query
        // string falls through to pure-filter semantics; if there are
        // no other filters and no matching rows, we get an empty list.
        let (_td, idx) = new_index();
        assert!(idx.search(&SearchQuery::text("")).unwrap().is_empty());
        assert!(idx.search(&SearchQuery::text("   ")).unwrap().is_empty());
    }

    #[test]
    fn redacted_excluded_by_default_but_shown_with_flag() {
        let (_td, idx) = new_index();
        let a = Uuid::now_v7().to_string();
        let b = Uuid::now_v7().to_string();
        insert_raw(&idx, &a, "observe", "u", 1000, "clean", "", "", 0);
        insert_raw(&idx, &b, "observe", "u", 1001, "dirty", "", "", 1);

        let default_hits = idx
            .search(&SearchQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(default_hits.len(), 1);

        let with_red = idx
            .search(&SearchQuery {
                include_redacted: true,
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(with_red.len(), 2);
    }

    #[test]
    fn pure_filter_sorts_newest_first() {
        let (_td, idx) = new_index();
        for t in [1000i64, 2000, 3000] {
            let id = Uuid::now_v7().to_string();
            insert_raw(&idx, &id, "observe", "u", t, "x", "", "", 0);
        }
        let hits = idx
            .search(&SearchQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hits.len(), 3);
        assert!(hits[0].time_observed > hits[1].time_observed);
        assert!(hits[1].time_observed > hits[2].time_observed);
    }

    #[test]
    fn current_pref_returns_none_when_absent() {
        let (_td, idx) = new_index();
        assert!(idx.current_pref("cat", "k").unwrap().is_none());
    }

    #[test]
    fn current_pref_round_trip() {
        let (_td, idx) = new_index();
        idx.connection().execute(
            "INSERT INTO pref_current(category, key, value_json, event_id, updated_at)
             VALUES('auth','provider','\"fastapi-users\"','019daf10-0000-7000-8000-000000000001',9999)",
            [],
        ).unwrap();
        let p = idx.current_pref("auth", "provider").unwrap().unwrap();
        assert_eq!(p.category, "auth");
        assert_eq!(p.value, serde_json::json!("fastapi-users"));
    }

    #[test]
    fn current_claim_round_trip() {
        let (_td, idx) = new_index();
        idx.connection()
            .execute(
                "INSERT INTO claim_current(schema_type,key,event_id,chain_id,content_json,
              supersedes_event_id,verdict,verdict_event_id,verdict_at,is_archived,updated_at)
             VALUES('fact','sky',
               '019daf10-0000-7000-8000-000000000001',
               '019daf10-0000-7000-8000-000000000001',
               '\"blue\"', NULL, NULL, NULL, NULL, 0, 5000)",
                [],
            )
            .unwrap();
        let c = idx.current_claim("fact", "sky").unwrap().unwrap();
        assert_eq!(c.schema_type, "fact");
        assert_eq!(c.content, serde_json::json!("blue"));
        assert!(c.verdict.is_none());
        assert!(!c.is_archived);
    }

    #[test]
    fn is_archived_defaults_false_and_flips_true() {
        let (_td, idx) = new_index();
        let target = Uuid::now_v7();
        assert!(!idx.is_archived(target).unwrap());
        idx.connection().execute(
            "INSERT INTO entity_archive(target_entity,direction,updated_at) VALUES(?1,'archive',1)",
            params![target.to_string()],
        ).unwrap();
        assert!(idx.is_archived(target).unwrap());
    }

    #[test]
    fn escape_fts_balanced_quotes_pass_through() {
        // Balanced quotes = legitimate phrase query. Old behavior doubled
        // them always, which turned `a"b"c` into `a""b""c` (meaningless
        // inside FTS5). Now even quote counts pass through untouched so
        // phrase queries work as users expect.
        assert_eq!(escape_fts(r#"a"b"c"#), r#"a"b"c"#);
    }

    #[test]
    fn escape_fts_odd_quotes_are_doubled() {
        // An unbalanced `"` would leave FTS5's phrase parser dangling.
        // Doubling it escapes to a literal `"` token and keeps parsing
        // safe. This path trades one pathological UX (typo'd phrase)
        // for never crashing the index.
        assert_eq!(escape_fts(r#"a"b"#), r#"a""b"#);
    }

    #[test]
    fn escape_fts_bare_word_becomes_prefix() {
        // Regression guard: typing `fast` must match `fastapi-users`.
        // Without auto-prefixing, bare FTS5 terms are exact-token match.
        assert_eq!(escape_fts("fast"), "fast*");
        assert_eq!(escape_fts("tokio"), "tokio*");
    }

    #[test]
    fn escape_fts_multiple_words_all_prefix_match() {
        // Implicit AND — all prefixes must match (in any order, any col).
        assert_eq!(escape_fts("fast auth"), "fast* auth*");
    }

    #[test]
    fn escape_fts_splits_on_hyphens_and_punctuation() {
        // Hyphens, slashes, dots are tokenizer boundaries in unicode61,
        // so we split the user's query the same way — otherwise
        // `fastapi-users*` would try to match a single token that
        // doesn't exist in the index (the indexed tokens are `fastapi`
        // and `users` separately).
        assert_eq!(escape_fts("fastapi-users"), "fastapi* users*");
        assert_eq!(escape_fts("a.b.c"), "a* b* c*");
    }

    #[test]
    fn escape_fts_respects_explicit_wildcard() {
        // User already provided a star — don't double-star to avoid
        // turning `auth*` into `auth**` which FTS5 rejects.
        assert_eq!(escape_fts("auth*"), "auth*");
    }

    #[test]
    fn escape_fts_respects_column_filter_and_quotes() {
        assert_eq!(escape_fts("title:fast"), "title:fast");
        assert_eq!(escape_fts(r#""exact phrase""#), r#""exact phrase""#);
    }

    #[test]
    fn escape_fts_respects_boolean_operators() {
        assert_eq!(escape_fts("fast AND auth"), "fast AND auth");
        assert_eq!(escape_fts("fast OR slow"), "fast OR slow");
        assert_eq!(escape_fts("fast and auth"), "fast and auth");
        assert_eq!(escape_fts("fast or slow"), "fast or slow");
        assert_eq!(escape_fts("fast not slow"), "fast not slow");
    }

    #[test]
    fn escape_fts_empty_and_whitespace_are_empty() {
        assert_eq!(escape_fts(""), "");
        assert_eq!(escape_fts("   "), "");
    }

    #[test]
    fn filter_only_search_tie_breaks_by_event_id_desc() {
        let (_td, idx) = new_index();
        let low = "00000000-0000-7000-8000-000000000001";
        let high = "00000000-0000-7000-8000-000000000002";
        insert_raw(&idx, low, "observe", "actor", 1_000, "same", "", "", 0);
        insert_raw(&idx, high, "observe", "actor", 1_000, "same", "", "", 0);

        let hits = idx
            .search(&SearchQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].event_id.to_string(), high);
        assert_eq!(hits[1].event_id.to_string(), low);
    }

    #[test]
    fn text_search_tie_breaks_by_time_then_event_id() {
        let (_td, idx) = new_index();
        let older = "00000000-0000-7000-8000-000000000001";
        let newer_low = "00000000-0000-7000-8000-000000000002";
        let newer_high = "00000000-0000-7000-8000-000000000003";
        insert_raw(&idx, older, "observe", "actor", 1_000, "alpha", "", "", 0);
        insert_raw(
            &idx, newer_low, "observe", "actor", 2_000, "alpha", "", "", 0,
        );
        insert_raw(
            &idx, newer_high, "observe", "actor", 2_000, "alpha", "", "", 0,
        );

        let hits = idx
            .search(&SearchQuery {
                text: Some("alpha".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();

        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].event_id.to_string(), newer_high);
        assert_eq!(hits[1].event_id.to_string(), newer_low);
        assert_eq!(hits[2].event_id.to_string(), older);
    }

    #[test]
    fn filter_by_known_since_on_time_recorded() {
        // known_since filters on time_recorded (transaction-time) —
        // NOT time_observed. Events observed at the same real-world
        // instant but recorded at different times are filtered
        // independently.
        let (_td, idx) = new_index();
        for (i, t_recorded) in [("a", 1_000i64), ("b", 2_000), ("c", 3_000)] {
            let id = Uuid::now_v7().to_string();
            // All happened at time_observed=500 (same event-time) but
            // were learned at different time_recorded.
            let _ = i;
            insert_raw_bitemporal(&idx, &id, 500, t_recorded);
        }
        let q = SearchQuery {
            known_since: Some(DateTime::<Utc>::from_timestamp_millis(2_000).unwrap()),
            limit: 10,
            ..Default::default()
        };
        let hits = idx.search(&q).unwrap();
        assert_eq!(
            hits.len(),
            2,
            "known_since = 2000ms must include rows recorded at 2000 and 3000"
        );
    }

    #[test]
    fn filter_by_known_until_bounds_transaction_time() {
        // known_until filters `time_recorded <= T`. This is NOT a true
        // "as known at T" bitemporal read — archive/redact state still
        // reflects the current moment. See SearchQuery doc comment.
        let (_td, idx) = new_index();
        for t_recorded in [1_000i64, 2_000, 3_000] {
            let id = Uuid::now_v7().to_string();
            insert_raw_bitemporal(&idx, &id, 500, t_recorded);
        }
        let q = SearchQuery {
            known_until: Some(DateTime::<Utc>::from_timestamp_millis(2_000).unwrap()),
            limit: 10,
            ..Default::default()
        };
        let hits = idx.search(&q).unwrap();
        assert_eq!(
            hits.len(),
            2,
            "known_until = 2000ms must include rows recorded at 1000 and 2000"
        );
    }

    #[test]
    fn bitemporal_filter_combines_event_time_and_transaction_time() {
        // Verify `since` (event-time lower) + `known_until` (transaction-
        // time upper) compose correctly: events whose time_observed is
        // after X AND whose time_recorded is at-or-before Y. Archive/
        // redact state is still evaluated at the current moment, not Y.
        let (_td, idx) = new_index();
        // Matrix: (time_observed, time_recorded)
        //   A: (100, 500)   — too old, drop via `since`
        //   B: (1000, 500)  — in range on both axes: KEEP
        //   C: (1000, 9000) — recorded too late, drop via `known_until`
        insert_raw_bitemporal(&idx, &Uuid::now_v7().to_string(), 100, 500);
        let keep_id = Uuid::now_v7().to_string();
        insert_raw_bitemporal(&idx, &keep_id, 1_000, 500);
        insert_raw_bitemporal(&idx, &Uuid::now_v7().to_string(), 1_000, 9_000);

        let q = SearchQuery {
            since: Some(DateTime::<Utc>::from_timestamp_millis(500).unwrap()),
            known_until: Some(DateTime::<Utc>::from_timestamp_millis(1_000).unwrap()),
            limit: 10,
            ..Default::default()
        };
        let hits = idx.search(&q).unwrap();
        assert_eq!(hits.len(), 1, "only B is in range on both axes");
        assert_eq!(hits[0].event_id.to_string(), keep_id);
    }
}
