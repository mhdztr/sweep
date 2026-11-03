//! State paths + UTC run id + single-instance lock.

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

/// Pure seam over unix seconds, testable without a clock.
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
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (y + i64::from(u32::from(m <= 2)), m, d)
}

/// `SWEEP_STATE_DIR ?? {home}/.local/state/sweep`
pub fn state_dir(home: &Path) -> PathBuf {
    match std::env::var_os("SWEEP_STATE_DIR") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home.join(".local/state/sweep"),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("another sweep is running (pid {pid}, since {since})")]
    Held { pid: u32, since: String },
    #[error("cannot lock {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub struct LockGuard {
    fd: RawFd,
    _file: std::fs::File,
}

impl std::fmt::Debug for LockGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockGuard").field("fd", &self.fd).finish()
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.fd, libc::LOCK_UN);
        }
    }
}

pub fn acquire_lock(state_dir: &Path) -> Result<LockGuard, LockError> {
    use std::os::unix::fs::DirBuilderExt as _;

    let path = state_dir.join("sweep.lock");
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

    let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::WouldBlock {
            let (pid, since) = read_holder(&path);
            return Err(LockError::Held { pid, since });
        }
        return Err(LockError::Io {
            path: path.clone(),
            source: err,
        });
    }

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

// History and logging (M2)

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    pub id: String,
    pub freed: u64,
    pub removed: u64,
    #[serde(default)]
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub v: u32,
    pub ts: String,
    pub preset: String,
    pub mode: String,
    pub items: Vec<HistoryItem>,
    pub freed_total: u64,
    pub duration_ms: u64,
    pub dry_run: bool,
    pub trash: bool,
}

fn logs_dir_path(state_dir: &Path) -> PathBuf {
    state_dir.join("logs")
}

fn history_file_path(state_dir: &Path) -> PathBuf {
    state_dir.join("history.jsonl")
}

/// Write an action/audit log for a run.
pub fn write_action_log(state_dir: &Path, run_id: &str, content: &str) -> std::io::Result<PathBuf> {
    let dir = logs_dir_path(state_dir);
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }
    let path = dir.join(format!("{}.log", run_id));
    fs::write(&path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(path)
}

/// Append a history record to history.jsonl.
pub fn append_history(state_dir: &Path, record: &HistoryRecord) -> std::io::Result<()> {
    fs::create_dir_all(state_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(state_dir, fs::Permissions::from_mode(0o700));
    }
    let path = history_file_path(state_dir);
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    let line = serde_json::to_string(record).map_err(std::io::Error::other)?;
    writeln!(f, "{}", line)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Prune history.jsonl to keep only the last `keep` records.
pub fn prune_history(state_dir: &Path, keep: u32) -> std::io::Result<()> {
    if keep == 0 {
        return Ok(());
    }
    let path = history_file_path(state_dir);
    if !path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&path)?;
    let mut lines: Vec<String> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();
    if lines.len() > keep as usize {
        let start = lines.len() - keep as usize;
        lines.drain(0..start);
        let new_content = lines.join("\n") + "\n";
        fs::write(&path, new_content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
    }
    Ok(())
}

/// Prune old log files, keeping only the last `keep` most recent.
pub fn prune_logs(state_dir: &Path, keep: usize) -> std::io::Result<()> {
    let dir = logs_dir_path(state_dir);
    if !dir.exists() {
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("log"))
        .collect();
    if entries.len() <= keep {
        return Ok(());
    }
    entries.sort_by_key(|p| {
        fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    let to_remove = entries.len() - keep;
    for p in entries.iter().take(to_remove) {
        let _ = fs::remove_file(p);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_format_is_compact_utc() {
        assert_eq!(run_id_from_unix(0), "19700101T000000");
        assert_eq!(run_id_from_unix(86_400), "19700102T000000");
        assert_eq!(run_id_from_unix(1_704_067_200), "20240101T000000");
        assert_eq!(run_id_from_unix(19_782 * 86_400), "20240229T000000");
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
