//! Renderer-agnostic checklist core tests (M2 task 2, slice 1).
//!
//! Pure: no tty, no stdin, no spawned process, no I/O. Everything runs
//! through the public `ui::checklist` API.
//! Spec: docs/CLI-UX.md §3; gates: docs/SAFETY.md §2.

use std::collections::BTreeMap;
use sweep::cli::Preset;
use sweep::rules::Tier;
use sweep::scan::{RuleResult, ScanInfo};
use sweep::ui::checklist::{Row, RowState, Selection};

/// Minimal `RuleResult` builder (all fields are pub).
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

fn guards(pairs: &[(&'static str, &str)]) -> BTreeMap<&'static str, String> {
    pairs.iter().map(|(k, v)| (*k, (*v).to_string())).collect()
}

fn state_of<'a>(sel: &'a Selection, id: &str) -> &'a RowState {
    &sel.rows()
        .iter()
        .find(|r| r.id == id)
        .expect("row id present")
        .state
}

fn ids(sel: &Selection) -> Vec<&'static str> {
    sel.rows().iter().map(|r| r.id).collect()
}

fn one_safe() -> Vec<RuleResult> {
    vec![rr("safe-a", Tier::Safe, true, 10)]
}

// ---- default selection per preset (SAFETY §2) ----

#[test]
fn safe_checked_in_safe_preset() {
    let sel = Selection::from_results(&one_safe(), Preset::Safe);
    assert_eq!(state_of(&sel, "safe-a"), &RowState::Ready { checked: true });
    assert_eq!(sel.selected_count(), 1);
}

#[test]
fn safe_checked_in_regular_preset() {
    let sel = Selection::from_results(&one_safe(), Preset::Regular);
    assert_eq!(state_of(&sel, "safe-a"), &RowState::Ready { checked: true });
}

#[test]
fn caution_unchecked_in_all_presets() {
    let results = vec![rr("caut", Tier::Caution, true, 5)];
    for preset in [Preset::Safe, Preset::Regular, Preset::Full] {
        let sel = Selection::from_results(&results, preset);
        assert_eq!(
            state_of(&sel, "caut"),
            &RowState::Ready { checked: false },
            "caution must be unchecked under {preset:?}"
        );
        assert_eq!(sel.selected_count(), 0);
    }
}

#[test]
fn destructive_locked_outside_full() {
    let results = vec![rr("dang", Tier::Destructive, true, 100)];
    for preset in [Preset::Safe, Preset::Regular] {
        let sel = Selection::from_results(&results, preset);
        assert_eq!(state_of(&sel, "dang"), &RowState::Locked);
        assert_eq!(sel.locked_count(), 1);
        assert!(!sel.rows()[0].is_toggleable());
    }
}

#[test]
fn destructive_unlocked_and_unchecked_under_full() {
    let results = vec![rr("dang", Tier::Destructive, true, 100)];
    let sel = Selection::from_results(&results, Preset::Full);
    assert_eq!(state_of(&sel, "dang"), &RowState::Ready { checked: false });
    assert_eq!(sel.locked_count(), 0);
    assert!(sel.rows()[0].is_toggleable());
    assert_eq!(sel.selected_count(), 0, "unlocked but NOT checked");
}

#[test]
fn i_know_what_im_doing_unlocks_destructive_outside_full() {
    let events = vec![rr("dang", Tier::Destructive, true, 100)];
    let sel = Selection::new(&events, Preset::Safe, false, true, &BTreeMap::new());
    assert_eq!(state_of(&sel, "dang"), &RowState::Ready { checked: false });
}

#[test]
fn i_know_what_im_doing_survives_preset_cycle() {
    // The flag is a CLI-level unlock (SAFETY §2 bypass column): it must keep
    // destructive unlocked in EVERY preset, including after a `p` cycle.
    let events = vec![rr("dang", Tier::Destructive, true, 100)];
    let mut sel = Selection::new(&events, Preset::Safe, false, true, &BTreeMap::new());
    for _ in 0..3 {
        assert_eq!(
            state_of(&sel, "dang"),
            &RowState::Ready { checked: false },
            "i-know must keep destructive unlocked under {:?}",
            sel.preset()
        );
        assert!(sel.rows()[0].is_toggleable());
        assert_eq!(sel.locked_count(), 0);
        sel.cycle_preset();
    }
    // Full cycle returns to Safe; still unlocked.
    assert_eq!(sel.preset(), Preset::Safe);
    assert_eq!(state_of(&sel, "dang"), &RowState::Ready { checked: false });
    assert_eq!(sel.selected_count(), 0, "unlocked but NOT auto-checked");
}

