use super::{Detect, Rule, Tier, any_path_exists, scan_targets};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

pub struct XdgStateJunkRule;

impl Rule for XdgStateJunkRule {
    fn id(&self) -> &'static str {
        "xdg-state-junk"
    }
    fn title(&self) -> &'static str {
        "Crash dumps & state junk"
    }
    fn category(&self) -> &'static str {
        "state/junk"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".local/state"))
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.xdg_state_junk.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".into(),
            };
        }
        if !any_path_exists(&target_paths(ctx)) {
            return Detect::NotApplicable {
                reason: "crash reports not found".into(),
            };
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        scan_targets(target_paths(ctx), svc, None, None)
    }
}

pub fn target_paths(ctx: &Ctx) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    for app in super::child_dirs(&ctx.home.join(".cache")) {
        let crashpad = app.join("Crashpad");
        for path in super::child_dirs(&crashpad) {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("Reports") || name.starts_with("crashpad_metrics")
                })
            {
                targets.push(path);
            }
        }
    }
    for app in super::child_dirs(&ctx.home.join(".config")) {
        targets.push(app.join("crashes"));
    }
    for app in super::child_dirs(&ctx.home.join(".local/share")) {
        targets.push(app.join("Crashpad"));
    }
    targets
}

pub struct XdgStateCoredumpsRule;

impl Rule for XdgStateCoredumpsRule {
    fn id(&self) -> &'static str {
        "xdg-state-coredumps"
    }
    fn title(&self) -> &'static str {
        "User coredumps"
    }
    fn category(&self) -> &'static str {
        "coredumps (default off)"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".local/state/libuuid/coredump"))
    }
    fn tier(&self) -> Tier {
        Tier::Caution
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.xdg_state_junk.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".into(),
            };
        }
        match target_paths_for_coredumps(ctx) {
            Ok(paths) if paths.is_empty() => {
                return Detect::NotApplicable {
                    reason: "user coredumps not found".into(),
                };
            }
            Ok(_) => {}
            Err(error) => {
                return Detect::NotApplicable {
                    reason: format!("cannot inspect user coredumps: {error}"),
                };
            }
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        scan_targets(
            target_paths_for_coredumps(ctx).map_err(|source| ScanError::Io {
                path: ctx.home.join(".local/state/libuuid/coredump"),
                source,
            })?,
            svc,
            None,
            Some(Duration::from_secs(86_400)),
        )
    }
}

fn target_paths_for_coredumps(ctx: &Ctx) -> io::Result<Vec<PathBuf>> {
    let root = ctx.home.join(".local/state/libuuid/coredump");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("core."))
        {
            paths.push(path);
        }
    }
    Ok(paths)
}
