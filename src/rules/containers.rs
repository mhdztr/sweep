use super::{Detect, Rule, Tier, command_exists, command_output};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanInfo, ScanService};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

pub struct DockerRule;
pub struct PodmanRule;

impl Rule for DockerRule {
    fn id(&self) -> &'static str {
        "docker"
    }
    fn title(&self) -> &'static str {
        "Docker reclaim"
    }
    fn category(&self) -> &'static str {
        "docker"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "docker".to_string()
    }
    fn tier(&self) -> Tier {
        Tier::Caution
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.docker.enabled {
            return not_applicable("disabled in config");
        }
        if !ctx.commands_enabled {
            return not_applicable("external commands disabled in test context");
        }
        if !command_exists("docker") {
            return not_applicable("docker not found");
        }
        match super::command_success_timeout("docker", &["info"], Duration::from_secs(2)) {
            Ok(true) => Detect::Ready,
            _ => not_applicable("docker daemon not running"),
        }
    }
    fn scan(&self, _ctx: &Ctx, _svc: &ScanService) -> Result<RuleScan, ScanError> {
        scan_engine("docker", &["system", "df", "--format", "json"])
    }
    fn volume_gate(&self) -> bool {
        true
    }
}

impl Rule for PodmanRule {
    fn id(&self) -> &'static str {
        "podman"
    }
    fn title(&self) -> &'static str {
        "Podman reclaim"
    }
    fn category(&self) -> &'static str {
        "podman"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "podman".to_string()
    }
    fn tier(&self) -> Tier {
        Tier::Caution
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.podman.enabled {
            return not_applicable("disabled in config");
        }
        if !ctx.commands_enabled {
            return not_applicable("external commands disabled in test context");
        }
        if !command_exists("podman") {
            return not_applicable("podman not found");
        }
        match super::command_success_timeout("podman", &["info"], Duration::from_secs(2)) {
            Ok(true) => Detect::Ready,
            _ => not_applicable("podman storage unavailable"),
        }
    }
    fn scan(&self, _ctx: &Ctx, _svc: &ScanService) -> Result<RuleScan, ScanError> {
        scan_engine("podman", &["system", "df", "--format", "json"])
    }
}

fn scan_engine(engine: &str, args: &[&str]) -> Result<RuleScan, ScanError> {
    let output = command_output(engine, args).map_err(|source| ScanError::Io {
        path: PathBuf::from(engine),
        source,
    })?;
    if !output.status.success() {
        return Err(ScanError::Io {
            path: PathBuf::from(engine),
            source: std::io::Error::other(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let rows = parse_system_df(&stdout).ok_or_else(|| ScanError::Io {
        path: PathBuf::from(format!("{engine} system df")),
        source: std::io::Error::other("could not parse system df JSON"),
    })?;
    let bytes = rows.iter().fold(0_u64, |total, row| {
        total.saturating_add(row.reclaimable_bytes)
    });
    let details = rows
        .iter()
        .map(|row| format!("{}: {}", row.kind, row.reclaimable))
        .collect::<Vec<_>>()
        .join("; ");
    Ok(RuleScan {
        info: ScanInfo {
            bytes,
            files: rows.len() as u64,
            note: Some(details),
            ..ScanInfo::default()
        },
        skipped: BTreeMap::new(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SystemDfRow {
    kind: String,
    reclaimable: String,
    reclaimable_bytes: u64,
}

fn parse_system_df(json: &str) -> Option<Vec<SystemDfRow>> {
    let value = serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .or_else(|| {
            let rows: Vec<_> = json
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()
                .ok()?;
            Some(serde_json::Value::Array(rows))
        })?;
    let rows = value.as_array()?;
    rows.iter()
        .map(|row| {
            let kind = row
                .get("Type")
                .or_else(|| row.get("type"))?
                .as_str()?
                .to_string();
            let reclaimable = row
                .get("Reclaimable")
                .or_else(|| row.get("reclaimable"))?
                .as_str()?
                .to_string();
            let reclaimable_bytes = reclaimable
                .split_whitespace()
                .find_map(super::parse_human_size)?;
            Some(SystemDfRow {
                kind,
                reclaimable_bytes,
                reclaimable,
            })
        })
        .collect()
}

fn not_applicable(reason: &str) -> Detect {
    Detect::NotApplicable {
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_docker_json_array_and_json_lines() {
        let array = r#"[{"Type":"Images","Reclaimable":"1.5GB (50%)"},{"Type":"Build Cache","Reclaimable":"256MB (100%)"}]"#;
        let lines = "{\"Type\":\"Images\",\"Reclaimable\":\"1.5GB (50%)\"}\n{\"Type\":\"Build Cache\",\"Reclaimable\":\"256MB (100%)\"}\n";
        for parsed in [parse_system_df(array), parse_system_df(lines)] {
            let rows = parsed.expect("parse");
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].kind, "Images");
            assert_eq!(rows[0].reclaimable_bytes, 1_610_612_736);
            assert_eq!(rows[1].reclaimable_bytes, 268_435_456);
        }
    }

    #[test]
    fn malformed_or_incomplete_system_df_is_rejected() {
        assert!(parse_system_df("not json").is_none());
        assert!(parse_system_df(r#"[{"Type":"Images"}]"#).is_none());
        assert!(parse_system_df(r#"[{"Type":"Images","Reclaimable":"unknown"}]"#).is_none());
    }
}
