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
// Unknown top-level sections remain forward-compatible; rule/preset sections
// are validated explicitly in `load_from`.
pub struct Config {
    pub general: GeneralConfig,
    pub scan: ScanConfig,
    pub rules: RulesConfig,
    pub presets: PresetsConfig,
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
    /// Retain this many history records after a mutating clean run.
    pub history_keep_runs: u32,
    /// UI hint threshold for slow scans.
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
            history_keep_runs: 100,
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
    #[serde(rename = "journal-user")]
    pub journal_user: JournalUserConfig,
    #[serde(rename = "browser-cache")]
    pub browser_cache: RuleToggle,
    #[serde(rename = "editor-cache")]
    pub editor_cache: RuleToggle,
    #[serde(rename = "tool-caches")]
    pub tool_caches: ToolCachesConfig,
    #[serde(rename = "app-caches")]
    pub app_caches: AppCachesConfig,
    #[serde(rename = "log-rotate")]
    pub log_rotate: RuleToggle,
    #[serde(rename = "xdg-state-junk")]
    pub xdg_state_junk: RuleToggle,
    #[serde(rename = "pacman-cache")]
    pub pacman_cache: PacmanCacheConfig,
    #[serde(rename = "journal-system")]
    pub journal_system: JournalSystemConfig,
    #[serde(rename = "aur-builds")]
    pub aur_builds: AurBuildsConfig,
    #[serde(rename = "orphans")]
    pub orphans: OrphansConfig,
    #[serde(rename = "flatpak-unused")]
    pub flatpak_unused: RuleToggle,
    #[serde(rename = "docker")]
    pub docker: RuleToggle,
    #[serde(rename = "podman")]
    pub podman: RuleToggle,
    #[serde(rename = "dev-artifacts")]
    pub dev_artifacts: DevArtifactsConfig,
    #[serde(rename = "steam-shader")]
    pub steam_shader: RuleToggle,
    #[serde(rename = "snapshots-report")]
    pub snapshots_report: RuleToggle,
    #[serde(rename = "downloads")]
    pub downloads: DownloadsConfig,
    #[serde(rename = "cache-deep")]
    pub cache_deep: RuleToggle,
    #[serde(rename = "tmp-full")]
    pub tmp_full: RuleToggle,
    #[serde(rename = "pkgcache-zero")]
    pub pkgcache_zero: RuleToggle,
    #[serde(rename = "purge-logs")]
    pub purge_logs: RuleToggle,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(default)]
pub struct PresetsConfig {
    pub safe: PresetRuleConfig,
    pub regular: PresetRuleConfig,
    pub full: PresetRuleConfig,
}

