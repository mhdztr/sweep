//! The prunable directory walker. Spec: plan §2.7.
//!
//! Deviation D-2: directories contribute **0 bytes** (files + symlinks only;
//! symlinks are 0-byte files, never followed). `du` counts dir inode sizes;
//! du-comparison tolerances must cover `dirs × 4096`.

use crate::scan::ScanEvent;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkipKind {
    Protected,
    Hot,
    Whitelist,
    MountBound,
    KeptNewer,
}

#[derive(Debug, Clone)]
pub struct EntryInfo {
    pub name: OsString,
    /// Root = 0, direct children = 1.
    pub depth: usize,
    pub kind: EntryKind,
    /// lstat; None on stat failure.
    pub mtime: Option<SystemTime>,
    /// st_size; ALWAYS 0 for Symlink.
    pub len: u64,
    pub dev: Option<u64>,
}

impl EntryInfo {
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, EntryKind::Dir)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Socket,
    Fifo,
    CharDevice,
    Other,
}

// jwalk's file_type() is lstat-based: is_dir/is_symlink/is_socket/is_fifo/
// is_char_device reflect the link itself, never the target.
fn kind_of(ft: &std::fs::FileType) -> EntryKind {
    if ft.is_symlink() {
        EntryKind::Symlink
    } else if ft.is_dir() {
        EntryKind::Dir
    } else if ft.is_file() {
        EntryKind::File
    } else if ft.is_socket() {
        EntryKind::Socket
    } else if ft.is_fifo() {
        EntryKind::Fifo
    } else if ft.is_char_device() {
        EntryKind::CharDevice
    } else {
        EntryKind::Other
    }
}

/// Subtree skip, evaluated at listing time (the subtree is neither yielded
/// nor descended, see plan settled decision 1).
pub enum Prune {
    Keep,
    Drop(SkipKind),
}
pub type PruneFn = dyn Fn(&Path, &EntryInfo) -> Prune + Send + Sync;
/// Entry-level gate: Err = counted out (still descended if dir; gates are
/// documented as non-dir-only, rules enforce that).
pub type GateFn = dyn Fn(&Path, &EntryInfo) -> Result<(), SkipKind> + Send + Sync;

/// jwalk client_state payload: one lstat per entry, mtime/dev/len stashed in
/// the listing callback and reused by the consumer.
#[derive(Debug, Default, Clone, Copy)]
pub struct StatInfo {
    pub len: u64,
    /// 0 = unknown (pathological epoch-0 mtimes are treated as unknown;
    /// conservative for age gates).
    pub mtime_unix: i64,
    /// 0 = unknown (a real Linux st_dev is never 0).
    pub dev: u64,
}

#[derive(Clone)]
pub struct WalkOptions {
    pub max_samples: usize,
    /// Checked per directory (listing) AND per entry (consume).
    pub cancel: Arc<AtomicBool>,
    pub progress: Option<mpsc::Sender<ScanEvent>>,
    pub prune: Option<Arc<PruneFn>>,
    pub gate: Option<Arc<GateFn>>,
    /// Injected clock (consistent cutoffs, determinism).
    pub now: SystemTime,
    /// TEST SEAM (D-10): None ⇒ stat root; tests inject to exercise the
    /// mount-bound path without a real second device.
    pub root_dev: Option<u64>,
    /// Shared scan pool (dedicated, sized available_parallelism).
    pub pool: Arc<rayon::ThreadPool>,
}

impl WalkOptions {
    /// The pool MUST be supplied (one per scan run), there is no Default on
    /// purpose: defaulting would silently build a second pool.
    pub fn new(pool: Arc<rayon::ThreadPool>) -> Self {
        WalkOptions {
            max_samples: 20,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: None,
            prune: None,
            gate: None,
            now: SystemTime::now(),
            root_dev: None,
            pool,
        }
    }
}

#[derive(Debug, Default)]
pub struct WalkStats {
    pub bytes: u64,
    pub files: u64,
    pub dirs: u64,
    /// Newest COUNTED entry (files, symlinks AND dirs).
    pub newest: Option<SystemTime>,
    /// First ≤ max_samples counted paths, walk order (sorted). The root
    /// itself is counted (dirs) but never sampled, samples preview
    /// reclaimable content.
    pub samples: Vec<PathBuf>,
    pub skipped: BTreeMap<SkipKind, u64>,
    /// Per-entry stat/read failures (entry skipped, walk continues).
    pub errors: u64,
    /// Cancellation observed.
    pub partial: bool,
}

impl WalkStats {
    /// Merge another root's stats into this one (tmp = /tmp + /var/tmp).
    pub fn merge(&mut self, other: WalkStats) {
        self.bytes += other.bytes;
        self.files += other.files;
        self.dirs += other.dirs;
        self.errors += other.errors;
        self.partial |= other.partial;
        self.newest = match (self.newest, other.newest) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, b) => b,
        };
        for s in other.samples {
            if self.samples.len() < 20 {
                self.samples.push(s);
            }
        }
        for (k, v) in other.skipped {
            *self.skipped.entry(k).or_insert(0) += v;
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("{0}: not found")]
    Missing(PathBuf),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("directory walk busy (pool saturated): {0}")]
    Busy(String),
}

