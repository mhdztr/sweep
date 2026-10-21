//! Pure checklist frame renderer (M2 task 2, slice 2). Spec: docs/CLI-UX.md
//! §3.1/§3.3/§3.4 (layout, row states, live bar), §8 (visual tokens).
//!
//! No I/O, no tty, no `Selection` mutation: the whole frame is one `String`, so
//! every branch (tier groups, cursor marker, state notes, truncation, ASCII
//! mode) is unit-testable without a terminal. The impure shell lives in
//! `ui/checklist.rs::run_interactive`; raw mode/keys in `ui/tty.rs`.
//!
//! Three deliberate slice-2 choices (recorded in CONTEXT R10/R11, docs-synced):
//! - the checklist draws a **compact two-line header**, not `render_header`'s
//!   box (`cli/clean.rs` prints the box before the checklist in a later slice);
//! - there is **no alternate screen**: aborting keeps the scrollback;
//! - the caller (the impure loop) erases the previous frame by moving the
//!   cursor up `frame_height` rows; the renderer itself is pure and emits no
//!   erase/ANSI-cursor sequences.

use crate::cli::Preset;
use crate::rules::Tier;
use crate::ui::checklist::{Row, RowState, Selection, Totals};
use crate::ui::format::format_bytes;
use crate::ui::table::{display_width, truncate};
use crate::ui::{CAUTION, DESTRUCTIVE, DIM, INFO, SAFE, Theme};

/// Left-to-right column layout of one row line. Fixed constants (no width-based
/// column switching in slice 2): the design handles every width from ~50 cols to
/// well past any terminal. They also define [`MIN_WIDTH`].
/// Cursor/marker cell (`>` or a space).
const MARKER_COL: usize = 1;
/// The `[x]`/`[ ]`/report-only marker cell plus its trailing space.
const CHECK_COL: usize = 4;
/// Space between the marker cell and the checkbox cell.
const GAP1: usize = 1;
/// The tier glyph cell (`+ ! # ~`).
const TIER_COL: usize = 1;
/// Space between the tier glyph and the title text.
const GAP2: usize = 1;
/// Gap between the title/location text and the right-aligned size.
const SIZE_GAP: usize = 2;
/// Group/row indent before the cursor marker.
const INDENT: usize = 2;
/// Minimum frame width: the narrowest a row can be without wrapping
/// (`INDENT + MARKER_COL + GAP1 + CHECK_COL + GAP2 + TIER_COL + SIZE_GAP`, one
/// column of title, one of size). `render_frame` clamps to this so every emitted
/// line is exactly one physical row, which is what the loop's `frame_height`-row
/// erase assumes.
pub const MIN_WIDTH: usize =
    INDENT + MARKER_COL + GAP1 + CHECK_COL + GAP2 + TIER_COL + SIZE_GAP + 1 + 1;

/// A row and the tier group it belongs to (`~` marks the report-only group,
/// which CLI-UX §3.1 orders last regardless of the row's declared tier).
fn group_of(row: &Row) -> (char, &'static str) {
    match row.state {
        RowState::ReportOnly => ('~', "report-only"),
        RowState::Ready { .. }
        | RowState::Guarded { .. }
        | RowState::Locked
        | RowState::Disabled { .. } => (
            row.tier.glyph().chars().next().unwrap_or('+'),
            group_label(row.tier),
        ),
    }
}

/// Spelled-out group heading (CLI-UX §3.1 uses `+ safe` style headings, not the
/// abbreviated `Tier::label()` used inside the scan table).
fn group_label(tier: Tier) -> &'static str {
    match tier {
        Tier::Safe => "safe",
        Tier::Caution => "caution",
        Tier::Destructive => "destructive",
    }
}

/// Color for a tier (and the report-only pseudo-tier, per CLI-UX §8: `info`).
fn group_color(tier: Tier) -> (u8, u8, u8) {
    match tier {
        Tier::Safe => SAFE,
        Tier::Caution => CAUTION,
        Tier::Destructive => DESTRUCTIVE,
    }
}

/// The cursor/checkbox/state marker cell for one row state: the exact ASCII
/// glyph set (D13). Never colored: state is glyph + word, not color-only
/// (CLI-UX §11).
fn state_marker(state: &RowState) -> String {
    match state {
        RowState::Ready { checked } => {
            if *checked {
                "[x]".to_string()
            } else {
                "[ ]".to_string()
            }
        }
        RowState::Guarded { checked, .. } => {
            if *checked {
                "[x]".to_string()
            } else {
                "[!]".to_string()
            }
        }
        RowState::Locked => "[#]".to_string(),
        RowState::Disabled { .. } => "[-]".to_string(),
        RowState::ReportOnly => "[~]".to_string(),
    }
}

