//! Scan pipeline: walker, rule orchestration, aggregation. Spec: plan §2.8.

pub mod fsinfo;
pub mod walker;

pub use walker::ScanError;

use crate::ctx::Ctx;
use crate::rules::Tier;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, SystemTime};
use walker::SkipKind;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanInfo {
    pub bytes: u64,
    pub files: u64,
    pub dirs: u64,
    /// now − newest counted entry.
    pub newest_age: Option<Duration>,
    /// ≤ 20.
    pub samples: Vec<PathBuf>,
    /// e.g. "1 newer than keep-days kept" | "(excl. thumbnails)" — human
    /// table only (parenthesization happens in the renderer).
    pub note: Option<String>,
}

impl ScanInfo {
    /// Sum bytes/files/dirs, max newest (min age), concat samples capped at
    /// 20, prefer Some(note).
    pub fn merge(&mut self, other: ScanInfo) {
        self.bytes += other.bytes;
        self.files += other.files;
        self.dirs += other.dirs;
        self.newest_age = match (self.newest_age, other.newest_age) {
            (Some(a), Some(b)) => Some(a.min(b)), // younger = smaller age
            (Some(a), None) => Some(a),
            (None, b) => b,
        };
        for s in other.samples {
            if self.samples.len() < 20 {
                self.samples.push(s);
            }
        }
        if self.note.is_none() {
            self.note = other.note;
        }
    }
}

