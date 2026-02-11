//! Episodic memory layer for crew-rs.
//!
//! This crate provides persistent memory for agents:
//! - Episode storage (summaries of completed tasks)
//! - Task state persistence
//! - Context handoff between agents
//!
//! Designed to wrap/extend codex-state when that dependency is enabled.

mod episode;
mod memory_store;
mod store;
mod task_store;

pub use episode::{Episode, EpisodeOutcome};
pub use memory_store::MemoryStore;
pub use store::EpisodeStore;
pub use task_store::{TaskState, TaskStore};
