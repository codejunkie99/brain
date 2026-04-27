//! brain-store: libgit2-backed append-only storage for brain events.
//!
//! v0.1 scope: `BrainRepo::init / open / append_event / read_event`.
//! Batched writer, writer lease, tag snapshots, and signing are future work.

pub mod error;
pub mod repo;
pub mod secrets;

pub use error::StoreError;
pub use repo::BrainRepo;

// Re-export the types crate so callers of brain-store don't need to
// import brain_types separately.
pub use brain_types;
