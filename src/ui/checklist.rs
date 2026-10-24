//! Renderer-agnostic interactive checklist state (M2 task 2, slice 1) plus the
//! impure event loop and the pure key→intent seam (slice 2).
//! Spec: docs/CLI-UX.md §3; gates: docs/SAFETY.md §2.
//! The core (`Selection`/`Row`/`RowState`/`Totals`/`Selected`) is pure: no I/O,
//! no tty, no key handling. Rendering is pure and lives in `ui/render.rs`; raw
//! mode/keys live in `ui/tty.rs` (D15).

use crate::cli::Preset;
use crate::rules::Tier;
use crate::scan::RuleResult;
use crate::ui::Theme;
use crate::ui::render::{self, FrameOpts};
use crate::ui::tty::{self, Key};
use std::collections::BTreeMap;
use std::io::Write;

/// Rule ids whose rows are report-only (`~`, D9): visible, never selectable.
///
/// TODO(M3): the btrfs/snapper snapshot rule populates this (see plan §9.3;
/// there is no report-only `Tier` yet, so the marker is an id list for now).
const REPORT_ONLY_IDS: &[&str] = &[];

/// Per-row state (CLI-UX §3.3 + SAFETY §2). Renderer maps these to glyphs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowState {
    /// Toggleable; `checked` is user-editable. Used for safe/caution/destructive-unlocked.
    Ready { checked: bool },
    /// Guard held (browser running, etc.): shown, pre-unchecked, override allowed.
    /// Slice 1: always `checked: false` at construction; `toggle` may set it true
    /// (the P6 confirm is a *gate* concern, not the core's).
    Guarded { reason: String, checked: bool },
    /// Destructive row outside `full`: visible, toggle refused.
    Locked,
    /// `applied == false` for a non-guard reason ("disabled in config", root missing).
    Disabled { reason: String },
    /// `~` report-only row (btrfs snapshots, D9): no checkbox, never selectable.
    ReportOnly,
}

/// One checklist row. Owns a *snapshot* of the display fields the renderer needs.
#[derive(Debug, Clone)]
pub struct Row {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub tier: Tier,
    /// Display path (from RuleResult.location).
    pub location: String,
    /// Bytes the row contributes when selected (0 for report-only/disabled).
    pub bytes: u64,
    pub needs_sudo: bool,
    pub state: RowState,
}

impl Row {
    /// Whether this row currently counts toward the selection.
    pub fn is_selected(&self) -> bool {
        match &self.state {
            RowState::Ready { checked } => *checked,
            RowState::Guarded { checked, .. } => *checked,
            RowState::Locked | RowState::Disabled { .. } | RowState::ReportOnly => false,
        }
    }

    /// Whether `space` is allowed to change it (renderer hint; gates live elsewhere).
    pub fn is_toggleable(&self) -> bool {
        matches!(
            self.state,
            RowState::Ready { .. } | RowState::Guarded { .. }
        )
    }
}

/// Renderer-agnostic selection state over an ordered, tier-grouped row list.
#[derive(Debug, Clone)]
pub struct Selection {
    rows: Vec<Row>,
    cursor: usize,
    preset: Preset,
    /// Sticky CLI-level unlock (SAFETY §2 bypass column): destructive rows stay
    /// unlocked-but-unchecked in **every** preset, including after `p` cycles.
    i_know_what_im_doing: bool,
}

/// Totals snapshot for the live bar (CLI-UX §3.4), computed by one pass, O(n).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    pub selected_count: usize,
    pub selected_bytes: u64,
    pub guarded_count: usize,
    pub locked_count: usize,
}

/// One selected item handed to `build_plan` (ARCHITECTURE §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub id: &'static str,
    pub bytes: u64,
    pub needs_sudo: bool,
    /// true when the user overrode a guard (drives P6 at the gate stage).
    pub guard_overridden: bool,
}

/// Tier ordering for row display: safe → caution → destructive (CLI-UX §3.1).
/// A private rank, deliberately NOT an `Ord` impl on `Tier` (Decision C):
/// `Tier` is also the JSON serialization tier and reordering it would leak
/// into other surfaces.
fn tier_rank(t: Tier) -> u8 {
    match t {
        Tier::Safe => 0,
        Tier::Caution => 1,
        Tier::Destructive => 2,
    }
}

