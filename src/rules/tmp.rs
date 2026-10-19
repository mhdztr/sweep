//! Rule `tmp` — `/tmp` + `/var/tmp`. Spec: plan §2.10.

use super::{Detect, Rule};
use crate::ctx::Ctx;
use crate::scan::walker::{self, EntryInfo, EntryKind, Prune, SkipKind, WalkOptions};
use crate::scan::{RuleScan, ScanError, ScanService};
use std::ffi::OsStr;
use std::sync::Arc;
use std::time::Duration;

/// Single source of truth for never-touch entries inside tmp roots (shared
/// with future tmp-full, SAFETY §3). Applies at any depth, both roots.
pub const PROTECTED_DIR_PREFIXES: &[&str] = &["systemd-private-", "snap.mount", "dbus-"];
pub const PROTECTED_EXACT: &[&str] = &["X11-unix", ".XIM-unix", ".ICE-unix"];
/// Compiled once via glob::Pattern, matched on file_name only.
pub const PROTECTED_GLOBS: &[&str] = &[".X*-lock"];

pub fn is_protected(name: &OsStr, kind: EntryKind) -> bool {
    // Special file kinds (sockets / FIFOs / device nodes) are never safe to
    // delete even when the name looks ordinary.
    if matches!(
        kind,
        EntryKind::Socket | EntryKind::Fifo | EntryKind::CharDevice
    ) {
        return true;
    }
    let Some(name) = name.to_str() else {
        return false; // non-UTF8 names: only the kind check above applies
    };
    if PROTECTED_DIR_PREFIXES.iter().any(|p| name.starts_with(p)) {
        return true;
    }
    if PROTECTED_EXACT.contains(&name) {
        return true;
    }
    PROTECTED_GLOBS.iter().any(|g| {
        glob::Pattern::new(g).is_ok_and(|p| p.matches(name))
    })
}

pub struct TmpRule;

impl Rule for TmpRule {
    fn id(&self) -> &'static str {
        "tmp"
    }
    fn title(&self) -> &'static str {
        "Temp files"
    }
    fn category(&self) -> &'static str {
        "tmp & var-tmp"
    }
    fn location(&self, ctx: &Ctx) -> String {
        ctx.tmp_roots
            .iter()
            .map(|r| r.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.tmp.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".to_string(),
            };
        }
        if !ctx.tmp_roots.iter().any(|r| r.is_dir()) {
            return Detect::NotApplicable {
                reason: "/tmp and /var/tmp not found".to_string(),
            };
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        let prune: Arc<walker::PruneFn> = Arc::new(|_path: &std::path::Path, info: &EntryInfo| {
            if is_protected(&info.name, info.kind) {
                Prune::Drop(SkipKind::Protected)
            } else {
                Prune::Keep
            }
        });
        // D-9: age gate applies to non-dir entries only; dirs always counted
        // (0 bytes) and descended. keep_days=0 ⇒ gate off. Missing mtime is
        // treated as age 0 ⇒ excluded (conservative: never claim fresh files
        // are reclaimable).
        let keep_days = ctx.keep_days();
        let now = svc.now;
        let gate: Arc<walker::GateFn> = Arc::new(move |_path: &std::path::Path, info: &EntryInfo| {
            if keep_days == 0 || info.is_dir() {
                return Ok(());
            }
            let cutoff = now.checked_sub(Duration::from_secs(keep_days as u64 * 86_400));
            match (info.mtime, cutoff) {
                // age >= exactly N days ⇒ counted (D-9: mtime <= cutoff).
                (Some(m), Some(cut)) if m <= cut => Ok(()),
                _ => Err(SkipKind::KeptNewer),
            }
        });

        let mut total = walker::WalkStats::default();
        let mut root_errors: Vec<String> = Vec::new();
        for root in &ctx.tmp_roots {
            if !root.is_dir() {
                continue;
            }
            let opts = WalkOptions {
                max_samples: 20,
                cancel: svc.cancel.clone(),
                progress: svc.events.clone(),
                prune: Some(prune.clone()),
                gate: Some(gate.clone()),
                now,
                root_dev: None,
                pool: svc.pool.clone(),
            };
            match walker::walk(root, &opts) {
                Ok(stats) => total.merge(stats),
                Err(e) => root_errors.push(e.to_string()),
            }
        }
        // Root-level failure degrades the whole rule only when NO root
        // produced data (SAFETY §6: degrades, never vanishes).
        if total.dirs == 0 && !root_errors.is_empty() {
            return Err(ScanError::Io {
                path: ctx.tmp_roots[0].clone(),
                source: std::io::Error::other(root_errors.join("; ")),
            });
        }

        Ok(RuleScan {
            info: crate::scan::ScanInfo {
                bytes: total.bytes,
                files: total.files,
                dirs: total.dirs,
                newest_age: total.newest.map(|n| now.duration_since(n).unwrap_or_default()),
                samples: total.samples,
                note: None, // set by scan::run (needs skipped[KeptNewer] + keep_days)
            },
            skipped: total.skipped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_names_match_prefix_exact_glob_and_kind() {
        let f = EntryKind::File;
        assert!(is_protected(OsStr::new("systemd-private-abc"), f));
        assert!(is_protected(OsStr::new("snap.mount"), f));
        assert!(is_protected(OsStr::new("dbus-123"), f));
        assert!(is_protected(OsStr::new("X11-unix"), f));
        assert!(is_protected(OsStr::new(".XIM-unix"), f));
        assert!(is_protected(OsStr::new(".ICE-unix"), f));
        assert!(is_protected(OsStr::new(".X123-lock"), f));
        assert!(is_protected(OsStr::new("ordinary-name"), EntryKind::Socket));
        assert!(is_protected(OsStr::new("ordinary-name"), EntryKind::Fifo));
        assert!(is_protected(OsStr::new("ordinary-name"), EntryKind::CharDevice));
        assert!(!is_protected(OsStr::new("ordinary-name"), f));
        assert!(!is_protected(OsStr::new("X11-unix.lockfile"), f));
        assert!(!is_protected(OsStr::new("dbusx"), f));
        assert!(is_protected(OsStr::new("dbus-tool"), f)); // prefix match is broad by spec
    }
}
