use super::{Detect, Rule, Tier, any_path_exists, command_exists, command_output, scan_targets};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanInfo, ScanService};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct PacmanCacheRule;
pub struct JournalSystemRule;
pub struct AurBuildsRule;
pub struct OrphansRule;
pub struct FlatpakUnusedRule;

impl Rule for PacmanCacheRule {
    fn id(&self) -> &'static str {
        "pacman-cache"
    }
    fn title(&self) -> &'static str {
        "Pacman package cache"
    }
    fn category(&self) -> &'static str {
        "pacman cache"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.var_cache_root.join("pacman/pkg"))
    }
    fn tier(&self) -> Tier {
        Tier::Caution
    }
    fn needs_sudo(&self) -> bool {
        true
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.pacman_cache.enabled {
            return not_applicable("disabled in config");
        }
        if !ctx.commands_enabled {
            return not_applicable("external commands disabled in test context");
        }
        if ctx.system.pkgman.as_deref() != Some("pacman") {
            return not_applicable("pacman package manager not detected");
        }
        if !command_exists("paccache") {
            return not_applicable("paccache missing; install pacman-contrib");
        }
        if !ctx.var_cache_root.join("pacman/pkg").is_dir() {
            return not_applicable("pacman cache not found");
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        let keep = ctx.config.rules.pacman_cache.keep_versions.unwrap_or(
            if ctx.effective_preset() == crate::cli::Preset::Full {
                0
            } else {
                2
            },
        );
        let paths = paccache_candidates(&ctx.var_cache_root.join("pacman/pkg"), keep)?;
        let mut result = scan_targets(paths, svc, None, None)?;
        result.info.note = Some(format!("keeping newest {keep} versions"));
        Ok(result)
    }
}

fn paccache_candidates(root: &Path, keep: u32) -> Result<Vec<PathBuf>, ScanError> {
    let output = std::process::Command::new("paccache")
        .args([
            "--dryrun",
            "--verbose",
            "--verbose",
            "--null",
            "--quiet",
            "--keep",
        ])
        .arg(keep.to_string())
        .arg("--cachedir")
        .arg(root)
        .arg("--nocolor")
        .output()
        .map_err(|source| ScanError::Io {
            path: PathBuf::from("paccache --dryrun"),
            source,
        })?;
    if !output.status.success() {
        return Err(command_error("paccache --dryrun", &output.stderr));
    }
    let mut paths =
        parse_paccache_candidates(root, &output.stdout).map_err(|source| ScanError::Io {
            path: PathBuf::from("paccache --dryrun"),
            source,
        })?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn parse_paccache_candidates(root: &Path, stdout: &[u8]) -> std::io::Result<Vec<PathBuf>> {
    let stdout = stdout.strip_suffix(b"\n").unwrap_or(stdout);
    stdout
        .split(|byte| *byte == 0)
        .filter(|candidate| !candidate.is_empty())
        .map(|candidate| {
            let path = PathBuf::from(OsString::from_vec(candidate.to_vec()));
            if !path.is_absolute() || !path.starts_with(root) {
                return Err(std::io::Error::other(format!(
                    "paccache returned a path outside its cache directory: {}",
                    path.display()
                )));
            }
            Ok(path)
        })
        .collect()
}

impl Rule for JournalSystemRule {
    fn id(&self) -> &'static str {
        "journal-system"
    }
    fn title(&self) -> &'static str {
        "System journald"
    }
    fn category(&self) -> &'static str {
        "journal (system)"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.var_log_root.join("journal"))
    }
    fn tier(&self) -> Tier {
        Tier::Caution
    }
    fn needs_sudo(&self) -> bool {
        true
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.journal_system.enabled {
            return not_applicable("disabled in config");
        }
        if !ctx.commands_enabled {
            return not_applicable("external commands disabled in test context");
        }
        if !command_exists("journalctl") {
            return not_applicable("journalctl not found");
        }
        if !any_path_exists(&journal_system_paths(ctx)) {
            return not_applicable("system journal not found");
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        let max_size = ctx
            .config
            .rules
            .journal_system
            .max_size
            .as_deref()
            .unwrap_or("100M");
        let target = super::parse_human_size(max_size).ok_or_else(|| ScanError::Io {
            path: PathBuf::from("journal-system max_size"),
            source: std::io::Error::other("invalid configured journal size"),
        })?;
        let output = command_output("journalctl", &["--disk-usage"]);
        if let Ok(output) = output
            && output.status.success()
            && let Some(bytes) = parse_journal_usage(&String::from_utf8_lossy(&output.stdout))
        {
            return Ok(RuleScan {
                info: ScanInfo {
                    bytes: reclaimable_journal_bytes(bytes, target),
                    note: Some(format!(
                        "system journal {bytes} B; vacuum target {max_size}"
                    )),
                    ..ScanInfo::default()
                },
                skipped: BTreeMap::new(),
            });
        }
        let mut result = scan_targets(journal_system_paths(ctx), svc, None, None)?;
        result.info.bytes = reclaimable_journal_bytes(result.info.bytes, target);
        result.info.note = Some(format!(
            "estimated from readable files; vacuum target {max_size}"
        ));
        Ok(result)
    }
}

