//! Clean wiring tests (M2, R13): the accepted-selection preview seam.
//!
//! The full `sweep clean` arg -> dispatch path is covered by
//! `tests/cli_smoke.rs` (no-TTY refusal); the interactive loop is covered by
//! `tests/clean_mutation_free.rs` (scripted abort + real pty). These tests pin
//! the pure preview helper and the empty-selection copy without a terminal.

use std::collections::BTreeMap;

use sweep::cli::Preset;
use sweep::cli::clean::{EMPTY_SELECTION_COPY, selection_preview};
use sweep::rules::Tier;
use sweep::scan::{RuleResult, ScanInfo};
use sweep::ui::Theme;
use sweep::ui::checklist::Selection;
use sweep::ui::format::format_bytes;

fn rr(id: &'static str, tier: Tier, bytes: u64, needs_sudo: bool) -> RuleResult {
    RuleResult {
        id,
        title: "Test rule",
        category: "test",
        tier,
        applied: true,
        reason: None,
        needs_sudo,
        location: "~/.cache/test".to_string(),
        info: ScanInfo {
            bytes,
            ..Default::default()
        },
        net_of: Vec::new(),
        skipped: BTreeMap::new(),
    }
}

fn plain() -> Theme {
    Theme {
        color: false,
        unicode: false,
    }
}

#[test]
fn accepted_selection_preview_contains_ids_and_bytes() {
    let results = vec![
        rr("safe-a", Tier::Safe, 10, false),
        rr("safe-b", Tier::Safe, 2_048, true),
        rr("dang", Tier::Destructive, 99, false),
    ];
    let mut sel = Selection::new(&results, Preset::Regular, false, false, &BTreeMap::new());
    sel.select_all_safe();
    let items = sel.selected();
    assert!(items.iter().any(|item| item.id == "safe-a"));

    let preview = selection_preview(&items, &plain());
    assert!(preview.contains("safe-a"), "{preview}");
    assert!(preview.contains("safe-b"), "{preview}");
    assert!(preview.contains(&format_bytes(10)), "{preview}");
    assert!(preview.contains(&format_bytes(2_048)), "{preview}");
    assert!(
        preview.contains("[sudo]"),
        "sudo marker for safe-b: {preview}"
    );
    // Destructive is locked under `regular`, so it is never in the selection.
    assert!(!preview.contains("dang"), "{preview}");
    // Gate honesty: the preview must be unmistakably non-executing.
    assert!(
        preview.contains("preview only"),
        "preview must state that nothing is deleted yet: {preview}"
    );
    // D13/D16: no ANSI/escape byte may leak from the pure preview helper.
    // `is_ascii()` alone is satisfied by ESC (0x1b), so this is the real guard.
    assert!(
        !preview.contains('\x1b'),
        "preview must carry no ANSI/escape byte: {preview:?}"
    );
    // D13/D16: new prose is pure ASCII with no theme-dependent bytes.
    assert!(
        preview.is_ascii(),
        "preview must be pure ASCII: {preview:?}"
    );
}

#[test]
fn empty_selection_maps_to_exit_0_copy() {
    // CLI-UX section 3.2: enter with 0 selected -> this exact copy, exit 0.
    assert_eq!(EMPTY_SELECTION_COPY, "nothing selected, exiting");
    assert!(EMPTY_SELECTION_COPY.is_ascii());

    let results = vec![rr("safe-a", Tier::Safe, 10, false)];
    let mut sel = Selection::new(&results, Preset::Regular, false, false, &BTreeMap::new());
    sel.select_none();
    let items = sel.selected();
    assert!(items.is_empty());
    // The preview helper is never called for an empty accept.
    assert_eq!(EMPTY_SELECTION_COPY, "nothing selected, exiting");
}
