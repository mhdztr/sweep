//! Sandbox scaffold validation (own process: `common::sandbox()` mutates env).

mod common;

use common::sandbox;

#[test]
fn sandbox_provides_guarded_temp_home() {
    let sb = sandbox();
    sb.seed_file(".cache/fixture.txt", b"hello");

    assert!(sb.path().is_dir());
    assert!(sb.path().join(".cache/fixture.txt").is_file());

    // Under the guard, resolution must return the sandbox — never the real home.
    let got = sweep::env::resolve_home().expect("guarded resolve");
    assert_eq!(
        got.canonicalize().expect("canonical got"),
        sb.path().canonicalize().expect("canonical sandbox")
    );
}

// SAFETY note: `sandbox()` sets env vars under a process-local lock; this file
// contains no other env-mutating tests.
