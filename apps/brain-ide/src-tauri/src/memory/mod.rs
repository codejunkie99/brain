//! IDE memory client. Wraps `brain-store::BrainRepo` with convenience
//! methods the orchestrator and chat layers use:
//!
//!   - `note`: write an Observe event (the "remember this" surface).
//!   - `recent`: list recent events, newest first.
//!   - `search`: substring search across event text.
//!   - `record_card_event`: write a structured Observe whose summary
//!     references a CardId — used to thread card state changes into the
//!     same commit history as the user's manual notes.
//!
//! All writes are synchronous (libgit2 commits land before return) and
//! cheap enough to run on the Tauri command thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use brain_store::BrainRepo;
use brain_types::{
    Actor, ActorKind, Authority, AuthoritySource, Classification, Event, EventDraft, EventPayload,
    EventType, Layer, ObservePayload, SCHEMA_VERSION, SubjectRef,
};
use parking_lot::Mutex;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("brain dir not configured")]
    NotConfigured,
    #[error(transparent)]
    Store(#[from] brain_store::StoreError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// What the frontend sees when listing memory.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryEntry {
    pub event_id: Uuid,
    pub kind: String,
    pub summary: String,
    pub actor: String,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub commit_oid: String,
}

impl From<Event> for MemoryEntry {
    fn from(e: Event) -> Self {
        Self {
            event_id: e.event_id,
            kind: e.payload.event_type().as_str().to_string(),
            summary: e.payload.summary_line(160),
            actor: format!("{}:{}", actor_kind_label(&e.actor.kind), e.actor.id),
            recorded_at: e.time_recorded,
            commit_oid: e.commit_oid,
        }
    }
}

trait EventTypeExt {
    fn as_str(&self) -> &'static str;
}

impl EventTypeExt for EventType {
    fn as_str(&self) -> &'static str {
        match self {
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
}

fn actor_kind_label(k: &ActorKind) -> &'static str {
    match k {
        ActorKind::Agent => "agent",
        ActorKind::Human => "human",
        ActorKind::System => "system",
    }
}

/// Lazy-opened wrapper around a `BrainRepo`. The first call to `open`
/// or `note` initializes (or attaches to an existing) brain at the
/// configured dir; subsequent calls reuse the open handle.
#[derive(Clone)]
pub struct MemoryClient {
    inner: Arc<Mutex<Option<Inner>>>,
}

struct Inner {
    path: PathBuf,
    repo: BrainRepo,
}

impl Default for MemoryClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryClient {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Open or initialize the brain at `path`. Idempotent — calling it
    /// again with the same path is a no-op; with a different path it
    /// closes the current handle and opens the new one.
    pub fn open(&self, path: &Path) -> Result<PathBuf, MemoryError> {
        let mut guard = self.inner.lock();
        if let Some(inner) = guard.as_ref() {
            if inner.path == path {
                return Ok(inner.path.clone());
            }
        }
        let repo = if path.exists() && BrainRepo::open(path).is_ok() {
            BrainRepo::open(path)?
        } else {
            BrainRepo::init(path)?
        };
        let inner = Inner {
            path: path.to_path_buf(),
            repo,
        };
        let p = inner.path.clone();
        *guard = Some(inner);
        Ok(p)
    }

    pub fn is_open(&self) -> bool {
        self.inner.lock().is_some()
    }

    pub fn current_path(&self) -> Option<PathBuf> {
        self.inner.lock().as_ref().map(|i| i.path.clone())
    }

    /// Append an OBSERVE event with free text. Used for chat notes and
    /// card status timeline entries. `idempotency_key` should be stable
    /// for a given logical event so retries collapse.
    pub fn note(
        &self,
        summary: impl Into<String>,
        actor_id: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Uuid, MemoryError> {
        let guard = self.inner.lock();
        let inner = guard.as_ref().ok_or(MemoryError::NotConfigured)?;
        let draft = EventDraft {
            event_type: EventType::Observe,
            subject: SubjectRef::Entity { id: Uuid::now_v7() },
            parent_event_id: None,
            chain_id: None,
            actor: Actor {
                kind: ActorKind::Agent,
                id: actor_id.into(),
                harness: Some("brain-ide".into()),
                signing_key_fingerprint: None,
            },
            time_observed: None,
            layer: Layer::Episodic,
            authority: Authority {
                source_kind: AuthoritySource::Agent,
                score: None,
                attested_by: None,
            },
            classification: Classification::Private,
            payload: EventPayload::Observe(ObservePayload {
                summary: summary.into(),
                content: None,
                content_ref: None,
                source: Some("brain-ide".into()),
            }),
            schema_version: SCHEMA_VERSION,
            idempotency_key: idempotency_key.into(),
        };
        let ref_ = inner.repo.append_event(draft)?;
        Ok(ref_.event_id)
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<MemoryEntry>, MemoryError> {
        let guard = self.inner.lock();
        let inner = guard.as_ref().ok_or(MemoryError::NotConfigured)?;
        Ok(inner
            .repo
            .list_events(limit)?
            .into_iter()
            .map(MemoryEntry::from)
            .collect())
    }

    pub fn search(&self, needle: &str, limit: usize) -> Result<Vec<MemoryEntry>, MemoryError> {
        let guard = self.inner.lock();
        let inner = guard.as_ref().ok_or(MemoryError::NotConfigured)?;
        Ok(inner
            .repo
            .search_events(needle, limit)?
            .into_iter()
            .map(MemoryEntry::from)
            .collect())
    }
}
