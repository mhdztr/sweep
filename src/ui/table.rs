//! Hand-rolled scan table (comfy-table is kept as a dependency but unused;
//! plan §2.15 settles on a hand-rolled ANSI/emoji-aware renderer + unicode-width).
//! Spec: plan §2.15; copy: `docs/CLI-UX.md` §2.1 (mock) / §8 (tier glyphs).

use crate::rules::Tier;
use crate::scan::RuleResult;
use crate::scan::walker::SkipKind;
use crate::ui::format::{format_age, format_bytes};
use crate::ui::{CAUTION, DESTRUCTIVE, SAFE, Theme};
use unicode_width::UnicodeWidthChar;

/// ANSI-aware display width: CSI sequences (ESC `[` … final byte 0x40..=0x7E)
/// contribute 0; everything else counts via unicode-width (emoji = 2).
pub fn display_width(s: &str) -> usize {
    let mut width = 0usize;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                for n in it.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            // Non-CSI escape: skip the introducing char only.
            continue;
        }
        width += UnicodeWidthChar::width(c).unwrap_or(0);
    }
    width
}

/// Hard-truncate `s` to `max` display columns; appends `…`/`...` when
/// anything was cut.
///
/// ANSI-safe: a CSI sequence (ESC `[` … final byte 0x40..=0x7E) is *copied
/// whole* to the output and contributes 0 columns, so a cut can never land
/// inside `\x1b[38;2;…` and leave the terminal colored (the previous version
/// counted SGR parameter bytes as width and split sequences in half). When the
/// cut happens while a color is open, a `\x1b[0m` reset is appended so the
/// terminal is never left painted.
pub fn truncate(s: &str, max: usize, unicode: bool) -> String {
    let ellipsis = if unicode { "…" } else { "..." };
    if display_width(s) <= max {
        return s.to_string();
    }
    // Reserve the ellipsis's columns for the marker, but never force a text
    // budget larger than `max` allows: at `max` < ellipsis width the result
    // below is the ellipsis itself trimmed to `max`, so the output still fits.
    let budget = max.saturating_sub(display_width(ellipsis));
    let mut out = String::new();
    let mut w = 0usize;
    let mut chars = s.chars().peekable();
    // Whether an SGR/CSI paint is currently open in `out` (set by any CSI we
    // copied, cleared by a reset copy); drives the closing reset below.
    let mut painted = false;
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Copy the whole escape (CSI body or the lone ESC) verbatim: it
            // costs 0 columns and must never be split.
            out.push(c);
            if chars.peek() == Some(&'[') {
                out.push(chars.next().expect("peeked `[`"));
                let mut body = String::new();
                for n in chars.by_ref() {
                    body.push(n);
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
                if body == "m" || body.ends_with('m') {
                    // `\x1b[m` and `\x1b[0m` are resets; `\x1b[38;…m` opens one.
                    painted = !(body == "m" || body.starts_with('0'));
                }
                out.push_str(&body);
            }
            continue;
        }
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > budget {
            break;
        }
        out.push(c);
        w += cw;
    }
    if painted {
        out.push_str("\u{1b}[0m");
    }
    if budget == 0 {
        // `max` is narrower than the ellipsis: keep only `max` columns of it.
        out.push_str(&truncate_plain(ellipsis, max));
    } else {
        out.push_str(ellipsis);
    }
    out
}

/// Truncate an escape-free string to at most `max` display columns, with no
/// ellipsis (used only for the narrow-`max` ellipsis path in [`truncate`]).
fn truncate_plain(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut w = 0usize;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out
}

// ---------------------------------------------------------------------------
// Styled segments: pad on visible width, paint after padding so escapes never
// count toward alignment.
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum Style {
    Plain,
    Dim,
    Color((u8, u8, u8)),
}

#[derive(Clone)]
struct Seg {
    text: String,
    style: Style,
}

