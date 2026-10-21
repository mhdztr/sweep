//! Pure checklist renderer tests (M2 task 2, slice 2). No tty, no I/O: every
//! assertion runs against `ui::render`'s public functions.
//! Spec: docs/CLI-UX.md §3.1/§3.3/§3.4 (layout, row states, live bar), §8.

use sweep::cli::Preset;
use sweep::rules::Tier;
use sweep::ui::Theme;
use sweep::ui::checklist::{Row, RowState, Selection, Totals};
use sweep::ui::render::{
    FrameOpts, frame_height, render_frame, render_help, render_row, render_totals_bar,
};
use sweep::ui::table::display_width;

fn row(
    id: &'static str,
    title: &'static str,
    tier: Tier,
    bytes: u64,
    state: RowState,
    needs_sudo: bool,
) -> Row {
    Row {
        id,
        title,
        category: "test",
        tier,
        location: "~/.cache".to_string(),
        bytes,
        needs_sudo,
        state,
    }
}

fn plain() -> Theme {
    Theme {
        color: false,
        unicode: true,
    }
}

fn no_unicode() -> Theme {
    Theme {
        color: false,
        unicode: false,
    }
}

#[test]
fn render_totals_bar_matches_cli_ux() {
    let t = Totals {
        selected_count: 4,
        selected_bytes: 14_283_147_674,
        guarded_count: 2,
        locked_count: 1,
    };
    // The exact CLI-UX §3.4 string, including the two spaces before the second
    // middot.
    assert_eq!(
        render_totals_bar(&t, &plain()),
        "  4 selected · 13.3 GiB  ·  2 guarded · 1 locked"
    );
}

#[test]
fn totals_bar_always_shows_all_four_groups() {
    let t = Totals {
        selected_count: 0,
        selected_bytes: 0,
        guarded_count: 0,
        locked_count: 0,
    };
    assert_eq!(
        render_totals_bar(&t, &plain()),
        "  0 selected · 0 B  ·  0 guarded · 0 locked"
    );
}

#[test]
fn frame_groups_rows_by_tier_then_report_only_without_resorting() {
    let rows = vec![
        row(
            "safe",
            "Safe",
            Tier::Safe,
            10,
            RowState::Ready { checked: true },
            false,
        ),
        row(
            "caut",
            "Caution",
            Tier::Caution,
            20,
            RowState::Ready { checked: true },
            false,
        ),
        row(
            "dang",
            "Destructive",
            Tier::Destructive,
            30,
            RowState::Locked,
            false,
        ),
        row(
            "snap",
            "Snapshots",
            Tier::Caution,
            0,
            RowState::ReportOnly,
            false,
        ),
    ];
    let sel = Selection::from_rows(rows, Preset::Regular);
    let frame = render_frame(&sel, &FrameOpts::new(80), &plain());
    let pos = |needle: &str| {
        frame
            .find(needle)
            .unwrap_or_else(|| panic!("missing {needle:?}:\n{frame}"))
    };
    assert!(pos("+ safe") < pos("! caution"));
    assert!(pos("! caution") < pos("# destructive"));
    assert!(pos("# destructive") < pos("~ report-only"));
}

#[test]
fn frame_marks_cursor_row() {
    let rows = vec![
        row(
            "a",
            "First",
            Tier::Safe,
            1,
            RowState::Ready { checked: false },
            false,
        ),
        row(
            "b",
            "Second",
            Tier::Safe,
            2,
            RowState::Ready { checked: false },
            false,
        ),
    ];
    let mut sel = Selection::from_rows(rows, Preset::Regular);
    sel.move_down(); // cursor on row 1
    let frame = render_frame(&sel, &FrameOpts::new(80), &plain());
    let lines: Vec<&str> = frame.lines().collect();
    let first = lines
        .iter()
        .find(|l| l.contains("First"))
        .expect("First row");
    let second = lines
        .iter()
        .find(|l| l.contains("Second"))
        .expect("Second row");
    assert!(!first.contains('>'), "no cursor on row 0: {first:?}");
    assert!(second.contains('>'), "cursor on row 1: {second:?}");
}

#[test]
fn frame_checkbox_reflects_selection() {
    let rows = vec![
        row(
            "on",
            "On",
            Tier::Safe,
            1,
            RowState::Ready { checked: true },
            false,
        ),
        row(
            "off",
            "Off",
            Tier::Safe,
            2,
            RowState::Ready { checked: false },
            false,
        ),
    ];
    let sel = Selection::from_rows(rows, Preset::Regular);
    let frame = render_frame(&sel, &FrameOpts::new(80), &plain());
    let on = frame.lines().find(|l| l.contains("On")).expect("On row");
    let off = frame.lines().find(|l| l.contains("Off")).expect("Off row");
    assert!(on.contains("[x]"), "checked row: {on:?}");
    assert!(off.contains("[ ]"), "unchecked row: {off:?}");
}