/// Per-kind skip counters shared between the listing callback (worker
/// threads) and the consumer.
#[derive(Default)]
struct SkipCounters {
    protected: AtomicU64,
    hot: AtomicU64,
    whitelist: AtomicU64,
    mount_bound: AtomicU64,
    kept_newer: AtomicU64,
}

impl SkipCounters {
    fn bump(&self, kind: SkipKind) {
        let c = match kind {
            SkipKind::Protected => &self.protected,
            SkipKind::Hot => &self.hot,
            SkipKind::Whitelist => &self.whitelist,
            SkipKind::MountBound => &self.mount_bound,
            SkipKind::KeptNewer => &self.kept_newer,
        };
        c.fetch_add(1, Ordering::Relaxed);
    }

    fn to_map(&self) -> BTreeMap<SkipKind, u64> {
        let mut m = BTreeMap::new();
        for (kind, v) in [
            (SkipKind::Protected, self.protected.load(Ordering::Relaxed)),
            (SkipKind::Hot, self.hot.load(Ordering::Relaxed)),
            (SkipKind::Whitelist, self.whitelist.load(Ordering::Relaxed)),
            (
                SkipKind::MountBound,
                self.mount_bound.load(Ordering::Relaxed),
            ),
            (SkipKind::KeptNewer, self.kept_newer.load(Ordering::Relaxed)),
        ] {
            if v > 0 {
                m.insert(kind, v);
            }
        }
        m
    }
}