/// The trailing notes for one row (CLI-UX §3.3 + §8: `warn`/`guard`, `lock`,
/// `sudo`, `disabled in config`, `report`). Returned as `(color, text)` pairs
/// so the line builder can measure the *plain* text and paint afterwards.
fn row_notes(row: &Row) -> Vec<((u8, u8, u8), String)> {
    let mut notes: Vec<((u8, u8, u8), String)> = Vec::new();
    match &row.state {
        RowState::Guarded { reason, .. } => {
            // The amber `warn` marker plus the human reason ("firefox running").
            notes.push((CAUTION, "warn".to_string()));
            notes.push((CAUTION, format!("guard {reason}")));
        }
        RowState::Locked => notes.push((DESTRUCTIVE, "lock".to_string())),
        RowState::Disabled { reason } => notes.push((DIM, reason.clone())),
        RowState::ReportOnly => notes.push((INFO, "report".to_string())),
        RowState::Ready { .. } => {}
    }
    if row.needs_sudo {
        notes.push((DIM, "sudo".to_string()));
    }
    notes
}

/// Whether a dynamic text character must be stripped before it reaches the
/// frame, under the given unicode setting. Pure.
fn is_unsafe_char(c: char, unicode: bool) -> bool {
    // `is_control()` is the Unicode-general-category test: true for C0
    // (0x00-0x1f, includes ESC 0x1b), DEL (0x7f) **and** the C1 range
    // (0x80-0x9f, which includes the 8-bit CSI 0x9b and NEL 0x85). A lone C1
    // byte is a real escape-injection vector, and it is *not* caught by
    // `is_ascii_control` (0x9b is not ASCII), so the general test is the one
    // that closes the sink for both unicode settings.
    if c.is_control() {
        return true;
    }
    // D13: `--no-unicode` promises pure ASCII for all emitted text, dynamic
    // text included, not just the chrome.
    !unicode && !c.is_ascii()
}

/// Sanitize one piece of *dynamic* text (a row title, a guard/disabled reason,
/// any user- or system-supplied note) before it reaches the frame. Pure.
///
/// Two guarantees, both independent of the color flag:
/// - **No terminal escape injection.** Every control char is stripped, so a
///   crafted title can never smuggle an ANSI sequence (`\x1b[2J`) into the
///   terminal. This holds under `unicode == true` too: escaping is not a
///   Unicode-vs-ASCII question (B1's linked concern, D16).
/// - **Pure ASCII under `--no-unicode`.** Every non-ASCII char is stripped as
///   well, so the frame body carries no byte outside `0x20-0x7e` even when a
///   rule title or a `guards` reason is non-ASCII (D13's stated invariant, now
///   actually enforced for dynamic text and not just chrome).
///
/// Unsafe chars are **dropped**, never replaced: the sanitizer must not hand the
/// width/truncate math a longer string than it received (a `?`-per-char
/// substitution can overflow an exactly-full row). The sanitizer therefore
/// cannot mangle the ASCII titles the shipped rules use.
///
/// Content, not painted strings: sanitizing an already-`Theme`-painted line
/// would strip the SGR introducer and leave the CSI *body* as visible text.
fn sanitize(text: &str, unicode: bool) -> String {
    text.chars()
        .filter(|c| !is_unsafe_char(*c, unicode))
        .collect()
}

/// `-` when a row contributes no bytes (report-only/disabled), else the
/// human size. Always ASCII (D13/§8: the scan table's em dash does not apply).
fn row_size(row: &Row) -> String {
    if row.bytes == 0 {
        "-".to_string()
    } else {
        format_bytes(row.bytes)
    }
}

