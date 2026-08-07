//! Claude agent — Anthropic Messages API for chat, `claude` CLI for cards.

use std::process::Stdio;

use async_trait::async_trait;
use brain_orchestrator::Card;
use futures::{StreamExt, stream};
use serde::Serialize;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::auth::{AuthStore, CredentialKind};
use crate::settings::ClaudeSettings;

use super::transcript::ChatRole;
use super::types::{
    AgentDescriptor, AgentError, AgentKind, CardAgent, CardExecution, CardOutcome,
    CardOutputChunk, ChatAgent, ChatChunk, ChatRequest,
};

const ANTHROPIC_API: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub const AVAILABLE_CLAUDE_MODELS: &[&str] = &[
    "claude-opus-4-7",
    "claude-sonnet-4-6",
    "claude-haiku-4-5-20251001",
];

pub struct ClaudeChat {
    settings: ClaudeSettings,
}

impl ClaudeChat {
    pub fn new(settings: ClaudeSettings) -> Self {
        Self { settings }
    }

    fn api_key(&self) -> Option<String> {
        AuthStore::get(CredentialKind::Anthropic).ok().flatten()
    }
}

#[async_trait]
impl ChatAgent for ClaudeChat {
    fn descriptor(&self) -> AgentDescriptor {
        let has_key = self.api_key().is_some();
        AgentDescriptor {
            slug: "claude".into(),
            label: "Claude".into(),
            kind: AgentKind::Api,
            default_model: Some(self.settings.default_model.clone()),
            available_models: AVAILABLE_CLAUDE_MODELS.iter().map(|s| s.to_string()).collect(),
            supports_chat: true,
            supports_card_execution: false,
            ready: has_key,
            readiness_message: if has_key {
                None
            } else {
                Some("Set ANTHROPIC_API_KEY in Settings → API keys".into())
            },
        }
    }

    async fn stream(&self, req: ChatRequest) -> Result<super::types::ChatStream, AgentError> {
        let key = self
            .api_key()
            .ok_or_else(|| AgentError::NotConfigured("claude".into()))?;
        let model = req
            .model
            .clone()
            .unwrap_or_else(|| self.settings.default_model.clone());

        let body = build_anthropic_request(&model, &req);

        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| AgentError::Network(e.to_string()))?;

        let resp = client
            .post(ANTHROPIC_API)
            .header("x-api-key", &key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AgentError::Network(format!("HTTP {status}: {text}")));
        }

        let byte_stream = resp.bytes_stream().map(|r| {
            r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        });

        use eventsource_stream::Eventsource;
        let events = byte_stream.eventsource();

        let chunks = events.filter_map(|ev| async move {
            let ev = ev.ok()?;
            anthropic_event_to_chunk(&ev.event, &ev.data)
        });

        Ok(Box::pin(chunks))
    }
}

#[derive(Serialize)]
struct AnthropicMsg<'a> {
    role: &'a str,
    content: &'a str,
}

fn build_anthropic_request(model: &str, req: &ChatRequest) -> serde_json::Value {
    let mut messages: Vec<AnthropicMsg> = Vec::new();
    for m in &req.messages {
        let role = match m.role {
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
            // System messages go into the top-level `system` field below;
            // skip them in the messages array.
            ChatRole::System | ChatRole::Tool => continue,
        };
        messages.push(AnthropicMsg {
            role,
            content: &m.content,
        });
    }
    let system = req
        .system
        .clone()
        .or_else(|| {
            req.messages
                .iter()
                .find(|m| m.role == ChatRole::System)
                .map(|m| m.content.clone())
        })
        .unwrap_or_else(|| default_system_prompt(req.project_root.as_deref()));

    json!({
        "model": model,
        "max_tokens": req.max_tokens.unwrap_or(4096),
        "temperature": req.temperature.unwrap_or(0.7),
        "system": system,
        "messages": messages,
        "stream": true,
    })
}

fn default_system_prompt(project: Option<&str>) -> String {
    let mut s = String::from(
        "You are Brain IDE's pair-programming model. The user is working in a multi-agent IDE where you, Codex, and shell sessions execute feature cards in parallel against a shared brain memory layer. Be concise, plan with files and code blocks, and prefer concrete patches.",
    );
    if let Some(root) = project {
        s.push_str(&format!("\n\nActive project root: {root}"));
    }
    s
}

