//! IDE-side wiring around `brain-orchestrator`.
//!
//! Bridges the scheduler's broadcast channel into Tauri events the
//! frontend subscribes to, and runs the actual agent dispatch on a
//! background task. When the scheduler emits `DispatchCard`, this
//! module:
//!   1. finds the card's assigned agent in the registry
//!   2. calls `agent.execute(card, project_root)`
//!   3. forwards `CardOutputChunk` chunks to the frontend
//!   4. marks the card Succeeded/Failed when the outcome resolves

use std::sync::Arc;

use brain_orchestrator::{Scheduler, SchedulerConfig, SchedulerEvent, SchedulerHandle};
use futures::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::agents::{
    AgentRegistry, CardOutcome,
};
use crate::memory::MemoryClient;

const EVENT_GRAPH: &str = "orchestrator://event";
const EVENT_CARD_OUTPUT: &str = "card://output";
const EVENT_CARD_OUTCOME: &str = "card://outcome";

#[derive(Debug, Clone, Serialize)]
pub struct CardOutputEvent {
    pub card_id: Uuid,
    pub chunk: crate::agents::types::CardOutputChunk,
}

#[derive(Debug, Clone, Serialize)]
pub struct CardOutcomeEvent {
    pub card_id: Uuid,
    pub outcome: CardOutcome,
}

#[derive(Clone)]
pub struct OrchestratorBridge {
    pub scheduler: SchedulerHandle,
    pub registry: AgentRegistry,
    pub memory: MemoryClient,
    pub project_root: Arc<parking_lot::RwLock<Option<String>>>,
}

impl OrchestratorBridge {
    pub fn new(registry: AgentRegistry, memory: MemoryClient) -> Self {
        let scheduler = Scheduler::spawn(SchedulerConfig {
            default_concurrency: 1,
            ..Default::default()
        });
        Self {
            scheduler,
            registry,
            memory,
            project_root: Arc::new(parking_lot::RwLock::new(None)),
        }
    }

    pub fn set_project_root(&self, root: Option<String>) {
        *self.project_root.write() = root;
    }

    pub fn project_root(&self) -> Option<String> {
        self.project_root.read().clone()
    }

    /// Spawn the long-running task that fans scheduler events into
    /// Tauri events and dispatches card execution. Call once at boot.
    pub fn start(&self, app: AppHandle) {
        let mut rx = self.scheduler.subscribe();
        let registry = self.registry.clone();
        let memory = self.memory.clone();
        let scheduler = self.scheduler.clone();
        let project_root = self.project_root.clone();
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                let _ = app.emit(EVENT_GRAPH, &event);
                if let SchedulerEvent::DispatchCard { card } = event {
                    let app = app.clone();
                    let registry = registry.clone();
                    let memory = memory.clone();
                    let scheduler = scheduler.clone();
                    let root = project_root.read().clone().unwrap_or_else(|| ".".into());
                    let assigned = card.assigned_agent.clone();
                    tokio::spawn(async move {
                        let Some(agent_slug) = assigned else {
                            return;
                        };
                        let Some(agent) = registry.card(&agent_slug) else {
                            tracing::warn!(?agent_slug, "no card agent registered for slug");
                            let _ = scheduler.mark_failed(card.id);
                            return;
                        };
                        let _ = scheduler.mark_running(card.id);
                        let _ = memory.note(
                            format!(
                                "card running: {} (agent: {agent_slug})",
                                card.title
                            ),
                            "brain-ide",
                            format!("card:{}:running", card.id),
                        );
                        match agent.execute(&card, &root).await {
                            Ok(mut exec) => {
                                let card_id = card.id.0;
                                // Pump output chunks
                                let pump_app = app.clone();
                                let pump_task = tokio::spawn(async move {
                                    while let Some(chunk) = exec.output.next().await {
                                        let _ = pump_app.emit(
                                            EVENT_CARD_OUTPUT,
                                            CardOutputEvent { card_id, chunk },
                                        );
                                    }
                                });
                                let outcome = exec.outcome.await;
                                let _ = pump_task.await;
                                let _ = app.emit(
                                    EVENT_CARD_OUTCOME,
                                    CardOutcomeEvent {
                                        card_id,
                                        outcome: outcome.clone(),
                                    },
                                );
                                match &outcome {
                                    CardOutcome::Succeeded { .. } => {
                                        let _ = scheduler.mark_succeeded(card.id);
                                        let _ = memory.note(
                                            format!("card succeeded: {}", card.title),
                                            "brain-ide",
                                            format!("card:{}:succeeded", card.id),
                                        );
                                    }
                                    CardOutcome::Failed { reason } => {
                                        let _ = scheduler.mark_failed(card.id);
                                        let _ = memory.note(
                                            format!(
                                                "card failed: {} ({reason})",
                                                card.title
                                            ),
                                            "brain-ide",
                                            format!("card:{}:failed", card.id),
                                        );
                                    }
                                    CardOutcome::Cancelled => {
                                        // Leave the card in whatever state
                                        // the user puts it in (shell/human
                                        // cards expect manual transition).
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "agent execute failed");
                                let _ = scheduler.mark_failed(card.id);
                            }
                        }
                    });
                }
            }
        });
    }
}
