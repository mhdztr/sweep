//! Execution engine for `sweep clean` (M2).
//! SAFETY: mutating runs take lock, re-validate prefixes, no-follow symlinks,
//! continue-on-error, exit codes 0/2/3.

use crate::ctx::Ctx;
use crate::rules::Tier;
use crate::state;
use crate::ui::progress::RuleProgress;
use crate::ui::summary;
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
    pub guard_overridden: bool,
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

impl Plan {
    /// Validate the portion of the plan supported by the M2 user pass.
    pub fn validate(&self) -> Result<(), ExecError> {
        if self.schema != crate::SCHEMA_VERSION {
            return Err(ExecError::Other(format!(
                "unsupported plan schema {}; expected {}",
                self.schema,
                crate::SCHEMA_VERSION
            )));
        }
        if self
            .selected
            .iter()
            .any(|item| item.tier == Tier::Destructive)
        {
            return Err(ExecError::Other(
                "destructive rules require the typed PURGE gate".to_string(),
            ));
        }
        if self.selected.iter().any(|item| item.needs_sudo) {
            return Err(ExecError::Other(
                "sudo system pass is not available in this build".to_string(),
            ));
        }
        if self.trash
            && self
                .selected
                .iter()
                .any(|item| matches!(item.id, "trash" | "journal-user"))
        {
            return Err(ExecError::Other(
                "selected rule cannot be staged in XDG trash".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ItemReport {
    pub id: &'static str,
    pub freed: u64,
    pub removed: u64,
    pub staged: u64,
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
    pub dry_run: bool,
    pub trash: bool,
    pub aborted: bool,
    pub exit_hint: ExitCode,
    pub sudo_pass: SudoPassReport,
}

pub mod del;
pub mod sudo;
pub mod trash;

fn run_user_journal(ctx: &Ctx, item: &PlanItem) -> ItemReport {
    let max_size = &ctx.config.rules.journal_user.max_size;
    let before = journal_usage();
    let output = std::process::Command::new("journalctl")
        .arg("--user")
        .arg(format!("--vacuum-size={max_size}"))
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let after = journal_usage();
            let freed = match (before, after) {
                (Some(before), Some(after)) => before.saturating_sub(after),
                _ => {
                    eprintln!("warning: unable to measure user journal size after vacuum");
                    0
                }
            };
            ItemReport {
                id: item.id,
                freed,
                removed: 1,
                staged: 0,
                skipped: Vec::new(),
                errors: Vec::new(),
            }
        }
        Ok(output) => ItemReport {
            id: item.id,
            freed: 0,
            removed: 0,
            staged: 0,
            skipped: Vec::new(),
            errors: vec![format!(
                "journalctl --user vacuum failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )],
        },
        Err(error) => ItemReport {
            id: item.id,
            freed: 0,
            removed: 0,
            staged: 0,
            skipped: Vec::new(),
            errors: vec![format!("cannot run journalctl: {error}")],
        },
    }
}

fn journal_usage() -> Option<u64> {
    let output = std::process::Command::new("journalctl")
        .args(["--user", "--disk-usage"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace().find_map(parse_human_size)
}

fn parse_human_size(token: &str) -> Option<u64> {
    let (number, unit) = token.trim_end_matches(',').split_at(
        token
            .find(|character: char| !character.is_ascii_digit() && character != '.')
            .unwrap_or(token.len()),
    );
    let number: f64 = number.parse().ok()?;
    let unit = unit.trim_end_matches("B").trim_end_matches("i");
    let power = match unit.to_ascii_uppercase().as_str() {
        "" => 0,
        "K" => 1,
        "M" => 2,
        "G" => 3,
        "T" => 4,
        _ => return None,
    };
    Some((number * 1024_f64.powi(power)).round() as u64)
}

fn item_report_to_log(items: &[ItemReport]) -> String {
    let mut s = String::new();
    for it in items {
        s.push_str(&format!(
            "[{}] removed={} staged={} freed={} skipped={} errors={}\n",
            it.id,
            it.removed,
            it.staged,
            it.freed,
            it.skipped.len(),
            it.errors.len()
        ));
        for sk in &it.skipped {
            s.push_str(&format!("  SKIP: {}\n", sk));
        }
        for e in &it.errors {
            s.push_str(&format!("  ERR: {}\n", e));
        }
    }
    s
}

fn rfc3339_z(secs: u64) -> String {
    let rid = state::run_id_from_unix(secs);
    if rid.len() >= 15 {
        format!(
            "{}-{}-{}T{}:{}:{}Z",
            &rid[0..4],
            &rid[4..6],
            &rid[6..8],
            &rid[9..11],
            &rid[11..13],
            &rid[13..15]
        )
    } else {
        format!("{}Z", secs)
    }
}

pub fn execute(ctx: &Ctx, plan: &Plan) -> Result<RunReport, ExecError> {
    plan.validate()?;
    let start = std::time::Instant::now();
    let lock_guard = if !plan.dry_run {
        Some(
            state::acquire_lock(&state::state_dir(&ctx.home)).map_err(|e| match e {
                state::LockError::Held { .. } => ExecError::Other(format!("lock: {e}")),
                other => ExecError::Other(format!("lock: {other}")),
            })?,
        )
    } else {
        None
    };
    let _signals = if plan.dry_run {
        None
    } else {
        Some(crate::ui::tty::arm().map_err(ExecError::Io)?)
    };

    let progress = RuleProgress::new(plan.selected.len(), !ctx.flags.quiet && !ctx.flags.json);
    progress.start();

    let mut items: Vec<ItemReport> = Vec::new();
    let mut freed_total: u64 = 0;
    let mut has_error = false;
    let mut aborted = false;

    for (index, item) in plan.selected.iter().enumerate() {
        if !plan.dry_run && crate::ui::tty::interrupted() {
            aborted = true;
            record_unstarted(&plan.selected[index..], &mut items, &progress);
            break;
        }
        progress.start_rule(item.id);
        if item.needs_sudo {
            progress.finish_rule();
            continue;
        }
        if item.id == "journal-user" {
            if plan.dry_run {
                eprintln!(
                    "dry-run: journal-user\n  argv: journalctl --user --vacuum-size={}",
                    ctx.config.rules.journal_user.max_size
                );
                items.push(ItemReport {
                    id: item.id,
                    freed: 0,
                    removed: 0,
                    staged: 0,
                    skipped: vec!["dry-run".to_string()],
                    errors: vec![],
                });
            } else {
                let report = run_user_journal(ctx, item);
                has_error |= !report.errors.is_empty();
                freed_total += report.freed;
                items.push(report);
            }
            progress.finish_rule();
            continue;
        }
        if plan.dry_run {
            eprintln!("dry-run: {} ({} paths)", item.id, item.paths.len());
            for p in item.paths.iter().take(20) {
                eprintln!("  sample: {}", p.display());
            }
            if !item.paths.is_empty() {
                let mut argv_parts = if plan.trash {
                    vec!["trash".to_string()]
                } else {
                    vec!["rm".to_string(), "-rf".to_string()]
                };
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
                staged: 0,
                skipped: vec!["dry-run".to_string()],
                errors: vec![],
            });
            progress.finish_rule();
            continue;
        }
        match del::remove_items(ctx, item, plan.trash, plan.keep_days) {
            Ok(r) => {
                if !r.errors.is_empty() {
                    has_error = true;
                }
                freed_total += r.freed;
                items.push(r);
            }
            Err(e) => {
                has_error = true;
                items.push(ItemReport {
                    id: item.id,
                    freed: 0,
                    removed: 0,
                    staged: 0,
                    skipped: vec![],
                    errors: vec![e.to_string()],
                });
            }
        }
        progress.finish_rule();
        if !plan.dry_run && crate::ui::tty::interrupted() {
            aborted = true;
            record_unstarted(&plan.selected[index + 1..], &mut items, &progress);
            break;
        }
    }
    progress.finish();

    aborted |= !plan.dry_run && crate::ui::tty::interrupted();
    if aborted {
        eprintln!("execution interrupted; remaining rules skipped");
    }
    let exit_hint = if aborted {
        ExitCode::Gate
    } else if has_error {
        ExitCode::ExecErr
    } else {
        ExitCode::Ok
    };
    let duration = start.elapsed();

    if !plan.dry_run {
        let run_id = state::run_id_now();
        let state_dir = state::state_dir(&ctx.home);
        let log_content = item_report_to_log(&items);
        if let Err(e) = state::write_action_log(&state_dir, &run_id, &log_content) {
            eprintln!("warning: failed to write action log: {e}");
        }
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let record = state::HistoryRecord {
            v: crate::SCHEMA_VERSION,
            ts: rfc3339_z(now_secs),
            preset: ctx.preset_name().to_string(),
            mode: "clean".to_string(),
            items: items
                .iter()
                .map(|it| state::HistoryItem {
                    id: it.id.to_string(),
                    freed: it.freed,
                    removed: it.removed,
                    skipped: it.skipped.clone(),
                })
                .collect(),
            freed_total,
            duration_ms: duration.as_millis() as u64,
            dry_run: false,
            trash: plan.trash,
        };
        if let Err(e) = state::append_history(&state_dir, &record) {
            eprintln!("warning: failed to append history: {e}");
        }
        let _ = state::prune_logs(&state_dir, 30);
        let _ = state::prune_history(&state_dir, ctx.config.general.history_keep_runs);
        drop(lock_guard);
    }

    let report = RunReport {
        items,
        freed_total,
        duration,
        dry_run: plan.dry_run,
        trash: plan.trash,
        aborted,
        exit_hint,
        sudo_pass: SudoPassReport {
            declined: false,
            failed: false,
            items: vec![],
        },
    };
    summary::print_summary(&report);
    Ok(report)
}

fn record_unstarted(items: &[PlanItem], reports: &mut Vec<ItemReport>, progress: &RuleProgress) {
    for item in items {
        progress.start_rule(item.id);
        reports.push(ItemReport {
            id: item.id,
            freed: 0,
            removed: 0,
            staged: 0,
            skipped: vec!["aborted".into()],
            errors: Vec::new(),
        });
        progress.finish_rule();
    }
}
