//! Deletion primitives with safety checks.

use crate::ctx::Ctx;
use crate::exec::{ExecError, ItemReport, PlanItem};
use crate::scan::walker::EntryKind;
use std::fs;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::{Component, Path};

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

pub fn remove_items(ctx: &Ctx, item: &PlanItem, trash: bool) -> Result<ItemReport, ExecError> {
    if trash {
        return Err(ExecError::Other(
            "--trash is not implemented in this build".to_string(),
        ));
    }
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
        if !path.is_absolute() || has_parent_component(path) {
            errors.push(format!("refusing non-normalized path: {}", path.display()));
            continue;
        }
        if !check_prefix_allowed(ctx, path) {
            errors.push(format!("outside allowed prefix: {}", path.display()));
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
                if kind == EntryKind::Symlink {
                    if let Err(e) = fs::remove_file(path) {
                        errors.push(format!("{}: {}", path.display(), e));
                    } else {
                        removed += 1;
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
