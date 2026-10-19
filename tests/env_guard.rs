//! SWEEP_TEST_GUARD contract, integration level.
//! Lives in its own test file (own process): it mutates process-global env.

#[test]
fn home_guard_refuses_real_home_when_test_guard_active() {
    let real = dirs::home_dir().expect("home dir");

    // SAFETY: this test is the only env-mutating test in this process binary.
    unsafe {
        std::env::set_var("SWEEP_TEST_GUARD", "1");
        std::env::remove_var("SWEEP_HOME");
    }
    let err = sweep::env::resolve_home().expect_err("guard must refuse without override");
    assert!(err.to_string().contains("SWEEP_TEST_GUARD"));

    unsafe { std::env::set_var("SWEEP_HOME", &real) };
    let err = sweep::env::resolve_home().expect_err("guard must refuse real home");
    assert!(err.to_string().contains("real home"));

    let tmp = tempfile::tempdir().expect("tempdir");
    unsafe { std::env::set_var("SWEEP_HOME", tmp.path()) };
    let got = sweep::env::resolve_home().expect("sandbox override accepted");
    assert_eq!(
        got.canonicalize().expect("canonical got"),
        tmp.path().canonicalize().expect("canonical tmp")
    );

    unsafe {
        std::env::remove_var("SWEEP_HOME");
        std::env::remove_var("SWEEP_TEST_GUARD");
    }
}
