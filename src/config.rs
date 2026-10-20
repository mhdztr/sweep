//! Configuration model + loader. Spec: plan §2.4, ARCHITECTURE §7.
//!
//! No env mutation anywhere on this path: `load_from` takes the resolved path
//! explicitly (`Ctx::build` resolves `SWEEP_CONFIG ?? default_path()`).

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// `{message}` starts with `{line}:` for parse failures (line extracted
    /// from the toml span) so the copy matches `config {path}:{line}: …`.
    #[error("config {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("config {path}: file not found (explicitly set via SWEEP_CONFIG)")]
    Missing { path: PathBuf },
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(default)]
// NO deny_unknown_fields: unknown keys warn later (M3+), never fail.
pub struct Config {
    pub general: GeneralConfig,
    pub scan: ScanConfig,
    pub rules: RulesConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Default "regular"; validated: safe|regular|full.
    pub preset: String,
    /// Default 1; 0 = off.
    pub keep_days: u32,
    pub trash_instead_of_delete: bool,
    pub color: String,
    pub unicode: String,
    /// Parsed, unused until M2+.
    pub history_keep_runs: u32,
    /// Parsed, unused until M2+.
    pub scan_slow_after_ms: u64,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            preset: "regular".to_string(),
            keep_days: 1,
            trash_instead_of_delete: false,
            color: String::new(),
            unicode: String::new(),
            history_keep_runs: 50,
            scan_slow_after_ms: 2000,
        }
    }
}

/// Honored from M2/M6.
#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(default)]
pub struct ScanConfig {
    pub exclude_paths: Vec<String>,
    pub follow_mounts: bool,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(default)]
pub struct RulesConfig {
    #[serde(rename = "tmp")]
    pub tmp: RuleToggle,
    #[serde(rename = "user-cache")]
    pub user_cache: UserCacheConfig,
    #[serde(rename = "thumbnails")]
    pub thumbnails: RuleToggle,
    #[serde(rename = "trash")]
    pub trash: RuleToggle,
}

fn true_default() -> bool {
    true
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct RuleToggle {
    #[serde(default = "true_default")]
    pub enabled: bool,
}

impl Default for RuleToggle {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct UserCacheConfig {
    #[serde(default = "true_default")]
    pub enabled: bool,
    pub keep: Vec<String>,
}

impl Default for UserCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keep: Vec::new(),
        }
    }
}

/// `path` = `SWEEP_CONFIG` value (`Some`) or [`default_path`] (sandbox-aware;
/// caller resolves). `None` ⇒ absent default path ⇒
/// `Config::default()` (the "defaults-only path"). `Some(p)` missing ⇒
/// `Missing`. Parse/validation failure ⇒ `Parse`.
pub fn load_from(path: Option<&Path>) -> Result<Config, ConfigError> {
    let Some(path) = path else {
        return Ok(Config::default());
    };
    let text = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ConfigError::Missing {
                path: path.to_path_buf(),
            }
        } else {
            ConfigError::Parse {
                path: path.to_path_buf(),
                message: format!("{e} (unreadable)"),
            }
        }
    })?;
    let config: Config = toml::from_str(&text).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        message: parse_message(&text, &e),
    })?;
    match config.general.preset.as_str() {
        "safe" | "regular" | "full" => Ok(config),
        v => Err(ConfigError::Parse {
            path: path.to_path_buf(),
            message: format!("invalid preset '{v}' (expected: safe, regular, full)"),
        }),
    }
}

/// Render a `toml::de::Error` as `{line}: {message}` (line from the error
/// span, 1-based; `{message}` alone when the span is unavailable).
fn parse_message(text: &str, err: &toml::de::Error) -> String {
    let msg = err.message().to_string();
    match err.span() {
        Some(span) => {
            let line = text[..span.start.min(text.len())]
                .bytes()
                .filter(|&b| b == b'\n')
                .count()
                + 1;
            format!("{line}: {msg}")
        }
        None => msg,
    }
}

/// Default config path (plan §2.4, `dirs::config_dir()/sweep/config.toml`
/// semantics).
///
/// **Deliberate deviation from the plan's literal `dirs::config_dir()`:**
/// AGENTS invariant 4 / ARCHITECTURE §0.2 say every `$HOME`-relative path
/// goes through the resolved home (`dirs::config_dir()` reads `$HOME`/`$HOME/.config`
/// itself). When `SWEEP_HOME` points a run at a sandbox home, the default
/// config path must follow it, otherwise a sandboxed test would silently
/// load the developer's real `~/.config/sweep/config.toml` and become
/// non-deterministic the moment such a file exists. Outside a sandbox the
/// behavior is unchanged: exactly `dirs::config_dir()`.
///
/// `home` is the value `env::resolve_home()` produced (so `SWEEP_HOME` and
/// `Ctx::home` agree); the sandbox probe is isolated in `default_path_in`
/// so it is unit-testable without mutating process env (M1 bans that).
pub fn default_path(home: &Path) -> Option<PathBuf> {
    default_path_in(home, std::env::var_os("SWEEP_HOME"))
}

