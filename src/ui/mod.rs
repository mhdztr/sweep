//! Terminal output: theme, header/footer, table, progress spinner.
//! Spec: plan §2.15; copy: `docs/CLI-UX.md` §2.1 / §5.

pub mod checklist;
pub mod format;
pub mod progress;
pub mod table;

use crate::ctx::Ctx;
use crate::ui::format::format_bytes;
use owo_colors::OwoColorize as _;

pub use table::display_width;

/// Tier colors (plan §2.15).
pub const SAFE: (u8, u8, u8) = (0x4c, 0xaf, 0x50);
pub const CAUTION: (u8, u8, u8) = (0xff, 0xb3, 0x00);
pub const DESTRUCTIVE: (u8, u8, u8) = (0xef, 0x53, 0x50);
/// Header border color.
pub const INFO: (u8, u8, u8) = (0x4d, 0xd0, 0xe1);
/// Muted detail lines (`-v` output).
pub const DIM: (u8, u8, u8) = (0x8a, 0x8a, 0x8a);

/// Resolved output theme. `color`/`unicode` are already folded from flags +
/// env by `GlobalFlags::from_root` (NO_COLOR, FORCE_COLOR, TERM, LC_ALL…).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub color: bool,
    pub unicode: bool,
}

impl Theme {
    /// Wrap `s` in a truecolor SGR sequence when colors are on; otherwise
    /// return it unchanged (always an owned `String`).
    pub fn paint(&self, color: (u8, u8, u8), s: &str) -> String {
        if self.color {
            s.truecolor(color.0, color.1, color.2).to_string()
        } else {
            s.to_string()
        }
    }

    /// Bold when colors are on (bold is gated by the same resolved flag;
    /// NO_COLOR disables all SGR, attributes included).
    pub fn bold(&self, s: &str) -> String {
        if self.color {
            s.bold().to_string()
        } else {
            s.to_string()
        }
    }

    /// Dim when colors are on.
    pub fn dim(&self, s: &str) -> String {
        if self.color {
            s.dimmed().to_string()
        } else {
            s.to_string()
        }
    }
}

/// Terminal width via `ioctl(TIOCGWINSZ)` on stdout; 80 when the ioctl fails
/// (not a TTY, piped CI output) or the reported width is 0.
pub fn terminal_width() -> usize {
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `ws` is a valid, correctly aligned `winsize` out-pointer for
    // the duration of the call. fd 1 is always open (possibly a pipe/file);
    // then the ioctl simply fails with ENOTTY and we fall back to 80. No
    // other memory is touched and nothing is mutated.
    let rc = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) };
    if rc == 0 && ws.ws_col > 0 {
        usize::from(ws.ws_col)
    } else {
        80
    }
}

/// Header (plan §2.15 + D-1 render order: the header prints AFTER the scan
/// finishes, see `cli/scan.rs`).
///
/// Box geometry (documented interpretation of the CLI-UX §2.1 mock): the
/// mock's border (`╭`+62×`─`+`╮`) is 1–2 chars wider than its own row lines
/// (61 / 60), it is internally inconsistent. We derive a self-consistent
/// box instead: `inner = max(c1+c2+5, c3+3)`, so row 1 (`"  " + c1 + gap +
/// c2 + " "` with gap ≥ 2) and row 2 (`"  " + c3 + " "`, padded) always fit
/// their border exactly. For the mock's content this yields inner = 60
/// (mock 62; row-1 gap 11 vs the mock's 12). Borders INFO-colored, c1 bold.
///
/// Box only when `theme.unicode && width >= 60`; otherwise the two plain
/// content lines `c1` / `c3` (c2 dropped), hard-truncated to `width`.
/// Returns the render ending in `\n\n` (blank line after the box).
pub fn render_header(ctx: &Ctx, reclaimable: u64, width: usize, theme: &Theme) -> String {
    let c1 = format!("sweep {} · storage cleaner", env!("CARGO_PKG_VERSION"));
    let c2 = format!(
        "{} · {}",
        ctx.system.distro_name.to_lowercase(),
        ctx.system.hostname
    );
    let c3 = format!(
        "root: {} total · {} used · {} reclaimable",
        format_bytes(ctx.system.root_total),
        format_bytes(ctx.system.root_used),
        format_bytes(reclaimable)
    );

    if !theme.unicode || width < 60 {
        // Plain mode: c1 / c3, hard-truncated to the terminal width.
        return format!(
            "{}\n{}\n\n",
            table::truncate(&c1, width, theme.unicode),
            table::truncate(&c3, width, theme.unicode)
        );
    }

    // The box itself must fit the terminal: hard-truncate c2 (and c3) first.
    let mut c2 = c2;
    let mut c3 = c3;
    if display_width(&c3) + 5 > width {
        c3 = table::truncate(&c3, width.saturating_sub(5).max(10), theme.unicode);
    }
    if display_width(&c1) + display_width(&c2) + 7 > width {
        let room = width.saturating_sub(display_width(&c1) + 7).max(10);
        c2 = table::truncate(&c2, room, theme.unicode);
    }

    let c1w = display_width(&c1);
    let c2w = display_width(&c2);
    let c3w = display_width(&c3);
    let inner = (c1w + c2w + 5).max(c3w + 3);
    let gap = inner - c1w - c2w - 3; // ≥ 2 by construction

    let row1 = format!("  {}{}{c2} ", theme.bold(&c1), " ".repeat(gap));
    let row2 = format!("  {c3} ");
    let row2 = format!("{row2}{}", " ".repeat(inner - display_width(&row2)));
    let border = "─".repeat(inner);

    format!(
        "{top}\n{left}{row1}{right}\n{left}{row2}{right}\n{bot}\n\n",
        top = theme.paint(INFO, &format!("╭{border}╮")),
        left = theme.paint(INFO, "│"),
        right = theme.paint(INFO, "│"),
        bot = theme.paint(INFO, &format!("╰{border}╯")),
    )
}