#[test]
fn frame_state_notes() {
    let rows = vec![
        row(
            "g",
            "Guarded",
            Tier::Caution,
            5,
            RowState::Guarded {
                reason: "firefox running".into(),
                checked: false,
            },
            false,
        ),
        row("l", "Locked", Tier::Destructive, 6, RowState::Locked, false),
        row(
            "d",
            "Disabled",
            Tier::Safe,
            0,
            RowState::Disabled {
                reason: "disabled in config".into(),
            },
            true,
        ),
    ];
    let sel = Selection::from_rows(rows, Preset::Regular);
    let frame = render_frame(&sel, &FrameOpts::new(100), &plain());
    let g = frame
        .lines()
        .find(|l| l.contains("Guarded"))
        .expect("Guarded row");
    let l = frame
        .lines()
        .find(|l| l.contains("Locked"))
        .expect("Locked row");
    let d = frame
        .lines()
        .find(|l| l.contains("Disabled"))
        .expect("Disabled row");
    assert!(
        g.contains("warn") && g.contains("guard firefox running"),
        "guarded notes: {g:?}"
    );
    assert!(l.contains("lock"), "locked note: {l:?}");
    assert!(d.contains("disabled in config"), "disabled note: {d:?}");
    assert!(d.contains("sudo"), "sudo note: {d:?}");
}

#[test]
fn no_unicode_frame_is_pure_ascii() {
    // Non-ASCII fixtures on purpose: a title and a guard *reason* are the two
    // dynamic fields that used to be appended verbatim (B1). With ASCII-only
    // fixtures this test could not fail on the invariant it claims to check.
    let rows = vec![
        row(
            "a",
            "na\u{ef}ve \u{4e2d}\u{6587}",
            Tier::Safe,
            3282,
            RowState::Guarded {
                reason: "na\u{ef}ve running".into(),
                checked: true,
            },
            false,
        ),
        row(
            "b",
            "Logs",
            Tier::Caution,
            999,
            RowState::Ready { checked: false },
            false,
        ),
    ];
    let sel = Selection::from_rows(rows, Preset::Regular);
    let ascii = render_frame(&sel, &FrameOpts::new(60), &no_unicode());
    assert!(
        ascii.is_ascii(),
        "no non-ASCII byte under --no-unicode:\n{ascii}"
    );
    assert!(ascii.contains("  -"), "ascii separator `-`:\n{ascii}");
    assert!(
        ascii.contains("nave running"),
        "the ASCII remainder of the sanitized title/reason survives:\n{ascii}"
    );

    let unicode = render_frame(&sel, &FrameOpts::new(60), &plain());
    assert!(
        unicode.contains('\u{2500}'),
        "unicode separator `─`:\n{unicode}"
    );
    assert!(
        unicode.contains("na\u{ef}ve"),
        "Unicode text is kept when the flag is on:\n{unicode}"
    );
}

#[test]
fn frame_never_emits_a_control_byte_from_dynamic_text() {
    // Escape-injection sink (B1's linked concern): a crafted title / guard
    // reason must never be emitted verbatim. Every `ESC` in the frame under
    // test is the renderer's own SGR (`\u{1b}[38;2;…`), so the escape *body*
    // must be gone under both unicode settings.
    let rows = vec![
        row(
            "a",
            "\u{4e2d}\u{6587} \u{1b}[2J \u{9b}31m",
            Tier::Caution,
            1,
            RowState::Guarded {
                reason: "\u{4e2d}\u{6587} \u{1b}[2J \u{9b}31m running".into(),
                checked: false,
            },
            false,
        ),
        row(
            "b",
            "plain",
            Tier::Safe,
            0,
            RowState::Disabled {
                reason: "\u{4e2d} disabled \u{1b}[2J".into(),
            },
            true,
        ),
    ];
    let sel = Selection::from_rows(rows, Preset::Regular);
    for (name, theme) in [
        ("unicode", plain()),
        ("no-unicode", no_unicode()),
        (
            "colored",
            Theme {
                color: true,
                unicode: true,
            },
        ),
    ] {
        let frame = render_frame(&sel, &FrameOpts::new(80), &theme);
        if theme.color {
            // A colored frame carries SGR by design; the injected bytes must
            // still be absent, and every remaining ESC must introduce a
            // complete CSI (a bare `[2m` body with no ESC is the same bug in
            // another layer).
            for part in frame.split('\u{1b}').skip(1) {
                assert!(
                    part.starts_with('['),
                    "{name}: every ESC introduces a CSI:\n{frame:?}"
                );
            }
            assert!(
                !frame.contains("\u{1b}[2J"),
                "{name}: no injected sequence survives:\n{frame:?}"
            );
        } else {
            assert!(
                !frame.contains('\u{1b}'),
                "{name}: no ESC byte in the frame:\n{frame:?}"
            );
        }
        assert!(
            !frame.contains('\u{9b}') && !frame.contains('\u{7f}'),
            "{name}: no C1/DEL byte in the frame:\n{frame:?}"
        );
        if !theme.unicode {
            assert!(frame.is_ascii(), "{name}: frame body is pure ASCII");
        }
    }
}

