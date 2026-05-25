//! brain-orchestrator: task graph + dependency resolver for the IDE.
//!
//! The IDE turns a free-form spec into a directed acyclic graph of
//! "feature cards". Each card is owned by an agent (Claude, Codex, a
//! shell session, a human). Cards declare dependencies on other cards.
//! This crate resolves those deps, schedules ready work, and exposes a
//! stream of state transitions the UI can render.
//!
//! Storage of the *durable* task state belongs to the caller (the IDE
//! commits cards/edges as `brain` events through `brain-store`). This
//! crate only owns in-memory state and the scheduling algorithm.

pub mod card;
pub mod graph;
pub mod scheduler;
pub mod spec;

pub use card::{Card, CardId, CardKind, CardStatus, Edge, EdgeKind};
pub use graph::{FeatureGraph, GraphError};
pub use scheduler::{Scheduler, SchedulerEvent, SchedulerHandle};
pub use spec::{SpecBreakdown, SpecParser, SpecParserError};
