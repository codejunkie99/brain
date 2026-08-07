//! MCP-host commands.

use tauri::State;

use crate::mcp::{McpConfigSnippets, McpStatus};
use crate::state::AppState;

#[tauri::command]
pub fn mcp_status(state: State<'_, AppState>) -> McpStatus {
    state.mcp.status()
}

#[tauri::command]
pub async fn mcp_start(state: State<'_, AppState>) -> Result<McpStatus, String> {
    // Lazily sync the brain dir with what memory currently has open
    // so the user gets sensible behavior even before they touch
    // settings.
    if let Some(path) = state.memory.current_path() {
        state.mcp.set_brain_dir(path);
    }
    state.mcp.start().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mcp_stop(state: State<'_, AppState>) -> Result<(), String> {
    state.mcp.stop().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mcp_config_snippets(
    state: State<'_, AppState>,
) -> Result<McpConfigSnippets, String> {
    if let Some(path) = state.memory.current_path() {
        state.mcp.set_brain_dir(path);
    }
    state.mcp.config_snippets().map_err(|e| e.to_string())
}
