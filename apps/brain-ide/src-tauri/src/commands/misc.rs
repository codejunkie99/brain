//! Project, settings, and auth commands.

use std::path::PathBuf;

use serde::Deserialize;
use tauri::State;
use uuid::Uuid;

use crate::auth::{AuthStore, CredentialKind, CredentialStatus};
use crate::projects::Project;
use crate::settings::Settings;
use crate::state::AppState;

// --- Projects -------------------------------------------------------------

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Vec<Project> {
    state.projects.list()
}

#[derive(Debug, Deserialize)]
pub struct AddProjectInput {
    pub name: String,
    pub root: String,
    pub brain_dir: Option<String>,
}

#[tauri::command]
pub fn add_project(state: State<'_, AppState>, input: AddProjectInput) -> Result<Project, String> {
    let project = Project::new(
        input.name,
        PathBuf::from(input.root),
        input.brain_dir.map(PathBuf::from),
    );
    let project = state.projects.add(project).map_err(|e| e.to_string())?;
    // Open its brain so memory commands work immediately.
    if let Err(e) = state.memory.open(&project.brain_dir) {
        tracing::warn!(error = %e, "failed to open brain for new project");
    }
    Ok(project)
}

#[tauri::command]
pub fn set_active_project(state: State<'_, AppState>, id: Uuid) -> Result<Project, String> {
    let project = state.projects.set_active(id).map_err(|e| e.to_string())?;
    state.memory.open(&project.brain_dir).map_err(|e| e.to_string())?;
    state
        .orchestrator
        .set_project_root(Some(project.root.to_string_lossy().to_string()));
    Ok(project)
}

#[tauri::command]
pub fn remove_project(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    state.projects.remove(id).map_err(|e| e.to_string())
}

// --- Settings -------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.read()
}

#[tauri::command]
pub fn update_settings(
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    let updated = state
        .settings
        .update(|s| *s = settings)
        .map_err(|e| e.to_string())?;
    // Rebuild agent registry so binary/model/extra_args changes take effect.
    state.agents.read().rebuild(&updated);
    Ok(updated)
}

// --- Auth -----------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CredentialInput {
    pub kind: CredentialKind,
    pub secret: String,
}

#[tauri::command]
pub fn set_credential(
    state: State<'_, AppState>,
    input: CredentialInput,
) -> Result<(), String> {
    AuthStore::set(input.kind, &input.secret).map_err(|e| e.to_string())?;
    // Rebuild registry so the `ready` flag flips on agents this credential unlocks.
    let settings = state.settings.read();
    state.agents.read().rebuild(&settings);
    Ok(())
}

#[tauri::command]
pub fn clear_credential(
    state: State<'_, AppState>,
    kind: CredentialKind,
) -> Result<(), String> {
    AuthStore::clear(kind).map_err(|e| e.to_string())?;
    let settings = state.settings.read();
    state.agents.read().rebuild(&settings);
    Ok(())
}

#[tauri::command]
pub fn credential_status() -> Vec<CredentialStatus> {
    vec![
        AuthStore::status(CredentialKind::Anthropic),
        AuthStore::status(CredentialKind::Openai),
    ]
}

// --- Brain dir ------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct OpenBrainInput {
    pub path: String,
}

#[tauri::command]
pub fn open_brain(
    state: State<'_, AppState>,
    input: OpenBrainInput,
) -> Result<String, String> {
    let p = state
        .memory
        .open(std::path::Path::new(&input.path))
        .map_err(|e| e.to_string())?;
    Ok(p.to_string_lossy().to_string())
}
