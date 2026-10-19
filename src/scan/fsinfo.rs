//! Read-only system probes: statvfs, root fstype, hostname.
//! All failures are `io::Result` — callers degrade, never panic.

use std::path::Path;

/// Total/used bytes of the filesystem containing `path` (df-compatible:
/// used = `(f_blocks - f_bavail) * f_frsize`).
pub fn statvfs_totals(path: &Path) -> std::io::Result<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt as _;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    // SAFETY: `c_path` is a valid NUL-terminated string that outlives the
    // call; `statvfs` only writes through the out-pointer `st`, which is a
    // zeroed stack struct we own. No aliasing, no panics across the FFI
    // boundary.
    let (rc, st) = unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        let rc = libc::statvfs(c_path.as_ptr(), &mut st);
        (rc, st)
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let total = st.f_blocks * st.f_frsize;
    let avail = st.f_bavail * st.f_frsize;
    Ok((total, total.saturating_sub(avail)))
}

/// Filesystem type of `/` — parses `/proc/self/mountinfo` (field before `-`
/// for mountpoint `/`); falls back to `/proc/mounts`, then `"unknown"`.
pub fn root_fstype() -> std::io::Result<String> {
    if let Ok(s) = fstype_from("/proc/self/mountinfo", true) {
        return Ok(s);
    }
    if let Ok(s) = fstype_from("/proc/mounts", false) {
        return Ok(s);
    }
    Err(std::io::Error::other(
        "cannot determine root filesystem type",
    ))
}

/// mountinfo: `… - <fstype> <source> …` (separator `-`); mounts: `<device>
/// <mountpoint> <fstype> …`. Both: the line whose mountpoint field is `/`.
fn fstype_from(path: &str, mountinfo: bool) -> std::io::Result<String> {
    let text = std::fs::read_to_string(path)?;
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let fstype_idx = if mountinfo {
            // mountinfo: mountpoint = field 4; fstype = field after "-"
            let Some(sep) = fields.iter().position(|f| *f == "-") else {
                continue;
            };
            if fields.len() <= sep + 1 || fields.get(4) != Some(&"/") {
                continue;
            }
            sep + 1
        } else {
            // mounts: <device> <mountpoint "/" > <fstype>
            if fields.get(1) != Some(&"/") || fields.len() < 3 {
                continue;
            }
            2
        };
        if let Some(fstype) = fields.get(fstype_idx) {
            return Ok((*fstype).to_string());
        }
    }
    Err(std::io::Error::other(format!("{path}: no / entry")))
}

/// Kernel hostname (no unsafe, no dep).
pub fn hostname() -> std::io::Result<String> {
    Ok(std::fs::read_to_string("/proc/sys/kernel/hostname")?
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // These probe the live host (read-only): only assert shape, never exact
    // values.
    #[test]
    fn statvfs_on_root_returns_plausible_totals() {
        let (total, used) = statvfs_totals(Path::new("/")).expect("statvfs /");
        assert!(total > 0);
        assert!(used <= total);
    }

    #[test]
    fn root_fstype_is_nonempty() {
        let s = root_fstype().expect("fstype");
        assert!(!s.is_empty());
    }

    #[test]
    fn hostname_is_nonempty() {
        let s = hostname().expect("hostname");
        assert!(!s.is_empty());
    }
}
