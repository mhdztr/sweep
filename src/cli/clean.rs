//! `sweep clean`: scan, selection, P1 gate, safe execution, and reporting.
//! Spec: `docs/CLI-UX.md` sections 1/2.2/3/4/5/6; gates: `docs/SAFETY.md` section 2.
//!
//! Flow: scan -> `Selection` -> P1 `confirm_p1` (interactive) or preset-selected
//! scripted rows -> candidate-backed plan -> dry-run or safe execution.
//!
//! Deliberately deferred:
//! - destructive PURGE gates, the batched sudo pass, clean JSON output, and
//!   the history viewer;
//! - `--json` for clean (JSON-SCHEMAS section 12, M6 per ROADMAP.md:239);
//! - destructive and sudo execution, which are refused by the M2 plan validator.

use crate::cli::Root;
use crate::ctx::Ctx;
use crate::exec::{self, GateAck, Plan, PlanItem};
use crate::scan::{self, ScanEvent};
use crate::ui::Theme;
use crate::ui::checklist::{Outcome, Selected, Selection, run_interactive};
use crate::ui::confirm::{self, ConfirmItem, ConfirmOutcome};
use crate::ui::format::format_bytes;
use std::collections::BTreeMap;
use std::io::IsTerminal as _;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};

/// CLI-UX section 1/section 5: the copy-locked no-TTY message (stderr, exit 1).
pub const NO_TTY_COPY: &str =
    "error: interactive mode requires a TTY; use --preset/--only with --yes";

/// CLI-UX section 5: the interactive checklist has no JSON form.
pub const JSON_REFUSAL_COPY: &str =
    "error: --json requires --yes/--only/--preset non-interactive flow";

/// CLI-UX section 3.2: accepting an empty selection is a no-op, exit 0.
pub const EMPTY_SELECTION_COPY: &str = "nothing selected, exiting";

/// SAFETY section 2 / CLI-UX section 6: abort copy (stderr, exit 2), nothing was deleted.
pub const ABORT_COPY: &str = "aborted, nothing was deleted";

/// Marker copy used by tests and future scripted-flow diagnostics.
pub const SCRIPTED_MODE_COPY: &str = "scripted mode selects safe and caution rules";

