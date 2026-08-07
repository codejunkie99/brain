//! In-memory feature graph + dependency resolver.
//!
//! `FeatureGraph` is the source of truth for card state during a session.
//! The IDE persists snapshots through `brain-store` events, but mutations
//! and queries during execution happen here.
//!
//! Two operations matter:
//!   - `ready_cards()`: cards whose dependencies have all Succeeded.
//!     The scheduler pops these to dispatch to agents.
//!   - `propagate(card_id)`: when a card transitions to a terminal
//!     state, recompute the status of its descendants — Ready (newly
//!     unblocked) or Blocked (dep failed).

use std::collections::{HashMap, HashSet};

use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::card::{Card, CardId, CardStatus, Edge, EdgeKind};

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("unknown card: {0}")]
    UnknownCard(CardId),
    #[error("adding edge {from} -> {to} would create a cycle")]
    WouldCycle { from: CardId, to: CardId },
    #[error("duplicate card id: {0}")]
    DuplicateCard(CardId),
}

/// Snapshot the UI gets to render. The graph clones into this rather
/// than handing out internal node indices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSnapshot {
    pub cards: Vec<Card>,
    pub edges: Vec<Edge>,
}

pub struct FeatureGraph {
    g: DiGraph<Card, EdgeKind>,
    by_id: HashMap<CardId, NodeIndex>,
}

impl Default for FeatureGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureGraph {
    pub fn new() -> Self {
        Self {
            g: DiGraph::new(),
            by_id: HashMap::new(),
        }
    }

    pub fn from_snapshot(snapshot: GraphSnapshot) -> Result<Self, GraphError> {
        let mut graph = Self::new();
        for card in snapshot.cards {
            graph.add_card(card)?;
        }
        for edge in snapshot.edges {
            graph.add_edge(edge.from, edge.to, edge.kind)?;
        }
        Ok(graph)
    }

    pub fn snapshot(&self) -> GraphSnapshot {
        let cards: Vec<Card> = self.g.node_weights().cloned().collect();
        let edges: Vec<Edge> = self
            .g
            .edge_indices()
            .map(|ei| {
                let (a, b) = self.g.edge_endpoints(ei).expect("edge endpoints");
                Edge {
                    from: self.g[a].id,
                    to: self.g[b].id,
                    kind: *self.g.edge_weight(ei).expect("edge weight"),
                }
            })
            .collect();
        GraphSnapshot { cards, edges }
    }

    pub fn len(&self) -> usize {
        self.g.node_count()
    }

    pub fn is_empty(&self) -> bool {
        self.g.node_count() == 0
    }

    pub fn add_card(&mut self, card: Card) -> Result<CardId, GraphError> {
        if self.by_id.contains_key(&card.id) {
            return Err(GraphError::DuplicateCard(card.id));
        }
        let id = card.id;
        let idx = self.g.add_node(card);
        self.by_id.insert(id, idx);
        Ok(id)
    }

    pub fn remove_card(&mut self, id: CardId) -> Result<Card, GraphError> {
        let idx = self.idx_of(id)?;
        let card = self.g.remove_node(idx).ok_or(GraphError::UnknownCard(id))?;
        self.by_id.remove(&id);
        // After remove_node, NodeIndices shift. Rebuild the index map.
        self.by_id = self
            .g
            .node_indices()
            .map(|i| (self.g[i].id, i))
            .collect();
        Ok(card)
    }

    pub fn get(&self, id: CardId) -> Result<&Card, GraphError> {
        let idx = self.idx_of(id)?;
        Ok(&self.g[idx])
    }

    pub fn get_mut(&mut self, id: CardId) -> Result<&mut Card, GraphError> {
        let idx = self.idx_of(id)?;
        Ok(&mut self.g[idx])
    }

