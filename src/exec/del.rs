//! Deletion primitives with safety checks.

use crate::ctx::Ctx;
use crate::exec::{ExecError, ItemReport, PlanItem};
use crate::scan::walker::EntryKind;
use std::fs;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::{Component, Path};
use std::time::{Duration, SystemTime};

fn entry_kind(file_type: &std::fs::FileType) -> EntryKind {
    if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Dir
    } else if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_socket() {
        EntryKind::Socket
    } else if file_type.is_fifo() {
        EntryKind::Fifo
    } else if file_type.is_char_device() {
        EntryKind::CharDevice
    } else {
        EntryKind::Other
    }
}

fn is_protected_path(p: &Path, kind: EntryKind) -> bool {
    p.components()
        .any(|component| crate::rules::tmp::is_protected(component.as_os_str(), kind))
}

fn is_never_touch_home_path(ctx: &Ctx, p: &Path) -> bool {
    p == ctx.home
        || [
            "Documents",
            "Pictures",
            "Videos",
            "Music",
            "Projects",
            "Desktop",
            "Downloads",
        ]
        .iter()
        .map(|name| ctx.home.join(name))
        .any(|root| p.starts_with(root))
}

fn has_parent_component(p: &Path) -> bool {
    p.components()
        .any(|component| component == Component::ParentDir)
}

fn has_symlink_ancestor(p: &Path) -> bool {
    let components: Vec<Component<'_>> = p.components().collect();
    let mut current = std::path::PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        if index + 1 == components.len() {
            break;
        }
        if fs::symlink_metadata(&current).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return true;
        }
    }
    false
}

fn check_prefix_allowed(ctx: &Ctx, p: &Path) -> bool {
    let home = ctx.home.as_path();
    let tmp = Path::new("/tmp");
    let var_tmp = Path::new("/var/tmp");
    if is_never_touch_home_path(ctx, p) {
        return false;
    }
    if p.starts_with(home) || p.starts_with(tmp) || p.starts_with(var_tmp) {
        return true;
    }
    p.starts_with(Path::new("/var/cache")) || p.starts_with(Path::new("/var/log"))
}

pub fn remove_items(
    ctx: &Ctx,
    item: &PlanItem,
    trash: bool,
    keep_days: Option<u32>,
) -> Result<ItemReport, ExecError> {
    let mut freed: u64 = 0;
    let mut removed: u64 = 0;
    let mut staged: u64 = 0;
    let mut skipped: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let process_guard = match item.id {
        "browser-cache" => Some(crate::system::proc::BROWSERS),
        "editor-cache" => Some(crate::system::proc::EDITORS),
        "app-caches" => Some(crate::system::proc::APP_CACHES),
        _ => None,
    };
    if let Some(names) = process_guard {
        match ctx
            .proc_root
            .as_deref()
            .ok_or_else(|| std::io::Error::other("process state unavailable"))
            .and_then(crate::system::proc::processes)
        {
            Ok(processes) => {
                if let Some(name) = crate::system::proc::matching_process(&processes, names)
                    && !item.guard_overridden
                {
                    return Ok(ItemReport {
                        id: item.id,
                        freed: 0,
                        removed: 0,
                        staged: 0,
                        skipped: vec![format!("{name} running (guard re-check)")],
                        errors,
                    });
                }
            }
            Err(error) => {
                return Ok(ItemReport {
                    id: item.id,
                    freed: 0,
                    removed: 0,
                    staged: 0,
                    skipped: vec![format!("process state unavailable: {error}")],
                    errors,
                });
            }
        }
    }
    let open_files = if item.id == "tmp" {
        ctx.proc_root
            .as_deref()
            .map(crate::system::proc::open_files)
            .transpose()
            .map_err(ExecError::Io)?
    } else {
        None
    };

    let now = SystemTime::now();
    for path in &item.paths {
        if path.as_os_str().is_empty() {
            continue;
        }
        if path == Path::new("/") {
            errors.push("refusing to delete /".to_string());
            continue;
        }
        if !path.is_absolute() || has_parent_component(path) {
            errors.push(format!("refusing non-normalized path: {}", path.display()));
            continue;
        }
        if !check_prefix_allowed(ctx, path) {
            errors.push(format!("outside allowed prefix: {}", path.display()));
            continue;
        }
        if open_files
            .as_ref()
            .is_some_and(|files| files.contains(path))
        {
            skipped.push(format!("open file: {}", path.display()));
            continue;
        }
        if has_symlink_ancestor(path) {
            skipped.push(format!("symlink ancestor: {}", path.display()));
            continue;
        }
        match fs::symlink_metadata(path) {
            Ok(meta) => {
                let kind = entry_kind(&meta.file_type());
                if is_protected_path(path, kind) {
                    skipped.push(format!("protected: {}", path.display()));
                    continue;
                }
                if matches!(
                    kind,
                    EntryKind::Socket | EntryKind::Fifo | EntryKind::CharDevice
                ) {
                    skipped.push(format!("special: {}", path.display()));
                    continue;
                }
                if let Some(reason) = age_skip_reason(item.id, keep_days, meta.modified(), now) {
                    skipped.push(format!("{reason}: {}", path.display()));
                    continue;
                }
                if kind == EntryKind::Symlink {
                    let result = if trash {
                        super::trash::to_trash(&ctx.home, path, &ctx.run_id, removed as usize)
                    } else {
                        fs::remove_file(path)
                    };
                    if let Err(e) = result {
                        errors.push(format!("{}: {}", path.display(), e));
                    } else {
                        removed += 1;
                        staged += u64::from(trash);
                    }

                    continue;
                }
                if kind == EntryKind::Dir {
                    errors.push(format!(
                        "refusing to recursively delete directory: {}",
                        path.display()
                    ));
                    continue;
                } else if kind == EntryKind::File {
                    let sz = meta.size();
                    let result = if trash {
                        super::trash::to_trash(&ctx.home, path, &ctx.run_id, removed as usize)
                    } else {
                        fs::remove_file(path)
                    };
                    if let Err(e) = result {
                        errors.push(format!("{}: {}", path.display(), e));
                    } else {
                        removed += 1;
                        if trash {
                            staged += 1;
                        } else {
                            freed += sz;
                        }
                    }
                    continue;
                } else {
                    skipped.push(format!("special: {}", path.display()));
                    continue;
                }
            }
            Err(e) => {
                errors.push(format!("{}: {}", path.display(), e));
                continue;
            }
        }
    }

    Ok(ItemReport {
        id: item.id,
        freed,
        removed,
        staged,
        skipped,
        errors,
    })
}

fn age_skip_reason(
    id: &str,
    keep_days: Option<u32>,
    modified: std::io::Result<SystemTime>,
    now: SystemTime,
) -> Option<&'static str> {
    let (age, reason) = match id {
        "tmp" => {
            let days = keep_days.unwrap_or(1);
            if days == 0 {
                return None;
            }
            (
                Duration::from_secs(u64::from(days) * 86_400),
                "newer than keep-days",
            )
        }
        "user-cache" | "browser-cache" | "editor-cache" | "tool-caches" | "app-caches" => {
            (Duration::from_secs(300), "hot write")
        }
        "log-rotate" | "xdg-state-coredumps" => {
            (Duration::from_secs(86_400), "younger than one day")
        }
        _ => return None,
    };
    modified
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_none_or(|elapsed| elapsed < age)
        .then_some(reason)
}
