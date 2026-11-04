use super::{Detect, Rule, Tier};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};

pub struct CacheDeepRule;

impl Rule for CacheDeepRule {
    fn id(&self) -> &'static str {
        "cache-deep"
    }
    fn title(&self) -> &'static str {
        "Full ~/.cache wipe"
    }
    fn category(&self) -> &'static str {
        "cache (deep)"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "~/.cache".to_string()
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
