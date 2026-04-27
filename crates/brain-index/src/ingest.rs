//! Event → rows + projection updates. One transaction per `ingest`.
//!
//! Wire diagram per payload variant:
//!
//!   Observe / Lesson / SkillEdit / Import / Audit  →  events + events_fts
//!   Pref                                           →  + pref_current upsert
//!   Claim                                          →  + claim_current upsert (chain-aware)
//!   Verify                                         →  + annotate claim_current.verdict
//!   Archive                                        →  + entity_archive + flag claim_current
//!   Redact                                         →  flip events.is_redacted, drop from fts
//!
//! Idempotent: re-ingesting the same `event_id` overwrites the same rows.
//! This is load-bearing for crash-recovery catch-up after the process
//! dies between the git commit and the index write.

use brain_types::{Event, EventPayload};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use tracing::debug;

use crate::BrainIndex;
use crate::error::IndexError;

type ClaimPredecessor = (String, String, Option<String>, Option<String>, i64);

impl BrainIndex {
    /// Ingest a single committed event into the index. All writes happen
    /// in one IMMEDIATE transaction so a failure leaves no torn state.
    pub fn ingest(&mut self, event: &Event) -> Result<(), IndexError> {
        if event.commit_oid.is_empty() {
            return Err(IndexError::Invariant {
                detail: "ingest called with empty commit_oid — caller must enrich from git".into(),
            });
        }

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        // events row
        let (subject_kind, subject_id) = match &event.subject {
            brain_types::SubjectRef::Entity { id } => ("entity", Some(id.to_string())),
            brain_types::SubjectRef::Batch { id } => ("batch", Some(id.to_string())),
            brain_types::SubjectRef::Brain => ("brain", None),
        };

        let event_type = snake_case(&event.event_type)?;
        let actor_kind = snake_case(&event.actor.kind)?;
        let layer = snake_case(&event.layer)?;
        let authority_source_kind = snake_case(&event.authority.source_kind)?;
        let signature_state = snake_case(&event.signature_state)?;
        let classification = snake_case(&event.classification)?;
        let payload_json = serde_json::to_string(&event.payload)?;
        let event_id_str = event.event_id.to_string();

        tx.execute(
            r#"
            INSERT OR REPLACE INTO events(
                event_id, commit_oid, event_type,
                subject_kind, subject_id, chain_id, parent_event_id,
                actor_kind, actor_id, actor_harness,
                layer,
                authority_source_kind, authority_score, authority_attested_by,
                signature_state, classification,
                time_observed, time_recorded,
                idempotency_key, is_redacted, payload_json
            ) VALUES (?,?,?, ?,?,?,?, ?,?,?, ?, ?,?,?, ?,?, ?,?, ?, 0, ?)
            "#,
            params![
                event_id_str,
                event.commit_oid,
                event_type,
                subject_kind,
                subject_id,
                event.chain_id.map(|u| u.to_string()),
                event.parent_event_id.map(|u| u.to_string()),
                actor_kind,
                event.actor.id,
                event.actor.harness,
                layer,
                authority_source_kind,
                event.authority.score.map(|s| s as i64),
                event.authority.attested_by,
                signature_state,
                classification,
                event.time_observed.timestamp_millis(),
                event.time_recorded.timestamp_millis(),
                event.idempotency_key,
                payload_json,
            ],
        )?;

        // events_fts row. Delete-then-insert so re-ingest is clean (FTS5 has
        // no native UPSERT for contentless external tables).
        tx.execute(
            "DELETE FROM events_fts WHERE event_id = ?1",
            params![event_id_str],
        )?;
        let (title, body, extra_tags) = extract_fts_fields(&event.payload)?;
        let tags = format!(
            "{} {} {} {}",
            event.actor.id, layer, classification, extra_tags
        )
        .trim()
        .to_string();
        tx.execute(
            "INSERT INTO events_fts(event_id, title, body, tags) VALUES (?1, ?2, ?3, ?4)",
            params![event_id_str, title, body, tags],
        )?;

        // Payload-specific projection updates.
        let updated_at = event.time_recorded.timestamp_millis();
        match &event.payload {
            EventPayload::Pref(p) => {
                let value_json = serde_json::to_string(&p.value)?;
                tx.execute(
                    r#"INSERT INTO pref_current(category, key, value_json, event_id, updated_at)
                       VALUES(?1, ?2, ?3, ?4, ?5)
                       ON CONFLICT(category, key) DO UPDATE SET
                         value_json = excluded.value_json,
                         event_id   = excluded.event_id,
                         updated_at = excluded.updated_at"#,
                    params![p.category, p.key, value_json, event_id_str, updated_at],
                )?;
            }

            EventPayload::Claim(c) => {
                // Chain identity: Event.chain_id if set; otherwise this is
                // a chain root, so use event_id as the chain id.
                let chain_id = event
                    .chain_id
                    .map(|u| u.to_string())
                    .unwrap_or_else(|| event_id_str.clone());
                let content_json = serde_json::to_string(&c.content)?;
                let supersedes = c.supersedes.map(|u| u.to_string());

                // RULE: every Claim ingest resets `verdict` and
                // `is_archived` on the (schema_type, key) row. Each tip is
                // a new, user-asserted state. If the caller wants the new
                // tip to inherit a verification or archive state, they
                // issue a fresh Verify/Archive event against it. This is
                // simpler than trying to propagate state through chains
                // and closes Codex round 4 P1: "archive + supersede +
                // unarchive desyncs because Unarchive targets the old
                // event_id while claim_current has moved to the new tip."
                tx.execute(
                    r#"INSERT INTO claim_current(
                         schema_type, key, event_id, chain_id, content_json,
                         supersedes_event_id, verdict, verdict_event_id, verdict_at,
                         is_archived, updated_at
                       ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, 0, ?7)
                       ON CONFLICT(schema_type, key) DO UPDATE SET
                         event_id             = excluded.event_id,
                         chain_id             = excluded.chain_id,
                         content_json         = excluded.content_json,
                         supersedes_event_id  = excluded.supersedes_event_id,
                         verdict              = NULL,
                         verdict_event_id     = NULL,
                         verdict_at           = NULL,
                         is_archived          = 0,
                         updated_at           = excluded.updated_at"#,
                    params![
                        c.schema_type,
                        c.key,
                        event_id_str,
                        chain_id,
                        content_json,
                        supersedes,
                        updated_at
                    ],
                )?;
            }

            EventPayload::Verify(v) => {
                let verdict = snake_case(&v.verdict)?;
                let target = v.target_entity.to_string();
                // Only annotates the chain tip. If the target is a superseded
                // (non-current) claim, this is a no-op on the projection —
                // the Verify event itself is still in events/events_fts.
                let n = tx.execute(
                    r#"UPDATE claim_current
                       SET verdict = ?1, verdict_event_id = ?2, verdict_at = ?3
                       WHERE event_id = ?4"#,
                    params![verdict, event_id_str, updated_at, target],
                )?;
                if n == 0 {
                    debug!(
                        target_entity = %target,
                        "Verify targeted a non-current event; claim_current unchanged"
                    );
                }
            }

            EventPayload::Archive(a) => {
                let direction = snake_case(&a.direction)?;
                let target = a.target_entity.to_string();
                tx.execute(
                    r#"INSERT INTO entity_archive(target_entity, direction, updated_at)
                       VALUES(?1, ?2, ?3)
                       ON CONFLICT(target_entity) DO UPDATE SET
                         direction  = excluded.direction,
                         updated_at = excluded.updated_at"#,
                    params![target, direction, updated_at],
                )?;
                // Reflect the archive state onto the claim projection if the
                // target is a current claim. "archive" → 1, "unarchive" → 0.
                let flag: i64 = if direction == "archive" { 1 } else { 0 };
                tx.execute(
                    r#"UPDATE claim_current SET is_archived = ?1 WHERE event_id = ?2"#,
                    params![flag, target],
                )?;
            }

            EventPayload::Redact(r) => {
                let target = r.target_entity.to_string();
                // Audit row stays. Searchable text disappears. Projections
                // that materialized the redacted event's content must also
                // drop — otherwise current_pref / current_claim would still
                // serve the sensitive payload. Codex P1 round 3.
                tx.execute(
                    "UPDATE events SET is_redacted = 1 WHERE event_id = ?1",
                    params![target],
                )?;
                tx.execute(
                    "DELETE FROM events_fts WHERE event_id = ?1",
                    params![target],
                )?;

                // Rollback semantic (Codex R13 P1): a redact of the current
                // Pref / Claim tip must restore the most recent surviving
                // predecessor, not leave current_pref / current_claim
                // empty. Otherwise `Pref(A) → Pref(B) → Redact(B)` would
                // permanently lose A. Capture the projection keys BEFORE
                // deleting so we can find predecessors to re-project.

                // --- pref_current rollback ---
                let pref_keys: Option<(String, String)> = tx
                    .query_row(
                        "SELECT category, key FROM pref_current WHERE event_id = ?1",
                        params![target],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?;
                tx.execute(
                    "DELETE FROM pref_current WHERE event_id = ?1",
                    params![target],
                )?;
                if let Some((cat, key)) = pref_keys {
                    // Most recent non-redacted Pref event for the same
                    // (category, key), excluding the redacted one.
                    let predecessor: Option<(String, String, i64)> = tx
                        .query_row(
                            r#"SELECT event_id, payload_json, time_recorded
                                 FROM events
                                WHERE event_type = 'pref'
                                  AND is_redacted = 0
                                  AND event_id != ?1
                                  AND json_extract(payload_json, '$.category') = ?2
                                  AND json_extract(payload_json, '$.key')      = ?3
                             ORDER BY time_recorded DESC, event_id DESC
                                LIMIT 1"#,
                            params![target, cat, key],
                            |row| {
                                Ok((
                                    row.get::<_, String>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, i64>(2)?,
                                ))
                            },
                        )
                        .optional()?;
                    if let Some((pred_eid, pred_payload, pred_ts)) = predecessor {
                        // Parse in Rust rather than using SQLite's
                        // json_extract, which returns the extracted JSON
                        // value as its SQL-native type (e.g. a JSON string
                        // "x" comes back as bare text `x`, no longer valid
                        // JSON). Re-encode so `current_pref` can round-trip.
                        let payload: serde_json::Value = serde_json::from_str(&pred_payload)?;
                        let value = payload
                            .get("value")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        let value_json = serde_json::to_string(&value)?;
                        tx.execute(
                            r#"INSERT OR REPLACE INTO pref_current(
                                 category, key, value_json, event_id, updated_at
                               ) VALUES(?1, ?2, ?3, ?4, ?5)"#,
                            params![cat, key, value_json, pred_eid, pred_ts],
                        )?;
                    }
                    // If no predecessor, current_pref stays empty. Correct:
                    // the only state ever set for this (category, key) was
                    // the one we just redacted.
                }

                // --- claim_current rollback ---
                let claim_keys: Option<(String, String)> = tx
                    .query_row(
                        "SELECT schema_type, key FROM claim_current WHERE event_id = ?1",
                        params![target],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?;
                tx.execute(
                    "DELETE FROM claim_current WHERE event_id = ?1",
                    params![target],
                )?;
                if let Some((schema_type, key)) = claim_keys {
                    let predecessor: Option<ClaimPredecessor> = tx
                        .query_row(
                            r#"SELECT event_id, payload_json, chain_id,
                                      json_extract(payload_json, '$.supersedes'),
                                      time_recorded
                                 FROM events
                                WHERE event_type = 'claim'
                                  AND is_redacted = 0
                                  AND event_id != ?1
                                  AND json_extract(payload_json, '$.schema_type') = ?2
                                  AND json_extract(payload_json, '$.key')         = ?3
                             ORDER BY time_recorded DESC, event_id DESC
                                LIMIT 1"#,
                            params![target, schema_type, key],
                            |row| {
                                Ok((
                                    row.get::<_, String>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, Option<String>>(2)?,
                                    row.get::<_, Option<String>>(3)?,
                                    row.get::<_, i64>(4)?,
                                ))
                            },
                        )
                        .optional()?;
                    if let Some((pred_eid, pred_payload, pred_chain, pred_supersedes, pred_ts)) =
                        predecessor
                    {
                        // Chain id fallback: if the predecessor event had
                        // no explicit chain_id at ingest, use its own
                        // event_id as the chain root (same rule as the
                        // Claim arm above).
                        let chain_id = pred_chain.unwrap_or_else(|| pred_eid.clone());
                        // Parse content_json in Rust (same json_extract
                        // SQL-typing gotcha as the Pref arm above).
                        let payload: serde_json::Value = serde_json::from_str(&pred_payload)?;
                        let content = payload
                            .get("content")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        let content_json = serde_json::to_string(&content)?;
                        // Verify / Archive state is NOT replayed. Each
                        // Claim tip starts fresh per the "new tip = new
                        // assertion" rule; callers re-issue Verify /
                        // Archive against the restored tip if they want.
                        tx.execute(
                            r#"INSERT OR REPLACE INTO claim_current(
                                 schema_type, key, event_id, chain_id, content_json,
                                 supersedes_event_id, verdict, verdict_event_id, verdict_at,
                                 is_archived, updated_at
                               ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, 0, ?7)"#,
                            params![
                                schema_type,
                                key,
                                pred_eid,
                                chain_id,
                                content_json,
                                pred_supersedes,
                                pred_ts
                            ],
                        )?;
                    }
                }
            }

            EventPayload::Observe(_)
            | EventPayload::Lesson(_)
            | EventPayload::SkillEdit(_)
            | EventPayload::Import(_)
            | EventPayload::Audit(_) => {
                // Only events + events_fts — already handled above.
            }
        }