/// Row-display sort key: `(is_report_only, tier_rank)` (CLI-UX §3.1).
///
/// Report-only rows sort **after** every non-report-only row regardless of the
/// `Tier` they carry: a report-only row has no `Tier` of its own yet (D9), so
/// it inherits whatever tier its rule declares and would otherwise be slotted
/// into that tier's group mid-list. `sort_by_key` is stable, so input order is
/// preserved within a group.
fn row_sort_key(row: &Row) -> (u8, u8) {
    let report_only = u8::from(matches!(row.state, RowState::ReportOnly));
    (report_only, tier_rank(row.tier))
}

impl Selection {
    /// Build from scan results. Rows are ordered safe → caution → destructive →
    /// report-only, stable within tier by input order (CLI-UX §3.1).
    ///
    /// `preset`: drives destructive lock/unlock + initial selection.
    /// `yes`: recorded but has NO effect on selection (SAFETY §2: `--yes` never
    ///        selects destructive). Kept in the signature so slice 2 can pass
    ///        flags uniformly and so a test can prove it is inert.
    /// `i_know_what_im_doing`: unlocks destructive rows (unchecked) even outside
    ///        `full` (SAFETY §2 bypass column). Sticky: it is stored on the
    ///        `Selection` and survives `cycle_preset` (a preset cycle re-applies
    ///        defaults, but the flag is a CLI-level unlock, not a per-preset
    ///        default).
    /// `guards`: guard-held reasons keyed by rule id; slice 2 supplies this from
    ///        `system/proc.rs`. Slice 1 passes an empty map in tests (or a
    ///        synthetic one). A map, not `Detect` (Decision A).
    pub fn new(
        results: &[RuleResult],
        preset: Preset,
        yes: bool,
        i_know_what_im_doing: bool,
        guards: &BTreeMap<&str, String>,
    ) -> Selection {
        // `yes` is intentionally inert here (SAFETY §2 bypass applies at the
        // *gate*, not the checklist); name it so the intent is explicit.
        let _ = yes;

        let mut rows: Vec<Row> = results
            .iter()
            .map(|r| {
                let state = row_state_for(r, preset, i_know_what_im_doing, guards);
                Row {
                    id: r.id,
                    title: r.title,
                    category: r.category,
                    tier: r.tier,
                    location: r.location.clone(),
                    bytes: row_bytes(r),
                    needs_sudo: r.needs_sudo,
                    state,
                }
            })
            .collect();

        // Stable sort by (report-only last, tier rank), input order preserved
        // within a group. `sort_by_key` is stable, so no index key is needed.
        rows.sort_by_key(row_sort_key);

        Selection {
            rows,
            cursor: 0,
            preset,
            i_know_what_im_doing,
        }
    }

    /// Convenience for tests / slice 2 when no guards are held.
    pub fn from_results(results: &[RuleResult], preset: Preset) -> Selection {
        Selection::new(results, preset, false, false, &BTreeMap::new())
    }

    /// Test/advanced constructor: build directly from rows (drives report-only /
    /// disabled branches that no current rule produces). Keeps `new` thin.
    /// `i_know_what_im_doing` defaults to `false`; use [`Selection::new`] when
    /// the flag matters.
    pub fn from_rows(mut rows: Vec<Row>, preset: Preset) -> Selection {
        rows.sort_by_key(row_sort_key);
        Selection {
            rows,
            cursor: 0,
            preset,
            i_know_what_im_doing: false,
        }
    }

    // ---- cursor (semantics of Up/Down, Ctrl-P/N) ----

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Move the cursor up one row; clamps at the first row (no wrap:
    /// predictable). No-op on an empty list.
    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move the cursor down one row; clamps at the last row (no wrap).
    /// No-op on an empty list.
    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.rows.len() {
            self.cursor += 1;
        }
    }

    // ---- space ----

