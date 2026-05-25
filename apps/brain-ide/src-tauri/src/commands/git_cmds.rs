//! Git commands. Each accepts the project root explicitly — the
//! backend never picks one for the user.

use std::path::PathBuf;

use serde::Deserialize;
use tauri::State;

use crate::git::{self, CommitInfo, FileDiff, RepoStatus};
use crate::state::AppState;

fn project_root(input_root: Option<String>, state: &AppState) -> Result<PathBuf, String> {
    let root = input_root
        .or_else(|| state.orchestrator.project_root())
        .ok_or_else(|| "no project root".to_string())?;
    Ok(PathBuf::from(root))
}

#[derive(Debug, Deserialize)]
pub struct GitRootInput {
    pub root: Option<String>,
}

#[tauri::command]
pub fn git_status(
    state: State<'_, AppState>,
    input: GitRootInput,
) -> Result<RepoStatus, String> {
    let root = project_root(input.root, &state)?;
    git::status(&root).map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct GitDiffInput {
    pub root: Option<String>,
    pub path: String,
    #[serde(default)]
    pub staged: bool,
}

#[tauri::command]
pub fn git_diff(
    state: State<'_, AppState>,
    input: GitDiffInput,
) -> Result<FileDiff, String> {
    let root = project_root(input.root, &state)?;
    git::diff(&root, &input.path, input.staged).map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct GitPathsInput {
    pub root: Option<String>,
    pub paths: Vec<String>,
}

#[tauri::command]
pub fn git_stage(
    state: State<'_, AppState>,
    input: GitPathsInput,
) -> Result<(), String> {
    let root = project_root(input.root, &state)?;
    git::stage(&root, &input.paths).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_unstage(
    state: State<'_, AppState>,
    input: GitPathsInput,
) -> Result<(), String> {
    let root = project_root(input.root, &state)?;
    git::unstage(&root, &input.paths).map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct GitCommitInput {
    pub root: Option<String>,
    pub message: String,
}

#[tauri::command]
pub fn git_commit(
    state: State<'_, AppState>,
    input: GitCommitInput,
) -> Result<String, String> {
    let root = project_root(input.root, &state)?;
    git::commit(&root, &input.message).map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct GitLogInput {
    pub root: Option<String>,
    pub limit: Option<usize>,
}

#[tauri::command]
pub fn git_log(
    state: State<'_, AppState>,
    input: GitLogInput,
) -> Result<Vec<CommitInfo>, String> {
    let root = project_root(input.root, &state)?;
    git::log(&root, input.limit.unwrap_or(50)).map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct GitCheckoutInput {
    pub root: Option<String>,
    pub branch: String,
}

#[tauri::command]
pub fn git_checkout(
    state: State<'_, AppState>,
    input: GitCheckoutInput,
) -> Result<(), String> {
    let root = project_root(input.root, &state)?;
    git::checkout(&root, &input.branch).map_err(|e| e.to_string())
}