impl Seg {
    fn plain(text: impl Into<String>) -> Self {
        Seg {
            text: text.into(),
            style: Style::Plain,
        }
    }
    fn dim(text: impl Into<String>) -> Self {
        Seg {
            text: text.into(),
            style: Style::Dim,
        }
    }
    fn color(color: (u8, u8, u8), text: impl Into<String>) -> Self {
        Seg {
            text: text.into(),
            style: Style::Color(color),
        }
    }
    fn render(&self, theme: &Theme) -> String {
        match self.style {
            Style::Plain => self.text.clone(),
            Style::Dim => theme.dim(&self.text),
            Style::Color(c) => theme.paint(c, &self.text),
        }
    }
}

struct Cell {
    segs: Vec<Seg>,
    right: bool,
}

impl Cell {
    fn left(segs: Vec<Seg>) -> Self {
        Cell { segs, right: false }
    }
    fn right(segs: Vec<Seg>) -> Self {
        Cell { segs, right: true }
    }
    fn plain_right(text: impl Into<String>) -> Self {
        Cell::right(vec![Seg::plain(text)])
    }
    fn visible(&self) -> usize {
        self.segs.iter().map(|s| display_width(&s.text)).sum()
    }
    fn render(&self, theme: &Theme) -> String {
        self.segs.iter().map(|s| s.render(theme)).collect()
    }
}

/// Vertical bar: `│` unicode, `|` ascii (plan §2.15/§4).
fn vbar(unicode: bool) -> char {
    if unicode { '│' } else { '|' }
}

/// One table row: `" │" + cell + "│" + … + "│"`. Cells render `" "` + content
/// padded to (w−2) + `" "`; overflow squeezes the padding first.
fn render_line(cells: &[Cell], widths: &[usize], theme: &Theme) -> String {
    let bar = vbar(theme.unicode);
    let mut line = format!(" {bar}");
    for (cell, w) in cells.iter().zip(widths) {
        let inner = w.saturating_sub(2);
        let pad = inner.saturating_sub(cell.visible());
        let body = if cell.right {
            format!(" {}{} ", " ".repeat(pad), cell.render(theme))
        } else {
            format!(" {}{} ", cell.render(theme), " ".repeat(pad))
        };
        line.push_str(&body);
        line.push(bar);
    }
    line
}

/// Border row: unicode box-drawing (`┌┬┐├┼┤└┴┘`, `─`) or ASCII (`+`, `-`)
/// when `theme.unicode` is false, plan §2.15/§4 require `+-+|` in the ascii
/// golden (`--no-unicode` must emit no box-drawing glyphs at all).
fn border_line(kind: BorderKind, widths: &[usize], unicode: bool) -> String {
    let mut line = String::from(" ");
    line.push(kind.lead(unicode));
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            line.push(kind.mid(unicode));
        }
        line.push_str(&kind.fill(unicode).to_string().repeat(*w));
    }
    line.push(kind.tail(unicode));
    line
}

#[derive(Clone, Copy)]
enum BorderKind {
    Top,
    Mid,
    Bottom,
}

impl BorderKind {
    fn lead(self, unicode: bool) -> char {
        match (self, unicode) {
            (_, false) => '+',
            (BorderKind::Top, true) => '┌',
            (BorderKind::Mid, true) => '├',
            (BorderKind::Bottom, true) => '└',
        }
    }
    fn mid(self, unicode: bool) -> char {
        match (self, unicode) {
            (_, false) => '+',
            (BorderKind::Top, true) => '┬',
            (BorderKind::Mid, true) => '┼',
            (BorderKind::Bottom, true) => '┴',
        }
    }
    fn tail(self, unicode: bool) -> char {
        match (self, unicode) {
            (_, false) => '+',
            (BorderKind::Top, true) => '┐',
            (BorderKind::Mid, true) => '┤',
            (BorderKind::Bottom, true) => '┘',
        }
    }
    fn fill(self, unicode: bool) -> char {
        if unicode { '─' } else { '-' }
    }
}

fn tier_color(tier: Tier) -> (u8, u8, u8) {
    match tier {
        Tier::Safe => SAFE,
        Tier::Caution => CAUTION,
        Tier::Destructive => DESTRUCTIVE,
    }
}

fn tier_cell(r: &RuleResult) -> Cell {
    Cell::left(vec![Seg::color(
        tier_color(r.tier),
        format!("{} {}", r.tier.glyph(), r.tier.label()),
    )])
}

