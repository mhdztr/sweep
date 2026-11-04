use super::{Detect, Rule, Tier};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};

pub struct SteamShaderRule;

impl Rule for SteamShaderRule {
    fn id(&self) -> &'static str {
        "steam-shader"
    }
    fn title(&self) -> &'static str {
        "Steam shader cache"
    }
    fn category(&self) -> &'static str {
        "steam"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "~/.local/share/Steam".to_string()
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
