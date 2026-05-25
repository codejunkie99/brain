//! Chat commands. Each chat turn is streamed via Tauri events on the
//! `chat://<thread_id>/chunk` channel; the command itself returns once
//! the stream completes (or errors).

use dashmap::DashMap;
use futures::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::agents::transcript::{ChatMessage, ChatThread};
use crate::agents::types::{AgentDescriptor, ChatChunk, ChatRequest};
use crate::state::AppState;

/// In-flight stream tokens so `cancel_stream` can interrupt them.
static IN_FLIGHT: Lazy<DashMap<Uuid, CancellationToken>> = Lazy::new(DashMap::new);

#[derive(Debug, Clone, Serialize)]
pub struct ChatStreamEvent {
    pub thread_id: Uuid,
    pub assistant_message_id: Uuid,
    pub chunk: ChatChunk,
}

#[tauri::command]
pub fn list_threads(state: State<'_, AppState>) -> Vec<ChatThread> {
    state.transcripts.list()
}

#[tauri::command]
pub fn create_thread(state: State<'_, AppState>, title: String) -> ChatThread {
    state.transcripts.create(title)
}

#[tauri::command]
pub fn delete_thread(state: State<'_, AppState>, id: Uuid) {
    state.transcripts.delete(id);
}

#[tauri::command]
pub fn rename_thread(
    state: State<'_, AppState>,
    id: Uuid,
    title: String,
) -> Option<ChatThread> {
    state.transcripts.rename(id, title)
}

#[derive(Debug, Deserialize)]
pub struct SendMessageInput {
    pub thread_id: Uuid,
    pub agent: String,
    pub model: Option<String>,
    pub text: String,
    pub system: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SendMessageOutput {
    pub thread_id: Uuid,
    pub user_message_id: Uuid,
    pub assistant_message_id: Uuid,
}

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    input: SendMessageInput,
) -> Result<SendMessageOutput, String> {
    let agent = {
        let registry = state.agents.read();
        registry
            .chat(&input.agent)
            .ok_or_else(|| format!("no chat agent for {}", input.agent))?
    };

    // Append user turn.
    let user = ChatMessage::user(input.text.clone());
    let thread = state
        .transcripts
        .append(input.thread_id, user.clone())
        .ok_or_else(|| format!("thread {} not found", input.thread_id))?;

    let project_root = state.orchestrator.project_root();
    let req = ChatRequest {
        model: input.model.clone(),
        messages: thread.messages.clone(),
        system: input.system.clone(),
        project_root,
        max_tokens: None,
        temperature: None,
    };

    // Allocate the assistant message up front so the frontend can target it.
    let assistant = ChatMessage::assistant(String::new(), input.model.clone());
    let assistant_id = assistant.id;
    let _ = state
        .transcripts
        .append(input.thread_id, assistant.clone());

    let thread_id = input.thread_id;
    let stream = agent
        .stream(req)
        .await
        .map_err(|e| e.to_string())?;
    let token = CancellationToken::new();
    IN_FLIGHT.insert(assistant_id, token.clone());

    let transcripts = state.transcripts.clone();
    let app_for_pump = app.clone();
    tokio::spawn(async move {
        let mut stream = stream;
        let mut accumulated = String::new();
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    let _ = app_for_pump.emit(
                        &format!("chat://{thread_id}/chunk"),
                        ChatStreamEvent {
                            thread_id,
                            assistant_message_id: assistant_id,
                            chunk: ChatChunk::Done {
                                input_tokens: None,
                                output_tokens: None,
                                cached_tokens: None,
                                stop_reason: Some("cancelled".into()),
                            },
                        },
                    );
                    break;
                }
                next = stream.next() => {
                    match next {
                        Some(chunk) => {
                            if let ChatChunk::Delta { text } = &chunk {
                                accumulated.push_str(text);
                                let _ = transcripts.update_message(
                                    thread_id,
                                    assistant_id,
                                    accumulated.clone(),
                                );
                            }
                            let _ = app_for_pump.emit(
                                &format!("chat://{thread_id}/chunk"),
                                ChatStreamEvent {
                                    thread_id,
                                    assistant_message_id: assistant_id,
                                    chunk,
                                },
                            );
                        }
                        None => break,
                    }
                }
            }
        }
        IN_FLIGHT.remove(&assistant_id);
    });

    Ok(SendMessageOutput {
        thread_id,
        user_message_id: user.id,
        assistant_message_id: assistant_id,
    })
}

#[tauri::command]
pub fn cancel_stream(assistant_message_id: Uuid) {
    if let Some((_, token)) = IN_FLIGHT.remove(&assistant_message_id) {
        token.cancel();
    }
}

#[tauri::command]
pub fn list_agents(state: State<'_, AppState>) -> Vec<AgentDescriptor> {
    state.agents.read().descriptors()
}