pub fn run(root: Root, only: &[String], exclude: &[String], no_prompt: bool) -> i32 {
    let ctx = match Ctx::build(&root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    let scripted = no_prompt || ctx.flags.yes;
    if !scripted && !std::io::stdin().is_terminal() {
        eprintln!("{NO_TTY_COPY}");
        return 1;
    }

    // Clean JSON output is deferred until the complete clean report schema.
    if ctx.flags.json {
        eprintln!("{JSON_REFUSAL_COPY}");
        return 1;
    }

    // Validate `--only`/`--exclude` against the registry before scanning, so a
    // typo fails fast and never silently drops a category (CLI-UX section 2.2/section 5).
    if let Some(unknown) = first_unknown_rule(only, exclude) {
        eprintln!("error: unknown rule '{unknown}'");
        return 1;
    }

    // M1 spinner pattern (see `cli/scan.rs`): stderr only, never with --quiet.
    let cancel = Arc::new(AtomicBool::new(false));
    let show_spinner = !ctx.flags.quiet && std::io::stderr().is_terminal();
    let (tx, rx) = mpsc::channel::<ScanEvent>();
    let spinner = crate::ui::progress::ScanSpinner::spawn(
        crate::rules::scan_registry().len(),
        rx,
        show_spinner,
        ctx.flags.unicode,
    );
    let mut out = scan::run_with_candidates(&ctx, cancel, Some(tx), true);
    let _ = spinner.join();

    // Option (A): filter the scan results BEFORE building the selection.
    // `exclude` wins over `only` (CLI-UX section 2.2).
    out.results.retain(|r| keep_rule(r.id, only, exclude));

    let guards = process_guards(&ctx);
    let theme = Theme {
        color: ctx.flags.color,
        unicode: ctx.flags.unicode,
    };
    let mut sel = Selection::new(
        &out.results,
        ctx.effective_preset(),
        ctx.flags.yes,
        ctx.flags.i_know_what_im_doing,
        &guards,
    );

    if scripted {
        let items = sel.selected();
        if items.is_empty() {
            println!("{EMPTY_SELECTION_COPY}");
            return 0;
        }
        let gate_ack = caution_acknowledgements(&items, &out.results);
        return execute_selected(&ctx, &out.results, &items, gate_ack);
    }

    match run_interactive(&mut sel, &theme) {
        Err(e) if e.kind() == std::io::ErrorKind::NotConnected => {
            eprintln!("{NO_TTY_COPY}");
            1
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
        Ok(Outcome::Aborted) => {
            // SAFETY section 2: abort deletes nothing, exit 2.
            eprintln!("{ABORT_COPY}");
            2
        }
        Ok(Outcome::Accepted(items)) => {
            if items.is_empty() {
                // CLI-UX section 3.2/section 6: an empty accept is a successful no-op.
                println!("{EMPTY_SELECTION_COPY}");
                return 0;
            }
            // P1 summary (default cancel). Compute destructive count by looking up tiers from registry.
            let confirm_items: Vec<ConfirmItem> = items
                .iter()
                .map(|i| ConfirmItem {
                    id: i.id,
                    bytes: i.bytes,
                    needs_sudo: i.needs_sudo,
                    trash: ctx.flags.trash,
                })
                .collect();
            let guard_items: Vec<_> = items
                .iter()
                .filter(|item| item.guard_overridden)
                .map(|item| confirm::GuardConfirmItem {
                    id: item.id,
                    reason: guards
                        .get(item.id)
                        .cloned()
                        .unwrap_or_else(|| "application state changed".into()),
                })
                .collect();
            if !guard_items.is_empty() {
                match confirm::confirm_p6(&guard_items, &theme) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotConnected => {
                        eprintln!("{NO_TTY_COPY}");
                        return 1;
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        return 1;
                    }
                    Ok(ConfirmOutcome::Abort) => return 2,
                    Ok(ConfirmOutcome::Proceed) => {}
                }
            }
            let mut gate_ack = BTreeMap::new();
            for item in &items {
                let Some(result) = out.results.iter().find(|result| result.id == item.id) else {
                    eprintln!("error: selected rule '{}' has no scan result", item.id);
                    return 1;
                };
                if result.tier != crate::rules::Tier::Caution {
                    continue;
                }
                let outcome = if item.id == "xdg-state-coredumps" {
                    let Some(confirm_item) = confirm_items
                        .iter()
                        .find(|confirm_item| confirm_item.id == item.id)
                        .cloned()
                    else {
                        eprintln!(
                            "error: selected rule '{}' has no confirmation data",
                            item.id
                        );
                        return 1;
                    };
                    confirm::confirm_p2(&[confirm_item], &theme)
                } else {
                    let confirm_item = confirm::RuleConfirmItem {
                        id: result.id,
                        title: result.title,
                        bytes: result.info.bytes,
                        files: result.info.files,
                        note: result.info.note.clone(),
                        samples: result
                            .info
                            .samples
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect(),
                        needs_sudo: result.needs_sudo,
                    };
                    if result.id == "orphans" {
                        let Some(packages) = orphan_confirmation_entries(
                            result.info.note.as_deref(),
                            result.info.files,
                        ) else {
                            eprintln!("error: orphan package list is unavailable for confirmation");
                            return 1;
                        };
                        let batch_size = ctx
                            .config
                            .rules
                            .orphans
                            .confirm_batch
                            .or_else(|| {
                                crate::rules::registry()
                                    .into_iter()
                                    .find(|rule| rule.id() == result.id)
                                    .and_then(|rule| rule.confirm_batch_size())
                            })
                            .unwrap_or(200);
                        confirm::confirm_rule_p2_list(&confirm_item, &packages, batch_size, &theme)
                    } else {
                        confirm::confirm_rule_p2(&confirm_item, &theme)
                    }
                };
                match outcome {
                    Err(e) if e.kind() == std::io::ErrorKind::NotConnected => {
                        eprintln!("{NO_TTY_COPY}");
                        return 1;
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        return 1;
                    }
                    Ok(ConfirmOutcome::Abort) => return 2,
                    Ok(ConfirmOutcome::Proceed) => {
                        gate_ack.insert(
                            item.id.to_string(),
                            GateAck {
                                purge_typed: false,
                                caution_confirmed: true,
                                at: std::time::SystemTime::now(),
                            },
                        );
                    }
                }
            }
            match confirm::confirm_p1(&confirm_items, &theme) {
                Err(e) if e.kind() == std::io::ErrorKind::NotConnected => {
                    eprintln!("{NO_TTY_COPY}");
                    1
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
                Ok(ConfirmOutcome::Abort) => 2,
                Ok(ConfirmOutcome::Proceed) => {
                    execute_selected(&ctx, &out.results, &items, gate_ack)
                }
            }
        }
    }
}

fn process_guards(ctx: &Ctx) -> BTreeMap<&'static str, String> {
    let mut guards = BTreeMap::new();
    let Some(proc_root) = ctx.proc_root.as_deref() else {
        return BTreeMap::new();
    };
    match crate::system::proc::processes(proc_root) {
        Ok(processes) => {
            for (id, names) in [
                ("browser-cache", crate::system::proc::BROWSERS),
                ("editor-cache", crate::system::proc::EDITORS),
                ("app-caches", crate::system::proc::APP_CACHES),
                ("steam-shader", crate::system::proc::STEAM),
            ] {
                if let Some(name) = crate::system::proc::matching_process(&processes, names) {
                    guards.insert(id, format!("{name} running"));
                }
            }
            match crate::system::proc::matching_process_under(
                proc_root,
                &["makepkg", "git"],
                &[
                    ctx.home.join(".cache/paru/clone"),
                    ctx.home.join(".cache/yay"),
                ],
            ) {
                Ok(Some(name)) => {
                    guards.insert("aur-builds", format!("{name} running in an AUR checkout"));
                }
                Ok(None) => {}
                Err(error) => {
                    guards.insert("aur-builds", format!("process state unavailable: {error}"));
                }
            }
        }
        Err(error) => {
            for id in [
                "browser-cache",
                "editor-cache",
                "app-caches",
                "steam-shader",
                "aur-builds",
            ] {
                guards.insert(id, format!("process state unavailable: {error}"));
            }
        }
    }
    guards
}

fn execute_selected(
    ctx: &Ctx,
    results: &[scan::RuleResult],
    items: &[Selected],
    gate_ack: BTreeMap<String, GateAck>,
) -> i32 {
    let plan = Plan {
        selected: build_plan_items(items, results),
        gate_ack,
        keep_days: Some(ctx.keep_days()),
        dry_run: ctx.flags.dry_run,
        trash: ctx.flags.trash,
        schema: crate::SCHEMA_VERSION,
    };
    match exec::execute(ctx, &plan) {
        Ok(report) => report.exit_hint as i32,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("lock") {
                eprintln!("error: another sweep is running");
                return 1;
            }
            eprintln!("error: {e}");
            3
        }
    }
}

