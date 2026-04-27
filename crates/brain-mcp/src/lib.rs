//! brain-mcp: rmcp-based MCP server exposing brain to any MCP client.
//!
//! v0.1: stdio transport with five tools (`ping`, `note`, `log`, `ask`,
//! `doctor`). Resources, prompts, sampling, and richer typed memory tools
//! come in follow-ups.

use brain_app::LocalBrain;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, ServiceExt, tool, tool_router, transport::stdio};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("rmcp service error: {0}")]
    Service(String),
    #[error("app error: {0}")]
    App(#[from] brain_app::AppError),
}

/// The MCP server handler. Clones LocalBrain per call so we stay
/// Send-friendly; LocalBrain's Clone is cheap (just a PathBuf).
#[derive(Clone)]
pub struct BrainMcp {
    brain: LocalBrain,
}

impl BrainMcp {
    pub fn new(brain: LocalBrain) -> Self {
        Self { brain }
    }
}

/// Arguments for the `note` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoteArgs {
    /// What to remember.
    pub text: String,
}

/// Arguments for the `ask` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AskArgs {
    /// What to search for.
    pub query: String,
}

#[tool_router(server_handler)]
impl BrainMcp {
    /// Quick liveness check. Does not touch the brain.
    #[tool(description = "Ping the brain server; returns 'pong' if alive.")]
    async fn ping(&self) -> String {
        "pong".to_string()
    }