/// Footer (exact plan §2.15 copy). Deliberately plain, `_theme` is accepted
/// for signature stability but no color is applied (plan: "Plain").
/// Returned WITHOUT a trailing newline (callers `println!` it).
pub fn render_footer(reclaimable: u64, regular: u64, scan_ms: u64, _theme: &Theme) -> String {
    format!(
        "reclaimable: {} · regular preset selects: {} · scanned in {:.1}s",
        format_bytes(reclaimable),
        format_bytes(regular),
        scan_ms as f64 / 1000.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx() -> Ctx {
        Ctx::for_test(PathBuf::from("/tmp/sweep-ui-test-home"))
    }

    #[test]
    fn theme_without_color_returns_plain_strings() {
        let t = Theme {
            color: false,
            unicode: true,
        };
        assert_eq!(t.paint(SAFE, "x"), "x");
        assert_eq!(t.bold("x"), "x");
        assert_eq!(t.dim("x"), "x");
        let colored = Theme {
            color: true,
            unicode: true,
        }
        .paint(SAFE, "x");
        assert!(colored.contains('\u{1b}'), "color theme must emit SGR");
    }

    #[test]
    fn header_plain_mode_is_two_truncated_lines() {
        let s = render_header(
            &ctx(),
            20_602,
            40,
            &Theme {
                color: false,
                unicode: false,
            },
        );
        let lines: Vec<&str> = s.split('\n').collect();
        assert_eq!(lines[0], "sweep 0.1.0 · storage cleaner");
        assert!(lines[1].starts_with("root: 1 TiB total"));
        assert_eq!(lines[2], "", "blank line after header");
        assert!(display_width(lines[0]) <= 40 && display_width(lines[1]) <= 40);
    }

    #[test]
    fn header_box_rows_fit_the_border_exactly() {
        let s = render_header(
            &ctx(),
            20_602,
            120,
            &Theme {
                color: false,
                unicode: true,
            },
        );
        let lines: Vec<&str> = s.split('\n').collect();
        let inner = display_width(lines[0]) - 2; // strip ╭/╮
        assert_eq!(display_width(lines[1]), inner + 2, "row1 must fit border");
        assert_eq!(display_width(lines[2]), inner + 2, "row2 must fit border");
        assert!(lines[0].starts_with('╭') && lines[0].ends_with('╮'));
        assert!(lines[3].starts_with('╰') && lines[3].ends_with('╯'));
        assert!(lines[4].is_empty(), "trailing blank line");
        assert!(lines[1].contains("sweep 0.1.0 · storage cleaner"));
        assert!(lines[1].contains("arch linux · testhost"));
    }

    #[test]
    fn footer_matches_the_plan_copy_exactly() {
        let t = Theme {
            color: true,
            unicode: true,
        };
        assert_eq!(
            render_footer(20_602, 20_602, 410, &t),
            "reclaimable: 20.1 KiB · regular preset selects: 20.1 KiB · scanned in 0.4s"
        );
    }
}
