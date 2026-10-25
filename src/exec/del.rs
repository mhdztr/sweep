//! Deletion primitives with safety checks.

use crate::ctx::Ctx;
use crate::exec::{ExecError, ItemReport, PlanItem};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

fn is_protected_path(p: &Path) -> bool {
    let s = p.to_string_lossy();
    if s.contains("/systemd-private-") {
        return true;
    }
    if s.contains("/X11-unix") || s.contains("/.X11-unix") {
        return true;
    }
    if s.contains("/.XIM-unix") || s.contains("/.ICE-unix") {
        return true;
    }
    if s.ends_with(".X0-lock") {
        return true;
    }
    if s.contains("/X") && s.ends_with("-lock") {
        return true;
    }
    false
}

fn check_prefix_allowed(ctx: &Ctx, p: &Path) -> bool {
    let home = ctx.home.as_path();
    let tmp = Path::new("/tmp");
    let var_tmp = Path::new("/var/tmp");
    if p.starts_with(home) || p.starts_with(tmp) || p.starts_with(var_tmp) {
        return true;
    }
    p.starts_with(Path::new("/var/cache"))
        || p.starts_with(Path::new("/var/log"))
        || p.starts_with(Path::new("/var/tmp"))
}

pub fn remove_items(ctx: &Ctx, item: &PlanItem, _trash: bool) -> Result<ItemReport, ExecError> {
    let mut freed: u64 = 0;
    let mut removed: u64 = 0;
    let mut skipped: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for path in &item.paths {
        if path.as_os_str().is_empty() {
            continue;
        }
        if path == Path::new("/") {
            errors.push("refusing to delete /".to_string());
            continue;
        }
        if !check_prefix_allowed(ctx, path) {
            errors.push(format!("outside allowed prefix: {}", path.display()));
            continue;
        }
        if is_protected_path(path) {
            skipped.push(format!("protected: {}", path.display()));
            continue;
        }
        match fs::symlink_metadata(path) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    if let Err(e) = fs::remove_file(path) {
                        errors.push(format!("{}: {}", path.display(), e));
                    } else {
                        removed += 1;
                    }
                    continue;
                }
                if meta.is_dir() {
                    if let Err(e) = remove_dir_bottom_up(path) {
                        errors.push(format!("{}: {}", path.display(), e));
                    } else {
                        freed += meta.size();
                        removed += 1;
                    }
                    continue;
                } else if meta.is_file() {
                    let sz = meta.size();
                    if let Err(e) = fs::remove_file(path) {
                        errors.push(format!("{}: {}", path.display(), e));
                    } else {
                        freed += sz;
                        removed += 1;
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
        skipped,
        errors,
    })
}

fn remove_dir_bottom_up(dir: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            fs::remove_file(&path)?;
            continue;
        }
        if meta.is_dir() {
            remove_dir_bottom_up(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    fs::remove_dir(dir)
}
