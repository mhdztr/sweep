use super::{Detect, Rule, Tier};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanInfo, ScanService};
use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub struct DevArtifactsRule;

impl Rule for DevArtifactsRule {
    fn id(&self) -> &'static str {
        "dev-artifacts"
    }
    fn title(&self) -> &'static str {
        "Stale build artifacts"
    }
    fn category(&self) -> &'static str {
        "dev artifacts"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home)
    }
    fn tier(&self) -> Tier {
        Tier::Caution
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.dev_artifacts.enabled {
            return not_applicable("disabled in config");
        }
        if ctx.config.rules.dev_artifacts.paths.is_empty() {
            return not_applicable("no artifact directory names configured");
        }
        if !ctx.home.is_dir() {
            return not_applicable("home directory not found");
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        scan_home(ctx, svc)
    }
}

#[derive(Default)]
struct ArtifactTree {
    bytes: u64,
    files: u64,
    newest: Option<SystemTime>,
    candidates: Vec<PathBuf>,
    errors: u64,
}

impl ArtifactTree {
    fn merge(&mut self, other: Self) {
        self.bytes = self.bytes.saturating_add(other.bytes);
        self.files = self.files.saturating_add(other.files);
        self.errors = self.errors.saturating_add(other.errors);
        self.newest = match (self.newest, other.newest) {
            (Some(current), Some(candidate)) => Some(current.max(candidate)),
            (current, candidate) => current.or(candidate),
        };
        self.candidates.extend(other.candidates);
    }
}

fn scan_home(ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
    let root_metadata = std::fs::metadata(&ctx.home).map_err(|source| ScanError::Io {
        path: ctx.home.clone(),
        source,
    })?;
    let root_dev = root_metadata.dev();
    let excluded = expanded_excludes(ctx);
    let artifact_names = &ctx.config.rules.dev_artifacts.paths;
    let keep_days = ctx.config.rules.dev_artifacts.min_age_days.unwrap_or(30);
    let minimum_age = Duration::from_secs(u64::from(keep_days) * 86_400);
    let mut total = ArtifactTree::default();
    let mut largest: Vec<(u64, PathBuf)> = Vec::new();
    let mut stack = vec![ctx.home.clone()];

    while let Some(directory) = stack.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if directory == ctx.home => {
                return Err(ScanError::Io {
                    path: directory,
                    source: error,
                });
            }
            Err(_) => {
                total.errors += 1;
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    total.errors += 1;
                    continue;
                }
            };
            let path = entry.path();
            if should_skip_directory(ctx, &path, &excluded) {
                continue;
            }
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    total.errors += 1;
                    continue;
                }
            };
            if !metadata.file_type().is_dir() || metadata.dev() != root_dev {
                continue;
            }
            let relative = match path.strip_prefix(&ctx.home) {
                Ok(relative) => relative,
                Err(_) => continue,
            };
            if artifact_names
                .iter()
                .any(|name| matches_artifact_path(relative, name))
            {
                let tree = collect_tree(&path, root_dev, ctx, &excluded);
                if tree.errors == 0
                    && tree
                        .newest
                        .is_some_and(|modified| is_old_enough(modified, svc.now, minimum_age))
                {
                    let bytes = tree.bytes;
                    total.merge(tree);
                    largest.push((bytes, path));
                } else {
                    total.errors = total.errors.saturating_add(tree.errors);
                }
                continue;
            }
            stack.push(path);
            if let Some(tx) = &svc.events {
                let _ = tx.send(crate::scan::ScanEvent::Walk {
                    dirs: 1,
                    bytes: total.bytes,
                });
            }
        }
    }

    let artifact_count = largest.len() as u64;
    largest.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let samples = largest.into_iter().take(20).map(|(_, path)| path).collect();
    let newest_age = total
        .newest
        .and_then(|modified| svc.now.duration_since(modified).ok());
    let note = if total.errors == 0 {
        format!("stale artifact directories; keep {keep_days} days")
    } else {
        format!(
            "stale artifact directories; keep {keep_days} days; {} paths unreadable, totals may be low",
            total.errors
        )
    };
    Ok(RuleScan {
        info: ScanInfo {
            bytes: total.bytes,
            files: total.files,
            dirs: artifact_count,
            newest_age,
            samples,
            candidates: if svc.capture_candidates {
                total.candidates
            } else {
                Vec::new()
            },
            note: Some(note),
        },
        skipped: BTreeMap::new(),
    })
}

