//! Event-loop tests through the injected-`Key` seam (M2 task 2, slice 2).
//!
//! These drive [`run_interactive_with`], which injects *decoded* `Key` values
//! straight into the loop, so they exercise the real frame writer, the real
//! `apply_key` mapping and the erase/redraw logic — but **not** the byte
//! decoder or `read_key_event`. The real decoder on a real pty is covered by
//! `tests/checklist_pty.rs`; this seam stays useful for deterministic
//! key-sequence assertions without a terminal.
//! Spec: docs/CLI-UX.md §3.2/§6; gates: docs/SAFETY.md §2 (abort deletes
//! nothing).

use std::collections::BTreeMap;

use sweep::cli::Preset;
use sweep::rules::Tier;
use sweep::scan::{RuleResult, ScanInfo};
use sweep::ui::Theme;
use sweep::ui::checklist::{
    Outcome, Selection, Source, run_interactive_with, run_interactive_with_source,
};
use sweep::ui::render::FrameOpts;
use sweep::ui::tty::Key;

fn rr(id: &'static str, tier: Tier, applied: bool, bytes: u64) -> RuleResult {
    RuleResult {
        id,
        title: "Test rule",
        category: "test",
        tier,
        applied,
        reason: if applied {
            None
        } else {
            Some("disabled in config".to_string())
        },
        needs_sudo: false,
        location: "~/.cache/test".to_string(),
        info: ScanInfo {
            bytes,
            ..Default::default()
        },
        net_of: Vec::new(),
        skipped: BTreeMap::new(),
    }
}

fn fixture() -> Vec<RuleResult> {
    vec![
        rr("safe-a", Tier::Safe, true, 10),
        rr("safe-b", Tier::Safe, true, 20),
        rr("dang", Tier::Destructive, true, 40),
    ]
}

fn plain() -> Theme {
    Theme {
        color: false,
        unicode: true,
    }
}

/// A key source that replays `keys`, then returns `Ok(None)` forever (a no-op
/// poll), recording every frame it was shown.
fn script<'a>(
    keys: &'a [Key],
) -> impl FnMut(&Selection, &FrameOpts) -> std::io::Result<Option<Key>> + 'a {
    let mut i = 0usize;
    move |_sel, _frame| {
        if i < keys.len() {
            let k = keys[i];
            i += 1;
            Ok(Some(k))
        } else {
            Ok(None)
        }
    }
}

#[test]
fn enter_accepts_the_current_selection() {
    let mut sel = Selection::from_results(&fixture(), Preset::Regular);
    sel.select_none();
    // Toggle the cursor row (safe-a) on, then proceed.
    let keys = [Key::Space, Key::Enter];
    let outcome = run_interactive_with(&mut sel, &plain(), script(&keys)).expect("loop runs");
    match outcome {
        Outcome::Accepted(items) => {
            assert_eq!(items.len(), 1, "one row selected");
            assert_eq!(items[0].id, "safe-a");
        }
        Outcome::Aborted => panic!("enter must accept"),
    }
}

#[test]
fn enter_with_nothing_selected_is_allowed_and_empty() {
    let mut sel = Selection::from_results(&fixture(), Preset::Regular);
    sel.select_none();
    let keys = [Key::Enter];
    let outcome = run_interactive_with(&mut sel, &plain(), script(&keys)).expect("loop runs");
    assert_eq!(
        outcome,
        Outcome::Accepted(Vec::new()),
        "0 selected is a legal accept"
    );
}

#[test]
fn abort_deletes_nothing() {
    let mut sel = Selection::from_results(&fixture(), Preset::Regular);
    let before: Vec<&'static str> = sel.selected().iter().map(|s| s.id).collect();
    let keys = [Key::Abort];
    let outcome = run_interactive_with(&mut sel, &plain(), script(&keys)).expect("loop runs");
    assert_eq!(outcome, Outcome::Aborted);
    let after: Vec<&'static str> = sel.selected().iter().map(|s| s.id).collect();
    assert_eq!(before, after, "abort mutates no selection");
}

#[test]
fn scripted_keys_reach_the_expected_final_selection() {
    let mut sel = Selection::from_results(&fixture(), Preset::Regular);
    sel.select_none();
    // a = select all safe, then enter.
    let keys = [Key::All, Key::Enter];
    let outcome = run_interactive_with(&mut sel, &plain(), script(&keys)).expect("loop runs");
    match outcome {
        Outcome::Accepted(items) => {
            let ids: Vec<&'static str> = items.iter().map(|s| s.id).collect();
            assert_eq!(ids, vec!["safe-a", "safe-b"], "`a` selected both safe rows");
        }
        Outcome::Aborted => panic!("enter must accept"),
    }
}

#[test]
fn empty_rows_still_accept_on_enter() {
    let mut sel = Selection::from_results(&[], Preset::Regular);
    let keys = [Key::Enter];
    let outcome = run_interactive_with(&mut sel, &plain(), script(&keys)).expect("loop runs");
    assert_eq!(outcome, Outcome::Accepted(Vec::new()));
}

#[test]
fn eof_source_ends_the_loop_with_abort_semantics() {
    // The production loop maps `tty::Input::Closed` to `Source::Eof`; a source
    // that yields it must stop the loop with abort semantics (nothing deleted)
    // rather than redraw forever. This is the loop-level half of the EOF
    // regression (the tty-level `Input::Closed` mapping is unit-tested in
    // `src/ui/tty.rs`).
    let mut sel = Selection::from_results(&fixture(), Preset::Regular);
    let calls = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let seen = std::rc::Rc::clone(&calls);
    let source = move |_sel: &Selection, _frame: &FrameOpts| {
        let n = seen.get() + 1;
        seen.set(n);
        // One idle window, then EOF: the loop must not spin past it.
        if n == 1 {
            Ok(Source::Idle)
        } else {
            Ok(Source::Eof)
        }
    };
    let outcome = run_interactive_with_source(&mut sel, &plain(), source).expect("loop runs");
    assert!(
        matches!(outcome, Outcome::Aborted),
        "EOF must end the loop with abort semantics"
    );
    assert_eq!(
        calls.get(),
        2,
        "the loop must stop at the first Eof, not iterate"
    );
}
