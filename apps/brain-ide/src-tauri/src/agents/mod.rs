//! Agents: the things that talk to models or shells on behalf of cards.
//!
//! Two surfaces:
//!
//!   1. **Chat** (`ChatAgent`): streaming text-in / text-out conversation
//!      that powers the left chat pane. The chat thread persists across
//!      model switches — the IDE holds the message log, agents only
//!      see "here's the current transcript, stream a reply".
//!
//!   2. **Card execution** (`CardAgent`): an agent claims a card,
//!      executes it (possibly via a CLI subprocess with file-edit
//!      powers), and reports success/failure. Card output streams to a
//!      dedicated PTY/terminal tab.
//!
//! The registry is keyed by a string slug ("claude", "codex", "shell",
//! "human"). The frontend uses these slugs as drag-drop targets.

pub mod claude;
pub mod codex;
pub mod human;
pub mod registry;
pub mod shell;
pub mod transcript;
pub mod types;

pub use registry::{AgentRegistry, AgentSlug};

/// Cross-platform `which` for agent readiness checks.
pub(crate) fn claude_which(bin: &str) -> bool {
    if bin.contains('/') {
        return std::path::Path::new(bin).exists();
    }
    if let Ok(paths) = std::env::var("PATH") {
        for p in std::env::split_paths(&paths) {
            if p.join(bin).exists() {
                return true;
            }
        }
    }
    false
}
pub use transcript::{ChatMessage, ChatRole, ChatTranscript};
pub use types::{
    AgentDescriptor, AgentError, CardAgent, CardExecution, CardOutcome, CardOutputChunk,
    ChatAgent, ChatChunk, ChatRequest,
};