#[test]
fn yes_changes_nothing() {
    // `--yes` is inert in the checklist (SAFETY §2: bypass applies at the gate).
    // Compare full state + totals with yes=false vs yes=true across every preset.
    let events = vec![
        rr("safe-a", Tier::Safe, true, 10),
        rr("caut", Tier::Caution, true, 20),
        rr("dang", Tier::Destructive, true, 100),
    ];
    let g = guards(&[("caut", "editor running")]);
    for preset in [Preset::Safe, Preset::Regular, Preset::Full] {
        let without = Selection::new(&events, preset, false, false, &g);
        let with = Selection::new(&events, preset, true, false, &g);
        let states =
            |s: &Selection| -> Vec<RowState> { s.rows().iter().map(|r| r.state.clone()).collect() };
        assert_eq!(
            states(&without),
            states(&with),
            "yes must not change row states"
        );
        assert_eq!(without.totals(), with.totals());
        assert_eq!(without.selected(), with.selected());
    }
}

#[test]
fn yes_does_not_select_destructive() {
    let events = vec![rr("dang", Tier::Destructive, true, 100)];
    let sel = Selection::new(&events, Preset::Safe, true, false, &BTreeMap::new());
    assert_eq!(state_of(&sel, "dang"), &RowState::Locked);
    assert!(
        sel.selected().is_empty(),
        "--yes must never select destructive"
    );
    assert_eq!(sel.selected_bytes(), 0);
}

// ---- space (toggle) ----

#[test]
fn toggle_current_checks_then_unchecks() {
    let sel = {
        let mut s = Selection::from_results(&one_safe(), Preset::Safe);
        s.toggle_current();
        assert_eq!(state_of(&s, "safe-a"), &RowState::Ready { checked: false });
        s.toggle_current();
        s
    };
    assert_eq!(state_of(&sel, "safe-a"), &RowState::Ready { checked: true });
}

#[test]
fn toggle_current_noops_on_locked() {
    let mut sel =
        Selection::from_results(&[rr("dang", Tier::Destructive, true, 100)], Preset::Safe);
    let before = sel.totals();
    sel.toggle_current();
    assert_eq!(state_of(&sel, "dang"), &RowState::Locked);
    assert_eq!(sel.totals(), before);
    assert_eq!(sel.selected_count(), 0);
}

#[test]
fn toggle_current_noops_on_disabled_and_report_only() {
    let rows = vec![
        Row {
            id: "disabled-row",
            title: "Disabled",
            category: "test",
            tier: Tier::Safe,
            location: "~/.cache/x".to_string(),
            bytes: 0,
            needs_sudo: false,
            state: RowState::Disabled {
                reason: "disabled in config".to_string(),
            },
        },
        Row {
            id: "snapshot-row",
            title: "Snapshots",
            category: "test",
            tier: Tier::Caution,
            location: "~/.snapshots".to_string(),
            bytes: 0,
            needs_sudo: false,
            state: RowState::ReportOnly,
        },
    ];
    let mut sel = Selection::from_rows(rows, Preset::Regular);
    let before = sel.totals();
    sel.move_down();
    sel.toggle_current();
    assert_eq!(sel.totals(), before);
    assert!(!sel.rows()[0].is_toggleable());
    assert!(!sel.rows()[1].is_toggleable());
    assert!(sel.selected().is_empty());
}

// ---- guards ----

