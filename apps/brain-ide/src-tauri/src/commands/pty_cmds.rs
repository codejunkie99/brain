//! PTY commands. Backed by `pty::PtyManager`.

use base64::Engine;
use serde::Deserialize;
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::pty::{PtySessionInfo, SpawnOptions};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SpawnInput {
    pub cwd: Option<String>,
    pub program: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<Vec<(String, String)>>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    pub label: Option<String>,
    pub card_id: Option<Uuid>,
}

#[tauri::command]
pub fn pty_spawn(
    app: AppHandle,
    state: State<'_, AppState>,
    input: SpawnInput,
) -> Result<PtySessionInfo, String> {
    let settings = state.settings.read();
    let cwd = input
        .cwd
        .or_else(|| {
            state
                .orchestrator
                .project_root()
                .or_else(|| std::env::current_dir().ok().map(|p| p.display().to_string()))
        })
        .unwrap_or_else(|| ".".into());
    let program = input
        .program
        .unwrap_or_else(|| settings.shell.default_shell.clone());
    let label = input.label.unwrap_or_else(|| {
        std::path::Path::new(&program)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| program.clone())
    });
    state
        .pty
        .spawn(
            &app,
            SpawnOptions {
                program,
                args: input.args.unwrap_or_default(),
                cwd,
                env: input.env.unwrap_or_default(),
                cols: input.cols.unwrap_or(settings.shell.cols),
                rows: input.rows.unwrap_or(settings.shell.rows),
                label,
                card_id: input.card_id,
            },
        )
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct WriteInput {
    pub id: Uuid,
    pub data: String,
    #[serde(default)]
    pub base64: bool,
}

#[tauri::command]
pub fn pty_write(state: State<'_, AppState>, input: WriteInput) -> Result<(), String> {
    let bytes = if input.base64 {
        base64::engine::general_purpose::STANDARD
            .decode(input.data.as_bytes())
            .map_err(|e| e.to_string())?
    } else {
        input.data.into_bytes()
    };
    state.pty.write(input.id, &bytes).map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct ResizeInput {
    pub id: Uuid,
    pub cols: u16,
    pub rows: u16,
}

#[tauri::command]
pub fn pty_resize(state: State<'_, AppState>, input: ResizeInput) -> Result<(), String> {
    state
        .pty
        .resize(input.id, input.cols, input.rows)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_close(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    state.pty.close(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_list(state: State<'_, AppState>) -> Vec<PtySessionInfo> {
    state.pty.list()
}
