//! Codex agent. Mirrors the Claude module but talks to the OpenAI
//! Responses API for chat and `codex` CLI for cards.

use std::process::Stdio;

use async_trait::async_trait;
use brain_orchestrator::Card;
use futures::StreamExt;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::auth::{AuthStore, CredentialKind};
use crate::settings::CodexSettings;

use super::claude::render_card_prompt;
use super::transcript::ChatRole;
use super::types::{
    AgentDescriptor, AgentError, AgentKind, CardAgent, CardExecution, CardOutcome,
    CardOutputChunk, ChatAgent, ChatChunk, ChatRequest,
};

const OPENAI_RESPONSES_API: &str = "https://api.openai.com/v1/responses";

pub const AVAILABLE_CODEX_MODELS: &[&str] = &[
    "gpt-5",
    "gpt-5-mini",
    "o4-mini",
    "gpt-4.1",
];

pub struct CodexChat {
    settings: CodexSettings,
}

impl CodexChat {
    pub fn new(settings: CodexSettings) -> Self {
        Self { settings }
    }
    fn api_key(&self) -> Option<String> {
        AuthStore::get(CredentialKind::Openai).ok().flatten()
    }
}

#[async_trait]
impl ChatAgent for CodexChat {
    fn descriptor(&self) -> AgentDescriptor {
        let has_key = self.api_key().is_some();
        AgentDescriptor {
            slug: "codex".into(),
            label: "Codex".into(),
            kind: AgentKind::Api,
            default_model: Some(self.settings.default_model.clone()),
            available_models: AVAILABLE_CODEX_MODELS.iter().map(|s| s.to_string()).collect(),
            supports_chat: true,
            supports_card_execution: false,
            ready: has_key,
            readiness_message: if has_key {
                None
            } else {
                Some("Set OPENAI_API_KEY in Settings → API keys".into())
            },
        }
    }

    async fn stream(&self, req: ChatRequest) -> Result<super::types::ChatStream, AgentError> {
        let key = self
            .api_key()
            .ok_or_else(|| AgentError::NotConfigured("codex".into()))?;
        let model = req
            .model
            .clone()
            .unwrap_or_else(|| self.settings.default_model.clone());

        let mut input: Vec<serde_json::Value> = Vec::new();
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
        input.push(json!({
            "role": "system",
            "content": [{ "type": "input_text", "text": system }],
        }));
        for m in &req.messages {
            let role = match m.role {
                ChatRole::User => "user",
                ChatRole::Assistant => "assistant",
                ChatRole::Tool | ChatRole::System => continue,
            };
            let text_kind = if matches!(m.role, ChatRole::Assistant) {
                "output_text"
            } else {
                "input_text"
            };
            input.push(json!({
                "role": role,
                "content": [{ "type": text_kind, "text": m.content }],
            }));
        }

        let body = json!({
            "model": model,
            "input": input,
            "stream": true,
            "max_output_tokens": req.max_tokens.unwrap_or(4096),
            "temperature": req.temperature.unwrap_or(0.7),
        });

        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| AgentError::Network(e.to_string()))?;
        let resp = client
            .post(OPENAI_RESPONSES_API)
            .bearer_auth(key)
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
            openai_event_to_chunk(&ev.event, &ev.data)
        });

        Ok(Box::pin(chunks))
    }
}

fn default_system_prompt(project: Option<&str>) -> String {
    let mut s = String::from(
        "You are Codex inside Brain IDE — a multi-agent IDE where you collaborate with Claude and shell sessions on a shared brain memory layer. Be terse, show patches in unified diff or fenced code blocks, and assume the reader is a senior engineer.",
    );
    if let Some(root) = project {
        s.push_str(&format!("\n\nActive project root: {root}"));
    }
    s
}

fn openai_event_to_chunk(event_name: &str, data: &str) -> Option<ChatChunk> {
    // OpenAI Responses streaming events of interest:
    //   - response.output_text.delta { delta }
    //   - response.reasoning_summary_text.delta { delta }
    //   - response.completed { response.usage }
    //   - response.error { error.message }
    if data == "[DONE]" {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    let kind = event_name;
    match kind {
        "response.output_text.delta" => {
            let text = value.get("delta")?.as_str()?.to_string();
            Some(ChatChunk::Delta { text })
        }
        "response.reasoning_summary_text.delta"
        | "response.reasoning.delta"
        | "response.thinking.delta" => {
            let text = value
                .get("delta")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string())?;
            Some(ChatChunk::Thinking { text })
        }
        "response.completed" => {
            let usage = value.get("response")?.get("usage")?;
            Some(ChatChunk::Done {
                input_tokens: usage.get("input_tokens").and_then(|v| v.as_u64()).map(|v| v as u32),
                output_tokens: usage
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                cached_tokens: usage
                    .get("input_tokens_details")
                    .and_then(|d| d.get("cached_tokens"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                stop_reason: value
                    .get("response")
                    .and_then(|r| r.get("status"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            })
        }
        "response.error" | "error" => {
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
// Codex CLI runner.
// ---------------------------------------------------------------------------

pub struct CodexCli {
    settings: CodexSettings,
}

impl CodexCli {
    pub fn new(settings: CodexSettings) -> Self {
        Self { settings }
    }
    fn binary(&self) -> String {
        self.settings
            .binary_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "codex".into())
    }
}

#[async_trait]
impl CardAgent for CodexCli {
    fn descriptor(&self) -> AgentDescriptor {
        let bin = self.binary();
        let ready = crate::agents::claude_which(&bin);
        AgentDescriptor {
            slug: "codex".into(),
            label: "Codex (CLI)".into(),
            kind: AgentKind::Cli,
            default_model: Some(self.settings.default_model.clone()),
            available_models: AVAILABLE_CODEX_MODELS.iter().map(|s| s.to_string()).collect(),
            supports_chat: false,
            supports_card_execution: true,
            ready,
            readiness_message: if ready {
                None
            } else {
                Some(format!(
                    "Install Codex CLI (`brew install codex` or set codex.binary_path in Settings). Looked for: {bin}"
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
        cmd.arg("exec")
            .arg("--model")
            .arg(&model)
            .arg("--sandbox")
            .arg("workspace-write");
        for extra in &self.settings.extra_args {
            cmd.arg(extra);
        }
        cmd.arg(prompt);
        cmd.current_dir(project_root)
            .env("BRAIN_IDE_CARD", card.id.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| AgentError::Other(format!("spawn codex: {e}")))?;
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
                    reason: format!("codex exited with {status}"),
                },
                Err(e) => CardOutcome::Failed {
                    reason: format!("wait failed: {e}"),
                },
            }
        };

        Ok(CardExecution {
            card_id,
            pty_session: None,
            output: Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx)),
            outcome: Box::pin(outcome),
        })
    }
}