/// Read-only walk of ONE root. Mutates nothing.
///
/// Depth semantics: the root is counted unconditionally (`dirs += 1`, 0
/// bytes), prune/mount/gate never apply to it. Newest includes the root's
/// mtime (it is a counted entry).
pub fn walk(root: &Path, opts: &WalkOptions) -> Result<WalkStats, ScanError> {
    // Root check: lstat; a root symlink is followed once for the root itself
    // only (matches jwalk's root-symlink special case).
    let lmeta = match std::fs::symlink_metadata(root) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ScanError::Missing(root.to_path_buf()));
        }
        Err(e) => {
            return Err(ScanError::Io {
                path: root.to_path_buf(),
                source: e,
            });
        }
    };
    let is_dir = if lmeta.file_type().is_symlink() {
        std::fs::metadata(root).is_ok_and(|m| m.is_dir())
    } else {
        lmeta.is_dir()
    };
    if !is_dir {
        return Err(ScanError::Io {
            path: root.to_path_buf(),
            source: std::io::Error::other("not a directory"),
        });
    }
    let root_dev = opts.root_dev.unwrap_or(lmeta.dev());

    let cancel_listing = opts.cancel.clone();
    let prune = opts.prune.clone();
    let stat_errors = Arc::new(AtomicU64::new(0));
    let stat_errors_cb = stat_errors.clone();
    let skipped = Arc::new(SkipCounters::default());
    let skipped_cb = skipped.clone();

    let iter = jwalk::WalkDirGeneric::<((), Option<StatInfo>)>::new(root)
        // CRITICAL: jwalk's default is true; dotfiles in /tmp/.cache must
        // count.
        .skip_hidden(false)
        // NEVER follow; symlinks counted as 0-byte files, never descended.
        .follow_links(false)
        .process_read_dir(move |_depth, _path, _state, children| {
            // Cancellation checked per directory: stop listing children.
            if cancel_listing.load(Ordering::Relaxed) {
                children.clear();
                return;
            }
            // Deterministic order (also makes samples order deterministic);
            // do NOT rely on a jwalk builder sort(); sort here.
            children.sort_by(|a, b| match (a, b) {
                (Ok(x), Ok(y)) => x.file_name.cmp(&y.file_name),
                (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            });
            children.retain_mut(|child| {
                let Ok(entry) = child else {
                    return true; // Err entries kept; counted as errors at consume
                };
                let depth = entry.depth; // children of root ⇒ 1
                // LOAD-BEARING depth check: process_read_dir also runs once
                // at into_iter() time over the root entry (depth 0); prune
                // and mount checks must never remove the root (D-10 seam:
                // an injected root_dev would otherwise prune it).
                let kind = kind_of(&entry.file_type());
                let (mtime, dev, len) = match std::fs::symlink_metadata(entry.path()) {
                    Ok(m) => (
                        m.modified().ok(),
                        Some(m.dev()),
                        if kind == EntryKind::Symlink { 0 } else { m.len() },
                    ),
                    Err(_) => {
                        stat_errors_cb.fetch_add(1, Ordering::Relaxed);
                        if depth > 0 {
                            // **Deviation from the plan's literal snippet**
                            // (which returned `(None, None, 0)` and let the
                            // consumer count a fabricated 0-byte FILE): a
                            // non-root entry we cannot lstat (EACCES on a
                            // list-only parent, TOCTOU deletion) has no
                            // measurable size/age/mtime, so a later `clean`
                            // could not verify it either, drop it instead of
                            // inflating `files` (honesty: counts reflect what
                            // a clean could reclaim). Still counted once in
                            // `stats.errors` above.
                            return false;
                        }
                        // Root is exempt: walk() lstat'ed it before iterating;
                        // losing it here would break depth-0 counting.
                        (None, None, 0) // TOCTOU/perm: root only
                    }
                };
                let info = EntryInfo {
                    name: entry.file_name.clone(),
                    depth,
                    kind,
                    mtime,
                    len,
                    dev,
                };
                if depth == 0 {
                    // Root entry: count it (consumer), never prune it.
                    entry.client_state = Some(stat_info(len, mtime, dev));
                    return true;
                }
                // Mount-bound: only for real dirs; dev != root_dev ⇒ subtree
                // never descended, never counted.
                if kind == EntryKind::Dir && dev.is_some_and(|d| d != root_dev) {
                    skipped_cb.bump(SkipKind::MountBound);
                    return false;
                }
                if let Some(prune) = &prune {
                    match prune(&entry.path(), &info) {
                        Prune::Drop(k) => {
                            skipped_cb.bump(k);
                            return false;
                        }
                        Prune::Keep => {}
                    }
                }
                // One lstat per entry total, mtime/dev/len reused by the
                // consumer via client_state.
                entry.client_state = Some(stat_info(len, mtime, dev));
                true
            });
        })
        .parallelism(jwalk::Parallelism::RayonExistingPool {
            pool: opts.pool.clone(),
            // Hang guard: consumers run outside the pool, so saturation here
            // is pathological, degrade the rule, never hang.
            busy_timeout: Some(Duration::from_secs(5)),
        });

    let mut iter = iter.into_iter();
    let mut stats = WalkStats::default();
    let mut counted: u64 = 0;

    loop {
        // Cancel checked at the TOP of the loop (before pulling the next
        // entry) so a pre-set cancel yields partial=true with zero counters
        // including before the root is consumed.
        if opts.cancel.load(Ordering::Relaxed) {
            stats.partial = true;
            break;
        }
        let Some(result) = iter.next() else { break };
        let mut entry = match result {
            Ok(e) => e,
            Err(e) if e.is_busy() => {
                return Err(ScanError::Busy(e.to_string()));
            }
            Err(_) => {
                stats.errors += 1;
                continue;
            }
        };
        // A directory listing failure (EACCES/TOCTOU) attaches to the dir's
        // own Ok entry as `read_children_error`, count it as a per-entry
        // error (the walk continues).
        if entry.read_children_error.is_some() {
            stats.errors += 1;
        }

        // Rebuild EntryInfo: client_state from the listing callback, or fall
        // back to metadata() when absent.
        let info = match entry.client_state.take() {
            Some(si) => EntryInfo {
                name: entry.file_name.clone(),
                depth: entry.depth,
                kind: kind_of(&entry.file_type()),
                mtime: (si.mtime_unix != 0)
                    .then(|| SystemTime::UNIX_EPOCH + Duration::from_secs(si.mtime_unix as u64)),
                len: si.len,
                dev: (si.dev != 0).then_some(si.dev),
            },
            None => match std::fs::symlink_metadata(entry.path()) {
                Ok(m) => EntryInfo {
                    name: entry.file_name.clone(),
                    depth: entry.depth,
                    kind: kind_of(&m.file_type()),
                    mtime: m.modified().ok(),
                    len: if m.file_type().is_symlink() {
                        0
                    } else {
                        m.len()
                    },
                    dev: Some(m.dev()),
                },
                Err(_) => {
                    stats.errors += 1;
                    continue;
                }
            },
        };

        if info.depth > 0 {
            // Gate (newer-than-cutoff logic lives in the rule's closure, not
            // here). A gated dir would still be descended if one ever
            // occurred, gates are documented as non-dir-only.
            if let Some(gate) = &opts.gate
                && let Err(kind) = gate(&entry.path(), &info)
            {
                skipped.bump(kind);
                continue;
            }
        }

        // Counted.
        counted += 1;
        match info.kind {
            EntryKind::Dir => {
                stats.dirs += 1; // 0 bytes (D-2)
            }
            _ => {
                stats.files += 1;
                stats.bytes += info.len; // symlinks: len already forced 0
            }
        }
        if let Some(m) = info.mtime {
            stats.newest = Some(match stats.newest {
                Some(cur) => cur.max(m),
                None => m,
            });
        }
        if info.depth > 0 && stats.samples.len() < opts.max_samples {
            stats.samples.push(entry.path());
        }
        if counted.is_multiple_of(256)
            && let Some(tx) = &opts.progress
        {
            let _ = tx.send(ScanEvent::Walk {
                dirs: stats.dirs,
                bytes: stats.bytes,
            });
        }
    }

    stats.errors += stat_errors.load(Ordering::Relaxed);
    stats.skipped = skipped.to_map();
    Ok(stats)
}

fn stat_info(len: u64, mtime: Option<SystemTime>, dev: Option<u64>) -> StatInfo {
    StatInfo {
        len,
        mtime_unix: mtime
            .map(|t| {
                t.duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0)
            })
            .unwrap_or(0),
        dev: dev.unwrap_or(0),
    }
}
