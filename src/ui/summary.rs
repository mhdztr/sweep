//! Summary card for clean runs.

use crate::exec::RunReport;
use crate::ui::format::format_bytes;
use std::path::Path;

pub fn print_summary(report: &RunReport) {
    let total_freed = report.freed_total;
    let cleaned = report.items.len();
    let skipped: usize = report.items.iter().map(|i| i.skipped.len()).sum();
    let errors: usize = report.items.iter().map(|i| i.errors.len()).sum();

    println!("  ╭──────────────────────────────────────────────────────╮");
    if report.duration.as_secs() == 0 {
        println!(
            "  │  [x] freed {} in {:?}                         │",
            format_bytes(total_freed),
            report.duration
        );
    } else {
        println!(
            "  │  [x] freed {} in {:.1}s                         │",
            format_bytes(total_freed),
            report.duration.as_secs_f64()
        );
    }
    println!(
        "  │     cleaned: {} · skipped: {} (see log)       │",
        cleaned, skipped
    );
    if errors > 0 {
        println!("  │     errors:  {}                                      │", errors);
    } else {
        println!("  │     errors:  {}                                      │", errors);
    }
    if let Ok(home) = std::env::var("HOME") {
        let log_dir = Path::new(&home).join(".local/state/sweep/logs");
        println!(
            "  │     log: {}                         │",
            log_dir.display()
        );
    } else {
        println!("  │     log: ~/.local/state/sweep/logs                │");
    }
    println!("  ╰──────────────────────────────────────────────────────╯");
}
