//! Rule trait + tier model + the M1 rule registry. Spec: plan §2.9.

use crate::ctx::Ctx;
use crate::scan::walker::{self, EntryInfo, EntryKind, Prune, SkipKind, WalkOptions};
use crate::scan::{RuleScan, ScanError, ScanInfo, ScanService};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Safe,
    Caution,
    Destructive,
}

impl Tier {
    /// JSON tier string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Safe => "safe",
            Tier::Caution => "caution",
            Tier::Destructive => "destructive",
        }
    }

    /// Table tier glyph (CLI-UX §8: `+ [x] [ ] ! #`). ASCII only (D13).
    pub fn glyph(&self) -> &'static str {
        match self {
            Tier::Safe => "+",
            Tier::Caution => "!",
            Tier::Destructive => "#",
        }
    }

    /// Table tier label (mock table copy).
    pub fn label(&self) -> &'static str {
        match self {
            Tier::Safe => "safe",
            Tier::Caution => "caut.",
            Tier::Destructive => "dang.",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Detect {
    Ready,
    NotApplicable {
        /// "~/.cache not found" (user-cache), "~/.cache/thumbnails not
        /// found" (thumbnails), "disabled in config", "/tmp and /var/tmp
        /// not found", "trash not found". Reason names the RULE'S ROOT (not
        /// an ancestor of it), copy-locked by tests.
        reason: String,
    },
}

pub trait Rule: Send + Sync {
    /// "tmp"
    fn id(&self) -> &'static str;
    /// Catalog title, JSON `title`: "Temp files".
    fn title(&self) -> &'static str;
    /// Table label (D-5): "tmp & var-tmp".
    fn category(&self) -> &'static str;
    /// display_path: "/tmp", "~/.cache", "~/.local/share/Trash".
    fn location(&self, ctx: &Ctx) -> String;
    fn tier(&self) -> Tier {
        Tier::Safe
    }
    fn needs_sudo(&self) -> bool {
        false
    }
    fn detect(&self, ctx: &Ctx) -> Detect;
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError>;
    fn report_only(&self) -> bool {
        false
    }
    fn confirm_batch_size(&self) -> Option<usize> {
        None
    }
    fn volume_gate(&self) -> bool {
        false
    }
}

/// Registry order is the output order as per catalog.
pub fn registry() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(tmp::TmpRule),
        Box::new(cache::UserCacheRule),
        Box::new(cache::ThumbnailsRule),
        Box::new(trash::TrashRule),
        Box::new(journal_user::JournalUserRule),
        Box::new(browser_cache::BrowserCacheRule),
        Box::new(editor_cache::EditorCacheRule),
        Box::new(tool_caches::ToolCachesRule),
        Box::new(app_caches::AppCachesRule),
        Box::new(log_rotate::LogRotateRule),
        Box::new(xdg_state::XdgStateJunkRule),
        Box::new(pkg::PacmanCacheRule),
        Box::new(pkg::JournalSystemRule),
        Box::new(pkg::AurBuildsRule),
        Box::new(pkg::OrphansRule),
        Box::new(pkg::FlatpakUnusedRule),
        Box::new(containers::DockerRule),
        Box::new(containers::PodmanRule),
        Box::new(dev::DevArtifactsRule),
        Box::new(steam::SteamShaderRule),
        Box::new(snapshots::SnapshotsReportRule),
        Box::new(downloads::DownloadsRule),
        Box::new(cache_deep::CacheDeepRule),
        Box::new(tmp_full::TmpFullRule),
        Box::new(pkgcache_zero::PkgcacheZeroRule),
        Box::new(purge_logs::PurgeLogsRule),
    ]
}

pub fn scan_registry() -> Vec<Box<dyn Rule>> {
    let mut rules: Vec<_> = registry()
        .into_iter()
        .filter(|rule| rule.tier() != Tier::Destructive && !rule.report_only())
        .collect();
    rules.push(Box::new(snapshots::SnapshotsReportRule));
    rules.push(Box::new(xdg_state::XdgStateCoredumpsRule));
    rules
}

