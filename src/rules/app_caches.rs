use super::{Detect, Rule, any_path_exists, scan_targets};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};
use std::path::PathBuf;
use std::time::Duration;

const HOT_WINDOW: Duration = Duration::from_secs(300);
const APP_GUARDS: &[&str] = &["obsidian", "signal", "telegram-desktop"];

pub struct AppCachesRule;

impl Rule for AppCachesRule {
    fn id(&self) -> &'static str {
        "app-caches"
    }
    fn title(&self) -> &'static str {
        "Misc app caches"
    }
    fn category(&self) -> &'static str {
        "apps"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".cache"))
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.app_caches.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".into(),
            };
        }
        if !any_path_exists(&target_paths(ctx)) {
            return Detect::NotApplicable {
                reason: "app caches not found".into(),
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
    let mut targets: Vec<_> = [
        "electron",
        "mesa_shader_cache",
        "fontconfig",
        "libvirt",
        "gdk-pixbuf",
        "ibus",
        "dconf",
        "glib-2.0",
        "QtWebEngine",
    ]
    .iter()
    .map(|path| cache.join(path))
    .collect();

    for app in APP_GUARDS {
        let root = cache.join(app);
        targets.extend(super::child_dirs(&root).into_iter().filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("Cache"))
        }));
    }
    for extra in &ctx.config.rules.app_caches.extra_paths {
        let path = if let Some(relative) = extra.strip_prefix("~/") {
            ctx.home.join(relative)
        } else {
            PathBuf::from(extra)
        };
        if path != cache && path.starts_with(&cache) {
            targets.push(path);
        }
    }
    targets
}
