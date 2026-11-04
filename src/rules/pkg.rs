use super::{Detect, Rule, Tier};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};

pub struct PacmanCacheRule;
pub struct JournalSystemRule;
pub struct AurBuildsRule;
pub struct OrphansRule;
pub struct FlatpakUnusedRule;

impl Rule for PacmanCacheRule {
    fn id(&self) -> &'static str {
        "pacman-cache"
    }
    fn title(&self) -> &'static str {
        "Pacman package cache"
    }
    fn category(&self) -> &'static str {
        "pacman cache"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "/var/cache/pacman/pkg".to_string()
    }
    fn tier(&self) -> Tier {
        Tier::Caution
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

impl Rule for JournalSystemRule {
    fn id(&self) -> &'static str {
        "journal-system"
    }
    fn title(&self) -> &'static str {
        "System journald"
    }
    fn category(&self) -> &'static str {
        "journal (system)"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "/var/log/journal".to_string()
    }
    fn tier(&self) -> Tier {
        Tier::Caution
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

impl Rule for AurBuildsRule {
    fn id(&self) -> &'static str {
        "aur-builds"
    }
    fn title(&self) -> &'static str {
        "AUR build artifacts"
    }
    fn category(&self) -> &'static str {
        "AUR/build"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "~/.cache/paru/clone".to_string()
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

impl Rule for OrphansRule {
    fn id(&self) -> &'static str {
        "orphans"
    }
    fn title(&self) -> &'static str {
        "Orphaned packages"
    }
    fn category(&self) -> &'static str {
        "orphans"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "pacman".to_string()
    }
    fn tier(&self) -> Tier {
        Tier::Caution
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

impl Rule for FlatpakUnusedRule {
    fn id(&self) -> &'static str {
        "flatpak-unused"
    }
    fn title(&self) -> &'static str {
        "Unused Flatpak runtimes"
    }
    fn category(&self) -> &'static str {
        "flatpak"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "flatpak".to_string()
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
