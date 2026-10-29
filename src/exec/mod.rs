//! Execution engine for `sweep clean` (M2).
//! SAFETY: mutating runs take lock, re-validate prefixes, no-follow symlinks,
//! continue-on-error, exit codes 0/2/3.

use crate::ctx::Ctx;
use crate::rules::Tier;
use crate::state;
use crate::ui::progress::RuleProgress;
use std::collections::BTreeMap;
use std::fmt::Write as _;
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

fn chrono_like_ts() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}Z", state::run_id_from_unix(secs))
}

pub fn execute(ctx: &Ctx, plan: &Plan) -> Result<RunReport, ExecError> {
    let start = std::time::Instant::now();
    let run_id = state::run_id_now();
    // Acquire lock for mutating runs
    let _lock = if !plan.dry_run {
        Some(
            state::acquire_lock(&state::state_dir(&ctx.home))
                .map_err(|e| ExecError::Other(format!("lock: {e}")))?,
        )
    } else {
        None
    };

    let progress = RuleProgress::new(!ctx.flags.quiet && std::io::stderr().is_terminal(), ctx.flags.unicode);

    let mut items: Vec<ItemReport> = Vec::new();
    let mut freed_total: u64 = 0;
    let mut has_error = false;

    for item in &plan.selected {
        progress.start_rule(item.id);
        if item.needs_sudo {
            progress.finish_rule(item.id, true);
            continue;
        }
        if plan.dry_run {
            // Dry-run: print samples up to 20 + argv per rule (no sh -c)
            eprintln!("dry-run: {} ({} paths)", item.id, item.paths.len());
            let mut samples_shown = 0;
            for p in &item.paths {
                if samples_shown >= 20 {
                    break;
                }
                eprintln!("  sample: {}", p.display());
                samples_shown += 1;
            }
            if !item.paths.is_empty() {
                let mut argv_parts = vec!["rm".to_string(), "-rf".to_string()];
                for p in item.paths.iter().take(3) {
                    argv_parts.push(p.display().to_string());
                }
                if item.paths.len() > 3 {
                    argv_parts.push(format!("... (+{} more)", item.paths.len() - 3));
                }
                eprintln!("  argv: {}", argv_parts.join(" "));
            }
            items.push(ItemReport {
                id: item.id,
                freed: 0,
                removed: 0,
                skipped: vec!["dry-run".to_string()],
                errors: vec![],
            });
            progress.finish_rule(item.id, true);
            continue;
        }
        // Execute deletion
        match crate::exec::del::remove_items(ctx, item, plan.trash) {
            Ok(r) => {
                freed_total += r.freed;
                let ok = r.errors.is_empty();
                items.push(r);
                progress.finish_rule(item.id, ok);
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
                progress.finish_rule(item.id, false);
            }
        }
    }

    progress.finish();
    let duration = start.elapsed();
    let exit_hint = if has_error { ExitCode::ExecErr } else { ExitCode::Ok };

    let state_dir_path = state::state_dir(&ctx.home);
    let mut log_content = String::new();
    let _ = writeln!(log_content, "run_id: {}", run_id);
    let _ = writeln!(log_content, "preset: {}", ctx.preset_name());
    let _ = writeln!(log_content, "dry_run: {}", plan.dry_run);
    let _ = writeln!(log_content, "freed_total: {}", freed_total);
    for it in &items {
        let _ = writeln!(
            log_content,
            "item: {} freed={} removed={} skipped={} errors={}",
            it.id,
            it.freed,
            it.removed,
            it.skipped.len(),
            it.errors.len()
        );
        for s in &it.skipped {
            let _ = writeln!(log_content, "  skip: {}", s);
        }
        for e in &it.errors {
            let _ = writeln!(log_content, "  error: {}", e);
        }
    }
    let _ = state::write_action_log(&state_dir_path, &run_id, &log_content);
    let _ = state::prune_logs(&state_dir_path, 30);

    let history_items: Vec<state::HistoryItem> = items
        .iter()
        .map(|it| state::HistoryItem {
            id: it.id.to_string(),
            freed: it.freed,
            removed: it.removed,
            skipped: it.skipped.clone(),
        })
        .collect();
    let ts = chrono_like_ts();
    let record = state::HistoryRecord {
        v: 1,
        ts,
        preset: ctx.preset_name().to_string(),
        mode: "interactive".to_string(),
        items: history_items,
        freed_total,
        duration_ms: duration.as_millis() as u64,
        dry_run: plan.dry_run,
        trash: plan.trash,
    };
    let _ = state::append_history(&state_dir_path, &record);
    let keep = ctx.config.general.history_keep_runs;
    let _ = state::prune_history(&state_dir_path, if keep == 0 { 50 } else { keep });

    Ok(RunReport {
        items,
        freed_total,
        duration,
        exit_hint,
        sudo_pass: SudoPassReport {
            declined: false,
            failed: false,
            items: vec![],
        },
    })
}
use std::io::IsTerminal as _;
