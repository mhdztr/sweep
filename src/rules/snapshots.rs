use super::{Detect, Rule};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};

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
    fn detect(&self, _ctx: &Ctx) -> Detect {
        Detect::Ready
    }
    fn scan(&self, _ctx: &Ctx, _svc: &ScanService) -> Result<RuleScan, ScanError> {
        Ok(RuleScan {
            info: Default::default(),
            skipped: Default::default(),
        })
    }
    fn report_only(&self) -> bool {
        true
    }
}
