//! Rule execution progress bars.

use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;

#[derive(Clone)]
pub struct RuleProgress {
    pb: Option<ProgressBar>,
}

impl RuleProgress {
    pub fn new(show: bool, unicode: bool) -> Self {
        if !show || !std::io::stderr().is_terminal() {
            return Self { pb: None };
        }
        let pb = ProgressBar::new_spinner();
        let style = ProgressStyle::with_template("{spinner:.green} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .tick_strings(if unicode {
                &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
            } else {
                &["|", "/", "-", "\\"]
            });
        pb.set_style(style);
        pb.enable_steady_tick(std::time::Duration::from_millis(80));
        Self { pb: Some(pb) }
    }

    pub fn start_rule(&self, id: &str) {
        if let Some(pb) = &self.pb {
            pb.set_message(format!("{} ...", id));
        }
    }

    pub fn update(&self, msg: &str) {
        if let Some(pb) = &self.pb {
            pb.set_message(msg.to_string());
        }
    }

    pub fn finish_rule(&self, id: &str, ok: bool) {
        if let Some(pb) = &self.pb {
            if ok {
                pb.set_message(format!("{} done", id));
            } else {
                pb.set_message(format!("{} done (errors)", id));
            }
        }
    }

    pub fn finish(&self) {
        if let Some(pb) = &self.pb {
            pb.finish_and_clear();
        }
    }
}

impl Default for RuleProgress {
    fn default() -> Self {
        Self::new(false, true)
    }
}


pub struct ScanSpinner;

impl ScanSpinner {
    pub fn spawn(
        total: usize,
        rx: std::sync::mpsc::Receiver<crate::scan::ScanEvent>,
        show: bool,
        unicode: bool,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            use crate::scan::ScanEvent;
            use indicatif::{ProgressBar, ProgressStyle};
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
