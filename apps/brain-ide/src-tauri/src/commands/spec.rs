//! Spec parsing — either heuristic (offline) or via an LLM agent.

use brain_orchestrator::{
    CardId, SpecBreakdown,
    spec::{HeuristicSpecParser, SpecParser, parse_llm_envelope, spec_breakdown_prompt},
};
use futures::StreamExt;
use serde::Deserialize;
use tauri::State;

use crate::agents::transcript::ChatMessage;
use crate::agents::types::{ChatChunk, ChatRequest};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ParseSpecInput {
    pub spec: String,
    /// "heuristic", or an agent slug ("claude", "codex") to use the LLM
    /// path. Defaults to "heuristic".
    pub backend: Option<String>,
    pub model: Option<String>,
}

#[tauri::command]
pub async fn parse_spec(
    state: State<'_, AppState>,
    input: ParseSpecInput,
) -> Result<SpecBreakdown, String> {
    let backend = input.backend.unwrap_or_else(|| "heuristic".into());
    if backend == "heuristic" {
        return HeuristicSpecParser
            .parse(&input.spec)
            .await
            .map_err(|e| e.to_string());
    }
    // LLM path: build a chat request, accumulate the streamed text,
    // then parse the strict JSON envelope.
    let agent = {
        let registry = state.agents.read();
        registry
            .chat(&backend)
            .ok_or_else(|| format!("no chat agent for {backend}"))?
    };
    let prompt = spec_breakdown_prompt(&input.spec);
    let req = ChatRequest {
        model: input.model,
        messages: vec![ChatMessage::user(prompt)],
        system: Some(
            "You translate product specs into DAGs of feature cards. Reply with strict JSON only."
                .into(),
        ),
        project_root: state.orchestrator.project_root(),
        max_tokens: Some(4096),
        temperature: Some(0.2),
    };
    let mut stream = agent.stream(req).await.map_err(|e| e.to_string())?;
    let mut accumulated = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            ChatChunk::Delta { text } => accumulated.push_str(&text),
            ChatChunk::Error { message } => return Err(message),
            ChatChunk::Done { .. } => break,
            _ => {}
        }
    }

    // Strip ``` fences if the model wrapped the JSON anyway.
    let cleaned = strip_code_fence(&accumulated);
    parse_llm_envelope(&cleaned).map_err(|e| {
        format!(
            "failed to parse model output as breakdown: {e}\n---\n{}",
            accumulated.chars().take(2000).collect::<String>()
        )
    })
}

#[derive(Debug, Deserialize)]
pub struct ApplyBreakdownInput {
    pub breakdown: SpecBreakdown,
    /// Optional default agent slug to stamp on every unassigned card.
    pub default_agent: Option<String>,
    pub default_model: Option<String>,
}

#[tauri::command]
pub fn apply_breakdown(
    state: State<'_, AppState>,
    input: ApplyBreakdownInput,
) -> Vec<CardId> {
    let mut breakdown = input.breakdown;
    if input.default_agent.is_some() || input.default_model.is_some() {
        for card in &mut breakdown.cards {
            if card.assigned_agent.is_none() {
                card.assigned_agent = input.default_agent.clone();
            }
            if card.model.is_none() {
                card.model = input.default_model.clone();
            }
        }
    }
    state.orchestrator.scheduler.apply_breakdown(breakdown)
}

fn strip_code_fence(s: &str) -> String {
    let t = s.trim();
    let stripped = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let stripped = stripped.trim_start_matches('\n');
    let stripped = stripped.strip_suffix("```").unwrap_or(stripped);
    stripped.trim().to_string()
}