fn collect_tree(root: &Path, root_dev: u64, ctx: &Ctx, excluded: &[PathBuf]) -> ArtifactTree {
    let mut result = ArtifactTree::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                result.errors += 1;
                continue;
            }
        };
        if metadata.dev() != root_dev || metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.file_type().is_dir() && should_skip_directory(ctx, &path, excluded) {
            continue;
        }
        let modified = match metadata.modified() {
            Ok(modified) => Some(modified),
            Err(_) => {
                result.errors += 1;
                None
            }
        };
        if metadata.file_type().is_dir() {
            if let Some(modified) = modified {
                result.newest = Some(
                    result
                        .newest
                        .map_or(modified, |current| current.max(modified)),
                );
            }
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => stack.push(entry.path()),
                            Err(_) => result.errors += 1,
                        }
                    }
                }
                Err(_) => result.errors += 1,
            }
        } else if metadata.file_type().is_file() {
            result.bytes = result.bytes.saturating_add(metadata.len());
            result.files += 1;
            if let Some(modified) = modified {
                result.newest = Some(
                    result
                        .newest
                        .map_or(modified, |current| current.max(modified)),
                );
            }
            result.candidates.push(path);
        }
    }
    result
}

fn expanded_excludes(ctx: &Ctx) -> Vec<PathBuf> {
    ctx.config
        .scan
        .exclude_paths
        .iter()
        .filter_map(|path| {
            if path == "~" {
                Some(ctx.home.clone())
            } else if let Some(relative) = path.strip_prefix("~/") {
                Some(ctx.home.join(relative))
            } else {
                let path = PathBuf::from(path);
                path.is_absolute().then_some(path)
            }
        })
        .collect()
}

fn should_skip_directory(ctx: &Ctx, path: &Path, excluded: &[PathBuf]) -> bool {
    if path == ctx.project_root.as_deref().unwrap_or(Path::new(""))
        || path.file_name().is_some_and(|name| name == ".git")
        || excluded.iter().any(|exclude| path.starts_with(exclude))
        || ctx
            .project_root
            .as_ref()
            .is_some_and(|project| path.starts_with(project))
    {
        return true;
    }
    path.strip_prefix(&ctx.home)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some(
                    "Documents"
                        | "Pictures"
                        | "Videos"
                        | "Music"
                        | "Projects"
                        | "Desktop"
                        | "Downloads"
                )
            )
        })
}

fn matches_artifact_path(relative: &Path, configured: &str) -> bool {
    let configured = Path::new(configured);
    if configured.components().count() == 1 {
        return relative.file_name() == configured.file_name();
    }
    relative.ends_with(configured)
}

fn is_old_enough(modified: SystemTime, now: SystemTime, age: Duration) -> bool {
    now.duration_since(modified)
        .is_ok_and(|elapsed| elapsed >= age)
}

