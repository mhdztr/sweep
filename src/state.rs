//! State paths + UTC run id + single-instance lock.
//!
//! `scan` NEVER touches any of this (SAFETY §10): the lock and the state dir
//! exist for mutating commands only (M2+). M1 implements them so the rest of
//! the codebase has one source of truth for run ids and state paths.

use std::os::fd::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};

/// UTC compact run id: `YYYYMMDDTHHMMSS`. Pure date math (no chrono).
pub fn run_id_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    run_id_from_unix(secs)
}

/// Pure seam over unix seconds — testable without a clock.
pub fn run_id_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let h = rem / 3600;
    let mi = (rem % 3600) / 60;
    let s = rem % 60;
    format!("{y:04}{m:02}{d:02}T{h:02}{mi:02}{s:02}")
}

/// Howard Hinnant's `civil_from_days` (integer inverse of days_from_civil).
/// `z` = days since 1970-01-01; returns (year, month, day).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11] (0 = March)
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (y + i64::from(u32::from(m <= 2)), m, d)
}

/// `SWEEP_STATE_DIR ?? {home}/.local/state/sweep` (read-only resolution;
/// creates nothing).
pub fn state_dir(home: &Path) -> PathBuf {
    match std::env::var_os("SWEEP_STATE_DIR") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home.join(".local/state/sweep"),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Exact CLI-UX §5 copy; `since` = HH:MM UTC (recorded timezone is UTC,
    /// see plan §7 risk 6).
    #[error("another sweep is running (pid {pid}, since {since})")]
    Held { pid: u32, since: String },
    #[error("cannot lock {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// RAII flock guard. Drop releases the lock and closes the fd.
pub struct LockGuard {
    fd: RawFd,
    // Owns the File so the fd stays valid for the guard's lifetime.
    _file: std::fs::File,
}

impl std::fmt::Debug for LockGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockGuard").field("fd", &self.fd).finish()
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // SAFETY: `fd` is a valid open file descriptor owned by `_file`;
        // flock(LOCK_UN) on it is safe (no pointer arguments, no aliasing
        // concerns beyond the kernel lock itself). The file is closed when
        // `_file` drops after this.
        unsafe {
            libc::flock(self.fd, libc::LOCK_UN);
        }
    }
}

/// MUTATING-ONLY. Creates the state dir (0700) + lock file, takes
/// `flock(LOCK_EX|LOCK_NB)`, then writes `{pid}\n{HH:MM}\n` so a second
/// instance can render the contention message. `scan` must never call this.
pub fn acquire_lock(state_dir: &Path) -> Result<LockGuard, LockError> {
    use std::os::unix::fs::DirBuilderExt as _;

    let path = state_dir.join("sweep.lock");
    // SAFETY (creation): std fs APIs, no unsafe involved — create_dir_all below.
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(state_dir)
        .map_err(|source| LockError::Io {
            path: state_dir.to_path_buf(),
            source,
        })?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| LockError::Io {
            path: path.clone(),
            source,
        })?;
    let fd = file.as_raw_fd();

    // SAFETY: `fd` is a valid open descriptor owned by `file` (which outlives
    // the guard). flock with LOCK_EX|LOCK_NB takes an advisory lock without
    // dereferencing any pointer; a nonzero return means the lock is held by
    // another process (or an OS error) — we never dereference memory here.
    let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::WouldBlock {
            // Another instance holds it: read its recorded pid/HH:MM so the
            // message names the actual holder.
            let (pid, since) = read_holder(&path);
            return Err(LockError::Held { pid, since });
        }
        return Err(LockError::Io {
            path: path.clone(),
            source: err,
        });
    }

    // Record holder info (best effort — a failure here keeps the lock).
    let pid = std::process::id();
    let since = utc_hhmm_now();
    let _ = std::fs::write(&path, format!("{pid}\n{since}\n"));

    Ok(LockGuard { fd, _file: file })
}

fn read_holder(path: &Path) -> (u32, String) {
    let fallback = (0, utc_hhmm_now());
    let Ok(text) = std::fs::read_to_string(path) else {
        return fallback;
    };
    let mut lines = text.lines();
    let pid = lines.next().and_then(|l| l.parse().ok()).unwrap_or(0);
    let since = lines.next().unwrap_or("unknown").to_string();
    (pid, since)
}

fn utc_hhmm_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let rem = secs % 86_400;
    format!("{:02}:{:02}", rem / 3600, (rem % 3600) / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_format_is_compact_utc() {
        assert_eq!(run_id_from_unix(0), "19700101T000000");
        // Known epochs (civil_from_days spot checks):
        assert_eq!(run_id_from_unix(86_400), "19700102T000000");
        assert_eq!(run_id_from_unix(1_704_067_200), "20240101T000000");
        // Leap-year civil-from-days: 2024-02-29 = 19782 days after epoch.
        assert_eq!(run_id_from_unix(19_782 * 86_400), "20240229T000000");
        // End of leap year boundary.
        assert_eq!(
            run_id_from_unix(19_783 * 86_400 + 3_600 + 60 + 5),
            "20240301T010105"
        );
        assert!(regex_like(&run_id_now()));
    }

    fn regex_like(s: &str) -> bool {
        s.len() == 15
            && s.as_bytes()[8] == b'T'
            && s.bytes().enumerate().all(|(i, b)| {
                if i == 8 {
                    b == b'T'
                } else {
                    b.is_ascii_digit()
                }
            })
    }

    #[test]
    fn second_exclusive_lock_on_same_path_fails_with_pid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let g1 = acquire_lock(dir.path()).expect("first lock");
        match acquire_lock(dir.path()) {
            Err(LockError::Held { pid, since }) => {
                assert_eq!(pid, std::process::id());
                assert!(regex_hhmm(&since), "since should be HH:MM, got {since:?}");
            }
            other => panic!("expected Held, got {other:?}"),
        }
        drop(g1);
    }

    #[test]
    fn lock_guard_releases_on_drop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let g1 = acquire_lock(dir.path()).expect("first lock");
        drop(g1);
        let g2 = acquire_lock(dir.path()).expect("re-acquire after drop");
        drop(g2);
    }

    fn regex_hhmm(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() == 5
            && b[0].is_ascii_digit()
            && b[1].is_ascii_digit()
            && b[2] == b':'
            && b[3].is_ascii_digit()
            && b[4].is_ascii_digit()
    }
}
