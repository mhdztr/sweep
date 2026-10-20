//! Command-line surface. Spec of record: `docs/CLI-UX.md`.

pub mod scan;

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Advanced, safety-first storage cleaner for Linux.
///
/// No subcommand behaves like `sweep clean` (interactive; requires a TTY).
#[derive(Debug, Parser)]
#[command(
    name = "sweep",
    version,
    about = "Advanced, safety-first storage cleaner for Linux",
    long_about = "scan → select categories in a checklist → gated cleanup.\n\
                  Arch-first (pacman/AUR/orphans/journal/snapper), distro-aware everywhere.\n\
                  Nothing is deleted without showing size, count and tier first.",
    propagate_version = true
)]
pub struct Root {
    #[command(subcommand)]
    pub command: Option<Commands>,

    // ---- global flags (apply to every command; spec: docs/CLI-UX.md §1) ----
    /// Preview only: show sample paths and would-run commands, change nothing
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Machine-readable JSON on stdout (human output goes to stderr)
    #[arg(long, global = true)]
    pub json: bool,

    /// Disable ANSI colors (NO_COLOR is honored too)
    #[arg(long, global = true)]
    pub no_color: bool,

    /// ASCII output: box chars and checkboxes instead of unicode
    #[arg(long, global = true)]
    pub no_unicode: bool,

    /// Age gate override: keep files newer than N days
    #[arg(long, global = true, value_name = "N")]
    pub keep_days: Option<u32>,

    /// Stage deletions into the XDG trash instead of hard-deleting
    #[arg(long, global = true)]
    pub trash: bool,

    /// Auto-confirm safe & caution prompts (never destructive ones)
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,

    /// The only bypass of the typed PURGE gate (scripted full purges)
    #[arg(long, global = true, hide = true)]
    pub i_know_what_im_doing: bool,

    /// Selection preset: safe | regular | full
    #[arg(long, global = true, value_enum)]
    pub preset: Option<Preset>,

    /// Verbose output (argv, errno context, per-path samples)
    #[arg(long, short, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Only errors and the final summary line
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Read-only scan: measure reclaimable space (never changes anything)
    Scan,

    /// Scan + interactive checklist + gated cleanup (default flow)
    Clean {
        /// Only these rule ids (comma-separated)
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,

        /// Never select these rule ids (comma-separated)
        #[arg(long, value_delimiter = ',')]
        exclude: Vec<String>,

        /// Scripted mode: no prompts (implies --yes for safe/caution)
        #[arg(long)]
        no_prompt: bool,
    },

    /// Biggest files/directories report
    Top {
        /// Root to measure (default: home)
        #[arg(long, default_value = "~")]
        path: String,

        /// Report directories (default mode)
        #[arg(long, conflicts_with = "file")]
        dir: bool,

        /// Report individual files
        #[arg(long)]
        file: bool,

        /// Directory report depth (default 3)
        #[arg(long, default_value_t = 3)]
        depth: u8,

        /// Minimum size (human: 100M, 2G, …)
        #[arg(long, default_value = "100M")]
        min_size: String,

        /// Max entries to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    /// History of past runs (what was cleaned, how much freed)
    History {
        /// Show only the last N runs
        #[arg(long, default_value_t = 20)]
        last: usize,

        /// Also list audit log files
        #[arg(long)]
        logs: bool,
    },

    /// Environment diagnostics: distro, tools, permissions, active rules
    Doctor,

    /// Inspect the rule catalog (must match docs/RULES-CATALOG.md)
    Rules {
        #[command(subcommand)]
        command: RulesCommand,
    },

    /// Configuration file management
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// Generate shell completions
    Completion {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// (internal) batched sudo system pass, invoked by sweep itself
    #[command(name = "internal-system-pass", hide = true)]
    InternalSystemPass {
        #[arg(value_name = "PLAN_PATH")]
        path: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum RulesCommand {
    /// List every rule with tier, sudo, default and live state
    List {
        /// Filter by tier
        #[arg(long, value_enum)]
        tier: Option<TierArg>,
        /// JSON output
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Write the annotated default config (refuses to overwrite)
    Init {
        /// Overwrite an existing config
        #[arg(long)]
        force: bool,
    },
    /// Print the resolved config path
    Path,
    /// Open the config in $EDITOR (prints the path if no editor set)
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Preset {
    /// Only always-safe categories
    Safe,
    /// Safe categories + your config includes (default)
    Regular,
    /// Everything, destructive unlocked behind typed PURGE
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TierArg {
    Safe,
    Caution,
    Destructive,
    Report,
}

impl Root {
    /// The effective preset: explicit flag wins, else config default (later), else regular.
    pub fn effective_preset(&self) -> Preset {
        self.preset.unwrap_or(Preset::Regular)
    }
}