/// Pure core of [`default_path`]: sandboxed (`SWEEP_HOME` non-empty) ⇒
/// `{home}/.config/sweep/config.toml`; otherwise `dirs::config_dir()`.
pub(crate) fn default_path_in(
    home: &Path,
    sweep_home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    match sweep_home {
        Some(v) if !v.is_empty() => Some(home.join(".config/sweep/config.toml")),
        _ => dirs::config_dir().map(|d| d.join("sweep/config.toml")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, text).expect("write config");
        p
    }

    #[test]
    fn none_yields_defaults() {
        let c = load_from(None).expect("defaults");
        assert_eq!(c.general.preset, "regular");
        assert_eq!(c.general.keep_days, 1);
        assert!(c.rules.tmp.enabled);
        assert!(c.rules.user_cache.enabled);
        assert!(c.rules.user_cache.keep.is_empty());
        assert!(c.rules.thumbnails.enabled);
        assert!(c.rules.trash.enabled);
        assert!(c.scan.exclude_paths.is_empty());
    }

    #[test]
    fn parses_keep_days_and_keep() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write(
            &dir,
            "config.toml",
            "[general]\nkeep_days = 7\n\n[rules.user-cache]\nkeep = [\"pip\", \"npm\"]\n",
        );
        let c = load_from(Some(&p)).expect("parse");
        assert_eq!(c.general.keep_days, 7);
        assert_eq!(c.rules.user_cache.keep, vec!["pip", "npm"]);
    }

    #[test]
    fn unknown_keys_ignored_forward_compatible() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write(
            &dir,
            "config.toml",
            "[future-section]\nunknown = true\n\n[rules.tmp]\nenabled = false\nextra_key = 1\n",
        );
        let c = load_from(Some(&p)).expect("parse");
        assert!(!c.rules.tmp.enabled);
    }

    #[test]
    fn bad_toml_reports_line_in_display() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write(&dir, "config.toml", "[general\nkeep_days = 1\n");
        let err = load_from(Some(&p)).expect_err("bad toml");
        let display = err.to_string();
        let path_str = p.display().to_string();
        assert!(
            display.starts_with(&format!("config {path_str}:")),
            "display {display:?}"
        );
        assert!(
            display[format!("config {path_str}: ").len()..]
                .starts_with(|c: char| c.is_ascii_digit()),
            "message should start with the line number: {display:?}"
        );
        assert!(display.contains(':'), "line:msg colon present: {display:?}");
    }

    #[test]
    fn bad_preset_message() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write(&dir, "config.toml", "[general]\npreset = \"turbo\"\n");
        let err = load_from(Some(&p)).expect_err("bad preset");
        assert_eq!(
            err.to_string(),
            format!(
                "config {}: invalid preset 'turbo' (expected: safe, regular, full)",
                p.display()
            )
        );
    }

    #[test]
    fn explicitly_set_missing_file_is_missing_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("nope.toml");
        let err = load_from(Some(&p)).expect_err("missing");
        assert!(matches!(err, ConfigError::Missing { .. }));
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn default_path_sandbox_follows_sweep_home_home() {
        // Env-free by design (M1 bans env-mutating tests): the sandbox probe
        // is a parameter, not a read. A SWEEP_HOME run must resolve ITS own
        // config, never the developer's `~/.config/sweep/config.toml`
        // (AGENTS invariant 4; critic finding on `dirs::config_dir()`).
        let sandbox = PathBuf::from("/tmp/sweep-sandbox-home");
        assert_eq!(
            default_path_in(
                &sandbox,
                Some(std::ffi::OsString::from("/tmp/sweep-sandbox-home"))
            ),
            Some(sandbox.join(".config/sweep/config.toml")),
            "sandboxed default config path lives inside the sandbox home"
        );
        // Empty SWEEP_HOME is treated as unset (matches `resolve_home`'s
        // "must be a real dir" contract defensively) → legacy XDG/default
        // behavior via `dirs::config_dir()`; only the sandbox branch is
        // asserted here (the XDG branch reads process env).
        assert!(default_path_in(&sandbox, Some(std::ffi::OsString::new())).is_some());
    }
}
