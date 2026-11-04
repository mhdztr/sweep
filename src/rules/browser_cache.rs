use super::{Detect, Rule, any_path_exists, scan_targets};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};
use std::path::PathBuf;
use std::time::Duration;

const HOT_WINDOW: Duration = Duration::from_secs(300);

pub struct BrowserCacheRule;

impl Rule for BrowserCacheRule {
    fn id(&self) -> &'static str {
        "browser-cache"
    }
    fn title(&self) -> &'static str {
        "Browser caches"
    }
    fn category(&self) -> &'static str {
        "browser"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".cache"))
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.browser_cache.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".into(),
            };
        }
        if !any_path_exists(&target_paths(ctx)) {
            return Detect::NotApplicable {
                reason: "browser caches not found".into(),
            };
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        scan_targets(target_paths(ctx), svc, Some(HOT_WINDOW), None)
    }
}

pub fn target_paths(ctx: &Ctx) -> Vec<PathBuf> {
    let cache = ctx.home.join(".cache");
    let mut targets = Vec::new();

    for root in [
        cache.join("mozilla/firefox"),
        ctx.home.join("snap/firefox/common/.cache/mozilla/firefox"),
    ] {
        for profile in super::child_dirs(&root) {
            targets.extend([
                profile.join("cache2"),
                profile.join("startupCache"),
                profile.join("OfflineCache"),
            ]);
        }
    }

    for root in [
        cache.join("chromium"),
        cache.join("google-chrome"),
        cache.join("BraveSoftware/Brave-Browser"),
        cache.join("microsoft-edge"),
    ] {
        for profile in super::child_dirs(&root) {
            targets.extend([
                profile.join("Cache"),
                profile.join("Code Cache"),
                profile.join("GPUCache"),
                profile.join("Service Worker/CacheStorage"),
            ]);
        }
    }
    targets
}
