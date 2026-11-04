use super::{Detect, Rule, any_path_exists, scan_targets};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};
use std::path::PathBuf;
use std::time::Duration;

const HOT_WINDOW: Duration = Duration::from_secs(300);
const CACHE_NAMES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "CachedData",
    "CachedExtensionVSIXs",
];

pub struct EditorCacheRule;

impl Rule for EditorCacheRule {
    fn id(&self) -> &'static str {
        "editor-cache"
    }
    fn title(&self) -> &'static str {
        "Editor caches"
    }
    fn category(&self) -> &'static str {
        "editor"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".config"))
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.editor_cache.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".into(),
            };
        }
        if !any_path_exists(&target_paths(ctx)) {
            return Detect::NotApplicable {
                reason: "editor caches not found".into(),
            };
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        scan_targets(target_paths(ctx), svc, Some(HOT_WINDOW), None)
    }
}

pub fn target_paths(ctx: &Ctx) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    for app in ["Code", "Cursor", "VSCodium", ".vscode-oss"] {
        let root = ctx.home.join(".config").join(app);
        targets.extend(CACHE_NAMES.iter().map(|name| root.join(name)));
    }
    for app in ["Slack", "Discord", "Element"] {
        let root = ctx.home.join(".config").join(app);
        targets.extend(super::child_dirs(&root).into_iter().filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("Cache"))
        }));
    }
    targets
}
