//! Summary card for clean runs.

use crate::exec::RunReport;
use crate::ui::format;

pub fn print_summary(report: &RunReport) {
    let errors: usize = report.items.iter().map(|i| i.errors.len()).sum();
    let skipped: usize = report.items.iter().map(|i| i.skipped.len()).sum();
    let cleaned = report.items.len();
    let status = if report.dry_run {
        "dry run, nothing changed"
    } else if errors > 0 {
        "completed with errors"
    } else {
        "clean completed"
    };
    println!("{}", border());
    println!("{}", row(status));
    println!(
        "{}",
        row(&format!(
            "freed: {} in {:.1}s",
            format::format_bytes(report.freed_total),
            report.duration.as_secs_f64()
        ))
    );
    println!(
        "{}",
        row(&format!(
            "cleaned: {cleaned}  skipped: {skipped}  errors: {errors}"
        ))
    );
    println!("{}", border());
}

fn row(content: &str) -> String {
    const CONTENT_WIDTH: usize = 54;
    let clipped = content.chars().take(CONTENT_WIDTH).collect::<String>();
    format!("  |  {clipped:<CONTENT_WIDTH$}  |")
}

fn border() -> String {
    "  +".to_string() + &"-".repeat(58) + "+"
}

#[cfg(test)]
mod tests {
    use super::{border, row};

    #[test]
    fn summary_rows_are_ascii_and_aligned() {
        assert_eq!(row("clean completed").len(), border().len());
        assert!(row("clean completed").is_ascii());
        assert!(
            row("a very long message that must be clipped before it can overflow the card")
                .is_ascii()
        );
    }
}
