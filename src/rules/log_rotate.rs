use super::{Detect, Rule, any_path_exists, scan_targets};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};
use std::path::{Path, PathBuf};
use std::time::Duration;

const ROTATED_AGE: Duration = Duration::from_secs(86_400);

pub struct LogRotateRule;

impl Rule for LogRotateRule {
    fn id(&self) -> &'static str {
        "log-rotate"
    }
    fn title(&self) -> &'static str {
        "Rotated logs"
    }
    fn category(&self) -> &'static str {
        "logs"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.var_log_root)
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.log_rotate.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".into(),
            };
        }
        if !any_path_exists(&target_paths(ctx)) {
            return Detect::NotApplicable {
                reason: "rotated logs not found".into(),
            };
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        scan_targets(target_paths(ctx), svc, None, Some(ROTATED_AGE))
    }
}

pub fn target_paths(ctx: &Ctx) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    add_matching_files(&ctx.var_log_root, &mut paths);
    add_journal_rotations(&ctx.var_log_root, &mut paths);
    collect_user_logs(&ctx.home.join(".local/state"), &mut paths);
    add_thunderbird_logs(ctx, &mut paths);
    paths
}

fn add_matching_files(root: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| !kind.is_dir())
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_rotated_name)
        {
            paths.push(path);
        }
    }
}

fn add_journal_rotations(var_log_root: &Path, paths: &mut Vec<PathBuf>) {
    let root = var_log_root.join("journal");
    for machine in super::child_dirs(&root) {
        if let Ok(entries) = std::fs::read_dir(machine) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "journal~")
                    && entry.file_type().is_ok_and(|kind| !kind.is_dir())
                {
                    paths.push(path);
                }
            }
        }
    }
}

fn collect_user_logs(root: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if entry.file_name() == "log" {
                add_matching_files(&path, paths);
            } else {
                collect_user_logs(&path, paths);
            }
        }
    }
}

fn add_thunderbird_logs(ctx: &Ctx, paths: &mut Vec<PathBuf>) {
    let root = ctx.home.join(".cache/thunderbird");
    for profile in super::child_dirs(&root) {
        let mail = profile.join("Mail");
        if let Ok(entries) = std::fs::read_dir(mail) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "log")
                    && entry.file_type().is_ok_and(|kind| !kind.is_dir())
                {
                    paths.push(path);
                }
            }
        }
    }
}

fn is_rotated_name(name: &str) -> bool {
    name.ends_with(".gz")
        || name.rsplit_once('.').is_some_and(|(_, suffix)| {
            !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
        })
}
