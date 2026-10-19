//! sweep — advanced, safety-first storage cleaner for Linux.
//!
//! Library target exists so integration tests (and the CLI) share the same
//! code paths. See `docs/ARCHITECTURE.md` for module responsibilities.

pub mod cli;
pub mod config;
pub mod ctx;
pub mod env;
pub mod report;
pub mod rules;
pub mod scan;
pub mod state;
pub mod system;
pub mod ui;

/// Plan/schema version for JSON outputs (`kind` + `version` fields).
pub const SCHEMA_VERSION: u32 = 1;