fn anthropic_event_to_chunk(event_name: &str, data: &str) -> Option<ChatChunk> {
    // Anthropic SSE events we care about:
    //   - content_block_delta with delta.type == text_delta
    //   - content_block_delta with delta.type == thinking_delta
    //   - message_delta with usage
    //   - message_stop
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    match event_name {
        "content_block_delta" => {
            let delta = value.get("delta")?;
            let kind = delta.get("type").and_then(|v| v.as_str())?;
            match kind {
                "text_delta" => {
                    let text = delta.get("text")?.as_str()?.to_string();
                    Some(ChatChunk::Delta { text })
                }
                "thinking_delta" => {
                    let text = delta.get("thinking")?.as_str()?.to_string();
                    Some(ChatChunk::Thinking { text })
                }
                _ => None,
            }
        }
        "message_delta" => {
            let usage = value.get("usage")?;
            Some(ChatChunk::Done {
                input_tokens: usage.get("input_tokens").and_then(|v| v.as_u64()).map(|v| v as u32),
                output_tokens: usage
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                cached_tokens: usage
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                stop_reason: value
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            })
        }
        "error" => {
            let msg = value
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
                .to_string();
            Some(ChatChunk::Error { message: msg })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// CLI runner: spawn `claude` with the card's prompt and stream output.
// ---------------------------------------------------------------------------

pub struct ClaudeCli {
    settings: ClaudeSettings,
}

impl ClaudeCli {
    pub fn new(settings: ClaudeSettings) -> Self {
        Self { settings }
    }

    fn binary(&self) -> String {
        self.settings
            .binary_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "claude".into())
    }
}

#[async_trait]
impl CardAgent for ClaudeCli {
    fn descriptor(&self) -> AgentDescriptor {
        // CLI readiness check: try `which claude` once at descriptor time
        // (it's only consulted by the model picker; cheap and acceptable).
        let bin = self.binary();
        let ready = which_exists(&bin);
        AgentDescriptor {
            slug: "claude".into(),
            label: "Claude (CLI)".into(),
            kind: AgentKind::Cli,
            default_model: Some(self.settings.default_model.clone()),
            available_models: AVAILABLE_CLAUDE_MODELS.iter().map(|s| s.to_string()).collect(),
            supports_chat: false,
            supports_card_execution: true,
            ready,
            readiness_message: if ready {
                None
            } else {
                Some(format!(
                    "Install Claude Code (`brew install claude-code` or set claude.binary_path in Settings). Looked for: {bin}"
                ))
            },
        }
    }

    async fn execute(
        &self,
        card: &Card,
        project_root: &str,
    ) -> Result<CardExecution, AgentError> {
        let prompt = render_card_prompt(card);
        let model = card
            .model
            .clone()
            .unwrap_or_else(|| self.settings.default_model.clone());

        let mut cmd = Command::new(self.binary());
        cmd.arg("--print")
            .arg("--output-format")
            .arg("text")
            .arg("--model")
            .arg(&model);
        for extra in &self.settings.extra_args {
            cmd.arg(extra);
        }
        cmd.arg(prompt);
        cmd.current_dir(project_root)
            .env("BRAIN_IDE_CARD", card.id.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            AgentError::Other(format!("failed to spawn `{}`: {e}", self.binary()))
        })?;

        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<CardOutputChunk>();
        let tx2 = tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = tx2.send(CardOutputChunk::Log { text: line });
            }
        });
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = tx.send(CardOutputChunk::Log {
                    text: format!("⚠ {line}"),
                });
            }
        });

        let card_id = card.id.0;
        let outcome = async move {
            match child.wait().await {
                Ok(status) if status.success() => CardOutcome::Succeeded { commit_oid: None },
                Ok(status) => CardOutcome::Failed {
                    reason: format!("claude exited with {status}"),
                },
                Err(e) => CardOutcome::Failed {
                    reason: format!("wait failed: {e}"),
                },
            }
        };

        let output =
            tokio_stream::wrappers::UnboundedReceiverStream::new(rx);

        Ok(CardExecution {
            card_id,
            pty_session: None,
            output: Box::pin(output),
            outcome: Box::pin(outcome),
        })
    }
}

fn which_exists(bin: &str) -> bool {
    if bin.contains('/') {
        return std::path::Path::new(bin).exists();
    }
    if let Ok(paths) = std::env::var("PATH") {
        for p in std::env::split_paths(&paths) {
            if p.join(bin).exists() {
                return true;
            }
        }
    }
    false
}

pub fn render_card_prompt(card: &Card) -> String {
    let context = serde_json::to_string_pretty(&card.context).unwrap_or_default();
    format!(
        "Card: {title}\nKind: {kind}\n\nSummary:\n{summary}\n\nContext (JSON):\n{context}\n\nExecute this card end to end. Follow conventions in the repo. Commit when work is shippable.",
        title = card.title,
        kind = card.kind.label(),
        summary = card.summary,
    )
}

// Quiet the unused-import lint in case `stream` ever goes idle.
#[allow(dead_code)]
fn _unused() {
    let _ = stream::empty::<()>();
}