impl PresetsConfig {
    pub fn for_preset(&self, preset: crate::cli::Preset) -> &PresetRuleConfig {
        match preset {
            crate::cli::Preset::Safe => &self.safe,
            crate::cli::Preset::Regular => &self.regular,
            crate::cli::Preset::Full => &self.full,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(default)]
pub struct PresetRuleConfig {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct JournalUserConfig {
    #[serde(default = "true_default")]
    pub enabled: bool,
    pub max_size: String,
}

impl Default for JournalUserConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_size: "100M".into(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct ToolCachesConfig {
    #[serde(default = "true_default")]
    pub enabled: bool,
    pub disable_tools: Vec<String>,
}

impl Default for ToolCachesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            disable_tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct AppCachesConfig {
    #[serde(default = "true_default")]
    pub enabled: bool,
    pub extra_paths: Vec<String>,
}

impl Default for AppCachesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            extra_paths: Vec::new(),
        }
    }
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

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct PacmanCacheConfig {
    #[serde(default = "true_default")]
    pub enabled: bool,
    pub keep_versions: Option<u32>,
}

impl Default for PacmanCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keep_versions: None,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct JournalSystemConfig {
    #[serde(default = "true_default")]
    pub enabled: bool,
    pub max_size: Option<String>,
}

impl Default for JournalSystemConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_size: Some("100M".to_string()),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct AurBuildsConfig {
    #[serde(default = "true_default")]
    pub enabled: bool,
    pub keep_days: Option<u32>,
}

impl Default for AurBuildsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keep_days: Some(1),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct OrphansConfig {
    #[serde(default = "true_default")]
    pub enabled: bool,
    pub confirm_batch: Option<usize>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl Default for OrphansConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            confirm_batch: Some(200),
            exclude: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct DevArtifactsConfig {
    #[serde(default = "true_default")]
    pub enabled: bool,
    pub min_age_days: Option<u32>,
    #[serde(default)]
    pub paths: Vec<String>,
}

impl Default for DevArtifactsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_age_days: Some(30),
            paths: vec![
                "node_modules".to_string(),
                "target".to_string(),
                ".venv".to_string(),
                "__pycache__".to_string(),
                ".pytest_cache".to_string(),
                ".gradle".to_string(),
                ".cache/pypoetry".to_string(),
                "elm-stuff".to_string(),
                ".n2".to_string(),
                ".bundle".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct DownloadsConfig {
    #[serde(default = "true_default")]
    pub enabled: bool,
    pub keep_days: Option<u32>,
}

impl Default for DownloadsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keep_days: Some(0),
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
    let document = text
        .parse::<toml::Table>()
        .map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            message: parse_message(&text, &e),
        })?;
    validate_rule_sections(path, &document)?;
    validate_preset_sections(path, &document)?;
    let config: Config = toml::from_str(&text).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        message: parse_message(&text, &e),
    })?;
    match config.general.preset.as_str() {
        "safe" | "regular" | "full" => {
            validate_preset_rule_ids(path, &config)?;
            if !crate::rules::journal_user::validate_max_size(&config.rules.journal_user.max_size) {
                return Err(ConfigError::Parse {
                    path: path.to_path_buf(),
                    message: "invalid [rules.journal-user].max_size".into(),
                });
            }
            if config
                .rules
                .journal_system
                .max_size
                .as_deref()
                .is_some_and(|size| !crate::rules::journal_user::validate_max_size(size))
            {
                return Err(ConfigError::Parse {
                    path: path.to_path_buf(),
                    message: "invalid [rules.journal-system].max_size".into(),
                });
            }
            if config.rules.orphans.confirm_batch == Some(0) {
                return Err(ConfigError::Parse {
                    path: path.to_path_buf(),
                    message: "invalid [rules.orphans].confirm_batch (expected a positive integer)"
                        .into(),
                });
            }
            Ok(config)
        }
        v => Err(ConfigError::Parse {
            path: path.to_path_buf(),
            message: format!("invalid preset '{v}' (expected: safe, regular, full)"),
        }),
    }
}

fn validate_rule_sections(path: &Path, document: &toml::Table) -> Result<(), ConfigError> {
    let Some(rules) = document.get("rules").and_then(toml::Value::as_table) else {
        return Ok(());
    };

    for (rule, value) in rules {
        let Some(fields) = rule_fields(rule) else {
            let known_ids: Vec<_> = crate::rules::registry()
                .iter()
                .map(|rule| rule.id())
                .collect();
            let suggestion = closest_match(rule, &known_ids);
            return Err(ConfigError::Parse {
                path: path.to_path_buf(),
                message: match suggestion {
                    Some(candidate) => {
                        format!("unknown [rules] section '{rule}'; did you mean '{candidate}'?")
                    }
                    None => format!("unknown [rules] section '{rule}'"),
                },
            });
        };
        let Some(section) = value.as_table() else {
            continue;
        };
        for field in section.keys() {
            if fields.contains(&field.as_str()) {
                continue;
            }
            let suggestion = closest_match(field, fields);
            return Err(ConfigError::Parse {
                path: path.to_path_buf(),
                message: match suggestion {
                    Some(candidate) => {
                        format!("unknown key '[rules.{rule}].{field}'; did you mean '{candidate}'?")
                    }
                    None => format!("unknown key '[rules.{rule}].{field}'"),
                },
            });
        }
    }
    Ok(())
}

