//! Scan progress spinner (indicatif, stderr). Spec: plan §2.15.
//!
//! The bar renders on stderr only when the caller says `show` (stderr is a
//! TTY and neither `--json` nor `--quiet` is set); piped output stays clean.

use crate::scan::ScanEvent;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::mpsc;
use std::thread::JoinHandle;

pub struct ScanSpinner;

impl ScanSpinner {
    /// Spawn the thread owning `rx` + the indicatif bar.
    ///
    /// Template (plan §2.15): `" {msg} {bar} {pos}/{len} rules {elapsed_precise}"`
    /// with `msg = "Scanning…"` / `"Scanning..."`, bar chars `█░` / `#-`.
    /// Loops `recv()` → `RuleDone` ⇒ `set_position(done)`; exits when the
    /// channel closes (scan's `ScanService` drops the sender) and clears the
    /// line with `finish_and_clear()`. `show == false` just drains the
    /// channel so senders never block.
    ///
    /// **Deviation (documented):** the plan sketch is
    /// `spawn(total, rx, show)` but *also* demands the ascii template — the
    /// spinner cannot know the charset from `rx`, so `unicode` is an
    /// explicit parameter (callers pass `ctx.flags.unicode`).
    pub fn spawn(
        total: usize,
        rx: mpsc::Receiver<ScanEvent>,
        show: bool,
        unicode: bool,
    ) -> JoinHandle<()> {
        std::thread::spawn(move || {
            if !show {
                while rx.recv().is_ok() {}
                return;
            }
            let pb = ProgressBar::new(total as u64);
            let style = ProgressStyle::with_template(" {msg} {bar} {pos}/{len} rules {elapsed_precise}")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars(if unicode { "█░" } else { "#-" });
            pb.set_style(style);
            pb.set_message(if unicode { "Scanning…" } else { "Scanning..." });
            while let Ok(ev) = rx.recv() {
                if let ScanEvent::RuleDone { done, .. } = ev {
                    pb.set_position(done as u64);
                }
            }
            // Channel closed: scan finished. Clear the bar so stdout output
            // (header/table/footer) starts on a clean line.
            pb.finish_and_clear();
        })
    }
}