fn journal_system_paths(ctx: &Ctx) -> Vec<PathBuf> {
    vec![
        ctx.var_log_root.join("journal"),
        ctx.run_root.join("log/journal"),
    ]
}

fn parse_journal_usage(output: &str) -> Option<u64> {
    output.split_whitespace().find_map(super::parse_human_size)
}

fn reclaimable_journal_bytes(usage: u64, target: u64) -> u64 {
    usage.saturating_sub(target)
}

impl Rule for AurBuildsRule {
    fn id(&self) -> &'static str {
        "aur-builds"
    }
    fn title(&self) -> &'static str {
        "AUR build artifacts"
    }
    fn category(&self) -> &'static str {
        "AUR/build"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".cache"))
    }
    fn tier(&self) -> Tier {
        Tier::Caution
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.aur_builds.enabled {
            return not_applicable("disabled in config");
        }
        let roots = aur_roots(ctx);
        if !any_path_exists(&roots) {
            return not_applicable("AUR build caches not found");
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        let age = Duration::from_secs(
            u64::from(ctx.config.rules.aur_builds.keep_days.unwrap_or(1)) * 86_400,
        );
        let paths = aur_candidates(ctx, svc.now, age);
        let mut result = scan_targets(paths, svc, None, None)?;
        result.info.note = Some("git checkouts kept; only stale build outputs included".into());
        Ok(result)
    }
}

fn aur_roots(ctx: &Ctx) -> Vec<PathBuf> {
    vec![
        ctx.home.join(".cache/paru/clone"),
        ctx.home.join(".cache/yay"),
        ctx.home.join(".cache/.temp"),
    ]
}

fn aur_artifact_roots(ctx: &Ctx) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let roots = aur_roots(ctx);
    for (index, root) in roots.iter().enumerate() {
        if index == 2 && root.exists() {
            result.push(root.clone());
            continue;
        }
        for repo in super::child_dirs(root) {
            for name in ["src", "pkg"] {
                let path = repo.join(name);
                if path.exists() {
                    result.push(path);
                }
            }
            if let Ok(entries) = std::fs::read_dir(&repo) {
                result.extend(entries.flatten().filter_map(|entry| {
                    let path = entry.path();
                    let name = path.file_name()?.to_str()?;
                    (entry.file_type().is_ok_and(|kind| kind.is_file())
                        && (name.ends_with(".log")
                            || name.contains(".pkg.tar.")
                            || name.ends_with(".pkg.tar")))
                    .then_some(path)
                }));
            }
        }
    }
    result
}

fn aur_candidates(ctx: &Ctx, now: std::time::SystemTime, age: Duration) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for root in aur_artifact_roots(ctx) {
        let Ok(metadata) = std::fs::symlink_metadata(&root) else {
            continue;
        };
        if metadata.file_type().is_dir() {
            let (mut files, newest, errors) = collect_aur_tree(&root, metadata.dev());
            if errors == 0
                && newest.is_some_and(|modified| {
                    now.duration_since(modified)
                        .is_ok_and(|elapsed| elapsed >= age)
                })
            {
                candidates.append(&mut files);
            }
        } else if metadata.file_type().is_file()
            && metadata.modified().is_ok_and(|modified| {
                now.duration_since(modified)
                    .is_ok_and(|elapsed| elapsed >= age)
            })
        {
            candidates.push(root);
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn collect_aur_tree(
    root: &Path,
    root_dev: u64,
) -> (Vec<PathBuf>, Option<std::time::SystemTime>, u64) {
    let mut stack = vec![root.to_path_buf()];
    let mut newest: Option<std::time::SystemTime> = None;
    let mut files = Vec::new();
    let mut errors = 0;
    while let Some(path) = stack.pop() {
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        if metadata.dev() != root_dev || metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.file_type().is_dir() {
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            match std::fs::read_dir(path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => stack.push(entry.path()),
                            Err(_) => errors += 1,
                        }
                    }
                }
                Err(_) => errors += 1,
            }
        } else if metadata.file_type().is_file() {
            match metadata.modified() {
                Ok(modified) => {
                    newest = Some(newest.map_or(modified, |current| current.max(modified)));
                }
                Err(_) => errors += 1,
            }
            files.push(path);
        }
    }
    (files, newest, errors)
}

