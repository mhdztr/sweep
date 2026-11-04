use super::{Detect, Rule, Tier};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};

pub struct PurgeLogsRule;

impl Rule for PurgeLogsRule {
    fn id(&self) -> &'static str {
        "purge-logs"
    }
    fn title(&self) -> &'static str {
        "All logs"
    }
    fn category(&self) -> &'static str {
        "logs (purge)"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "/var/log".to_string()
    }
    fn tier(&self) -> Tier {
        Tier::Destructive
    }
    fn needs_sudo(&self) -> bool {
        true
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
}
