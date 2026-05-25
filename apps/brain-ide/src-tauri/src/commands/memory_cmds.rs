//! Memory-layer commands.

use serde::Deserialize;
use tauri::State;
use uuid::Uuid;

use crate::memory::MemoryEntry;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct NoteInput {
    pub text: String,
    pub actor: Option<String>,
    pub idempotency_key: Option<String>,
}

#[tauri::command]
pub fn memory_note(state: State<'_, AppState>, input: NoteInput) -> Result<Uuid, String> {
    let actor = input.actor.unwrap_or_else(|| "user".into());
    let key = input
        .idempotency_key
        .unwrap_or_else(|| format!("user-note:{}", Uuid::now_v7()));
    state
        .memory
        .note(input.text, actor, key)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct RecentInput {
    pub limit: Option<usize>,
}

#[tauri::command]
pub fn memory_recent(
    state: State<'_, AppState>,
    input: RecentInput,
) -> Result<Vec<MemoryEntry>, String> {
    state
        .memory
        .recent(input.limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct SearchInput {
    pub query: String,
    pub limit: Option<usize>,
}

#[tauri::command]
pub fn memory_search(
    state: State<'_, AppState>,
    input: SearchInput,
) -> Result<Vec<MemoryEntry>, String> {
    state
        .memory
        .search(&input.query, input.limit.unwrap_or(25))
        .map_err(|e| e.to_string())
}