/// "~/…" when under home, else absolute. Used by Rule::location + table.
pub fn display_path(home: &Path, p: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

pub(crate) fn any_path_exists(paths: &[std::path::PathBuf]) -> bool {
    paths
        .iter()
        .any(|path| std::fs::symlink_metadata(path).is_ok())
}

pub(crate) fn child_dirs(path: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .collect()
}

pub(crate) fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| {
            let candidate = directory.join(command);
            std::fs::metadata(candidate).is_ok_and(|metadata| {
                metadata.is_file() && {
                    use std::os::unix::fs::PermissionsExt as _;
                    metadata.permissions().mode() & 0o111 != 0
                }
            })
        })
    })
}

pub(crate) fn command_output(
    command: &str,
    args: &[&str],
) -> std::io::Result<std::process::Output> {
    std::process::Command::new(command).args(args).output()
}

pub(crate) fn command_success_timeout(
    command: &str,
    args: &[&str],
    timeout: Duration,
) -> std::io::Result<bool> {
    use std::process::Stdio;
    let mut child = std::process::Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success());
        }
        if started.elapsed() >= timeout {
            child.kill()?;
            let _ = child.wait()?;
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn parse_human_size(value: &str) -> Option<u64> {
    let value = value.trim().trim_end_matches(',');
    let split = value.find(|character: char| !character.is_ascii_digit() && character != '.')?;
    let number: f64 = value[..split].parse().ok()?;
    let unit = value[split..]
        .trim()
        .trim_end_matches("B")
        .trim_end_matches('i')
        .to_ascii_uppercase();
    let power = match unit.as_str() {
        "" => 0,
        "K" => 1,
        "M" => 2,
        "G" => 3,
        "T" => 4,
        _ => return None,
    };
    let bytes = number * 1024_f64.powi(power);
    (bytes.is_finite() && bytes >= 0.0 && bytes <= u64::MAX as f64).then_some(bytes.round() as u64)
}

pub(crate) fn scan_targets(
    mut roots: Vec<std::path::PathBuf>,
    svc: &ScanService,
    hot_window: Option<Duration>,
    minimum_age: Option<Duration>,
) -> Result<RuleScan, ScanError> {
    roots.sort();
    roots.dedup();
    let now = svc.now;
    let prune: Option<Arc<walker::PruneFn>> = hot_window.map(|window| {
        Arc::new(move |_path: &Path, info: &EntryInfo| {
            if !info.is_dir() && is_recent(info.mtime, now, window) {
                Prune::Drop(SkipKind::Hot)
            } else {
                Prune::Keep
            }
        }) as Arc<walker::PruneFn>
    });
    let gate: Option<Arc<walker::GateFn>> = minimum_age.map(|age| {
        Arc::new(move |_path: &Path, info: &EntryInfo| {
            if info.is_dir() || is_old_enough(info.mtime, now, age) {
                Ok(())
            } else {
                Err(SkipKind::KeptNewer)
            }
        }) as Arc<walker::GateFn>
    });

    let mut total = walker::WalkStats::default();
    let mut errors = Vec::new();
    for root in roots {
        if has_symlink_ancestor(&root) {
            bump(&mut total.skipped, SkipKind::Protected);
            continue;
        }
        let metadata = match std::fs::symlink_metadata(&root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                errors.push(format!("{}: {error}", root.display()));
                continue;
            }
        };
        let kind = entry_kind(&metadata.file_type());
        if kind == EntryKind::Dir {
            let opts = WalkOptions {
                max_samples: 20,
                capture_candidates: svc.capture_candidates,
                cancel: svc.cancel.clone(),
                progress: svc.events.clone(),
                prune: prune.clone(),
                gate: gate.clone(),
                now,
                root_dev: None,
                pool: svc.pool.clone(),
            };
            match walker::walk(&root, &opts) {
                Ok(stats) => total.merge(stats),
                Err(ScanError::Missing(_)) => {}
                Err(error) => errors.push(error.to_string()),
            }
            continue;
        }
        if !matches!(kind, EntryKind::File | EntryKind::Symlink) {
            continue;
        }
        let modified = metadata.modified().ok();
        if hot_window.is_some_and(|window| is_recent(modified, now, window)) {
            bump(&mut total.skipped, SkipKind::Hot);
            continue;
        }
        if minimum_age.is_some_and(|age| !is_old_enough(modified, now, age)) {
            bump(&mut total.skipped, SkipKind::KeptNewer);
            continue;
        }
        total.files += 1;
        if kind == EntryKind::File {
            total.bytes += metadata.len();
        }
        total.newest = modified.or(total.newest);
        if total.samples.len() < 20 {
            total.samples.push(root.clone());
        }
        if svc.capture_candidates {
            total.candidates.push(root);
        }
    }

    fn has_symlink_ancestor(path: &Path) -> bool {
        let components: Vec<_> = path.components().collect();
        let mut current = std::path::PathBuf::new();
        for (index, component) in components.iter().enumerate() {
            if index + 1 == components.len() {
                break;
            }
            current.push(component.as_os_str());
            if std::fs::symlink_metadata(&current)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                return true;
            }
        }
        false
    }

    if total.files == 0 && total.dirs == 0 && !errors.is_empty() {
        return Err(ScanError::Io {
            path: std::path::PathBuf::from("rule targets"),
            source: std::io::Error::other(errors.join("; ")),
        });
    }

    Ok(RuleScan {
        info: ScanInfo {
            bytes: total.bytes,
            files: total.files,
            dirs: total.dirs,
            newest_age: total
                .newest
                .map(|modified| now.duration_since(modified).unwrap_or_default()),
            samples: total.samples,
            candidates: total.candidates,
            note: None,
        },
        skipped: total.skipped,
    })
}

