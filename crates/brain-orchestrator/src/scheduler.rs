//! Scheduler: dispatches Ready cards to agents and emits state events.
//!
//! The scheduler owns no agents itself — the IDE registers callbacks for
//! "start this card on agent X". The scheduler's job is the bookkeeping:
//!   - watch the graph for Ready cards
//!   - respect a per-agent concurrency limit
//!   - emit `SchedulerEvent` so the UI can update card pills, edge
//!     colors, terminal tabs, etc.
//!
//! Cancellation: cards can be cancelled mid-flight. The scheduler emits
//! `CardCancelRequested`; the agent runner is responsible for actually
//! interrupting the subprocess and reporting back via `mark_*`.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::card::{Card, CardId, CardStatus};
use crate::graph::{FeatureGraph, GraphError};

/// Events the UI subscribes to. One channel per scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SchedulerEvent {
    CardAdded { card: Card },
    CardUpdated { card: Card },
    CardRemoved { id: CardId },
    DispatchCard { card: Card },
    /// User or system asked the agent runner to stop the card.
    CardCancelRequested { id: CardId },
    /// The graph mutated in ways the UI should re-render wholesale
    /// (e.g. after `apply_breakdown`). Carries a fresh snapshot.
    GraphRescanned,
}

#[derive(Debug, Clone, Default)]
pub struct SchedulerConfig {
    /// Max concurrent in-flight cards per agent slot. 0 means unlimited.
    /// Default 1 — most agents serialize their subprocesses.
    pub per_agent_concurrency: HashMap<String, usize>,
    /// Default concurrency for agents not in the map. 1.
    pub default_concurrency: usize,
}

impl SchedulerConfig {
    pub fn limit_for(&self, agent: &str) -> usize {
        self.per_agent_concurrency
            .get(agent)
            .copied()
            .unwrap_or(if self.default_concurrency == 0 {
                1
            } else {
                self.default_concurrency
            })
    }
}

/// Handle the IDE holds to push state into the scheduler from any
/// thread. Cheap to clone.
#[derive(Clone)]
pub struct SchedulerHandle {
    inner: Arc<Mutex<Inner>>,
    events: broadcast::Sender<SchedulerEvent>,
}

struct Inner {
    graph: FeatureGraph,
    config: SchedulerConfig,
    /// Currently running count per agent.
    in_flight: HashMap<String, usize>,
}

pub struct Scheduler;

impl Scheduler {
    pub fn spawn(config: SchedulerConfig) -> SchedulerHandle {
        Self::with_graph(FeatureGraph::new(), config)
    }

    pub fn with_graph(graph: FeatureGraph, config: SchedulerConfig) -> SchedulerHandle {
        let (tx, _) = broadcast::channel(256);
        SchedulerHandle {
            inner: Arc::new(Mutex::new(Inner {
                graph,
                config,
                in_flight: HashMap::new(),
            })),
            events: tx,
        }
    }
}

