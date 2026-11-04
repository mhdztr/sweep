use super::{Detect, Rule, Tier};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};

pub struct TmpFullRule;

impl Rule for TmpFullRule {
    fn id(&self) -> &'static str {
        "tmp-full"
    }
    fn title(&self) -> &'static str {
        "Full /tmp + /var/tmp"
    }
    fn category(&self) -> &'static str {
        "tmp (full)"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "/tmp".to_string()
    }
    fn tier(&self) -> Tier {
        Tier::Destructive
    }
    fn needs_sudo(&self) -> bool {
        false
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