fn entry_kind(file_type: &std::fs::FileType) -> EntryKind {
    if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Dir
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

fn is_recent(mtime: Option<SystemTime>, now: SystemTime, window: Duration) -> bool {
    mtime.is_none_or(|mtime| now.duration_since(mtime).unwrap_or_default() < window)
}

fn is_old_enough(mtime: Option<SystemTime>, now: SystemTime, age: Duration) -> bool {
    mtime.is_some_and(|mtime| now.duration_since(mtime).unwrap_or_default() >= age)
}

fn bump(skipped: &mut BTreeMap<SkipKind, u64>, kind: SkipKind) {
    *skipped.entry(kind).or_default() += 1;
}

pub mod app_caches;
pub mod browser_cache;
pub mod cache;
pub mod cache_deep;
pub mod containers;
pub mod dev;
pub mod downloads;
pub mod editor_cache;
pub mod journal_user;
pub mod log_rotate;
pub mod pkg;
pub mod pkgcache_zero;
pub mod purge_logs;
pub mod snapshots;
pub mod steam;
pub mod tmp;
pub mod tmp_full;
pub mod tool_caches;
pub mod trash;
pub mod xdg_state;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_order_is_tmp_usercache_thumbnails_trash() {
        let ids: Vec<&str> = registry().iter().map(|r| r.id()).collect();
        assert_eq!(ids[0..4], vec!["tmp", "user-cache", "thumbnails", "trash"]);
    }

    #[test]
    fn display_path_under_home() {
        let home = Path::new("/home/u");
        assert_eq!(display_path(home, Path::new("/home/u/.cache")), "~/.cache");
        assert_eq!(display_path(home, Path::new("/tmp")), "/tmp");
    }

    #[test]
    fn tier_strings() {
        assert_eq!(Tier::Safe.as_str(), "safe");
        assert_eq!(Tier::Caution.as_str(), "caution");
        assert_eq!(Tier::Destructive.as_str(), "destructive");
        assert_eq!(Tier::Safe.glyph(), "+");
        assert_eq!(Tier::Caution.glyph(), "!");
        assert_eq!(Tier::Destructive.glyph(), "#");
        assert_eq!(Tier::Caution.label(), "caut.");
    }
}
