use super::{Detect, Rule, any_path_exists, scan_targets};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};
use std::path::PathBuf;
use std::time::Duration;

const HOT_WINDOW: Duration = Duration::from_secs(300);
pub struct ToolCachesRule;

impl Rule for ToolCachesRule {
    fn id(&self) -> &'static str {
        "tool-caches"
    }
    fn title(&self) -> &'static str {
        "Dev tool caches"
    }
    fn category(&self) -> &'static str {
        "dev tools"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".cache"))
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.tool_caches.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".into(),
            };
        }
        if !any_path_exists(&target_paths(ctx)) {
            return Detect::NotApplicable {
                reason: "tool caches not found".into(),
            };
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        scan_targets(target_paths(ctx), svc, Some(HOT_WINDOW), None)
    }
}

pub fn target_paths(ctx: &Ctx) -> Vec<PathBuf> {
    let disabled = &ctx.config.rules.tool_caches.disable_tools;
    let mut targets = Vec::new();
    let mut add = |tool: &str, paths: &[PathBuf]| {
        if !disabled.iter().any(|entry| entry == tool) {
            targets.extend(paths.iter().cloned());
        }
    };
    add("npm", &[ctx.home.join(".npm/_cacache")]);
    add("pnpm", &[ctx.home.join(".local/share/pnpm/store")]);
    add("yarn", &[ctx.home.join(".cache/yarn")]);
    add("pip", &[ctx.home.join(".cache/pip")]);
    add("uv", &[ctx.home.join(".cache/uv")]);
    add(
        "cargo",
        &[
            ctx.home.join(".cargo/registry/cache"),
            ctx.home.join(".cargo/registry/.package-cache"),
        ],
    );
    add(
        "rustup",
        &[
            ctx.home.join(".rustup/downloads"),
            ctx.home.join(".rustup/tmp"),
        ],
    );
    add("go", &go_download_cache(ctx));
    add("gem", &[ctx.home.join(".cache/gem")]);
    add("composer", &[ctx.home.join(".cache/composer")]);
    add("deno", &[ctx.home.join(".cache/deno")]);
    add("pipx", &[ctx.home.join(".cache/pipx")]);
    targets
}

fn go_download_cache(ctx: &Ctx) -> Vec<PathBuf> {
    let module_cache = std::process::Command::new("go")
        .args(["env", "GOMODCACHE"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.starts_with(&ctx.home))
        .unwrap_or_else(|| ctx.home.join("go/pkg/mod"));
    vec![module_cache.join("cache/download")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_targets_the_documented_cache_subtrees() {
        let ctx = Ctx::for_test(PathBuf::from("/home/test"));
        let targets = target_paths(&ctx);
        assert!(targets.contains(&PathBuf::from("/home/test/.cargo/registry/cache")));
        assert!(!targets.contains(&PathBuf::from("/home/test/.cargo/registry/src")));
        assert!(targets.contains(&PathBuf::from("/home/test/go/pkg/mod/cache/download")));
    }

    #[test]
    fn can_disable_a_single_tool() {
        let mut ctx = Ctx::for_test(PathBuf::from("/home/test"));
        ctx.config
            .rules
            .tool_caches
            .disable_tools
            .push("cargo".into());
        assert!(
            !target_paths(&ctx)
                .iter()
                .any(|path| path.ends_with(".cargo/registry/cache"))
        );
        assert!(
            target_paths(&ctx)
                .iter()
                .any(|path| path.ends_with(".cache/pip"))
        );
    }

    #[test]
    fn known_tool_names_cover_catalog() {
        assert_eq!(
            [
                "npm", "pnpm", "yarn", "pip", "uv", "cargo", "rustup", "go", "gem", "composer",
                "deno", "pipx"
            ]
            .len(),
            12
        );
    }
}
