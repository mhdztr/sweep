//! Scan progress spinner (indicatif, stderr). Spec: plan §2.15.

use crate::scan::ScanEvent;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::mpsc;
use std::thread::JoinHandle;

pub struct ScanSpinner;

impl ScanSpinner {
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
            let style =
                ProgressStyle::with_template(" {msg} {bar} {pos}/{len} rules {elapsed_precise}")
                    .unwrap_or_else(|_| ProgressStyle::default_bar())
                    .progress_chars(if unicode { "█░" } else { "#-" });
            pb.set_style(style);
            pb.set_message(if unicode { "Scanning…" } else { "Scanning..." });
            while let Ok(ev) = rx.recv() {
                if let ScanEvent::RuleDone { done, total, .. } = ev {
                    pb.set_length(total as u64);
                    pb.set_position(done as u64);
                }
            }
            pb.finish_and_clear();
        })
    }
}

pub struct RuleProgress;

impl RuleProgress {
    pub fn new() -> Self {
        Self
    }
    pub fn update(&self, _msg: &str) {}
    pub fn finish(&self) {}
}

impl Default for RuleProgress {
    fn default() -> Self {
        Self::new()
    }
}