#[test]
fn guard_override_toggles_guarded_row() {
    let g = guards(&[("browser-cache", "firefox running (pid 123)")]);
    let mut sel = Selection::new(
        &[rr("browser-cache", Tier::Safe, true, 42)],
        Preset::Regular,
        false,
        false,
        &g,
    );
    assert!(matches!(
        state_of(&sel, "browser-cache"),
        RowState::Guarded { checked: false, .. }
    ));
    sel.toggle_current();
    assert!(matches!(
        state_of(&sel, "browser-cache"),
        RowState::Guarded { checked: true, .. }
    ));
    let selected = sel.selected();
    assert_eq!(selected.len(), 1);
    assert!(selected[0].guard_overridden);
    assert_eq!(selected[0].bytes, 42);
}

#[test]
fn guarded_rows_unchecked_by_default() {
    let g = guards(&[("caution-guarded", "editor running")]);
    let mut sel = Selection::new(
        &[
            rr("safe-a", Tier::Safe, true, 10),
            rr("caution-guarded", Tier::Caution, true, 20),
        ],
        Preset::Regular,
        false,
        false,
        &g,
    );
    assert!(matches!(
        state_of(&sel, "caution-guarded"),
        RowState::Guarded { checked: false, .. }
    ));
    sel.select_all_safe();
    assert!(
        matches!(
            state_of(&sel, "caution-guarded"),
            RowState::Guarded { checked: false, .. }
        ),
        "select_all_safe must not touch guarded rows"
    );
    assert_eq!(sel.guarded_count(), 1);
}

#[test]
fn destructive_guarded_stays_locked_outside_full() {
    // SAFETY §2: destructive rows are toggleable only under `full`. A guard must
    // never widen what is selectable — a guarded destructive row stays Locked.
    let g = guards(&[("dang", "docker daemon running")]);
    let events = vec![rr("dang", Tier::Destructive, true, 100)];
    for preset in [Preset::Safe, Preset::Regular] {
        let sel = Selection::new(&events, preset, false, false, &g);
        assert_eq!(
            state_of(&sel, "dang"),
            &RowState::Locked,
            "guarded destructive must stay locked under {preset:?}"
        );
        assert!(!sel.rows()[0].is_toggleable());
        assert_eq!(sel.guarded_count(), 0);
        assert_eq!(sel.locked_count(), 1);
    }
    // Under `full` (and with the sticky unlock) it becomes a normal override.
    let full = Selection::new(&events, Preset::Full, false, false, &g);
    assert!(matches!(
        state_of(&full, "dang"),
        RowState::Guarded { checked: false, .. }
    ));
    assert_eq!(full.guarded_count(), 1);
    assert_eq!(full.locked_count(), 0);

    let know = Selection::new(&events, Preset::Safe, false, true, &g);
    assert!(matches!(
        state_of(&know, "dang"),
        RowState::Guarded { checked: false, .. }
    ));
}

#[test]
fn not_applied_guarded_is_disabled() {
    // A rule the scan never applied (with a reason) is Disabled — the guard map
    // cannot promote it to a selectable Guarded row.
    let g = guards(&[("tmp", "should not win")]);
    let events = vec![rr("tmp", Tier::Safe, false, 0)];
    let sel = Selection::new(&events, Preset::Regular, false, false, &g);
    assert_eq!(
        state_of(&sel, "tmp"),
        &RowState::Disabled {
            reason: "disabled in config".to_string()
        }
    );
    assert!(!sel.rows()[0].is_toggleable());
    assert_eq!(sel.guarded_count(), 0);
}

// ---- a / n / i ----

#[test]
fn select_all_safe_checks_only_safe() {
    let g = guards(&[("safe-guarded", "in use")]);
    let mut sel = Selection::new(
        &[
            rr("safe-a", Tier::Safe, true, 10),
            rr("safe-guarded", Tier::Safe, true, 20),
            rr("caut", Tier::Caution, true, 30),
            rr("dang", Tier::Destructive, true, 40),
        ],
        Preset::Regular,
        false,
        false,
        &g,
    );
    sel.select_none();
    sel.select_all_safe();
    assert_eq!(state_of(&sel, "safe-a"), &RowState::Ready { checked: true });
    assert!(matches!(
        state_of(&sel, "safe-guarded"),
        RowState::Guarded { checked: false, .. }
    ));
    assert_eq!(state_of(&sel, "caut"), &RowState::Ready { checked: false });
    assert_eq!(state_of(&sel, "dang"), &RowState::Locked);
    assert_eq!(sel.selected_count(), 1);
}