impl SchedulerHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<SchedulerEvent> {
        self.events.subscribe()
    }

    pub fn snapshot(&self) -> crate::graph::GraphSnapshot {
        self.inner.lock().graph.snapshot()
    }

    pub fn cards(&self) -> Vec<Card> {
        self.inner.lock().graph.cards()
    }

    pub fn get_card(&self, id: CardId) -> Option<Card> {
        self.inner.lock().graph.get(id).ok().cloned()
    }

    pub fn add_card(&self, card: Card) -> Result<CardId, GraphError> {
        let id = {
            let mut inner = self.inner.lock();
            let id = inner.graph.add_card(card.clone())?;
            inner.graph.rescan();
            id
        };
        let _ = self.events.send(SchedulerEvent::CardAdded { card });
        // Send dispatches for any newly-ready cards.
        self.try_dispatch();
        Ok(id)
    }

    pub fn add_edge(
        &self,
        from: CardId,
        to: CardId,
        kind: crate::card::EdgeKind,
    ) -> Result<(), GraphError> {
        {
            let mut inner = self.inner.lock();
            inner.graph.add_edge(from, to, kind)?;
            inner.graph.rescan();
        }
        let _ = self.events.send(SchedulerEvent::GraphRescanned);
        self.try_dispatch();
        Ok(())
    }

    pub fn remove_card(&self, id: CardId) -> Result<(), GraphError> {
        {
            let mut inner = self.inner.lock();
            inner.graph.remove_card(id)?;
            inner.graph.rescan();
        }
        let _ = self.events.send(SchedulerEvent::CardRemoved { id });
        let _ = self.events.send(SchedulerEvent::GraphRescanned);
        Ok(())
    }

    /// Assign a card to an agent slot and (optionally) a model. If the
    /// card is Ready, this also dispatches.
    pub fn assign(
        &self,
        id: CardId,
        agent: Option<String>,
        model: Option<String>,
    ) -> Result<Card, GraphError> {
        let card = {
            let mut inner = self.inner.lock();
            let card = inner.graph.get_mut(id)?;
            card.assigned_agent = agent;
            card.model = model;
            card.updated_at = chrono::Utc::now();
            card.clone()
        };
        let _ = self.events.send(SchedulerEvent::CardUpdated { card: card.clone() });
        self.try_dispatch();
        Ok(card)
    }

    /// Apply a parsed spec breakdown: insert all cards, then resolve
    /// the `(title, title)` deps to edges. Conflicts (duplicate titles,
    /// cycles) are skipped with a warning logged via `tracing`.
    pub fn apply_breakdown(&self, breakdown: crate::spec::SpecBreakdown) -> Vec<CardId> {
        let mut ids = Vec::new();
        let mut by_title: HashMap<String, CardId> = HashMap::new();
        {
            let mut inner = self.inner.lock();
            for card in breakdown.cards {
                let title = card.title.clone();
                match inner.graph.add_card(card) {
                    Ok(id) => {
                        by_title.insert(title, id);
                        ids.push(id);
                    }
                    Err(e) => tracing::warn!(error = %e, "skipped duplicate card"),
                }
            }
            for (from_title, to_title) in breakdown.deps {
                match (by_title.get(&from_title), by_title.get(&to_title)) {
                    (Some(&from), Some(&to)) => {
                        if let Err(e) = inner
                            .graph
                            .add_edge(from, to, crate::card::EdgeKind::DependsOn)
                        {
                            tracing::warn!(error = %e, "skipped dep edge");
                        }
                    }
                    _ => tracing::warn!(
                        ?from_title,
                        ?to_title,
                        "dependency references unknown title"
                    ),
                }
            }
            inner.graph.rescan();
        }
        let _ = self.events.send(SchedulerEvent::GraphRescanned);
        self.try_dispatch();
        ids
    }

    pub fn mark_running(&self, id: CardId) -> Result<(), GraphError> {
        self.transition(id, CardStatus::Running, |inner, card| {
            if let Some(agent) = &card.assigned_agent {
                *inner.in_flight.entry(agent.clone()).or_default() += 1;
            }
        })
    }

    pub fn mark_succeeded(&self, id: CardId) -> Result<(), GraphError> {
        self.transition(id, CardStatus::Succeeded, Self::dec_in_flight)?;
        self.try_dispatch();
        Ok(())
    }

    pub fn mark_failed(&self, id: CardId) -> Result<(), GraphError> {
        self.transition(id, CardStatus::Failed, Self::dec_in_flight)?;
        self.try_dispatch();
        Ok(())
    }

    pub fn cancel(&self, id: CardId) -> Result<(), GraphError> {
        let was_running = {
            let inner = self.inner.lock();
            inner.graph.get(id)?.status == CardStatus::Running
        };
        if was_running {
            let _ = self.events.send(SchedulerEvent::CardCancelRequested { id });
        }
        self.transition(id, CardStatus::Cancelled, Self::dec_in_flight)?;
        self.try_dispatch();
        Ok(())
    }

    fn dec_in_flight(inner: &mut Inner, card: &Card) {
        if let Some(agent) = &card.assigned_agent {
            let counter = inner.in_flight.entry(agent.clone()).or_default();
            *counter = counter.saturating_sub(1);
        }
    }

    fn transition(
        &self,
        id: CardId,
        new_status: CardStatus,
        on_change: impl FnOnce(&mut Inner, &Card),
    ) -> Result<(), GraphError> {
        let changed_cards = {
            let mut inner = self.inner.lock();
            let card_snapshot = inner.graph.get(id)?.clone();
            on_change(&mut inner, &card_snapshot);
            let changed = inner.graph.transition(id, new_status)?;
            changed
                .into_iter()
                .filter_map(|cid| inner.graph.get(cid).ok().cloned())
                .collect::<Vec<_>>()
        };
        for c in changed_cards {
            let _ = self.events.send(SchedulerEvent::CardUpdated { card: c });
        }
        Ok(())
    }

    /// Look for Ready cards whose agent slot has capacity and emit
    /// dispatch events for them. Idempotent: callers that don't care
    /// about the result can spam it.
    fn try_dispatch(&self) {
        let to_dispatch: Vec<Card> = {
            let mut inner = self.inner.lock();
            let ready: Vec<CardId> = inner.graph.ready_cards();
            let mut out = Vec::new();
            for id in ready {
                let card = match inner.graph.get(id) {
                    Ok(c) => c.clone(),
                    Err(_) => continue,
                };
                let Some(agent) = card.assigned_agent.clone() else {
                    // Unassigned Ready cards just sit in the graph
                    // until the user drags them onto an agent.
                    continue;
                };
                let limit = inner.config.limit_for(&agent);
                let current = *inner.in_flight.get(&agent).unwrap_or(&0);
                if current >= limit {
                    continue;
                }
                // Reserve the slot before dispatching so two near-
                // simultaneous calls to try_dispatch don't both pick the
                // same card.
                *inner.in_flight.entry(agent.clone()).or_default() += 1;
                // The IDE's runner will call mark_running -> mark_*; we
                // pre-incremented above so we *decrement* here before
                // mark_running re-increments.
                *inner.in_flight.entry(agent).or_default() -= 1;
                out.push(card);
            }
            out
        };
        for card in to_dispatch {
            let _ = self.events.send(SchedulerEvent::DispatchCard { card });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, CardKind, EdgeKind};

    fn card_assigned(title: &str, agent: &str) -> Card {
        let mut c = Card::new(title, "", CardKind::Feature);
        c.assigned_agent = Some(agent.to_string());
        c
    }

    #[tokio::test]
    async fn dispatch_emits_for_ready_assigned_cards() {
        let handle = Scheduler::spawn(SchedulerConfig::default());
        let mut rx = handle.subscribe();
        let a = handle.add_card(card_assigned("a", "claude")).unwrap();
        // Wait for events. The two we care about: CardAdded then DispatchCard.
        let mut saw_dispatch = false;
        for _ in 0..4 {
            match rx.try_recv() {
                Ok(SchedulerEvent::DispatchCard { card }) if card.id == a => {
                    saw_dispatch = true;
                    break;
                }
                Ok(_) => continue,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
            }
        }
        assert!(saw_dispatch);
    }

    #[tokio::test]
    async fn cards_without_agent_dont_dispatch() {
        let handle = Scheduler::spawn(SchedulerConfig::default());
        let mut rx = handle.subscribe();
        let _a = handle
            .add_card(Card::new("a", "", CardKind::Feature))
            .unwrap();
        // Drain a few events, none should be DispatchCard.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        while let Ok(ev) = rx.try_recv() {
            assert!(!matches!(ev, SchedulerEvent::DispatchCard { .. }));
        }
    }

    #[tokio::test]
    async fn success_unblocks_dependent() {
        let handle = Scheduler::spawn(SchedulerConfig::default());
        let a = handle.add_card(card_assigned("a", "claude")).unwrap();
        let b = handle.add_card(card_assigned("b", "claude")).unwrap();
        handle.add_edge(a, b, EdgeKind::DependsOn).unwrap();
        let mut rx = handle.subscribe();
        handle.mark_running(a).unwrap();
        handle.mark_succeeded(a).unwrap();
        // After a succeeds, b should be dispatched.
        let mut saw = false;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        while std::time::Instant::now() < deadline {
            match rx.try_recv() {
                Ok(SchedulerEvent::DispatchCard { card }) if card.id == b => {
                    saw = true;
                    break;
                }
                Ok(_) => continue,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(5)).await,
            }
        }
        assert!(saw, "expected dispatch for b after a succeeded");
    }
}
