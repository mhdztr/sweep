use super::{Detect, Rule, Tier};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};

pub struct DownloadsRule;

impl Rule for DownloadsRule {
    fn id(&self) -> &'static str {
        "downloads"
    }
    fn title(&self) -> &'static str {
        "Entire ~/Downloads"
    }
    fn category(&self) -> &'static str {
        "downloads"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join("Downloads"))
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
