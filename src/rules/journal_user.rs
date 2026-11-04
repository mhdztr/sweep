use super::{Detect, Rule, any_path_exists, scan_targets};
use crate::ctx::Ctx;
use crate::scan::{RuleScan, ScanError, ScanService};
use std::path::PathBuf;

pub struct JournalUserRule;

impl Rule for JournalUserRule {
    fn id(&self) -> &'static str {
        "journal-user"
    }
    fn title(&self) -> &'static str {
        "User systemd journal"
    }
    fn category(&self) -> &'static str {
        "journal (user)"
    }
    fn location(&self, ctx: &Ctx) -> String {
        super::display_path(&ctx.home, &ctx.home.join(".local/state/log/journal"))
    }
    fn detect(&self, ctx: &Ctx) -> Detect {
        if !ctx.config.rules.journal_user.enabled {
            return Detect::NotApplicable {
                reason: "disabled in config".into(),
            };
        }
        if ctx.flags.trash {
            return Detect::NotApplicable {
                reason: "journal vacuum cannot be staged in trash".into(),
            };
        }
        if !command_exists("journalctl") {
            return Detect::NotApplicable {
                reason: "journalctl not found".into(),
            };
        }
        if !any_path_exists(&target_paths(ctx)) {
            return Detect::NotApplicable {
                reason: "user journal not found".into(),
            };
        }
        Detect::Ready
    }
    fn scan(&self, ctx: &Ctx, svc: &ScanService) -> Result<RuleScan, ScanError> {
        scan_targets(target_paths(ctx), svc, None, None)
    }
}

pub fn target_paths(ctx: &Ctx) -> Vec<PathBuf> {
    vec![
        ctx.home.join(".local/state/log/journal"),
        ctx.runtime_dir.join("log/journal"),
    ]
}

pub fn validate_max_size(value: &str) -> bool {
    let value = value.trim();
    let digits = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    if digits == 0 || value[..digits].parse::<u64>().is_err() {
        return false;
    }
    matches!(
        value[digits..].to_ascii_uppercase().as_str(),
        "" | "B"
            | "K"
            | "KB"
            | "KIB"
            | "M"
            | "MB"
            | "MIB"
            | "G"
            | "GB"
            | "GIB"
            | "T"
            | "TB"
            | "TIB"
    )
}

fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| {
            let candidate = directory.join(command);
            std::fs::metadata(candidate).is_ok_and(|metadata| {
                metadata.is_file() && {
                    use std::os::unix::fs::PermissionsExt as _;
                    metadata.permissions().mode() & 0o111 != 0
                }
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_size_accepts_simple_systemd_sizes_and_rejects_argument_injection() {
        assert!(validate_max_size("100M"));
        assert!(validate_max_size("1GiB"));
        assert!(validate_max_size("0"));
        assert!(!validate_max_size(""));
        assert!(!validate_max_size("--help"));
        assert!(!validate_max_size("100M --vacuum-time=1s"));
    }
}
