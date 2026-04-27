//! brain-types: the shared domain for the version-control-for-memory engine.
//!
//! Every event, payload, ack level, and identifier used across brain-store,
//! brain-index, brain-app, brain-mcp, and brain-cli originates here.
//!
//! Schema design source of truth lives in the project's plan archive
//! (`brain-event-schema-writeapi.md`, v2 post-Codex adversarial pass).

pub mod ack;
pub mod event;
pub mod payload;
pub mod subject;

pub use ack::*;
pub use event::*;
pub use payload::*;
pub use subject::*;

/// The first on-disk schema version. A brain with any other
/// `schema_version` is rejected by this binary; migration is a separate
/// future workflow. Incremented whenever a breaking change lands in any
/// of the `brain-types` wire shapes.
pub const SCHEMA_VERSION: u32 = 1;
