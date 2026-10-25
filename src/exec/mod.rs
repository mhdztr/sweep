//! Execution engine for `sweep clean` (M2).
//! SAFETY: mutating runs take lock, re-validate prefixes, no-follow symlinks,
//! continue-on-error, exit codes 0/2/3.

use crate::ctx::Ctx;
use crate::rules::Tier;
use crate::state;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("path outside allowed prefix: {0}")]
    OutsidePrefix(String),
    #[error("protected path: {0}")]
    Protected(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Ok = 0,
    Gate = 2,
    ExecErr = 3,
}

impl From<ExitCode> for i32 {
    fn from(c: ExitCode) -> i32 {
        c as i32
    }
}

#[derive(Debug, Clone)]
pub struct GateAck {
    pub purge_typed: bool,
    pub at: SystemTime,
}

#[derive(Debug, Clone)]
pub struct PlanItem {
    pub id: &'static str,
    pub needs_sudo: bool,
    pub tier: Tier,
    pub paths: Vec<PathBuf>,
    pub purge_ack: bool,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub selected: Vec<PlanItem>,
    pub gate_ack: BTreeMap<String, GateAck>,
    pub keep_days: Option<u32>,
    pub dry_run: bool,
    pub trash: bool,
    pub schema: u32,
}

#[derive(Debug, Clone)]
pub struct ItemReport {
    pub id: &'static str,
    pub freed: u64,
    pub removed: u64,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SudoPassReport {
    pub declined: bool,
    pub failed: bool,
    pub items: Vec<ItemReport>,
}

#[derive(Debug, Clone)]
pub struct RunReport {
    pub items: Vec<ItemReport>,
    pub freed_total: u64,
    pub duration: Duration,
    pub exit_hint: ExitCode,
    pub sudo_pass: SudoPassReport,
}

pub mod del;
pub mod sudo;
pub mod trash;

pub fn execute(ctx: &Ctx, plan: &Plan) -> Result<RunReport, ExecError> {
    let start = std::time::Instant::now();
    // Acquire lock for mutating runs
    let _lock = if !plan.dry_run {
        Some(
            state::acquire_lock(&state::state_dir(&ctx.home))
                .map_err(|e| ExecError::Other(format!("lock: {e}")))?,
        )
    } else {
        None
    };

    let mut items: Vec<ItemReport> = Vec::new();
    let mut freed_total: u64 = 0;
    let mut has_error = false;

    for item in &plan.selected {
        if item.needs_sudo {
            continue;
        }
        if plan.dry_run {
            // Dry-run: print samples up to 20
            eprintln!("dry-run: {} ({} paths)", item.id, item.paths.len());
            items.push(ItemReport {
                id: item.id,
                freed: 0,
                removed: 0,
                skipped: vec!["dry-run".to_string()],
                errors: vec![],
            });
            continue;
        }
        // Execute deletion
        match crate::exec::del::remove_items(ctx, item, plan.trash) {
            Ok(r) => {
                freed_total += r.freed;
                items.push(r);
            }
            Err(e) => {
                has_error = true;
                items.push(ItemReport {
                    id: item.id,
                    freed: 0,
                    removed: 0,
                    skipped: vec![],
                    errors: vec![e.to_string()],
                });
            }
        }
    }

    let exit_hint = if has_error { ExitCode::ExecErr } else { ExitCode::Ok };
    Ok(RunReport {
        items,
        freed_total,
        duration: start.elapsed(),
        exit_hint,
        sudo_pass: SudoPassReport {
            declined: false,
            failed: false,
            items: vec![],
        },
    })
}