fn caution_acknowledgements(
    items: &[Selected],
    results: &[scan::RuleResult],
) -> BTreeMap<String, GateAck> {
    let at = std::time::SystemTime::now();
    let mut acknowledgements = BTreeMap::new();
    for item in items {
        let Some(result) = results.iter().find(|result| result.id == item.id) else {
            continue;
        };
        if result.tier == crate::rules::Tier::Caution {
            acknowledgements.insert(
                item.id.to_string(),
                GateAck {
                    purge_typed: false,
                    caution_confirmed: true,
                    at,
                },
            );
        }
    }
    acknowledgements
}

fn orphan_confirmation_entries(note: Option<&str>, count: u64) -> Option<Vec<String>> {
    if count == 0 {
        return Some(Vec::new());
    }
    let (_, entries) = note?.split_once(" · ")?;
    let entries: Vec<_> = entries.split(", ").map(str::to_string).collect();
    (entries.len() as u64 == count && entries.iter().all(|entry| !entry.is_empty()))
        .then_some(entries)
}

/// Whether a scan result survives `--only`/`--exclude` (CLI-UX section 2.2).
///
/// `only` empty means "every rule"; `exclude` always wins, so an id listed in
/// both is dropped.
fn keep_rule(id: &str, only: &[String], exclude: &[String]) -> bool {
    let in_only = only.is_empty() || only.iter().any(|wanted| wanted == id);
    let excluded = exclude.iter().any(|banned| banned == id);
    in_only && !excluded
}