/// One row line: `{indent}{cursor} {checkbox} {tier-glyph} {title}{notes}{size}`,
/// hard-truncated to `width` (display-width aware). Pure.
///
/// The location is deliberately not in the row: it is the row's *title* that
/// carries the path in the checklist (`external`, `~/.cache/foo`), and slice 2
/// keeps one line per row so the totals bar plus hints stay on screen.
pub fn render_row(row: &Row, cursor: bool, width: usize, theme: &Theme) -> String {
    let lead = format!("{}{} ", " ".repeat(INDENT), if cursor { ">" } else { " " });
    let check = format!("{} ", state_marker(&row.state));
    let tier = format!("{} ", row.tier.glyph());

    let notes = row_notes(row);
    // Each note is prefixed by a two-space gap; measure that gap per note
    // directly (the previous version built a `notes_plain` string only to
    // split it again and discard the split).
    let notes_width: usize = notes.iter().map(|(_, t)| 2 + display_width(t)).sum();

    let size = row_size(row);
    let fixed = display_width(&lead)
        + display_width(&check)
        + display_width(&tier)
        + notes_width
        + SIZE_GAP
        + display_width(&size);
    // Clamp the *sanitized* title: the sanitizer drops bytes, so `title_room`
    // is an upper bound, not a promise the text fills it.
    let title_room = width.saturating_sub(fixed).max(4);
    let title = truncate(
        &sanitize(row.title, theme.unicode),
        title_room,
        theme.unicode,
    );

    let mut line = String::new();
    line.push_str(&lead);
    line.push_str(&check);
    line.push_str(&theme.paint(group_color(row.tier), &tier));
    line.push_str(&title);
    for (color, text) in &notes {
        line.push_str("  ");
        line.push_str(&theme.paint(*color, &sanitize(text, theme.unicode)));
    }

    // Right-align the size using *plain* widths, keeping at least `SIZE_GAP`
    // columns between the text and the size so they are never glued together.
    let used = display_width(&line);
    let pad = width.saturating_sub(used + display_width(&size));
    line.push_str(&" ".repeat(pad.max(SIZE_GAP)));
    line.push_str(&size);
    // `pad` is only the gap *after* the notes; the size itself may still
    // overflow on a very narrow terminal, so clamp once more. This clamp is
    // the B1 backstop: `truncate` never emits a non-ASCII byte or a control
    // char it was not handed, and by this point every dynamic string came
    // through `sanitize`, so a `--no-unicode` line cannot gain one here. It
    // is also ANSI-safe, so a colored line cannot be cut mid-sequence (B4).
    if display_width(&line) > width {
        truncate(&line, width, theme.unicode)
    } else {
        line
    }
}

/// The live totals bar (CLI-UX §3.4, exact): always all four groups, so the bar
/// does not jump around as the selection changes.
/// `  4 selected · 13.3 GiB  ·  2 guarded · 1 locked`, with the middots
/// degraded to `|` under `--no-unicode` (D13: pure ASCII).
pub fn render_totals_bar(t: &Totals, theme: &Theme) -> String {
    let s = sep(theme);
    theme.dim(&format!(
        "  {} selected {s} {}  {s}  {} guarded {s} {} locked",
        t.selected_count,
        format_bytes(t.selected_bytes),
        t.guarded_count,
        t.locked_count
    ))
}

/// The `?` help overlay (CLI-UX §3.2 key table, verbatim behaviours). Pure.
pub fn render_help(theme: &Theme) -> String {
    [
        "  keys",
        "    space       toggle row",
        "    up/down     move cursor (also Ctrl-P / Ctrl-N)",
        "    a           select all safe rows",
        "    n           deselect all",
        "    i           invert selection",
        "    p           cycle preset safe -> regular -> full -> safe",
        "    enter       proceed to gates (works with 0 selected)",
        "    ?           hide this help",
        "    esc / q     abort, nothing is deleted (exit 2)",
        "    Ctrl-C      abort, nothing is deleted (exit 2)",
    ]
    .iter()
    .map(|l| theme.dim(l))
    .collect::<Vec<_>>()
    .join("\n")
}

/// The one-line footer key hints (CLI-UX §3.2, condensed). Pure. The rendered
/// string starts with the same two-space indent every frame line uses; CLI-UX
/// §3.5 shows the copy without that indent.
///
/// The hint text is a module literal, but it is still run through
/// `sanitize` *before* `Theme::dim` paints it: content first, paint second.
/// Sanitizing a painted string would strip the SGR introducer and leave the
/// CSI body as visible text, and this is the one chrome line a future edit is
/// most likely to build from derived data.
pub fn render_footer(theme: &Theme) -> String {
    let s = sep(theme);
    let text = sanitize(
        &format!(
            "  space toggle {s} a all-safe {s} n none {s} i invert {s} p preset {s} enter proceed {s} ? help {s} esc abort"
        ),
        theme.unicode,
    );
    theme.dim(&text)
}

/// Extra panes/state of one frame. Plain data so it stays `Copy` and trivially
/// unit-testable; `notice` is the one-frame `p` warning ("replaces your
/// selection", CLI-UX §3.2) and is *renderer* state (the core has no UI state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameOpts {
    pub help: bool,
    pub width: usize,
    pub notice: bool,
}

impl FrameOpts {
    /// Default frame state at width `width`: no help, no notice.
    pub fn new(width: usize) -> FrameOpts {
        FrameOpts {
            help: false,
            width,
            notice: false,
        }
    }
}

/// The preset cycle order as shown in the header (CLI-UX §3.2).
fn preset_name(preset: Preset) -> &'static str {
    match preset {
        Preset::Safe => "safe",
        Preset::Regular => "regular",
        Preset::Full => "full",
    }
}