        // Watermark at end of same txn.
        tx.execute(
            r#"INSERT INTO watermark(key, value) VALUES('last_commit_oid', ?1)
               ON CONFLICT(key) DO UPDATE SET value = excluded.value"#,
            params![event.commit_oid],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// Wipe and replay the whole journal. Called from brain-app's doctor
    /// path or on schema bump. Returns the number of events indexed.
    ///
    /// The caller must provide events in deterministic oldest-first journal
    /// order. Projection state is order-dependent: Pref/Claim tips, Verify,
    /// Archive, and Redact rollback all mean "latest event wins".
    pub fn rebuild_from<I: IntoIterator<Item = Event>>(
        &mut self,
        events: I,
    ) -> Result<usize, IndexError> {
        self.truncate()?;
        let mut n = 0usize;
        for ev in events {
            self.ingest(&ev)?;
            n += 1;
        }
        Ok(n)
    }
}

/// Extract `(title, body, extra_tags)` text for the FTS5 row. `title` is
/// the most-specific per event (boosted by bm25 weights at search time);
/// `body` is the long-form content; `extra_tags` adds per-payload keywords
/// that aren't covered by actor/layer/classification (which the caller
/// prepends).
fn extract_fts_fields(p: &EventPayload) -> Result<(String, String, String), IndexError> {
    use EventPayload::*;
    Ok(match p {
        // `source` is bookkeeping metadata ("brain-cli", "mcp",
        // etc.) — NOT searchable content. Indexing it poisoned every
        // CLI-saved note with the token `brain`, so searching for
        // "brain" matched every note via the tag column.
        // source stays in payload_json for retrieval / inspection,
        // just not in the FTS tokens. Agent team review.
        Observe(o) => (
            o.summary.clone(),
            o.content.clone().unwrap_or_default(),
            String::new(),
        ),
        Claim(c) => (
            format!("{}/{}", c.schema_type, c.key),
            serde_json::to_string(&c.content)?,
            String::new(),
        ),
        Lesson(l) => (l.category.clone(), l.text.clone(), String::new()),
        Pref(p) => (
            format!("{}/{}", p.category, p.key),
            serde_json::to_string(&p.value)?,
            String::new(),
        ),
        SkillEdit(s) => (
            s.skill_name.clone(),
            s.file.clone(),
            s.reason.clone().unwrap_or_default(),
        ),
        Verify(v) => (
            format!("verify {}", v.target_entity),
            v.attestation.clone(),
            String::new(),
        ),
        Archive(a) => (
            format!("archive {}", a.target_entity),
            a.reason.clone().unwrap_or_default(),
            String::new(),
        ),
        Redact(r) => (
            format!("redact {}", r.target_entity),
            r.reason.clone(),
            r.triggered_by_pattern.clone().unwrap_or_default(),
        ),
        Import(i) => (
            format!("import {} items", i.item_count),
            i.manifest_hash.clone(),
            String::new(),
        ),
        Audit(a) => (
            format!("audit: {:?}", a.kind),
            a.detail.clone(),
            String::new(),
        ),
    })
}

/// Serialize a `rename_all = "snake_case"` enum variant to its wire string.
/// Works for every non-internally-tagged enum in brain-types. Returns an
/// `IndexError::Invariant` if the type doesn't serialize to a JSON string
/// (that's a brain-types bug, but panicking the entire writer is worse).
fn snake_case<T: serde::Serialize>(t: &T) -> Result<String, IndexError> {
    let v = serde_json::to_value(t)?;
    v.as_str()
        .map(str::to_string)
        .ok_or_else(|| IndexError::Invariant {
            detail: format!("enum variant serialized to non-string JSON: {}", v),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_types::*;
    use chrono::Utc;
    use tempfile::TempDir;
    use uuid::Uuid;

    fn sample_event(payload: EventPayload, commit_oid: &str, idem: &str) -> Event {
        let event_type = match &payload {
            EventPayload::Observe(_) => EventType::Observe,
            EventPayload::Claim(_) => EventType::Claim,
            EventPayload::Lesson(_) => EventType::Lesson,
            EventPayload::Pref(_) => EventType::Pref,
            EventPayload::SkillEdit(_) => EventType::SkillEdit,
            EventPayload::Verify(_) => EventType::Verify,
            EventPayload::Archive(_) => EventType::Archive,
            EventPayload::Redact(_) => EventType::Redact,
            EventPayload::Import(_) => EventType::Import,
            EventPayload::Audit(_) => EventType::Audit,
        };
        Event {
            event_id: Uuid::now_v7(),
            event_type,
            subject: SubjectRef::Entity { id: Uuid::now_v7() },
            parent_event_id: None,
            chain_id: None,
            actor: Actor {
                kind: ActorKind::Agent,
                id: "claude".to_string(),
                harness: Some("test".to_string()),
                signing_key_fingerprint: None,
            },
            commit_oid: commit_oid.to_string(),
            time_observed: Utc::now(),
            time_recorded: Utc::now(),
            layer: Layer::Episodic,
            authority: Authority {
                source_kind: AuthoritySource::Agent,
                score: Some(50),
                attested_by: None,
            },
            signature_state: SignatureState::Unsigned,
            classification: Classification::Private,
            payload,
            schema_version: SCHEMA_VERSION,
            idempotency_key: Some(idem.to_string()),
        }
    }

    fn obs(summary: &str) -> EventPayload {
        EventPayload::Observe(ObservePayload {
            summary: summary.to_string(),
            content: None,
            content_ref: None,
            source: None,
        })
    }

    fn new_index() -> (TempDir, BrainIndex) {
        let td = TempDir::new().unwrap();
        let idx = BrainIndex::open(td.path().join("i.sqlite")).unwrap();
        (td, idx)
    }

    #[test]
    fn observe_source_metadata_does_not_pollute_fts_index() {
        // Agent-team review: `source: "brain-cli"` on every CLI-saved
        // note used to land in the FTS tags column, so searching for
        // `brain` matched every note via the tag instead of only the
        // notes that actually mentioned "brain" in their text.
        // This test saves a note whose ONLY connection to "brain" is
        // the source field and asserts "brain" returns no FTS hit.
        let (_td, mut idx) = new_index();
        let ev = sample_event(
            EventPayload::Observe(ObservePayload {
                summary: "plain note with no special words".into(),
                content: None,
                content_ref: None,
                source: Some("brain-cli".into()),
            }),
            "c1",
            "i1",
        );
        idx.ingest(&ev).unwrap();

        // Direct FTS5 query. Before the fix this would return 1
        // because `brain-cli` tokenizes to `brain` + `cli`.
        let hit_brain: i64 = idx
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM events_fts WHERE events_fts MATCH 'brain'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        assert_eq!(
            hit_brain, 0,
            "source metadata must NOT be searchable — that's pollution"
        );

        // Sanity: real content IS indexed.
        let hit_plain: i64 = idx
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM events_fts WHERE events_fts MATCH 'plain'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        assert_eq!(hit_plain, 1, "title tokens must still be indexed");
    }

    #[test]
    fn ingests_observe_and_creates_events_and_fts_rows() {
        let (_td, mut idx) = new_index();
        idx.ingest(&sample_event(obs("hello"), "cmt1", "idem1"))
            .unwrap();
        let c: i64 = idx
            .connection()
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(c, 1);
        let f: i64 = idx
            .connection()
            .query_row("SELECT COUNT(*) FROM events_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(f, 1);
        assert_eq!(idx.last_committed_oid().unwrap().as_deref(), Some("cmt1"));
    }

    #[test]
    fn ingest_is_idempotent() {
        let (_td, mut idx) = new_index();
        let ev = sample_event(obs("once"), "cmt1", "idem-once");
        idx.ingest(&ev).unwrap();
        idx.ingest(&ev).unwrap();
        assert_eq!(idx.event_count().unwrap(), 1);
        let f: i64 = idx
            .connection()
            .query_row("SELECT COUNT(*) FROM events_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(f, 1);
    }

    #[test]
    fn pref_projection_upserts_on_second_write() {
        let (_td, mut idx) = new_index();
        let p1 = EventPayload::Pref(PrefPayload {
            category: "auth".into(),
            key: "provider".into(),
            value: serde_json::json!("authlib"),
            previous_value: None,
        });
        let p2 = EventPayload::Pref(PrefPayload {
            category: "auth".into(),
            key: "provider".into(),
            value: serde_json::json!("fastapi-users"),
            previous_value: Some(serde_json::json!("authlib")),
        });
        idx.ingest(&sample_event(p1, "c1", "i1")).unwrap();
        idx.ingest(&sample_event(p2, "c2", "i2")).unwrap();

        let val: String = idx
            .connection()
            .query_row(
                "SELECT value_json FROM pref_current WHERE category='auth' AND key='provider'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(val, r#""fastapi-users""#);
        let count: i64 = idx
            .connection()
            .query_row("SELECT COUNT(*) FROM pref_current", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn claim_projection_tracks_supersession() {
        let (_td, mut idx) = new_index();
        let a = sample_event(
            EventPayload::Claim(ClaimPayload {
                schema_type: "drug.contraindication".into(),
                key: "warfarin/ibuprofen".into(),
                content: serde_json::json!({"severity": "high"}),
                supersedes: None,
                support_refs: vec![],
                valid_from: None,
                valid_to: None,
            }),
            "c-a",
            "idem-a",
        );
        let b = sample_event(
            EventPayload::Claim(ClaimPayload {
                schema_type: "drug.contraindication".into(),
                key: "warfarin/ibuprofen".into(),
                content: serde_json::json!({"severity": "moderate"}),
                supersedes: Some(a.event_id),
                support_refs: vec![],
                valid_from: None,
                valid_to: None,
            }),
            "c-b",
            "idem-b",
        );
        idx.ingest(&a).unwrap();
        idx.ingest(&b).unwrap();

        let (event_id, supersedes, verdict): (String, Option<String>, Option<String>) = idx
            .connection()
            .query_row(
                "SELECT event_id, supersedes_event_id, verdict FROM claim_current
                 WHERE schema_type='drug.contraindication' AND key='warfarin/ibuprofen'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(event_id, b.event_id.to_string());
        assert_eq!(
            supersedes.as_deref(),
            Some(a.event_id.to_string()).as_deref()
        );
        assert!(verdict.is_none());
    }

    #[test]
    fn verify_annotates_claim_without_removing_row() {
        let (_td, mut idx) = new_index();
        let claim = sample_event(
            EventPayload::Claim(ClaimPayload {
                schema_type: "fact".into(),
                key: "sky-is-green".into(),
                content: serde_json::json!(true),
                supersedes: None,
                support_refs: vec![],
                valid_from: None,
                valid_to: None,
            }),
            "c-claim",
            "idem-claim",
        );
        idx.ingest(&claim).unwrap();

        let v = sample_event(
            EventPayload::Verify(VerifyPayload {
                target_entity: claim.event_id,
                attestation: "observation contradicts".into(),
                verdict: VerifyVerdict::Refuted,
            }),
            "c-v",
            "idem-v",
        );
        idx.ingest(&v).unwrap();

        let (verdict, content_json): (Option<String>, String) = idx.connection().query_row(
            "SELECT verdict, content_json FROM claim_current WHERE schema_type='fact' AND key='sky-is-green'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).unwrap();
        assert_eq!(verdict.as_deref(), Some("refuted"));
        assert_eq!(content_json, "true");
    }

    #[test]
    fn redact_soft_removes_from_fts_and_flags_row() {
        let (_td, mut idx) = new_index();
        let target = sample_event(obs("sensitive stuff"), "c-t", "idem-t");
        let target_id = target.event_id;
        idx.ingest(&target).unwrap();

        let red = sample_event(
            EventPayload::Redact(RedactPayload {
                target_entity: target_id,
                reason: "operator request".into(),
                triggered_by_pattern: None,
                redacted_spans: vec![],
            }),
            "c-r",
            "idem-r",
        );
        idx.ingest(&red).unwrap();

        let is_red: i64 = idx
            .connection()
            .query_row(
                "SELECT is_redacted FROM events WHERE event_id = ?1",
                params![target_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(is_red, 1);

        let fts_count: i64 = idx
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM events_fts WHERE event_id = ?1",
                params![target_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_count, 0, "redacted event must vanish from fts");
    }

    #[test]
    fn archive_flips_is_archived_on_claim_current() {
        let (_td, mut idx) = new_index();
        let claim = sample_event(
            EventPayload::Claim(ClaimPayload {
                schema_type: "note".into(),
                key: "k".into(),
                content: serde_json::json!("body"),
                supersedes: None,
                support_refs: vec![],
                valid_from: None,
                valid_to: None,
            }),
            "c-claim",
            "idem-c",
        );
        let claim_event_id = claim.event_id;
        idx.ingest(&claim).unwrap();

        let arch = sample_event(
            EventPayload::Archive(ArchivePayload {
                target_entity: claim_event_id,
                direction: ArchiveDirection::Archive,
                reason: Some("tidy".into()),
            }),
            "c-arch",
            "idem-a",
        );
        idx.ingest(&arch).unwrap();

        let is_archived: i64 = idx
            .connection()
            .query_row(
                "SELECT is_archived FROM claim_current WHERE schema_type='note' AND key='k'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(is_archived, 1);
    }

    #[test]
    fn fresh_claim_on_archived_key_resets_is_archived() {
        // Codex P1: Archive targets a specific event_id; a later *fresh*
        // Claim (supersedes=None) on the same (schema_type, key) is an
        // unrelated claim. The prior row's is_archived flag must not
        // carry over.
        let (_td, mut idx) = new_index();
        let a = sample_event(
            EventPayload::Claim(ClaimPayload {
                schema_type: "pol".into(),
                key: "k".into(),
                content: serde_json::json!("old"),
                supersedes: None,
                support_refs: vec![],
                valid_from: None,
                valid_to: None,
            }),
            "c-a",
            "i-a",
        );
        let a_id = a.event_id;
        idx.ingest(&a).unwrap();

        idx.ingest(&sample_event(
            EventPayload::Archive(ArchivePayload {
                target_entity: a_id,
                direction: ArchiveDirection::Archive,
                reason: None,
            }),
            "c-arch",
            "i-arch",
        ))
        .unwrap();

        // Fresh claim at same (schema_type, key), unrelated to A.
        idx.ingest(&sample_event(
            EventPayload::Claim(ClaimPayload {
                schema_type: "pol".into(),
                key: "k".into(),
                content: serde_json::json!("brand-new"),
                supersedes: None,
                support_refs: vec![],
                valid_from: None,
                valid_to: None,
            }),
            "c-b",
            "i-b",
        ))
        .unwrap();

        let is_arch: i64 = idx
            .connection()
            .query_row(
                "SELECT is_archived FROM claim_current WHERE schema_type='pol' AND key='k'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(is_arch, 0, "fresh chain root must reset is_archived");
    }

    #[test]
    fn redact_scrubs_claim_and_pref_projections() {
        // Codex P1: Redact leaves claim_current / pref_current populated
        // with the sensitive payload. Projections must drop on Redact.
        let (_td, mut idx) = new_index();
        let claim = sample_event(
            EventPayload::Claim(ClaimPayload {
                schema_type: "s".into(),
                key: "k".into(),
                content: serde_json::json!({"secret": "classified"}),
                supersedes: None,
                support_refs: vec![],
                valid_from: None,
                valid_to: None,
            }),
            "c-cl",
            "i-cl",
        );
        let claim_id = claim.event_id;
        idx.ingest(&claim).unwrap();

        let pref = sample_event(
            EventPayload::Pref(PrefPayload {
                category: "x".into(),
                key: "y".into(),
                value: serde_json::json!("dangerous default"),
                previous_value: None,
            }),
            "c-pf",
            "i-pf",
        );
        let pref_id = pref.event_id;
        idx.ingest(&pref).unwrap();

        assert!(idx.current_claim("s", "k").unwrap().is_some());
        assert!(idx.current_pref("x", "y").unwrap().is_some());

        // Redact the claim.
        idx.ingest(&sample_event(
            EventPayload::Redact(RedactPayload {
                target_entity: claim_id,
                reason: "classified".into(),
                triggered_by_pattern: None,
                redacted_spans: vec![],
            }),
            "c-rc",
            "i-rc",
        ))
        .unwrap();
        // Redact the pref.
        idx.ingest(&sample_event(
            EventPayload::Redact(RedactPayload {
                target_entity: pref_id,
                reason: "stale".into(),
                triggered_by_pattern: None,
                redacted_spans: vec![],
            }),
            "c-rp",
            "i-rp",
        ))
        .unwrap();

        assert!(
            idx.current_claim("s", "k").unwrap().is_none(),
            "redacted claim must not surface in current_claim"
        );
        assert!(
            idx.current_pref("x", "y").unwrap().is_none(),
            "redacted pref must not surface in current_pref"
        );
    }

    #[test]
    fn redact_of_current_pref_tip_restores_predecessor() {
        // Codex R13 P1: redacting the current Pref tip used to leave
        // current_pref empty. The correct semantic is: roll back to the
        // most recent non-redacted prior Pref for the same (category,
        // key). Pref(A) → Pref(B) → Redact(B) must end with
        // current_pref = A.
        let (_td, mut idx) = new_index();
        let a = sample_event(
            EventPayload::Pref(PrefPayload {
                category: "auth".into(),
                key: "provider".into(),
                value: serde_json::json!("authlib"),
                previous_value: None,
            }),
            "c-a",
            "i-a",
        );
        let b = sample_event(
            EventPayload::Pref(PrefPayload {
                category: "auth".into(),
                key: "provider".into(),
                value: serde_json::json!("fastapi-users"),
                previous_value: Some(serde_json::json!("authlib")),
            }),
            "c-b",
            "i-b",
        );
        let b_id = b.event_id;
        idx.ingest(&a).unwrap();
        idx.ingest(&b).unwrap();

        let redact = sample_event(
            EventPayload::Redact(RedactPayload {
                target_entity: b_id,
                reason: "classified".into(),
                triggered_by_pattern: None,
                redacted_spans: vec![],
            }),
            "c-r",
            "i-r",
        );
        idx.ingest(&redact).unwrap();

        let tip = idx
            .current_pref("auth", "provider")
            .unwrap()
            .expect("redact must restore predecessor, not erase projection");
        assert_eq!(tip.value, serde_json::json!("authlib"));
        assert_eq!(tip.event_id, a.event_id);
    }

    #[test]
    fn redact_of_current_claim_tip_restores_predecessor() {
        // Claim version of the same invariant.
        let (_td, mut idx) = new_index();
        let a = sample_event(
            EventPayload::Claim(ClaimPayload {
                schema_type: "drug.contraindication".into(),
                key: "warfarin/ibuprofen".into(),
                content: serde_json::json!({"severity": "high"}),
                supersedes: None,
                support_refs: vec![],
                valid_from: None,
                valid_to: None,
            }),
            "c-a",
            "i-a",
        );
        let b = sample_event(
            EventPayload::Claim(ClaimPayload {
                schema_type: "drug.contraindication".into(),
                key: "warfarin/ibuprofen".into(),
                content: serde_json::json!({"severity": "moderate"}),
                supersedes: Some(a.event_id),
                support_refs: vec![],
                valid_from: None,
                valid_to: None,
            }),
            "c-b",
            "i-b",
        );
        let b_id = b.event_id;
        idx.ingest(&a).unwrap();
        idx.ingest(&b).unwrap();

        let redact = sample_event(
            EventPayload::Redact(RedactPayload {
                target_entity: b_id,
                reason: "classified".into(),
                triggered_by_pattern: None,
                redacted_spans: vec![],
            }),
            "c-r",
            "i-r",
        );
        idx.ingest(&redact).unwrap();

        let tip = idx
            .current_claim("drug.contraindication", "warfarin/ibuprofen")
            .unwrap()
            .expect("redact must restore predecessor claim");
        assert_eq!(tip.content, serde_json::json!({"severity": "high"}));
        assert_eq!(tip.event_id, a.event_id);
    }

    #[test]
    fn redact_with_no_predecessor_leaves_projection_empty() {
        // If the redacted event was the only one for its (category,
        // key), there's nothing to restore — current_pref must stay
        // empty, not panic or crash.
        let (_td, mut idx) = new_index();
        let only = sample_event(
            EventPayload::Pref(PrefPayload {
                category: "solo".into(),
                key: "thing".into(),
                value: serde_json::json!(42),
                previous_value: None,
            }),
            "c-o",
            "i-o",
        );
        let only_id = only.event_id;
        idx.ingest(&only).unwrap();

        let redact = sample_event(
            EventPayload::Redact(RedactPayload {
                target_entity: only_id,
                reason: "oops".into(),
                triggered_by_pattern: None,
                redacted_spans: vec![],
            }),
            "c-r",
            "i-r",
        );
        idx.ingest(&redact).unwrap();

        assert!(idx.current_pref("solo", "thing").unwrap().is_none());
    }

    #[test]
    fn redact_predecessor_skips_redacted_events_and_ties_break_by_event_id() {
        // Agent-team review R14: same-millisecond inserts made the
        // predecessor pick non-deterministic. The SQL now
        // `ORDER BY time_recorded DESC, event_id DESC`, so ties
        // resolve by the UUIDv7 event_id (which is itself
        // time-ordered at millisecond resolution, with a random tail
        // for tiebreak). Also asserts that an older redacted
        // predecessor is SKIPPED.
        let (_td, mut idx) = new_index();

        // A, then B_redacted, then C, all for the same (cat,key).
        // Then redact C. Predecessor must be A (B is redacted) even
        // though B is time-wise closer to C.
        let a = sample_event(
            EventPayload::Pref(PrefPayload {
                category: "k".into(),
                key: "v".into(),
                value: serde_json::json!("A-value"),
                previous_value: None,
            }),
            "c-a",
            "i-a",
        );
        let b = sample_event(
            EventPayload::Pref(PrefPayload {
                category: "k".into(),
                key: "v".into(),
                value: serde_json::json!("B-value"),
                previous_value: None,
            }),
            "c-b",
            "i-b",
        );
        let c = sample_event(
            EventPayload::Pref(PrefPayload {
                category: "k".into(),
                key: "v".into(),
                value: serde_json::json!("C-value"),
                previous_value: None,
            }),
            "c-c",
            "i-c",
        );
        let b_id = b.event_id;
        let c_id = c.event_id;
        idx.ingest(&a).unwrap();
        idx.ingest(&b).unwrap();
        idx.ingest(&c).unwrap();

        // Redact B first (it's not the current tip, so this just
        // flips is_redacted but doesn't touch pref_current). Then
        // redact C (the current tip). Rollback must pick A, not B.
        idx.ingest(&sample_event(
            EventPayload::Redact(RedactPayload {
                target_entity: b_id,
                reason: "scrub B".into(),
                triggered_by_pattern: None,
                redacted_spans: vec![],
            }),
            "c-r1",
            "i-r1",
        ))
        .unwrap();
        idx.ingest(&sample_event(
            EventPayload::Redact(RedactPayload {
                target_entity: c_id,
                reason: "scrub C".into(),
                triggered_by_pattern: None,
                redacted_spans: vec![],
            }),
            "c-r2",
            "i-r2",
        ))
        .unwrap();

        let tip = idx
            .current_pref("k", "v")
            .unwrap()
            .expect("redact rollback must skip the redacted B and pick A");
        assert_eq!(tip.value, serde_json::json!("A-value"));
        assert_eq!(tip.event_id, a.event_id);
    }

    #[test]
    fn rebuild_from_clears_and_replays() {
        let (_td, mut idx) = new_index();
        idx.ingest(&sample_event(obs("a"), "c1", "i1")).unwrap();
        idx.ingest(&sample_event(obs("b"), "c2", "i2")).unwrap();
        idx.ingest(&sample_event(obs("c"), "c3", "i3")).unwrap();
        assert_eq!(idx.event_count().unwrap(), 3);

        let replay = vec![
            sample_event(obs("x"), "r1", "j1"),
            sample_event(obs("y"), "r2", "j2"),
        ];
        let n = idx.rebuild_from(replay).unwrap();
        assert_eq!(n, 2);
        assert_eq!(idx.event_count().unwrap(), 2);
        assert_eq!(idx.last_committed_oid().unwrap().as_deref(), Some("r2"));
    }
}
