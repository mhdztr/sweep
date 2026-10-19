//! Runtime context: resolved flags + config + system info + run id.
//!
//! `Ctx::build` NEVER creates directories and NEVER acquires the lock —
//! `scan` must mutate nothing (AGENTS.md safety invariant 2).

use crate::cli::{Preset, Root};
use crate::config::{self, Config, ConfigError};
use crate::system::SystemCtx;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

/// Resolved global flags. `color`/`unicode` are *resolved* booleans (env and
/// TTY state folded in at build time); raw overrides live alongside for
/// precedence diagnostics.
#[derive(Debug, Clone)]
pub struct GlobalFlags {
    pub dry_run: bool,
    pub json: bool,
    /// RESOLVED: NO_COLOR(non-empty) or --no-color ⇒ false; FORCE_COLOR
    /// (non-empty) && !NO_COLOR ⇒ true; TERM=="dumb" ⇒ false; else
    /// stdout.is_terminal().
    pub color: bool,
    /// RESOLVED: --no-unicode or SWEEP_NO_UNICODE(non-empty) ⇒ false; else
    /// LC_ALL or LANG exactly "C"/"POSIX" ⇒ false; else true.
    pub unicode: bool,
    /// Raw override: --keep-days > env SWEEP_KEEP_DAYS; None ⇒ config.
    pub keep_days: Option<u32>,
    pub trash: bool,
    pub yes: bool,
    pub i_know_what_im_doing: bool,
    /// --preset > env SWEEP_PRESET; invalid env ⇒ CtxError::Flags.
    pub preset: Option<Preset>,
    pub verbose: u8,
    pub quiet: bool,
}