/// Pure: the whole checklist frame as one `String`, `'\n'`-terminated after
/// every line (so the result ends with a newline; the caller's `frame_height`
/// counts exactly the physical rows this occupies). Pure: no erase/ANSI-cursor
/// sequences are emitted; the impure loop erases the previous frame by moving
/// up `frame_height` rows.
///
/// `opts.width` is clamped to at least `MIN_WIDTH` so the layout constants
/// (indent + marker + checkbox + tier glyph) always fit and a row can never
/// wrap into more physical rows than `frame_height` counted.
pub fn render_frame(sel: &Selection, opts: &FrameOpts, theme: &Theme) -> String {
    let width = opts.width.max(MIN_WIDTH);
    let mut out = String::new();

    // 1. Compact one-line header (deliberate: the full `render_header` box is
    //    drawn by the caller before the checklist in a later slice). Every line
    //    is hard-truncated to `width` so a narrow terminal never wraps or
    //    overflows (CLI-UX §8).
    push_line(
        &mut out,
        &format!(
            "  {} {} preset: {}",
            theme.bold(&format!("sweep {}", env!("CARGO_PKG_VERSION"))),
            sep(theme),
            preset_name(sel.preset())
        ),
        width,
        theme,
    );
    push_line(
        &mut out,
        &format!(
            "  {} rows {} space toggle {} ? help {} esc abort",
            sel.rows().len(),
            sep(theme),
            sep(theme),
            sep(theme)
        ),
        width,
        theme,
    );

    if sel.rows().is_empty() {
        push_line(
            &mut out,
            &theme.dim("  nothing to clean; you're already tidy"),
            width,
            theme,
        );
    }

    // 2/3. Rows in the core's order (safe -> caution -> destructive ->
    // report-only), with one heading per contiguous group. The renderer never
    // re-sorts: `Selection` owns the ordering (CLI-UX §3.1).
    let mut current: Option<char> = None;
    for (index, row) in sel.rows().iter().enumerate() {
        let (glyph, label) = group_of(row);
        if current != Some(glyph) {
            current = Some(glyph);
            let color = if glyph == '~' {
                INFO
            } else {
                group_color(row.tier)
            };
            push_line(
                &mut out,
                &theme.paint(color, &format!("  {glyph} {label}")),
                width,
                theme,
            );
        }
        out.push_str(&render_row(row, index == sel.cursor(), width, theme));
        out.push('\n');
    }

    // 4. Separator before the bar, `|`/`-` only when unicode is on (§8).
    let bar_char = if theme.unicode { '\u{2500}' } else { '-' };
    push_line(
        &mut out,
        &theme.dim(&format!(
            "  {}",
            bar_char.to_string().repeat(width.saturating_sub(2))
        )),
        width,
        theme,
    );

    // 5. Live totals bar (exact copy, §3.4).
    push_line(
        &mut out,
        &render_totals_bar(&sel.totals(), theme),
        width,
        theme,
    );

    // 6. Footer hints (or the help overlay when `?` was toggled).
    if opts.help {
        for line in render_help(theme).lines() {
            push_line(&mut out, line, width, theme);
        }
    } else if opts.notice {
        push_line(
            &mut out,
            &theme.paint(
                CAUTION,
                &format!(
                    "  preset: {} (replaces your selection)",
                    preset_name(sel.preset())
                ),
            ),
            width,
            theme,
        );
    }
    push_line(&mut out, &render_footer(theme), width, theme);

    out
}

/// The frame's inline separator: a non-emoji middot when unicode is on, and an
/// ASCII `|` under `--no-unicode` (D13/§8: `--no-unicode` output is pure ASCII).
fn sep(theme: &Theme) -> &'static str {
    if theme.unicode { "\u{00b7}" } else { "|" }
}

/// Append one width-clamped, newline-terminated line. Pure. Truncates by
/// *display* width (so a wide line never wraps) and pads nothing, so trailing
/// blanks are not emitted.
///
/// Callers hand this a line whose *content* is already sanitized: `render_row`
/// runs [`sanitize`] over the title and every note before painting, and every
/// other caller builds the line from module literals. Sanitizing here instead
/// would be actively wrong: a painted line's own `ESC` (SGR) is a control char,
/// so a blanket `sanitize` pass would strip the introducer and leave the CSI
/// *body* (`[2m`) as visible text (this is not hypothetical, it is what the
/// first draft of the footer path did).
fn push_line(out: &mut String, line: &str, width: usize, theme: &Theme) {
    let clamped = if display_width(line) > width {
        truncate(line, width, theme.unicode)
    } else {
        line.to_string()
    };
    out.push_str(&clamped);
    out.push('\n');
}

