use super::{Detect, Rule, Tier};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};

pub struct DockerRule;
pub struct PodmanRule;

impl Rule for DockerRule {
    fn id(&self) -> &'static str {
        "docker"
    }
    fn title(&self) -> &'static str {
        "Docker reclaim"
    }
    fn category(&self) -> &'static str {
        "docker"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "docker".to_string()
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
    fn volume_gate(&self) -> bool {
        true
    }
}

impl Rule for PodmanRule {
    fn id(&self) -> &'static str {
        "podman"
    }
    fn title(&self) -> &'static str {
        "Podman reclaim"
    }
    fn category(&self) -> &'static str {
        "podman"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "podman".to_string()
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
