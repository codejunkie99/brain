//! Tauri command handlers. Each `#[tauri::command]` is a method the
//! React frontend calls through `invoke()`.
//!
//! Grouped by surface:
//!   - chat   — send/stream chat turns, manage transcripts
//!   - graph  — feature graph CRUD, drag-drop assignment
//!   - spec   — parse spec into cards via heuristic or model
//!   - pty    — open/write/resize/close terminals
//!   - fs     — read project tree + open/save files
//!   - memory — recent events, search, manual note
//!   - projects/settings/auth — straight CRUD around the stores
//!
//! The actual `generate_handler!` invocation lives in `lib.rs`; the
//! macro needs the Runtime generic at the call site, so we just re-
//! export the command functions here for it to consume.

pub mod chat;
pub mod fs;
pub mod graph;
pub mod memory_cmds;
pub mod misc;
pub mod pty_cmds;
pub mod spec;
