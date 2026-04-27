//! Subjects, actors, authority, layers, classification, signing state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What an event is about. Not every event has a single entity.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubjectRef {
    /// A specific entity (claim, lesson, skill, preference, observation).
    Entity { id: Uuid },
    /// A batch operation (IMPORT, bulk admin).
    Batch { id: Uuid },
    /// A global, repo-level operation (schema migration, brain-level action).
    Brain,
}

/// The writer of an event.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Actor {
    pub kind: ActorKind,
    /// Stable identifier: model name, user email, "brain-daemon".
    pub id: String,
    /// Harness name for Agent actors: "claude-desktop", "cursor", etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// SSH key fingerprint if the event was signed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_key_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Agent,
    Human,
    System,
}

/// Authority: who attested this event and how much weight it carries.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Authority {
    pub source_kind: AuthoritySource,
    /// 0..=100 if ranking matters. None means unranked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<u8>,
    /// Identifier of the attesting party (regulator name, org, user).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attested_by: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthoritySource {
    Regulator,
    Doctor,
    Lawyer,
    SeniorEngineer,
    Agent,
    Intern,
    User,
    System,
    /// Catch-all for domain-specific authorities defined in authority.yaml.
    Other,
}

/// Which memory layer an event belongs to.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Working,
    Episodic,
    Semantic,
    Personal,
    Skill,
    Protocol,
}

/// Signature state at the time of commit.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignatureState {
    Unsigned,
    Signed,
    SigningRequired,
}

/// Classification for redaction + safe-export.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Public,
    #[default]
    Private,
    Secret,
}

/// A reference from a claim to its supporting observation(s).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupportRef {
    pub observation_event_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noted_at: Option<DateTime<Utc>>,
}
