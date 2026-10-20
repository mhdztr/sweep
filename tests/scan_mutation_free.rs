//! Safety invariant 2 (AGENTS.md) / plan §4: `scan` mutates nothing;
//! library and CLI level, proven with a before/after filesystem manifest.
//!
//! No process-env mutation here: the library test uses `Ctx::for_test(home)`
//! and the CLI test passes env to the child process only (plan §4 pattern).

mod common;

use sweep::ctx::Ctx;

#[test]
fn library_scan_leaves_fixture_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    common::scan_fixture(dir.path());
    let before = common::manifest(dir.path());

    let ctx = Ctx::for_test(dir.path().to_path_buf());
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let out = sweep::scan::run(&ctx, cancel, None);
    assert_eq!(
        out.totals.reclaimable_bytes, 20_602,
        "golden fixture total (plan §4)"
    );

    let after = common::manifest(dir.path());
    assert_eq!(
        before, after,
        "library scan must not create/modify/delete anything"
    );
}

#[test]
fn cli_scan_creates_no_state_dir_or_lock() {
    let dir = tempfile::tempdir().expect("tempdir");
    common::scan_fixture(dir.path());
    let before = common::manifest(dir.path());
    let state = dir.path().join(".state");

    assert_cmd::Command::cargo_bin("sweep")
        .expect("cargo_bin")
        .arg("scan")
        .env("SWEEP_HOME", dir.path())
        .env("SWEEP_TEST_GUARD", "1")
        .env("SWEEP_STATE_DIR", &state)
        .assert()
        .success();

    assert!(!state.exists(), "scan must not create the state dir");
    assert_eq!(
        before,
        common::manifest(dir.path()),
        "CLI scan must not create/modify/delete anything under the home"
    );
}