    /// Toggle the cursor row. No-op for Locked/Disabled/ReportOnly.
    /// For Guarded: toggles `checked` (P6 is enforced by the gate, not core).
    pub fn toggle_current(&mut self) {
        let Some(row) = self.rows.get_mut(self.cursor) else {
            return;
        };
        match &mut row.state {
            RowState::Ready { checked } => *checked = !*checked,
            RowState::Guarded { checked, .. } => *checked = !*checked,
            RowState::Locked | RowState::Disabled { .. } | RowState::ReportOnly => {}
        }
    }

    // ---- a / n / i ----

    /// `a`: check every Ready{Safe}. Leaves Caution, Locked, Guarded, Disabled,
    /// ReportOnly untouched.
    pub fn select_all_safe(&mut self) {
        for row in &mut self.rows {
            if row.tier == Tier::Safe
                && let RowState::Ready { checked } = &mut row.state
            {
                *checked = true;
            }
        }
    }

    /// `n`: uncheck every row (Ready|Guarded -> checked:false). Locked/Disabled/
    /// ReportOnly unchanged.
    pub fn select_none(&mut self) {
        for row in &mut self.rows {
            match &mut row.state {
                RowState::Ready { checked } | RowState::Guarded { checked, .. } => *checked = false,
                RowState::Locked | RowState::Disabled { .. } | RowState::ReportOnly => {}
            }
        }
    }

    /// `i`: invert every Ready row's checked bit (Safe+Caution+unlocked
    /// Destructive). Guarded/Locked/Disabled/ReportOnly untouched.
    pub fn invert(&mut self) {
        for row in &mut self.rows {
            if let RowState::Ready { checked } = &mut row.state {
                *checked = !*checked;
            }
        }
    }

    // ---- p ----

    /// Cycle safe -> regular -> full -> safe and RE-APPLY that preset's
    /// defaults (CLI-UX §3.2: "re-applies defaults; warns 'replaces your
    /// selection'"). The warning is the renderer's job; this method just
    /// replaces state.
    pub fn cycle_preset(&mut self) {
        // Derived from `tty::PRESET_CYCLE` (the documented order) so the key
        // hint, the renderer's label and this cycle cannot drift apart.
        let cycle = crate::ui::tty::PRESET_CYCLE;
        let idx = cycle.iter().position(|&p| p == self.preset).unwrap_or(0);
        let next = cycle[(idx + 1) % cycle.len()];
        self.set_preset(next);
    }

    /// Re-apply `preset`'s defaults, discarding manual selection.
    /// `i_know_what_im_doing` is sticky (see [`Selection::new`]): a preset cycle
    /// re-applies *preset* defaults, it does not revoke the CLI-level unlock.
    fn set_preset(&mut self, preset: Preset) {
        self.preset = preset;
        let i_know = self.i_know_what_im_doing;
        for row in &mut self.rows {
            row.state = apply_preset_to_state(&row.state, row.tier, preset, i_know);
        }
    }

    pub fn preset(&self) -> Preset {
        self.preset
    }

    // ---- live totals (one O(n) pass; CLI-UX §3.4 "<16 ms pre-aggregated") ----

    pub fn totals(&self) -> Totals {
        let mut t = Totals::default();
        for row in &self.rows {
            match &row.state {
                RowState::Ready { checked } => {
                    if *checked {
                        t.selected_count += 1;
                        t.selected_bytes += row.bytes;
                    }
                }
                RowState::Guarded { checked, .. } => {
                    t.guarded_count += 1;
                    if *checked {
                        t.selected_count += 1;
                        t.selected_bytes += row.bytes;
                    }
                }
                RowState::Locked => t.locked_count += 1,
                RowState::Disabled { .. } | RowState::ReportOnly => {}
            }
        }
        t
    }

    /// Number of rows currently selected.
    pub fn selected_count(&self) -> usize {
        self.totals().selected_count
    }

    /// Bytes the current selection would reclaim.
    pub fn selected_bytes(&self) -> u64 {
        self.totals().selected_bytes
    }

    /// Guarded rows, regardless of checked.
    pub fn guarded_count(&self) -> usize {
        self.totals().guarded_count
    }

