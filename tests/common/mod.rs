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

// ---------------------------------------------------------------------------
// M1 scan-test helpers (plan §4). These take an explicit home path and mutate
// NO process env — library-level tests use Ctx::for_test(home) directly.
// ---------------------------------------------------------------------------

/// Seed a file of `len` zero bytes at `home/rel`, creating parents.
pub fn seed(home: &Path, rel: &str, len: u64, mtime: filetime::FileTime) {
    use std::os::unix::fs::DirBuilderExt as _;
    let p = home.join(rel);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(p.parent().expect("parent"))
        .expect("mkdir");
    std::fs::write(&p, vec![0u8; len as usize]).expect("write");
    filetime::set_file_mtime(&p, mtime).expect("set mtime");
}

/// The canonical M1 scan fixture (plan §4 table). `home/.fake-tmp` and
/// `home/.fake-var-tmp` match `Ctx::for_test` tmp_roots.
///
/// Expected golden values (see plan §4): tmp 3282/2/2 (+1 protected,
/// +1 kept-newer), thumbnails 5120/2/1, user-cache 15144/4/3 (hot=1) →
/// net 10024/2/2, trash 2176/2/2, totals 20602.
pub fn scan_fixture(home: &Path) {
    use std::time::Duration;
    let now = SystemTime::now();
    let old = filetime::FileTime::from_system_time(now - Duration::from_secs(10 * 86_400));
    let hour = filetime::FileTime::from_system_time(now - Duration::from_secs(3600));
    let fresh = filetime::FileTime::from_system_time(now);

    // ~/.cache
    seed(home, ".cache/app-cache/big.bin", 10_000, old);
    seed(home, ".cache/app-cache/small.txt", 24, old);
    seed(home, ".cache/thumbnails/f1.png", 4_096, old);
    seed(home, ".cache/thumbnails/f2.png", 1_024, old);
    seed(home, ".cache/fresh-entry/hot.txt", 500, fresh);
    // ~/.local/share/Trash
    seed(home, ".local/share/Trash/files/trashed.dat", 2_048, old);
    seed(home, ".local/share/Trash/info/trashed.trashinfo", 128, old);
    // tmp roots (for_test)
    seed(home, ".fake-tmp/old.bin", 1_234, old);
    seed(home, ".fake-tmp/newer.bin", 600, hour);
    seed(home, ".fake-tmp/systemd-private-x/run/file", 9_999, old);
    seed(home, ".fake-var-tmp/var-old.tmp", 2_048, old);

    // Depth-1 .cache dir mtimes drive the hot skip (300 s window): the two
    // old cache dirs must NOT look hot; fresh-entry must.
    filetime::set_file_mtime(&home.join(".cache/app-cache"), old).expect("mtime");
    filetime::set_file_mtime(&home.join(".cache/thumbnails"), old).expect("mtime");
    filetime::set_file_mtime(&home.join(".cache/fresh-entry"), fresh).expect("mtime");
}

/// Recursive manifest of (kind, len, mtime_ns) keyed by absolute path.
/// kind: 0=file, 1=dir, 2=symlink, 3=other. No atime — reads bump it.
pub fn manifest(path: &Path) -> std::collections::BTreeMap<PathBuf, (u8, u64, u64)> {
    let mut map = std::collections::BTreeMap::new();
    collect(path, &mut map);
    map
}

fn collect(dir: &Path, map: &mut std::collections::BTreeMap<PathBuf, (u8, u64, u64)>) {
    use std::os::unix::fs::MetadataExt as _;
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let p = entry.path();
        let Ok(md) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        let ft = md.file_type();
        let kind = if ft.is_symlink() {
            2
        } else if ft.is_dir() {
            1
        } else if ft.is_file() {
            0
        } else {
            3
        };
        let mtime_ns = ((md.mtime() as i128) * 1_000_000_000 + md.mtime_nsec() as i128) as u64;
        map.insert(p.clone(), (kind, md.len(), mtime_ns));
        if ft.is_dir() {
            collect(&p, map);
        }
    }
}

pub use std::path::PathBuf;
pub use std::time::SystemTime;
