use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

pub const BROWSERS: &[&str] = &["firefox", "chromium", "chrome", "brave", "edge", "msedge"];
pub const EDITORS: &[&str] = &[
    "code",
    "cursor",
    "codium",
    "slack",
    "discord",
    "element",
    "obsidian",
    "signal",
    "telegram-desktop",
];
pub const APP_CACHES: &[&str] = &["obsidian", "signal", "telegram-desktop"];
pub const STEAM: &[&str] = &["steam", "steamwebhelper"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub comm: String,
}

pub fn processes(proc_root: &Path) -> io::Result<Vec<ProcessInfo>> {
    let mut result = Vec::new();
    for entry in std::fs::read_dir(proc_root)? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        else {
            continue;
        };
        let comm = match std::fs::read_to_string(entry.path().join("comm")) {
            Ok(comm) => comm.trim().to_ascii_lowercase(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return Err(error),
            Err(error) => return Err(error),
        };
        if !comm.is_empty() {
            result.push(ProcessInfo { pid, comm });
        }
    }
    Ok(result)
}

pub fn matching_process(processes: &[ProcessInfo], names: &[&str]) -> Option<String> {
    processes
        .iter()
        .find(|process| names.contains(&process.comm.as_str()))
        .map(|process| process.comm.clone())
}

pub fn matching_process_under(
    proc_root: &Path,
    names: &[&str],
    roots: &[PathBuf],
) -> io::Result<Option<String>> {
    for process in processes(proc_root)? {
        if !names.contains(&process.comm.as_str()) {
            continue;
        }
        match std::fs::read_link(proc_root.join(process.pid.to_string()).join("cwd")) {
            Ok(cwd) if roots.iter().any(|root| cwd.starts_with(root)) => {
                return Ok(Some(process.comm));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

pub fn open_files(proc_root: &Path) -> io::Result<BTreeSet<PathBuf>> {
    let mut result = BTreeSet::new();
    for process in std::fs::read_dir(proc_root)? {
        let process = process?;
        if process
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
            .is_none()
        {
            continue;
        }
        let fds = process.path().join("fd");
        let entries = match std::fs::read_dir(fds) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                continue;
            }
            Err(error) => return Err(error),
        };
        for fd in entries {
            let fd = match fd {
                Ok(fd) => fd,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            match std::fs::read_link(fd.path()) {
                Ok(target) => {
                    let target = target.to_string_lossy().into_owned();
                    let target = target.strip_suffix(" (deleted)").unwrap_or(&target);
                    if target.starts_with('/') {
                        result.insert(PathBuf::from(target));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(result)
}

pub fn is_open_file(path: &Path) -> bool {
    open_files(Path::new("/proc")).map_or(true, |files| files.contains(path))
}

pub fn default_proc_root() -> &'static Path {
    Path::new("/proc")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_and_open_file_scans_use_an_injected_proc_root() {
        let proc = tempfile::tempdir().expect("proc root");
        let process = proc.path().join("42");
        let fds = process.join("fd");
        std::fs::create_dir_all(&fds).expect("fd directory");
        std::fs::write(process.join("comm"), "Firefox\n").expect("comm");
        let target = tempfile::NamedTempFile::new().expect("target");
        std::os::unix::fs::symlink(target.path(), fds.join("3")).expect("fd link");

        let processes = processes(proc.path()).expect("processes");
        assert_eq!(
            matching_process(&processes, BROWSERS).as_deref(),
            Some("firefox")
        );
        assert!(
            open_files(proc.path())
                .expect("open files")
                .contains(target.path())
        );
    }

    #[test]
    fn process_match_is_exact_not_a_substring() {
        let processes = vec![ProcessInfo {
            pid: 3,
            comm: "chromium-helper".into(),
        }];
        assert_eq!(matching_process(&processes, BROWSERS), None);
        assert_eq!(
            matching_process(
                &[ProcessInfo {
                    pid: 4,
                    comm: "msedge".into(),
                }],
                BROWSERS
            )
            .as_deref(),
            Some("msedge")
        );
    }
}
