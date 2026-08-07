//! Core agent traits.

use std::pin::Pin;

use async_trait::async_trait;
use brain_orchestrator::Card;
use futures::Stream;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::transcript::ChatMessage;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("agent {0} is not configured (missing binary or API key)")]
    NotConfigured(String),
    #[error("network: {0}")]
    Network(String),
    #[error("model returned no content")]
    Empty,
    #[error("subprocess exited with status {0:?}")]
    Subprocess(Option<i32>),
    #[error("{0}")]
    Other(String),
}

/// Static metadata about an agent slug. Powers the model picker and
/// settings UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDescriptor {
    pub slug: String,
    pub label: String,
    pub kind: AgentKind,
    pub default_model: Option<String>,
    pub available_models: Vec<String>,
    pub supports_chat: bool,
    pub supports_card_execution: bool,
    /// True if the agent is ready to use (API key set / binary present).
    pub ready: bool,
    pub readiness_message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// Direct API client (Anthropic Messages, OpenAI Responses, ...).
    Api,
    /// Spawned CLI subprocess (`claude`, `codex`, shell).
    Cli,
    /// A human assignee — the card just sits Ready until manually marked.
    Human,
}

/// Request for a chat turn. The IDE sends the full transcript every
/// turn (cheap for the chat-pane scale we care about and keeps the
/// agent stateless).
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    /// Optional system prompt; defaults to the IDE's standard one.
    pub system: Option<String>,
    /// Hints about the current project so the model knows what to assume.
    pub project_root: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

/// One streamed delta from a chat turn.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatChunk {
    Delta { text: String },
    /// Streamed thinking — surfaced as collapsed reasoning in the UI.
    Thinking { text: String },
    /// Tool/function call request the agent wants the IDE to fulfill.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Final cumulative usage info (tokens, cost, model).
    Done {
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
        cached_tokens: Option<u32>,
        stop_reason: Option<String>,
    },
    Error { message: String },
}

pub type ChatStream = Pin<Box<dyn Stream<Item = ChatChunk> + Send>>;

#[async_trait]
pub trait ChatAgent: Send + Sync {
    fn descriptor(&self) -> AgentDescriptor;
    async fn stream(&self, req: ChatRequest) -> Result<ChatStream, AgentError>;
}

/// One streamed chunk of card execution output. Plumbed to the card's
/// PTY tab and a status-pill animation.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CardOutputChunk {
    /// Raw bytes from the subprocess (base64-encoded). Rendered to the
    /// terminal tab via xterm.js.
    Bytes { b64: String },
    /// A high-level status update ("Reading src/foo.rs", "Editing
    /// 3 files"). Surfaced on the card pill itself.
    Status { text: String },
    /// Persistent log line — shown in a side-panel transcript.
    Log { text: String },
    /// Suggested follow-up cards the agent discovered while working.
    Discovered {
        title: String,
        summary: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardOutcome {
    Succeeded { commit_oid: Option<String> },
    Failed { reason: String },
    Cancelled,
}

/// Active execution handle returned by `CardAgent::execute`. Drop to
/// cancel.
pub struct CardExecution {
    pub card_id: Uuid,
    pub pty_session: Option<Uuid>,
    pub output:
        Pin<Box<dyn Stream<Item = CardOutputChunk> + Send>>,
    pub outcome: Pin<Box<dyn std::future::Future<Output = CardOutcome> + Send>>,
}

#[async_trait]
pub trait CardAgent: Send + Sync {
    fn descriptor(&self) -> AgentDescriptor;
    /// Begin executing the card. Returns immediately with the streaming
    /// handle; the scheduler awaits `outcome` on a background task.
    async fn execute(
        &self,
        card: &Card,
        project_root: &str,
    ) -> Result<CardExecution, AgentError>;
}