/// Note/reason suffix for the wide category cell: ` {note}` when the note is
/// already parenthesized ("(excl. thumbnails)"), else ` ({note})` (D-6 /
/// D-5 interpretation recorded in plan notes).
fn suffix(note: &str) -> String {
    if note.starts_with('(') {
        format!(" {note}")
    } else {
        format!(" ({note})")
    }
}

/// Size + files cells: applied ⇒ formatted values; not applied ⇒ `—`.
fn size_cell(r: &RuleResult) -> Cell {
    if r.applied {
        Cell::plain_right(format_bytes(r.info.bytes))
    } else {
        Cell::plain_right("—")
    }
}

fn files_cell(r: &RuleResult) -> Cell {
    if r.applied {
        Cell::plain_right(r.info.files.to_string())
    } else {
        Cell::plain_right("—")
    }
}

/// Natural (content) widths + 2 padding chars each; shrink order: category
/// first, then location (floors = content floors 14/9/… plus padding;
/// location may go below its floor as the last resort so the table never
/// wraps past `width`).
fn fit_widths(mut widths: [usize; 5], budget: usize) -> [usize; 5] {
    let mut total: usize = widths.iter().sum();
    if total > budget {
        let shrink = (total - budget).min(widths[1] - 16);
        widths[1] -= shrink;
        total -= shrink;
    }
    if total > budget {
        let shrink = (total - budget).min(widths[2] - 11);
        widths[2] -= shrink;
        total -= shrink;
    }
    if total > budget {
        let shrink = (total - budget).min(widths[2].saturating_sub(6));
        widths[2] -= shrink;
    }
    widths
}

/// `render_scan_table`, width >= 100: full `tier | category | location |
/// size | files` table (one separator row after the header only); 60–99:
/// `tier | category | size` (category shrinks + ellipsis, location/notes
/// dropped); < 60: two-line compact rows. `-v` appends per-rule detail lines
/// (dim) after the table. All lines start with a leading space.
pub fn render_scan_table(
    results: &[RuleResult],
    width: usize,
    theme: &Theme,
    verbose: bool,
) -> String {
    let mut out = String::new();
    if width >= 100 {
        render_wide(results, width, theme, &mut out);
    } else if width >= 60 {
        render_narrow(results, width, theme, &mut out);
    } else {
        render_compact(results, width, theme, &mut out);
    }
    if verbose {
        for r in results.iter().filter(|r| r.applied) {
            out.push_str(&theme.dim(&detail_line(r)));
            out.push('\n');
        }
    }
    out
}

