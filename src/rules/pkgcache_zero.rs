use super::{Detect, Rule, Tier};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};

pub struct PkgcacheZeroRule;

impl Rule for PkgcacheZeroRule {
    fn id(&self) -> &'static str {
        "pkgcache-zero"
    }
    fn title(&self) -> &'static str {
        "Pacman cache → zero"
    }
    fn category(&self) -> &'static str {
        "pacman cache (zero)"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "/var/cache/pacman/pkg".to_string()
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
