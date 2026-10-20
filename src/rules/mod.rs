//! Rule trait + tier model + the M1 rule registry. Spec: plan §2.9.

use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};
use std::path::Path;

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
        /// an ancestor of it) — copy-locked by tests.
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
}

/// Registry order is the output order: tmp, user-cache, thumbnails, trash.
pub fn registry() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(tmp::TmpRule),
        Box::new(cache::UserCacheRule),
        Box::new(cache::ThumbnailsRule),
        Box::new(trash::TrashRule),
    ]
}

/// "~/…" when under home, else absolute. Used by Rule::location + table.
pub fn display_path(home: &Path, p: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

pub mod cache;
pub mod tmp;
pub mod trash;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_order_is_tmp_usercache_thumbnails_trash() {
        let ids: Vec<&str> = registry().iter().map(|r| r.id()).collect();
        assert_eq!(ids, vec!["tmp", "user-cache", "thumbnails", "trash"]);
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
