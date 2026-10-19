//! Rule `trash` — XDG trash (`~/.local/share/Trash/{files,info}`).
//! New file (D-8): added to the ARCHITECTURE §1 tree in the same commit.
//! Spec: plan §2.12.

use super::{Detect, Rule};
use crate::ctx::Ctx;
use crate::scan::walker::{self, WalkOptions};
use crate::scan::{RuleScan, ScanError, ScanService};

pub struct TrashRule;

impl Rule for TrashRule {
    fn id(&self) -> &'static str {
        "trash"
    }
    fn title(&self) -> &'static str {
        "XDG trash"
    }
    fn category(&self) -> &'static str {
        "trash"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".local/share/Trash"))
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.trash.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".to_string(),
            };
        }
        let trash = ctx.home.join(".local/share/Trash");
        if !trash.join("files").is_dir() && !trash.join("info").is_dir() {
            return Detect::NotApplicable {
                reason: "trash not found".to_string(),
            };
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        let trash = ctx.home.join(".local/share/Trash");
        let mut total = walker::WalkStats::default();
        let mut root_errors: Vec<String> = Vec::new();
        // Both roots walked + merged; only existing roots walked. No
        // prune/gate (trash entries were already user-deleted).
        for root in [trash.join("files"), trash.join("info")] {
            if !root.is_dir() {
                continue;
            }
            let opts = WalkOptions {
                max_samples: 20,
                cancel: svc.cancel.clone(),
                progress: svc.events.clone(),
                prune: None,
                gate: None,
                now: svc.now,
                root_dev: None,
                pool: svc.pool.clone(),
            };
            match walker::walk(&root, &opts) {
                Ok(stats) => total.merge(stats),
                Err(e) => root_errors.push(e.to_string()),
            }
        }
        if total.dirs == 0 && !root_errors.is_empty() {
            return Err(ScanError::Io {
                path: trash,
                source: std::io::Error::other(root_errors.join("; ")),
            });
        }
        let now = svc.now;
        Ok(RuleScan {
            info: crate::scan::ScanInfo {
                bytes: total.bytes,
                files: total.files,
                dirs: total.dirs,
                newest_age: total.newest.map(|n| now.duration_since(n).unwrap_or_default()),
                samples: total.samples,
                note: None,
            },
            skipped: total.skipped,
        })
    }
}
