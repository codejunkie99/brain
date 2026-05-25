//! Chat transcript. The same transcript persists across model switches —
//! the user can type a question to Claude, switch to Codex, and ask
//! "what did Claude say?" without losing context.
//!
//! Transcripts are stored in-memory; they're committed to brain as
//! Observe events when the user explicitly saves or when a card execution
//! references them. Ephemeral chat doesn't pollute long-term memory.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
    System,
    /// A response from a tool call the assistant requested. The IDE
    /// pretty-prints these inline below the assistant turn.
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub role: ChatRole,
    pub content: String,
    /// Which model produced the message, if it was an assistant turn.
    /// Lets the UI render "via Claude Opus 4.7" vs "via Codex".
    pub model: Option<String>,
    /// Free-form metadata (tool_use blocks, citations, latency).
    pub meta: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl ChatMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self::new(ChatRole::User, text.into(), None)
    }
    pub fn assistant(text: impl Into<String>, model: Option<String>) -> Self {
        Self::new(ChatRole::Assistant, text.into(), model)
    }
    pub fn system(text: impl Into<String>) -> Self {
        Self::new(ChatRole::System, text.into(), None)
    }
    pub fn tool(text: impl Into<String>) -> Self {
        Self::new(ChatRole::Tool, text.into(), None)
    }

    fn new(role: ChatRole, content: String, model: Option<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            role,
            content,
            model,
            meta: serde_json::Value::Null,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThread {
    pub id: Uuid,
    pub title: String,
    pub messages: Vec<ChatMessage>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ChatThread {
    pub fn new(title: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            title: title.into(),
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Clone, Default)]
pub struct ChatTranscript {
    inner: Arc<RwLock<Vec<ChatThread>>>,
}

impl ChatTranscript {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn list(&self) -> Vec<ChatThread> {
        self.inner.read().clone()
    }

    pub fn get(&self, id: Uuid) -> Option<ChatThread> {
        self.inner.read().iter().find(|t| t.id == id).cloned()
    }

    pub fn create(&self, title: impl Into<String>) -> ChatThread {
        let thread = ChatThread::new(title);
        self.inner.write().push(thread.clone());
        thread
    }

    pub fn append(&self, id: Uuid, message: ChatMessage) -> Option<ChatThread> {
        let mut guard = self.inner.write();
        let thread = guard.iter_mut().find(|t| t.id == id)?;
        thread.messages.push(message);
        thread.updated_at = Utc::now();
        Some(thread.clone())
    }

    pub fn delete(&self, id: Uuid) {
        self.inner.write().retain(|t| t.id != id);
    }

    pub fn rename(&self, id: Uuid, title: impl Into<String>) -> Option<ChatThread> {
        let mut guard = self.inner.write();
        let thread = guard.iter_mut().find(|t| t.id == id)?;
        thread.title = title.into();
        thread.updated_at = Utc::now();
        Some(thread.clone())
    }

    /// Patch an existing message in-place. Used by the streaming chat
    /// path to keep the persisted transcript in sync with the deltas
    /// being emitted to the frontend.
    pub fn update_message(
        &self,
        thread_id: Uuid,
        message_id: Uuid,
        new_content: String,
    ) -> Option<ChatMessage> {
        let mut guard = self.inner.write();
        let thread = guard.iter_mut().find(|t| t.id == thread_id)?;
        let message = thread.messages.iter_mut().find(|m| m.id == message_id)?;
        message.content = new_content;
        thread.updated_at = Utc::now();
        Some(message.clone())
    }
}