    /// Locked rows.
    pub fn locked_count(&self) -> usize {
        self.totals().locked_count
    }

    // ---- enter ----

    /// Selected items in cursor/tier order. Empty selection -> empty Vec
    /// (renderer prints "nothing selected" and exits 0).
    pub fn selected(&self) -> Vec<Selected> {
        self.rows
            .iter()
            .filter(|r| r.is_selected())
            .map(|r| Selected {
                id: r.id,
                bytes: r.bytes,
                needs_sudo: r.needs_sudo,
                guard_overridden: matches!(r.state, RowState::Guarded { .. }),
            })
            .collect()
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// No selected rows.
    pub fn is_empty(&self) -> bool {
        !self.rows.iter().any(|r| r.is_selected())
    }
}

// ---------------------------------------------------------------------------
// Slice 2: the event loop (impure shell) + the pure key→intent seam.
// Spec: docs/CLI-UX.md §3.2 (keys) / §6 (exit codes); SAFETY §2 (abort gate).
// ---------------------------------------------------------------------------

/// What the event loop decided (see `run_interactive`). A later slice maps
/// these to exit codes: `Aborted` → 2 + `aborted, nothing was deleted`
/// (CLI-UX §6, SAFETY §2), `Accepted` → on to the gates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `enter`: proceed with these items (possibly empty, CLI-UX §3.2).
    Accepted(Vec<Selected>),
    /// `esc` / `q` / Ctrl-C: abort, exit 2, nothing done.
    Aborted,
}

/// The event loop's directive after one decoded key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopStep {
    Continue,
    /// Stop the loop; `true` = accepted (`enter`), `false` = aborted.
    Stop(bool),
}

/// PURE: apply one decoded key to `sel`/`frame` and say whether the loop ends.
///
/// This is the seam that makes key→intent mapping testable without a tty
/// (CLI-UX §3.2, normative):
///
/// | key | intent |
/// |---|---|
/// | `space` | [`Selection::toggle_current`] |
/// | `up`/`down`, `Ctrl-P`/`Ctrl-N` | [`Selection::move_up`] / [`Selection::move_down`] |
/// | `a` | [`Selection::select_all_safe`] |
/// | `n` | [`Selection::select_none`] |
/// | `i` | [`Selection::invert`] |
/// | `p` | [`Selection::cycle_preset`] + one-frame "replaces your selection" notice |
/// | `?` | toggle `frame.help` |
/// | `enter` | stop, accepted (allowed with 0 selected) |
/// | `esc` / `q` | stop, aborted |
/// | anything else | no-op |
///
/// Ctrl-C is *not* in this table: with `ISIG` left on (`tty::raw_flags`) the
/// terminal raises `SIGINT` instead of delivering a `0x03` byte, so
/// `run_interactive` observes it via `tty::interrupted()` and aborts there.
/// (`decode` still maps a raw `0x03` to [`Key::Abort`] for sources — tests, a
/// pty wired without `ISIG` — that deliver the byte.)
pub fn apply_key(sel: &mut Selection, key: Key, frame: &mut FrameOpts) -> LoopStep {
    match key {
        Key::Space => sel.toggle_current(),
        Key::Up => sel.move_up(),
        Key::Down => sel.move_down(),
        Key::All => sel.select_all_safe(),
        Key::None => sel.select_none(),
        Key::Invert => sel.invert(),
        Key::Preset => {
            sel.cycle_preset();
            // CLI-UX §3.2: the cycle replaces the selection, so warn for one
            // frame. UI state, deliberately not on `Selection` (slice 1 note).
            frame.notice = true;
        }
        Key::Help => frame.help = !frame.help,
        Key::Enter => return LoopStep::Stop(true),
        Key::Abort => return LoopStep::Stop(false),
        Key::Yes => {}
        Key::Resize | Key::Unknown => {}
    }
    LoopStep::Continue
}

