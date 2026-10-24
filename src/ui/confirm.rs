//! P1 end-of-selection summary gate (all tiers; default = cancel).
//! Spec: docs/CLI-UX.md §4 P1; gates: docs/SAFETY.md §2.

use crate::rules::{Tier, registry};
use crate::ui::Theme;
use crate::ui::format::format_bytes;
use crate::ui::table::display_width;
use crate::ui::tty::{self, Key};
use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};

/// Exit code for abort (SAFETY §2).
pub const ABORT_EXIT: i32 = 2;

/// Abort copy (stderr).
pub const ABORT_COPY: &str = "aborted, nothing was deleted";

/// No-TTY copy for interactive prompts.
pub const NO_TTY_COPY: &str =
    "error: interactive mode requires a TTY; use --preset/--only with --yes";

#[derive(Debug, Clone)]
pub struct ConfirmItem {
    pub id: &'static str,
    pub bytes: u64,
    pub needs_sudo: bool,
}

fn tier_for_id(id: &str) -> Tier {
    for rule in registry() {
        if rule.id() == id {
            return rule.tier();
        }
    }
    Tier::Safe
}

fn sanitize_text(s: &str, unicode: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let b = c as u32;
        if b < 0x20 && c != '\t' {
            continue;
        }
        if b == 0x7f {
            continue;
        }
        if (0x80..=0x9f).contains(&b) {
            continue;
        }
        if !unicode && !c.is_ascii() {
            continue;
        }
        out.push(c);
    }
    out
}

pub fn render_p1(items: &[ConfirmItem], theme: &Theme) -> String {
    let total_bytes: u64 = items.iter().map(|i| i.bytes).sum();
    let count = items.len();
    let noun = if count == 1 { "category" } else { "categories" };

    // Compute destructive count by looking up tiers from registry
    let mut destructive_count = 0;
    let mut seen = HashSet::new();
    for item in items {
        if seen.contains(item.id) {
            continue;
        }
        seen.insert(item.id);
        if matches!(tier_for_id(item.id), Tier::Destructive) {
            destructive_count += 1;
        }
    }

    let line1 = sanitize_text(
        &format!(
            "{} {} · {} will be deleted",
            count,
            noun,
            format_bytes(total_bytes)
        ),
        theme.unicode,
    );
    let line2 = sanitize_text(
        &format!("{} destructive category included", destructive_count),
        theme.unicode,
    );
    let line3 = sanitize_text("deletions are permanent (use --trash to", theme.unicode);
    let line4 = sanitize_text("stage instead)", theme.unicode);

    if !theme.unicode {
        let mut out = String::new();
        out.push_str(&format!("  {}\n", line1));
        out.push_str(&format!("  {}\n", line2));
        out.push_str(&format!("  {}\n", line3));
        out.push_str(&format!("  {}\n", line4));
        out.push('\n');
        out.push_str("  Proceed? [y/N] (cancel is default)  \n");
        out
    } else {
        let lines = vec![line1.clone(), line2.clone(), line3.clone(), line4.clone()];
        let mut inner = 0;
        for l in &lines {
            let dw = display_width(l) + 2;
            if dw > inner {
                inner = dw;
            }
        }
        if inner < 45 {
            inner = 45;
        }
        let border = "─".repeat(inner);
        let mut out = String::new();
        out.push_str(&format!("  ┌{}┐\n", border));
        for l in &lines {
            let c = format!("  {}", l);
            let pad = if inner >= display_width(&c) {
                inner - display_width(&c)
            } else {
                0
            };
            out.push_str(&format!("  │{}{}│\n", c, " ".repeat(pad)));
        }
        out.push_str(&format!("  └{}┘\n", border));
        out.push('\n');
        out.push_str("  Proceed? [y/N] (cancel is default)  ▸\n");
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmOutcome {
    Proceed,
    Abort,
}

/// Run P1 confirmation. Default is cancel (Enter/Esc/q/Ctrl-C → abort).
/// Only 'y' or 'Y' proceeds. --yes does NOT auto-confirm P1.
pub fn confirm_p1(items: &[ConfirmItem], theme: &Theme) -> io::Result<ConfirmOutcome> {
    if !std::io::stdin().is_terminal() {
        return Err(io::Error::new(io::ErrorKind::NotConnected, NO_TTY_COPY));
    }

    // Install signal handlers for this prompt (restore on drop).
    let _signals = tty::arm()?;

    // Determine if stdout is a tty for raw mode/cursor handling
    let stdout_is_tty = unsafe { libc::isatty(libc::STDOUT_FILENO) } == 1;
    let output = if stdout_is_tty {
        Some(libc::STDOUT_FILENO)
    } else {
        None
    };
    let _guard = tty::RawMode::enter(output)?;

    let prompt = render_p1(items, theme);
    print!("{}", prompt);
    io::stdout().flush()?;

    let mut decoder = tty::decoder();
    let mut buf = [0u8; tty::READ_BUF];
    loop {
        if tty::interrupted() {
            eprintln!("\n{ABORT_COPY}");
            return Ok(ConfirmOutcome::Abort);
        }
        match tty::read_key_event(&mut decoder, &mut buf)? {
            tty::Input::Closed => {
                eprintln!("{ABORT_COPY}");
                return Ok(ConfirmOutcome::Abort);
            }
            tty::Input::Idle => {
                continue;
            }
            tty::Input::Key(key) => match key {
                Key::Yes => return Ok(ConfirmOutcome::Proceed),
                Key::Enter => {
                    eprintln!("{ABORT_COPY}");
                    return Ok(ConfirmOutcome::Abort);
                }
                Key::Abort => {
                    eprintln!("{ABORT_COPY}");
                    return Ok(ConfirmOutcome::Abort);
                }
                _ => {
                    // Any other input aborts (including 'n', 'q' etc., and we must not proceed on partial)
                    eprintln!("{ABORT_COPY}");
                    return Ok(ConfirmOutcome::Abort);
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Theme;

    #[test]
    fn render_p1_contains_prompt_and_counts() {
        let items = vec![
            ConfirmItem {
                id: "tmp",
                bytes: 1024,
                needs_sudo: false,
            },
            ConfirmItem {
                id: "downloads",
                bytes: 2048,
                needs_sudo: false,
            },
        ];
        let theme = Theme {
            color: false,
            unicode: false,
        };
        let s = render_p1(&items, &theme);
        assert!(s.contains("2 categories"));
        assert!(s.contains("Proceed? [y/N]"));
    }
}
