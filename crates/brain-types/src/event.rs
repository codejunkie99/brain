//! The Event and EventDraft records.
//!
//! `Event` is a committed, durable record that has a commit_oid and has
//! passed pre-validation. `EventDraft` is the input to the write APIs
//! (`BrainRepo::append_event` / `LocalBrain::append`); it carries an
//! idempotency key and may be rejected before commit.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ack::{IDEMPOTENCY_KEY_MAX_LEN, RejectReason};
use crate::payload::{EventPayload, EventType};
use crate::subject::{Actor, Authority, Classification, Layer, SignatureState, SubjectRef};

/// A committed, durable event with fields validated against its payload type.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Event {
    /// UUID v7, sortable by time.
    pub event_id: Uuid,
    pub event_type: EventType,
    pub subject: SubjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<Uuid>,
    /// Present for chained events (CLAIM supersession chains).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<Uuid>,
    pub actor: Actor,
    /// Hex representation of the git object id. 40 chars for SHA-1 or 64 for
    /// SHA-256 (git 2.42+ supports both). Always present on committed events
    /// in memory, but NOT serialized to disk (the commit containing an event
    /// cannot know its own oid before the commit is written — classic
    /// self-reference). Readers enrich this field from the enclosing commit.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub commit_oid: String,
    pub time_observed: DateTime<Utc>,
    pub time_recorded: DateTime<Utc>,
    pub layer: Layer,
    pub authority: Authority,
    pub signature_state: SignatureState,
    pub classification: Classification,
    pub payload: EventPayload,
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// A write draft. Must carry an idempotency_key so writers can collapse
/// retries into a single committed event.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventDraft {
    pub event_type: EventType,
    pub subject: SubjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<Uuid>,
    pub actor: Actor,
    /// When the fact happened in reality. Writers fill `now` when None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_observed: Option<DateTime<Utc>>,
    pub layer: Layer,
    pub authority: Authority,
    pub classification: Classification,
    pub payload: EventPayload,
    pub schema_version: u32,
    /// REQUIRED on drafts. Writers map key -> EventRef for idempotency.
    pub idempotency_key: String,
}

impl EventDraft {
    /// Shallow structural validation. Full redaction / secret scanning is
    /// a separate pass in brain-store's write path; this only catches
    /// obvious shape errors. Returns the unified `RejectReason` so draft
    /// failures, batch failures, and RPC wire responses all speak the same
    /// vocabulary.
    pub fn shallow_validate(&self, binary_schema_version: u32) -> Result<(), RejectReason> {
        if self.schema_version != binary_schema_version {
            return Err(RejectReason::UnsupportedSchema {
                given: self.schema_version,
                expected: binary_schema_version,
            });
        }
        validate_idempotency_key(&self.idempotency_key)?;
        if !self.event_type.matches(&self.payload) {
            return Err(RejectReason::TypeMismatch {
                detail: format!(
                    "event_type={:?} but payload variant is {:?}",
                    self.event_type,
                    self.payload.event_type()
                ),
            });
        }
        if let Some(t) = self.time_observed {
            validate_time_observed(t)?;
        }
        Ok(())
    }
}

