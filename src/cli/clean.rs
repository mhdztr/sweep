//! `sweep clean`: scan, interactive checklist, P1 gate, accepted-selection preview.
//! Spec: `docs/CLI-UX.md` sections 1/2.2/3/4/5/6; gates: `docs/SAFETY.md` section 2.
//!
//! Flow: scan -> `Selection` -> `run_interactive` -> P1 `confirm_p1` (raw mode,
//! signal-safe, only `y`/`Y` proceeds) -> print accepted selection or abort.
//! It performs ZERO mutations and takes NO lock (SAFETY section 10: the lock is
//! for mutating runs only).
//!
//! Deliberately deferred (all still M2/M4 tasks):
//! - `exec/` deletion, `Plan`, dry-run sample/argv printing, `--trash`,
//!   history/log writes, the batched sudo pass and exit code 3;
//! - `--json` for clean (JSON-SCHEMAS section 12, M6 per ROADMAP.md:239);
//! - `--no-prompt`, which needs the scripted `--yes` flow and `exec/`, so it is
//!   refused here instead of being silently ignored.

use crate::cli::Root;
use crate::ctx::Ctx;
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

/// This slice has no non-interactive flow yet, so `--no-prompt` is refused.
pub const NO_PROMPT_REFUSAL_COPY: &str =
    "error: --no-prompt requires the non-interactive flow (not implemented yet)";

pub fn run(root: Root, only: &[String], exclude: &[String], no_prompt: bool) -> i32 {
    let ctx = match Ctx::build(&root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    // CLI-UX section 1/section 5: fail fast, before any scan or raw-mode setup. Same
    // predicate as `tty::RawMode::enter` (`isatty` on fd 0); the NotConnected
    // mapping below is a belt-and-braces fallback if stdin closes between this
    // preflight and the loop.
    if !std::io::stdin().is_terminal() {
        eprintln!("{NO_TTY_COPY}");
        return 1;
    }

    // CLI-UX section 5: `--json` is refused for the interactive flow. There is no
    // non-interactive flow in this slice, so any `--json` on clean lands here.
    if ctx.flags.json {
        eprintln!("{JSON_REFUSAL_COPY}");
        return 1;
    }

    // `--no-prompt` is the scripted flow (implies `--yes`); that needs the
    // gates and `exec/`, so refuse it rather than ignoring it.
    if no_prompt {
        eprintln!("{NO_PROMPT_REFUSAL_COPY}");
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
        crate::rules::registry().len(),
        rx,
        show_spinner,
        ctx.flags.unicode,
    );
    let mut out = scan::run(&ctx, cancel, Some(tx));
    let _ = spinner.join();

    // Option (A): filter the scan results BEFORE building the selection.
    // `exclude` wins over `only` (CLI-UX section 2.2).
    out.results.retain(|r| keep_rule(r.id, only, exclude));

    // `system/proc.rs` does not exist yet, so no guard is held in this slice
    // (docs/SAFETY.md section 5). `--yes` is inert on the checklist by design.
    let guards: BTreeMap<&str, String> = BTreeMap::new();
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
                })
                .collect();
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
                    print!("{}", selection_preview(&items, &theme));
                    0
                }
            }
        }
    }
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
    let known: Vec<&str> = crate::rules::registry().iter().map(|r| r.id()).collect();
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
    fn copy_constants_stay_ascii_and_exact() {
        assert_eq!(EMPTY_SELECTION_COPY, "nothing selected, exiting");
        assert_eq!(ABORT_COPY, "aborted, nothing was deleted");
        assert!(NO_TTY_COPY.is_ascii());
        assert!(JSON_REFUSAL_COPY.is_ascii());
        assert!(NO_PROMPT_REFUSAL_COPY.is_ascii());
    }
}