#[test]
fn select_none_unchecks_ready_and_guarded() {
    let g = guards(&[("safe-guarded", "in use")]);
    let mut sel = Selection::new(
        &[
            rr("safe-a", Tier::Safe, true, 10),
            rr("safe-guarded", Tier::Safe, true, 20),
            rr("caut", Tier::Caution, true, 30),
        ],
        Preset::Regular,
        false,
        false,
        &g,
    );
    sel.toggle_current(); // check safe-a off? no: safe-a starts checked -> uncheck
    sel.select_all_safe();
    sel.select_none();
    for id in ["safe-a", "caut"] {
        assert_eq!(
            state_of(&sel, id),
            &RowState::Ready { checked: false },
            "{id} must be unchecked"
        );
    }
    assert!(matches!(
        state_of(&sel, "safe-guarded"),
        RowState::Guarded { checked: false, .. }
    ));
    assert_eq!(sel.selected_count(), 0);
}

#[test]
fn invert_flips_ready_rows_only() {
    let g = guards(&[("safe-guarded", "in use")]);
    let mut sel = Selection::new(
        &[
            rr("safe-a", Tier::Safe, true, 10),
            rr("safe-guarded", Tier::Safe, true, 20),
            rr("caut", Tier::Caution, true, 30),
            rr("dang", Tier::Destructive, true, 40),
        ],
        Preset::Regular,
        false,
        false,
        &g,
    );
    // safe-a checked -> unchecked; caut unchecked -> checked.
    sel.invert();
    assert_eq!(
        state_of(&sel, "safe-a"),
        &RowState::Ready { checked: false }
    );
    assert_eq!(state_of(&sel, "caut"), &RowState::Ready { checked: true });
    assert!(matches!(
        state_of(&sel, "safe-guarded"),
        RowState::Guarded { checked: false, .. }
    ));
    assert_eq!(state_of(&sel, "dang"), &RowState::Locked);
    assert_eq!(sel.selected_count(), 1);
}

// ---- p (preset cycle) ----

#[test]
fn cycle_preset_reapplies_defaults() {
    let results = vec![
        rr("safe-a", Tier::Safe, true, 10),
        rr("dang", Tier::Destructive, true, 40),
    ];
    let mut sel = Selection::from_results(&results, Preset::Safe);

    assert_eq!(sel.preset(), Preset::Safe);
    assert_eq!(state_of(&sel, "dang"), &RowState::Locked);

    sel.cycle_preset();
    assert_eq!(sel.preset(), Preset::Regular);
    assert_eq!(state_of(&sel, "dang"), &RowState::Locked);

    sel.cycle_preset();
    assert_eq!(sel.preset(), Preset::Full);
    assert_eq!(state_of(&sel, "dang"), &RowState::Ready { checked: false });
    assert_eq!(sel.selected_count(), 1, "only safe is selected");

    sel.cycle_preset();
    assert_eq!(sel.preset(), Preset::Safe);
    assert_eq!(state_of(&sel, "dang"), &RowState::Locked);
    assert_eq!(sel.selected_count(), 1);
}

#[test]
fn preset_cycle_replaces_manual_selection() {
    let results = vec![
        rr("safe-a", Tier::Safe, true, 10),
        rr("caut", Tier::Caution, true, 30),
        rr("dang", Tier::Destructive, true, 40),
    ];
    let mut sel = Selection::from_results(&results, Preset::Safe);
    // Manually select the caution row (safe-a is at 0, caut at 1, dang at 2).
    sel.move_down();
    sel.toggle_current();
    assert_eq!(state_of(&sel, "caut"), &RowState::Ready { checked: true });
    // Cycle to regular: manual caution selection is discarded, destructive
    // stays locked (regular).
    sel.cycle_preset();
    assert_eq!(
        state_of(&sel, "caut"),
        &RowState::Ready { checked: false },
        "preset cycle re-applies defaults, discarding manual selection"
    );
    assert_eq!(state_of(&sel, "dang"), &RowState::Locked);
    // Cycle to full: destructive unlocks (unchecked); manual selection already gone.
    sel.cycle_preset();
    assert_eq!(sel.preset(), Preset::Full);
    assert_eq!(state_of(&sel, "dang"), &RowState::Ready { checked: false });
    assert_eq!(sel.selected_count(), 1, "only the safe row is selected");
}