    pub fn add_edge(
        &mut self,
        from: CardId,
        to: CardId,
        kind: EdgeKind,
    ) -> Result<(), GraphError> {
        let from_idx = self.idx_of(from)?;
        let to_idx = self.idx_of(to)?;

        // For real dependency edges, refuse cycles. Informational edges
        // (DiscoveredBy) are allowed to violate DAG-ness because they
        // are not consulted by the scheduler.
        if kind == EdgeKind::DependsOn && self.would_create_cycle(from_idx, to_idx) {
            return Err(GraphError::WouldCycle { from, to });
        }
        self.g.add_edge(from_idx, to_idx, kind);
        Ok(())
    }

    /// Cards whose status is currently `Ready`. The scheduler dispatches
    /// these in creation order (UUIDv7 is monotonic, so iteration over
    /// the underlying node store roughly preserves it; we sort to be
    /// explicit).
    pub fn ready_cards(&self) -> Vec<CardId> {
        let mut out: Vec<CardId> = self
            .g
            .node_weights()
            .filter(|c| c.status == CardStatus::Ready)
            .map(|c| c.id)
            .collect();
        out.sort_by_key(|id| id.0);
        out
    }

    /// All cards in the graph, sorted by creation time.
    pub fn cards(&self) -> Vec<Card> {
        let mut out: Vec<Card> = self.g.node_weights().cloned().collect();
        out.sort_by_key(|c| c.created_at);
        out
    }

    /// Initial pass: any card with no unsatisfied dependencies becomes
    /// Ready. Called once when the graph is hydrated and again whenever
    /// the spec parser dumps new cards into the graph.
    pub fn rescan(&mut self) {
        let ids: Vec<CardId> = self.g.node_weights().map(|c| c.id).collect();
        for id in ids {
            self.refresh_status(id);
        }
    }

    /// Mark a card as the given status and propagate to descendants.
    /// Returns the set of cards whose status changed (for the UI to
    /// re-render and the scheduler to dispatch).
    pub fn transition(
        &mut self,
        id: CardId,
        new_status: CardStatus,
    ) -> Result<HashSet<CardId>, GraphError> {
        let idx = self.idx_of(id)?;
        let mut changed = HashSet::new();

        let before = self.g[idx].status;
        if before == new_status {
            return Ok(changed);
        }
        self.g[idx].status = new_status;
        self.g[idx].updated_at = chrono::Utc::now();
        changed.insert(id);

        // Walk descendants and refresh their status. We do this via a
        // worklist so the propagation cascades — a card that flips to
        // Blocked may unblock or re-block its own descendants.
        let mut work: Vec<CardId> = self.descendants(id);
        while let Some(d) = work.pop() {
            if changed.contains(&d) {
                continue;
            }
            let before = self.g[self.idx_of(d)?].status;
            self.refresh_status(d);
            let after = self.g[self.idx_of(d)?].status;
            if before != after {
                changed.insert(d);
                work.extend(self.descendants(d));
            }
        }
        Ok(changed)
    }

    /// Recompute the status of a single card from the status of its
    /// dependencies. Only flips between Pending / Ready / Blocked.
    /// Cards in Running / Succeeded / Failed / Cancelled retain their
    /// status — they have outcomes, not derivations.
    fn refresh_status(&mut self, id: CardId) {
        let Ok(idx) = self.idx_of(id) else {
            return;
        };
        let current = self.g[idx].status;
        if matches!(
            current,
            CardStatus::Running
                | CardStatus::Succeeded
                | CardStatus::Failed
                | CardStatus::Cancelled
        ) {
            return;
        }

        // Look at incoming DependsOn edges only.
        let deps: Vec<CardStatus> = self
            .g
            .edges_directed(idx, Direction::Incoming)
            .filter(|e| *e.weight() == EdgeKind::DependsOn)
            .map(|e| self.g[e.source()].status)
            .collect();

        let new_status = if deps
            .iter()
            .any(|s| matches!(s, CardStatus::Failed | CardStatus::Cancelled | CardStatus::Blocked))
        {
            CardStatus::Blocked
        } else if deps.iter().all(|s| *s == CardStatus::Succeeded) {
            CardStatus::Ready
        } else {
            CardStatus::Pending
        };

        if current != new_status {
            self.g[idx].status = new_status;
            self.g[idx].updated_at = chrono::Utc::now();
        }
    }

