//! Shared application state. One instance lives in Tauri's
//! `app.manage()` so every `#[tauri::command]` can read it.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use tauri::{AppHandle, Manager};

use crate::agents::{AgentRegistry, ChatTranscript};
use crate::mcp::McpHost;
use crate::memory::MemoryClient;
use crate::orchestrator::OrchestratorBridge;
use crate::projects::ProjectStore;
use crate::pty::PtyManager;
use crate::settings::SettingsStore;

pub struct AppState {
    pub settings: SettingsStore,
    pub projects: ProjectStore,
    pub memory: MemoryClient,
    pub agents: Arc<RwLock<AgentRegistry>>,
    pub transcripts: ChatTranscript,
    pub orchestrator: OrchestratorBridge,
    pub pty: PtyManager,
    pub mcp: McpHost,
    pub data_dir: PathBuf,
}

impl AppState {
    pub fn initialize(app: &AppHandle) -> anyhow::Result<Self> {
        let data_dir = resolve_data_dir(app)?;
        std::fs::create_dir_all(&data_dir)?;

        let settings = SettingsStore::open_or_default(data_dir.join("settings.json"));
        let projects = ProjectStore::open_or_default(data_dir.join("projects.json"));
        let memory = MemoryClient::new();

        // If a project is active, open its brain immediately so the
        // first chat note / card execution doesn't pay the init cost.
        if let Some(active) = projects.active() {
            if let Err(e) = memory.open(&active.brain_dir) {
                tracing::warn!(error = %e, brain_dir = %active.brain_dir.display(), "failed to open brain at startup");
            }
        } else if let Some(brain_dir) = settings.read().brain_dir.as_ref() {
            if let Err(e) = memory.open(brain_dir) {
                tracing::warn!(error = %e, brain_dir = %brain_dir.display(), "failed to open default brain");
            }
        }

        let registry = AgentRegistry::default_for(&settings.read());
        let orchestrator = OrchestratorBridge::new(registry.clone(), memory.clone());

        if let Some(active) = projects.active() {
            orchestrator.set_project_root(Some(active.root.to_string_lossy().to_string()));
        }

        orchestrator.start(app.clone());

        // The brain CLI is what runs `brain serve --mcp` for external
        // agents. We accept None here and let the user override via
        // settings; the McpHost falls back to plain `brain` on PATH.
        let mcp = McpHost::new(None);
        if let Some(path) = memory.current_path() {
            mcp.set_brain_dir(path);
        }

        Ok(Self {
            settings,
            projects,
            memory,
            agents: Arc::new(RwLock::new(registry)),
            transcripts: ChatTranscript::new(),
            orchestrator,
            pty: PtyManager::new(),
            mcp,
            data_dir,
        })
    }
}

fn resolve_data_dir(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let path = app.path().app_data_dir()?;
    Ok(path)
}