fn validate_preset_sections(path: &Path, document: &toml::Table) -> Result<(), ConfigError> {
    let Some(presets) = document.get("presets").and_then(toml::Value::as_table) else {
        return Ok(());
    };
    const PRESET_NAMES: &[&str] = &["safe", "regular", "full"];
    const PRESET_FIELDS: &[&str] = &["include", "exclude"];

    for (name, value) in presets {
        if !PRESET_NAMES.contains(&name.as_str()) {
            let suggestion = closest_match(name, PRESET_NAMES);
            return Err(ConfigError::Parse {
                path: path.to_path_buf(),
                message: match suggestion {
                    Some(candidate) => {
                        format!("unknown [presets] section '{name}'; did you mean '{candidate}'?")
                    }
                    None => format!("unknown [presets] section '{name}'"),
                },
            });
        }
        let Some(section) = value.as_table() else {
            continue;
        };
        for field in section.keys() {
            if PRESET_FIELDS.contains(&field.as_str()) {
                continue;
            }
            let suggestion = closest_match(field, PRESET_FIELDS);
            return Err(ConfigError::Parse {
                path: path.to_path_buf(),
                message: match suggestion {
                    Some(candidate) => format!(
                        "unknown key '[presets.{name}].{field}'; did you mean '{candidate}'?"
                    ),
                    None => format!("unknown key '[presets.{name}].{field}'"),
                },
            });
        }
    }
    Ok(())
}

fn validate_preset_rule_ids(path: &Path, config: &Config) -> Result<(), ConfigError> {
    let registry = crate::rules::registry();
    let known_ids: Vec<_> = registry.iter().map(|rule| rule.id()).collect();
    for (preset, values) in [
        ("safe", &config.presets.safe),
        ("regular", &config.presets.regular),
        ("full", &config.presets.full),
    ] {
        for (key, ids) in [("include", &values.include), ("exclude", &values.exclude)] {
            for id in ids {
                if known_ids.contains(&id.as_str()) {
                    continue;
                }
                let suggestion = closest_match(id, &known_ids);
                return Err(ConfigError::Parse {
                    path: path.to_path_buf(),
                    message: match suggestion {
                        Some(candidate) => format!(
                            "unknown rule '{id}' in [presets.{preset}].{key}; \
                             did you mean '{candidate}'?"
                        ),
                        None => format!("unknown rule '{id}' in [presets.{preset}].{key}"),
                    },
                });
            }
        }
    }
    Ok(())
}

fn rule_fields(rule: &str) -> Option<&'static [&'static str]> {
    const TOGGLE: &[&str] = &["enabled"];
    const USER_CACHE: &[&str] = &["enabled", "keep"];
    const JOURNAL_USER: &[&str] = &["enabled", "max_size"];
    const TOOL_CACHES: &[&str] = &["enabled", "disable_tools"];
    const APP_CACHES: &[&str] = &["enabled", "extra_paths"];
    const PACMAN_CACHE: &[&str] = &["enabled", "keep_versions"];
    const JOURNAL_SYSTEM: &[&str] = &["enabled", "max_size"];
    const AUR_BUILDS: &[&str] = &["enabled", "keep_days"];
    const ORPHANS: &[&str] = &["enabled", "confirm_batch", "exclude"];
    const DEV_ARTIFACTS: &[&str] = &["enabled", "min_age_days", "paths"];
    const DOWNLOADS: &[&str] = &["enabled", "keep_days"];

    Some(match rule {
        "user-cache" => USER_CACHE,
        "journal-user" => JOURNAL_USER,
        "tool-caches" => TOOL_CACHES,
        "app-caches" => APP_CACHES,
        "pacman-cache" => PACMAN_CACHE,
        "journal-system" => JOURNAL_SYSTEM,
        "aur-builds" => AUR_BUILDS,
        "orphans" => ORPHANS,
        "dev-artifacts" => DEV_ARTIFACTS,
        "downloads" => DOWNLOADS,
        "tmp" | "thumbnails" | "trash" | "browser-cache" | "editor-cache" | "log-rotate"
        | "xdg-state-junk" | "flatpak-unused" | "docker" | "podman" | "steam-shader"
        | "snapshots-report" | "cache-deep" | "tmp-full" | "pkgcache-zero" | "purge-logs" => TOGGLE,
        _ => return None,
    })
}