    fn descendants(&self, id: CardId) -> Vec<CardId> {
        let Ok(idx) = self.idx_of(id) else {
            return Vec::new();
        };
        self.g
            .edges_directed(idx, Direction::Outgoing)
            .filter(|e| *e.weight() == EdgeKind::DependsOn)
            .map(|e| self.g[e.target()].id)
            .collect()
    }

    fn idx_of(&self, id: CardId) -> Result<NodeIndex, GraphError> {
        self.by_id.get(&id).copied().ok_or(GraphError::UnknownCard(id))
    }

    fn would_create_cycle(&self, from: NodeIndex, to: NodeIndex) -> bool {
        // Adding from -> to creates a cycle iff `from` is already
        // reachable from `to`. DFS check.
        if from == to {
            return true;
        }
        let mut stack = vec![to];
        let mut seen = HashSet::new();
        while let Some(n) = stack.pop() {
            if !seen.insert(n) {
                continue;
            }
            if n == from {
                return true;
            }
            for edge in self.g.edges_directed(n, Direction::Outgoing) {
                if *edge.weight() == EdgeKind::DependsOn {
                    stack.push(edge.target());
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, CardKind};

    fn card(title: &str) -> Card {
        Card::new(title, "", CardKind::Feature)
    }

    #[test]
    fn rescan_marks_no_dep_cards_ready() {
        let mut g = FeatureGraph::new();
        let a = g.add_card(card("a")).unwrap();
        let b = g.add_card(card("b")).unwrap();
        g.add_edge(a, b, EdgeKind::DependsOn).unwrap();
        g.rescan();
        assert_eq!(g.get(a).unwrap().status, CardStatus::Ready);
        assert_eq!(g.get(b).unwrap().status, CardStatus::Pending);
    }

    #[test]
    fn success_unblocks_dependent() {
        let mut g = FeatureGraph::new();
        let a = g.add_card(card("a")).unwrap();
        let b = g.add_card(card("b")).unwrap();
        g.add_edge(a, b, EdgeKind::DependsOn).unwrap();
        g.rescan();
        let changed = g.transition(a, CardStatus::Succeeded).unwrap();
        assert!(changed.contains(&a));
        assert!(changed.contains(&b));
        assert_eq!(g.get(b).unwrap().status, CardStatus::Ready);
    }

    #[test]
    fn failure_blocks_downstream() {
        let mut g = FeatureGraph::new();
        let a = g.add_card(card("a")).unwrap();
        let b = g.add_card(card("b")).unwrap();
        let c = g.add_card(card("c")).unwrap();
        g.add_edge(a, b, EdgeKind::DependsOn).unwrap();
        g.add_edge(b, c, EdgeKind::DependsOn).unwrap();
        g.rescan();
        g.transition(a, CardStatus::Failed).unwrap();
        assert_eq!(g.get(b).unwrap().status, CardStatus::Blocked);
        assert_eq!(g.get(c).unwrap().status, CardStatus::Blocked);
    }

    #[test]
    fn cycle_refused() {
        let mut g = FeatureGraph::new();
        let a = g.add_card(card("a")).unwrap();
        let b = g.add_card(card("b")).unwrap();
        g.add_edge(a, b, EdgeKind::DependsOn).unwrap();
        let err = g.add_edge(b, a, EdgeKind::DependsOn).unwrap_err();
        assert!(matches!(err, GraphError::WouldCycle { .. }));
    }

    #[test]
    fn snapshot_round_trip() {
        let mut g = FeatureGraph::new();
        let a = g.add_card(card("a")).unwrap();
        let b = g.add_card(card("b")).unwrap();
        g.add_edge(a, b, EdgeKind::DependsOn).unwrap();
        let snap = g.snapshot();
        let g2 = FeatureGraph::from_snapshot(snap).unwrap();
        assert_eq!(g2.len(), 2);
    }
}