// ---- totals ----

#[test]
fn totals_math_counts_and_bytes() {
    let g = guards(&[("guarded", "in use")]);
    let results = vec![
        rr("safe-a", Tier::Safe, true, 10),
        rr("safe-b", Tier::Safe, true, 20),
        rr("safe-c", Tier::Safe, true, 30),
        rr("guarded", Tier::Caution, true, 999),
        rr("dang", Tier::Destructive, true, 999),
    ];
    let sel = Selection::new(&results, Preset::Regular, false, false, &g);
    let t = sel.totals();
    assert_eq!(t.selected_count, 3);
    assert_eq!(t.selected_bytes, 60);
    assert_eq!(t.guarded_count, 1);
    assert_eq!(t.locked_count, 1);
    assert_eq!(sel.selected_count(), 3);
    assert_eq!(sel.selected_bytes(), 60);
    assert_eq!(sel.guarded_count(), 1);
    assert_eq!(sel.locked_count(), 1);
}

#[test]
fn totals_exclude_disabled_and_report_only() {
    let rows = vec![
        Row {
            id: "disabled-row",
            title: "Disabled",
            category: "test",
            tier: Tier::Safe,
            location: "~/.cache/x".to_string(),
            bytes: 500,
            needs_sudo: false,
            state: RowState::Disabled {
                reason: "disabled in config".to_string(),
            },
        },
        Row {
            id: "snapshot-row",
            title: "Snapshots",
            category: "test",
            tier: Tier::Caution,
            location: "~/.snapshots".to_string(),
            bytes: 700,
            needs_sudo: false,
            state: RowState::ReportOnly,
        },
    ];
    let sel = Selection::from_rows(rows, Preset::Regular);
    let t = sel.totals();
    assert_eq!(t.selected_count, 0);
    assert_eq!(t.selected_bytes, 0);
    assert_eq!(t.guarded_count, 0);
    assert_eq!(t.locked_count, 0);
    assert!(sel.is_empty());
    assert!(sel.selected().is_empty());
}

// ---- selected() ----

#[test]
fn selected_is_empty_when_nothing_selected() {
    let results = vec![
        rr("caut", Tier::Caution, true, 10),
        rr("dang", Tier::Destructive, true, 20),
    ];
    let sel = Selection::from_results(&results, Preset::Safe);
    assert!(sel.selected().is_empty());
    assert!(sel.is_empty());
}

#[test]
fn selected_returns_ids_in_row_order() {
    let results = vec![
        rr("safe-a", Tier::Safe, true, 10),
        rr("safe-b", Tier::Safe, true, 20),
        rr("caut", Tier::Caution, true, 30),
    ];
    let mut sel = Selection::from_results(&results, Preset::Regular);
    sel.move_down();
    sel.move_down();
    sel.toggle_current(); // select the caution row too
    let selected = sel.selected();
    let got: Vec<&str> = selected.iter().map(|s| s.id).collect();
    assert_eq!(got, vec!["safe-a", "safe-b", "caut"]);
    assert_eq!(selected[0].bytes, 10);
    assert!(!selected[0].guard_overridden);
}

// ---- ordering ----

#[test]
fn row_order_is_safe_caution_destructive_then_report_only() {
    // Non-report-only rows, deliberately given out of order.
    let results = vec![
        rr("dang", Tier::Destructive, true, 40),
        rr("caut-b", Tier::Caution, true, 30),
        rr("safe-b", Tier::Safe, true, 20),
        rr("caut-a", Tier::Caution, true, 25),
        rr("safe-a", Tier::Safe, true, 10),
    ];
    let sel = Selection::from_results(&results, Preset::Regular);
    assert_eq!(
        ids(&sel),
        vec!["safe-b", "safe-a", "caut-b", "caut-a", "dang"],
        "tier-grouped, input-stable within tier"
    );
}

