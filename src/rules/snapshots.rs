use super::{Detect, Rule, command_exists, command_output};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanInfo, ScanService};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct SnapshotsReportRule;

impl Rule for SnapshotsReportRule {
    fn id(&self) -> &'static str {
        "snapshots-report"
    }
    fn title(&self) -> &'static str {
        "btrfs/snapper report"
    }
    fn category(&self) -> &'static str {
        "snapshots"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "/".to_string()
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.snapshots_report.enabled {
            return not_applicable("disabled in config");
        }
        if !ctx.commands_enabled {
            return not_applicable("external commands disabled in test context");
        }
        if ctx.system.fstype != "btrfs" {
            return not_applicable("root filesystem is not btrfs");
        }
        if !command_exists("snapper") && !snapper_configs_exist() {
            return not_applicable("snapper not found");
        }
        Detect::Ready
    }
    fn scan(&self, _ctx: &Ctx, _svc: &ScanService) -> Result<RuleScan, ScanError> {
        let output = command_output("snapper", &["list", "--all-configs", "--columns", "number"]);
        let (count, note) = match output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let count = parse_snapshot_count(&stdout);
                (
                    count,
                    format!("btrfs; {count} snapshots; size unavailable without snapper sizing"),
                )
            }
            Ok(output) => (
                0,
                format!(
                    "btrfs; snapshot count unavailable: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
                0,
                "btrfs; snapper is not installed; count unavailable".into(),
            ),
            Err(source) => {
                return Err(ScanError::Io {
                    path: PathBuf::from("snapper list"),
                    source,
                });
            }
        };
        Ok(RuleScan {
            info: ScanInfo {
                files: count,
                note: Some(note),
                ..ScanInfo::default()
            },
            skipped: BTreeMap::new(),
        })
    }
    fn report_only(&self) -> bool {
        true
    }
}

fn snapper_configs_exist() -> bool {
    std::fs::read_dir("/etc/snapper/configs").is_ok_and(|mut entries| entries.next().is_some())
}

fn parse_snapshot_count(output: &str) -> u64 {
    output
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && trimmed
                    .split_whitespace()
                    .next()
                    .is_some_and(|first| first.parse::<u64>().is_ok())
        })
        .count() as u64
}

fn not_applicable(reason: &str) -> Detect {
    Detect::NotApplicable {
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_count_ignores_headers_and_empty_lines() {
        let output = "  # | Type | Pre | Date\n----+------+-----+-----\n  0 | single | | now\n  1 | pre    | | yesterday\n";
        assert_eq!(parse_snapshot_count(output), 2);
        assert_eq!(parse_snapshot_count("No snapshots found"), 0);
    }
}
