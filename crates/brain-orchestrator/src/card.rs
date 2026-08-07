//! Feature cards: the unit of work in the orchestrator graph.
//!
//! A `Card` represents one feature, refactor, bug fix, or research
//! task. It is owned by an agent assignment (Claude / Codex / shell /
//! human), declares dependencies on other cards, and carries enough
//! context for the assigned agent to execute it from a cold start.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier for a card. UUIDv7 so cards sort by creation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardId(pub Uuid);

impl CardId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for CardId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CardId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What kind of work the card represents. The kind drives the default
/// prompt template the spec parser writes into the card and the icon
/// the UI shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardKind {
    Feature,
    Refactor,
    BugFix,
    Research,
    Test,
    Docs,
    Setup,
}

impl CardKind {
    pub fn label(self) -> &'static str {
        match self {
            CardKind::Feature => "Feature",
            CardKind::Refactor => "Refactor",
            CardKind::BugFix => "Bug fix",
            CardKind::Research => "Research",
            CardKind::Test => "Test",
            CardKind::Docs => "Docs",
            CardKind::Setup => "Setup",
        }
    }
}

/// Lifecycle of a single card.
///
/// Transitions:
///   Pending -> Ready -> Running -> {Succeeded | Failed | Cancelled}
///   any -> Blocked (when a dep transitions to Failed/Cancelled)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardStatus {
    /// Created, dependencies not yet satisfied.
    Pending,
    /// All deps satisfied, waiting for an agent slot.
    Ready,
    /// An agent has claimed the card and is executing it.
    Running,
    /// Agent reported success.
    Succeeded,
    /// Agent reported failure. Downstream cards become Blocked.
    Failed,
    /// User or scheduler cancelled. Downstream cards become Blocked.
    Cancelled,
    /// A transitive dependency failed; this card cannot run.
    Blocked,
}

impl CardStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            CardStatus::Succeeded
                | CardStatus::Failed
                | CardStatus::Cancelled
                | CardStatus::Blocked
        )
    }

    pub fn is_success(self) -> bool {
        matches!(self, CardStatus::Succeeded)
    }
}

/// A single card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: CardId,
    pub title: String,
    pub summary: String,
    pub kind: CardKind,
    pub status: CardStatus,
    /// The agent slot this card is bound to, if any.
    /// Free-form (e.g. "claude", "codex", "shell:repl"); the agent
    /// registry resolves it to a concrete runner. None means unassigned.
    pub assigned_agent: Option<String>,
    /// The model the agent should use ("claude-opus-4-7", "gpt-5", ...).
    /// Optional override; otherwise the agent picks its default.
    pub model: Option<String>,
    /// Extra context the spec parser captured (files to touch, acceptance
    /// criteria, etc). Free-form JSON so the UI can render it without
    /// needing schema migrations every time we change a template.
    pub context: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Card {
    pub fn new(title: impl Into<String>, summary: impl Into<String>, kind: CardKind) -> Self {
        let now = Utc::now();
        Self {
            id: CardId::new(),
            title: title.into(),
            summary: summary.into(),
            kind,
            status: CardStatus::Pending,
            assigned_agent: None,
            model: None,
            context: serde_json::Value::Null,
            created_at: now,
            updated_at: now,
        }
    }
}

/// How one card relates to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// `from` must succeed before `to` becomes Ready.
    DependsOn,
    /// `to` was discovered while planning `from`; informational only.
    DiscoveredBy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: CardId,
    pub to: CardId,
    pub kind: EdgeKind,
}