#[test]
fn row_order_places_report_only_last_regardless_of_tier() {
    // `Tier` is used by `from_rows` directly; construct a report-only row that
    // *carries* a Safe/Caution/Destructive tier (as the M3 btrfs snapshots row
    // will) and assert it still sorts after every non-report-only row.
    let mk = |id: &'static str, tier: Tier, state: RowState| Row {
        id,
        title: "Test rule",
        category: "test",
        tier,
        location: "~/.cache/test".to_string(),
        bytes: 0,
        needs_sudo: false,
        state,
    };
    let rows = vec![
        // Report-only rows carrying lower tiers first in input order.
        mk("snap-safe", Tier::Safe, RowState::ReportOnly),
        mk("caut", Tier::Caution, RowState::Ready { checked: false }),
        mk("snap-caut", Tier::Caution, RowState::ReportOnly),
        mk("safe", Tier::Safe, RowState::Ready { checked: true }),
        mk(
            "dang",
            Tier::Destructive,
            RowState::Ready { checked: false },
        ),
        mk("snap-dang", Tier::Destructive, RowState::ReportOnly),
    ];
    let sel = Selection::from_rows(rows, Preset::Regular);
    assert_eq!(
        ids(&sel),
        vec![
            "safe",
            "caut",
            "dang",
            "snap-safe",
            "snap-caut",
            "snap-dang"
        ],
        "CLI-UX §3.1: safe → caution → destructive → report-only"
    );
}

// ---- cursor ----

#[test]
fn move_down_clamps_and_move_up_clamps() {
    // Empty list: both no-op.
    let mut empty = Selection::from_results(&[], Preset::Regular);
    empty.move_down();
    empty.move_up();
    assert_eq!(empty.cursor(), 0);

    let results = vec![
        rr("safe-a", Tier::Safe, true, 10),
        rr("safe-b", Tier::Safe, true, 20),
    ];
    let mut sel = Selection::from_results(&results, Preset::Regular);
    assert_eq!(sel.cursor(), 0);
    sel.move_down();
    sel.move_down();
    assert_eq!(sel.cursor(), 1, "clamps at last row");
    sel.move_up();
    sel.move_up();
    assert_eq!(sel.cursor(), 0, "clamps at first row");
}

// ---- report-only / disabled ----

#[test]
fn report_only_never_selectable() {
    let rows = vec![
        Row {
            id: "snapshot-row",
            title: "Snapshots",
            category: "test",
            tier: Tier::Caution,
            location: "~/.snapshots".to_string(),
            bytes: 700,
            needs_sudo: false,
            state: RowState::ReportOnly,
        },
        Row {
            id: "safe-a",
            title: "Safe",
            category: "test",
            tier: Tier::Safe,
            location: "~/.cache".to_string(),
            bytes: 10,
            needs_sudo: false,
            state: RowState::Ready { checked: true },
        },
    ];
    let mut sel = Selection::from_rows(rows, Preset::Regular);
    // Rows are tier-sorted: safe-a first, the report-only row second.
    assert_eq!(sel.rows()[1].id, "snapshot-row");
    assert!(!sel.rows()[1].is_toggleable());
    sel.move_down();
    sel.toggle_current(); // cursor on the report-only row
    assert!(matches!(sel.rows()[1].state, RowState::ReportOnly));
    sel.select_all_safe();
    assert_eq!(sel.selected().len(), 1, "safe-a re-checked");
    sel.invert();
    assert!(
        sel.selected().is_empty(),
        "invert flips only the Ready safe row back off"
    );
    assert_eq!(sel.selected_bytes(), 0, "report-only bytes never counted");
}

#[test]
fn disabled_row_shows_reason_and_is_unselectable() {
    let results = vec![rr("tmp", Tier::Safe, false, 0)];
    let sel = Selection::from_results(&results, Preset::Regular);
    let row = &sel.rows()[0];
    assert_eq!(
        row.state,
        RowState::Disabled {
            reason: "disabled in config".to_string()
        }
    );
    assert!(!row.is_toggleable());
    assert_eq!(row.bytes, 0, "disabled rows contribute no bytes");
    assert!(sel.selected().is_empty());
}
