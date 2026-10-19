//! JSON report models — `sweep scan --json`, schema v1. Spec: plan §2.13,
//! `docs/CLI-UX.md` §9, `docs/ARCHITECTURE.md` §12.

use crate::SCHEMA_VERSION;
use crate::ctx::Ctx;
use crate::scan::ScanOutput;

#[derive(serde::Serialize)]
pub struct ScanReport {
    pub version: u32,
    pub kind: &'static str,
    pub host: HostInfo,
    pub rules: Vec<RuleJson>,
    pub totals: TotalsJson,
    pub config: ConfigJson,
}

#[derive(serde::Serialize)]
pub struct HostInfo {
    pub distro: String,
    pub family: String,
    /// `null` on Other — key always present.
    pub pkgman: Option<String>,
    pub fstype: String,
    /// Only with `-v` (omitted otherwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

#[derive(serde::Serialize)]
pub struct RuleJson {
    pub id: &'static str,
    pub title: &'static str,
    pub tier: &'static str,
    pub applied: bool,
    /// `null` when applied.
    pub reason: Option<String>,
    pub needs_sudo: bool,
    pub bytes: u64,
    pub files: u64,
    pub dirs: u64,
    /// `null` when the rule counted no entries ("never").
    pub newest_age_secs: Option<u64>,
    pub net_of: Vec<&'static str>,
    /// Only with `-v`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub samples: Option<Vec<String>>,
}

#[derive(serde::Serialize)]
pub struct TotalsJson {
    pub reclaimable_bytes: u64,
    pub regular_bytes: u64,
    pub scan_ms: u64,
}

#[derive(serde::Serialize)]
pub struct ConfigJson {
    pub preset: &'static str,
    pub keep_days: u32,
}

impl ScanReport {
    /// Field order above = serialization order (serde preserves declaration
    /// order). Per-rule exactly the 11 spec keys (+ `samples` only when
    /// verbose) — no extra keys (D-7). `host.hostname` / `samples` omitted
    /// unless verbose (ARCHITECTURE §12).
    pub fn from_scan(ctx: &Ctx, out: &ScanOutput, verbose: bool) -> ScanReport {
        ScanReport {
            version: SCHEMA_VERSION,
            kind: "scan",
            host: HostInfo {
                distro: ctx.system.distro_id.clone(),
                family: ctx.system.family.as_str().to_string(),
                pkgman: ctx.system.pkgman.clone(),
                fstype: ctx.system.fstype.clone(),
                hostname: verbose.then(|| ctx.system.hostname.clone()),
            },
            rules: out
                .results
                .iter()
                .map(|r| RuleJson {
                    id: r.id,
                    title: r.title,
                    tier: r.tier.as_str(),
                    applied: r.applied,
                    reason: r.reason.clone(),
                    needs_sudo: r.needs_sudo,
                    bytes: r.info.bytes,
                    files: r.info.files,
                    dirs: r.info.dirs,
                    newest_age_secs: r.info.newest_age.map(|a| a.as_secs()),
                    net_of: r.net_of.clone(),
                    samples: verbose.then(|| {
                        r.info
                            .samples
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect()
                    }),
                })
                .collect(),
            totals: TotalsJson {
                reclaimable_bytes: out.totals.reclaimable_bytes,
                regular_bytes: out.totals.regular_bytes,
                scan_ms: out.totals.scan_ms,
            },
            config: ConfigJson {
                preset: ctx.preset_name(),
                keep_days: ctx.keep_days(),
            },
        }
    }
}
