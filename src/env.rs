//! Environment resolution: home override + test-guard refusal logic.
//!
//! Every `$HOME`-relative path in the codebase must resolve through
//! [`resolve_home`] (fed into `Ctx::home`), this is what makes rule tests
//! runnable against sandboxed temp homes and what makes them unable to touch
//! the real home by accident.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum EnvError {
    #[error("SWEEP_HOME is set but is not a directory: {0}")]
    NotADirectory(PathBuf),

    #[error(
        "refusing to run: SWEEP_TEST_GUARD is set but no sandboxed SWEEP_HOME was \
         provided (would operate on the real home directory {0})"
    )]
    GuardNoOverride(PathBuf),

    #[error(
        "refusing to run: SWEEP_TEST_GUARD is set but SWEEP_HOME resolves to the \
         real home directory ({0})"
    )]
    GuardRealHome(PathBuf),
}

/// Resolve the home directory sweep operates on.
///
/// * `SWEEP_HOME`, test/container override; all rule paths derive from it.
/// * `SWEEP_TEST_GUARD`, when set (by the test harness), refuse to resolve to
///   the real home directory under any circumstance.
pub fn resolve_home() -> Result<PathBuf, EnvError> {
    let real = real_home();

    match std::env::var_os("SWEEP_HOME") {
        Some(override_val) => {
            let path = PathBuf::from(override_val);
            if !path.is_dir() {
                return Err(EnvError::NotADirectory(path));
            }
            if guard_active() && same_path(&path, &real) {
                return Err(EnvError::GuardRealHome(path));
            }
            Ok(path)
        }
        None => {
            if guard_active() {
                return Err(EnvError::GuardNoOverride(real));
            }
            Ok(real)
        }
    }
}

fn guard_active() -> bool {
    std::env::var_os("SWEEP_TEST_GUARD").is_some()
}

fn real_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

fn same_path(a: &Path, b: &Path) -> bool {
    // Best effort: canonicalize when possible, compare bytes otherwise.
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env vars are process-global: serialize these tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // SAFETY: every env mutation in this module runs while ENV_LOCK is held,
    // inside a test binary where all env-touching tests share that lock.
    fn set_env(key: &str, value: impl AsRef<std::ffi::OsStr>) {
        unsafe { std::env::set_var(key, value) }
    }
    fn remove_env(key: &str) {
        unsafe { std::env::remove_var(key) }
    }

    #[test]
    fn plain_resolution_returns_some_dir() {
        let _g = ENV_LOCK.lock().expect("env lock");
        remove_env("SWEEP_HOME");
        remove_env("SWEEP_TEST_GUARD");
        let home = resolve_home().expect("plain resolve");
        assert!(home.is_absolute());
    }

    #[test]
    fn override_must_be_directory() {
        let _g = ENV_LOCK.lock().expect("env lock");
        set_env("SWEEP_HOME", "/definitely/not/a/dir/sweep-test");
        remove_env("SWEEP_TEST_GUARD");
        assert!(matches!(resolve_home(), Err(EnvError::NotADirectory(_))));
        remove_env("SWEEP_HOME");
    }

    #[test]
    fn guard_refuses_without_override() {
        let _g = ENV_LOCK.lock().expect("env lock");
        remove_env("SWEEP_HOME");
        set_env("SWEEP_TEST_GUARD", "1");
        assert!(matches!(resolve_home(), Err(EnvError::GuardNoOverride(_))));
        remove_env("SWEEP_TEST_GUARD");
    }

    #[test]
    fn guard_refuses_real_home_override() {
        let _g = ENV_LOCK.lock().expect("env lock");
        let real = dirs::home_dir().expect("home");
        set_env("SWEEP_HOME", &real);
        set_env("SWEEP_TEST_GUARD", "1");
        assert!(matches!(resolve_home(), Err(EnvError::GuardRealHome(_))));
        remove_env("SWEEP_HOME");
        remove_env("SWEEP_TEST_GUARD");
    }

    #[test]
    fn guard_accepts_sandbox_override() {
        let _g = ENV_LOCK.lock().expect("env lock");
        let tmp = tempfile::tempdir().expect("tempdir");
        set_env("SWEEP_HOME", tmp.path());
        set_env("SWEEP_TEST_GUARD", "1");
        let got = resolve_home().expect("sandbox resolve");
        assert!(same_path(&got, tmp.path()));
        remove_env("SWEEP_HOME");
        remove_env("SWEEP_TEST_GUARD");
    }
}
