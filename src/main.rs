//! sweep CLI entry point.
//!
//! M0: full command surface parses; commands land milestone by milestone
//! (see ROADMAP.md). Everything here must stay thin: parse → dispatch → exit code.

use clap::{CommandFactory, Parser};
use std::io::Write as _;
use sweep::cli::{Commands, ConfigCommand, Root, RulesCommand};

fn main() {
    let root = Root::parse();
    let code = dispatch(root);
    std::process::exit(code);
}

fn dispatch(mut root: Root) -> i32 {
    // take() (not unwrap_or) leaves `root` fully intact, the scan arm needs
    // the global flags on it (`Ctx::build(&root)` reads them).
    let cmd = root.command.take().unwrap_or(Commands::Clean {
        only: Vec::new(),
        exclude: Vec::new(),
        no_prompt: false,
    });

    match cmd {
        Commands::Completion { shell } => {
            let mut out = std::io::stdout();
            clap_complete::generate(shell, &mut Root::command(), "sweep", &mut out);
            let _ = out.flush();
            0
        }

        Commands::InternalSystemPass { path } => {
            // Lands in M5 (batched sudo pass). Never trust a plan silently.
            eprintln!(
                "error: internal-system-pass is not available yet (M5): {}",
                path.display()
            );
            1
        }

        Commands::Scan => sweep::cli::scan::run(root),
        Commands::Clean { .. } => not_yet("clean", "M2: interactive clean"),
        Commands::Top { .. } => not_yet("top", "M6: biggest-dirs report"),
        Commands::History { .. } => not_yet("history", "M4: run history"),
        Commands::Doctor => not_yet("doctor", "M5: environment diagnostics"),
        Commands::Rules {
            command: RulesCommand::List { .. },
        } => not_yet("rules list", "M3: rule catalog registry"),
        Commands::Config {
            command: ConfigCommand::Init { .. },
        } => not_yet("config init", "M6: configuration"),
        Commands::Config {
            command: ConfigCommand::Path,
        } => not_yet("config path", "M6: configuration"),
        Commands::Config {
            command: ConfigCommand::Edit,
        } => not_yet("config edit", "M6: configuration"),
    }
}

fn not_yet(cmd: &str, milestone: &str) -> i32 {
    eprintln!("error: `sweep {cmd}` is not implemented yet: lands in {milestone}");
    eprintln!("hint: `sweep --version` and `sweep completion <shell>` work today; see ROADMAP.md");
    1
}