fn render_wide(results: &[RuleResult], width: usize, theme: &Theme, out: &mut String) {
    let tier_cells: Vec<usize> = results
        .iter()
        .map(|r| display_width(&format!("{} {}", r.tier.glyph(), r.tier.label())))
        .collect();
    let cat_cells: Vec<usize> = results
        .iter()
        .map(|r| {
            let note = if r.applied {
                r.info.note.as_deref()
            } else {
                r.reason.as_deref()
            };
            display_width(r.category) + note.map(|n| display_width(&suffix(n))).unwrap_or(0)
        })
        .collect();
    let loc_cells: Vec<usize> = results.iter().map(|r| display_width(&r.location)).collect();
    let size_cells: Vec<usize> = results
        .iter()
        .map(|r| {
            display_width(&if r.applied {
                format_bytes(r.info.bytes)
            } else {
                "—".into()
            })
        })
        .collect();
    let files_cells: Vec<usize> = results
        .iter()
        .map(|r| {
            display_width(&if r.applied {
                r.info.files.to_string()
            } else {
                "—".into()
            })
        })
        .collect();

    // Widths = max(content, header) + 2 padding, floors (content) 6/14/9/4/5
    // (+2 each). Budget = width −1 (leading space) −6 (border chars).
    let natural = |cells: Vec<usize>, header: usize| {
        cells
            .into_iter()
            .chain(std::iter::once(header))
            .max()
            .unwrap_or(0)
            + 2
    };
    let widths = fit_widths(
        [
            natural(tier_cells, display_width("tier")).max(8),
            natural(cat_cells, display_width("category")).max(16),
            natural(loc_cells, display_width("location")).max(11),
            natural(size_cells, display_width("size")).max(6),
            natural(files_cells, display_width("files")).max(7),
        ],
        width.saturating_sub(7),
    );

    out.push_str(&border_line(BorderKind::Top, &widths, theme.unicode));
    out.push('\n');
    out.push_str(&render_line(
        &[
            Cell::left(vec![Seg::plain("tier")]),
            Cell::left(vec![Seg::plain("category")]),
            Cell::left(vec![Seg::plain("location")]),
            Cell::left(vec![Seg::plain("size")]),
            Cell::left(vec![Seg::plain("files")]),
        ],
        &widths,
        theme,
    ));
    out.push('\n');
    out.push_str(&border_line(BorderKind::Mid, &widths, theme.unicode));
    out.push('\n');

    for r in results {
        // Category cell: plain category + dim (note) (wide only, D-6). The
        // plain part shrinks first; a note that cannot fit at all is dropped
        // (visible via `-v`).
        let mut segs = vec![Seg::plain(r.category)];
        let note = if r.applied {
            r.info.note.as_deref()
        } else {
            r.reason.as_deref()
        };
        if let Some(note) = note {
            let s = suffix(note);
            let room = widths[1]
                .saturating_sub(2)
                .saturating_sub(display_width(r.category));
            if room >= display_width(&s) {
                segs.push(Seg::dim(s));
            } else {
                let cat_room = widths[1].saturating_sub(2);
                segs[0] = Seg::plain(truncate(r.category, cat_room, theme.unicode));
            }
        } else {
            let cat_room = widths[1].saturating_sub(2);
            if display_width(r.category) > cat_room {
                segs[0] = Seg::plain(truncate(r.category, cat_room, theme.unicode));
            }
        }
        out.push_str(&render_line(
            &[
                tier_cell(r),
                Cell::left(segs),
                Cell::left(vec![Seg::dim(truncate(
                    &r.location,
                    widths[2].saturating_sub(2),
                    theme.unicode,
                ))]),
                size_cell(r),
                files_cell(r),
            ],
            &widths,
            theme,
        ));
        out.push('\n');
    }

    out.push_str(&border_line(BorderKind::Bottom, &widths, theme.unicode));
    out.push('\n');
}

fn render_narrow(results: &[RuleResult], width: usize, theme: &Theme, out: &mut String) {
    let tier_w = results
        .iter()
        .map(|r| display_width(&format!("{} {}", r.tier.glyph(), r.tier.label())))
        .chain([display_width("tier")])
        .max()
        .unwrap_or(0)
        .max(6)
        + 2;
    let size_w = results
        .iter()
        .map(|r| {
            display_width(&if r.applied {
                format_bytes(r.info.bytes)
            } else {
                "—".into()
            })
        })
        .chain([display_width("size")])
        .max()
        .unwrap_or(0)
        .max(4)
        + 2;
    // Category takes the remaining width; floor 12 so a pathological width
    // still leaves a readable column (lines may squeeze below at <60 only).
    let cat_w = width.saturating_sub(1 + 4 + tier_w + size_w).max(12);
    let widths = [tier_w, cat_w, size_w];

    out.push_str(&border_line(BorderKind::Top, &widths, theme.unicode));
    out.push('\n');
    out.push_str(&render_line(
        &[
            Cell::left(vec![Seg::plain("tier")]),
            Cell::left(vec![Seg::plain("category")]),
            Cell::left(vec![Seg::plain("size")]),
        ],
        &widths,
        theme,
    ));
    out.push('\n');
    out.push_str(&border_line(BorderKind::Mid, &widths, theme.unicode));
    out.push('\n');

    for r in results {
        // Location + notes dropped (visible via `-v`).
        out.push_str(&render_line(
            &[
                tier_cell(r),
                Cell::left(vec![Seg::plain(truncate(
                    r.category,
                    widths[1].saturating_sub(2),
                    theme.unicode,
                ))]),
                size_cell(r),
            ],
            &widths,
            theme,
        ));
        out.push('\n');
    }

    out.push_str(&border_line(BorderKind::Bottom, &widths, theme.unicode));
    out.push('\n');
}

