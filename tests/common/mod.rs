//! Shared test helpers — sandboxed homes for rule tests.
//!
//! Contract (see AGENTS.md): every rule test runs against a temp home via
//! `SWEEP_HOME` with `SWEEP_TEST_GUARD=1`, which makes `env::resolve_home()`
//! refuse real-home resolution. Env vars are process-global: `sandbox()` holds
//! a lock for its whole lifetime, so tests using it are serialized with each
//! other.
#![allow(dead_code)] // scaffold: helpers are consumed starting M1

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

pub struct Sandbox {
    dir: tempfile::TempDir,
    _guard: MutexGuard<'static, ()>,
}

impl Sandbox {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Seed a file under the sandbox home, creating parent dirs.
    pub fn seed_file(&self, rel: &str, contents: &[u8]) -> std::path::PathBuf {
        let p = self.dir.path().join(rel);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        std::fs::write(&p, contents).expect("write");
        p
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // SAFETY: callers hold the process-local ENV_LOCK for our whole lifetime.
        unsafe {
            std::env::remove_var("SWEEP_HOME");
            std::env::remove_var("SWEEP_TEST_GUARD");
        }
    }
}

/// Create a sandboxed home and export the guard env vars for this test.
pub fn sandbox() -> Sandbox {
    let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().expect("tempdir");
    // SAFETY: we hold ENV_LOCK; this file contains no other env mutation.
    unsafe {
        std::env::set_var("SWEEP_HOME", dir.path());
        std::env::set_var("SWEEP_TEST_GUARD", "1");
    }
    Sandbox { dir, _guard: guard }
}
