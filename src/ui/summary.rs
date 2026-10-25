//! Summary card.

use crate::exec::RunReport;

pub fn print_summary(report: &RunReport) {
    println!("  ╭──────────────────────────────────────────────────────╮");
    println!(
        "  │  [x] freed {} B in {:?}                         │",
        report.freed_total, report.duration
    );
    println!("  │     cleaned: {} · errors: {}                       │", report.items.len(), report.items.iter().map(|i| i.errors.len()).sum::<usize>());
    println!("  ╰──────────────────────────────────────────────────────╯");
}