/// The number of terminal rows a rendered frame occupies (its physical line
/// count). The impure loop moves the cursor up exactly this many rows before
/// redrawing, which is how the previous frame is erased without scroll queries.
/// Pure.
pub fn frame_height(body: &str) -> usize {
    body.lines().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{RuleResult, ScanInfo};
    use std::collections::BTreeMap;

    fn rr(
        id: &'static str,
        title: &'static str,
        tier: Tier,
        applied: bool,
        bytes: u64,
    ) -> RuleResult {
        RuleResult {
            id,
            title,
            category: "test",
            tier,
            applied,
            reason: if applied {
                None
            } else {
                Some("disabled in config".into())
            },
            needs_sudo: false,
            location: "~/.cache/test".into(),
            info: ScanInfo {
                bytes,
                ..Default::default()
            },
            net_of: Vec::new(),
            skipped: BTreeMap::new(),
        }
    }

    fn theme() -> Theme {
        Theme {
            color: false,
            unicode: true,
        }
    }

    #[test]
    fn totals_bar_matches_cli_ux_exactly() {
        // 13.3 GiB per `format_bytes`: 13.3 * 1024^3 + rounding.
        let t = Totals {
            selected_count: 4,
            selected_bytes: 14_283_147_674,
            guarded_count: 2,
            locked_count: 1,
        };
        assert_eq!(format_bytes(t.selected_bytes), "13.3 GiB");
        assert_eq!(
            render_totals_bar(&t, &theme()),
            "  4 selected · 13.3 GiB  ·  2 guarded · 1 locked"
        );
    }

    #[test]
    fn totals_bar_always_shows_all_four_groups() {
        // Zero groups still render (stable bar, no jumping; CLI-UX §3.4 only
        // documents the four-part form).
        let t = Totals::default();
        assert_eq!(
            render_totals_bar(&t, &theme()),
            "  0 selected · 0 B  ·  0 guarded · 0 locked"
        );
    }

    #[test]
    fn group_of_marks_report_only_last_regardless_of_tier() {
        let row = Row {
            id: "snap",
            title: "Snapshots",
            category: "test",
            tier: Tier::Caution,
            location: "~/.snapshots".into(),
            bytes: 700,
            needs_sudo: false,
            state: RowState::ReportOnly,
        };
        assert_eq!(group_of(&row).0, '~');
        assert_eq!(group_of(&row).1, "report-only");
    }

    #[test]
    fn state_markers_are_ascii_and_readable_without_color() {
        assert_eq!(state_marker(&RowState::Ready { checked: true }), "[x]");
        assert_eq!(state_marker(&RowState::Ready { checked: false }), "[ ]");
        assert_eq!(
            state_marker(&RowState::Guarded {
                reason: "firefox running".into(),
                checked: false
            }),
            "[!]"
        );
        assert_eq!(
            state_marker(&RowState::Guarded {
                reason: "firefox running".into(),
                checked: true
            }),
            "[x]"
        );
        assert_eq!(state_marker(&RowState::Locked), "[#]");
        assert_eq!(
            state_marker(&RowState::Disabled { reason: "x".into() }),
            "[-]"
        );
        assert_eq!(state_marker(&RowState::ReportOnly), "[~]");
        for state in [
            RowState::Ready { checked: true },
            RowState::Ready { checked: false },
            RowState::Guarded {
                reason: "r".into(),
                checked: false,
            },
            RowState::Locked,
            RowState::Disabled { reason: "r".into() },
            RowState::ReportOnly,
        ] {
            assert!(state_marker(&state).is_ascii(), "D13: markers are ASCII");
        }
    }

    #[test]
    fn row_notes_cover_every_state() {
        let mk = |state: RowState, needs_sudo: bool| Row {
            id: "r",
            title: "T",
            category: "test",
            tier: Tier::Safe,
            location: "~/.cache".into(),
            bytes: 1,
            needs_sudo,
            state,
        };
        let notes = |row: &Row| -> String {
            row_notes(row)
                .iter()
                .map(|(_, t)| t.clone())
                .collect::<Vec<_>>()
                .join("|")
        };
        assert_eq!(notes(&mk(RowState::Ready { checked: true }, false)), "");
        assert_eq!(
            notes(&mk(
                RowState::Guarded {
                    reason: "firefox running".into(),
                    checked: false
                },
                false
            )),
            "warn|guard firefox running"
        );
        assert_eq!(notes(&mk(RowState::Locked, false)), "lock");
        assert_eq!(
            notes(&mk(
                RowState::Disabled {
                    reason: "disabled in config".into()
                },
                false
            )),
            "disabled in config"
        );
        assert_eq!(notes(&mk(RowState::ReportOnly, false)), "report");
        assert_eq!(notes(&mk(RowState::Ready { checked: true }, true)), "sudo");
    }

    #[test]
    fn render_row_marks_cursor_and_fits_the_width() {
        let row = Row {
            id: "tmp",
            title: "Temp files",
            category: "test",
            tier: Tier::Safe,
            location: "/tmp".into(),
            bytes: 3282,
            needs_sudo: false,
            state: RowState::Ready { checked: true },
        };
        let plain = render_row(&row, false, 60, &theme());
        let cursor = render_row(&row, true, 60, &theme());
        assert!(
            plain.starts_with("    [x]"),
            "no marker off-cursor: {plain:?}"
        );
        assert!(
            cursor.starts_with("  > [x]"),
            "marker on the cursor row: {cursor:?}"
        );
        assert!(plain.contains("Temp files"));
        assert!(plain.contains("3.2 KiB"));
        for w in [20usize, 30, 50, 200] {
            let line = render_row(&row, true, w, &theme());
            assert!(
                display_width(&line) <= w.max(20),
                "row must fit {w}: {line:?} ({} cols)",
                display_width(&line)
            );
        }
    }

    #[test]
    fn render_frame_groups_rows_by_tier_then_report_only() {
        let results = vec![
            rr("safe", "Safe", Tier::Safe, true, 10),
            rr("caut", "Caution", Tier::Caution, true, 20),
            rr("dang", "Destructive", Tier::Destructive, true, 30),
        ];
        let mut sel = Selection::from_results(&results, crate::cli::Preset::Regular);
        sel.cycle_preset(); // regular -> full: destructive leaves Locked
        let rows = vec![Row {
            id: "snap",
            title: "Snapshots",
            category: "test",
            tier: Tier::Caution,
            location: "~/.snapshots".into(),
            bytes: 0,
            needs_sudo: false,
            state: RowState::ReportOnly,
        }];
        let mut with_report = Selection::from_rows(rows, crate::cli::Preset::Regular);
        // Append the report-only row behind the tiered ones by rebuilding.
        let mut rows = sel.rows().to_vec();
        rows.extend(with_report.rows().iter().cloned());
        with_report = Selection::from_rows(rows, crate::cli::Preset::Full);

        let frame = render_frame(&with_report, &FrameOpts::new(80), &theme());
        let heading = |label: &str| {
            frame
                .lines()
                .filter(|l| l.trim_end().ends_with(label))
                .count()
        };
        assert!(frame.contains("  + safe\n"), "safe heading: {frame}");
        assert!(frame.contains("  ! caution\n"), "caution heading: {frame}");
        assert!(
            frame.contains("  # destructive\n"),
            "destructive heading: {frame}"
        );
        assert!(
            frame.contains("  ~ report-only\n"),
            "report-only heading: {frame}"
        );
        assert_eq!(heading("safe"), 1);
        // Order of the headings is the core's order, not re-sorted here.
        let pos = |needle: &str| frame.find(needle).expect("heading present");
        assert!(pos("+ safe") < pos("! caution"));
        assert!(pos("! caution") < pos("# destructive"));
        assert!(pos("# destructive") < pos("~ report-only"));
    }

    /// A guarded row with a caller-supplied title and guard reason: the two
    /// dynamic-text fields B1 proved were never sanitized.
    fn guarded(title: &'static str, reason: &str) -> Row {
        Row {
            id: "guard",
            title,
            category: "test",
            tier: Tier::Caution,
            location: "~/.cache/test".into(),
            bytes: 1,
            needs_sudo: false,
            state: RowState::Guarded {
                reason: reason.to_string(),
                checked: false,
            },
        }
    }

    fn no_unicode_theme() -> Theme {
        Theme {
            color: false,
            unicode: false,
        }
    }

    #[test]
    fn sanitize_strips_controls_and_is_ascii_only_when_asked() {
        // Unicode on: controls (C0 incl. ESC, DEL, and the C1 range incl. the
        // 8-bit CSI 0x9b and NEL 0x85) go, ordinary Unicode stays.
        assert_eq!(sanitize("na\u{ef}ve", true), "na\u{ef}ve");
        assert_eq!(sanitize("a\u{1b}[2Jb", true), "a[2Jb");
        assert_eq!(sanitize("tab\there\rdone", true), "tabheredone");
        assert_eq!(sanitize("del\u{7f}ete", true), "delete");
        assert_eq!(sanitize("\u{9b}31m", true), "31m");
        assert_eq!(sanitize("nel\u{85}x", true), "nelx");
        assert_eq!(sanitize("nul\u{0}byte", true), "nulbyte");
        // Unicode off: non-ASCII goes too, so the result is pure ASCII.
        assert_eq!(sanitize("na\u{ef}ve running", false), "nave running");
        assert_eq!(sanitize("\u{2500}\u{00b7}\u{2026}", false), "");
        assert!(sanitize("na\u{ef}ve \u{1b}[2J \u{4e2d}\u{6587} \u{9b}", false).is_ascii());
        // And the ASCII text the shipped rules use is untouched (no mangling).
        assert_eq!(sanitize("firefox running", false), "firefox running");
        assert_eq!(sanitize("~/.cache/test", false), "~/.cache/test");
    }

    #[test]
    fn sanitize_drops_not_replaces_so_the_result_is_a_legal_substring() {
        // Regression guard for the deliberately-broken `?`-substitution
        // experiment (D16): the sanitizer hands `truncate` a shorter string,
        // never a longer one, so a full-width row still renders its text.
        let row = guarded("na\u{ef}ve \u{1b}[31mred", "na\u{ef}ve \u{1b}[31mred");
        let line = render_row(&row, true, 70, &no_unicode_theme());
        assert!(line.is_ascii(), "pure ASCII: {line:?}");
        assert!(!line.contains('\u{1b}'), "no ESC: {line:?}");
        assert!(
            line.contains("nave [31mred"),
            "the sanitized text is still rendered: {line:?}"
        );
        assert!(
            !line.contains("..."),
            "the row did not collapse to a bare ellipsis: {line:?}"
        );
    }

    #[test]
    fn no_unicode_row_drops_non_ascii_title_and_reason() {
        let row = guarded("na\u{ef}ve \u{4e2d}\u{6587}", "na\u{ef}ve running");
        let line = render_row(&row, true, 60, &no_unicode_theme());
        assert!(line.is_ascii(), "row is pure ASCII: {line:?}");
        assert!(
            line.contains("nave"),
            "the ASCII remainder of the title survives: {line:?}"
        );
        assert!(
            !line.contains('\u{e9}') && !line.contains('\u{4e2d}'),
            "no non-ASCII byte in the row: {line:?}"
        );
    }

    #[test]
    fn unicode_row_keeps_non_ascii_but_still_loses_the_escape_byte() {
        // `unicode == true` is the flag's whole point: the accented title and
        // reason stay. The embedded ESC does not (B1's linked concern).
        let row = guarded("na\u{ef}ve \u{1b}[2J", "na\u{ef}ve \u{1b}[2J running");
        let line = render_row(&row, true, 80, &theme());
        assert!(line.contains("na\u{ef}ve"), "Unicode kept: {line:?}");
        assert!(!line.contains('\u{1b}'), "no ESC anywhere: {line:?}");
        assert!(
            line.contains("na\u{ef}ve [2J running"),
            "reason survived minus the ESC: {line:?}"
        );
    }

    #[test]
    fn no_unicode_frame_is_pure_ascii() {
        // Replace the ASCII fixtures with the ones B1 proved the old test could
        // not fail on: a non-ASCII *title* and a non-ASCII *guard reason* are
        // exactly the two fields that used to reach the frame verbatim.
        let rows = vec![guarded("na\u{ef}ve \u{4e2d}\u{6587}", "na\u{ef}ve running")];
        let sel = Selection::from_rows(rows, crate::cli::Preset::Regular);
        let ascii = render_frame(&sel, &FrameOpts::new(60), &no_unicode_theme());
        // The frame body carries no non-ASCII byte at all under --no-unicode.
        assert!(
            ascii.is_ascii(),
            "no non-ASCII byte in --no-unicode:\n{ascii}"
        );
        assert!(ascii.contains("  -"), "ascii separator `-`:\n{ascii}");
        assert!(!ascii.contains('\u{2500}'), "no box drawing");
        assert!(!ascii.contains('\u{00b7}'), "no middot under --no-unicode");
        assert!(
            ascii.contains("nave running"),
            "the ASCII prefix of the sanitized dynamic text survives:\n{ascii}"
        );

        let unicode = render_frame(&sel, &FrameOpts::new(60), &theme());
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
        // B1's linked concern: a crafted title/reason must not reach the
        // terminal as an ANSI sequence, under *either* unicode setting. The
        // `\u{9b}` (8-bit CSI) and `\u{7f}` (DEL) cases pin the boundary that
        // the first draft of `sanitize` let through while it only tested
        // `is_control()`.
        let rows = vec![
            guarded(
                "\u{4e2d}\u{6587} \u{1b}[2J \u{9b}31m \u{7f}x",
                "\u{4e2d}\u{6587} \u{1b}[2J \u{9b}31m \u{7f}x running",
            ),
            Row {
                id: "disabled",
                title: "plain",
                category: "test",
                tier: Tier::Safe,
                location: "~/.cache/test".into(),
                bytes: 0,
                needs_sudo: false,
                state: RowState::Disabled {
                    reason: "\u{4e2d} disabled \u{1b}[2J".into(),
                },
            },
        ];
        let sel = Selection::from_rows(rows, crate::cli::Preset::Regular);
        for (name, theme) in [
            ("unicode", theme()),
            ("no-unicode", no_unicode_theme()),
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
                // A colored frame carries SGR by design, but every ESC must
                // *open* something: a CSI body with no introducing ESC (e.g. a
                // bare `[2m`) is the same injection bug in a different layer.
                for part in frame.split('\u{1b}').skip(1) {
                    assert!(
                        part.starts_with('['),
                        "{name}: every ESC introduces a CSI: {frame:?}"
                    );
                    let body = &part[1..];
                    let complete = body
                        .char_indices()
                        .take_while(|(_, c)| !('@'..='~').contains(c))
                        .count();
                    assert!(
                        complete < body.len(),
                        "{name}: every CSI has a final byte: {frame:?}"
                    );
                }
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
            assert!(
                !frame.contains("\u{1b}[2J"),
                "{name}: the injected escape sequence is gone:\n{frame:?}"
            );
            if !theme.color {
                assert!(
                    !frame.contains('\u{1b}'),
                    "{name}: a non-colored frame has no ESC at all:\n{frame:?}"
                );
            }
            if !theme.unicode {
                assert!(frame.is_ascii(), "{name}: frame body is ASCII");
            }
        }
        // The colored frame still emits its own SGR, and nothing else: every
        // escape is one of the renderer's own codes (bold/dim/reset and the
        // truecolor openers/closers), never something the injected text asked
        // for.
        let colored = render_frame(
            &sel,
            &FrameOpts::new(80),
            &Theme {
                color: true,
                unicode: true,
            },
        );
        assert!(
            colored.contains("\u{1b}[38;2;"),
            "the frame's own truecolor SGR is present"
        );
        let renderer_codes = [
            "\u{1b}[1m",
            "\u{1b}[2m",
            "\u{1b}[0m",
            "\u{1b}[39m",
            "\u{1b}[38;2;",
        ];
        let own = renderer_codes
            .iter()
            .map(|c| colored.matches(c).count())
            .sum::<usize>();
        assert_eq!(
            colored.matches('\u{1b}').count(),
            own,
            "every ESC in the frame is the renderer's own SGR:\n{colored:?}"
        );
        assert!(
            !colored.contains("\u{1b}[2J") && !colored.contains('\u{9b}'),
            "no injected sequence survives in a colored frame:\n{colored:?}"
        );
    }

    #[test]
    fn no_color_frame_has_no_escape_sequences_but_the_erase_prefix() {
        let results = vec![rr("safe", "Temp files", Tier::Safe, true, 3282)];
        let sel = Selection::from_results(&results, crate::cli::Preset::Regular);
        let plain = render_frame(&sel, &FrameOpts::new(60), &theme());
        // A plain frame has no escape byte anywhere (no SGR, no cursor moves).
        assert!(
            !plain.contains('\u{1b}'),
            "no SGR in a plain frame:\n{plain}"
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
    fn help_overlay_lists_every_key() {
        let help = render_help(&theme());
        for token in [
            "space", "up/down", "a ", "n ", "i ", "p ", "enter", "esc", "q ", "?",
        ] {
            assert!(help.contains(token), "help must mention {token:?}:\n{help}");
        }
        assert!(help.contains("Ctrl-C"));
    }

    #[test]
    fn every_frame_line_fits_the_width() {
        let results = vec![
            rr(
                "safe",
                "A very long title that will not fit at all",
                Tier::Safe,
                true,
                12345,
            ),
            rr("caut", "Logs", Tier::Caution, false, 0),
        ];
        let sel = Selection::from_results(&results, crate::cli::Preset::Regular);
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
                let frame = render_frame(&sel, &opts, &theme());
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
    fn frame_erases_exactly_the_previous_frame() {
        let results = vec![rr("safe", "Temp files", Tier::Safe, true, 3282)];
        let sel = Selection::from_results(&results, crate::cli::Preset::Regular);

        let plain = render_frame(&sel, &FrameOpts::new(60), &theme());
        // The loop erases exactly `frame_height` rows before redrawing.
        assert_eq!(
            frame_height(&plain),
            plain.lines().count(),
            "height is the physical line count"
        );

        let with_help = render_frame(
            &sel,
            &FrameOpts {
                help: true,
                ..FrameOpts::new(60)
            },
            &theme(),
        );
        assert!(
            frame_height(&with_help) > frame_height(&plain),
            "the help overlay occupies more rows"
        );
    }
}