#[derive(Debug)]
pub struct RuleScan {
    pub info: ScanInfo,
    pub skipped: BTreeMap<SkipKind, u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanEvent {
    RuleDone {
        id: &'static str,
        done: usize,
        total: usize,
    },
    Walk {
        dirs: u64,
        bytes: u64,
    },
}

/// Shared services handed to every rule — lives for the duration of `run()`.
pub struct ScanService {
    pub cancel: Arc<AtomicBool>,
    pub events: Option<mpsc::Sender<ScanEvent>>,
    /// num_threads = available_parallelism(), min 1.
    pub pool: Arc<rayon::ThreadPool>,
    /// Captured ONCE per run (consistent age cutoffs).
    pub now: SystemTime,
}

#[derive(Debug)]
pub struct RuleResult {
    pub id: &'static str,
    pub title: &'static str,
    /// Table label (D-5) — differs from JSON `title`.
    pub category: &'static str,
    pub tier: Tier,
    /// detect()==Ready AND scan succeeded.
    pub applied: bool,
    /// detect reason OR root-level scan error text.
    pub reason: Option<String>,
    pub needs_sudo: bool,
    /// display_path(home, primary root).
    pub location: String,
    /// zeros when !applied.
    pub info: ScanInfo,
    /// ["thumbnails"] for user-cache after subtraction.
    pub net_of: Vec<&'static str>,
    /// for -v output only.
    pub skipped: BTreeMap<SkipKind, u64>,
}

pub struct Totals {
    pub reclaimable_bytes: u64,
    pub regular_bytes: u64,
    pub scan_ms: u64,
}

pub struct ScanOutput {
    pub results: Vec<RuleResult>,
    pub totals: Totals,
}

/// Run the full scan: detect → parallel rule walks (scoped threads outside
/// the shared pool) → registry-ordered assembly → totals.
pub fn run(
    ctx: &Ctx,
    cancel: Arc<AtomicBool>,
    events: Option<mpsc::Sender<ScanEvent>>,
) -> ScanOutput {
    let started = SystemTime::now();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
        )
        .thread_name(|i| format!("sweep-scan-{i}"))
        .build()
        .unwrap_or_else(|_| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .expect("rayon single-thread fallback pool")
        });
    let svc = ScanService {
        cancel,
        events,
        pool: Arc::new(pool),
        now: SystemTime::now(),
    };

    let registry = crate::rules::registry();
    let total_rules = registry.len();

    // Detect phase (main thread, sequential — trivial cost).
    let mut ready: Vec<Option<Box<dyn crate::rules::Rule>>> = Vec::with_capacity(total_rules);
    let mut results: Vec<Option<RuleResult>> = Vec::with_capacity(total_rules);
    for rule in registry {
        match rule.detect(ctx) {
            crate::rules::Detect::Ready => {
                ready.push(Some(rule));
                results.push(None);
            }
            crate::rules::Detect::NotApplicable { reason } => {
                results.push(Some(RuleResult {
                    id: rule.id(),
                    title: rule.title(),
                    category: rule.category(),
                    tier: rule.tier(),
                    applied: false,
                    reason: Some(reason),
                    needs_sudo: rule.needs_sudo(),
                    location: rule.location(ctx),
                    info: ScanInfo::default(),
                    net_of: Vec::new(),
                    skipped: BTreeMap::new(),
                }));
                ready.push(None);
            }
        }
    }

    // `RuleDone` total = rules that will actually RUN. NotApplicable rules
    // never emit RuleDone, so the registry length would pin the spinner at
    // e.g. `3/4 rules` until it clears (critic finding); the spinner takes
    // its length from this field, not from its spawn-time registry guess.
    let ready_total = ready.iter().filter(|r| r.is_some()).count();

    // Scan phase: one scoped thread per Ready rule (consumers never block
    // pool workers — no jwalk busy-deadlock). Results collected indexed by
    // registry position: parallel completion, deterministic output order.
    let done_counter = Arc::new(AtomicU64::new(0));
    let scans: Vec<Option<Result<RuleScan, String>>> = std::thread::scope(|scope| {
        let handles: Vec<_> = ready
            .iter()
            .zip(results.iter())
            .map(|(rule_slot, _result_slot)| {
                let rule = rule_slot.as_ref()?;
                // Ready rules have a `None` results slot until assembly
                // fills it — take the id from the rule itself.
                let id = rule.id();
                let svc = &svc;
                let done_counter = &done_counter;
                Some(scope.spawn(move || {
                    let r = rule.scan(ctx, svc).map_err(|e| e.to_string());
                    let done = done_counter.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(tx) = &svc.events {
                        let _ = tx.send(ScanEvent::RuleDone {
                            id,
                            done: done as usize,
                            total: ready_total,
                        });
                    }
                    r
                }))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| Err("rule panicked".to_string()))
                })
            })
            .collect()
    });

    // Assemble in registry order (parallel completion, deterministic output
    // order). `newest_age` is already an age (rules compute now − newest
    // with the shared svc.now clock).
    let mut results: Vec<RuleResult> = results
        .into_iter()
        .zip(ready.iter())
        .zip(scans.iter())
        .map(|((slot, rule_slot), scan)| {
            if let Some(done) = slot {
                return done; // NotApplicable, already assembled
            }
            let rule = rule_slot.as_ref().expect("Ready slot present");
            let (applied, reason, info, skipped) = match scan {
                Some(Ok(rs)) => (true, None, rs.info.clone(), rs.skipped.clone()),
                Some(Err(e)) => (false, Some(e.clone()), ScanInfo::default(), BTreeMap::new()),
                None => (
                    false,
                    Some("rule slot missing".to_string()),
                    ScanInfo::default(),
                    BTreeMap::new(),
                ),
            };
            RuleResult {
                id: rule.id(),
                title: rule.title(),
                category: rule.category(),
                tier: rule.tier(),
                applied,
                reason,
                needs_sudo: rule.needs_sudo(),
                location: rule.location(ctx),
                info,
                net_of: Vec::new(),
                skipped,
            }
        })
        .collect();

    // Rule-specific notes + overlap subtraction (registry order known).
    // tmp: kept-newer note.
    if let Some(tmp) = results.iter_mut().find(|r| r.id == "tmp") {
        let kept_newer = tmp.skipped.get(&SkipKind::KeptNewer).copied().unwrap_or(0);
        if kept_newer > 0 && ctx.keep_days() > 0 {
            tmp.info.note = Some(format!("{kept_newer} newer than keep-days kept"));
        }
    }
    // user-cache: subtract thumbnails when applied.
    let thumbs = results
        .iter()
        .find(|r| r.id == "thumbnails")
        .map(|r| (r.applied, r.info.bytes, r.info.files, r.info.dirs));
    if let Some(uc) = results.iter_mut().find(|r| r.id == "user-cache")
        && let Some((true, tb, tf, td)) = thumbs
    {
        uc.info.bytes = uc.info.bytes.saturating_sub(tb);
        uc.info.files = uc.info.files.saturating_sub(tf);
        uc.info.dirs = uc.info.dirs.saturating_sub(td);
        uc.net_of = vec!["thumbnails"];
        uc.info.note = Some("(excl. thumbnails)".to_string());
    }

    // Totals: reclaimable = Σ applied bytes (already net of overlaps);
    // regular = Σ applied Safe bytes (config-preset includes arrive M3 —
    // identical to reclaimable in M1).
    let reclaimable_bytes: u64 = results
        .iter()
        .filter(|r| r.applied)
        .map(|r| r.info.bytes)
        .sum();
    let regular_bytes: u64 = results
        .iter()
        .filter(|r| r.applied && r.tier == Tier::Safe)
        .map(|r| r.info.bytes)
        .sum();
    let scan_ms = started.elapsed().map(|d| d.as_millis() as u64).unwrap_or(0);

    ScanOutput {
        results,
        totals: Totals {
            reclaimable_bytes,
            regular_bytes,
            scan_ms,
        },
    }
}