/// Drive `sel` against the real tty until `enter` (`Accepted`) or an abort
/// (`Aborted`). Owns the [`tty::RawMode`] and [`tty::SignalGuard`] guards, so
/// the terminal *and* the previous SIGINT/SIGWINCH dispositions are restored on
/// every in-process exit path, including panic unwind.
///
/// `Err` only for setup failure with no mutation (stdin is not a TTY, or
/// `tcgetattr`/`tcsetattr`/`sigaction` failed); `src/main.rs` maps the TTY case
/// to the copy-locked `error: interactive mode requires a TTY; use
/// --preset/--only with --yes` (CLI-UX §5). The whole loop body is one pure
/// render + one pure [`apply_key`], so only raw-mode entry, the key read and
/// the two signals are impure.
pub fn run_interactive(sel: &mut Selection, theme: &Theme) -> std::io::Result<Outcome> {
    // Install the signal handlers for this session (an RAII guard restores the
    // previous dispositions on the way out, *before* raw mode is attempted, so
    // a Ctrl-C after this function returns terminates normally again).
    let _signals = tty::arm()?;

    // stdout is the cursor/erase target: only when it is a TTY. A piped run
    // (`sweep clean | tee`) must not receive `\x1b[…F\x1b[J` bytes, so `None`
    // suppresses both the cursor sequences and the loop's erase sequences.
    // This module's only `unsafe` (the rest of the terminal I/O lives in
    // `ui/tty.rs`, which owns raw mode): a bare `isatty` probe on fd 1.
    // SAFETY: `isatty` only inspects fd 1; no memory is touched, nothing mutated.
    let stdout_is_tty = unsafe { libc::isatty(libc::STDOUT_FILENO) } == 1;
    let output = if stdout_is_tty {
        Some(libc::STDOUT_FILENO)
    } else {
        None
    };
    // `RawMode` is created here and dropped at the end of this body; panics
    // unwind through it, restoring the terminal.
    let guard = tty::RawMode::enter(output)?;
    let erase = guard.owns_stdout();

    let mut decoder = tty::decoder();
    // `READ_BUF` is a drain chunk, not a cap: `read_key_event` loops on fd 0
    // until it would block, so a paste bigger than this buffer is consumed in
    // several reads instead of stranding its tail.
    let mut buf = [0u8; tty::READ_BUF];
    let mut out = std::io::stdout();
    drive(sel, theme, &mut out, erase, |_sel, _frame| {
        match tty::read_key_event(&mut decoder, &mut buf)? {
            // stdin reached EOF: end the session rather than spinning forever.
            tty::Input::Closed => Ok(Source::Eof),
            tty::Input::Idle => Ok(Source::Idle),
            tty::Input::Key(key) => Ok(Source::Key(key)),
        }
    })
}

/// One observation from a key source: a decoded press, an idle poll window, or
/// stdin EOF (the loop stops instead of redrawing forever).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Key(Key),
    Idle,
    Eof,
}

/// The loop parameterized by a key source, for scripted tests: it drives the
/// *real* renderer + `apply_key` mapping without entering raw mode or touching
/// the terminal (`tests/checklist_tty.rs`). The production path is
/// [`run_interactive`], and `tests/checklist_pty.rs` exercises the real
/// `tty::read_key_event` against a PTY.
///
/// The injected source does **not** block: returning `Ok(None)` maps to
/// `Source::Idle`, which redraws immediately. A scripted source must therefore
/// always terminate (return a key or an error) rather than yield `None` forever,
/// or the loop will spin hot. `erase` is unconditionally `false` here, so this
/// seam also cannot assert the piped-stdout erase suppression byte-for-byte.
pub fn run_interactive_with<S>(
    sel: &mut Selection,
    theme: &Theme,
    mut next: S,
) -> std::io::Result<Outcome>
where
    S: FnMut(&Selection, &FrameOpts) -> std::io::Result<Option<Key>>,
{
    run_interactive_with_source(sel, theme, move |sel, frame| {
        Ok(match next(sel, frame)? {
            Some(key) => Source::Key(key),
            None => Source::Idle,
        })
    })
}

