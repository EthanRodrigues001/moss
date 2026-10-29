//! Sync with Overleaf: Pull, Push and Sync, as a three-way merge against the Base snapshot.
//! See ADR 0005 and `docs/plans/m2-sync-and-versions.md`.
//!
//! The flow is always two steps. [`Syncer::preview`] changes nothing and returns what would
//! happen (a Review). [`Syncer::apply`] fetches Overleaf again, refuses if either side moved since
//! the Review, and then does it.

mod engine;
pub mod fileset;
pub mod link;
mod lock;
mod plan;
pub mod remote;

#[cfg(test)]
mod tests;

pub use engine::{
    ApplyRequest, Applied, Failure, LocalChange, MassDelete, Outcome, Preview, Staged, StagedFile, StaleSide,
    Summary, Syncer, local_changes,
};
pub use fileset::{Blob, FileSet};
pub use plan::{Conflict, ConflictKind, Direction, FileChoice, Item, Mode, Op, Resolution};
pub use remote::{Fetched, FolderRemote, MemoryRemote, Remote};
