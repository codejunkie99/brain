//! Strongly-typed payloads per event variant. Replaces the previous
//! `payload: serde_json::Value` to eliminate runtime type drift.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::subject::SupportRef;

/// The tag for an event, matching the payload variant.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Observe,
    Claim,
    Lesson,
    Pref,
    SkillEdit,
    Verify,
    Archive,
    Redact,
    Import,
    Audit,
}

impl EventType {
    pub fn matches(&self, payload: &EventPayload) -> bool {
        matches!(
            (self, payload),
            (EventType::Observe, EventPayload::Observe(_))
                | (EventType::Claim, EventPayload::Claim(_))
                | (EventType::Lesson, EventPayload::Lesson(_))
                | (EventType::Pref, EventPayload::Pref(_))
                | (EventType::SkillEdit, EventPayload::SkillEdit(_))
                | (EventType::Verify, EventPayload::Verify(_))
                | (EventType::Archive, EventPayload::Archive(_))
                | (EventType::Redact, EventPayload::Redact(_))
                | (EventType::Import, EventPayload::Import(_))
                | (EventType::Audit, EventPayload::Audit(_))
        )
    }
}

/// The typed payload enum. Serde tag is `type` so JSON stays legible.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    Observe(ObservePayload),
    Claim(ClaimPayload),
    Lesson(LessonPayload),
    Pref(PrefPayload),
    SkillEdit(SkillEditPayload),
    Verify(VerifyPayload),
    Archive(ArchivePayload),
    Redact(RedactPayload),
    Import(ImportPayload),
    Audit(AuditPayload),
}

impl EventPayload {
    pub fn event_type(&self) -> EventType {
        match self {
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
        }
    }

    /// One-line display string for a payload, truncated to `max`
    /// characters. Consolidated here so brain-cli, brain-mcp, and
    /// brain-tui share one implementation instead of three copies
    /// that drifted.
    ///
    /// Newlines collapse to spaces. If the raw string is longer than
    /// `max`, the tail is replaced with `…` so the total is `max`
    /// chars. `max == 0` returns empty; `max == 1` returns just `…`.
    pub fn summary_line(&self, max: usize) -> String {
        let raw = match self {
            EventPayload::Observe(o) => o.summary.clone(),
            EventPayload::Claim(c) => format!("{}/{}", c.schema_type, c.key),
            EventPayload::Lesson(l) => l.text.clone(),
            EventPayload::Pref(p) => format!("{}/{}", p.category, p.key),
            EventPayload::SkillEdit(s) => format!("{} {}", s.skill_name, s.file),
            EventPayload::Verify(v) => format!("verify {}", v.target_entity),
            EventPayload::Archive(a) => format!("archive {}", a.target_entity),
            EventPayload::Redact(r) => format!("redact {}: {}", r.target_entity, r.reason),
            EventPayload::Import(i) => format!("import {} items", i.item_count),
            EventPayload::Audit(a) => format!("audit {:?}", a.kind),
        };
        let flat: String = raw
            .chars()
            .map(|c| if c == '\n' { ' ' } else { c })
            .collect();
        if max == 0 {
            return String::new();
        }
        if flat.chars().count() <= max {
            flat
        } else if max == 1 {
            "…".to_string()
        } else {
            flat.chars()
                .take(max - 1)
                .chain(std::iter::once('…'))
                .collect()
        }
    }
}

/// An append-only source observation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservePayload {
    /// Human/agent-readable title or short summary.
    pub summary: String,
    /// The actual content. Large payloads go to `content_ref` (blob).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Blake3 hex of the full content when stored as a separate blob.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_ref: Option<String>,
    /// Original source (URL, file path, tool output tag).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// A typed claim. Supersession is a field, not a separate event type.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimPayload {
    /// e.g. "drug.contraindication", "auth.library-choice".
    pub schema_type: String,
    /// Unique-per-schema key. Human-meaningful.
    pub key: String,
    /// Structured content; schema-specific validation lives in brain-schema.
    pub content: serde_json::Value,
    /// Entity id of the claim this one supersedes, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<Uuid>,
    /// Supporting observations.
    #[serde(default)]
    pub support_refs: Vec<SupportRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<DateTime<Utc>>,
}

/// A semantic-layer lesson.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LessonPayload {
    pub category: String,
    pub text: String,
    pub transition: LessonTransition,
    #[serde(default)]
    pub rank: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LessonTransition {
    Added,
    Modified,
    Retired,
    Promoted,
    Decayed,
}

/// A personal-layer preference.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrefPayload {
    pub category: String,
    pub key: String,
    pub value: serde_json::Value,
    /// Prior value if this is an update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_value: Option<serde_json::Value>,
}

/// A skill file edit (SKILL.md or KNOWLEDGE.md).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillEditPayload {
    pub skill_name: String,
    pub file: String,
    pub edit_kind: SkillEditKind,
    /// Blake3 hex of the new file content.
    pub content_hash: String,
    /// Reason for the edit (from self-rewrite hook or manual edit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillEditKind {
    Created,
    SelfRewrite,
    ManualEdit,
    ConstraintTightened,
    Retired,
}

/// A verification attestation on a target entity.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifyPayload {
    pub target_entity: Uuid,
    pub attestation: String,
    pub verdict: VerifyVerdict,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerifyVerdict {
    Confirmed,
    Refuted,
    Uncertain,
}

/// Archive an entity (or restore from archive).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchivePayload {
    pub target_entity: Uuid,
    pub direction: ArchiveDirection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveDirection {
    Archive,
    Restore,
}

/// Redaction: content removed or classification lowered.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedactPayload {
    pub target_entity: Uuid,
    pub reason: String,
    /// Pattern that triggered redaction (if automated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggered_by_pattern: Option<String>,
    /// Fields or spans that were redacted.
    #[serde(default)]
    pub redacted_spans: Vec<RedactedSpan>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedactedSpan {
    pub field: String,
    pub replacement: String,
}

/// Bulk import from an external source.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportPayload {
    pub source: ImportSource,
    pub item_count: u64,
    /// Blake3 hash of the source manifest for idempotency.
    pub manifest_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportSource {
    AgentStackFolder { path: String },
    NotionExport { path: String },
    ObsidianVault { path: String },
    ClaudeCodeProject { path: String },
    OpaqueArchive { path: String, description: String },
}

/// An operational audit record. Covers manual markdown edits, schema
/// migrations, admin actions, and drift detection by the daemon.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditPayload {
    pub kind: AuditKind,
    pub detail: String,
    /// Related entity if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_entity: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    ManualEdit,
    SchemaMigration,
    AdminAction,
    DreamCycle,
    ReindexStart,
    ReindexComplete,
    LeaseAcquired,
    LeaseReleased,
    DaemonStart,
    DaemonStop,
}