impl Rule for OrphansRule {
    fn id(&self) -> &'static str {
        "orphans"
    }
    fn title(&self) -> &'static str {
        "Orphaned packages"
    }
    fn category(&self) -> &'static str {
        "orphans"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "pacman".to_string()
    }
    fn tier(&self) -> Tier {
        Tier::Caution
    }
    fn needs_sudo(&self) -> bool {
        true
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.orphans.enabled {
            return not_applicable("disabled in config");
        }
        if !ctx.commands_enabled {
            return not_applicable("external commands disabled in test context");
        }
        if ctx.system.pkgman.as_deref() != Some("pacman") || !command_exists("pacman") {
            return not_applicable("pacman package manager not detected");
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, _svc: &ScanService) -> Result<RuleScan, ScanError> {
        let output = command_output("pacman", &["-Qtdq"]).map_err(|source| ScanError::Io {
            path: PathBuf::from("pacman -Qtdq"),
            source,
        })?;
        if !output.status.success() {
            return Err(command_error("pacman -Qtdq", &output.stderr));
        }
        let excluded = &ctx.config.rules.orphans.exclude;
        let packages = parse_orphans(&String::from_utf8_lossy(&output.stdout), excluded);
        Ok(RuleScan {
            info: ScanInfo {
                files: packages.len() as u64,
                note: Some(if packages.is_empty() {
                    "0 orphans found".into()
                } else {
                    format!("{} · {}", packages.len(), packages.join(", "))
                }),
                ..ScanInfo::default()
            },
            skipped: BTreeMap::new(),
        })
    }
    fn confirm_batch_size(&self) -> Option<usize> {
        Some(200)
    }
}

fn parse_orphans(output: &str, exclude: &[String]) -> Vec<String> {
    let mut packages: Vec<_> = output
        .lines()
        .map(str::trim)
        .filter(|package| !package.is_empty() && !exclude.iter().any(|item| item == package))
        .map(str::to_string)
        .collect();
    packages.sort();
    packages.dedup();
    packages
}

impl Rule for FlatpakUnusedRule {
    fn id(&self) -> &'static str {
        "flatpak-unused"
    }
    fn title(&self) -> &'static str {
        "Unused Flatpak runtimes"
    }
    fn category(&self) -> &'static str {
        "flatpak"
    }
    fn location(&self, _ctx: &Ctx) -> String {
        "flatpak".to_string()
    }
    fn tier(&self) -> Tier {
        Tier::Caution
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.flatpak_unused.enabled {
            return not_applicable("disabled in config");
        }
        if !ctx.commands_enabled {
            return not_applicable("external commands disabled in test context");
        }
        if !command_exists("flatpak") {
            return not_applicable("flatpak not found");
        }
        match command_output("flatpak", &["remotes", "--columns=name"]) {
            Ok(output) if output.status.success() && !output.stdout.is_empty() => Detect::Ready,
            _ => not_applicable("no Flatpak installation detected"),
        }
    }
    fn scan(&self, _ctx: &Ctx, _svc: &ScanService) -> Result<RuleScan, ScanError> {
        let output = command_output("flatpak", &["uninstall", "--unused", "--dry-run"]).map_err(
            |source| ScanError::Io {
                path: PathBuf::from("flatpak uninstall --unused --dry-run"),
                source,
            },
        )?;
        if !output.status.success() {
            return Err(command_error(
                "flatpak uninstall --unused --dry-run",
                &output.stderr,
            ));
        }
        let (runtimes, bytes) = parse_flatpak_dry_run(&String::from_utf8_lossy(&output.stdout));
        Ok(RuleScan {
            info: ScanInfo {
                bytes,
                files: runtimes.len() as u64,
                note: Some(if runtimes.is_empty() {
                    "no unused runtimes found".into()
                } else {
                    format!("{} · {}", runtimes.len(), runtimes.join(", "))
                }),
                ..ScanInfo::default()
            },
            skipped: BTreeMap::new(),
        })
    }
}