/// Like [`run_interactive_with`] but the scripted source yields a full
/// [`Source`], so a test can inject [`Source::Eof`] (which the `Option<Key>`
/// seam cannot express) and assert the loop stops with abort semantics instead
/// of redrawing forever. A `Source::Idle` returned without blocking still spins
/// the loop hot; scripted sources must terminate.
pub fn run_interactive_with_source<S>(
    sel: &mut Selection,
    theme: &Theme,
    next: S,
) -> std::io::Result<Outcome>
where
    S: FnMut(&Selection, &FrameOpts) -> std::io::Result<Source>,
{
    // A discard sink: the scripted tests assert the `Outcome`, not the bytes.
    let mut sink = std::io::sink();
    drive(sel, theme, &mut sink, false, next)
}

/// The impure loop body, factored out of [`run_interactive`] so the scripted
/// tests can run it against a non-tty writer + key source. Draws one frame,
/// reads one key, applies it; returns on `enter` (accepted) or an abort.
///
/// `erase` is false when stdout is not a TTY: the loop then emits no cursor or
/// erase sequences at all (a piped session gets frames only).
fn drive<W, S>(
    sel: &mut Selection,
    theme: &Theme,
    out: &mut W,
    erase: bool,
    mut next: S,
) -> std::io::Result<Outcome>
where
    W: Write,
    S: FnMut(&Selection, &FrameOpts) -> std::io::Result<Source>,
{
    let mut frame = FrameOpts::new(crate::ui::terminal_width());
    // Rows the previous frame occupied, so the redraw erases exactly it.
    let mut prev_height = 0usize;

    loop {
        // SIGWINCH: re-read the width before drawing, so a resize never leaves
        // a stale frame on screen.
        if tty::take_resized() {
            frame.width = crate::ui::terminal_width();
        }

        // Ctrl-C: ISIG stays on, so the terminal raises the signal we poll here
        // (CLI-UX §6 / SAFETY §2: abort, exit 2, nothing deleted).
        if tty::interrupted() {
            return Ok(Outcome::Aborted);
        }

        // Draw. `render_frame` is pure and emits no cursor sequences; the loop
        // erases the previous frame by moving up its row count and clearing to
        // the end of the screen, then prints the new frame. Piped output gets
        // neither (C2: no stray erase bytes in a file).
        let text = render::render_frame(sel, &frame, theme);
        if erase {
            erase_previous(out, prev_height)?;
        }
        write_frame(out, &text)?;
        prev_height = render::frame_height(&text);

        match next(sel, &frame)? {
            // The key source already waited one `READ_TIMEOUT_MS` poll window
            // for input (tty::read_key_event), which paces the loop and keeps
            // SIGINT/SIGWINCH observation prompt; an extra sleep here would
            // double every idle cycle.
            //
            // Pacing is therefore the *source's* job, not the loop's: any
            // injected source that returns `Idle` without blocking (see
            // `run_interactive_with`) will spin this loop hot. Scripted sources
            // must terminate.
            Source::Idle => {}
            // EOF: stop drawing and report the abort semantics (nothing was
            // deleted) rather than redrawing until the heat death of the pty.
            Source::Eof => return Ok(Outcome::Aborted),
            Source::Key(key) => {
                frame.notice = false; // one-frame notice: cleared by any key
                if let LoopStep::Stop(accept) = apply_key(sel, key, &mut frame) {
                    return if accept {
                        Ok(Outcome::Accepted(sel.selected()))
                    } else {
                        Ok(Outcome::Aborted)
                    };
                }
            }
        }
    }
}