fn render_compact(results: &[RuleResult], width: usize, theme: &Theme, out: &mut String) {
    for r in results {
        let tier = format!("{} {}", r.tier.glyph(), r.tier.label());
        // "  {tier}  {category}", category shrinks to fit, location/notes
        // dropped (visible via `-v`).
        let room = width.saturating_sub(4 + display_width(&tier) + 2);
        out.push_str(&format!(
            "  {}  {}\n",
            theme.paint(tier_color(r.tier), &tier),
            truncate(r.category, room, theme.unicode)
        ));
        let (size, files) = if r.applied {
            (format_bytes(r.info.bytes), r.info.files.to_string())
        } else {
            ("—".to_string(), "—".to_string())
        };
        out.push_str(&format!("    {size} · {files} files\n"));
    }
}

/// `-v` detail line (dim; plan §2.15): counts + newest + nonzero skips +
/// first sample.
fn detail_line(r: &RuleResult) -> String {
    let mut s = format!(
        "  {}: {} files · {} dirs · newest {}",
        r.id,
        r.info.files,
        r.info.dirs,
        format_age(r.info.newest_age)
    );
    let skips = [
        (SkipKind::KeptNewer, "newer-than-gate"),
        (SkipKind::Protected, "protected"),
        (SkipKind::Hot, "hot"),
        (SkipKind::Whitelist, "whitelist"),
        (SkipKind::MountBound, "mount-bound"),
    ];
    let parts: Vec<String> = skips
        .iter()
        .filter_map(|(kind, label)| {
            r.skipped
                .get(kind)
                .copied()
                .filter(|n| *n > 0)
                .map(|n| format!("{n} {label}"))
        })
        .collect();
    if !parts.is_empty() {
        s.push_str(&format!(" · kept: {}", parts.join(", ")));
    }
    if let Some(first) = r.info.samples.first() {
        s.push_str(&format!(" · {}", first.display()));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::ScanInfo;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn display_width_strips_ansi_and_counts_unicode() {
        assert_eq!(display_width("\u{1b}[32m+ safe\u{1b}[0m"), 6);
        assert_eq!(display_width("plain"), 5);
        assert_eq!(display_width(""), 0);
        // unicode-width still earns its keep: box-drawing (1 col) and wide
        // CJK user data (2 cols) are counted correctly.
        assert_eq!(display_width("│"), 1);
        assert_eq!(display_width("漢"), 2);
    }

    #[test]
    fn truncate_math_is_display_width_aware() {
        assert_eq!(truncate("hello", 10, true), "hello");
        assert_eq!(truncate("hello world", 8, true), "hello w…");
        assert_eq!(truncate("hello world", 8, false), "hello...");
        // Wide (2-col) chars prove column-aware truncation.
        assert_eq!(truncate("漢字漢", 5, true), "漢字…");
        assert_eq!(truncate("漢字漢", 4, true), "漢…");
    }

    #[test]
    fn truncate_never_splits_an_ansi_sequence() {
        // A truecolor SGR opener is 18 bytes but 0 columns: the cut must land
        // in the *text*, never inside `\x1b[38;2;…m` (the bug that left the
        // terminal painted and swallowed the following characters).
        let s = "\u{1b}[38;2;255;179;0mtier text here\u{1b}[0m more";
        for max in 1..=24usize {
            let cut = truncate(s, max, true);
            assert!(
                display_width(&cut) <= max,
                "width {max}: {cut:?} is {} cols",
                display_width(&cut)
            );
            // Every CSI in the result is complete: ESC [ … final byte.
            let mut it = cut.chars().peekable();
            while let Some(c) = it.next() {
                if c == '\u{1b}' && it.peek() == Some(&'[') {
                    it.next();
                    let complete = it.by_ref().any(|n| ('\u{40}'..='\u{7e}').contains(&n));
                    assert!(complete, "truncated sequence with width {max}: {cut:?}");
                }
            }
            // A paint left open must be closed by a reset.
            if cut.matches('\u{1b}').count() % 2 == 1 {
                assert!(
                    cut.contains("\u{1b}[0m"),
                    "unclosed paint at width {max}: {cut:?}"
                );
            }
        }
    }

    fn result(applied: bool) -> RuleResult {
        RuleResult {
            id: "tmp",
            title: "Temp files",
            category: "tmp & var-tmp",
            tier: Tier::Safe,
            applied,
            reason: if applied {
                None
            } else {
                Some("/tmp and /var/tmp not found".into())
            },
            needs_sudo: false,
            location: "/tmp".into(),
            info: ScanInfo {
                bytes: 3282,
                files: 2,
                dirs: 2,
                newest_age: None,
                samples: vec![PathBuf::from("/tmp/old.bin")],
                note: Some("1 newer than keep-days kept".into()),
            },
            net_of: Vec::new(),
            skipped: BTreeMap::new(),
        }
    }

    #[test]
    fn compact_mode_is_two_lines_per_row() {
        let results = vec![result(true), result(false)];
        let s = render_scan_table(
            &results,
            50,
            &Theme {
                color: false,
                unicode: true,
            },
            false,
        );
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 4, "compact = 2 lines per row:\n{s}");
        assert_eq!(lines[0], "  + safe  tmp & var-tmp");
        assert_eq!(lines[1], "    3.2 KiB · 2 files");
        assert_eq!(lines[2], "  + safe  tmp & var-tmp");
        assert_eq!(lines[3], "    — · — files");
    }

    #[test]
    fn wide_mode_has_one_header_separator_and_aligned_rows() {
        let results = vec![result(true)];
        let s = render_scan_table(
            &results,
            120,
            &Theme {
                color: false,
                unicode: true,
            },
            false,
        );
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 5, "top + header + sep + row + bottom:\n{s}");
        let width_line = display_width(lines[0]);
        for l in &lines {
            assert_eq!(display_width(l), width_line, "all rows same width:\n{s}");
        }
        assert!(lines[1].contains("tier") && lines[1].contains("location"));
        assert!(lines[3].contains("/tmp") && lines[3].contains("3.2 KiB"));
        assert!(lines[3].contains("1 newer than keep-days kept"));
    }

    #[test]
    fn narrow_mode_drops_location_and_notes() {
        let results = vec![result(true)];
        let s = render_scan_table(
            &results,
            80,
            &Theme {
                color: false,
                unicode: true,
            },
            false,
        );
        assert!(!s.contains("/tmp"), "location dropped in narrow mode:\n{s}");
        assert!(!s.contains("newer"), "notes dropped in narrow mode:\n{s}");
        assert!(s.contains("tmp & var-tmp"));
        assert!(s.contains("3.2 KiB"));
    }

    #[test]
    fn ascii_mode_uses_ascii_borders_and_glyphs() {
        // plan §2.15/§4: `scan_table_ascii` = unicode false → borders `+-+|`,
        // tier `+ safe`, `...` (no box-drawing glyphs anywhere in the table).
        let results = vec![result(true), result(false)];
        let s = render_scan_table(
            &results,
            120,
            &Theme {
                color: false,
                unicode: false,
            },
            false,
        );
        let lines: Vec<&str> = s.lines().collect();
        assert!(
            lines[0].chars().all(|c| "+- ".contains(c)),
            "ascii top border must be +/−:\n{s}"
        );
        assert!(
            lines[1].starts_with(" | tier"),
            "ascii header row uses '|':\n{s}"
        );
        assert!(s.contains("+ safe"), "ascii tier glyph:\n{s}");
        assert!(
            !s.contains('│') && !s.contains('─') && !s.contains('┌'),
            "no box-drawing glyphs in ascii mode:\n{s}"
        );
    }

    #[test]
    fn verbose_appends_dim_detail_lines() {
        let mut r = result(true);
        r.skipped.insert(SkipKind::KeptNewer, 1);
        let s = render_scan_table(
            &[r],
            120,
            &Theme {
                color: true,
                unicode: true,
            },
            true,
        );
        let detail = s
            .lines()
            .find(|l| l.contains("tmp: 2 files"))
            .expect("detail line");
        assert!(detail.contains("newest never"));
        assert!(detail.contains("kept: 1 newer-than-gate"));
        assert!(detail.contains("/tmp/old.bin"));
        assert!(
            detail.contains('\u{1b}'),
            "detail lines are dim (SGR present)"
        );
    }
}
