//! Pure key->intent tests (M2 task 2, slice 2). No tty: `apply_key` is the
//! pure seam and `tty::decode` is the pure byte decoder.
//! Spec: docs/CLI-UX.md §3.2 (key table); gates: docs/SAFETY.md §2 (abort).

use std::collections::BTreeMap;

use sweep::cli::Preset;
use sweep::rules::Tier;
use sweep::scan::{RuleResult, ScanInfo};
use sweep::ui::checklist::{LoopStep, Selection, apply_key};
use sweep::ui::render::FrameOpts;
use sweep::ui::tty::{Key, decode};

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

fn mixed() -> Vec<RuleResult> {
    vec![
        rr("safe-a", Tier::Safe, true, 10),
        rr("safe-b", Tier::Safe, true, 20),
        rr("caut", Tier::Caution, true, 30),
        rr("dang", Tier::Destructive, true, 40),
    ]
}

fn opts() -> FrameOpts {
    FrameOpts::new(80)
}

fn checked(sel: &Selection, id: &str) -> bool {
    sel.rows()
        .iter()
        .find(|r| r.id == id)
        .expect("row present")
        .is_selected()
}

#[test]
fn space_toggles_only_the_cursor_row() {
    let mut sel = Selection::from_results(&mixed(), Preset::Regular);
    // Start with everything unselected.
    sel.select_none();
    sel.move_down(); // cursor to safe-b
    let mut f = opts();
    assert_eq!(apply_key(&mut sel, Key::Space, &mut f), LoopStep::Continue);
    assert!(checked(&sel, "safe-b"));
    assert!(!checked(&sel, "safe-a"));
}

#[test]
fn space_noops_on_locked_rows() {
    let mut sel = Selection::from_results(&mixed(), Preset::Regular);
    sel.select_none();
    // Move to the destructive (locked outside full) row by id.
    while sel.rows()[sel.cursor()].id != "dang" {
        sel.move_down();
    }
    let before = sel.selected_count();
    let mut f = opts();
    apply_key(&mut sel, Key::Space, &mut f);
    assert_eq!(sel.selected_count(), before, "locked row refuses toggling");
}

#[test]
fn a_selects_only_safe_rows() {
    let mut sel = Selection::from_results(&mixed(), Preset::Regular);
    sel.select_none();
    let mut f = opts();
    apply_key(&mut sel, Key::All, &mut f);
    assert!(checked(&sel, "safe-a") && checked(&sel, "safe-b"));
    assert!(!checked(&sel, "caut"), "caution is not selected by `a`");
    assert!(!checked(&sel, "dang"));
}

#[test]
fn n_deselects_everything() {
    let mut sel = Selection::from_results(&mixed(), Preset::Regular);
    let mut f = opts();
    apply_key(&mut sel, Key::None, &mut f);
    assert_eq!(sel.selected_count(), 0);
}

#[test]
fn i_inverts_the_selection() {
    let mut sel = Selection::from_results(&mixed(), Preset::Regular);
    sel.select_none();
    sel.move_down(); // safe-b checked
    let mut f = opts();
    apply_key(&mut sel, Key::Space, &mut f);
    assert!(checked(&sel, "safe-b") && !checked(&sel, "safe-a"));
    apply_key(&mut sel, Key::Invert, &mut f);
    assert!(
        checked(&sel, "safe-a") && !checked(&sel, "safe-b"),
        "invert flips ready rows"
    );
}

#[test]
fn p_cycles_the_preset_and_sets_the_notice() {
    let mut sel = Selection::from_results(&mixed(), Preset::Regular);
    let mut f = opts();
    assert_eq!(sel.preset(), Preset::Regular);
    apply_key(&mut sel, Key::Preset, &mut f);
    assert_eq!(sel.preset(), Preset::Full);
    assert!(f.notice, "the cycle sets the one-frame notice");
}

#[test]
fn up_and_down_move_the_cursor_and_clamp() {
    let mut sel = Selection::from_results(&mixed(), Preset::Regular);
    let mut f = opts();
    let top = sel.cursor();
    apply_key(&mut sel, Key::Up, &mut f);
    assert_eq!(sel.cursor(), top, "up at the top clamps");
    let end = sel.rows().len() - 1;
    while sel.cursor() < end {
        apply_key(&mut sel, Key::Down, &mut f);
    }
    apply_key(&mut sel, Key::Down, &mut f);
    assert_eq!(sel.cursor(), end, "down at the bottom clamps");
}

#[test]
fn help_toggles_the_help_flag() {
    let mut sel = Selection::from_results(&mixed(), Preset::Regular);
    let mut f = opts();
    apply_key(&mut sel, Key::Help, &mut f);
    assert!(f.help);
    apply_key(&mut sel, Key::Help, &mut f);
    assert!(!f.help);
}

#[test]
fn apply_key_returns_stop_true_on_enter_and_false_on_abort() {
    let mut sel = Selection::from_results(&mixed(), Preset::Regular);
    let mut f = opts();
    assert_eq!(
        apply_key(&mut sel, Key::Enter, &mut f),
        LoopStep::Stop(true)
    );
    assert_eq!(
        apply_key(&mut sel, Key::Abort, &mut f),
        LoopStep::Stop(false)
    );
}

#[test]
fn decode_maps_every_documented_key() {
    assert_eq!(decode(b" "), Key::Space);
    assert_eq!(decode(b"\x1b[A"), Key::Up);
    assert_eq!(decode(b"\x1b[B"), Key::Down);
    assert_eq!(decode(&[0x10]), Key::Up); // Ctrl-P
    assert_eq!(decode(&[0x0e]), Key::Down); // Ctrl-N
    assert_eq!(decode(b"a"), Key::All);
    assert_eq!(decode(b"n"), Key::None);
    assert_eq!(decode(b"i"), Key::Invert);
    assert_eq!(decode(b"p"), Key::Preset);
    assert_eq!(decode(b"?"), Key::Help);
    assert_eq!(decode(b"\r"), Key::Enter);
    assert_eq!(decode(b"\n"), Key::Enter);
    assert_eq!(decode(&[0x1b]), Key::Abort); // lone esc
    assert_eq!(decode(b"q"), Key::Abort);
    assert_eq!(decode(b"z"), Key::Unknown);
}
