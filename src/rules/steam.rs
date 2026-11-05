use super::{Detect, Rule, Tier, any_path_exists, scan_targets};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};
use std::path::PathBuf;

pub struct SteamShaderRule;

impl Rule for SteamShaderRule {
    fn id(&self) -> &'static str {
        "steam-shader"
    }
    fn title(&self) -> &'static str {
        "Steam shader cache"
    }
    fn category(&self) -> &'static str {
        "steam"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".local/share/Steam"))
    }
    fn tier(&self) -> Tier {
        Tier::Caution
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.steam_shader.enabled {
            return not_applicable("disabled in config");
        }
        if !any_path_exists(&target_paths(ctx)) {
            return not_applicable("Steam shader caches not found");
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        let mut result = scan_targets(target_paths(ctx), svc, None, None)?;
        result.info.note = Some("shaders regenerate; may stutter on next launch".into());
        Ok(result)
    }
}

pub fn target_paths(ctx: &Ctx) -> Vec<PathBuf> {
    let mut paths = vec![
        ctx.home.join(".steam/steam/appshadercache"),
        ctx.home.join(".local/share/Steam/appshadercache"),
    ];
    for root in [
        ctx.home.join(".local/share/Steam/shadercache"),
        ctx.home.join(".steam/steam/steamapps/shadercache"),
    ] {
        paths.extend(super::child_dirs(&root));
    }
    paths
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
    use std::time::SystemTime;

    #[test]
    fn only_includes_shader_cache_paths() {
        let temp = tempfile::tempdir().expect("home");
        let home = temp.path();
        let cache = home.join(".local/share/Steam/shadercache/123");
        std::fs::create_dir_all(&cache).expect("shader cache");
        std::fs::write(cache.join("shader.bin"), vec![0; 20]).expect("shader");
        std::fs::create_dir_all(home.join(".local/share/Steam/steamapps/common/game"))
            .expect("game");
        std::fs::write(
            home.join(".local/share/Steam/steamapps/common/game/keep.bin"),
            vec![0; 30],
        )
        .expect("game file");
        let ctx = Ctx::for_test(home.to_path_buf());
        let service = ScanService {
            cancel: Arc::new(AtomicBool::new(false)),
            events: None,
            pool: Arc::new(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .build()
                    .expect("thread pool"),
            ),
            now: SystemTime::now(),
            capture_candidates: true,
        };
        let result = SteamShaderRule.scan(&ctx, &service).expect("scan");
        assert_eq!(result.info.bytes, 20);
        assert_eq!(result.info.files, 1);
        assert_eq!(result.info.candidates, [cache.join("shader.bin")]);
    }
}