/// First requested rule id that the registry does not know, if any.
///
/// The plan's M6 task adds an edit-distance suggestion to the error copy
/// (ROADMAP.md:240); this slice prints the plain copy locked by the
/// orchestrator: `error: unknown rule '<id>'`.
fn first_unknown_rule<'a>(only: &'a [String], exclude: &'a [String]) -> Option<&'a str> {
    let known: Vec<&str> = crate::rules::scan_registry()
        .iter()
        .map(|r| r.id())
        .collect();
    only.iter()
        .chain(exclude.iter())
        .find(|id| !known.contains(&id.as_str()))
        .map(String::as_str)
}

/// Render the accepted selection as a plain, pure-ASCII preview.
///
/// PURE: no I/O, no terminal state, no theme-dependent bytes. `theme` is
/// accepted for signature stability with the rest of the UI layer but is
/// deliberately unused: this is new prose, and D13/D16 require every byte to
/// be ASCII, so no non-ASCII bullets/separators and no SGR are emitted even
/// when color is on. The shape mirrors the SAFETY section 2 invariant 2 dry-run
/// summary minus samples/argv (those need `exec/`, which does not exist yet).
pub fn selection_preview(items: &[Selected], _theme: &Theme) -> String {
    let total: u64 = items.iter().map(|item| item.bytes).sum();
    let noun = if items.len() == 1 {
        "category"
    } else {
        "categories"
    };
    let mut out = format!(
        "selected {} {noun} | {} would be deleted (preview only; nothing deleted yet)\n",
        items.len(),
        format_bytes(total)
    );
    for item in items {
        out.push_str("  - ");
        out.push_str(item.id);
        out.push_str("  ");
        out.push_str(&format_bytes(item.bytes));
        if item.needs_sudo {
            out.push_str("  [sudo]");
        }
        out.push('\n');
    }
    out
}

/// Build an execution plan from the scan's full eligible candidate paths.
/// Samples are display-only and must never become deletion targets.
pub fn build_plan_items(selected: &[Selected], results: &[scan::RuleResult]) -> Vec<PlanItem> {
    let by_id: BTreeMap<&str, &scan::RuleResult> =
        results.iter().map(|result| (result.id, result)).collect();
    selected
        .iter()
        .filter_map(|item| {
            by_id.get(item.id).map(|result| PlanItem {
                id: item.id,
                needs_sudo: result.needs_sudo,
                tier: result.tier,
                paths: result.info.candidates.clone(),
                purge_ack: false,
                guard_overridden: item.guard_overridden,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_and_exclude_filter_with_exclude_precedence() {
        let only = vec!["tmp".to_string(), "trash".to_string()];
        let exclude = vec!["trash".to_string()];
        assert!(keep_rule("tmp", &only, &exclude));
        assert!(!keep_rule("trash", &only, &exclude));
        assert!(!keep_rule("user-cache", &only, &exclude));

        let none: Vec<String> = Vec::new();
        assert!(keep_rule("tmp", &none, &none));
        let excluded = ["tmp".to_string()];
        assert!(!keep_rule("tmp", &none, &excluded));
    }

    #[test]
    fn unknown_ids_are_found_in_either_list() {
        let bogus = vec!["bogus".to_string()];
        let none: Vec<String> = Vec::new();
        assert_eq!(first_unknown_rule(&bogus, &none), Some("bogus"));
        assert_eq!(first_unknown_rule(&none, &bogus), Some("bogus"));

        let known = vec!["tmp".to_string()];
        assert_eq!(first_unknown_rule(&known, &none), None);
    }

    #[test]
    fn orphan_confirmation_list_requires_a_complete_scan_note() {
        assert_eq!(
            orphan_confirmation_entries(Some("2 · alpha, beta"), 2),
            Some(vec!["alpha".into(), "beta".into()])
        );
        assert_eq!(
            orphan_confirmation_entries(Some("0 orphans found"), 0),
            Some(vec![])
        );
        assert_eq!(orphan_confirmation_entries(Some("2 · alpha"), 2), None);
        assert_eq!(orphan_confirmation_entries(None, 1), None);
    }

    #[test]
    fn copy_constants_stay_ascii_and_exact() {
        assert_eq!(EMPTY_SELECTION_COPY, "nothing selected, exiting");
        assert_eq!(ABORT_COPY, "aborted, nothing was deleted");
        assert!(NO_TTY_COPY.is_ascii());
        assert!(JSON_REFUSAL_COPY.is_ascii());
        assert!(SCRIPTED_MODE_COPY.is_ascii());
    }
}