fn closest_match<'a>(input: &str, candidates: &'a [&str]) -> Option<&'a str> {
    let (candidate, distance) = candidates
        .iter()
        .map(|candidate| (*candidate, edit_distance(input, candidate)))
        .min_by_key(|(_, distance)| *distance)?;
    let threshold = (input.chars().count() / 3).max(2);
    (distance <= threshold).then_some(candidate)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<_> = right.chars().collect();
    let mut row: Vec<_> = (0..=right.len()).collect();
    for (i, left_char) in left.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, right_char) in right.iter().enumerate() {
            let previous = row[j + 1];
            row[j + 1] = (row[j + 1] + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(left_char != *right_char));
            diagonal = previous;
        }
    }
    row[right.len()]
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
    fn invalid_system_journal_size_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write(
            &dir,
            "config.toml",
            "[rules.journal-system]\nmax_size = \"100M --vacuum-time=1s\"\n",
        );
        let error = load_from(Some(&p)).expect_err("invalid vacuum size");
        assert!(
            error
                .to_string()
                .contains("invalid [rules.journal-system].max_size")
        );
    }

    #[test]
    fn orphan_confirmation_batch_must_be_positive() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write(&dir, "config.toml", "[rules.orphans]\nconfirm_batch = 0\n");
        let error = load_from(Some(&p)).expect_err("zero-sized confirmation page");
        assert!(
            error
                .to_string()
                .contains("invalid [rules.orphans].confirm_batch")
        );
    }

    #[test]
    fn unknown_top_level_sections_remain_forward_compatible() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write(&dir, "config.toml", "[future-section]\nunknown = true\n");
        let c = load_from(Some(&p)).expect("parse");
        assert!(c.rules.tmp.enabled);
    }

    #[test]
    fn unknown_rule_section_suggests_known_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write(&dir, "config.toml", "[rules.thumbnail]\nenabled = false\n");
        let error = load_from(Some(&p)).expect_err("unknown rule section");
        assert!(
            error
                .to_string()
                .contains("unknown [rules] section 'thumbnail'; did you mean 'thumbnails'?")
        );
    }

    #[test]
    fn unknown_rule_key_suggests_known_field() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write(&dir, "config.toml", "[rules.tmp]\nenable = false\n");
        let error = load_from(Some(&p)).expect_err("unknown rule key");
        assert!(
            error
                .to_string()
                .contains("unknown key '[rules.tmp].enable'; did you mean 'enabled'?")
        );
    }

    #[test]
    fn parses_preset_include_and_exclude() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write(
            &dir,
            "config.toml",
            "[presets.regular]\ninclude = [\"docker\"]\nexclude = [\"tmp\"]\n",
        );
        let config = load_from(Some(&p)).expect("valid preset lists");
        assert_eq!(config.presets.regular.include, ["docker"]);
        assert_eq!(config.presets.regular.exclude, ["tmp"]);
    }

    #[test]
    fn unknown_preset_rule_id_suggests_known_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = write(
            &dir,
            "config.toml",
            "[presets.regular]\ninclude = [\"thumbnail\"]\n",
        );
        let error = load_from(Some(&p)).expect_err("unknown preset rule id");
        assert!(error.to_string().contains(
            "unknown rule 'thumbnail' in [presets.regular].include; did you mean 'thumbnails'?"
        ));
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