impl GlobalFlags {
    /// Reads env mirrors: SWEEP_KEEP_DAYS (invalid ⇒ `CtxError::Flags(
    /// "invalid SWEEP_KEEP_DAYS '{v}' (expected a non-negative integer)")`),
    /// SWEEP_PRESET, SWEEP_TRASH=1, SWEEP_YES=1, SWEEP_VERBOSE (parses u8,
    /// else 1 when non-empty), NO_COLOR, FORCE_COLOR, SWEEP_NO_UNICODE,
    /// LC_ALL, LANG, TERM.
    ///
    /// Note (plan §7 risk 9): env precedence is deliberately untested in M1
    /// (would need process-env mutation); precedence matrix is an M6 task.
    pub fn from_root(root: &Root) -> Result<Self, CtxError> {
        use std::io::IsTerminal as _;

        let keep_days = match root.keep_days {
            Some(v) => Some(v),
            None => match std::env::var("SWEEP_KEEP_DAYS") {
                Ok(v) if !v.trim().is_empty() => Some(v.trim().parse::<u32>().map_err(|_| {
                    CtxError::Flags(format!(
                        "invalid SWEEP_KEEP_DAYS '{v}' (expected a non-negative integer)"
                    ))
                })?),
                _ => None,
            },
        };

        let preset = match root.preset {
            Some(p) => Some(p),
            None => match std::env::var("SWEEP_PRESET") {
                Ok(v) if !v.trim().is_empty() => {
                    let v = v.trim();
                    match v {
                        "safe" => Some(Preset::Safe),
                        "regular" => Some(Preset::Regular),
                        "full" => Some(Preset::Full),
                        _ => {
                            return Err(CtxError::Flags(format!(
                                "invalid preset '{v}' (expected: safe, regular, full)"
                            )));
                        }
                    }
                }
                _ => None,
            },
        };

        let env_flag = |key: &str| std::env::var_os(key).is_some_and(|v| !v.is_empty());

        let trash = root.trash || env_flag("SWEEP_TRASH");
        let yes = root.yes || env_flag("SWEEP_YES");
        let verbose = if root.verbose > 0 {
            root.verbose
        } else if env_flag("SWEEP_VERBOSE") {
            std::env::var("SWEEP_VERBOSE")
                .ok()
                .and_then(|v| v.trim().parse::<u8>().ok())
                .unwrap_or(1)
        } else {
            0
        };

        let no_color = root.no_color || env_flag("NO_COLOR");
        let force_color = env_flag("FORCE_COLOR") && !env_flag("NO_COLOR");
        let term_dumb = std::env::var("TERM").is_ok_and(|t| t == "dumb");
        let color = if no_color {
            false
        } else if force_color {
            true
        } else if term_dumb {
            false
        } else {
            std::io::stdout().is_terminal()
        };

        let c_locale = |key: &str| std::env::var(key).is_ok_and(|v| v == "C" || v == "POSIX");
        let unicode = !(root.no_unicode
            || env_flag("SWEEP_NO_UNICODE")
            || c_locale("LC_ALL")
            || c_locale("LANG"));

        Ok(GlobalFlags {
            dry_run: root.dry_run,
            json: root.json,
            color,
            unicode,
            keep_days,
            trash,
            yes,
            i_know_what_im_doing: root.i_know_what_im_doing,
            preset,
            verbose,
            quiet: root.quiet,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CtxError {
    #[error(transparent)]
    Env(#[from] crate::env::EnvError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("{0}")]
    Flags(String),
    #[error("cannot detect system info: {0}")]
    System(String),
}

#[derive(Debug, Clone)]
pub struct Ctx {
    pub home: PathBuf,
    pub config: Config,
    pub system: SystemCtx,
    pub flags: GlobalFlags,
    /// `state::run_id_now()`.
    pub run_id: String,
    /// CLI: `["/tmp", "/var/tmp"]`; for_test: `[home/".fake-tmp",
    /// home/".fake-var-tmp"]`.
    pub tmp_roots: Vec<PathBuf>,
}

impl Ctx {
    /// CLI path: resolve_home() → GlobalFlags::from_root →
    /// config::load_from(env SWEEP_CONFIG or default_path()) →
    /// SystemCtx::detect → run_id → tmp_roots. NEVER creates directories,
    /// NEVER acquires the lock (scan must mutate nothing).
    pub fn build(root: &Root) -> Result<Ctx, CtxError> {
        let home = crate::env::resolve_home()?;
        let flags = GlobalFlags::from_root(root)?;
        let config_path = match std::env::var_os("SWEEP_CONFIG") {
            Some(v) if !v.is_empty() => Some(PathBuf::from(v)),
            // Default path: absent file ⇒ the "defaults-only path"
            // (`load_from(None)`); only an EXPLICIT SWEEP_CONFIG that is
            // missing is an error (`ConfigError::Missing`). Takes `home` so
            // a SWEEP_HOME sandbox resolves to ITS config, never the
            // developer's real one (AGENTS invariant 4).
            _ => config::default_path(&home).filter(|p| p.exists()),
        };
        let config = config::load_from(config_path.as_deref())?;
        let system = SystemCtx::detect().map_err(|e| CtxError::System(e.to_string()))?;
        let run_id = crate::state::run_id_now();
        Ok(Ctx {
            home,
            config,
            system,
            flags,
            run_id,
            tmp_roots: vec![PathBuf::from("/tmp"), PathBuf::from("/var/tmp")],
        })
    }

    /// Deterministic test constructor (plan §2.6). Fields are pub so tests
    /// can override (e.g. `config.rules.user_cache.keep`).
    pub fn for_test(home: PathBuf) -> Ctx {
        Ctx {
            tmp_roots: vec![home.join(".fake-tmp"), home.join(".fake-var-tmp")],
            home,
            config: Config::default(),
            system: SystemCtx::for_test(),
            flags: GlobalFlags {
                dry_run: false,
                json: false,
                color: false,
                unicode: true,
                keep_days: None,
                trash: false,
                yes: false,
                i_know_what_im_doing: false,
                preset: None,
                verbose: 0,
                quiet: false,
            },
            run_id: "19700101T000000".to_string(),
        }
    }

    pub fn effective_preset(&self) -> Preset {
        self.flags
            .preset
            .unwrap_or(match self.config.general.preset.as_str() {
                "safe" => Preset::Safe,
                "full" => Preset::Full,
                // validated at config load; default "regular"
                _ => Preset::Regular,
            })
    }

    pub fn preset_name(&self) -> &'static str {
        match self.effective_preset() {
            Preset::Safe => "safe",
            Preset::Regular => "regular",
            Preset::Full => "full",
        }
    }

    pub fn keep_days(&self) -> u32 {
        self.flags
            .keep_days
            .unwrap_or(self.config.general.keep_days)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_test_is_deterministic() {
        let home = PathBuf::from("/tmp/sweep-ctx-test-home");
        let ctx = Ctx::for_test(home.clone());
        assert_eq!(ctx.home, home);
        assert_eq!(
            ctx.tmp_roots,
            vec![home.join(".fake-tmp"), home.join(".fake-var-tmp")]
        );
        assert_eq!(ctx.run_id, "19700101T000000");
        assert_eq!(ctx.flags.keep_days, None);
        assert!(ctx.flags.unicode);
        assert!(!ctx.flags.color);
    }

    #[test]
    fn preset_and_keep_days_resolution() {
        let mut ctx = Ctx::for_test(PathBuf::from("/tmp/x"));
        assert_eq!(ctx.effective_preset(), Preset::Regular);
        assert_eq!(ctx.preset_name(), "regular");
        assert_eq!(ctx.keep_days(), 1);
        ctx.flags.preset = Some(Preset::Safe);
        ctx.flags.keep_days = Some(0);
        assert_eq!(ctx.effective_preset(), Preset::Safe);
        assert_eq!(ctx.keep_days(), 0);
        ctx.config.general.preset = "full".to_string();
        ctx.flags.preset = None;
        assert_eq!(ctx.effective_preset(), Preset::Full);
    }
}