#[test]
fn no_color_frame_has_no_escape_sequences() {
    let rows = vec![row(
        "a",
        "Temp files",
        Tier::Safe,
        3282,
        RowState::Ready { checked: true },
        false,
    )];
    let sel = Selection::from_rows(rows, Preset::Regular);
    let plainf = render_frame(&sel, &FrameOpts::new(60), &plain());
    assert!(
        !plainf.contains('\u{1b}'),
        "no SGR in a plain frame:\n{plainf}"
    );

    let colored = render_frame(
        &sel,
        &FrameOpts::new(60),
        &Theme {
            color: true,
            unicode: true,
        },
    );
    assert!(colored.contains('\u{1b}'), "colored frame emits SGR");
}

#[test]
fn help_overlay_lists_all_keys() {
    let help = render_help(&plain());
    for token in [
        "space", "up/down", "a ", "n ", "i ", "p ", "enter", "esc", "q ", "?",
    ] {
        assert!(help.contains(token), "help must mention {token:?}:\n{help}");
    }
    assert!(help.contains("Ctrl-C"));
}

#[test]
fn every_frame_line_fits_the_width() {
    let rows = vec![
        row(
            "a",
            "A very long title that will not fit at all",
            Tier::Safe,
            12345,
            RowState::Ready { checked: true },
            false,
        ),
        row(
            "b",
            "Logs",
            Tier::Caution,
            0,
            RowState::Ready { checked: false },
            false,
        ),
    ];
    let sel = Selection::from_rows(rows, Preset::Regular);
    for width in [20usize, 40, 60, 100, 160] {
        for opts in [
            FrameOpts::new(width),
            FrameOpts {
                help: true,
                ..FrameOpts::new(width)
            },
            FrameOpts {
                notice: true,
                ..FrameOpts::new(width)
            },
        ] {
            let frame = render_frame(&sel, &opts, &plain());
            for line in frame.lines() {
                assert!(
                    display_width(line) <= width,
                    "line exceeds {width}: {line:?}"
                );
            }
        }
    }
}

#[test]
fn row_line_marks_cursor_and_fits_the_width() {
    let r = row(
        "tmp",
        "Temp files",
        Tier::Safe,
        3282,
        RowState::Ready { checked: true },
        false,
    );
    let off = render_row(&r, false, 60, &plain());
    let on = render_row(&r, true, 60, &plain());
    assert!(off.starts_with("    [x]"), "no marker off-cursor: {off:?}");
    assert!(
        on.starts_with("  > [x]"),
        "marker on the cursor row: {on:?}"
    );
    for w in [20usize, 30, 50, 200] {
        let line = render_row(&r, true, w, &plain());
        assert!(
            display_width(&line) <= w.max(20),
            "row must fit {w}: {line:?}"
        );
    }
}

#[test]
fn frame_height_is_the_physical_line_count() {
    let rows = vec![row(
        "a",
        "Temp files",
        Tier::Safe,
        3282,
        RowState::Ready { checked: true },
        false,
    )];
    let sel = Selection::from_rows(rows, Preset::Regular);
    let plainf = render_frame(&sel, &FrameOpts::new(60), &plain());
    assert_eq!(frame_height(&plainf), plainf.lines().count());
    let with_help = render_frame(
        &sel,
        &FrameOpts {
            help: true,
            ..FrameOpts::new(60)
        },
        &plain(),
    );
    assert!(
        frame_height(&with_help) > frame_height(&plainf),
        "help occupies more rows"
    );
}
