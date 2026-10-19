//! `sweep scan` — read-only measurement. Spec: plan §2.16, `docs/CLI-UX.md` §2.1.
//!
//! **Render order (D-1 deviation):** the spinner runs on stderr DURING the
//! scan and is cleared before anything else prints — the CLI-UX §2.1 mock
//! shows the header box first; we intentionally do not (a scan can take
//! seconds; the header must not scroll away above a live progress bar).
//! Order: spinner → header → table → (`-v` detail lines) → footer.

use crate::cli::Root;
use crate::ctx::Ctx;
use crate::scan::{self, ScanEvent};
use std::io::IsTerminal as _;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};

pub fn run(root: Root) -> i32 {
    let ctx = match Ctx::build(&root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    // No SIGINT handler yet — cooperative cancel lands with the M2 exec
    // milestone; M1 scan still threads the flag through for symmetry.
    let cancel = Arc::new(AtomicBool::new(false));

    // D-1 spinner only on a real stderr, never with --json/--quiet.
    let show_spinner = !ctx.flags.json && !ctx.flags.quiet && std::io::stderr().is_terminal();
    let (tx, rx) = mpsc::channel::<ScanEvent>();
    let spinner = crate::ui::progress::ScanSpinner::spawn(
        crate::rules::registry().len(),
        rx,
        show_spinner,
        ctx.flags.unicode,
    );
    // `tx` moves into scan::run; the ScanService (owner of the sender) drops
    // when run() returns, closing the channel — the spinner thread then
    // exits and clears its line before we print anything.
    let out = scan::run(&ctx, cancel, Some(tx));
    let _ = spinner.join();

    if ctx.flags.json {
        // D-3: --json is stdout-only — a single JSON doc, nothing else.
        let report = crate::report::ScanReport::from_scan(&ctx, &out, ctx.flags.verbose > 0);
        let stdout = std::io::stdout();
        if serde_json::to_writer(stdout.lock(), &report).is_err() {
            eprintln!("error: failed to write JSON report");
            return 1;
        }
        println!();
        return 0;
    }

    let theme = crate::ui::Theme {
        color: ctx.flags.color,
        unicode: ctx.flags.unicode,
    };
    if ctx.flags.quiet {
        // D-4: --quiet = stdout-only footer (no header, no table).
        println!(
            "{}",
            crate::ui::render_footer(
                out.totals.reclaimable_bytes,
                out.totals.regular_bytes,
                out.totals.scan_ms,
                &theme,
            )
        );
        return 0;
    }

    let width = crate::ui::terminal_width();
    print!(
        "{}",
        crate::ui::render_header(&ctx, out.totals.reclaimable_bytes, width, &theme)
    );
    print!(
        "{}",
        crate::ui::table::render_scan_table(&out.results, width, &theme, ctx.flags.verbose > 0,)
    );
    println!(
        "{}",
        crate::ui::render_footer(
            out.totals.reclaimable_bytes,
            out.totals.regular_bytes,
            out.totals.scan_ms,
            &theme,
        )
    );
    0
}
