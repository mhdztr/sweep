//! Golden render snapshots (plan §4 scan_golden): header + table + footer
//! at five injected width/theme combinations.
//!
//! Determinism: fixture-driven sizes, `Ctx::for_test` host fields, fixed
//! `scan_ms` (410), `display_path` locations (home-relative, never an
//! absolute tempdir path); non-verbose output renders no ages. Snapshots are
//! created deliberately — run, INSPECT each `.snap.new`, then accept via
//! `cargo insta review` / rename (never blind-accept; AGENTS.md).

mod common;

use sweep::ctx::Ctx;
use sweep::ui::table::render_scan_table;
use sweep::ui::{Theme, render_footer, render_header};

/// Full non-verbose render (header → table → footer), as `cli/scan.rs`
/// prints it, with injected width/theme and a pinned `scan_ms`.
fn render(width: usize, theme: Theme) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    common::scan_fixture(dir.path());
    let ctx = Ctx::for_test(dir.path().to_path_buf());
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut out = sweep::scan::run(&ctx, cancel, None);
    out.totals.scan_ms = 410; // pinned for snapshot determinism
    // No path sanitization: every `location` is rendered through
    // `display_path(home, …)`, so fixture paths come out as `~/…` — content
    // and column widths are deterministic WITHOUT rewriting anything (the
    // old `<HOME>` replace existed to mask the tmp rule's joined non-spec
    // "/tmp, /var/tmp"-style location; that string is gone by design). A
    // leaked absolute tempdir path would now FAIL loudly instead of being
    // normalized away.

    let mut s = String::new();
    s.push_str(&render_header(
        &ctx,
        out.totals.reclaimable_bytes,
        width,
        &theme,
    ));
    s.push_str(&render_scan_table(&out.results, width, &theme, false));
    s.push_str(&render_footer(
        out.totals.reclaimable_bytes,
        out.totals.regular_bytes,
        out.totals.scan_ms,
        &theme,
    ));
    s.push('\n');
    s
}

#[test]
fn scan_table_wide_color() {
    insta::assert_snapshot!(render(
        120,
        Theme {
            color: true,
            unicode: true
        }
    ));
}

#[test]
fn scan_table_narrow_color() {
    insta::assert_snapshot!(render(
        80,
        Theme {
            color: true,
            unicode: true
        }
    ));
}

#[test]
fn scan_table_ascii() {
    // ASCII borders/glyphs; colors still on to prove the two axes compose.
    insta::assert_snapshot!(render(
        120,
        Theme {
            color: true,
            unicode: false
        }
    ));
}

#[test]
fn scan_table_no_color() {
    insta::assert_snapshot!(render(
        120,
        Theme {
            color: false,
            unicode: true
        }
    ));
}

#[test]
fn scan_table_compact_no_color() {
    // < 60 cols: two-line compact rows, no border box.
    insta::assert_snapshot!(render(
        50,
        Theme {
            color: false,
            unicode: true
        }
    ));
}