fn parse_flatpak_dry_run(output: &str) -> (Vec<String>, u64) {
    let mut refs = Vec::new();
    let mut bytes = 0_u64;
    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        if let Some(size) = fields
            .windows(2)
            .find_map(|pair| super::parse_human_size(&format!("{} {}", pair[0], pair[1])))
        {
            bytes = bytes.saturating_add(size);
        }
        if line.contains('.') && !line.starts_with("Warning:") {
            refs.push(line.to_string());
        }
    }
    refs.sort();
    refs.dedup();
    (refs, bytes)
}

fn not_applicable(reason: &str) -> Detect {
    Detect::NotApplicable {
        reason: reason.into(),
    }
}

fn command_error(command: &str, stderr: &[u8]) -> ScanError {
    ScanError::Io {
        path: PathBuf::from(command),
        source: std::io::Error::other(String::from_utf8_lossy(stderr).trim().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paccache_candidates_are_nul_delimited_and_confined() {
        let root = Path::new("/var/cache/pacman/pkg");
        let stdout =
            b"/var/cache/pacman/pkg/foo.pkg.tar.zst\0/var/cache/pacman/pkg/foo.pkg.tar.zst.sig\0\n";
        let names: Vec<_> = parse_paccache_candidates(root, stdout)
            .expect("parse candidates")
            .iter()
            .filter_map(|path| path.file_name()?.to_str().map(str::to_string))
            .collect();
        assert_eq!(names, ["foo.pkg.tar.zst", "foo.pkg.tar.zst.sig"]);
        assert!(
            parse_paccache_candidates(root, b"")
                .expect("empty output")
                .is_empty()
        );
        assert!(parse_paccache_candidates(root, b"/etc/passwd\0").is_err());
    }

    #[test]
    fn journal_estimate_excludes_the_configured_vacuum_target() {
        assert_eq!(
            reclaimable_journal_bytes(1024 * 1024 * 1024, 100 * 1024 * 1024),
            924 * 1024 * 1024
        );
        assert_eq!(reclaimable_journal_bytes(50, 100), 0);
    }

    #[test]
    fn human_size_parser_rejects_malformed_and_overflow() {
        assert_eq!(
            super::super::parse_human_size("1.5 GiB"),
            Some(1_610_612_736)
        );
        assert_eq!(
            super::super::parse_human_size("12M"),
            Some(12 * 1024 * 1024)
        );
        assert_eq!(super::super::parse_human_size("n/a"), None);
        assert_eq!(
            super::super::parse_human_size("999999999999999999999T"),
            None
        );
    }

    #[test]
    fn orphan_exclusions_are_exact_and_deduplicated() {
        let result = parse_orphans("alpha\nbeta\nalpha\n", &["alpha2".into()]);
        assert_eq!(result, ["alpha", "beta"]);
        assert_eq!(parse_orphans("", &[]), Vec::<String>::new());
    }

    #[test]
    fn flatpak_dry_run_parser_collects_refs_and_size() {
        let (refs, bytes) =
            parse_flatpak_dry_run("org.example.Platform  1.5 GiB\norg.example.Locale  200 MiB\n");
        assert_eq!(
            refs,
            [
                "org.example.Locale  200 MiB",
                "org.example.Platform  1.5 GiB"
            ]
        );
        assert_eq!(bytes, 1_820_327_936);
    }

    #[test]
    fn aur_age_gate_uses_newest_descendant() {
        let temp = tempfile::tempdir().expect("repo");
        let src = temp.path().join("src");
        std::fs::create_dir_all(&src).expect("src directory");
        let old = src.join("old.c");
        let fresh = src.join("new.c");
        std::fs::write(&old, b"old").expect("old");
        std::fs::write(&fresh, b"new").expect("fresh");
        let now = std::time::SystemTime::now();
        filetime::set_file_mtime(
            &old,
            filetime::FileTime::from_system_time(now - Duration::from_secs(86_400 * 2)),
        )
        .expect("old mtime");
        let (_, newest, errors) =
            collect_aur_tree(&src, std::fs::metadata(&src).expect("src metadata").dev());
        assert_eq!(errors, 0);
        assert!(newest.is_some_and(|modified| {
            now.duration_since(modified)
                .is_ok_and(|elapsed| elapsed < Duration::from_secs(86_400))
        }));
        std::fs::remove_file(&fresh).expect("remove fresh fixture");
        let (files, newest, errors) =
            collect_aur_tree(&src, std::fs::metadata(&src).expect("src metadata").dev());
        assert_eq!(files, [old].into_iter().collect::<Vec<_>>());
        assert_eq!(errors, 0);
        assert!(newest.is_some_and(|modified| {
            now.duration_since(modified)
                .is_ok_and(|elapsed| elapsed >= Duration::from_secs(86_400))
        }));
    }
}
