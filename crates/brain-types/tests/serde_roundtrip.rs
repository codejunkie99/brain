use brain_types::*;
use chrono::Utc;
use uuid::Uuid;

fn sample_observe_draft() -> EventDraft {
    EventDraft {
        event_type: EventType::Observe,
        subject: SubjectRef::Entity {
            id: Uuid::parse_str("01932b6a-0000-7000-8000-000000000001").unwrap(),
        },
        parent_event_id: None,
        chain_id: None,
        actor: Actor {
            kind: ActorKind::Agent,
            id: "claude-opus-4-7".to_string(),
            harness: Some("claude-desktop".to_string()),
            signing_key_fingerprint: None,
        },
        time_observed: Some(Utc::now()),
        layer: Layer::Episodic,
        authority: Authority {
            source_kind: AuthoritySource::Agent,
            score: Some(50),
            attested_by: None,
        },
        classification: Classification::Private,
        payload: EventPayload::Observe(ObservePayload {
            summary: "Agent observed a 429 from the billing API".to_string(),
            content: Some("rate_limit_reached: 100 req/min exceeded".to_string()),
            content_ref: None,
            source: Some("billing-api-call#42".to_string()),
        }),
        schema_version: SCHEMA_VERSION,
        idempotency_key: "tests::sample_observe::fixed-key".to_string(),
    }
}

#[test]
fn draft_roundtrips_through_json() {
    let draft = sample_observe_draft();
    let json = serde_json::to_string(&draft).expect("serialize");
    let parsed: EventDraft = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(draft, parsed);
}

#[test]
fn shallow_validate_passes_on_matching_type() {
    let draft = sample_observe_draft();
    draft
        .shallow_validate(SCHEMA_VERSION)
        .expect("should validate");
}

#[test]
fn shallow_validate_rejects_type_mismatch() {
    let mut draft = sample_observe_draft();
    draft.event_type = EventType::Claim;
    let err = draft
        .shallow_validate(SCHEMA_VERSION)
        .expect_err("should reject");
    assert!(matches!(err, RejectReason::TypeMismatch { .. }));
}

#[test]
fn shallow_validate_rejects_empty_idempotency_key() {
    let mut draft = sample_observe_draft();
    draft.idempotency_key = "   ".to_string();
    let err = draft
        .shallow_validate(SCHEMA_VERSION)
        .expect_err("should reject");
    assert!(matches!(err, RejectReason::EmptyIdempotencyKey));
}

#[test]
fn shallow_validate_rejects_wrong_schema_version() {
    let draft = sample_observe_draft();
    let err = draft
        .shallow_validate(SCHEMA_VERSION + 99)
        .expect_err("should reject");
    assert!(matches!(err, RejectReason::UnsupportedSchema { .. }));
}

#[test]
fn shallow_validate_rejects_oversized_idempotency_key() {
    let mut draft = sample_observe_draft();
    draft.idempotency_key = "a".repeat(IDEMPOTENCY_KEY_MAX_LEN + 1);
    let err = draft
        .shallow_validate(SCHEMA_VERSION)
        .expect_err("should reject");
    assert!(matches!(err, RejectReason::IdempotencyKeyTooLong { .. }));
}

#[test]
fn shallow_validate_rejects_non_ascii_idempotency_key() {
    let mut draft = sample_observe_draft();
    // Unicode that passes is_empty/len checks but is not ASCII-printable.
    draft.idempotency_key = "tests::\u{1F410}".to_string();
    let err = draft
        .shallow_validate(SCHEMA_VERSION)
        .expect_err("should reject");
    assert!(matches!(err, RejectReason::IdempotencyKeyInvalidChars));
}

#[test]
fn observe_payload_allows_both_content_and_content_ref() {
    // We deliberately allow both to be present: a caller may include a
    // short inline excerpt alongside a blob ref for larger material. This
    // test documents the intent so no future reviewer tightens it without
    // thinking.
    let payload = EventPayload::Observe(ObservePayload {
        summary: "both fields set".to_string(),
        content: Some("inline excerpt".to_string()),
        content_ref: Some("blake3-hex-placeholder".to_string()),
        source: None,
    });
    let json = serde_json::to_string(&payload).expect("serialize");
    let parsed: EventPayload = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(payload, parsed);
}

#[test]
fn observe_payload_allows_neither_content_nor_content_ref() {
    // Also valid: a metadata-only observation with a summary only.
    let payload = EventPayload::Observe(ObservePayload {
        summary: "metadata only".to_string(),
        content: None,
        content_ref: None,
        source: None,
    });
    let json = serde_json::to_string(&payload).expect("serialize");
    let parsed: EventPayload = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(payload, parsed);
}

#[test]
fn claim_payload_with_supersedes_roundtrips() {
    let supersedes_id = Uuid::parse_str("01932b6a-0000-7000-8000-000000000002").unwrap();
    let payload = EventPayload::Claim(ClaimPayload {
        schema_type: "auth.library-choice".to_string(),
        key: "primary".to_string(),
        content: serde_json::json!({ "library": "fastapi-users", "reason": "PKCE ergonomic" }),
        supersedes: Some(supersedes_id),
        support_refs: vec![SupportRef {
            observation_event_id: Uuid::parse_str("01932b6a-0000-7000-8000-000000000003").unwrap(),
            excerpt: Some("PKCE with fastapi-users passed all integration tests".to_string()),
            noted_at: None,
        }],
        valid_from: Some(Utc::now()),
        valid_to: None,
    });
    let json = serde_json::to_string(&payload).expect("serialize");
    let parsed: EventPayload = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(payload, parsed);
    assert_eq!(parsed.event_type(), EventType::Claim);
}

#[test]
fn all_ten_event_types_serialize_distinctly() {
    let types = [
        EventType::Observe,
        EventType::Claim,
        EventType::Lesson,
        EventType::Pref,
        EventType::SkillEdit,
        EventType::Verify,
        EventType::Archive,
        EventType::Redact,
        EventType::Import,
        EventType::Audit,
    ];
    let tags: Vec<String> = types
        .iter()
        .map(|t| serde_json::to_string(t).unwrap())
        .collect();
    let unique: std::collections::HashSet<_> = tags.iter().collect();
    assert_eq!(unique.len(), 10, "tags collided: {tags:?}");
}
