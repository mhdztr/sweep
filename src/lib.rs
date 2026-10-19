//! sweep — advanced, safety-first storage cleaner for Linux.
//!
//! Library target exists so integration tests (and the CLI) share the same
//! code paths. See `docs/ARCHITECTURE.md` for module responsibilities.

pub mod cli;
pub mod env;

/// Plan/schema version for JSON outputs (`kind` + `version` fields).
pub const SCHEMA_VERSION: u32 = 1;
