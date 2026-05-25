//! Shell agent — drops the user into an interactive shell for a card.
//!
//! Unlike the API/CLI agents, the shell agent doesn't have a notion of
//! "success" without the user. It marks the card Running and waits for
//! the user to mark it Succeeded/Failed from the UI. Useful for cards
//! that are ops work (run a migration, restart a service, debug live).

use async_trait::async_trait;
use brain_orchestrator::Card;
use futures::stream;

use crate::settings::ShellSettings;

use super::types::{
    AgentDescriptor, AgentError, AgentKind, CardAgent, CardExecution, CardOutcome,
    CardOutputChunk,
};

pub struct ShellAgent {
    settings: ShellSettings,
}

impl ShellAgent {
    pub fn new(settings: ShellSettings) -> Self {
        Self { settings }
    }
}

#[async_trait]
impl CardAgent for ShellAgent {
    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            slug: "shell".into(),
            label: "Shell session".into(),
            kind: AgentKind::Cli,
            default_model: Some(self.settings.default_shell.clone()),
            available_models: vec![
                "/bin/zsh".into(),
                "/bin/bash".into(),
                "/opt/homebrew/bin/fish".into(),
            ],
            supports_chat: false,
            supports_card_execution: true,
            ready: true,
            readiness_message: None,
        }
    }

    async fn execute(&self, card: &Card, _project_root: &str) -> Result<CardExecution, AgentError> {
        // The PTY is spawned by the orchestrator using the shell's
        // pty manager; this agent just signals "interactive — please
        // open a terminal tab for me". The orchestrator owns the PTY
        // lifecycle.
        let banner = format!(
            "[brain-ide] Shell session for card '{}'. Mark Succeeded/Failed from the UI when done.\n",
            card.title
        );
        let output = stream::iter(vec![
            CardOutputChunk::Status {
                text: "awaiting user".into(),
            },
            CardOutputChunk::Log { text: banner },
        ]);
        Ok(CardExecution {
            card_id: card.id.0,
            pty_session: None,
            output: Box::pin(output),
            // Cards assigned to the shell agent never auto-complete.
            // The user owns the outcome. Resolve the future immediately
            // with Cancelled — the orchestrator interprets shell cards
            // specially and waits for explicit user transition.
            outcome: Box::pin(async { CardOutcome::Cancelled }),
        })
    }
}