/// Supported range for `time_observed`. A DateTime outside this window
/// serializes fine but `DateTime::from_timestamp_millis` rejects on
/// read, making the event durably invisible. Rejecting at draft time
/// closes the content-hiding primitive. Picked as the intersection of
/// the Unix epoch and the year-9999 boundary chrono uses for display
/// formatting — any real "time this fact happened" falls inside.
pub fn validate_time_observed(t: DateTime<Utc>) -> Result<(), RejectReason> {
    use chrono::{NaiveDate, TimeZone};
    let min = Utc.from_utc_datetime(
        &NaiveDate::from_ymd_opt(1970, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
    );
    let max = Utc.from_utc_datetime(
        &NaiveDate::from_ymd_opt(9999, 12, 31)
            .unwrap()
            .and_hms_opt(23, 59, 59)
            .unwrap(),
    );
    if t < min || t > max {
        return Err(RejectReason::TimeObservedOutOfRange {
            given: t.to_rfc3339(),
            min: min.to_rfc3339(),
            max: max.to_rfc3339(),
        });
    }
    Ok(())
}

/// Validate that an idempotency key is non-empty, within length bounds, and
/// ASCII-printable only. See `IDEMPOTENCY_KEY_MAX_LEN`.
pub fn validate_idempotency_key(key: &str) -> Result<(), RejectReason> {
    if key.trim().is_empty() {
        return Err(RejectReason::EmptyIdempotencyKey);
    }
    if key.len() > IDEMPOTENCY_KEY_MAX_LEN {
        return Err(RejectReason::IdempotencyKeyTooLong {
            len: key.len(),
            max: IDEMPOTENCY_KEY_MAX_LEN,
        });
    }
    // ASCII printable: 0x20..=0x7E. No control characters, no non-ASCII.
    if !key.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
        return Err(RejectReason::IdempotencyKeyInvalidChars);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// TODOs surfaced by the 11th Codex pass, deferred to a later version:
//
// - Forward-compat serde wrapper. Today, adding an 11th EventType / 4th
//   SubjectRef variant makes older binaries fail deserialization before
//   `shallow_validate` even runs. Fix pattern: a raw-tagged envelope
//   (`{ "type": "...", "content": <raw JSON> }`) that we parse in two stages
//   and return a structured `Unknown { raw: Value }` arm. Premature at v0.1
//   since no client exists yet; revisit before the first breaking release.
//
// - `chain_id` mandatory-for-chainable. Today it is `Option<Uuid>` on every
//   event. For CLAIM supersession chains it should be Some from the first
//   event so chain queries are O(1). Fix: split EventDraft into discriminated
//   types per family, where Chainable::Claim carries a non-optional chain_id.
//
// - `SupportRef` relation semantics. Today it only points at
//   `observation_event_id`. Domain reasoning (confirms / contradicts /
//   weakens) will want an edge type. Add a `relation: SupportRelation` field
//   when we land `brain-verify`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Regression tests added by the agent-team review R14.
    use super::*;
    use crate::payload::{EventPayload, EventType, ObservePayload};
    use crate::subject::SubjectRef;
    use crate::{Actor, ActorKind, Authority, AuthoritySource, Classification, Layer};

    fn sample_draft_with_time(t: Option<DateTime<Utc>>) -> EventDraft {
        EventDraft {
            event_type: EventType::Observe,
            subject: SubjectRef::Entity { id: Uuid::now_v7() },
            parent_event_id: None,
            chain_id: None,
            actor: Actor {
                kind: ActorKind::Agent,
                id: "t".into(),
                harness: None,
                signing_key_fingerprint: None,
            },
            time_observed: t,
            layer: Layer::Episodic,
            authority: Authority {
                source_kind: AuthoritySource::Agent,
                score: None,
                attested_by: None,
            },
            classification: Classification::Private,
            payload: EventPayload::Observe(ObservePayload {
                summary: "ok".into(),
                content: None,
                content_ref: None,
                source: None,
            }),
            schema_version: crate::SCHEMA_VERSION,
            idempotency_key: "t".into(),
        }
    }

    #[test]
    fn shallow_validate_rejects_far_future_time_observed() {
        // Content-hiding primitive: a DateTime::MAX (or year 10000+)
        // commits fine but `from_timestamp_millis` returns None on
        // read, hiding the event from every filter.
        use chrono::TimeZone;
        let future = Utc
            .with_ymd_and_hms(10_000, 1, 1, 0, 0, 0)
            .single()
            .unwrap_or_else(Utc::now);
        let d = sample_draft_with_time(Some(future));
        let err = d.shallow_validate(crate::SCHEMA_VERSION).unwrap_err();
        match err {
            RejectReason::TimeObservedOutOfRange { .. } => {}
            other => panic!("expected TimeObservedOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn shallow_validate_rejects_pre_epoch_time_observed() {
        use chrono::TimeZone;
        let past = Utc.with_ymd_and_hms(1900, 1, 1, 0, 0, 0).unwrap();
        let d = sample_draft_with_time(Some(past));
        let err = d.shallow_validate(crate::SCHEMA_VERSION).unwrap_err();
        match err {
            RejectReason::TimeObservedOutOfRange { .. } => {}
            other => panic!("expected TimeObservedOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn shallow_validate_accepts_normal_time_observed() {
        let d = sample_draft_with_time(Some(Utc::now()));
        assert!(d.shallow_validate(crate::SCHEMA_VERSION).is_ok());
        // None also fine (daemon fills `now`).
        let d2 = sample_draft_with_time(None);
        assert!(d2.shallow_validate(crate::SCHEMA_VERSION).is_ok());
    }
}
