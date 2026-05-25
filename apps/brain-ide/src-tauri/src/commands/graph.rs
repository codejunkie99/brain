//! Feature-graph commands.

use brain_orchestrator::{
    Card, CardId, CardKind, CardStatus, EdgeKind, graph::GraphSnapshot,
};
use serde::Deserialize;
use tauri::State;
use uuid::Uuid;

use crate::state::AppState;

#[tauri::command]
pub fn graph_snapshot(state: State<'_, AppState>) -> GraphSnapshot {
    state.orchestrator.scheduler.snapshot()
}

#[derive(Debug, Deserialize)]
pub struct AddCardInput {
    pub title: String,
    pub summary: String,
    pub kind: Option<String>,
    pub assigned_agent: Option<String>,
    pub model: Option<String>,
    pub depends_on: Option<Vec<Uuid>>,
}

#[tauri::command]
pub fn add_card(state: State<'_, AppState>, input: AddCardInput) -> Result<Card, String> {
    let kind = input
        .kind
        .as_deref()
        .map(parse_kind)
        .unwrap_or(CardKind::Feature);
    let mut card = Card::new(input.title, input.summary, kind);
    card.assigned_agent = input.assigned_agent;
    card.model = input.model;
    let id = state
        .orchestrator
        .scheduler
        .add_card(card)
        .map_err(|e| e.to_string())?;
    if let Some(deps) = input.depends_on {
        for dep in deps {
            state
                .orchestrator
                .scheduler
                .add_edge(CardId(dep), id, EdgeKind::DependsOn)
                .map_err(|e| e.to_string())?;
        }
    }
    state
        .orchestrator
        .scheduler
        .get_card(id)
        .ok_or_else(|| "card vanished after insert".into())
}

#[tauri::command]
pub fn remove_card(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    state
        .orchestrator
        .scheduler
        .remove_card(CardId(id))
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct AddEdgeInput {
    pub from: Uuid,
    pub to: Uuid,
    #[serde(default = "default_edge_kind")]
    pub kind: String,
}

fn default_edge_kind() -> String {
    "depends_on".into()
}

#[tauri::command]
pub fn add_edge(state: State<'_, AppState>, input: AddEdgeInput) -> Result<(), String> {
    let kind = match input.kind.as_str() {
        "discovered_by" => EdgeKind::DiscoveredBy,
        _ => EdgeKind::DependsOn,
    };
    state
        .orchestrator
        .scheduler
        .add_edge(CardId(input.from), CardId(input.to), kind)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct AssignCardInput {
    pub id: Uuid,
    pub agent: Option<String>,
    pub model: Option<String>,
}

#[tauri::command]
pub fn assign_card(state: State<'_, AppState>, input: AssignCardInput) -> Result<Card, String> {
    state
        .orchestrator
        .scheduler
        .assign(CardId(input.id), input.agent, input.model)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct TransitionInput {
    pub id: Uuid,
    pub status: String,
}

#[tauri::command]
pub fn transition_card(
    state: State<'_, AppState>,
    input: TransitionInput,
) -> Result<(), String> {
    let id = CardId(input.id);
    let result = match input.status.as_str() {
        "running" => state.orchestrator.scheduler.mark_running(id),
        "succeeded" => state.orchestrator.scheduler.mark_succeeded(id),
        "failed" => state.orchestrator.scheduler.mark_failed(id),
        "cancelled" => state.orchestrator.scheduler.cancel(id),
        other => return Err(format!("unknown status: {other}")),
    };
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reset_graph(state: State<'_, AppState>) -> Result<(), String> {
    // Re-spawn the scheduler with a fresh graph. The orchestrator
    // bridge's subscriber will reconnect via the next snapshot pull
    // the frontend does after this completes.
    let ids: Vec<CardId> = state
        .orchestrator
        .scheduler
        .cards()
        .into_iter()
        .map(|c| c.id)
        .collect();
    for id in ids {
        let _ = state.orchestrator.scheduler.remove_card(id);
    }
    Ok(())
}

fn parse_kind(s: &str) -> CardKind {
    match s.to_ascii_lowercase().as_str() {
        "feature" => CardKind::Feature,
        "refactor" => CardKind::Refactor,
        "bug" | "bug_fix" | "bugfix" => CardKind::BugFix,
        "research" => CardKind::Research,
        "test" => CardKind::Test,
        "docs" => CardKind::Docs,
        "setup" => CardKind::Setup,
        _ => CardKind::Feature,
    }
}

// Used to silence unused import lints when tests below stay quiet.
#[allow(dead_code)]
fn _types() -> CardStatus {
    CardStatus::Pending
}