fn not_applicable(reason: &str) -> Detect {
    Detect::NotApplicable {
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn service(now: SystemTime) -> ScanService {
        ScanService {
            cancel: Arc::new(AtomicBool::new(false)),
            events: None,
            pool: Arc::new(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .build()
                    .expect("thread pool"),
            ),
            now,
            capture_candidates: true,
        }
    }

    fn age(path: &Path, modified: SystemTime) {
        filetime::set_file_mtime(path, filetime::FileTime::from_system_time(modified))
            .expect("set mtime");
    }

    #[test]
    fn only_removes_artifacts_when_newest_descendant_is_past_cutoff() {
        let temp = tempfile::tempdir().expect("home");
        let home = temp.path();
        let target = home.join("project/target");
        std::fs::create_dir_all(&target).expect("target");
        let old = target.join("old.bin");
        let fresh = target.join("fresh.bin");
        std::fs::write(&old, vec![0; 10]).expect("old file");
        std::fs::write(&fresh, vec![0; 20]).expect("fresh file");
        let now = SystemTime::now();
        let cutoff = now - Duration::from_secs(31 * 86_400);
        age(&old, cutoff);
        age(&fresh, now - Duration::from_secs(29 * 86_400));
        age(&target, cutoff);
        age(target.parent().expect("project"), cutoff);

        let mut ctx = Ctx::for_test(home.to_path_buf());
        ctx.config.rules.dev_artifacts.paths = vec!["target".into()];
        ctx.config.rules.dev_artifacts.min_age_days = Some(30);
        let result = scan_home(&ctx, &service(now)).expect("scan");
        assert_eq!(result.info.bytes, 0);

        age(&fresh, cutoff);
        age(&target, cutoff);
        let result = scan_home(&ctx, &service(now)).expect("scan");
        assert_eq!(result.info.bytes, 30);
        assert_eq!(result.info.files, 2);
        assert_eq!(result.info.candidates.len(), 2);
        assert_eq!(result.info.samples, [target]);
    }

    #[test]
    fn protects_git_self_excluded_and_excluded_roots() {
        let temp = tempfile::tempdir().expect("home");
        let home = temp.path();
        let project = home.join("work/project");
        let target = project.join("target");
        std::fs::create_dir_all(target.join(".git/target")).expect("git target");
        std::fs::create_dir_all(home.join("Projects/sweep/target")).expect("protected");
        std::fs::create_dir_all(home.join("work/sweep/target")).expect("self project");
        std::fs::create_dir_all(home.join("Documents/target")).expect("documents");
        std::fs::create_dir_all(home.join("excluded/target")).expect("excluded");
        std::fs::write(target.join("artifact"), vec![0; 17]).expect("artifact");
        std::fs::write(target.join(".git/target/git-data"), vec![0; 19]).expect("git data");
        std::fs::write(home.join("Projects/sweep/target/self"), vec![0; 23]).expect("project");
        std::fs::write(home.join("work/sweep/target/self"), vec![0; 37]).expect("self");
        std::fs::write(home.join("Documents/target/data"), vec![0; 29]).expect("documents data");
        std::fs::write(home.join("excluded/target/data"), vec![0; 31]).expect("excluded data");

        let now = SystemTime::now();
        let old = now - Duration::from_secs(40 * 86_400);
        for path in [
            target.join("artifact"),
            target.join(".git/target/git-data"),
            home.join("Projects/sweep/target/self"),
            home.join("work/sweep/target/self"),
            home.join("Documents/target/data"),
            home.join("excluded/target/data"),
            target.as_path().to_path_buf(),
        ] {
            age(&path, old);
        }
        age(&home.join("work/sweep/target"), old);
        let mut ctx = Ctx::for_test(home.to_path_buf());
        ctx.project_root = Some(home.join("work/sweep"));
        ctx.config.rules.dev_artifacts.paths = vec!["target".into()];
        ctx.config.rules.dev_artifacts.min_age_days = Some(30);
        ctx.config.scan.exclude_paths = vec!["~/excluded".into()];

        let result = scan_home(&ctx, &service(now)).expect("scan");
        assert_eq!(result.info.bytes, 17);
        assert!(
            result
                .info
                .candidates
                .iter()
                .all(|path| !path.starts_with(home.join("Projects")))
        );
        assert!(
            result
                .info
                .candidates
                .iter()
                .all(|path| !path.starts_with(home.join("work/sweep")))
        );
        assert!(
            result
                .info
                .candidates
                .iter()
                .all(|path| !path.to_string_lossy().contains(".git"))
        );
    }
}
