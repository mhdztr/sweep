use super::{Detect, Rule, Tier};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};

pub struct DevArtifactsRule;

impl Rule for DevArtifactsRule {
    fn id(&self) -> &'static str {
        "dev-artifacts"
    }
    fn title(&self) -> &'static str {
        "Stale build artifacts"
    }
    fn category(&self) -> &'static str {
        "dev artifacts"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "~".to_string()
    }
    fn tier(&self) -> Tier {
        Tier::Caution
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
