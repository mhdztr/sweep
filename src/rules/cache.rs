//! Rules `user-cache` + `thumbnails`. Spec: plan §2.11.

use super::{Detect, Rule};
use crate::ctx::Ctx;
use crate::scan::walker::{self, EntryInfo, Prune, SkipKind, WalkOptions};
use crate::scan::{RuleScan, ScanError, ScanService};
use std::sync::Arc;
use std::time::Duration;

/// 5-minute hot window: depth-1 entries touched within it are excluded
/// (scan honesty, a file still being written is not reclaimable). The
/// executor rechecks candidate mtimes before removal.
const HOT_WINDOW: Duration = Duration::from_secs(300);

pub struct UserCacheRule;

impl Rule for UserCacheRule {
    fn id(&self) -> &'static str {
        "user-cache"
    }
    fn title(&self) -> &'static str {
        "User caches"
    }
    fn category(&self) -> &'static str {
        "user caches"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".cache"))
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.user_cache.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".to_string(),
            };
        }
        if !ctx.home.join(".cache").is_dir() {
            return Detect::NotApplicable {
                reason: "~/.cache not found".to_string(),
            };
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        let keep: Vec<String> = ctx.config.rules.user_cache.keep.clone();
        let overlaps: Vec<_> = overlap_paths(ctx)
            .into_iter()
            .map(|(_, path)| path)
            .collect();
        let now = svc.now;
        // Prune at depth == 1 ONLY (children of the `.cache` root):
        // whitelisted keeps + hot entries. No gate.
        let prune: Arc<walker::PruneFn> =
            Arc::new(move |_path: &std::path::Path, info: &EntryInfo| {
                if overlaps.iter().any(|root| root == _path) {
                    return Prune::Drop(SkipKind::Overlap);
                }
                if info.depth != 1 {
                    return Prune::Keep;
                }
                let name = info.name.to_string_lossy();
                if keep.iter().any(|k| k == &*name) {
                    return Prune::Drop(SkipKind::Whitelist);
                }
                // Missing mtime treated as hot (conservative, mirrors the tmp
                // gate's honesty: never claim unreadable files reclaimable).
                match info.mtime {
                    Some(m) if now.duration_since(m).unwrap_or_default() >= HOT_WINDOW => {
                        Prune::Keep
                    }
                    _ => Prune::Drop(SkipKind::Hot),
                }
            });

        let root = ctx.home.join(".cache");
        let opts = WalkOptions {
            max_samples: 20,
            capture_candidates: svc.capture_candidates,
            cancel: svc.cancel.clone(),
            progress: svc.events.clone(),
            prune: Some(prune),
            gate: None,
            now,
            root_dev: None,
            pool: svc.pool.clone(),
        };
        let stats = walker::walk(&root, &opts)?;
        Ok(RuleScan {
            info: crate::scan::ScanInfo {
                bytes: stats.bytes,
                files: stats.files,
                dirs: stats.dirs,
                newest_age: stats
                    .newest
                    .map(|n| now.duration_since(n).unwrap_or_default()),
                samples: stats.samples,
                candidates: stats.candidates,
                note: None,
            },
            skipped: stats.skipped,
        })
    }
}

pub(crate) fn overlap_paths(ctx: &Ctx) -> Vec<(&'static str, std::path::PathBuf)> {
    let cache = ctx.home.join(".cache");
    let mut paths = Vec::new();
    if ctx.config.rules.thumbnails.enabled {
        paths.push(("thumbnails", cache.join("thumbnails")));
    }
    if ctx.config.rules.browser_cache.enabled {
        paths.extend(
            super::browser_cache::target_paths(ctx)
                .into_iter()
                .map(|path| ("browser-cache", path)),
        );
    }
    if ctx.config.rules.tool_caches.enabled {
        paths.extend(
            super::tool_caches::target_paths(ctx)
                .into_iter()
                .map(|path| ("tool-caches", path)),
        );
    }
    if ctx.config.rules.app_caches.enabled {
        paths.extend(
            super::app_caches::target_paths(ctx)
                .into_iter()
                .map(|path| ("app-caches", path)),
        );
    }
    if ctx.config.rules.xdg_state_junk.enabled {
        paths.extend(
            super::xdg_state::target_paths(ctx)
                .into_iter()
                .map(|path| ("xdg-state-junk", path)),
        );
    }
    if ctx.config.rules.log_rotate.enabled {
        paths.extend(
            super::log_rotate::target_paths(ctx)
                .into_iter()
                .map(|path| ("log-rotate", path)),
        );
    }
    paths
        .into_iter()
        .filter(|(_, path)| path != &cache && path.starts_with(&cache))
        .collect()
}

pub struct ThumbnailsRule;

impl Rule for ThumbnailsRule {
    fn id(&self) -> &'static str {
        "thumbnails"
    }
    fn title(&self) -> &'static str {
        "Thumbnail cache"
    }
    fn category(&self) -> &'static str {
        "thumbnails"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".cache"))
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.thumbnails.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".to_string(),
            };
        }
        if !ctx.home.join(".cache/thumbnails").is_dir() {
            return Detect::NotApplicable {
                // Truthful: the rule's ROOT is `~/.cache/thumbnails`, not
                // `~/.cache`, `~/.cache` may well exist (critic finding).
                reason: "~/.cache/thumbnails not found".to_string(),
            };
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        // Plain walk, no prune/gate.
        let root = ctx.home.join(".cache/thumbnails");
        let opts = WalkOptions {
            max_samples: 20,
            capture_candidates: svc.capture_candidates,
            cancel: svc.cancel.clone(),
            progress: svc.events.clone(),
            prune: None,
            gate: None,
            now: svc.now,
            root_dev: None,
            pool: svc.pool.clone(),
        };
        let stats = walker::walk(&root, &opts)?;
        Ok(RuleScan {
            info: crate::scan::ScanInfo {
                bytes: stats.bytes,
                files: stats.files,
                dirs: stats.dirs,
                newest_age: stats
                    .newest
                    .map(|n| svc.now.duration_since(n).unwrap_or_default()),
                samples: stats.samples,
                candidates: stats.candidates,
                note: None,
            },
            skipped: stats.skipped,
        })
    }
}