    /// Save a note.
    #[tool(description = "\
Save a persistent note to the user's long-term memory. CALL THIS WHENEVER:
- the user states a preference or convention (\"I use X over Y because Z\")
- the user shares a decision or rationale you'd want to recall next session
- you finish a non-trivial task and a lesson emerged (root cause, gotcha,
  reason for a choice)
- the user asks you to remember something
Do NOT call this for ephemeral context that only matters in the current turn.
The note is written to a git-backed event log — durable across sessions and
across tools (Claude Code, Cursor, Codex, etc. all share the same brain).
Input is a single string. Keep it one observation, one line when possible.
No need to prefix with 'Remember:' — just the content.")]
    async fn note(&self, Parameters(args): Parameters<NoteArgs>) -> Result<String, ErrorData> {
        let brain = self.brain.clone();
        let text = args.text;
        let ev = tokio::task::spawn_blocking(move || brain.observe_summary(text))
            .await
            .map_err(|e| ErrorData::internal_error(format!("spawn: {e}"), None))?
            .map_err(|e| ErrorData::internal_error(format!("save: {e}"), None))?;
        Ok(if ev.was_idempotent_replay {
            "Already saved.".to_string()
        } else {
            "Saved.".to_string()
        })
    }

    /// Show recent notes.
    #[tool(description = "\
Show the user's 20 most recent long-term-memory notes, newest first.
Call this when the user asks 'what have we worked on?' / 'what's recent?' /
'what did I save?' or when you need a quick orientation at the start of
a session. For specific lookups use `ask` instead — `log` is for
chronological browsing.")]
    async fn log(&self) -> Result<String, ErrorData> {
        let brain = self.brain.clone();
        let events = tokio::task::spawn_blocking(move || brain.log(20))
            .await
            .map_err(|e| ErrorData::internal_error(format!("spawn: {e}"), None))?
            .map_err(|e| ErrorData::internal_error(format!("log: {e}"), None))?;
        if events.is_empty() {
            return Ok("No notes yet.".to_string());
        }
        let mut out = String::from("Recent notes:\n");
        for ev in events {
            out.push_str(&format!("- {}\n", ev.payload.summary_line(80)));
        }
        Ok(out)
    }

    /// Search notes.
    #[tool(description = "\
Search the user's long-term memory (git-backed event log, FTS5). Returns
up to 5 matches. CALL THIS PROACTIVELY at the start of non-trivial tasks:
- before picking a library, pattern, or config value
- before running a migration, deploy, or schema change
- when the user references prior decisions (\"what did we pick for auth?\")
- when the user asks 'didn't we fix this before?'
- when you suspect the user has already stated a preference
Prefix-matching is automatic — typing `fast` finds `fastapi-users`.
Multi-word queries AND together — `auth lib` finds notes with both.
Hyphens and slashes split into separate tokens — `fastapi-users`
matches either `fastapi` or `users`. Explicit operators (`*`, `\"phrase\"`,
`AND`/`OR`/`NOT`, `title:x`) are respected when present.
If a search returns nothing relevant, cost was ~1ms — cheap to try.")]
    async fn ask(&self, Parameters(args): Parameters<AskArgs>) -> Result<String, ErrorData> {
        let brain = self.brain.clone();
        let q = args.query;
        let q_for_msg = q.clone();
        let hits = tokio::task::spawn_blocking(move || brain.search(&q, 5))
            .await
            .map_err(|e| ErrorData::internal_error(format!("spawn: {e}"), None))?
            .map_err(|e| ErrorData::internal_error(format!("search: {e}"), None))?;
        if hits.is_empty() {
            return Ok(format!("No matches for {:?}.", q_for_msg));
        }
        let mut out = String::from("Matches:\n");
        for ev in hits {
            out.push_str(&format!("- {}\n", ev.payload.summary_line(80)));
        }
        Ok(out)
    }

    /// Report brain status.
    #[tool(description = "\
Health check for the user's long-term memory. Returns schema version,
event count, indexed count, index lag, and any detected issues. Call this when the
user asks 'is brain working?' / 'how many notes do I have?' or when
`ask` / `log` return unexpected results and you suspect index drift.
Rarely needed in normal flow.")]
    async fn doctor(&self) -> Result<String, ErrorData> {
        let brain = self.brain.clone();
        let r = tokio::task::spawn_blocking(move || brain.doctor())
            .await
            .map_err(|e| ErrorData::internal_error(format!("spawn: {e}"), None))?
            .map_err(|e| ErrorData::internal_error(format!("doctor: {e}"), None))?;
        if r.issues.is_empty() {
            let mut out = format!(
                "Ready. schema_version={}, {} note{} ({} indexed, {} lagging).",
                r.schema_version,
                r.event_count,
                if r.event_count == 1 { "" } else { "s" },
                r.indexed_event_count,
                r.index_lag
            );
            if !r.warnings.is_empty() {
                out.push_str("\nWarnings:\n");
                for warning in &r.warnings {
                    out.push_str(&format!("- {}\n", warning));
                }
            }
            Ok(out)
        } else {
            let mut out = String::from("Problems:\n");
            for issue in &r.issues {
                out.push_str(&format!("- {}\n", issue));
            }
            Ok(out)
        }
    }
}

/// Run an MCP server on stdio against the given brain. Blocks until the
/// client disconnects.
///
/// Known limitation (Codex F4 round 5): rmcp 1.5's stdio transport uses
/// `JsonRpcMessageCodec::default()` which sets `max_length = usize::MAX`.
/// A hostile stdio peer can write a multi-gigabyte frame with no newline
/// and force unbounded allocation before JSON decode. Fixing it cleanly
/// requires an rmcp API change (expose `AsyncRwTransport::with_codec` or
/// similar); local workarounds like reactive caps fire AFTER decode, too
/// late. Threat model here is local trust (stdio peer is the AI client
/// we chose to wire in) so the practical risk is low, but document and
/// file upstream. Tracking issue: TODO(rmcp).
pub async fn serve_stdio(brain: LocalBrain) -> Result<(), McpError> {
    let server = BrainMcp::new(brain);
    let service = server
        .serve(stdio())
        .await
        .map_err(|e| McpError::Service(e.to_string()))?;
    service
        .waiting()
        .await
        .map_err(|e| McpError::Service(e.to_string()))?;
    Ok(())
}