/// Write a rendered frame, flushing so the user sees it immediately.
fn write_frame(out: &mut impl Write, text: &str) -> std::io::Result<()> {
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// Erase the `height` rows the previous frame occupied: move the cursor to the
/// start of that block and clear from there to the end of the screen. `0` (the
/// first draw) is a no-op. Keeps the redraw from smearing as the frame grows
/// (the `?` help overlay) or shrinks.
///
/// Only called when stdout is a TTY: `CSI F` clamps at the top of the screen,
/// so a frame taller than the terminal cannot be fully erased (documented in
/// docs/CLI-UX.md §3.1 — the checklist is the last thing drawn and scrollback
/// keeps the overflow).
fn erase_previous(out: &mut impl Write, height: usize) -> std::io::Result<()> {
    if height > 0 {
        write!(out, "\x1b[{height}F\x1b[J")?;
    }
    Ok(())
}

/// Bytes a row contributes when selected (0 when it cannot be selected).
fn row_bytes(r: &RuleResult) -> u64 {
    if r.applied && !REPORT_ONLY_IDS.contains(&r.id) {
        r.info.bytes
    } else {
        0
    }
}

/// Initial row state from one scan result (SAFETY §2, normative matrix).
///
/// Precedence, deliberate and test-covered:
/// 1. report-only marker (D9), never selectable, wins over everything;
/// 2. `applied == false`, `Disabled` with its reason (the guard map cannot
///    promote a rule the scan never applied to a *selectable* state);
/// 3. guard held, `Guarded`, but **only** when the tier would otherwise be
///    selectable. A destructive row outside `full` stays `Locked` even when
///    guarded: SAFETY §2 ("destructive ... locked → unlocked-but-unchecked
///    under full") and CLI-UX §3.2 ("destructive rows: only when preset=full")
///    both say destructive is not toggleable outside `full`, and a guard must
///    never *widen* what is selectable.
/// 4. otherwise the tier/preset default.
fn row_state_for(
    r: &RuleResult,
    preset: Preset,
    i_know_what_im_doing: bool,
    guards: &BTreeMap<&str, String>,
) -> RowState {
    if REPORT_ONLY_IDS.contains(&r.id) {
        return RowState::ReportOnly;
    }
    if !r.applied {
        return RowState::Disabled {
            reason: r
                .reason
                .clone()
                .unwrap_or_else(|| "not applicable".to_string()),
        };
    }
    if let Some(reason) = guards.get(r.id) {
        // Destructive locking wins outside `full` (SAFETY §2 invariant).
        if r.tier == Tier::Destructive && preset != Preset::Full && !i_know_what_im_doing {
            return RowState::Locked;
        }
        return RowState::Guarded {
            reason: reason.clone(),
            checked: false,
        };
    }
    default_state_for(r.tier, preset, i_know_what_im_doing)
}

/// The default selection for a live, non-guarded, non-report-only row.
///
/// Transcribed from SAFETY §2:
/// - safe: checked in every preset (`--yes` is inert here)
/// - caution: always unchecked
/// - destructive: locked outside `full`; unlocked-but-unchecked under `full`
///   or with `--i-know-what-im-doing`
fn default_state_for(tier: Tier, preset: Preset, i_know_what_im_doing: bool) -> RowState {
    match tier {
        Tier::Safe => RowState::Ready { checked: true },
        Tier::Caution => RowState::Ready { checked: false },
        Tier::Destructive => {
            if preset == Preset::Full || i_know_what_im_doing {
                RowState::Ready { checked: false }
            } else {
                RowState::Locked
            }
        }
    }
}

/// Re-derive a row's state for a new preset, keeping guards intact.
///
/// `i_know_what_im_doing` is sticky across preset cycles (SAFETY §2 bypass is a
/// CLI-level unlock): destructive rows re-derive as unlocked-but-unchecked in
/// every preset when the flag is set.
fn apply_preset_to_state(
    state: &RowState,
    tier: Tier,
    preset: Preset,
    i_know_what_im_doing: bool,
) -> RowState {
    match state {
        RowState::Guarded { reason, .. } => {
            // Same precedence as `row_state_for`: re-lock a guarded destructive
            // row if the new preset no longer allows toggling it.
            if tier == Tier::Destructive && preset != Preset::Full && !i_know_what_im_doing {
                return RowState::Locked;
            }
            RowState::Guarded {
                reason: reason.clone(),
                checked: false,
            }
        }
        RowState::Disabled { reason } => RowState::Disabled {
            reason: reason.clone(),
        },
        RowState::ReportOnly => RowState::ReportOnly,
        // Ready/Locked: recompute the preset default. `i_know_what_im_doing`
        // is sticky (a CLI-level unlock survives a preset cycle), the rest of
        // the state is a fresh default application (CLI-UX §3.2).
        RowState::Ready { .. } | RowState::Locked => {
            default_state_for(tier, preset, i_know_what_im_doing)
        }
    }
}
