//! Human assignee — the card is parked for a human to handle.
//!
//! The orchestrator dispatches the card so it shows "claimed" on the
//! graph, but no work happens until the human marks it Succeeded or
//! Failed. Useful for review, deploy-approval, design-decision cards.

use async_trait::async_trait;
use brain_orchestrator::Card;
use futures::stream;

use super::types::{
    AgentDescriptor, AgentError, AgentKind, CardAgent, CardExecution, CardOutcome,
    CardOutputChunk,
};

pub struct HumanAssignee;

#[async_trait]
impl CardAgent for HumanAssignee {
    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            slug: "human".into(),
            label: "Human".into(),
            kind: AgentKind::Human,
            default_model: None,
            available_models: Vec::new(),
            supports_chat: false,
            supports_card_execution: true,
            ready: true,
            readiness_message: None,
        }
    }

    async fn execute(&self, card: &Card, _project_root: &str) -> Result<CardExecution, AgentError> {
        let chunks = vec![CardOutputChunk::Status {
            text: format!("Waiting on human review: {}", card.title),
        }];
        Ok(CardExecution {
            card_id: card.id.0,
            pty_session: None,
            output: Box::pin(stream::iter(chunks)),
            outcome: Box::pin(async { CardOutcome::Cancelled }),
        })
    }
}
