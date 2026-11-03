//! Scan progress spinner (indicatif, stderr). Spec: plan §2.15.

use crate::scan::ScanEvent;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;
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
            pb.set_message(if unicode {
                "Scanning…"
            } else {
                "Scanning..."
            });
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

pub struct RuleProgress {
    pb: Option<ProgressBar>,
}

impl RuleProgress {
    pub fn new(_total: usize, requested: bool) -> Self {
        let show = requested && std::io::stderr().is_terminal();
        let pb = if show {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::with_template("{spinner:.green} {msg}")
                    .unwrap_or_else(|_| ProgressStyle::default_spinner()),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(80));
            Some(pb)
        } else {
            None
        };
        Self { pb }
    }

    pub fn start(&self) {
        if let Some(pb) = &self.pb {
            pb.set_message("Executing...");
        }
    }

    pub fn start_rule(&self, id: &str) {
        if let Some(pb) = &self.pb {
            pb.set_message(format!("Running {id}"));
        }
    }

    pub fn update_rule(&self, _msg: &str) {
        // no-op for minimal implementation
    }

    pub fn finish_rule(&self) {
        // no-op
    }

    pub fn finish(&self) {
        if let Some(pb) = &self.pb {
            pb.finish_and_clear();
        }
    }
}

impl Default for RuleProgress {
    fn default() -> Self {
        Self::new(0, false)
    }
}
