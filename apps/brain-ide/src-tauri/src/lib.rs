//! Brain IDE — Tauri 2 backend.
//!
//! Layout:
//!   - `state`: the shared `AppState` (memory, projects, agents, sched).
//!   - `memory`: wraps `brain-store` for the IDE's needs.
//!   - `agents`: Claude / Codex / shell agent runners + registry.
//!   - `orchestrator`: glue between `brain-orchestrator` and the IDE.
//!   - `pty`: terminal sessions (one per card or free-floating).
//!   - `projects`: project list + active project bookkeeping.
//!   - `settings`: persisted settings (theme, default model, etc.).
//!   - `auth`: API key storage via the macOS keychain.
//!   - `commands`: every `#[tauri::command]` lives under here.

pub mod agents;
pub mod auth;
pub mod commands;
pub mod memory;
pub mod orchestrator;
pub mod projects;
pub mod pty;
pub mod settings;
pub mod state;

use tauri::Manager;

/// Entry point called from main.rs. Builds the Tauri runtime, registers
/// plugins + commands, and starts the event loop.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("BRAIN_IDE_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let state = state::AppState::initialize(&handle)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // chat
            commands::chat::list_threads,
            commands::chat::create_thread,
            commands::chat::delete_thread,
            commands::chat::rename_thread,
            commands::chat::send_message,
            commands::chat::cancel_stream,
            commands::chat::list_agents,
            // graph
            commands::graph::graph_snapshot,
            commands::graph::add_card,
            commands::graph::remove_card,
            commands::graph::add_edge,
            commands::graph::assign_card,
            commands::graph::transition_card,
            commands::graph::reset_graph,
            // spec
            commands::spec::parse_spec,
            commands::spec::apply_breakdown,
            // pty
            commands::pty_cmds::pty_spawn,
            commands::pty_cmds::pty_write,
            commands::pty_cmds::pty_resize,
            commands::pty_cmds::pty_close,
            commands::pty_cmds::pty_list,
            // fs
            commands::fs::list_tree,
            commands::fs::read_file,
            commands::fs::write_file,
            // memory
            commands::memory_cmds::memory_note,
            commands::memory_cmds::memory_recent,
            commands::memory_cmds::memory_search,
            // misc
            commands::misc::list_projects,
            commands::misc::add_project,
            commands::misc::set_active_project,
            commands::misc::remove_project,
            commands::misc::get_settings,
            commands::misc::update_settings,
            commands::misc::set_credential,
            commands::misc::clear_credential,
            commands::misc::credential_status,
            commands::misc::open_brain,
        ])
        .run(tauri::generate_context!())
        .expect("error while running brain-ide");
}
